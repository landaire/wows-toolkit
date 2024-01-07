use std::{io, path::PathBuf};
use thiserror::Error;
use wowsunpack::idx::IdxError;

#[derive(Error, Debug)]
pub enum ToolkitError {
    #[error("Invalid World of Warships directory {0:?} specified")]
    InvalidWowsDirectory(PathBuf),

    #[error("Invalid GameParams.data")]
    InvalidGameParams,

    #[error("Could not read IDX file")]
    UnpackerIdx(#[from] IdxError),

    #[error("I/O")]
    Io(#[from] io::Error),

    #[error("Could not deserialize GameParams.data")]
    GameParamsDeserialization(#[from] pickled::Error),

    #[error("Unexpected field type for {0:?}")]
    GameParamsUnexpectedType(&'static str),

    #[error("Replay version {expected:?} does not match loaded game version {actual:?}")]
    ReplayVersionMismatch { expected: String, actual: String },

    #[error("Background task completed")]
    BackgroundTaskCompleted,
}
