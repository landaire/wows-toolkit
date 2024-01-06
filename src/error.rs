use std::{io, path::PathBuf};
use thiserror::Error;
use wowsunpack::idx::IdxError;

#[derive(Error, Debug)]
pub enum DataLoadError {
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
}
