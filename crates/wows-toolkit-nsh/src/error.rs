use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("reading replay directory {path}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("no replay in the fixture directories parsed")]
    NoReplays,
}
