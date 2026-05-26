//! Video encoder backends.
//!
//! GPU encoders:
//!   - `gpu`: H.264 + H.265 via gpu-video / Vulkan Video (Linux/Windows)
//!   - `videotoolbox`: H.264 via Apple VideoToolbox (macOS)
//!
//! CPU encoders:
//!   - `cpu`: H.264 via openh264
//!   - `cpu_av1`: AV1 via rav1e

use std::collections::BTreeMap;

use rootcause::prelude::*;
#[cfg(any(
    feature = "cpu",
    feature = "cpu-av1",
    all(feature = "vulkan", not(target_os = "macos")),
    all(feature = "videotoolbox", target_os = "macos"),
))]
use tracing::info;

use crate::codec::EncoderKind;
use crate::codec::VideoCodec;
use crate::error::VideoError;

#[cfg(all(feature = "vulkan", not(target_os = "macos")))]
pub mod gpu;

#[cfg(all(feature = "videotoolbox", target_os = "macos"))]
pub mod videotoolbox;

#[cfg(feature = "cpu")]
pub mod cpu;

#[cfg(feature = "cpu-av1")]
pub mod cpu_av1;

/// Snapshot of which (codec, backend) combinations are usable on this system.
#[derive(Debug, Default, Clone)]
pub struct EncoderStatus {
    pub gpu_adapter_name: Option<String>,
    pub gpu_error: Option<String>,
    pub gpu_codecs: BTreeMap<VideoCodec, CodecSupport>,
    pub cpu_codecs: BTreeMap<VideoCodec, CodecSupport>,
}

#[derive(Debug, Clone)]
pub enum CodecSupport {
    Supported,
    Unsupported(String),
}

impl CodecSupport {
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Supported)
    }
}

impl EncoderStatus {
    pub fn supports(&self, kind: EncoderKind, codec: VideoCodec) -> bool {
        let table = match kind {
            EncoderKind::Gpu => &self.gpu_codecs,
            EncoderKind::Cpu => &self.cpu_codecs,
        };
        table.get(&codec).is_some_and(CodecSupport::is_supported)
    }

    /// Recommended default codec.
    ///
    /// Honors `prefer_cpu` as a hard constraint. When the GPU is available, prefer
    /// H.265 (better compression than H.264, broadly supported). Otherwise
    /// default to AV1 (CPU, best compression).
    pub fn best_codec(&self, prefer_cpu: bool) -> VideoCodec {
        if !prefer_cpu {
            for codec in [VideoCodec::H265, VideoCodec::H264, VideoCodec::Av1] {
                if self.supports(EncoderKind::Gpu, codec) {
                    return codec;
                }
            }
        }
        for codec in [VideoCodec::Av1, VideoCodec::H264, VideoCodec::H265] {
            if self.supports(EncoderKind::Cpu, codec) {
                return codec;
            }
        }
        VideoCodec::H264
    }

    pub fn gpu_available(&self) -> bool {
        self.gpu_codecs.values().any(CodecSupport::is_supported)
    }

    pub fn cpu_available(&self) -> bool {
        self.cpu_codecs.values().any(CodecSupport::is_supported)
    }

    /// Iterate over codecs that are usable via either GPU or CPU.
    pub fn supported_codecs(&self) -> impl Iterator<Item = VideoCodec> + '_ {
        VideoCodec::ALL.into_iter().filter(|c| {
            self.supports(EncoderKind::Gpu, *c) || self.supports(EncoderKind::Cpu, *c)
        })
    }
}

impl std::fmt::Display for EncoderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Encoder status:")?;
        match (&self.gpu_adapter_name, &self.gpu_error) {
            (Some(name), _) if self.gpu_available() => writeln!(f, "  GPU: {name}")?,
            (Some(name), Some(err)) => writeln!(f, "  GPU: {name} (no encode support: {err})")?,
            (Some(name), None) => writeln!(f, "  GPU: {name} (no encode support)")?,
            (None, Some(err)) => writeln!(f, "  GPU: unavailable - {err}")?,
            (None, None) => writeln!(f, "  GPU: not compiled in")?,
        }
        for codec in VideoCodec::ALL {
            write_codec_line(f, "    GPU", codec, self.gpu_codecs.get(&codec))?;
        }
        writeln!(f, "  CPU:")?;
        for codec in VideoCodec::ALL {
            write_codec_line(f, "   ", codec, self.cpu_codecs.get(&codec))?;
        }
        Ok(())
    }
}

fn write_codec_line(
    f: &mut std::fmt::Formatter<'_>,
    prefix: &str,
    codec: VideoCodec,
    support: Option<&CodecSupport>,
) -> std::fmt::Result {
    match support {
        Some(CodecSupport::Supported) => writeln!(f, "{prefix} {}: supported", codec.display_name()),
        Some(CodecSupport::Unsupported(why)) => {
            writeln!(f, "{prefix} {}: unsupported ({why})", codec.display_name())
        }
        None => writeln!(f, "{prefix} {}: not compiled in", codec.display_name()),
    }
}

/// Probe the system to determine which encoders are available for which codecs.
pub fn check_encoder() -> EncoderStatus {
    let mut status = EncoderStatus::default();

    #[cfg(feature = "cpu")]
    {
        status.cpu_codecs.insert(VideoCodec::H264, CodecSupport::Supported);
    }
    #[cfg(not(feature = "cpu"))]
    {
        status
            .cpu_codecs
            .insert(VideoCodec::H264, CodecSupport::Unsupported("openh264 backend not compiled in".into()));
    }

    #[cfg(feature = "cpu-av1")]
    {
        status.cpu_codecs.insert(VideoCodec::Av1, CodecSupport::Supported);
    }
    #[cfg(not(feature = "cpu-av1"))]
    {
        status
            .cpu_codecs
            .insert(VideoCodec::Av1, CodecSupport::Unsupported("rav1e backend not compiled in".into()));
    }

    status.cpu_codecs.insert(
        VideoCodec::H265,
        CodecSupport::Unsupported("no CPU H.265 backend available".into()),
    );

    #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
    {
        status.gpu_adapter_name = Some("VideoToolbox".to_string());
        status.gpu_codecs.insert(VideoCodec::H264, CodecSupport::Supported);
        status
            .gpu_codecs
            .insert(VideoCodec::H265, CodecSupport::Unsupported("VideoToolbox H.265 not yet wired up".into()));
        status
            .gpu_codecs
            .insert(VideoCodec::Av1, CodecSupport::Unsupported("no AV1 GPU encoder".into()));
    }

    #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
    {
        gpu::probe_status(&mut status);
    }

    #[cfg(not(any(
        all(feature = "vulkan", not(target_os = "macos")),
        all(feature = "videotoolbox", target_os = "macos")
    )))]
    {
        status.gpu_error = Some("GPU encoder not compiled in".to_string());
        for codec in VideoCodec::ALL {
            status
                .gpu_codecs
                .insert(codec, CodecSupport::Unsupported("GPU backend not compiled in".into()));
        }
    }

    status
}

pub enum EncoderBackend {
    #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
    Gpu(Box<gpu::GpuEncoder>),
    #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
    VideoToolbox(Box<videotoolbox::VideoToolboxEncoder>),
    #[cfg(feature = "cpu")]
    CpuH264(Box<cpu::CpuEncoder>),
    #[cfg(feature = "cpu-av1")]
    CpuAv1(Box<cpu_av1::CpuAv1Encoder>),
}

/// Outcome of creating an encoder: the backend plus the codec it ended up using.
///
/// The codec may differ from what the caller requested in `Mode::Auto` when a
/// GPU encoder for the requested codec isn't available — the GUI uses this to
/// silently fall back to CPU.
pub struct CreatedEncoder {
    pub backend: EncoderBackend,
    pub codec: VideoCodec,
    pub kind: EncoderKind,
}

/// How strictly the caller wants to honor the requested codec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Try requested codec via GPU first, fall back to CPU silently if needed.
    /// Used by the GUI.
    Auto,
    /// Caller asked for CPU explicitly; require CPU support for the codec.
    ForceCpu,
    /// Caller asked for GPU explicitly; require GPU support for the codec.
    ForceGpu,
}

impl EncoderBackend {
    pub fn create(
        width: u32,
        height: u32,
        codec: VideoCodec,
        mode: Mode,
    ) -> rootcause::Result<CreatedEncoder, VideoError> {
        let status = check_encoder();

        if mode != Mode::ForceCpu && status.supports(EncoderKind::Gpu, codec) {
            match Self::create_gpu(width, height, codec) {
                Ok(backend) => {
                    info!(codec = %codec, "Using GPU encoder");
                    return Ok(CreatedEncoder { backend, codec, kind: EncoderKind::Gpu });
                }
                Err(e) if mode == Mode::ForceGpu => return Err(e),
                Err(e) => tracing::warn!(error = %e, "GPU encoder init failed; falling back to CPU"),
            }
        } else if mode == Mode::ForceGpu {
            return Err(report!(VideoError::UnsupportedCodec {
                codec: codec.as_str(),
                backend: "gpu",
                reason: status
                    .gpu_codecs
                    .get(&codec)
                    .and_then(|s| match s {
                        CodecSupport::Unsupported(why) => Some(why.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "not available on this device".to_string()),
            }));
        }

        if !status.supports(EncoderKind::Cpu, codec) {
            return Err(report!(VideoError::UnsupportedCodec {
                codec: codec.as_str(),
                backend: "cpu",
                reason: status
                    .cpu_codecs
                    .get(&codec)
                    .and_then(|s| match s {
                        CodecSupport::Unsupported(why) => Some(why.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "not compiled in".to_string()),
            }));
        }

        let backend = Self::create_cpu(width, height, codec)?;
        info!(codec = %codec, "Using CPU encoder");
        Ok(CreatedEncoder { backend, codec, kind: EncoderKind::Cpu })
    }

    #[allow(unused_variables)]
    fn create_gpu(width: u32, height: u32, codec: VideoCodec) -> rootcause::Result<Self, VideoError> {
        #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
        {
            if codec != VideoCodec::H264 {
                return Err(report!(VideoError::UnsupportedCodec {
                    codec: codec.as_str(),
                    backend: "videotoolbox",
                    reason: "only H.264 is wired up".into(),
                }));
            }
            return Ok(Self::VideoToolbox(Box::new(videotoolbox::VideoToolboxEncoder::new(width, height)?)));
        }

        #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
        {
            return Ok(Self::Gpu(Box::new(gpu::GpuEncoder::new(width, height, codec)?)));
        }

        #[allow(unreachable_code)]
        Err(report!(VideoError::UnsupportedCodec {
            codec: codec.as_str(),
            backend: "gpu",
            reason: "no GPU backend compiled in".into(),
        }))
    }

    #[allow(unused_variables)]
    fn create_cpu(width: u32, height: u32, codec: VideoCodec) -> rootcause::Result<Self, VideoError> {
        match codec {
            #[cfg(feature = "cpu")]
            VideoCodec::H264 => Ok(Self::CpuH264(Box::new(cpu::CpuEncoder::new()?))),
            #[cfg(feature = "cpu-av1")]
            VideoCodec::Av1 => Ok(Self::CpuAv1(Box::new(cpu_av1::CpuAv1Encoder::new(width, height)?))),
            other => Err(report!(VideoError::UnsupportedCodec {
                codec: other.as_str(),
                backend: "cpu",
                reason: "no CPU encoder for this codec".into(),
            })),
        }
    }

    /// Encode one input frame. Returns zero or more output packets.
    ///
    /// H.264 and H.265 always return exactly one packet. AV1 (rav1e) may buffer
    /// frames internally and return zero packets for several initial calls; the
    /// muxer is expected to handle a stream of packets with explicit PTS rather
    /// than a 1:1 frame-to-packet mapping.
    #[allow(unused_variables)]
    pub fn encode_frame(
        &mut self,
        rgb: &[u8],
        width: u32,
        height: u32,
    ) -> rootcause::Result<Vec<EncodedFrame>, VideoError> {
        match self {
            #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
            Self::Gpu(enc) => Ok(vec![EncodedFrame::AnnexB(enc.encode_frame(rgb, width, height)?)]),
            #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
            Self::VideoToolbox(enc) => Ok(vec![EncodedFrame::AnnexB(enc.encode_frame(rgb, width, height)?)]),
            #[cfg(feature = "cpu")]
            Self::CpuH264(enc) => Ok(vec![EncodedFrame::AnnexB(
                enc.encode_frame(rgb, width as usize, height as usize)?,
            )]),
            #[cfg(feature = "cpu-av1")]
            Self::CpuAv1(enc) => {
                Ok(enc.encode_frame(rgb)?.into_iter().map(EncodedFrame::Av1Packet).collect())
            }
            #[cfg(not(any(
                feature = "cpu",
                feature = "cpu-av1",
                all(feature = "vulkan", not(target_os = "macos")),
                all(feature = "videotoolbox", target_os = "macos")
            )))]
            _ => unreachable!(),
        }
    }

    /// Drain any frames buffered by the encoder. Required for AV1 which
    /// holds frames before emitting output; no-op for H.264/H.265 paths.
    pub fn finish(&mut self) -> rootcause::Result<Vec<EncodedFrame>, VideoError> {
        match self {
            #[cfg(feature = "cpu-av1")]
            Self::CpuAv1(enc) => Ok(enc.flush()?.into_iter().map(EncodedFrame::Av1Packet).collect()),
            _ => Ok(Vec::new()),
        }
    }

    /// AV1 container sequence header (av1C box contents). None for non-AV1.
    pub fn av1_sequence_header(&self) -> Option<Vec<u8>> {
        match self {
            #[cfg(feature = "cpu-av1")]
            Self::CpuAv1(enc) => Some(enc.container_sequence_header()),
            _ => None,
        }
    }
}

pub enum EncodedFrame {
    /// H.264 or H.265 Annex B bitstream with start codes.
    AnnexB(Vec<u8>),
    /// AV1 OBU packet from rav1e, one entry per output frame.
    Av1Packet(Av1Packet),
}

#[cfg(feature = "cpu-av1")]
pub use cpu_av1::Av1Packet;

#[cfg(not(feature = "cpu-av1"))]
pub struct Av1Packet {
    pub data: Vec<u8>,
    pub input_frameno: u64,
    pub is_keyframe: bool,
}
