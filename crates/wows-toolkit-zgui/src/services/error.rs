#[derive(Debug, thiserror::Error)]
pub enum TokioError {
    #[error("the tokio runtime could not be started")]
    NoRuntime,
    #[error("the tokio task did not complete")]
    Join(#[source] tokio::task::JoinError),
}

/// Why a change to the saved session was not written.
#[derive(Debug, thiserror::Error)]
pub enum SessionWriteError {
    #[error("the config database is not open")]
    NoDatabase,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Runtime(#[from] TokioError),
}
