use std::fmt;

/// Typed error for video encoding and muxing operations.
#[derive(Debug)]
pub enum VideoError {
    EncoderInit(String),
    EncodeFailed(String),
    MuxFailed(String),
    Io(std::io::Error),
    /// Requested codec is not supported by any compiled-in backend or
    /// available device for the chosen execution mode.
    UnsupportedCodec {
        codec: &'static str,
        backend: &'static str,
        reason: String,
    },
}

impl fmt::Display for VideoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncoderInit(msg) => write!(f, "encoder initialization failed: {msg}"),
            Self::EncodeFailed(msg) => write!(f, "encode failed: {msg}"),
            Self::MuxFailed(msg) => write!(f, "MP4 mux failed: {msg}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::UnsupportedCodec { codec, backend, reason } => {
                write!(f, "{backend} backend does not support codec {codec}: {reason}")
            }
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
