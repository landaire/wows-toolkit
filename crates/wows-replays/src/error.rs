use thiserror::Error;
use winnow::error::ErrMode;
use winnow::error::ParserError;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("failed to parse packet bytes")]
    InvalidPacketData,

    #[error("replay file is incomplete or truncated")]
    IncompleteReplay,

    #[error("invalid JSON in replay data")]
    InvalidJson(#[from] serde_json::Error),

    #[error("invalid UTF-8 in replay data")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    #[error("invalid UTF-8 in replay data")]
    InvalidUtf8Owned(#[from] std::string::FromUtf8Error),

    #[error("unsupported replay version: {version}")]
    UnsupportedReplayVersion { version: String },

    #[error("failed to parse RPC value for {method} arg #{argnum} (type {argtype}, def {semantic_name:?}): {error}")]
    RpcValueParseFailed { method: String, argnum: usize, argtype: String, semantic_name: Option<String>, error: String },

    #[error("internal property set on unsupported entity {entity_type} (id={entity_id})")]
    UnsupportedInternalPropSet { entity_id: u32, entity_type: String },

    #[error("packet references entity {entity_id} that has not been created")]
    UnknownEntity { entity_id: u32 },

    #[error("entity type {entity_type} is out of bounds for {spec_count} loaded entity specs")]
    EntityTypeOutOfBounds { entity_type: u16, spec_count: usize },

    #[error("property id {prop_id} is out of bounds for entity type {entity_type}")]
    PropertyIdOutOfBounds { prop_id: u32, entity_type: u16 },

    #[error("method id {method_id} is out of bounds for entity type {entity_type}")]
    MethodIdOutOfBounds { method_id: u32, entity_type: u16 },

    #[error(
        "entity-method {method} (id {method_id}, entity type {entity_type}) has only {got} payload bytes but its fixed-size args require {expected}; \
         the exposed-method id table is out of sync with the client (a type's on-wire size is misclassified as fixed vs variable), \
         which would otherwise silently mislabel every later method"
    )]
    ExposedMethodMappingDrift { method: String, method_id: u32, entity_type: u16, expected: usize, got: usize },

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
            ErrMode::Incomplete(_) => ParseError::IncompleteReplay,
            ErrMode::Backtrack(e) | ErrMode::Cut(e) => e,
        }
    }
}

pub type PResult<T> = winnow::error::ModalResult<T, ParseError>;

pub fn failure(err: ParseError) -> ErrMode<ParseError> {
    ErrMode::Cut(err)
}
