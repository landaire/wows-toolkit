//! Encoding a baked battle to a video file, one replay or a batch of them.

use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use rust_i18n::t;
use wows_minimap_renderer::EncoderKind;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::VideoCodec;
use wows_minimap_renderer::VideoEncoder;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::encoder::EncoderStatus;
use wows_minimap_renderer::error::VideoError;
use wows_minimap_renderer::video::CodecChoice;

use crate::load::GameDataCache;
use crate::minimap_preview::SharedPreviewRenderer;
use crate::replay_render::options::batch_options;
use crate::replay_render::ships::ShipChoices;
use crate::replay_render::track::BAKE_INTERVAL;
use crate::replay_render::track::Track;
use crate::replay_render::track::bake;

/// What an export is encoded with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportSettings {
    /// Encode in software even where the GPU could do it.
    pub prefer_cpu: bool,
    /// Start the video at the loading screen rather than at the battle.
    pub include_pre_battle: bool,
    /// The codec to encode with. `None` lets the encoder pick the best one it
    /// can, which is what most readers want.
    pub codec: Option<VideoCodec>,
}

/// How far an export has got, in frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportProgress {
    pub done: u64,
    pub total: u64,
    pub stage: ExportStage,
}

impl ExportProgress {
    /// How much of the export is done, from 0 to 1.
    ///
    /// `None` while there is nothing to measure against yet, which is while
    /// the encoder is still starting.
    pub fn fraction(&self) -> Option<f32> {
        if self.total == 0 {
            return None;
        }

        Some(self.done.min(self.total) as f32 / self.total as f32)
    }
}

/// Which part of an export is running.
///
/// Muxing happens after the last frame and can take a noticeable while on a
/// long battle, so saying so is the difference between a bar that has
/// stopped and one that is nearly done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStage {
    Encoding,
    Muxing,
}

impl ExportStage {
    /// The translation key of what the progress overlay calls it, worded as
    /// the egui renderer words it.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Encoding => "ui.renderer.encoding",
            Self::Muxing => "ui.renderer.muxing",
        }
    }
}

/// Where a render goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportTarget {
    /// A file the reader chose.
    File,
    /// A temporary file, handed to the clipboard.
    Clipboard,
}

/// Why an export stopped before it finished.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("{}", t!("ui.replay.renderer.export_path_unusable"))]
    UnusablePath { path: PathBuf },
    #[error("the encoder could not start: {0}")]
    Start(rootcause::Report<VideoError>),
    #[error("frame {index} could not be encoded: {report}")]
    Frame { index: usize, report: rootcause::Report<VideoError> },
    #[error("the video could not be finished: {0}")]
    Finish(rootcause::Report<VideoError>),
    /// Stopped before the file was written because whoever asked for it went away.
    #[error("the export was cancelled")]
    Cancelled,
}

/// Which replay of how many a batch has reached.
///
/// `done` counts the ones behind it, so the first report reads as none done of
/// however many there are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchStep {
    pub done: usize,
    pub total: usize,
    /// The replay being read, by its file stem.
    pub replay: String,
}

/// What a batch wrote, and which replays it could not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    pub written: Vec<PathBuf>,
    pub failed: Vec<PathBuf>,
}

/// What this machine can encode with, probed once.
///
/// The probe builds a GPU device to ask it, which costs long enough to notice,
/// and the answer cannot change while the app runs.
pub fn encoder_status() -> &'static EncoderStatus {
    static STATUS: LazyLock<EncoderStatus> = LazyLock::new(wows_minimap_renderer::check_encoder);
    &STATUS
}

/// Whether an export has to be encoded in software.
///
/// A codec the GPU cannot encode falls back rather than failing: picking AV1
/// is a choice of codec, not a demand for the GPU.
pub fn must_use_cpu(settings: &ExportSettings, status: &EncoderStatus) -> bool {
    if settings.prefer_cpu || !status.gpu_available() {
        return true;
    }
    match settings.codec {
        Some(codec) => !status.supports(EncoderKind::Gpu, codec),
        None => false,
    }
}

/// What an export is encoded with, as the reader last saved it.
pub fn export_settings_of(saved: &SavedRenderOptions) -> ExportSettings {
    ExportSettings {
        prefer_cpu: saved.prefer_cpu_encoder,
        codec: saved.video_codec,
        include_pre_battle: saved.include_pre_battle,
    }
}

/// The file name a save dialog suggests for a replay titled `title`.
pub fn suggested_name(title: &str) -> String {
    format!("{title}.mp4")
}

/// The frames an export of `track` writes.
///
/// What is on screen, not what was baked: a layer the reader turned off
/// should not come back in the file. The overlays drawn over a battle
/// (trails, annotations, the ranges placed on the map) are derived at draw
/// time and are not written.
pub fn export_frames(
    track: &Track,
    settings: &ExportSettings,
    options: &RenderOptions,
    show_dead_ships: bool,
    choices: &ShipChoices,
) -> Vec<Vec<DrawCommand>> {
    track
        .frames_to_export(settings.include_pre_battle)
        .iter()
        .map(|frame| {
            frame
                .iter()
                .filter(|command| should_draw_command(command, options, show_dead_ships))
                .filter(|command| choices.allows(command))
                .cloned()
                .collect()
        })
        .collect()
}

/// How much game time `frame_count` exported frames cover, in seconds.
pub fn export_duration(frame_count: usize) -> f32 {
    frame_count.saturating_sub(1) as f32 * BAKE_INTERVAL
}

/// A place to render to that outlives this process, for the clipboard.
///
/// `None` when no temporary directory could be made, which is the only way
/// this fails.
pub fn temporary_output(name: &str) -> Option<PathBuf> {
    let dir = tempfile::Builder::new().prefix("wt-render-").tempdir().ok()?;
    let path = dir.path().join(name);
    // Kept: the clipboard will hold this path, and a directory removed on
    // drop would leave it pointing at nothing.
    let _ = dir.keep();

    Some(path)
}

/// A directory a batch can write into that outlives this process, for the
/// clipboard.
///
/// `None` when no temporary directory could be made, which is the only way this
/// fails.
pub fn temporary_batch_dir() -> Option<PathBuf> {
    let dir = tempfile::Builder::new().prefix("wt-batch-").tempdir().ok()?;
    let path = dir.path().to_path_buf();
    // Kept: the clipboard will hold paths under it, and a directory removed on
    // drop would leave them pointing at nothing.
    let _ = dir.keep();

    Some(path)
}

/// Rasterises `frames` and encodes them to `output`. Blocks for the whole
/// encode.
///
/// The renderer is the viewport's own, which is why a caller holds it for
/// the duration rather than drawing through it at the same time. `progress`
/// is told after every frame, and once more before the encoder finishes.
/// Setting `cancel` stops the encode before the next frame, or before the file is written.
pub fn encode_track(
    renderer: &SharedPreviewRenderer,
    frames: &[Vec<DrawCommand>],
    duration_seconds: f32,
    output: &Path,
    settings: ExportSettings,
    cancel: &AtomicBool,
    mut progress: impl FnMut(ExportProgress),
) -> Result<(), ExportError> {
    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (width, height) = drawing.canvas_size();

    let Some(path) = output.to_str() else {
        return Err(ExportError::UnusablePath { path: output.to_path_buf() });
    };
    let mut encoder = VideoEncoder::new(Some(path), None, false, duration_seconds, width, height);
    encoder.set_prefer_cpu(must_use_cpu(&settings, encoder_status()));
    encoder.set_codec(match settings.codec {
        Some(codec) => CodecChoice::Explicit(codec),
        None => CodecChoice::Auto,
    });
    encoder.init().map_err(ExportError::Start)?;

    // The encoder writes `output` only once it finishes, so a cancelled encode leaves whatever was
    // there before untouched.
    let total = frames.len() as u64;
    for (index, commands) in frames.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ExportError::Cancelled);
        }
        let image = drawing.render(commands);
        encoder.submit_frame(&image).map_err(|report| ExportError::Frame { index, report })?;
        progress(ExportProgress { done: index as u64 + 1, total, stage: ExportStage::Encoding });
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }

    // Said before rather than after, because finishing is the part that
    // takes a while with nothing else to show for it.
    progress(ExportProgress { done: total, total, stage: ExportStage::Muxing });
    encoder.finish_submitted().map_err(ExportError::Finish)
}

/// Renders each replay in `paths` to its own file under `out_dir`, without
/// opening a viewport for any of them. Blocks until every replay has been
/// tried; a caller runs it on a background thread.
///
/// Sequential, because each encode holds a GPU device and the machine has one:
/// two at once would contend for it and finish no sooner. A replay that fails is
/// reported and the rest still run, since one unreadable file in a marked set
/// should not lose the other forty.
///
/// `defaults` is what the reader saved as what a renderer opens with, so a
/// batch writes what a viewport would have shown rather than the built-in set.
/// `report` is told which replay the batch has reached before it is read, so the
/// reader is not left watching a still line for forty files.
pub fn batch_export(
    paths: Vec<PathBuf>,
    game_data: &GameDataCache,
    out_dir: &Path,
    defaults: &SavedRenderOptions,
    mut report: impl FnMut(BatchStep),
) -> BatchOutcome {
    let settings = export_settings_of(defaults);
    let (options, show_dead_ships) = batch_options(defaults);
    let mut outcome = BatchOutcome::default();
    // A batch runs to the end: nothing asks it to stop part-way.
    let cancel = AtomicBool::new(false);
    let total = paths.len();

    for (index, path) in paths.into_iter().enumerate() {
        let named = path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
        report(BatchStep { done: index, total, replay: named });
        let Some(stem) = path.file_stem() else {
            outcome.failed.push(path);
            continue;
        };
        let output = out_dir.join(stem).with_extension("mp4");

        let (track, renderer) = match bake(&path, &[], game_data, &cancel) {
            Ok(baked) => baked,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "batch render: the replay did not bake");
                outcome.failed.push(path);
                continue;
            }
        };

        // The bake holds every layer it could; which of them are drawn is
        // what the saved defaults decide, as they do in a viewport.
        let frames: Vec<Vec<DrawCommand>> = track
            .frames_to_export(settings.include_pre_battle)
            .iter()
            .map(|frame| {
                frame
                    .iter()
                    .filter(|command| should_draw_command(command, &options, show_dead_ships))
                    .cloned()
                    .collect()
            })
            .collect();
        if frames.is_empty() {
            outcome.failed.push(path);
            continue;
        }
        let duration = export_duration(frames.len());

        // The batch reports per replay rather than per frame.
        match encode_track(&renderer, &frames, duration, &output, settings, &cancel, |_progress| {}) {
            Ok(()) => outcome.written.push(output),
            Err(reason) => {
                tracing::warn!(path = %path.display(), %reason, "batch render: the encode failed");
                outcome.failed.push(path);
            }
        }
    }

    outcome
}
