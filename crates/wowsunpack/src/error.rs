use thiserror::Error;

#[derive(Debug, Error)]
pub enum GameDataError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[cfg(feature = "vfs")]
    #[error(transparent)]
    Vfs(#[from] vfs::VfsError),
    #[error("Unexpected GameParams data type")]
    InvalidGameParamsData,
    #[error(transparent)]
    Pickle(#[from] pickled::Error),
    #[error(transparent)]
    FileTree(#[from] crate::data::idx::IdxError),
    #[error("Build {build} not found in game directory")]
    BuildNotFound { build: u32 },
    #[error("replay version carries no build number")]
    BuildUnknown,
    #[error("res_packages directory not found")]
    ResPackagesNotFound,
    #[cfg(feature = "json")]
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[cfg(feature = "cbor")]
    #[error(transparent)]
    Cbor(#[from] ciborium::ser::Error<std::io::Error>),
}

/// Why the mod folder of a build could not be found.
#[derive(Debug, Error)]
pub enum ModFolderError {
    #[error("{path} could not be read")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not well-formed XML")]
    Parse { path: std::path::PathBuf },
    #[error("{path} lists no search path")]
    NoSearchPath { path: std::path::PathBuf },
}
