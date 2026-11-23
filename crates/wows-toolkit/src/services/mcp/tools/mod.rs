//! The tools the app adds to the MCP server: the replay index, a replay's header, complete
//! results, replayshark's analyses, and the files of the game builds on this machine.
//!
//! Every path a caller names is confined: replays to the replay folders the app knows, game files
//! to the virtual file system of an installed or cached build. The index is only read.

mod analysis;
mod error;
mod files;
mod index;
mod replays;

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::model::CallToolResult;
use rmcp::model::JsonObject;
use rmcp::model::Tool;
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::Semaphore;
use tokio::sync::watch;
use wows_mcp::ToolFuture;
use wows_mcp::ToolSet;
use wows_mcp::confine::PathRoots;
use wows_mcp::tool_set::parse_arguments;
use wows_mcp::tool_set::structured_answer;
use wows_mcp::tool_set::tool_failure;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::replay_folder::app_install_replay_dirs;

use crate::services::mcp::tools::error::ToolError;
use crate::services::mcp::tools::files::BuildFiles;

const INSTRUCTIONS: &str = "WoWs Toolkit replay tools. Find matches with replay_index_search, then name a replay by its arena_id (or a path inside the replay folders) in the other replay tools. replay_battle_results needs the game data of the replay's build; replay_header does not. replay_analysis runs replayshark's analyses: summary, chat_log, players and roster. Game files are read per build from game_builds.";

/// What the tools read, as the app knows it now. Replaced whenever a setting it comes from
/// changes.
#[derive(Clone, Default)]
pub struct ToolContext {
    /// The game install. `None` until one is set.
    pub wows_dir: Option<PathBuf>,
    /// The app's game data, which loads each build once. `None` until settings are read.
    pub game_data: Option<GameDataCache>,
    /// The game data cache's dump directory, which holds cached builds.
    pub dump_base: Option<PathBuf>,
    /// The app's replay index. `None` until the settings database is open.
    pub index: Option<SqlitePool>,
}

impl ToolContext {
    /// The install's live replay folders, the roots known without the index: in the install and,
    /// for an install the launcher ran from an overlay, where the game's writes land.
    pub fn replay_roots(&self) -> PathRoots {
        PathRoots::new(self.install_replay_dirs())
    }

    fn install_replay_dirs(&self) -> Vec<PathBuf> {
        self.wows_dir.iter().flat_map(|dir| app_install_replay_dirs(dir)).collect()
    }
}

/// The app's tools, over the context the app publishes.
pub struct ReplayTools {
    context: watch::Receiver<ToolContext>,
    /// Bounds the replays parsed at once; each holds a build's game data and the parse.
    parses: Arc<Semaphore>,
    files: BuildFiles,
}

impl ReplayTools {
    pub fn new(context: watch::Receiver<ToolContext>) -> Self {
        const PARALLEL_PARSES: usize = 2;

        Self { context, parses: Arc::new(Semaphore::new(PARALLEL_PARSES)), files: BuildFiles::new() }
    }

    fn context(&self) -> ToolContext {
        self.context.borrow().clone()
    }

    /// The app's index.
    fn index(&self) -> Result<SqlitePool, ToolError> {
        self.context().index.ok_or(ToolError::IndexUnavailable)
    }

    /// Every folder a replay may be read from: the install's live folders and each folder the
    /// index holds replays of.
    async fn replay_roots(&self) -> Result<PathRoots, ToolError> {
        let mut roots = self.context().install_replay_dirs();
        if let Ok(pool) = self.index() {
            let sources = wows_toolkit_config::index::query::list_sources(&pool)
                .await
                .map_err(|source| ToolError::Index { source })?;
            roots.extend(sources.into_iter().filter_map(|source| source.root_path));
        }

        Ok(PathRoots::new(roots))
    }
}

impl ToolSet for ReplayTools {
    fn tools(&self) -> Vec<Tool> {
        vec![
            index::search_tool(),
            index::players_tool(),
            replays::header_tool(),
            replays::results_tool(),
            analysis::analysis_tool(),
            files::builds_tool(),
            files::list_tool(),
            files::read_tool(),
        ]
    }

    fn call<'a>(&'a self, name: &'a str, arguments: JsonObject) -> ToolFuture<'a> {
        Box::pin(async move {
            match name {
                index::SEARCH => answer(index::search(self, parse_arguments(name, arguments)?).await),
                index::PLAYERS => answer(index::players(self, parse_arguments(name, arguments)?).await),
                replays::HEADER => answer(replays::header(self, parse_arguments(name, arguments)?).await),
                replays::RESULTS => answer(replays::results(self, parse_arguments(name, arguments)?).await),
                analysis::ANALYSIS => answer(analysis::analyze(self, parse_arguments(name, arguments)?).await),
                files::BUILDS => answer(files::builds(self, parse_arguments(name, arguments)?).await),
                files::LIST => answer(files::list(self, parse_arguments(name, arguments)?).await),
                files::READ => answer(files::read(self, parse_arguments(name, arguments)?).await),
                other => Err(ErrorData::invalid_params(format!("no tool named {other:?}"), None)),
            }
        })
    }

    fn instructions(&self) -> Option<String> {
        Some(INSTRUCTIONS.to_owned())
    }
}

/// A tool's answer as structured content, or its failure shown to the model.
fn answer<O>(result: Result<O, ToolError>) -> Result<CallToolResult, ErrorData>
where
    O: Serialize,
{
    match result {
        Ok(answer) => structured_answer(&answer),
        Err(ToolError::InvalidArgument { message }) => Err(ErrorData::invalid_params(message, None)),
        Err(error) => Ok(tool_failure(error)),
    }
}

/// Runs blocking work off the runtime's workers.
async fn blocking<T>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, ToolError>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work).await.map_err(|error| ToolError::Task { reason: error.to_string() })
}

/// A page of a longer list.
#[derive(Clone, Copy, Debug)]
struct Page {
    offset: usize,
    limit: usize,
}

impl Page {
    /// Checks a caller's paging against the bounds a tool allows.
    fn new(offset: Option<u32>, limit: Option<u32>, default_limit: u32, max_limit: u32) -> Result<Self, ToolError> {
        let limit = limit.unwrap_or(default_limit);
        if limit == 0 || limit > max_limit {
            return Err(ToolError::InvalidArgument { message: format!("limit must be from 1 to {max_limit}") });
        }

        Ok(Self { offset: offset.unwrap_or(0) as usize, limit: limit as usize })
    }

    /// This page of `items`, and the offset of the next page when there is one.
    fn slice<T>(self, items: Vec<T>) -> (Vec<T>, Option<u32>) {
        let total = items.len();
        let page: Vec<T> = items.into_iter().skip(self.offset).take(self.limit).collect();
        let end = self.offset + page.len();
        let next = (end < total).then(|| u32::try_from(end).unwrap_or(u32::MAX));

        (page, next)
    }
}

/// `path` as text for an answer.
fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
