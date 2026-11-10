//! A replay's battle drawn on its minimap: the hover preview's walked tracks,
//! the playback bake, the battle's events, and the renderers they are drawn
//! through.
//!
//! When the preview appears is [`wows_toolkit_viewmodel::preview_dwell`],
//! shared by the front ends so all wait the same amount. What it draws is
//! [`wows_minimap_renderer::preview`], the same command set the egui popup
//! paints and the video export encodes, rasterised to an image here for front
//! ends that have no painter of their own. Images leave this module as RGB;
//! each front end turns them into whatever it paints.

use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::SystemTime;

use image::GenericImageView;
use image::Rgb;
use image::RgbImage;
use wows_battle_world::ids::ShotTracking;
use wows_battle_world::merged::MergedReplays;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::assets;
use wows_minimap_renderer::config::RenderOptions;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::drawing::SidePanelLayout;
use wows_minimap_renderer::frame_track::PREVIEW_FPS;
use wows_minimap_renderer::frame_track::PREVIEW_MAX_FRAMES;
use wows_minimap_renderer::frame_track::SNAPSHOTS_PER_SECOND;
use wows_minimap_renderer::frame_track::TrackSink;
use wows_minimap_renderer::frame_track::build_frame_track;
use wows_minimap_renderer::frame_track::paint_options;
use wows_minimap_renderer::preview::PreviewRenderer;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::viewport::MapViewport;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replay_insights::timeline::TimelineExtractionResult;
use wows_replays::ReplayFile;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::vfs::VfsPath;

use crate::load::GameDataCache;
use crate::load::LoadedGameData;

/// A renderer shared between the previews that draw the same map.
///
/// Behind a mutex because rendering a frame mutates it; two previews of one
/// map therefore take turns, which is what they already did when each held
/// its own copy.
pub type SharedPreviewRenderer = Arc<Mutex<PreviewRenderer>>;

/// Which map's art a kept renderer carries.
///
/// The build is part of the name because a map's art changes between builds;
/// `None` is the entry a caller drawing against whichever build happens to be
/// open takes, which claims no build of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RendererKey {
    build: Option<NonZeroU32>,
    /// The map drawn, or the board whose art was handed over. A board's art
    /// comes off the wire rather than out of a build, so it cannot share the
    /// entry a map name names.
    map: String,
    /// The canvas a renderer was built for. A viewport showing the team
    /// rosters needs gutters the hover previews have no room for, so the two
    /// cannot share one.
    layout: SidePanelLayout,
}

/// One kept renderer and the map it draws.
struct CachedRenderer {
    key: RendererKey,
    renderer: SharedPreviewRenderer,
}

/// The renderers already built, least recently asked for first.
///
/// A `PreviewRenderer` carries the whole art set for one map, read out of one
/// build's VFS; building a fresh one per hover is what made sweeping the
/// listing cost hundreds of megabytes.
static RENDERERS: Mutex<Vec<CachedRenderer>> = Mutex::new(Vec::new());

/// How many maps' art is kept. A reader sweeping a listing moves between a
/// handful of maps, and every entry held is that map's whole art set.
const RENDERER_CACHE_SIZE: usize = 4;

/// The renderer kept under `key`, moved to the back of the cache.
fn kept_renderer(key: &RendererKey) -> Option<SharedPreviewRenderer> {
    let mut cache = RENDERERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let at = cache.iter().position(|held| &held.key == key)?;
    // Moved to the end, so the least recently asked for is the one dropped
    // when the cache is full.
    let entry = cache.remove(at);
    let renderer = Arc::clone(&entry.renderer);
    cache.push(entry);

    Some(renderer)
}

/// Keeps `renderer` under `key`, dropping the least recently asked for past
/// the cache's size.
fn keep_renderer(key: RendererKey, renderer: &SharedPreviewRenderer) {
    let mut cache = RENDERERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|held| held.key != key);
    cache.push(CachedRenderer { key, renderer: Arc::clone(renderer) });
    while cache.len() > RENDERER_CACHE_SIZE {
        cache.remove(0);
    }
}

/// The renderer for `map_name` on `build`, built once and kept.
///
/// `build` is `None` for a caller drawing against whichever build happens to
/// be open ([`render_map`]), which keys its own entry rather than claiming a
/// build's.
fn renderer_for(
    build: Option<NonZeroU32>,
    map_name: &str,
    vfs: &VfsPath,
    version: Option<&Version>,
    layout: SidePanelLayout,
) -> Result<SharedPreviewRenderer, PreviewError> {
    let key = RendererKey { build, map: map_name.to_owned(), layout };
    if let Some(renderer) = kept_renderer(&key) {
        return Ok(renderer);
    }

    // Built outside the lock: reading a build's art takes long enough that
    // holding the cache through it would stall every other preview. Two
    // callers racing on the same map build it twice and the second wins,
    // which costs one extra read rather than a stall.
    let renderer: SharedPreviewRenderer =
        Arc::new(Mutex::new(PreviewRenderer::with_layout(vfs, version, map_name, layout)?));
    keep_renderer(key, &renderer);

    Ok(renderer)
}

/// The renderer kept for `held_as`, built over `art` if there is not one yet.
fn renderer_for_art(held_as: &str, art: &RgbImage, vfs: &VfsPath) -> SharedPreviewRenderer {
    let key = RendererKey { build: None, map: held_as.to_owned(), layout: SidePanelLayout::None };
    if let Some(renderer) = kept_renderer(&key) {
        return renderer;
    }

    // Built outside the lock, as `renderer_for` builds its own and for the same
    // reason: reading a build's fonts and icons takes long enough to stall
    // every other preview if the cache were held through it.
    let renderer: SharedPreviewRenderer =
        Arc::new(Mutex::new(PreviewRenderer::with_art(art.clone(), vfs, None, SidePanelLayout::None)));
    keep_renderer(key, &renderer);

    renderer
}

/// The build a replay was recorded on, read from its header.
fn recorded_build(replay: &ReplayFile) -> Result<(Version, NonZeroU32), PreviewError> {
    let raw = &replay.meta.clientVersionFromExe;
    let version = Version::try_from_client_exe(raw).ok_or_else(|| PreviewError::UnknownBuild { raw: raw.clone() })?;
    // A replay whose header carries no build number names no build to load.
    let build = version.build.ok_or_else(|| PreviewError::UnknownBuild { raw: raw.clone() })?;

    Ok((version, build))
}

fn load_build(
    game_data: &GameDataCache,
    build: NonZeroU32,
    version: &Version,
) -> Result<Arc<LoadedGameData>, PreviewError> {
    game_data
        .get_or_load_build_for(build.get(), Some(version))
        .map_err(|err| PreviewError::NoGameData { version: version.to_path(), reason: err.to_string() })
}

/// The renderer `path`'s battle is drawn through on `layout`'s canvas, and
/// where the map's corner sits on it.
///
/// Kept per layout, so asking for the rosters' canvas builds it once and
/// switching back is free. Reads the replay's header again to find its build,
/// which is what a deliberate toggle can afford.
pub fn renderer_for_replay(
    path: &Path,
    game_data: &GameDataCache,
    layout: SidePanelLayout,
) -> Result<(SharedPreviewRenderer, (u32, u32)), PreviewError> {
    let replay = ReplayFile::from_file(path).map_err(PreviewError::UnreadableReplay)?;
    let (version, build) = recorded_build(&replay)?;
    let loaded = load_build(game_data, build, &version)?;

    let renderer = renderer_for(Some(build), &replay.meta.mapName, loaded.vfs(), Some(&version), layout)?;
    let origin = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).map_origin();

    Ok((renderer, origin))
}

/// The canvas `options` need.
///
/// Rosters live in gutters either side of the map, so a canvas that shows
/// them is wider and its map does not start at the left edge.
pub fn layout_for(options: &RenderOptions) -> SidePanelLayout {
    if options.show_team_rosters {
        SidePanelLayout::TeamRosters
    } else if options.show_stats_panel {
        SidePanelLayout::StatsPanel
    } else {
        SidePanelLayout::None
    }
}

/// Drops every kept renderer and walked track, for a caller that would rather
/// have the memory back than the next preview's speed, or whose install or
/// language has changed under them.
pub fn forget_kept_previews() {
    RENDERERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    let mut kept = TRACKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    kept.tracks.clear();
    kept.generation = TrackGeneration(kept.generation.0 + 1);
}

/// Which replay a kept track was walked from.
///
/// The modification time is part of it so a replay replaced in place is
/// walked again rather than playing the battle it replaced.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrackKey {
    path: PathBuf,
    modified: SystemTime,
}

impl TrackKey {
    /// `None` when the file's modification time cannot be read, which leaves
    /// nothing to tell a replaced file by; such a replay is walked every time.
    fn of(path: &Path) -> Option<Self> {
        let modified = std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()?;
        Some(Self { path: path.to_owned(), modified })
    }
}

/// A battle walked for the hover preview, kept as commands.
///
/// Commands rather than frames: a track's frames are tens of megabytes and its
/// commands a fraction of that, and rasterising them again is far quicker than
/// walking the battle.
struct WalkedTrack {
    frames: Vec<Vec<DrawCommand>>,
    build: NonZeroU32,
    version: Version,
    map_name: String,
}

/// One kept track and the replay it was walked from.
struct CachedTrack {
    key: TrackKey,
    track: Arc<WalkedTrack>,
}

/// How many times the kept tracks have been forgotten.
///
/// A walk notes it before it starts, so one that was running when the tracks
/// were forgotten, and may carry names in the language they were forgotten
/// for, is not kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrackGeneration(u64);

/// The tracks walked most recently, least recently asked for first, and the
/// generation they belong to.
struct KeptTracks {
    tracks: Vec<CachedTrack>,
    generation: TrackGeneration,
}

static TRACKS: Mutex<KeptTracks> = Mutex::new(KeptTracks { tracks: Vec::new(), generation: TrackGeneration(0) });

/// How many walked tracks are kept: the egui popup's own
/// `PREVIEW_CACHE_CAPACITY`, so a reader moving back over the last few rows
/// waits for no app.
const TRACK_CACHE_SIZE: usize = 8;

fn track_generation() -> TrackGeneration {
    TRACKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).generation
}

fn kept_track(key: &TrackKey) -> Option<Arc<WalkedTrack>> {
    let mut kept = TRACKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let at = kept.tracks.iter().position(|held| &held.key == key)?;
    // Moved to the end, so the least recently asked for is the one dropped.
    let entry = kept.tracks.remove(at);
    let track = Arc::clone(&entry.track);
    kept.tracks.push(entry);

    Some(track)
}

/// Keeps `track`, unless the kept tracks were forgotten after it was
/// `walked_in`.
fn keep_track(key: TrackKey, track: Arc<WalkedTrack>, walked_in: TrackGeneration) {
    let mut kept = TRACKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if kept.generation != walked_in {
        return;
    }
    kept.tracks.retain(|held| held.key != key);
    kept.tracks.push(CachedTrack { key, track });
    while kept.tracks.len() > TRACK_CACHE_SIZE {
        kept.tracks.remove(0);
    }
}

/// Whether `path`'s battle is kept walked, so its preview costs only the
/// rasterising.
pub fn is_walked(path: &Path) -> bool {
    let Some(key) = TrackKey::of(path) else { return false };
    TRACKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).tracks.iter().any(|held| held.key == key)
}

/// How long each baked frame is shown. The bake keeps an evenly spaced subset
/// of the battle, so this is a display rate rather than the replay's own
/// clock. Taken from the shared rate the egui popup repaints at, so the front
/// ends play a track at the same speed.
pub const FRAME_INTERVAL: Duration = Duration::from_nanos((1_000_000_000.0 / PREVIEW_FPS) as u64);

/// Why a preview could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    /// The file itself: missing, moved, or a container that will not parse.
    #[error("the replay could not be read: {0}")]
    UnreadableReplay(rootcause::Report<wows_replays::ParseError>),
    /// The packet stream read, but no battle could be walked from it: what a
    /// replay whose game data does not match its packets looks like.
    #[error("the replay's battle could not be walked: {0}")]
    UnwalkableBattle(#[from] wows_battle_world::merged::MergeError),
    #[error("the replay reports an unreadable client version {raw:?}")]
    UnknownBuild { raw: String },
    #[error("this replay's build {version} is not loaded: {reason}")]
    NoGameData { version: String, reason: String },
    #[error("the build ships no map info for {map:?}")]
    NoMapInfo { map: String },
    #[error("the preview was superseded")]
    Cancelled,
    #[error(transparent)]
    Render(#[from] wows_minimap_renderer::preview::PreviewRenderError),
}

/// What a surface says instead of a map: a translation key and the value it
/// takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewMessage {
    pub key: &'static str,
    pub value: String,
}

impl PreviewError {
    /// What a surface says instead of a map, in the egui popup's own words
    /// (`replay/renderer/preview.rs`'s `PreviewError::key`).
    ///
    /// `None` for a superseded bake, which is not something to tell the reader:
    /// another bake is already running for the row they moved to.
    pub fn message(&self) -> Option<PreviewMessage> {
        let (key, value) = match self {
            // The report's own rendering carries the source location, the
            // attached path and every frame under it. That belongs in the log,
            // not in a hover popup, so only the error it wraps is shown.
            Self::UnreadableReplay(err) => ("ui.replay.preview_unreadable_because", err.current_context().to_string()),
            Self::UnwalkableBattle(err) => ("ui.replay.preview_unwalkable", err.to_string()),
            Self::UnknownBuild { raw } => ("ui.replay.preview_unknown_version", raw.clone()),
            Self::NoGameData { version, .. } => ("ui.replay.preview_no_game_data", version.clone()),
            Self::NoMapInfo { map } => ("ui.replay.preview_no_map", map.clone()),
            Self::Render(err) => ("ui.replay.preview_render_failed", err.to_string()),
            Self::Cancelled => return None,
        };

        Some(PreviewMessage { key, value })
    }
}

/// Reads the replay at `path` and plays its preview against its own build.
///
/// The build is loaded through the shared cache, so a replay from a build
/// already open costs nothing extra and one from a build that is not
/// installed simply has no preview. A replay walked recently is not walked
/// again; only its rasterising is repeated.
///
/// `on_map` is handed the map on its own, with nothing drawn over it, as soon
/// as the art is loaded and before the battle is walked, the way the egui
/// popup shows the map rather than an empty box.
///
/// `on_frame` is handed each frame as it is rasterised rather than the track
/// at the end: rasterising a whole track costs seconds, and a preview that
/// waits for all of it shows a still map for the whole time.
///
/// Both are handed square images of the map at `edge`, with the HUD strip
/// cropped off.
pub fn bake_from_file(
    path: &Path,
    game_data: &GameDataCache,
    edge: PreviewEdge,
    cancel: &AtomicBool,
    on_map: impl FnOnce(RgbImage),
    on_frame: impl FnMut(RgbImage),
) -> Result<(), PreviewError> {
    // Noted before anything is read, so a forget at any point in the bake is
    // seen by the check after the walk.
    let started_in = track_generation();
    let key = TrackKey::of(path);
    if let Some(track) = key.as_ref().and_then(kept_track) {
        let loaded = load_build(game_data, track.build, &track.version)?;
        let renderer = renderer_for(
            Some(track.build),
            &track.map_name,
            loaded.vfs(),
            Some(&track.version),
            SidePanelLayout::None,
        )?;
        on_map(map_only(&renderer, edge));
        return rasterise(&renderer, &track.frames, edge, cancel, on_frame);
    }

    let replay = ReplayFile::from_file(path).map_err(PreviewError::UnreadableReplay)?;
    let (version, build) = recorded_build(&replay)?;

    // Before the build: loading one that is not open is seconds of work a
    // preview the pointer has already left should not start.
    if cancel.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }
    let loaded = load_build(game_data, build, &version)?;

    let map_name = replay.meta.mapName.clone();
    let renderer = renderer_for(Some(build), &map_name, loaded.vfs(), Some(&version), SidePanelLayout::None)?;
    on_map(map_only(&renderer, edge));

    // Forgotten while walking: the commands came from a source or language
    // that has since been replaced, and so did `loaded`, so walking again
    // with it would only repeat that. The owner bakes afresh on the next
    // hover, against the cache it holds then.
    let frames = walk(&replay, &loaded, cancel)?;
    if track_generation() != started_in {
        return Err(PreviewError::Cancelled);
    }
    let track = Arc::new(WalkedTrack { frames, build, version, map_name });
    if let Some(key) = key {
        keep_track(key, Arc::clone(&track), started_in);
    }

    rasterise(&renderer, &track.frames, edge, cancel, on_frame)
}

/// A whole battle, ready to be played back.
pub struct BakedTrack {
    /// One entry per kept frame, each the commands that draw it.
    pub frames: Vec<Vec<DrawCommand>>,
    /// The game clock each of those frames was taken at.
    pub clocks: Vec<GameClock>,
    /// The renderer they are drawn through, bound to the build and map the
    /// replay was recorded on.
    pub renderer: SharedPreviewRenderer,
    /// When the battle proper began. A replay starts recording during the
    /// loading screen, so the clock a frame carries runs ahead of the clock
    /// the game showed by this much.
    pub battle_start: GameClock,
    /// When `BattleEnd` arrived, if it did. Absent on a replay that was cut
    /// short.
    pub battle_end: Option<GameClock>,
    /// How wide the map is in the game's own world units, which is what
    /// turns a range in metres into a distance on the minimap.
    pub space_size: f32,
    /// The map's space name, which is what a peer loads art by.
    pub map_name: String,
    /// The build the replay was recorded on, which some of a ship's ranges
    /// are gated on.
    pub version: Version,
    /// Where the map's top-left corner sits in these frames. The layout
    /// decides it, and a side panel moves it.
    pub map_origin: (u32, u32),
}

/// What happened in a battle, and what each ship took.
pub type BattleEvents = (TimelineExtractionResult, std::collections::HashMap<EntityId, ShipShotTimeline>);

/// The events of `path`'s battle, in the order they happened.
///
/// A second walk of the replay rather than part of the bake: a viewport is
/// useful without it, and a reader who never presses the event controls should
/// not wait for it.
pub fn extract_events(path: &Path, game_data: &GameDataCache) -> Result<BattleEvents, PreviewError> {
    let replay = ReplayFile::from_file(path).map_err(PreviewError::UnreadableReplay)?;
    let (version, build) = recorded_build(&replay)?;
    let loaded = load_build(game_data, build, &version)?;

    // One walk for both: the shots come off the same scan as the events, so
    // reading them costs nothing beyond keeping them.
    Ok(wows_replay_insights::timeline::extract_timeline_and_shots(
        &replay,
        loaded.provider(),
        Some(loaded.base_constants()),
    ))
}

/// Walks `path`'s battle once, keeping the draw commands of up to `budget`
/// frames and the renderer they are drawn through.
///
/// The same pass the hover preview makes, kept as commands rather than
/// rasterised: a playback viewport draws one frame at a time and rasterising
/// a whole battle up front would cost gigabytes. `frame_interval` is how much
/// game time one frame covers; `budget` bounds the track, so a long battle is
/// sampled more coarsely rather than costing more.
///
/// `alts` are other recordings of the same battle, read alongside it: the map
/// then shows what the primary's team could not see. They are checked against
/// this battle before they reach a caller (`load::check_alt`).
pub fn bake_track(
    path: &Path,
    alts: &[PathBuf],
    game_data: &GameDataCache,
    cancel: &AtomicBool,
    budget: usize,
    frame_interval: f32,
    options: RenderOptions,
) -> Result<BakedTrack, PreviewError> {
    let replay = ReplayFile::from_file(path).map_err(PreviewError::UnreadableReplay)?;
    let mut merged = Vec::with_capacity(alts.len());
    for alt in alts {
        merged.push(ReplayFile::from_file(alt).map_err(PreviewError::UnreadableReplay)?);
    }
    let (version, build) = recorded_build(&replay)?;
    let loaded = load_build(game_data, build, &version)?;

    let provider = loaded.provider().as_ref();
    let vfs = loaded.vfs();
    let map_name = replay.meta.mapName.clone();
    let map_info =
        assets::load_map_info(&map_name, vfs).ok_or_else(|| PreviewError::NoMapInfo { map: map_name.clone() })?;

    let session_version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    let space_size = map_info.space_size as f32;
    let mut renderer = MinimapRenderer::new(Some(map_info), provider, session_version, options);
    renderer.set_fonts(assets::load_game_fonts(vfs));
    let target = renderer_for(Some(build), &map_name, vfs, Some(&version), SidePanelLayout::None)?;

    let mut session = MergedReplays::new(
        provider.entity_specs(),
        provider,
        loaded.base_constants(),
        session_version,
        &replay,
        &merged,
    )
    .map_err(PreviewError::UnwalkableBattle)?;
    session.world_mut().set_shot_tracking(ShotTracking::Tracked);

    let mut sink = TrackSink::with_budget(budget);
    build_frame_track(&mut session, &mut renderer, frame_interval, cancel, &mut sink);
    let (battle_start, battle_end) = {
        let view = session.world_mut().view();
        // A replay with no BattleStart packet was recorded from the loading
        // screen onwards with nothing to offset against, so its clock is
        // already elapsed time.
        (view.battle_start_clock().unwrap_or(GameClock(0.0)), view.battle_end_clock())
    };
    session.finish();

    if cancel.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }

    let clocks = sink.kept_clocks().to_vec();
    let map_origin = target.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).map_origin();

    Ok(BakedTrack {
        frames: sink.finish(),
        clocks,
        renderer: target,
        battle_start,
        battle_end,
        space_size,
        map_name,
        version,
        map_origin,
    })
}

/// The map `map_name` names, with nothing drawn over it, square at `edge`.
///
/// Rendered from whichever build is already open rather than the one the
/// replay was recorded on: loading that build is the slow half of a preview,
/// and the map is what the hover needs on screen first. `None` when no build
/// is open yet, or when the open one ships no art for that map: a preview
/// recorded on a build with a map since removed then waits for its own bake,
/// which carries the art it was recorded against.
pub fn map_frame(map_name: &str, game_data: &GameDataCache, edge: PreviewEdge) -> Option<RgbImage> {
    let loaded = game_data.newest_loaded()?;
    // The art alone, not a rendered frame of it: building a `PreviewRenderer`
    // reads this build's fonts and six icon sets and then runs a full render
    // pass, none of which draws anything on an empty command slice. The art
    // is the map itself rather than a canvas the renderer drew it into, so
    // there is no HUD strip to crop.
    let art = assets::load_map_image(map_name, loaded.vfs())?;

    Some(scaled(&art, edge))
}

/// Draws one map with `commands` on it, and nothing else.
///
/// What a tactics board and a watched playback show: the same renderer the
/// battle is drawn through, with the board's or the host's commands on it.
/// The battle's HUD strip is cropped off and the art is handed over at its own
/// size, so one pixel of the image is one pixel of the minimap: a board turns a
/// pointer position back into a world one, and a resample would put every press
/// somewhere else.
///
/// `None` when no build is open, or the open one ships no art for that map.
pub fn render_map(
    map_name: &str,
    game_data: &GameDataCache,
    view: MapViewport,
    commands: &[DrawCommand],
) -> Option<RgbImage> {
    let loaded = game_data.newest_loaded()?;
    let renderer = renderer_for(None, map_name, loaded.vfs(), None, SidePanelLayout::None).ok()?;
    let canvas = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).render_at(view, commands);

    Some(map_layer(&canvas))
}

/// The same, over art the caller holds rather than art this build ships.
///
/// What a collab peer draws a shared board or playback with: the art is sent
/// to it, because the reader who opened it may be on a build this one does not
/// have. `held_as` names the entry the built renderer is kept under, and is the
/// board's or playback's own id rather than a map name for that reason.
///
/// `None` when no build is open: the fonts and icons drawn over the art are this
/// build's, and a board with neither is not worth drawing.
pub fn render_map_art(
    held_as: &str,
    art: &RgbImage,
    game_data: &GameDataCache,
    view: MapViewport,
    commands: &[DrawCommand],
) -> Option<RgbImage> {
    let loaded = game_data.newest_loaded()?;
    let renderer = renderer_for_art(held_as, art, loaded.vfs());
    let canvas = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).render_at(view, commands);

    Some(map_layer(&canvas))
}

/// The map layer of a rendered canvas, at its own size.
///
/// The renderer draws the map at `(0, HUD_HEIGHT)` in a canvas that is taller
/// than the map by that strip, so the crop is the strip taken off the top.
pub fn map_layer(canvas: &RgbImage) -> RgbImage {
    let side = MINIMAP_SIZE.min(canvas.width()).min(canvas.height().saturating_sub(HUD_HEIGHT));

    image::imageops::crop_imm(canvas, 0, HUD_HEIGHT, side, side).to_image()
}

/// Walks `replay`'s battle once, keeping the commands of the frames a preview
/// plays.
///
/// One forward pass sampled by the shared [`TrackSink`], so every front end
/// plays the same frames. Baked with the paint preset rather than
/// [`bake_options`](wows_minimap_renderer::frame_track::bake_options): these
/// commands are only ever rasterised through it, so a class it hides is not
/// worth emitting, and the player names the egui popup drops at paint time
/// never reach a ship's command, which this renderer would draw.
fn walk(
    replay: &ReplayFile,
    loaded: &LoadedGameData,
    cancel: &AtomicBool,
) -> Result<Vec<Vec<DrawCommand>>, PreviewError> {
    let provider = loaded.provider().as_ref();
    let vfs = loaded.vfs();
    let map_name = &replay.meta.mapName;
    let map_info =
        assets::load_map_info(map_name, vfs).ok_or_else(|| PreviewError::NoMapInfo { map: map_name.clone() })?;

    let session_version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    let mut renderer = MinimapRenderer::new(Some(map_info), provider, session_version, paint_options());
    renderer.set_fonts(assets::load_game_fonts(vfs));

    let mut session =
        MergedReplays::new(provider.entity_specs(), provider, loaded.base_constants(), session_version, replay, &[])
            .map_err(PreviewError::UnwalkableBattle)?;
    // Tracked, not Untracked: the tracer commands come from `active_shots()`,
    // which stays empty unless shot recording is on.
    session.world_mut().set_shot_tracking(ShotTracking::Tracked);

    let mut sink = TrackSink::new();
    build_frame_track(&mut session, &mut renderer, 1.0 / SNAPSHOTS_PER_SECOND, cancel, &mut sink);
    session.finish();

    if cancel.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }

    Ok(sink.finish())
}

/// Rasterises `frames` one at a time through the paint preset, handing each
/// to `on_frame` as it is done.
///
/// The renderer is locked per frame rather than for the track: it is shared
/// with every other preview of the map and with the playback viewport, which
/// would otherwise wait out the whole track.
fn rasterise(
    renderer: &SharedPreviewRenderer,
    frames: &[Vec<DrawCommand>],
    edge: PreviewEdge,
    cancel: &AtomicBool,
    mut on_frame: impl FnMut(RgbImage),
) -> Result<(), PreviewError> {
    let options = paint_options();
    let edge = edge.for_track(frames.len());
    for commands in frames {
        if cancel.load(Ordering::Relaxed) {
            return Err(PreviewError::Cancelled);
        }
        // Dead ships are shown, as the egui popup shows them.
        let painted = commands.iter().filter(|command| should_draw_command(command, &options, true));
        let canvas = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).render(painted);
        on_frame(preview_image(&canvas, edge));
    }

    Ok(())
}

/// The map `renderer` draws, with nothing over it.
fn map_only(renderer: &SharedPreviewRenderer, edge: PreviewEdge) -> RgbImage {
    let canvas = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).render(std::iter::empty());

    preview_image(&canvas, edge)
}

/// The edge length, in logical pixels, a preview is drawn at.
pub const PREVIEW_PX: u32 = 384;

/// The edge length, in physical pixels, a preview's frames are rasterised at.
///
/// [`PREVIEW_PX`] times the display's scale factor, so one image pixel lands on
/// one screen pixel and a frame is not resampled again when it is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewEdge(u32);

impl PreviewEdge {
    pub fn for_scale_factor(scale_factor: f32) -> Self {
        let edge = (PREVIEW_PX as f32 * scale_factor).round() as u32;
        // Capped at the canvas the renderer composes: past it a frame would
        // only be enlarged.
        Self(edge.clamp(1, MINIMAP_SIZE))
    }

    /// The largest edge, no larger than this one, at which a track of `frames`
    /// frames fits the decoded budget; never below [`PREVIEW_PX`].
    ///
    /// A track's frames stay decoded for as long as it plays, so the budget
    /// holds every track to what a full one costs at scale one, whatever the
    /// display: a short battle is drawn at the display's own size, a long one
    /// on a dense display smaller, and enlarged when it is painted.
    pub fn for_track(self, frames: usize) -> Self {
        const BUDGET_BYTES: u64 = PREVIEW_MAX_FRAMES as u64 * PREVIEW_PX as u64 * PREVIEW_PX as u64 * 4;

        let pixels_per_frame = BUDGET_BYTES / 4 / frames.max(1) as u64;
        let fitting = u32::try_from(pixels_per_frame.isqrt()).unwrap_or(u32::MAX);

        Self(self.0.min(fitting.max(PREVIEW_PX)))
    }

    pub fn pixels(self) -> u32 {
        self.0
    }
}

/// A rendered preview: every frame, decoded, in whatever form a front end
/// paints.
pub struct PreviewFrames<T> {
    frames: Vec<T>,
    /// Whether every frame of the track is here. An unfinished track plays
    /// forward and holds on its newest frame; looping a track that is two
    /// frames long so far reads as a stutter, not as playback.
    complete: bool,
}

impl<T> PreviewFrames<T>
where
    T: Clone,
{
    /// An empty track, for a preview whose frames are still arriving.
    pub fn streaming() -> Self {
        Self { frames: Vec::new(), complete: false }
    }

    /// A finished track of one frame: the still map.
    pub fn still(frame: T) -> Self {
        Self { frames: vec![frame], complete: true }
    }

    /// Adopts one more rasterised frame.
    pub fn push(&mut self, frame: T) {
        self.frames.push(frame);
    }

    /// Says every frame has arrived, so playback loops.
    pub fn finish(&mut self) {
        self.complete = true;
    }

    /// Whether every frame of the track has been rasterised.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// How many frames have been rasterised so far.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Every frame, for a caller releasing the track's textures.
    pub fn images(self) -> Vec<T> {
        self.frames
    }

    /// Which frame to show `elapsed` into the preview, looping.
    ///
    /// `None` only when there are no frames at all, which is a track that
    /// baked nothing.
    pub fn index_at(&self, elapsed: Duration) -> Option<usize> {
        if self.frames.is_empty() {
            return None;
        }
        let step = (elapsed.as_millis() / FRAME_INTERVAL.as_millis().max(1)) as usize;
        let index = if self.complete { step % self.frames.len() } else { step.min(self.frames.len() - 1) };

        Some(index)
    }

    /// The frame to show `elapsed` into the preview, looping.
    pub fn at(&self, elapsed: Duration) -> Option<T> {
        self.frames.get(self.index_at(elapsed)?).cloned()
    }
}

/// The map out of a rendered canvas, square at `edge`.
///
/// The renderer draws the map at `(0, HUD_HEIGHT)` in a canvas taller than the
/// map by that strip, so the strip is cropped off: scaling the canvas whole
/// would squash the map, since the canvas is not square and the preview is.
pub fn preview_image(canvas: &RgbImage, edge: PreviewEdge) -> RgbImage {
    let side = MINIMAP_SIZE.min(canvas.width()).min(canvas.height().saturating_sub(HUD_HEIGHT));
    let map = image::imageops::crop_imm(canvas, 0, HUD_HEIGHT, side, side);

    scaled(&*map, edge)
}

/// `map` scaled to `edge`.
///
/// Lanczos rather than a triangle filter: on a scale-one display the renderer
/// composes at twice the drawn size, and a triangle filter halves that into a
/// blur over every name and icon edge.
fn scaled(map: &impl GenericImageView<Pixel = Rgb<u8>>, edge: PreviewEdge) -> RgbImage {
    let edge = edge.pixels();
    if map.dimensions() == (edge, edge) {
        let mut copy = RgbImage::new(edge, edge);
        for (x, y, pixel) in map.pixels() {
            copy.put_pixel(x, y, pixel);
        }
        return copy;
    }

    image::imageops::resize(map, edge, edge, image::imageops::FilterType::Lanczos3)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::SystemTime;

    use super::*;

    fn frames(count: usize) -> PreviewFrames<usize> {
        let mut track = PreviewFrames::streaming();
        for index in 0..count {
            track.push(index);
        }
        track.finish();
        track
    }

    /// A track still being rasterised plays forward and holds on its newest
    /// frame; a finished one loops.
    #[test]
    fn an_unfinished_track_holds_on_its_newest_frame_and_a_finished_one_loops() {
        let mut track = PreviewFrames::streaming();
        track.push(0);
        track.push(1);

        let past_the_end = FRAME_INTERVAL * 5;
        assert_eq!(track.at(past_the_end), Some(1), "an unfinished track holds rather than showing nothing");
        assert!(!track.is_complete());

        track.finish();
        assert_eq!(track.at(past_the_end), Some(1), "a finished track loops: five steps over two frames");
        assert_eq!(track.at(FRAME_INTERVAL * 4), Some(0));
    }

    /// A frame is sized for the display's scale factor, so it lands one image
    /// pixel per screen pixel instead of being resampled again when painted.
    #[test]
    fn a_frame_is_scaled_to_the_physical_drawn_size() {
        use wows_minimap_renderer::CANVAS_HEIGHT;

        let rendered = RgbImage::from_pixel(MINIMAP_SIZE, CANVAS_HEIGHT, Rgb([9, 9, 9]));
        let edge_at = |scale_factor: f32| {
            let image = preview_image(&rendered, PreviewEdge::for_scale_factor(scale_factor));
            assert_eq!(image.width(), image.height(), "the preview is square");
            image.width()
        };

        assert_eq!(edge_at(1.0), PREVIEW_PX);
        assert_eq!(edge_at(1.5), PREVIEW_PX * 3 / 2);
        assert_eq!(edge_at(2.0), MINIMAP_SIZE);
        assert_eq!(edge_at(3.0), MINIMAP_SIZE, "never enlarged past the canvas the renderer composes");
    }

    /// The HUD strip above the map is cropped off, not squeezed into the square.
    #[test]
    fn a_frame_shows_the_map_without_the_hud_strip() {
        use wows_minimap_renderer::CANVAS_HEIGHT;

        let hud = Rgb([255, 0, 0]);
        let map = Rgb([0, 0, 255]);
        let rendered = RgbImage::from_fn(MINIMAP_SIZE, CANVAS_HEIGHT, |_, y| if y < HUD_HEIGHT { hud } else { map });

        let image = preview_image(&rendered, PreviewEdge::for_scale_factor(2.0));

        assert!(image.pixels().all(|pixel| *pixel == map), "every pixel is the map's blue");
        assert!(map_layer(&rendered).pixels().all(|pixel| *pixel == map), "and so is the map layer at its own size");
    }

    #[test]
    fn a_track_with_no_frames_shows_nothing() {
        let empty = frames(0);
        assert_eq!(empty.len(), 0);
        assert!(empty.at(Duration::ZERO).is_none());
    }

    /// Each frame is held for the display interval, and the track loops
    /// rather than stopping on its last frame.
    #[test]
    fn frames_advance_on_the_interval_and_then_loop() {
        let track = frames(3);

        assert_eq!(track.at(Duration::ZERO), Some(0));
        assert_eq!(track.at(FRAME_INTERVAL / 2), Some(0), "a frame is held for the whole interval");
        assert_eq!(track.at(FRAME_INTERVAL), Some(1));
        assert_eq!(track.at(FRAME_INTERVAL * 2), Some(2));
        assert_eq!(track.at(FRAME_INTERVAL * 3), Some(0), "and it loops");
        assert_eq!(track.at(FRAME_INTERVAL * 7), Some(1));
    }

    /// A walked battle is kept for the next hover of its row, until its file
    /// is replaced, enough other battles have been walked since, or the kept
    /// tracks are forgotten for a change of language.
    #[test]
    fn a_walked_track_is_kept_until_its_file_changes_it_is_evicted_or_forgotten() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        let replay = |name: &str| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"replay").expect("the file can be written");
            path
        };
        let keep_walked_in = |path: &Path, walked_in: TrackGeneration| {
            let track = WalkedTrack {
                frames: Vec::new(),
                build: NonZeroU32::new(1).expect("nonzero"),
                version: Version::from_client_exe("0,15,7,0"),
                map_name: String::new(),
            };
            let key = TrackKey::of(path).expect("the file has a modification time");
            keep_track(key, Arc::new(track), walked_in);
        };
        let keep = |path: &Path| keep_walked_in(path, track_generation());

        let replaced = replay("replaced.wowsreplay");
        keep(&replaced);
        assert!(is_walked(&replaced));
        std::fs::File::options()
            .write(true)
            .open(&replaced)
            .and_then(|file| file.set_modified(SystemTime::now() + Duration::from_secs(60)))
            .expect("the modification time can be moved");
        assert!(!is_walked(&replaced), "a file replaced in place is walked again");

        let oldest = replay("oldest.wowsreplay");
        keep(&oldest);
        for index in 0..TRACK_CACHE_SIZE {
            keep(&replay(&format!("{index}.wowsreplay")));
        }
        assert!(!is_walked(&oldest), "the battle walked longest ago makes room");
        let newest = dir.path().join("0.wowsreplay");
        assert!(is_walked(&newest));

        let in_flight = replay("in_flight.wowsreplay");
        let walked_in = track_generation();
        forget_kept_previews();
        assert!(!is_walked(&newest), "a change of language forgets every kept track");
        keep_walked_in(&in_flight, walked_in);
        assert!(!is_walked(&in_flight), "and a walk begun before it is not kept");
    }

    /// A track's decoded frames cost no more than a full track does at scale
    /// one, whatever the display: a full track on a scale-two display would
    /// otherwise hold four times that.
    #[test]
    fn a_tracks_decoded_frames_stay_within_the_scale_one_footprint() {
        let bytes = |frames: usize, edge: u32| frames as u64 * u64::from(edge) * u64::from(edge) * 4;
        let scale_one = bytes(PREVIEW_MAX_FRAMES, PREVIEW_PX);
        let dense = PreviewEdge::for_scale_factor(2.0);

        let full = dense.for_track(PREVIEW_MAX_FRAMES).pixels();
        assert!(bytes(PREVIEW_MAX_FRAMES, full) <= scale_one, "{full}px frames exceed the budget");
        assert_eq!(full, PREVIEW_PX, "and are never drawn below the preview's own size");

        let short = dense.for_track(PREVIEW_MAX_FRAMES / 4).pixels();
        assert_eq!(short, MINIMAP_SIZE, "a short battle fits at the display's own size");
        assert!(bytes(PREVIEW_MAX_FRAMES / 4, short) <= scale_one);

        let between = dense.for_track(PREVIEW_MAX_FRAMES / 2).pixels();
        assert!(between > PREVIEW_PX && between < MINIMAP_SIZE, "a middling one takes what fits: {between}px");
        assert!(bytes(PREVIEW_MAX_FRAMES / 2, between) <= scale_one);
    }
}
