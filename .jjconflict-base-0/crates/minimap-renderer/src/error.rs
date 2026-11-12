use std::fmt;

/// Typed error categories for video encoding and muxing operations.
///
/// These are head-of-chain context markers, not string-encoded errors: the
/// underlying library error (muxide, gpu-video, openh264, rav1e, I/O, ...) is
/// preserved as a child in the rootcause report via `.context(...)`, and any
/// diagnostic detail is added with `.attach(...)`. Callers match on these
/// variants structurally rather than parsing a message.
#[derive(Debug)]
pub enum VideoError {
    EncoderInit,
    EncodeFailed,
    MuxFailed,
    Io,
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
            Self::EncoderInit => write!(f, "encoder initialization failed"),
            Self::EncodeFailed => write!(f, "encode failed"),
            Self::MuxFailed => write!(f, "MP4 mux failed"),
            Self::Io => write!(f, "I/O error"),
            Self::UnsupportedCodec { codec, backend, reason } => {
                write!(f, "{backend} backend does not support codec {codec}: {reason}")
            }
        }
    }
}

impl std::error::Error for VideoError {}
