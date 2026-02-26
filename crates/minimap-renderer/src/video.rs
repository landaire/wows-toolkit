use std::fs::File;
use std::io::BufWriter;

use bytes::Bytes;
use rootcause::prelude::*;
use tracing::{error, info};

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::types::GameClock;

use crate::error::VideoError;

use crate::draw_command::RenderTarget;
use crate::drawing::ImageTarget;
use crate::renderer::MinimapRenderer;
use crate::{CANVAS_HEIGHT, MINIMAP_SIZE};

pub const FPS: f64 = 30.0;
/// Target output video duration in seconds. The game is compressed to fit this length.
pub const OUTPUT_DURATION: f64 = 60.0;

#[derive(Clone, Debug)]
pub enum DumpMode {
    Frame(usize),
    Midpoint,
    Last,
}

/// Which phase of video rendering is in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderStage {
    Encoding,
    Muxing,
}

/// Progress update emitted during video rendering.
#[derive(Clone, Debug)]
pub struct RenderProgress {
    pub stage: RenderStage,
    pub current: u64,
    pub total: u64,
}

// ---------------------------------------------------------------------------
// GPU backend (vk-video + yuvutils-rs)
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu")]
mod gpu {
    use std::num::NonZeroU32;

    use rootcause::prelude::*;
    use vk_video::parameters::{RateControl, VideoParameters};
    use vk_video::{BytesEncoder, Frame, RawFrameData, VulkanInstance};
    use yuvutils_rs::{BufferStoreMut, YuvBiPlanarImageMut, YuvConversionMode, YuvRange, YuvStandardMatrix};

    use super::FPS;
    use crate::error::VideoError;

    pub struct GpuEncoder {
        encoder: BytesEncoder,
        nv12_buf: Vec<u8>,
        frame_count: u64,
    }

    impl GpuEncoder {
        pub fn new(width: u32, height: u32) -> rootcause::Result<Self, VideoError> {
            let instance = VulkanInstance::new()
                .map_err(|e| report!(VideoError::EncoderInit(format!("Vulkan init failed: {e:?}"))))?;
            let adapter = instance
                .create_adapter(None)
                .map_err(|e| report!(VideoError::EncoderInit(format!("No Vulkan adapter: {e:?}"))))?;

            if !adapter.supports_encoding() {
                bail!(VideoError::EncoderInit(format!(
                    "Vulkan adapter '{}' does not support video encoding",
                    adapter.info().name
                )));
            }

            let device = adapter
                .create_device(
                    wgpu::Features::empty(),
                    wgpu::ExperimentalFeatures::disabled(),
                    wgpu::Limits { max_immediate_size: 128, ..Default::default() },
                )
                .map_err(|e| report!(VideoError::EncoderInit(format!("Vulkan device creation failed: {e:?}"))))?;

            let params = device
                .encoder_parameters_high_quality(
                    VideoParameters {
                        width: NonZeroU32::new(width).expect("non-zero width"),
                        height: NonZeroU32::new(height).expect("non-zero height"),
                        target_framerate: (FPS as u32).into(),
                    },
                    RateControl::VariableBitrate {
                        average_bitrate: 20_000_000,
                        max_bitrate: 40_000_000,
                        virtual_buffer_size: std::time::Duration::from_secs(2),
                    },
                )
                .map_err(|e| report!(VideoError::EncoderInit(format!("Encoder params failed: {e:?}"))))?;

            let encoder = device
                .create_bytes_encoder(params)
                .map_err(|e| report!(VideoError::EncoderInit(format!("Encoder creation failed: {e:?}"))))?;

            let nv12_size = (width as usize) * (height as usize) * 3 / 2;

            Ok(Self { encoder, nv12_buf: vec![0u8; nv12_size], frame_count: 0 })
        }

        pub fn encode_frame(&mut self, rgb: &[u8], width: u32, height: u32) -> rootcause::Result<Vec<u8>, VideoError> {
            let y_len = (width * height) as usize;
            let uv_len = (width * height / 2) as usize;

            // Split nv12_buf into Y and UV planes
            let (y_plane, uv_plane) = self.nv12_buf[..y_len + uv_len].split_at_mut(y_len);

            let mut nv12_image = YuvBiPlanarImageMut {
                y_plane: BufferStoreMut::Borrowed(y_plane),
                y_stride: width,
                uv_plane: BufferStoreMut::Borrowed(uv_plane),
                uv_stride: width,
                width,
                height,
            };

            yuvutils_rs::rgb_to_yuv_nv12(
                &mut nv12_image,
                rgb,
                width * 3,
                YuvRange::Full,
                YuvStandardMatrix::Bt709,
                YuvConversionMode::Balanced,
            )
            .map_err(|e| report!(VideoError::EncodeFailed(format!("RGB→NV12 conversion failed: {e:?}"))))?;

            let force_keyframe = self.frame_count == 0;
            let frame = Frame {
                data: RawFrameData { frame: self.nv12_buf.clone(), width, height },
                pts: Some(self.frame_count),
            };

            let output = self
                .encoder
                .encode(&frame, force_keyframe)
                .map_err(|e| report!(VideoError::EncodeFailed(format!("GPU encode failed: {e:?}"))))?;

            self.frame_count += 1;
            Ok(output.data)
        }
    }
}

// ---------------------------------------------------------------------------
// CPU backend (openh264)
// ---------------------------------------------------------------------------

#[cfg(feature = "cpu")]
mod cpu {
    use openh264::OpenH264API;
    use openh264::encoder::{BitRate, Complexity, Encoder, EncoderConfig, FrameRate};
    use openh264::formats::{RgbSliceU8, YUVBuffer};
    use rootcause::prelude::*;

    use super::FPS;
    use crate::error::VideoError;

    pub struct CpuEncoder {
        encoder: Encoder,
    }

    impl CpuEncoder {
        pub fn new() -> rootcause::Result<Self, VideoError> {
            let config = EncoderConfig::new()
                .max_frame_rate(FrameRate::from_hz(FPS as f32))
                .rate_control_mode(openh264::encoder::RateControlMode::Quality)
                .bitrate(BitRate::from_bps(20_000_000))
                .complexity(Complexity::High);
            let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
                .map_err(|e| report!(VideoError::EncoderInit(format!("Failed to create H.264 encoder: {e:?}"))))?;
            Ok(Self { encoder })
        }

        pub fn encode_frame(
            &mut self,
            rgb: &[u8],
            width: usize,
            height: usize,
        ) -> rootcause::Result<Vec<u8>, VideoError> {
            let rgb_slice = RgbSliceU8::new(rgb, (width, height));
            let yuv = YUVBuffer::from_rgb_source(rgb_slice);
            let bitstream = self
                .encoder
                .encode(&yuv)
                .map_err(|e| report!(VideoError::EncodeFailed(format!("H.264 encode error: {e:?}"))))?;
            Ok(bitstream.to_vec())
        }
    }
}

// ---------------------------------------------------------------------------
// Encoder availability check
// ---------------------------------------------------------------------------

/// Result of checking encoder availability.
#[derive(Debug)]
pub struct EncoderStatus {
    pub gpu_available: bool,
    pub gpu_error: Option<String>,
    pub gpu_adapter_name: Option<String>,
    pub cpu_available: bool,
}

impl std::fmt::Display for EncoderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Encoder status:")?;
        if self.gpu_available {
            writeln!(f, "  GPU: available ({})", self.gpu_adapter_name.as_deref().unwrap_or("unknown"))?;
        } else if let Some(ref err) = self.gpu_error {
            writeln!(f, "  GPU: unavailable - {err}")?;
        } else {
            writeln!(f, "  GPU: not compiled in (enable 'gpu' feature)")?;
        }
        if self.cpu_available {
            writeln!(f, "  CPU: available (openh264)")?;
        } else {
            writeln!(f, "  CPU: not compiled in (enable 'cpu' feature)")?;
        }
        Ok(())
    }
}

/// Check which encoder backends are available on this system.
///
/// This probes the GPU for Vulkan Video encoding support without actually
/// creating a full encoder. Useful for diagnostics and UI.
pub fn check_encoder() -> EncoderStatus {
    let mut status = EncoderStatus {
        gpu_available: false,
        gpu_error: None,
        gpu_adapter_name: None,
        cpu_available: cfg!(feature = "cpu"),
    };

    #[cfg(feature = "gpu")]
    {
        use vk_video::VulkanInstance;
        match VulkanInstance::new() {
            Err(e) => {
                status.gpu_error = Some(format!("Vulkan init failed: {e:?}"));
            }
            Ok(instance) => match instance.create_adapter(None) {
                Err(e) => {
                    status.gpu_error = Some(format!("No Vulkan adapter: {e:?}"));
                }
                Ok(adapter) => {
                    let name = adapter.info().name.clone();
                    status.gpu_adapter_name = Some(name.clone());
                    if adapter.supports_encoding() {
                        status.gpu_available = true;
                    } else {
                        status.gpu_error = Some(format!("Vulkan adapter '{name}' does not support video encoding"));
                    }
                }
            },
        }
    }

    #[cfg(not(feature = "gpu"))]
    {
        status.gpu_error = Some("GPU feature not compiled in".to_string());
    }

    status
}

// ---------------------------------------------------------------------------
// Encoder backend dispatch
// ---------------------------------------------------------------------------

enum EncoderBackend {
    #[cfg(feature = "gpu")]
    Gpu(Box<gpu::GpuEncoder>),
    #[cfg(feature = "cpu")]
    Cpu(Box<cpu::CpuEncoder>),
}

impl EncoderBackend {
    fn create(_width: u32, _height: u32, prefer_cpu: bool) -> rootcause::Result<Self, VideoError> {
        let _ = prefer_cpu; // suppress unused warning when neither feature is enabled

        // GPU preferred (default): try GPU, fail if unavailable
        #[cfg(feature = "gpu")]
        if !prefer_cpu {
            match gpu::GpuEncoder::new(_width, _height) {
                Ok(enc) => {
                    info!("Using GPU (Vulkan Video) encoder");
                    return Ok(Self::Gpu(Box::new(enc)));
                }
                Err(e) => {
                    return Err(e.attach("GPU encoder failed. Enable prefer_cpu to use the CPU encoder instead."));
                }
            }
        }

        // CPU explicitly requested via prefer_cpu
        #[cfg(feature = "cpu")]
        {
            info!("Using CPU (openh264) encoder");
            Ok(Self::Cpu(Box::new(cpu::CpuEncoder::new()?)))
        }

        #[cfg(not(feature = "cpu"))]
        {
            bail!(VideoError::EncoderInit("CPU encoder requested but 'cpu' feature is not enabled".into()));
        }
    }

    fn encode_frame(&mut self, rgb: &[u8], width: u32, height: u32) -> rootcause::Result<Vec<u8>, VideoError> {
        match self {
            #[cfg(feature = "gpu")]
            Self::Gpu(enc) => enc.encode_frame(rgb, width, height),
            #[cfg(feature = "cpu")]
            Self::Cpu(enc) => enc.encode_frame(rgb, width as usize, height as usize),
        }
    }
}

// ---------------------------------------------------------------------------
// VideoEncoder (public API — unchanged from caller's perspective)
// ---------------------------------------------------------------------------

/// Handles H.264 encoding and MP4 muxing for the minimap renderer.
///
/// Encodes frames on-the-fly to avoid storing raw RGB data in memory.
/// Stores encoded H.264 Annex B NAL data per frame, then muxes to MP4 at the end.
///
/// Uses GPU (vk-video) by default, falls back to CPU (openh264) if the `cpu`
/// feature is enabled and GPU is unavailable.
pub struct VideoEncoder {
    output_path: String,
    dump_mode: Option<DumpMode>,
    game_duration: f32,
    last_rendered_frame: i64,
    backend: Option<EncoderBackend>,
    h264_frames: Vec<Vec<u8>>,
    /// Stored fatal encoder error. Once set, `advance_clock` is a no-op and
    /// the error is surfaced in `finish()` / `mux_to_mp4()` with full context.
    encoder_error: Option<String>,
    /// When true, skip the GPU encoder and use CPU (openh264) directly.
    prefer_cpu: bool,
    /// Expected number of frames to render, for progress reporting only.
    /// Defaults to `total_frames()` (1800). Updated by `set_battle_duration()`
    /// when the actual game length is shorter than `game_duration`.
    expected_frames: u64,
    /// Optional callback invoked after each frame is encoded or muxed.
    progress_callback: Option<Box<dyn Fn(RenderProgress)>>,
}

impl VideoEncoder {
    /// Create a new video encoder.
    ///
    /// `match_time_limit` is the maximum match duration from replay metadata
    /// (e.g. 1200s for a 20-minute mode). The actual battle may end earlier.
    /// Call `set_battle_duration()` with the true end time for accurate
    /// progress reporting.
    pub fn new(output_path: &str, dump_mode: Option<DumpMode>, match_time_limit: f32) -> Self {
        let total_frames = (OUTPUT_DURATION * FPS) as usize;
        Self {
            output_path: output_path.to_string(),
            dump_mode,
            game_duration: match_time_limit,
            last_rendered_frame: -1,
            backend: None,
            h264_frames: Vec::with_capacity(total_frames),
            encoder_error: None,
            prefer_cpu: false,
            expected_frames: total_frames as u64,
            progress_callback: None,
        }
    }

    /// Skip the GPU encoder and use CPU (openh264) directly.
    /// Only effective if the `cpu` feature is enabled.
    pub fn set_prefer_cpu(&mut self, prefer: bool) {
        self.prefer_cpu = prefer;
    }

    /// Set the actual battle duration for accurate progress reporting.
    ///
    /// When the constructor receives `meta.duration` (the match time limit,
    /// e.g. 1200s) but the battle ends earlier (e.g. 660s), fewer than
    /// `total_frames()` frames are rendered. Call this with the true battle
    /// duration so the progress callback reports the correct total.
    ///
    /// If the constructor already received the actual battle duration (not the
    /// time limit), there is no need to call this — all `total_frames()` frames
    /// will be rendered and the default total is correct.
    pub fn set_battle_duration(&mut self, duration: GameClock) {
        let total_frames = self.total_frames();
        let frame_duration = self.game_duration / total_frames as f32;
        self.expected_frames = (duration.seconds() / frame_duration) as u64;
    }

    /// Set a callback that receives progress updates during encoding and muxing.
    ///
    /// The callback uses `Fn` (not `FnMut`) so it works naturally with channel
    /// senders: `encoder.set_progress_callback(move |p| tx.send(p).ok());`
    pub fn set_progress_callback<F: Fn(RenderProgress) + 'static>(&mut self, callback: F) {
        self.progress_callback = Some(Box::new(callback));
    }

    /// Total output frames (fixed output duration * FPS).
    fn total_frames(&self) -> i64 {
        (OUTPUT_DURATION * FPS) as i64
    }

    /// Initialize the encoder backend eagerly.
    ///
    /// Normally the backend is created lazily on the first frame. Call this
    /// before the render loop to ensure any startup logging happens before
    /// a progress bar is displayed.
    pub fn init(&mut self) -> rootcause::Result<(), VideoError> {
        self.ensure_encoder()
    }

    /// Create the encoder backend on first use (no-op if already initialized).
    fn ensure_encoder(&mut self) -> rootcause::Result<(), VideoError> {
        if self.backend.is_some() {
            return Ok(());
        }
        self.backend = Some(EncoderBackend::create(MINIMAP_SIZE, CANVAS_HEIGHT, self.prefer_cpu)?);
        info!(
            frames = self.total_frames(),
            width = MINIMAP_SIZE,
            height = CANVAS_HEIGHT,
            duration = self.game_duration,
            fps = FPS,
            "Rendering"
        );
        Ok(())
    }

    /// Encode a rendered frame to H.264 immediately.
    fn encode_frame(&mut self, target: &ImageTarget) -> rootcause::Result<(), VideoError> {
        let backend =
            self.backend.as_mut().ok_or_else(|| report!(VideoError::EncodeFailed("Encoder not initialized".into())))?;
        let frame_image = target.frame();
        let rgb_data = frame_image.as_raw();
        let encoded = backend.encode_frame(rgb_data, MINIMAP_SIZE, CANVAS_HEIGHT)?;
        self.h264_frames.push(encoded);
        Ok(())
    }

    /// Called before each packet is processed by the controller.
    ///
    /// If the new clock has crossed one or more frame boundaries, renders
    /// frames from the controller's current state (which reflects all
    /// packets up to but not including this one).
    pub fn advance_clock(
        &mut self,
        new_clock: GameClock,
        controller: &dyn BattleControllerState,
        renderer: &mut MinimapRenderer,
        target: &mut ImageTarget,
    ) {
        if self.game_duration <= 0.0 || self.encoder_error.is_some() {
            return;
        }

        let total_frames = self.total_frames();
        let frame_duration = self.game_duration / total_frames as f32;
        let target_frame = (new_clock.seconds() / frame_duration) as i64;

        while self.last_rendered_frame < target_frame {
            self.last_rendered_frame += 1;

            // Populate player data (idempotent, runs once)
            renderer.populate_players(controller);
            // Update squadron info for any new planes
            renderer.update_squadron_info(controller);

            let commands = renderer.draw_frame(controller);

            if let Some(ref dump_mode) = self.dump_mode {
                let dump_frame = match dump_mode {
                    DumpMode::Frame(n) => *n as i64,
                    DumpMode::Midpoint => total_frames / 2,
                    DumpMode::Last => -1, // handled in finish()
                };
                if dump_frame >= 0 && self.last_rendered_frame == dump_frame {
                    target.begin_frame();
                    for cmd in &commands {
                        target.draw(cmd);
                    }
                    target.end_frame();

                    let png_path = self.output_path.replace(".mp4", ".png");
                    let png_path =
                        if png_path == self.output_path { format!("{}.png", self.output_path) } else { png_path };
                    if let Err(e) = target.frame().save(&png_path) {
                        error!(error = %e, "Failed to save frame");
                    } else {
                        let (w, h) = target.canvas_size();
                        info!(frame = dump_frame, path = %png_path, width = w, height = h, "Frame saved");
                    }
                }
            } else {
                // Full video mode: render, encode to H.264 immediately
                if let Err(e) = self.ensure_encoder() {
                    error!(error = %e, "Encoder initialization failed");
                    self.encoder_error = Some(format!("{e}"));
                    return;
                }

                target.begin_frame();
                for cmd in &commands {
                    target.draw(cmd);
                }
                target.end_frame();

                if let Err(e) = self.encode_frame(target) {
                    error!(error = %e, "Frame encoding failed");
                    self.encoder_error = Some(format!("{e}"));
                    return;
                }

                if let Some(ref cb) = self.progress_callback {
                    cb(RenderProgress {
                        stage: RenderStage::Encoding,
                        current: (self.last_rendered_frame + 1) as u64,
                        total: self.expected_frames,
                    });
                }
            }
        }
    }

    /// Finalize: flush any remaining frames and write the video file.
    pub fn finish(
        &mut self,
        controller: &dyn BattleControllerState,
        renderer: &mut MinimapRenderer,
        target: &mut ImageTarget,
    ) -> rootcause::Result<(), VideoError> {
        // Render up to the actual battle end (or last packet), not meta.duration.
        let end_clock = controller.battle_end_clock().unwrap_or(controller.clock());
        // Extend game_duration if the battle actually ran longer than meta.duration
        // (e.g. battleResult arrives a few seconds after the nominal duration).
        if end_clock.seconds() > self.game_duration {
            self.game_duration = end_clock.seconds();
        }

        self.advance_clock(end_clock, controller, renderer, target);

        if let Some(ref dump_mode) = self.dump_mode {
            if matches!(dump_mode, DumpMode::Last) {
                // Dump the final frame (includes result overlay if winner is known)
                let commands = renderer.draw_frame(controller);
                target.begin_frame();
                for cmd in &commands {
                    target.draw(cmd);
                }
                target.end_frame();

                let png_path = self.output_path.replace(".mp4", ".png");
                let png_path =
                    if png_path == self.output_path { format!("{}.png", self.output_path) } else { png_path };
                if let Err(e) = target.frame().save(&png_path) {
                    error!(error = %e, "Failed to save frame");
                } else {
                    let (w, h) = target.canvas_size();
                    info!(path = %png_path, width = w, height = h, "Result frame saved");
                }
            }
            return Ok(());
        }

        // Mux the already-encoded H.264 frames into MP4
        self.mux_to_mp4()
    }

    /// Mux pre-encoded H.264 Annex B frames into an MP4 file.
    fn mux_to_mp4(&self) -> rootcause::Result<(), VideoError> {
        if self.h264_frames.is_empty() {
            if let Some(ref err) = self.encoder_error {
                bail!(VideoError::MuxFailed(format!("No frames were encoded. Encoder failed earlier: {err}")));
            }
            bail!(VideoError::MuxFailed("No frames to mux".into()));
        }

        // Extract SPS and PPS from the first keyframe
        let first_frame = &self.h264_frames[0];
        let nals = parse_annexb_nals(first_frame);
        let sps = nals
            .iter()
            .find(|n| (n[0] & 0x1f) == 7)
            .ok_or_else(|| report!(VideoError::MuxFailed("No SPS found in first frame".into())))?;
        let pps = nals
            .iter()
            .find(|n| (n[0] & 0x1f) == 8)
            .ok_or_else(|| report!(VideoError::MuxFailed("No PPS found in first frame".into())))?;

        // Setup MP4 writer
        let mp4_config = mp4::Mp4Config {
            major_brand: str::parse("isom").unwrap(),
            minor_version: 512,
            compatible_brands: vec![
                str::parse("isom").unwrap(),
                str::parse("iso2").unwrap(),
                str::parse("avc1").unwrap(),
                str::parse("mp41").unwrap(),
            ],
            timescale: 1000,
        };

        let file = File::create(&self.output_path).context_transform(VideoError::Io)?;
        let writer = BufWriter::new(file);
        let mut mp4_writer = mp4::Mp4Writer::write_start(writer, &mp4_config)
            .map_err(|e| report!(VideoError::MuxFailed(format!("{e:?}"))))?;

        let track_config = mp4::TrackConfig {
            track_type: mp4::TrackType::Video,
            timescale: 1000,
            language: "und".to_string(),
            media_conf: mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
                width: MINIMAP_SIZE as u16,
                height: CANVAS_HEIGHT as u16,
                seq_param_set: sps.to_vec(),
                pic_param_set: pps.to_vec(),
            }),
        };
        mp4_writer.add_track(&track_config).map_err(|e| report!(VideoError::MuxFailed(format!("{e:?}"))))?;

        let sample_duration = 1000 / FPS as u32;
        let total_mux_frames = self.h264_frames.len() as u64;

        for (frame_idx, annexb_data) in self.h264_frames.iter().enumerate() {
            if annexb_data.is_empty() {
                continue;
            }
            let nals = parse_annexb_nals(annexb_data);
            let is_sync = nals.iter().any(|n| (n[0] & 0x1f) == 5);

            let mut avcc_data = Vec::new();
            for nal in &nals {
                let nal_type = nal[0] & 0x1f;
                if nal_type == 7 || nal_type == 8 {
                    continue;
                }
                let len = nal.len() as u32;
                avcc_data.extend_from_slice(&len.to_be_bytes());
                avcc_data.extend_from_slice(nal);
            }

            if avcc_data.is_empty() {
                continue;
            }

            let sample = mp4::Mp4Sample {
                start_time: frame_idx as u64 * sample_duration as u64,
                duration: sample_duration,
                rendering_offset: 0,
                is_sync,
                bytes: Bytes::from(avcc_data),
            };
            mp4_writer.write_sample(1, &sample).map_err(|e| report!(VideoError::MuxFailed(format!("{e:?}"))))?;

            if let Some(ref cb) = self.progress_callback {
                cb(RenderProgress {
                    stage: RenderStage::Muxing,
                    current: (frame_idx + 1) as u64,
                    total: total_mux_frames,
                });
            }
        }

        mp4_writer.write_end().map_err(|e| report!(VideoError::MuxFailed(format!("{e:?}"))))?;
        info!(path = %self.output_path, "Video saved");
        Ok(())
    }
}

/// Parse Annex B byte stream into individual NAL units (without start codes).
fn parse_annexb_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if i + 2 < data.len() && data[i] == 0 && data[i + 1] == 0 {
            let (start, _) = if i + 3 < data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                (i + 4, 4)
            } else if data[i + 2] == 1 {
                (i + 3, 3)
            } else {
                i += 1;
                continue;
            };
            let mut end = start;
            while end < data.len() {
                if end + 2 < data.len()
                    && data[end] == 0
                    && data[end + 1] == 0
                    && (data[end + 2] == 1 || (end + 3 < data.len() && data[end + 2] == 0 && data[end + 3] == 1))
                {
                    break;
                }
                end += 1;
            }
            if end > start {
                nals.push(&data[start..end]);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    nals
}
