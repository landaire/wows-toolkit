use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("link I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame of {len} bytes exceeds the {max} byte limit")]
    TooLong { len: u64, max: u32 },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TokenParseError {
    #[error("relay token has {found} hex digits, expected {expected}")]
    Length { found: usize, expected: usize },
    #[error("relay token has a non-hex character at offset {offset}")]
    NotHex { offset: usize },
}
