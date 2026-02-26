use std::fmt;

/// Typed error for video encoding and muxing operations.
#[derive(Debug)]
pub enum VideoError {
    /// Encoder initialization failed (Vulkan, openh264, etc.)
    EncoderInit(String),
    /// Frame encoding failed.
    EncodeFailed(String),
    /// MP4 muxing failed.
    MuxFailed(String),
    /// I/O error (file creation, writes, etc.)
    Io(std::io::Error),
}

impl fmt::Display for VideoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncoderInit(msg) => write!(f, "encoder initialization failed: {msg}"),
            Self::EncodeFailed(msg) => write!(f, "encode failed: {msg}"),
            Self::MuxFailed(msg) => write!(f, "MP4 mux failed: {msg}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for VideoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
