//! Encoding a baked battle to a video file, one replay or a batch of them.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use rust_i18n::t;
use wows_minimap_renderer::EncoderKind;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::ShipVisibility;
use wows_minimap_renderer::VideoCodec;
use wows_minimap_renderer::VideoEncoder;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::encoder::EncoderStatus;
use wows_minimap_renderer::error::VideoError;
use wows_minimap_renderer::map_data::MinimapPos;
use wows_minimap_renderer::trail::TrailHistory;
use wows_minimap_renderer::video::CodecChoice;
use wows_minimap_renderer::video::FPS;
use wows_minimap_renderer::video::OUTPUT_DURATION;
use wows_replays::types::EntityId;

use crate::load::GameDataCache;
use crate::minimap_preview::SharedPreviewRenderer;
use crate::minimap_preview::layout_for;
use crate::minimap_preview::renderer_for_replay;
use crate::replay_render::options::batch_options;
use crate::replay_render::raster::shown_commands;
use crate::replay_render::ships::ShipChoices;
use crate::replay_render::ships::track_players;
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

/// Which replay of how many a batch has reached, and how far its encode has got.
///
/// `done` counts the ones behind it, so the first report reads as none done of
/// however many there are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchStep {
    pub done: usize,
    pub total: usize,
    /// The replay being read, by its file stem.
    pub replay: String,
    /// The encode's progress through this replay's frames. `None` while it is read and baked.
    pub frames: Option<ExportProgress>,
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

/// The file name a save dialog suggests for a track whose video is called `video_stem`.
pub fn suggested_name(video_stem: &str) -> String {
    format!("{video_stem}.mp4")
}

/// How many frames every export writes: a fixed length at a fixed rate, the battle compressed to
/// fit, as the egui renderer writes it.
pub const EXPORT_FRAME_COUNT: usize = (OUTPUT_DURATION * FPS) as usize;

/// What an export draws of a battle: the layers and per-ship choices on screen, and what was
/// drawn over the battle rather than by it (annotations and placed ranges).
pub struct ExportLayers {
    pub options: RenderOptions,
    pub show_dead_ships: bool,
    pub choices: ShipChoices,
    pub drawn_on: Vec<DrawCommand>,
}

/// The frames an export writes, drawn one at a time as the encoder takes them.
///
/// [`EXPORT_FRAME_COUNT`] frames spread evenly over the exported stretch of the battle. A track
/// keeps fewer frames than that, and a long battle's are further apart, so each written frame is
/// the baked frame before it with the ships moved the matching part of the way to the next.
/// What is on screen is what is written: a layer the reader turned off does not come back in the
/// file, and trails are derived from the baked frames behind, as the viewport derives them.
pub struct ExportFrames {
    track: Arc<Track>,
    layers: ExportLayers,
    /// The first baked frame the export covers.
    first: usize,
    /// The game time of the first written frame, how much game time the export covers, and the
    /// game time between written frames.
    start: f32,
    duration: f32,
    step: f32,
    written: usize,
    trails: TrailHistory,
    /// How many baked frames `trails` has walked.
    walked: usize,
}

impl ExportFrames {
    /// `None` for a track with no frames to export.
    pub fn new(track: Arc<Track>, include_pre_battle: bool, layers: ExportLayers) -> Option<Self> {
        if track.is_empty() {
            return None;
        }
        let first = track.export_start(include_pre_battle);
        let start = track.seconds_at(first);
        let end = track.seconds_at(track.last());
        let duration = (end - start).max(0.0);
        let step = duration / (EXPORT_FRAME_COUNT - 1) as f32;

        Some(Self {
            track,
            layers,
            first: first.get(),
            start,
            duration,
            step,
            written: 0,
            trails: TrailHistory::new(),
            walked: 0,
        })
    }

    /// How many frames the export writes.
    pub fn len(&self) -> usize {
        EXPORT_FRAME_COUNT
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// How much game time the export covers, in seconds.
    pub fn duration_seconds(&self) -> f32 {
        self.duration
    }
}

impl Iterator for ExportFrames {
    type Item = Vec<DrawCommand>;

    fn next(&mut self) -> Option<Vec<DrawCommand>> {
        if self.written >= EXPORT_FRAME_COUNT {
            return None;
        }
        let at = self.start + self.step * self.written as f32;
        self.written += 1;

        let base = self.track.frame_at(at).get().max(self.first);
        let wants_trails = self.layers.options.show_trails || self.layers.options.show_speed_trails;
        while wants_trails && self.walked <= base {
            self.trails.push(&self.track.frames[self.walked]);
            self.walked += 1;
        }
        let mut trails = self.trails.trails(&self.layers.options);

        let layers = &self.layers;
        let between = interpolated(&self.track, base, at);
        reach_ships(&mut trails, &between);
        let mut shown =
            shown_commands(trails.into_iter().chain(between), &layers.options, layers.show_dead_ships, &layers.choices);
        shown.extend(layers.drawn_on.iter().cloned());

        Some(shown)
    }
}

/// Ends each trail where its ship is drawn, part of the way to the next baked frame, so the trail
/// does not stop short of the ship.
fn reach_ships(trails: &mut [DrawCommand], frame: &[DrawCommand]) {
    let drawn: HashMap<EntityId, MinimapPos> = frame
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Ship { entity_id, pos, .. } => Some((*entity_id, *pos)),
            _ => None,
        })
        .collect();
    for trail in trails {
        let DrawCommand::PositionTrail { entity_id, points, .. } = trail else { continue };
        let (Some(pos), Some(&(last, color))) = (drawn.get(entity_id), points.last()) else { continue };
        if (last.x, last.y) != (pos.x, pos.y) {
            points.push((*pos, color));
        }
    }
}

/// Where a ship is drawn in a baked frame.
#[derive(Clone, Copy, Debug)]
struct ShipPlace {
    pos: MinimapPos,
    yaw: f32,
    visibility: ShipVisibility,
}

/// How far one ship moves and turns between a baked frame and an export frame after it.
#[derive(Clone, Copy, Debug)]
struct ShipMove {
    dx: f32,
    dy: f32,
    turn: f32,
}

/// Frame `base`'s commands with each ship moved the part of the way to frame `base + 1` that
/// game time `at` is, and what is drawn at the ship (its health bar, ranges, consumables and
/// camera line) moved with it.
fn interpolated(track: &Track, base: usize, at: f32) -> Vec<DrawCommand> {
    let commands = &track.frames[base];
    let (Some(next), Some(from), Some(to)) =
        (track.frames.get(base + 1), track.clocks.get(base), track.clocks.get(base + 1))
    else {
        return commands.clone();
    };
    let span = to.seconds() - from.seconds();
    if span <= 0.0 {
        return commands.clone();
    }
    let fraction = ((at - from.seconds()) / span).clamp(0.0, 1.0);

    let ahead: HashMap<EntityId, ShipPlace> = next
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Ship { entity_id, pos, yaw, visibility, .. } => {
                Some((*entity_id, ShipPlace { pos: *pos, yaw: *yaw, visibility: *visibility }))
            }
            _ => None,
        })
        .collect();
    let moves: HashMap<EntityId, ShipMove> = commands
        .iter()
        .filter_map(|command| {
            let DrawCommand::Ship { entity_id, pos, yaw, visibility, .. } = command else { return None };
            let ShipPlace { pos: next_pos, yaw: next_yaw, visibility: next_visibility } = ahead.get(entity_id)?;
            // A ship coming into or going out of sight jumps between where it was last seen and
            // where it is, which is not a path it sailed.
            if next_visibility != visibility {
                return None;
            }
            let turn = (next_yaw - yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            Some((
                *entity_id,
                ShipMove {
                    dx: (next_pos.x - pos.x) * fraction,
                    dy: (next_pos.y - pos.y) * fraction,
                    turn: turn * fraction,
                },
            ))
        })
        .collect();

    commands
        .iter()
        .cloned()
        .map(|mut command| {
            let (entity_id, pos, yaw) = match &mut command {
                DrawCommand::Ship { entity_id, pos, yaw, .. } => (*entity_id, pos, Some(yaw)),
                DrawCommand::HealthBar { entity_id, pos, .. }
                | DrawCommand::CameraDirection { entity_id, pos, .. }
                | DrawCommand::ConsumableRadius { entity_id, pos, .. }
                | DrawCommand::ConsumableIcons { entity_id, pos, .. }
                | DrawCommand::ShipConfigCircle { entity_id, pos, .. } => (*entity_id, pos, None),
                _ => return command,
            };
            if let Some(step) = moves.get(&entity_id) {
                pos.x += step.dx;
                pos.y += step.dy;
                if let Some(yaw) = yaw {
                    *yaw += step.turn;
                }
            }
            command
        })
        .collect()
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
    frames: ExportFrames,
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
    let mut encoder = VideoEncoder::new(Some(path), None, false, frames.duration_seconds(), width, height);
    encoder.set_prefer_cpu(must_use_cpu(&settings, encoder_status()));
    encoder.set_codec(match settings.codec {
        Some(codec) => CodecChoice::Explicit(codec),
        None => CodecChoice::Auto,
    });
    encoder.init().map_err(ExportError::Start)?;

    // The encoder writes `output` only once it finishes, so a cancelled encode leaves whatever was
    // there before untouched.
    let total = frames.len() as u64;
    for (index, commands) in frames.enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ExportError::Cancelled);
        }
        let image = drawing.render(&commands);
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
/// `bake_options` is what each replay is baked with, normally
/// [`playback_options`](crate::replay_render::options::playback_options).
/// `report` is told which replay the batch has reached before it is read, and
/// then how far its encode has got after every frame.
pub fn batch_export(
    paths: Vec<PathBuf>,
    game_data: &GameDataCache,
    out_dir: &Path,
    defaults: &SavedRenderOptions,
    bake_options: RenderOptions,
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
        report(BatchStep { done: index, total, replay: named.clone(), frames: None });

        let (track, baked_on) = match bake(&path, &[], game_data, &cancel, bake_options.clone()) {
            Ok(baked) => baked,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "batch render: the replay did not bake");
                outcome.failed.push(path);
                continue;
            }
        };
        // The bake draws on the bare map; the saved panels need a wider canvas.
        let renderer = match layout_for(&options) {
            SidePanelLayout::None => baked_on,
            layout => match renderer_for_replay(&path, game_data, layout) {
                Ok((renderer, _)) => renderer,
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "batch render: no canvas for {layout:?}");
                    outcome.failed.push(path);
                    continue;
                }
            },
        };
        let output = out_dir.join(suggested_name(&track.video_stem));

        let layers = batch_layers(&options, show_dead_ships, &track);
        let Some(frames) = ExportFrames::new(Arc::new(track), settings.include_pre_battle, layers) else {
            outcome.failed.push(path);
            continue;
        };

        let encoded = encode_track(&renderer, frames, &output, settings, &cancel, |step| {
            report(BatchStep { done: index, total, replay: named.clone(), frames: Some(step) });
        });
        match encoded {
            Ok(()) => outcome.written.push(output),
            Err(reason) => {
                tracing::warn!(path = %path.display(), %reason, "batch render: the encode failed");
                outcome.failed.push(path);
            }
        }
    }

    outcome
}

/// What a batch draws of `track` under its saved `options`.
///
/// The bake holds every layer it could; which of them are drawn is what the saved defaults
/// decide, as they do in a viewport. Saved range circles are every ship's, as the egui batch draws
/// them, since a batch has no per-ship choices of its own.
pub(crate) fn batch_layers(options: &RenderOptions, show_dead_ships: bool, track: &Track) -> ExportLayers {
    let mut options = options.clone();
    let mut choices = ShipChoices::new();
    if options.show_ship_config {
        choices.all_ranges(&track_players(&track.frames), &mut options);
    }

    ExportLayers { options, show_dead_ships, choices, drawn_on: Vec::new() }
}
