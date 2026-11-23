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

/// Why a requested export timing was refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimingError {
    /// An export speed that is not finite or is below `min`.
    SpeedOutOfRange { multiplier: f32, min: f32 },
    /// A video length outside `min..=max` seconds.
    VideoLengthOutOfRange { seconds: f32, min: f32, max: f32 },
    /// Both a video length and a speed were given; an export takes one.
    LengthAndSpeed { seconds: f32, multiplier: f32 },
}

impl fmt::Display for TimingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpeedOutOfRange { multiplier, min } => {
                write!(f, "speed {multiplier} is not a finite number of at least {min}")
            }
            Self::VideoLengthOutOfRange { seconds, min, max } => {
                write!(f, "video length {seconds} s is not between {min} s and {max} s")
            }
            Self::LengthAndSpeed { seconds, multiplier } => {
                write!(f, "video length {seconds} s and speed {multiplier} were both given; choose one")
            }
        }
    }
}

impl std::error::Error for TimingError {}

/// A target file size smaller than a video of `output_seconds` can be made at
/// the lowest bitrate the encoder is given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSizeError {
    pub requested_bytes: u64,
    pub minimum_bytes: u64,
    pub output_seconds: f32,
}

impl fmt::Display for TargetSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const MIB: f64 = 1024.0 * 1024.0;
        write!(
            f,
            "a {:.2} MiB file cannot hold {:.1} s of video; the smallest achievable is {:.2} MiB",
            self.requested_bytes as f64 / MIB,
            self.output_seconds,
            self.minimum_bytes as f64 / MIB
        )
    }
}

impl std::error::Error for TargetSizeError {}

/// Why a track file could not be written or read.
#[cfg(feature = "track-file")]
#[derive(Debug, thiserror::Error)]
pub enum TrackFileError {
    #[error("not a track file")]
    NotATrackFile,
    #[error("track file version {} is not the supported version {}", found.get(), supported.get())]
    UnsupportedVersion { found: crate::track_file::TrackFileVersion, supported: crate::track_file::TrackFileVersion },
    #[error("track file payload could not be compressed")]
    Compress(#[source] std::io::Error),
    #[error("track file payload is not valid zlib")]
    Decompress(#[source] std::io::Error),
    #[error("track file payload decompresses to more than {limit} bytes")]
    PayloadTooLarge { limit: usize },
    #[error("track file archive is malformed")]
    Archive(#[source] rkyv::rancor::Error),
    #[error("track file holds no frames")]
    NoFrames,
    #[error("track file holds {frames} frames but {clocks} clocks")]
    ClockCount { frames: usize, clocks: usize },
    #[error("track file clock {index} is not finite or runs backwards")]
    BadClock { index: usize },
    #[error("track file icon {key} is {width}x{height} but holds {bytes} bytes of RGBA")]
    IconSize { key: String, width: u32, height: u32, bytes: usize },
}
