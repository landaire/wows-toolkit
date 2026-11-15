use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::JsonObject;
use rmcp::model::Tool;
use rootcause::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::sync::OnceCell;
use wows_query::GameData;
use wows_query::error::LoadError;
use wows_query::tools::ToolCallError;

use crate::confine::PathRoots;
use crate::tool_set::ToolFuture;
use crate::tool_set::ToolSet;
use crate::tool_set::tool_failure;

const INSTRUCTIONS: &str = "World of Warships game data for one game build. Ship arguments take the English ship name as players write it. Answer from tool output; numbers come from GameParams.";

const REPLAY_SUMMARY: &str = "replay_summary";

/// Arguments for `replay_summary`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaySummaryArgs {
    /// Path of a .wowsreplay file on the server's file system.
    pub path: String,
}

/// Which files `replay_summary` may open.
#[derive(Debug, Clone)]
pub enum ReplayFiles {
    /// Any path the server process can read, apart from network shares and devices.
    Any,
    /// Only files inside these directories.
    Within(PathRoots),
}

/// Where the query tools get their game build.
pub enum GameDataSource {
    Loaded(Arc<GameData>),
    /// A wows-replay-data dump root or build directory, loaded on the first
    /// query so serving does not wait for it.
    Dump {
        path: PathBuf,
        cache_dir: Option<PathBuf>,
    },
}

/// Loads a dump root (holding `builds.toml`, newest build served) or one
/// build directory (holding `metadata.toml`). Blocks for several seconds.
pub fn load_dump(path: &Path, cache_dir: Option<&Path>) -> Result<GameData, Report<LoadError>> {
    if path.join("metadata.toml").is_file() {
        GameData::load_dump(path, cache_dir)
    } else {
        GameData::load_latest(path, cache_dir)
    }
}

/// Every `wows_query::tools` tool over one game build, plus
/// `replay_summary`, which reads a replay from the server's file system.
pub struct GameDataTools {
    build: Build,
    replay_files: ReplayFiles,
}

enum Build {
    Loaded(Arc<GameData>),
    Dump { path: PathBuf, cache_dir: Option<PathBuf>, game: OnceCell<Arc<GameData>> },
}

impl GameDataTools {
    pub fn new(source: GameDataSource, replay_files: ReplayFiles) -> Self {
        let build = match source {
            GameDataSource::Loaded(game) => Build::Loaded(game),
            GameDataSource::Dump { path, cache_dir } => Build::Dump { path, cache_dir, game: OnceCell::new() },
        };

        Self { build, replay_files }
    }

    /// The build, loading it on first use. A failed load is retried by the
    /// next call.
    async fn game(&self) -> Result<Arc<GameData>, String> {
        let (path, cache_dir, game) = match &self.build {
            Build::Loaded(game) => return Ok(Arc::clone(game)),
            Build::Dump { path, cache_dir, game } => (path, cache_dir, game),
        };

        let game = game
            .get_or_try_init(|| async {
                let (path, cache_dir) = (path.clone(), cache_dir.clone());
                tokio::task::spawn_blocking(move || load_dump(&path, cache_dir.as_deref()))
                    .await
                    .map_err(|error| format!("game data load task failed: {error}"))?
                    .map(Arc::new)
                    .map_err(|report| format!("cannot load game data: {}", report.current_context()))
            })
            .await?;

        Ok(Arc::clone(game))
    }

    async fn replay_summary(&self, arguments: Value) -> Result<String, String> {
        // Replays are tens of MB; the bound keeps a device or FIFO path from
        // exhausting memory.
        const REPLAY_LIMIT: u64 = 256 * 1024 * 1024;

        let args: ReplaySummaryArgs = serde_json::from_value(arguments)
            .map_err(|error| format!("invalid arguments for {REPLAY_SUMMARY}: {error}"))?;
        let unreadable = || format!("cannot read replay {}", args.path);
        let path = match &self.replay_files {
            ReplayFiles::Any => {
                if crate::confine::is_network_or_device(args.path.as_ref()) {
                    return Err(format!("{} is a network or device path", args.path));
                }
                tokio::fs::canonicalize(&args.path).await.map_err(|_| unreadable())?
            }
            ReplayFiles::Within(roots) => {
                roots.resolve(args.path.as_ref()).map_err(|error| format!("cannot read replay: {error}"))?
            }
        };
        if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("wowsreplay")) {
            return Err(format!("{} is not a .wowsreplay file", args.path));
        }
        // Opening a FIFO would wait for a writer, so the type is checked before the open, and again
        // on the opened handle.
        let regular = |metadata: std::fs::Metadata| metadata.is_file();
        if !tokio::fs::metadata(&path).await.is_ok_and(regular) {
            return Err(unreadable());
        }
        let file = tokio::fs::File::open(&path).await.map_err(|_| unreadable())?;
        if !file.metadata().await.is_ok_and(regular) {
            return Err(unreadable());
        }
        let mut bytes = Vec::new();
        file.take(REPLAY_LIMIT + 1).read_to_end(&mut bytes).await.map_err(|_| unreadable())?;
        if bytes.len() as u64 > REPLAY_LIMIT {
            return Err(format!("{} is larger than any replay", args.path));
        }

        let game = self.game().await?;
        let summary = tokio::task::spawn_blocking(move || game.replay_summary(&bytes))
            .await
            .map_err(|error| format!("query task failed: {error}"))?
            .map_err(|error| error.to_string())?;

        serde_json::to_string(&summary).map_err(|error| format!("failed to encode the answer: {error}"))
    }

    /// Runs a registry tool off the async workers. The outer error is a
    /// protocol error; the inner one is shown to the caller.
    async fn registry_tool(&self, name: String, arguments: Value) -> Result<Result<String, String>, ErrorData> {
        let game = match self.game().await {
            Ok(game) => game,
            Err(message) => return Ok(Err(message)),
        };
        let answer = tokio::task::spawn_blocking(move || game.call_tool(&name, arguments))
            .await
            .map_err(|error| ErrorData::internal_error(format!("query task failed: {error}"), None))?;

        match answer {
            Ok(value) => Ok(Ok(value.to_string())),
            Err(error @ ToolCallError::UnknownTool { .. }) => Err(ErrorData::invalid_params(error.to_string(), None)),
            Err(error) => Ok(Err(error.to_string())),
        }
    }
}

impl ToolSet for GameDataTools {
    fn tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = wows_query::tools::specs()
            .into_iter()
            .map(|spec| Tool::new(spec.name, spec.description, spec.input_schema))
            .collect();
        tools.push(Tool::new(
            REPLAY_SUMMARY,
            "Summarize a .wowsreplay file on the server: recording player, map, mode, client version, date, duration and every player's ship and team.",
            wows_query::tools::input_schema::<ReplaySummaryArgs>(),
        ));

        tools
    }

    fn call<'a>(&'a self, name: &'a str, arguments: JsonObject) -> ToolFuture<'a> {
        Box::pin(async move {
            let arguments = Value::Object(arguments);
            let answer = match name {
                REPLAY_SUMMARY => self.replay_summary(arguments).await,
                name => self.registry_tool(name.to_owned(), arguments).await?,
            };

            Ok(match answer {
                Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
                Err(text) => tool_failure(text),
            })
        })
    }

    fn instructions(&self) -> Option<String> {
        Some(INSTRUCTIONS.to_owned())
    }
}
