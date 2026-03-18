use std::fs::File;
use std::io::BufWriter;

use bytes::Bytes;
use rootcause::prelude::*;
use tracing::error;
use tracing::info;

use wows_replays::analyzer::battle_controller::listener::BattleControllerState;
use wows_replays::types::GameClock;

use crate::draw_command::RenderTarget;
use crate::drawing::ImageTarget;
use crate::encoder::EncoderBackend;
use crate::error::VideoError;
use crate::renderer::MinimapRenderer;

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

/// Check which encoder backends are available on this system.
///
/// This probes the GPU for video encoding support without actually
/// creating a full encoder. Useful for diagnostics and UI.
pub fn check_encoder() -> crate::encoder::EncoderStatus {
    crate::encoder::check_encoder()
}

// ---------------------------------------------------------------------------
// VideoEncoder (public API — unchanged from caller's perspective)
// ---------------------------------------------------------------------------

/// Handles H.264 encoding and MP4 muxing for the minimap renderer.
///
/// Encodes frames on-the-fly to avoid storing raw RGB data in memory.
/// Stores encoded H.264 Annex B NAL data per frame, then muxes to MP4 at the end.
///
/// Uses GPU acceleration by default (VideoToolbox on macOS, Vulkan Video on
/// Linux/Windows), falls back to CPU (openh264) if the `cpu` feature is enabled
/// and GPU is unavailable.
pub struct VideoEncoder {
    output_path: String,
    dump_mode: Option<DumpMode>,
    dump_all_mode: bool,
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
    /// Actual canvas dimensions (may include stats panel width).
    canvas_width: u32,
    canvas_height: u32,
}

impl VideoEncoder {
    /// Create a new video encoder.
    ///
    /// `match_time_limit` is the maximum match duration from replay metadata
    /// (e.g. 1200s for a 20-minute mode). The actual battle may end earlier.
    /// Call `set_battle_duration()` with the true end time for accurate
    /// progress reporting.
    pub fn new(
        output_path: &str,
        dump_mode: Option<DumpMode>,
        dump_all_mode: bool,
        match_time_limit: f32,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        let total_frames = (OUTPUT_DURATION * FPS) as usize;
        Self {
            output_path: output_path.to_string(),
            dump_mode,
            dump_all_mode,
            game_duration: match_time_limit,
            last_rendered_frame: -1,
            backend: None,
            h264_frames: Vec::with_capacity(total_frames),
            encoder_error: None,
            prefer_cpu: false,
            expected_frames: total_frames as u64,
            progress_callback: None,
            canvas_width,
            canvas_height,
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
        self.backend = Some(EncoderBackend::create(self.canvas_width, self.canvas_height, self.prefer_cpu)?);
        info!(
            frames = self.total_frames(),
            width = self.canvas_width,
            height = self.canvas_height,
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
        let encoded = backend.encode_frame(rgb_data, self.canvas_width, self.canvas_height)?;
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

            if self.dump_all_mode {
                target.begin_frame();
                for cmd in &commands {
                    target.draw(cmd);
                }
                target.end_frame();

                let png_path =
                    format!("{}{}{}.png", self.output_path, std::path::MAIN_SEPARATOR, self.last_rendered_frame);
                if let Err(e) = target.frame().save(&png_path) {
                    error!(error = %e, "Failed to save frame");
                    return;
                }

                if let Some(ref cb) = self.progress_callback {
                    cb(RenderProgress {
                        stage: RenderStage::Encoding,
                        current: (self.last_rendered_frame + 1) as u64,
                        total: self.expected_frames,
                    });
                }
            } else if let Some(ref dump_mode) = self.dump_mode {
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

        if self.dump_all_mode {
            // Dump the final frame (includes result overlay if winner is known)
            let commands = renderer.draw_frame(controller);
            target.begin_frame();
            for cmd in &commands {
                target.draw(cmd);
            }
            target.end_frame();

            let png_path = format!("{}{}{}.png", self.output_path, std::path::MAIN_SEPARATOR, self.last_rendered_frame);
            if let Err(e) = target.frame().save(&png_path) {
                error!(error = %e, "Failed to save frame");
            }

            return Ok(());
        }

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
                width: self.canvas_width as u16,
                height: self.canvas_height as u16,
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
