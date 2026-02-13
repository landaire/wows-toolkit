use std::io;
use std::path::PathBuf;
use thiserror::Error;
use wowsunpack::error::ErrorKind;

#[derive(Error, Debug)]
pub enum ToolkitError {
    #[error("Invalid World of Warships directory {0} specified. Specify the correct directory in the Settings tab.")]
    InvalidWowsDirectory(PathBuf),

    #[error("Data unpacker error")]
    UnpackerError(#[from] ErrorKind),

    #[error("An I/O error occurred: {0}")]
    Io(#[from] io::Error),

    #[error("Could not deserialize GameParams.data")]
    GameParamsDeserialization(#[from] pickled::Error),

    #[error("Replay version {replay_version:?} does not match loaded game version {game_version:?}")]
    ReplayVersionMismatch { game_version: String, replay_version: String },

    #[error(
        "Game data for replay build {build} (version {version}) is not available. The game installation does not contain this build."
    )]
    ReplayBuildUnavailable { build: u32, version: String },

    #[error("Background task completed")]
    BackgroundTaskCompleted,

    #[error("A network error occurred while downloading an update: {0}")]
    UpdateHttpError(#[from] reqwest::Error),

    #[error("Could not not read update ZipArchive")]
    ZipReadError(#[from] zip::result::ZipError),

    #[error("JSON deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}
