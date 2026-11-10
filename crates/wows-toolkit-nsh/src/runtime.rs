//! The tokio runtime background network work runs on: launcher catalog
//! checks, downloads and sign-in, and collaboration sessions.

use std::sync::Arc;
use std::sync::LazyLock;

static RUNTIME: LazyLock<Result<Arc<tokio::runtime::Runtime>, RuntimeError>> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        // Network-bound work; the blocking pool takes the downloads.
        .worker_threads(2)
        .thread_name("wows-toolkit-background")
        .enable_all()
        .build()
        .map(Arc::new)
        .map_err(|error| RuntimeError { reason: error.to_string() })
});

/// Why the runtime could not start.
#[derive(Debug, Clone, thiserror::Error)]
#[error("could not start the background runtime: {reason}")]
pub struct RuntimeError {
    reason: String,
}

/// The app's runtime, started on first use.
pub fn shared() -> Result<Arc<tokio::runtime::Runtime>, RuntimeError> {
    RUNTIME.clone()
}
