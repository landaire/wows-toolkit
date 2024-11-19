use std::{io, path::PathBuf};
use thiserror::Error;
use wowsunpack::error::ErrorKind;

#[derive(Error, Debug)]
pub enum ToolkitError {
    #[error("Invalid World of Warships directory {0} specified. Specify the correct directory in the Settings tab.")]
    InvalidWowsDirectory(PathBuf),

    #[error("Invalid GameParams.data")]
    InvalidGameParams,

    #[error("Data unpacker error")]
    UnpackerError(#[from] ErrorKind),

    #[error("An I/O error occurred: {0}")]
    Io(#[from] io::Error),

    #[error("Could not deserialize GameParams.data")]
    GameParamsDeserialization(#[from] pickled::Error),

    #[error("Unexpected field type for {0:?}")]
    GameParamsUnexpectedType(&'static str),

    #[error("Replay version {replay_version:?} does not match loaded game version {game_version:?}")]
    ReplayVersionMismatch { game_version: String, replay_version: String },

    #[error("Background task completed")]
    BackgroundTaskCompleted,

    #[error("A network error occurred while downloading an update: {0}")]
    UpdateHttpError(#[from] reqwest::Error),

    #[error("Could not not read update ZipArchive")]
    ZipReadError(#[from] zip::result::ZipError),
}
