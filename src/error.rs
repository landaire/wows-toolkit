use std::io;
use thiserror::Error;
use wowsunpack::idx::IdxError;

#[derive(Error, Debug)]
pub enum DataLoadError {
    #[error("Invalid World of Warships directory specified")]
    InvalidWowsDirectory,
    #[error("Invalid GameParams.data")]
    InvalidGameParams,
    #[error("Could not read IDX file")]
    UnpackerIdx(#[from] IdxError),
    #[error("I/O")]
    Io(#[from] io::Error),
    #[error("Could not deserialize GameParams.data")]
    GameParamsDeserialization(#[from] serde_pickle::Error),
    #[error("Unexpected field type for {0:?}")]
    GameParamsUnexpectedType(&'static str),
}
