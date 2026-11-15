use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::RoleServer;
use rmcp::ServerHandler;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResponse;
use rmcp::model::Implementation;
use rmcp::model::ListToolsResult;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerConfig;
use rmcp::model::Tool;
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rootcause::prelude::*;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::error::BuildError;
use crate::error::ServeError;
use crate::game_data::GameDataSource;
use crate::game_data::GameDataTools;
use crate::http::HttpEndpoint;
use crate::tool_set::ToolSet;

/// How the server talks to its client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// One client on standard input and output.
    Stdio,
    /// Streamable HTTP on this address. Port 0 picks a free port.
    Http(SocketAddr),
}

/// Assembles an [`McpServer`] from a game build's query tools, extra tool
/// sets and a transport.
pub struct McpServerBuilder {
    description: String,
    tool_sets: Vec<Arc<dyn ToolSet>>,
    transport: Transport,
}

impl McpServerBuilder {
    fn new(transport: Transport) -> Self {
        Self { description: "World of Warships tools".to_owned(), tool_sets: Vec::new(), transport }
    }

    /// Serves the `wows_query` tools and `replay_summary` over `source`.
    pub fn game_data(mut self, source: GameDataSource) -> Self {
        self.description = match &source {
            GameDataSource::Loaded(game) => format!("World of Warships {} game data", game.version().to_path()),
            GameDataSource::Dump { .. } => "World of Warships game data".to_owned(),
        };
        self.tool_sets.insert(0, Arc::new(GameDataTools::new(source)));
        self
    }

    pub fn tool_set(mut self, tool_set: Arc<dyn ToolSet>) -> Self {
        self.tool_sets.push(tool_set);
        self
    }

    /// Indexes every tool and, for HTTP, binds the listener, so an occupied
    /// port is reported here rather than once serving starts.
    pub async fn build(self) -> Result<McpServer, Report<BuildError>> {
        let mut tools = Vec::new();
        let mut routes = HashMap::new();
        for (index, tool_set) in self.tool_sets.iter().enumerate() {
            for tool in tool_set.tools() {
                if routes.insert(tool.name.to_string(), index).is_some() {
                    return Err(report!(BuildError::DuplicateTool { name: tool.name.to_string() }));
                }
                tools.push(tool);
            }
        }

        let instructions: Vec<String> = self.tool_sets.iter().filter_map(|tool_set| tool_set.instructions()).collect();
        let handler = Handler {
            shared: Arc::new(Shared {
                tools: tools.into(),
                routes,
                tool_sets: self.tool_sets.into(),
                description: self.description,
                instructions: instructions.join("\n\n"),
            }),
        };

        let transport = match self.transport {
            Transport::Stdio => BoundTransport::Stdio,
            Transport::Http(address) => {
                let listener = TcpListener::bind(address)
                    .await
                    .map_err(|source| report!(BuildError::Listen { address, source }))?;
                let local = listener.local_addr().map_err(|source| report!(BuildError::Listen { address, source }))?;
                BoundTransport::Http { listener, endpoint: HttpEndpoint::new(local) }
            }
        };

        Ok(McpServer { handler, transport })
    }
}

/// A built MCP server, ready to run.
pub struct McpServer {
    handler: Handler,
    transport: BoundTransport,
}

impl McpServer {
    pub fn builder(transport: Transport) -> McpServerBuilder {
        McpServerBuilder::new(transport)
    }

    /// The bound HTTP endpoint, for [`Transport::Http`].
    pub fn http_endpoint(&self) -> Option<HttpEndpoint> {
        match &self.transport {
            BoundTransport::Stdio => None,
            BoundTransport::Http { endpoint, .. } => Some(*endpoint),
        }
    }

    /// Serves until the client disconnects (stdio) or `shutdown` is
    /// cancelled.
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), Report<ServeError>> {
        match self.transport {
            BoundTransport::Stdio => {
                let running = self.handler.serve_with_ct(stdio(), shutdown).await.context(ServeError::Stdio)?;
                running.waiting().await.context(ServeError::Stdio)?;
            }
            BoundTransport::Http { listener, endpoint } => {
                crate::http::serve(self.handler, listener, endpoint, shutdown).await;
            }
        }

        Ok(())
    }
}

enum BoundTransport {
    Stdio,
    Http { listener: TcpListener, endpoint: HttpEndpoint },
}

struct Shared {
    tools: Arc<[Tool]>,
    /// Tool name to its index in `tool_sets`.
    routes: HashMap<String, usize>,
    tool_sets: Arc<[Arc<dyn ToolSet>]>,
    description: String,
    instructions: String,
}

/// The rmcp handler, cloned once per session.
#[derive(Clone)]
pub(crate) struct Handler {
    shared: Arc<Shared>,
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        let shared = &self.shared;

        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                    .with_description(shared.description.clone()),
            )
            .with_instructions(shared.instructions.clone())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.shared.tools.to_vec()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some(&index) = self.shared.routes.get(request.name.as_ref()) else {
            return Err(ErrorData::invalid_params(format!("no tool named {:?}", request.name), None));
        };

        // MCP lets a client omit `arguments` for a tool that takes none; an
        // empty object then gives the usual missing-field message.
        let arguments = request.arguments.unwrap_or_default();
        let result = self.shared.tool_sets[index].call(&request.name, arguments).await?;

        Ok(result.into())
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.shared.tools.iter().find(|tool| tool.name == name).cloned()
    }
}
