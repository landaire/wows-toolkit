use std::net::SocketAddr;

/// Failure to assemble an [`crate::McpServer`].
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("two tool sets both provide a tool named {name:?}")]
    DuplicateTool { name: String },

    #[error("failed to listen on {address}")]
    Listen {
        address: SocketAddr,
        #[source]
        source: std::io::Error,
    },
}

impl BuildError {
    /// The OS error kind of a bind failure, so callers can tell an occupied
    /// port from other failures.
    pub fn listen_error_kind(&self) -> Option<std::io::ErrorKind> {
        match self {
            Self::Listen { source, .. } => Some(source.kind()),
            Self::DuplicateTool { .. } => None,
        }
    }
}

/// Failure while serving MCP.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("failed to serve MCP on stdio")]
    Stdio,
}
