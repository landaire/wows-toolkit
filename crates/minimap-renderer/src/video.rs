use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::io::stdout;

use image::RgbImage;
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
use crate::timing::ExportTiming;
use crate::timing::ExportWindow;
use crate::timing::FPS;
use crate::timing::PlaybackRate;

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

    /// The stretch of the replay the video covers. Packets before its start
    /// still build up controller state but produce no frames.
    window: ExportWindow,
    frame_count: usize,
    /// Game seconds between frames.
    frame_step: f32,
    rate: PlaybackRate,
    output_seconds: f32,
    /// Frames drawn or submitted so far, which is also the next frame's index.
    frames_done: usize,

    worker: Option<EncoderWorker>,
    encoder_error: Option<rootcause::Report<VideoError>>,
    codec_choice: CodecChoice,
    mode: Mode,
    encoder_config: crate::encoder::EncoderConfig,
    /// Codec resolved at encoder-init time; `None` until init().
    active_codec: Option<VideoCodec>,
    progress_callback: Option<Box<dyn Fn(RenderProgress)>>,
    canvas_width: u32,
    canvas_height: u32,
}

impl VideoEncoder {
    /// An encoder for a video of `window`, framed by `timing`.
    ///
    /// The window is fixed up front because `timing` places every frame
    /// relative to it: a caller streaming packets learns the end of the replay
    /// before drawing the first frame (`MergedReplays::total_duration`).
    pub fn new(
        output_path: Option<&str>,
        dump_mode: Option<DumpMode>,
        dump_all_mode: bool,
        timing: ExportTiming,
        window: ExportWindow,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        let seconds = window.seconds();
        Self {
            output_path: output_path.map(String::from),
            dump_mode,
            dump_all_mode,
            window,
            frame_count: timing.frame_count(seconds),
            frame_step: timing.frame_step(seconds),
            rate: timing.rate(seconds),
            output_seconds: timing.output_seconds(seconds),
            frames_done: 0,
            worker: None,
            encoder_error: None,
            codec_choice: CodecChoice::Auto,
            mode: Mode::Auto,
            encoder_config: crate::encoder::EncoderConfig::default(),
            active_codec: None,
            progress_callback: None,
            canvas_width,
            canvas_height,
        }
    }

    /// Frames the finished video holds.
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Seconds the finished video runs.
    pub fn output_seconds(&self) -> f32 {
        self.output_seconds
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
    /// under `target_size_bytes` for this video's length. Fails, leaving the
    /// tunables as they were, when the video cannot be made that small.
    pub fn target_max_file_size(&mut self, target_size_bytes: u64) -> Result<(), crate::error::TargetSizeError> {
        self.encoder_config = crate::encoder::EncoderConfig::from_target_size(target_size_bytes, self.output_seconds)?;
        Ok(())
    }

    pub fn set_progress_callback<F: Fn(RenderProgress) + 'static>(&mut self, callback: F) {
        self.progress_callback = Some(Box::new(callback));
    }

    /// How many frames show a game clock at or before `clock`.
    fn frames_due(&self, clock: GameClock) -> usize {
        let into_window = clock - self.window.start();
        if into_window < 0.0 {
            return 0;
        }

        // An empty window is one frame, due as soon as the window starts.
        if self.frame_step <= 0.0 {
            return self.frame_count;
        }

        ((into_window / self.frame_step).floor() as usize).saturating_add(1).min(self.frame_count)
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
        let config = self.encoder_config.sized_for(self.output_seconds);
        let (worker, resolved_codec, kind) =
            EncoderWorker::spawn(self.canvas_width, self.canvas_height, codec, self.mode, config)?;
        self.active_codec = Some(resolved_codec);
        self.worker = Some(worker);
        info!(
            frames = self.frame_count,
            width = self.canvas_width,
            height = self.canvas_height,
            window = self.window.seconds(),
            output = self.output_seconds,
            rate = self.rate.game_seconds_per_second(),
            codec = %resolved_codec,
            kind = %kind,
            fps = FPS,
            "Rendering"
        );
        Ok(())
    }

    /// Draws every frame whose game clock is at or before `new_clock`, from
    /// the battle state as it stands.
    pub fn advance_clock(
        &mut self,
        new_clock: GameClock,
        controller: &BattleView<'_>,
        renderer: &mut MinimapRenderer,
        target: &mut ImageTarget,
    ) {
        let due = self.frames_due(new_clock);
        self.draw_frames(due, controller, renderer, target);
    }

    /// Draws frames until `until` of them are done.
    fn draw_frames(
        &mut self,
        until: usize,
        controller: &BattleView<'_>,
        renderer: &mut MinimapRenderer,
        target: &mut ImageTarget,
    ) {
        if self.encoder_error.is_some() || self.frames_done >= until {
            return;
        }

        renderer.set_playback_rate(self.rate);
        let mut writer = BufWriter::new(stdout().lock());

        while self.frames_done < until {
            let index = self.frames_done;
            self.frames_done += 1;

            // Continuous per-frame time so positions interpolate and tracers /
            // torpedoes (paced by the render clock) animate between packets.
            let frame_seconds = self.window.start().seconds() + index as f32 * self.frame_step;
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
                        let png_path = format!("{}{}{}.png", path, std::path::MAIN_SEPARATOR, index);
                        if let Err(e) = target.frame().save(&png_path) {
                            error!(error = %e, "Failed to save frame");
                            return;
                        }
                    }
                }

                if let Some(cb) = &self.progress_callback {
                    cb(RenderProgress {
                        stage: RenderStage::Encoding,
                        current: self.frames_done as u64,
                        total: self.frame_count as u64,
                    });
                }
            } else if let Some(dump_mode) = &self.dump_mode {
                let dump_frame = match dump_mode {
                    DumpMode::Frame(n) => Some(*n),
                    DumpMode::Midpoint => Some(self.frame_count / 2),
                    // Drawn by `finish` from the final battle state.
                    DumpMode::Last => None,
                };
                if let Some(dump_frame) = dump_frame
                    && index == dump_frame
                {
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

                if let Some(cb) = &self.progress_callback {
                    let encoded = self.worker.as_ref().map(|w| w.encoded_count()).unwrap_or(0);
                    cb(RenderProgress {
                        stage: RenderStage::Encoding,
                        current: encoded,
                        total: self.frame_count as u64,
                    });
                }
            }
        }

        writer.flush().expect("flushing output to stdout failed");
    }

    /// Encodes one frame that the caller has already drawn.
    ///
    /// [`advance_clock`](Self::advance_clock) draws its own frames from a live
    /// battle, which a caller holding a finished track of draw commands does
    /// not have. This takes the drawn pixels instead, so the same encoder and
    /// muxer serve both. Pair it with
    /// [`finish_submitted`](Self::finish_submitted).
    ///
    /// The frame must be the canvas size this encoder was built for; one of
    /// any other size is refused rather than encoded into a stream whose
    /// header says otherwise.
    pub fn submit_frame(&mut self, frame: &RgbImage) -> rootcause::Result<(), VideoError> {
        if let Some(error) = &self.encoder_error {
            return Err(report!(VideoError::EncodeFailed).attach(format!("the encoder already failed: {error}")));
        }
        let (width, height) = (frame.width(), frame.height());
        if width != self.canvas_width || height != self.canvas_height {
            return Err(report!(VideoError::EncodeFailed).attach(format!(
                "frame is {width}x{height} but the encoder was built for {}x{}",
                self.canvas_width, self.canvas_height
            )));
        }

        self.ensure_encoder()?;
        let worker = self.worker.as_ref().expect("worker is Some after ensure_encoder succeeded");
        if let Err(e) = worker.submit(frame.as_raw().to_vec()) {
            self.encoder_error = Some(e);
            let error = self.encoder_error.as_ref().expect("just set");
            return Err(report!(VideoError::EncodeFailed).attach(format!("frame submission failed: {error}")));
        }
        self.frames_done += 1;

        if let Some(cb) = &self.progress_callback {
            let encoded = self.worker.as_ref().map(|w| w.encoded_count()).unwrap_or(0);
            cb(RenderProgress { stage: RenderStage::Encoding, current: encoded, total: self.frame_count as u64 });
        }
        Ok(())
    }

    /// Closes a stream fed by [`submit_frame`](Self::submit_frame) and writes
    /// the file.
    ///
    /// The counterpart to [`finish`](Self::finish) for a caller with no battle
    /// to draw a final frame from.
    pub fn finish_submitted(&mut self) -> rootcause::Result<(), VideoError> {
        let output = match self.worker.take() {
            Some(worker) => worker.finish()?,
            // Nothing was ever submitted, which is an empty track rather than
            // a failure; the mux below reports it as one.
            None => EncoderOutput { samples: Vec::new(), codec: self.active_codec.unwrap_or(VideoCodec::H264) },
        };

        if let Some(cb) = &self.progress_callback {
            cb(RenderProgress {
                stage: RenderStage::Encoding,
                current: output.samples.len() as u64,
                total: self.frame_count as u64,
            });
        }

        self.mux_to_mp4(&output.samples, output.codec)
    }

    /// Draws the frames not yet drawn from the final battle state, then
    /// writes the file.
    pub fn finish(
        &mut self,
        controller: &BattleView<'_>,
        renderer: &mut MinimapRenderer,
        target: &mut ImageTarget,
    ) -> rootcause::Result<(), VideoError> {
        self.draw_frames(self.frame_count, controller, renderer, target);

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
                    let last = self.frames_done.saturating_sub(1);
                    let png_path = format!("{}{}{}.png", path, std::path::MAIN_SEPARATOR, last);
                    if let Err(e) = target.frame().save(&png_path) {
                        error!(error = %e, "Failed to save frame");
                    }
                }
            }

            return Ok(());
        }

        if let Some(dump_mode) = &self.dump_mode {
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
            // The encoder failed to start, so no frame reached it; the mux
            // below reports the failure.
            None => EncoderOutput { samples: Vec::new(), codec: self.active_codec.unwrap_or(VideoCodec::H264) },
        };

        if let Some(cb) = &self.progress_callback {
            cb(RenderProgress {
                stage: RenderStage::Encoding,
                current: output.samples.len() as u64,
                total: self.frame_count as u64,
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

            if let Some(cb) = &self.progress_callback {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start: f32, end: f32) -> ExportWindow {
        ExportWindow::new(GameClock(start), GameClock(end)).expect("a window")
    }

    fn encoder(timing: ExportTiming, window: ExportWindow) -> VideoEncoder {
        VideoEncoder::new(Some("unused.mp4"), None, false, timing, window, 64, 32)
    }

    /// A frame of the wrong size is refused before the encoder is started, so
    /// the check costs nothing and needs no encode backend to exercise.
    #[test]
    fn a_frame_of_the_wrong_size_is_refused() {
        let mut encoder = encoder(ExportTiming::default(), window(0.0, 60.0));

        let wrong = RgbImage::new(32, 64);
        let refused = encoder.submit_frame(&wrong);

        assert!(refused.is_err(), "a 32x64 frame does not belong in a 64x32 stream");
        // Nothing was started, so nothing has to be torn down.
        assert!(encoder.worker.is_none());
    }

    #[test]
    fn a_stream_that_was_fed_nothing_reports_no_frames_to_mux() {
        let mut encoder = encoder(ExportTiming::default(), window(0.0, 60.0));

        let finished = encoder.finish_submitted();

        // An empty track is a failure to report, not a zero-byte file.
        assert!(finished.is_err());
    }

    /// Frames fall due as the clock passes each one's place in the window,
    /// and never beyond the frame count.
    #[test]
    fn frames_fall_due_by_the_window_clock() {
        let timing = ExportTiming::Speed(crate::timing::ExportSpeed::new(30.0).expect("an export speed"));
        let encoder = encoder(timing, window(10.0, 13.5));

        assert_eq!(encoder.frame_count(), 4);
        assert_eq!(encoder.frames_due(GameClock(9.0)), 0, "before the window");
        assert_eq!(encoder.frames_due(GameClock(10.0)), 1);
        assert_eq!(encoder.frames_due(GameClock(10.99)), 1);
        assert_eq!(encoder.frames_due(GameClock(11.0)), 2);
        assert_eq!(encoder.frames_due(GameClock(100.0)), 4, "after the window");
    }

    #[test]
    fn an_empty_window_is_one_frame_at_its_start() {
        let encoder = encoder(ExportTiming::default(), window(20.0, 20.0));

        assert_eq!(encoder.frames_due(GameClock(19.0)), 0);
        assert_eq!(encoder.frames_due(GameClock(20.0)), 1800);
    }

    /// A size the video cannot be made to fit is refused rather than
    /// exceeded, and leaves the encoder's tunables as they were.
    #[test]
    fn a_maximum_size_below_what_the_length_allows_is_refused() {
        let real_time = ExportTiming::Speed(crate::timing::ExportSpeed::new(1.0).expect("an export speed"));
        let mut encoder = encoder(real_time, window(0.0, 3600.0));

        let refused = encoder.target_max_file_size(crate::encoder::DEFAULT_TARGET_FILE_SIZE_BYTES);

        assert_eq!(refused.map_err(|error| error.minimum_bytes), Err(31_304_348));
        assert_eq!(encoder.encoder_config.target_bitrate_bps, None);
    }
}
