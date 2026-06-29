use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::io::stdout;

use image::codecs::png::PngEncoder;
use muxide::api::MuxerBuilder;
use muxide::api::VideoCodec as MuxideCodec;
use rootcause::prelude::*;
use tracing::error;
use tracing::info;

use wows_battle_world::view::BattleView;
use wows_replays::types::GameClock;

use crate::codec::VideoCodec;
use crate::draw_command::RenderTarget;
use crate::drawing::ImageTarget;
use crate::encoder::Mode;
use crate::encoder::worker::EncodedSample;
use crate::encoder::worker::EncoderOutput;
use crate::encoder::worker::EncoderWorker;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderStage {
    Encoding,
    Muxing,
}

#[derive(Clone, Debug)]
pub struct RenderProgress {
    pub stage: RenderStage,
    pub current: u64,
    pub total: u64,
}

pub fn check_encoder() -> crate::encoder::EncoderStatus {
    crate::encoder::check_encoder()
}

/// Codec selection passed to `VideoEncoder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CodecChoice {
    /// Pick the best codec for the prevailing mode at init time.
    #[default]
    Auto,
    /// Use the named codec.
    Explicit(VideoCodec),
}

pub struct VideoEncoder {
    output_path: Option<String>,
    dump_mode: Option<DumpMode>,
    dump_all_mode: bool,
    game_duration: f32,
    /// Clock at which the output video begins. Packets before this are processed
    /// into controller state but produce no frames, so the rendered window is
    /// `[start_clock, game_duration]`. Defaults to the replay start (no skip).
    start_clock: GameClock,
    last_rendered_frame: i64,
    worker: Option<EncoderWorker>,
    encoder_error: Option<rootcause::Report<VideoError>>,
    codec_choice: CodecChoice,
    mode: Mode,
    encoder_config: crate::encoder::EncoderConfig,
    /// Codec resolved at encoder-init time; `None` until init().
    active_codec: Option<VideoCodec>,
    expected_frames: u64,
    progress_callback: Option<Box<dyn Fn(RenderProgress)>>,
    canvas_width: u32,
    canvas_height: u32,
}

impl VideoEncoder {
    pub fn new(
        output_path: Option<&str>,
        dump_mode: Option<DumpMode>,
        dump_all_mode: bool,
        match_time_limit: f32,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        let total_frames = (OUTPUT_DURATION * FPS) as usize;
        Self {
            output_path: output_path.map(String::from),
            dump_mode,
            dump_all_mode,
            game_duration: match_time_limit,
            start_clock: GameClock(0.0),
            last_rendered_frame: -1,
            worker: None,
            encoder_error: None,
            codec_choice: CodecChoice::Auto,
            mode: Mode::Auto,
            encoder_config: crate::encoder::EncoderConfig::default(),
            active_codec: None,
            expected_frames: total_frames as u64,
            progress_callback: None,
            canvas_width,
            canvas_height,
        }
    }

    /// Force the CPU encoder (Mode::ForceCpu). When combined with a CodecChoice
    /// the codec must have a CPU implementation or `init()` returns an error.
    pub fn set_prefer_cpu(&mut self, prefer: bool) {
        self.mode = if prefer { Mode::ForceCpu } else { Mode::Auto };
    }

    /// Set the codec selection strategy. With `CodecChoice::Auto`, init picks the
    /// best codec consistent with the current `Mode` (CPU vs GPU preference).
    pub fn set_codec(&mut self, codec: CodecChoice) {
        self.codec_choice = codec;
    }

    /// Set the encoder mode directly. Useful for callers that need
    /// `Mode::ForceGpu` to bubble up an error if GPU encode is unavailable.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Set encoder tunables (bitrate, AV1 quantizer). Must be called before
    /// `init()`/the first frame submission to take effect.
    pub fn set_encoder_config(&mut self, config: crate::encoder::EncoderConfig) {
        self.encoder_config = config;
    }

    /// Convenience: pick a target bitrate that should keep the encoded file
    /// under `target_size_bytes`. Sizing is based on `OUTPUT_DURATION` (the
    /// renderer's hard cap on output video length), not the match's time
    /// limit, so the chosen bitrate is the worst-case rate that still hits
    /// the target even for a maximum-length clip.
    pub fn target_max_file_size(&mut self, target_size_bytes: u64) {
        self.encoder_config =
            crate::encoder::EncoderConfig::from_target_size(target_size_bytes, OUTPUT_DURATION as f32);
    }

    /// Begin the output video at `start`, skipping everything before it. Packets
    /// earlier than `start` still build up controller state but emit no frames.
    /// Pass `GameClock(0.0)` to render the full replay including the pre-battle
    /// phase.
    pub fn set_render_start(&mut self, start: GameClock) {
        self.start_clock = start;
    }

    /// Seconds of replay time the output window spans, i.e. from the render
    /// start to the end of the match.
    fn window_duration(&self) -> f32 {
        (self.game_duration - self.start_clock.seconds()).max(0.0)
    }

    pub fn set_battle_duration(&mut self, duration: GameClock) {
        let total_frames = self.total_frames();
        let frame_duration = self.window_duration() / total_frames as f32;
        if frame_duration <= 0.0 {
            return;
        }
        let window_end = (duration.seconds() - self.start_clock.seconds()).max(0.0);
        self.expected_frames = (window_end / frame_duration) as u64;
    }

    pub fn set_progress_callback<F: Fn(RenderProgress) + 'static>(&mut self, callback: F) {
        self.progress_callback = Some(Box::new(callback));
    }

    fn total_frames(&self) -> i64 {
        (OUTPUT_DURATION * FPS) as i64
    }

    pub fn init(&mut self) -> rootcause::Result<(), VideoError> {
        self.ensure_encoder()
    }

    fn ensure_encoder(&mut self) -> rootcause::Result<(), VideoError> {
        if self.worker.is_some() {
            return Ok(());
        }
        let status = crate::encoder::check_encoder();
        let codec = match self.codec_choice {
            CodecChoice::Explicit(c) => c,
            CodecChoice::Auto => status.best_codec(matches!(self.mode, Mode::ForceCpu)),
        };
        let (worker, resolved_codec, kind) =
            EncoderWorker::spawn(self.canvas_width, self.canvas_height, codec, self.mode, self.encoder_config)?;
        self.active_codec = Some(resolved_codec);
        self.worker = Some(worker);
        info!(
            frames = self.total_frames(),
            width = self.canvas_width,
            height = self.canvas_height,
            duration = self.game_duration,
            codec = %resolved_codec,
            kind = %kind,
            fps = FPS,
            "Rendering"
        );
        Ok(())
    }

    pub fn advance_clock(
        &mut self,
        new_clock: GameClock,
        controller: &BattleView<'_>,
        renderer: &mut MinimapRenderer,
        target: &mut ImageTarget,
    ) {
        if self.window_duration() <= 0.0 || self.encoder_error.is_some() {
            return;
        }

        // Clocks before the render start are pre-battle; skip them entirely so
        // frame 0 lands at the battle-start clock.
        if new_clock.seconds() < self.start_clock.seconds() {
            return;
        }

        let total_frames = self.total_frames();
        let frame_duration = self.window_duration() / total_frames as f32;
        let target_frame = ((new_clock.seconds() - self.start_clock.seconds()) / frame_duration) as i64;

        let mut writer = BufWriter::new(stdout().lock());

        while self.last_rendered_frame < target_frame {
            self.last_rendered_frame += 1;

            // Continuous per-frame time so positions interpolate and tracers /
            // torpedoes (paced by the render clock) animate between packets.
            let frame_seconds = self.start_clock.seconds() + self.last_rendered_frame as f32 * frame_duration;
            renderer.set_render_clock(GameClock(frame_seconds));

            renderer.populate_players(controller);
            renderer.update_squadron_info(controller);

            let commands = renderer.draw_frame(controller);

            if self.dump_all_mode {
                target.begin_frame();
                for cmd in &commands {
                    target.draw(cmd);
                }
                target.end_frame();

                match &self.output_path {
                    None => {
                        let encoder = PngEncoder::new(&mut writer);
                        if let Err(e) = target.frame().write_with_encoder(encoder) {
                            error!(error = %e, "Failed to write frame to stdout");
                            return;
                        }
                    }
                    Some(path) => {
                        let png_path = format!("{}{}{}.png", path, std::path::MAIN_SEPARATOR, self.last_rendered_frame);
                        if let Err(e) = target.frame().save(&png_path) {
                            error!(error = %e, "Failed to save frame");
                            return;
                        }
                    }
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
                    DumpMode::Last => -1,
                };
                if dump_frame >= 0 && self.last_rendered_frame == dump_frame {
                    target.begin_frame();
                    for cmd in &commands {
                        target.draw(cmd);
                    }
                    target.end_frame();

                    match &self.output_path {
                        None => {
                            let encoder = PngEncoder::new(&mut writer);
                            if let Err(e) = target.frame().write_with_encoder(encoder) {
                                error!(error = %e, "Failed to write frame to stdout");
                                return;
                            }
                        }
                        Some(path) => {
                            let png_path = path.replace(".mp4", ".png");
                            let png_path = if png_path == *path { format!("{}.png", path) } else { png_path };
                            if let Err(e) = target.frame().save(&png_path) {
                                error!(error = %e, "Failed to save frame");
                            } else {
                                let (w, h) = target.canvas_size();
                                info!(frame = dump_frame, path = %png_path, width = w, height = h, "Frame saved");
                            }
                        }
                    }
                }
            } else {
                if let Err(e) = self.ensure_encoder() {
                    error!(error = %e, "Encoder initialization failed");
                    self.encoder_error = Some(e);
                    return;
                }

                target.begin_frame();
                for cmd in &commands {
                    target.draw(cmd);
                }
                target.end_frame();

                let frame = target.frame().as_raw().to_vec();
                let worker = self.worker.as_ref().expect("worker is Some after ensure_encoder succeeded");
                if let Err(e) = worker.submit(frame) {
                    error!(error = %e, "Frame submission failed");
                    self.encoder_error = Some(e);
                    return;
                }

                if let Some(ref cb) = self.progress_callback {
                    let encoded = self.worker.as_ref().map(|w| w.encoded_count()).unwrap_or(0);
                    cb(RenderProgress { stage: RenderStage::Encoding, current: encoded, total: self.expected_frames });
                }
            }
        }

        writer.flush().expect("flushing output to stdout failed");
    }

    pub fn finish(
        &mut self,
        controller: &BattleView<'_>,
        renderer: &mut MinimapRenderer,
        target: &mut ImageTarget,
    ) -> rootcause::Result<(), VideoError> {
        let end_clock = controller.battle_end_clock().unwrap_or(controller.clock());
        if end_clock.seconds() > self.game_duration {
            self.game_duration = end_clock.seconds();
        }

        self.advance_clock(end_clock, controller, renderer, target);

        let mut writer = BufWriter::new(stdout().lock());

        if self.dump_all_mode {
            let commands = renderer.draw_frame(controller);
            target.begin_frame();
            for cmd in &commands {
                target.draw(cmd);
            }
            target.end_frame();

            match &self.output_path {
                None => {
                    let encoder = PngEncoder::new(&mut writer);
                    if let Err(e) = target.frame().write_with_encoder(encoder) {
                        error!(error = %e, "Failed to write frame to stdout");
                    }
                }
                Some(path) => {
                    let png_path = format!("{}{}{}.png", path, std::path::MAIN_SEPARATOR, self.last_rendered_frame);
                    if let Err(e) = target.frame().save(&png_path) {
                        error!(error = %e, "Failed to save frame");
                    }
                }
            }

            return Ok(());
        }

        if let Some(ref dump_mode) = self.dump_mode {
            if matches!(dump_mode, DumpMode::Last) {
                let commands = renderer.draw_frame(controller);
                target.begin_frame();
                for cmd in &commands {
                    target.draw(cmd);
                }
                target.end_frame();

                match &self.output_path {
                    None => {
                        let encoder = PngEncoder::new(&mut writer);
                        if let Err(e) = target.frame().write_with_encoder(encoder) {
                            error!(error = %e, "Failed to write frame to stdout");
                        }
                    }
                    Some(path) => {
                        let png_path = path.replace(".mp4", ".png");
                        let png_path = if png_path == *path { format!("{}.png", path) } else { png_path };
                        if let Err(e) = target.frame().save(&png_path) {
                            error!(error = %e, "Failed to save frame");
                        } else {
                            let (w, h) = target.canvas_size();
                            info!(path = %png_path, width = w, height = h, "Result frame saved");
                        }
                    }
                }
            }
            return Ok(());
        }

        writer.flush().expect("flushing output to stdout failed");

        let output = match self.worker.take() {
            Some(worker) => worker.finish()?,
            None => {
                // No frames were ever submitted (for example window_duration <= 0).
                EncoderOutput { samples: Vec::new(), codec: self.active_codec.unwrap_or(VideoCodec::H264) }
            }
        };

        if let Some(ref cb) = self.progress_callback {
            cb(RenderProgress {
                stage: RenderStage::Encoding,
                current: output.samples.len() as u64,
                total: self.expected_frames,
            });
        }

        self.mux_to_mp4(&output.samples, output.codec)
    }

    fn mux_to_mp4(&mut self, samples: &[EncodedSample], codec: VideoCodec) -> rootcause::Result<(), VideoError> {
        if samples.is_empty() {
            // Surface the earlier streaming failure as the cause, re-headed with
            // mux context, so the original encoder error chain is preserved.
            if let Some(err) = self.encoder_error.take() {
                return Err(err
                    .context(VideoError::MuxFailed)
                    .attach("no frames were encoded; encoder failed earlier"));
            }
            return Err(report!(VideoError::MuxFailed).attach("no frames to mux"));
        }

        let output_path = self.output_path.as_ref().expect("output path required for video mode");
        let file = File::create(output_path)
            .context(VideoError::Io)
            .attach_with(|| format!("creating output file {output_path}"))?;
        let writer = BufWriter::new(file);

        let mut muxer = MuxerBuilder::new(writer)
            .video(map_codec(codec), self.canvas_width, self.canvas_height, FPS)
            .build()
            .context(VideoError::MuxFailed)
            .attach("initializing MP4 muxer")?;

        let total = samples.len() as u64;
        for (idx, sample) in samples.iter().enumerate() {
            muxer
                .write_video(sample.pts_seconds, &sample.data, sample.is_keyframe)
                .context(VideoError::MuxFailed)
                .attach_with(|| format!("writing video frame {idx}"))?;

            if let Some(ref cb) = self.progress_callback {
                cb(RenderProgress { stage: RenderStage::Muxing, current: (idx + 1) as u64, total });
            }
        }

        muxer.finish().context(VideoError::MuxFailed).attach("finalizing MP4 container")?;
        info!(path = %output_path, codec = %codec, "Video saved");
        Ok(())
    }
}

fn map_codec(c: VideoCodec) -> MuxideCodec {
    match c {
        VideoCodec::H264 => MuxideCodec::H264,
        VideoCodec::H265 => MuxideCodec::H265,
        VideoCodec::Av1 => MuxideCodec::Av1,
    }
}
