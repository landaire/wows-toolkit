#[derive(Debug, thiserror::Error)]
pub enum TokioError {
    #[error("the tokio runtime could not be started")]
    NoRuntime,
    #[error("the tokio task did not complete")]
    Join(#[source] tokio::task::JoinError),
}
