use thiserror::Error;
use winnow::error::{ErrMode, ParserError};

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("failed to parse packet bytes")]
    InvalidPacketData,

    #[error("invalid JSON in replay data")]
    InvalidJson(#[from] serde_json::Error),

    #[error("invalid UTF-8 in replay data")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    #[error("invalid UTF-8 in replay data")]
    InvalidUtf8Owned(#[from] std::string::FromUtf8Error),

    #[error("unsupported replay version: {version}")]
    UnsupportedReplayVersion { version: String },

    #[error("failed to parse RPC value for {method} arg #{argnum} (type {argtype}): {error}")]
    RpcValueParseFailed { method: String, argnum: usize, argtype: String, error: String },

    #[error("internal property set on unsupported entity {entity_type} (id={entity_id})")]
    UnsupportedInternalPropSet { entity_id: u32, entity_type: String },

    #[error("I/O error")]
    Io(#[from] std::io::Error),
}

impl ParserError<&[u8]> for ParseError {
    type Inner = Self;

    fn from_input(_input: &&[u8]) -> Self {
        Self::InvalidPacketData
    }

    fn into_inner(self) -> Result<Self::Inner, Self> {
        Ok(self)
    }
}

impl From<ErrMode<ParseError>> for ParseError {
    fn from(x: ErrMode<ParseError>) -> Self {
        match x {
            ErrMode::Incomplete(_) => panic!("can't handle incomplete replay files"),
            ErrMode::Backtrack(e) | ErrMode::Cut(e) => e,
        }
    }
}

pub type PResult<T> = winnow::error::ModalResult<T, ParseError>;

pub fn failure(err: ParseError) -> ErrMode<ParseError> {
    ErrMode::Cut(err)
}
