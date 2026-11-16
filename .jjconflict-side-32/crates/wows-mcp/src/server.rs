use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::RoleServer;
use rmcp::ServerHandler;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResponse;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use rmcp::model::Implementation;
use rmcp::model::ListToolsResult;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerConfig;
use rmcp::model::Tool;
use rmcp::service::RequestContext;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use wows_query::GameData;
use wows_query::tools::ToolCallError;

const INSTRUCTIONS: &str = "World of Warships game data for one game build. Ship arguments take the English ship name as players write it. Answer from tool output; numbers come from GameParams.";

const REPLAY_SUMMARY: &str = "replay_summary";

/// Arguments for `replay_summary`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaySummaryArgs {
    /// Path of a .wowsreplay file on the server's file system.
    pub path: String,
}

/// MCP tools over one loaded game build: every `wows_query::tools` tool plus
/// `replay_summary`, which reads a replay from the server's file system.
#[derive(Clone)]
pub struct GameDataServer {
    game: Arc<GameData>,
    tools: Arc<[Tool]>,
}

impl GameDataServer {
    pub fn new(game: Arc<GameData>) -> Self {
        let mut tools: Vec<Tool> = wows_query::tools::specs()
            .into_iter()
            .map(|spec| Tool::new(spec.name, spec.description, spec.input_schema))
            .collect();
        tools.push(Tool::new(
            REPLAY_SUMMARY,
            "Summarize a .wowsreplay file on the server: recording player, map, mode, client version, date, duration and every player's ship and team.",
            wows_query::tools::input_schema::<ReplaySummaryArgs>(),
        ));

        Self { game, tools: tools.into() }
    }

    async fn replay_summary(&self, arguments: Value) -> Result<String, String> {
        // Replays are tens of MB; the bound keeps a device or FIFO path from
        // exhausting memory.
        const REPLAY_LIMIT: u64 = 256 * 1024 * 1024;

        let args: ReplaySummaryArgs = serde_json::from_value(arguments)
            .map_err(|error| format!("invalid arguments for {REPLAY_SUMMARY}: {error}"))?;
        let file = tokio::fs::File::open(&args.path).await.map_err(|_| format!("cannot read replay {}", args.path))?;
        let mut bytes = Vec::new();
        file.take(REPLAY_LIMIT + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| format!("cannot read replay {}", args.path))?;
        if bytes.len() as u64 > REPLAY_LIMIT {
            return Err(format!("{} is larger than any replay", args.path));
        }

        let game = Arc::clone(&self.game);
        let summary = tokio::task::spawn_blocking(move || game.replay_summary(&bytes))
            .await
            .map_err(|error| format!("query task failed: {error}"))?
            .map_err(|error| error.to_string())?;

        serde_json::to_string(&summary).map_err(|error| format!("failed to encode the answer: {error}"))
    }

    /// Runs a registry tool off the async workers. The outer error is a
    /// protocol error; the inner one is shown to the caller.
    async fn registry_tool(&self, name: String, arguments: Value) -> Result<Result<String, String>, ErrorData> {
        let game = Arc::clone(&self.game);
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

impl ServerHandler for GameDataServer {
    fn get_info(&self) -> ServerConfig {
        let version = self.game.version();
        let description = format!("World of Warships {} game data", version.to_path());

        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).with_description(description),
            )
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.to_vec()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // MCP lets a client omit `arguments` for a tool that takes none; an
        // empty object then gives the usual missing-field message.
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let answer = match request.name.as_ref() {
            REPLAY_SUMMARY => self.replay_summary(arguments).await,
            name => self.registry_tool(name.to_owned(), arguments).await?,
        };

        let result = match answer {
            Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
        };

        Ok(result.into())
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }
}
