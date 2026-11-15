//! MCP server answering World of Warships game data questions, extensible
//! with embedder-defined tool sets.

mod error;
mod game_data;
mod http;
mod server;
pub mod tool_set;

pub use error::BuildError;
pub use error::ServeError;
pub use game_data::GameDataSource;
pub use game_data::GameDataTools;
pub use game_data::ReplaySummaryArgs;
pub use game_data::load_dump;
pub use http::HttpEndpoint;
pub use server::McpServer;
pub use server::McpServerBuilder;
pub use server::Transport;
pub use tool_set::ToolFuture;
pub use tool_set::ToolSet;
