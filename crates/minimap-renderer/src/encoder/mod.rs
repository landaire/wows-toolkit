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
use tracing::info;

use crate::codec::EncoderKind;
use crate::codec::VideoCodec;
use crate::error::TargetSizeError;
use crate::error::VideoError;

/// Default target file size, in bytes. 10 MiB matches Discord's free-tier
/// upload cap, which is the most common destination for these clips. The
/// default bitrate is sized from this and the output video's length.
pub const DEFAULT_TARGET_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

/// The lowest bitrate the default and size-derived rates go down to, in bits
/// per second.
pub const MIN_BITRATE_BPS: u32 = 64_000;

/// The highest bitrate any export is encoded at, in bits per second. A clip of
/// a few frames would otherwise ask for more than an encoder accepts.
pub const MAX_BITRATE_BPS: u32 = 20_000_000;

/// Share of a target file size spent on the video stream; the rest covers
/// container overhead and encoder variance.
const TARGET_SIZE_USABLE: f64 = 0.92;

/// Default target bitrate, in bits per second, for a video of
/// `output_seconds`.
///
/// Sized for a video of the default length: about 79% of
/// [`DEFAULT_TARGET_FILE_SIZE_BYTES`] spread over 60 s (1.1 Mbit/s). The rest
/// covers container overhead and the Vulkan H.265 encoder's VBR overshoot (its
/// effective rate runs ~15% above target). One value covers both H.264 and
/// H.265 because picking the looser of the two safe targets keeps the clip
/// under 10 MiB for either codec. Other lengths share the same bit budget,
/// held within [`MIN_BITRATE_BPS`]..=[`MAX_BITRATE_BPS`]: an export longer
/// than about 17 minutes runs at the floor and so exceeds 10 MiB.
/// Callers prioritizing fidelity over file size should pass a higher
/// `target_bitrate_bps` override via [`EncoderConfig`].
pub fn default_bitrate_bps(output_seconds: f32) -> u32 {
    const BITS_PER_VIDEO: f64 = 66_000_000.0;

    (BITS_PER_VIDEO / f64::from(output_seconds.max(f32::MIN_POSITIVE)))
        .clamp(f64::from(MIN_BITRATE_BPS), f64::from(MAX_BITRATE_BPS)) as u32
}

/// rav1e quantizer (0-255, lower = higher quality). 60 produces visually
/// lossless output for HUD-heavy frames at moderate bitrates.
pub const DEFAULT_AV1_QUANTIZER: u8 = 60;

/// Encoder tunables, primarily bitrate-related. The optional fields fall back
/// to the backend's default when `None`.
#[derive(Debug, Clone, Copy)]
pub struct EncoderConfig {
    /// Target average bitrate in bits per second. Applies to H.264/H.265
    /// (VBR target) and is converted into a quantizer for AV1.
    pub target_bitrate_bps: Option<u32>,
    /// Peak bitrate for VBR rate control, in bits per second. Defaults to
    /// `2 * target_bitrate_bps` when unset.
    pub max_bitrate_bps: Option<u32>,
    /// AV1 quantizer override (0-255, lower = higher quality). Ignored when
    /// `target_bitrate_bps` is set; bitrate-based control wins.
    pub av1_quantizer: Option<u8>,
    /// Bitrate used when `target_bitrate_bps` is unset. The video encoder sizes
    /// it from the output length; elsewhere it is the default length's.
    default_bitrate_bps: u32,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl EncoderConfig {
    /// No overrides, with the default bitrate of a default-length export.
    pub fn new() -> Self {
        Self {
            target_bitrate_bps: None,
            max_bitrate_bps: None,
            av1_quantizer: None,
            default_bitrate_bps: default_bitrate_bps(crate::timing::VideoLength::DEFAULT.seconds()),
        }
    }

    /// A fixed target bitrate, with a peak of twice that.
    pub fn from_bitrate(bps: u32) -> Self {
        Self { target_bitrate_bps: Some(bps), max_bitrate_bps: Some(bps.saturating_mul(2)), ..Self::new() }
    }

    /// Pick a target bitrate that should land an encoded file under
    /// `target_size_bytes` for a clip of `duration_seconds`. Includes a small
    /// safety margin to account for container overhead and encoder variance.
    ///
    /// Fails when even [`MIN_BITRATE_BPS`] would exceed the target. A rate
    /// above [`MAX_BITRATE_BPS`] is capped there, which still meets it.
    pub fn from_target_size(target_size_bytes: u64, duration_seconds: f32) -> Result<Self, TargetSizeError> {
        let seconds = f64::from(duration_seconds.max(f32::MIN_POSITIVE));
        let bps = (target_size_bytes as f64) * TARGET_SIZE_USABLE * 8.0 / seconds;
        if bps < f64::from(MIN_BITRATE_BPS) {
            let minimum_bytes = (f64::from(MIN_BITRATE_BPS) * seconds / 8.0 / TARGET_SIZE_USABLE).ceil() as u64;
            return Err(TargetSizeError {
                requested_bytes: target_size_bytes,
                minimum_bytes,
                output_seconds: duration_seconds,
            });
        }

        Ok(Self::from_bitrate(bps.min(f64::from(MAX_BITRATE_BPS)) as u32))
    }

    /// These tunables with the default bitrate sized for a video of
    /// `output_seconds`.
    pub(crate) fn sized_for(self, output_seconds: f32) -> Self {
        Self { default_bitrate_bps: default_bitrate_bps(output_seconds), ..self }
    }

    /// Bitrate used when no target is set.
    pub fn default_bitrate_bps(&self) -> u32 {
        self.default_bitrate_bps
    }

    pub fn h264_bitrate_bps(&self) -> u32 {
        self.target_bitrate_bps.unwrap_or(self.default_bitrate_bps)
    }

    pub fn h265_bitrate_bps(&self) -> u32 {
        self.target_bitrate_bps.unwrap_or(self.default_bitrate_bps)
    }

    pub fn max_bitrate_for(&self, target: u32) -> u32 {
        self.max_bitrate_bps.unwrap_or_else(|| target.saturating_mul(2))
    }

    pub fn av1_quantizer(&self) -> u8 {
        self.av1_quantizer.unwrap_or(DEFAULT_AV1_QUANTIZER)
    }
}

#[cfg(all(feature = "vulkan", not(target_os = "macos")))]
pub mod gpu;

#[cfg(all(feature = "videotoolbox", target_os = "macos"))]
pub mod videotoolbox;

#[cfg(feature = "cpu")]
pub mod cpu;

#[cfg(feature = "cpu-av1")]
pub mod cpu_av1;

pub mod worker;

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
        VideoCodec::ALL
            .into_iter()
            .filter(|c| self.supports(EncoderKind::Gpu, *c) || self.supports(EncoderKind::Cpu, *c))
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
        status.cpu_codecs.insert(VideoCodec::Av1, CodecSupport::Unsupported("rav1e backend not compiled in".into()));
    }

    status.cpu_codecs.insert(VideoCodec::H265, CodecSupport::Unsupported("no CPU H.265 backend available".into()));

    #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
    {
        status.gpu_adapter_name = Some("VideoToolbox".to_string());
        status.gpu_codecs.insert(VideoCodec::H264, CodecSupport::Supported);
        status
            .gpu_codecs
            .insert(VideoCodec::H265, CodecSupport::Unsupported("VideoToolbox H.265 not yet wired up".into()));
        status.gpu_codecs.insert(VideoCodec::Av1, CodecSupport::Unsupported("no AV1 GPU encoder".into()));
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
            status.gpu_codecs.insert(codec, CodecSupport::Unsupported("GPU backend not compiled in".into()));
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
        config: EncoderConfig,
    ) -> rootcause::Result<CreatedEncoder, VideoError> {
        let status = check_encoder();

        if mode != Mode::ForceCpu && status.supports(EncoderKind::Gpu, codec) {
            match Self::create_gpu(width, height, codec, config) {
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

        let backend = Self::create_cpu(width, height, codec, config)?;
        info!(codec = %codec, "Using CPU encoder");
        Ok(CreatedEncoder { backend, codec, kind: EncoderKind::Cpu })
    }

    #[allow(unused_variables)]
    fn create_gpu(
        width: u32,
        height: u32,
        codec: VideoCodec,
        config: EncoderConfig,
    ) -> rootcause::Result<Self, VideoError> {
        #[cfg(all(feature = "videotoolbox", target_os = "macos"))]
        {
            if codec != VideoCodec::H264 {
                return Err(report!(VideoError::UnsupportedCodec {
                    codec: codec.as_str(),
                    backend: "videotoolbox",
                    reason: "only H.264 is wired up".into(),
                }));
            }
            return Ok(Self::VideoToolbox(Box::new(videotoolbox::VideoToolboxEncoder::new(width, height, config)?)));
        }

        #[cfg(all(feature = "vulkan", not(target_os = "macos")))]
        {
            return Ok(Self::Gpu(Box::new(gpu::GpuEncoder::new(width, height, codec, config)?)));
        }

        #[allow(unreachable_code)]
        Err(report!(VideoError::UnsupportedCodec {
            codec: codec.as_str(),
            backend: "gpu",
            reason: "no GPU backend compiled in".into(),
        }))
    }

    #[allow(unused_variables)]
    fn create_cpu(
        width: u32,
        height: u32,
        codec: VideoCodec,
        config: EncoderConfig,
    ) -> rootcause::Result<Self, VideoError> {
        match codec {
            #[cfg(feature = "cpu")]
            VideoCodec::H264 => Ok(Self::CpuH264(Box::new(cpu::CpuEncoder::new(config)?))),
            #[cfg(feature = "cpu-av1")]
            VideoCodec::Av1 => Ok(Self::CpuAv1(Box::new(cpu_av1::CpuAv1Encoder::new(width, height, config)?))),
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
            Self::CpuH264(enc) => {
                Ok(vec![EncodedFrame::AnnexB(enc.encode_frame(rgb, width as usize, height as usize)?)])
            }
            #[cfg(feature = "cpu-av1")]
            Self::CpuAv1(enc) => Ok(enc.encode_frame(rgb)?.into_iter().map(EncodedFrame::Av1Packet).collect()),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A shorter video gets a proportionally higher rate, so every default
    /// export lands at the same size.
    #[test]
    fn the_default_bitrate_follows_the_output_length() {
        assert_eq!(default_bitrate_bps(60.0), 1_100_000);
        assert_eq!(default_bitrate_bps(30.0), 2_200_000);
        assert_eq!(default_bitrate_bps(300.0), 220_000);
        assert_eq!(EncoderConfig::new().h264_bitrate_bps(), 1_100_000);
        assert_eq!(EncoderConfig::new().sized_for(120.0).h265_bitrate_bps(), 550_000);
        assert_eq!(EncoderConfig::from_bitrate(800_000).sized_for(120.0).h264_bitrate_bps(), 800_000);
    }

    #[test]
    fn a_target_size_sets_the_rate_for_the_output_length() {
        let ten_mib = DEFAULT_TARGET_FILE_SIZE_BYTES;
        let config = EncoderConfig::from_target_size(ten_mib, 60.0).expect("10 MiB holds a minute");

        assert_eq!(config.h264_bitrate_bps(), 1_286_253);
        assert_eq!(config.max_bitrate_for(config.h264_bitrate_bps()), 2_572_506);
    }

    /// One frame of video would otherwise ask for gigabits a second, past
    /// what an encoder's signed rate setting holds.
    #[test]
    fn a_target_size_rate_is_capped_at_the_ceiling() {
        let config = EncoderConfig::from_target_size(DEFAULT_TARGET_FILE_SIZE_BYTES, 1.0 / 30.0).expect("met");

        assert_eq!(config.h264_bitrate_bps(), MAX_BITRATE_BPS);
        assert!(i32::try_from(config.max_bitrate_for(MAX_BITRATE_BPS)).is_ok());
    }

    #[test]
    fn a_target_size_below_the_floor_is_refused() {
        let refused = EncoderConfig::from_target_size(DEFAULT_TARGET_FILE_SIZE_BYTES, 3600.0);

        assert_eq!(
            refused.map(|config| config.h264_bitrate_bps()),
            Err(TargetSizeError {
                requested_bytes: DEFAULT_TARGET_FILE_SIZE_BYTES,
                minimum_bytes: 31_304_348,
                output_seconds: 3600.0,
            })
        );
        assert!(EncoderConfig::from_target_size(31_304_348, 3600.0).is_ok(), "the minimum it names is met");
    }
}
