//! Video codec selection types shared across encoder backends, the CLI, and
//! the desktop GUI.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

/// Video codec used for the encoded MP4 stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
}

impl VideoCodec {
    pub const ALL: [Self; 3] = [Self::H264, Self::H265, Self::Av1];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Av1 => "av1",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::H264 => "H.264 (AVC)",
            Self::H265 => "H.265 (HEVC)",
            Self::Av1 => "AV1",
        }
    }
}

impl fmt::Display for VideoCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for VideoCodec {
    type Err = ParseCodecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "h264" | "avc" | "h.264" => Ok(Self::H264),
            "h265" | "hevc" | "h.265" => Ok(Self::H265),
            "av1" => Ok(Self::Av1),
            other => Err(ParseCodecError(other.to_owned())),
        }
    }
}

#[derive(Debug)]
pub struct ParseCodecError(pub String);

impl fmt::Display for ParseCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown codec '{}' (expected h264, h265, or av1)", self.0)
    }
}

impl std::error::Error for ParseCodecError {}

/// Which backend an encoder runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderKind {
    Gpu,
    Cpu,
}

impl EncoderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gpu => "gpu",
            Self::Cpu => "cpu",
        }
    }
}

impl fmt::Display for EncoderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
