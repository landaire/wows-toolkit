//! The minimap hover preview: a replay's battle played back as a small
//! animation while the pointer rests on its row.
//!
//! When the preview appears is [`wows_toolkit_viewmodel::preview_dwell`],
//! shared with the egui app so both wait the same amount. What it draws is
//! [`wows_minimap_renderer::preview`], the same command set the egui popup
//! paints and the video export encodes, rasterised to an image here because
//! this front end has no painter of its own.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use gpui_kit::RenderImage;
use image::RgbImage;
use wows_battle_world::ids::ShotTracking;
use wows_battle_world::merged::MergedReplays;
use wows_minimap_renderer::assets;
use wows_minimap_renderer::config::RenderOptions;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::frame_track::PREVIEW_FPS;
use wows_minimap_renderer::frame_track::SNAPSHOTS_PER_SECOND;
use wows_minimap_renderer::frame_track::TrackSink;
use wows_minimap_renderer::frame_track::bake_options;
use wows_minimap_renderer::frame_track::build_frame_track;
use wows_minimap_renderer::preview::PreviewRenderer;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_replay_insights::timeline::TimelineExtractionResult;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::VfsPath;

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
    map: String,
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

/// The renderer for `map_name` on `build`, built once and kept.
///
/// `build` is `None` for a caller drawing against whichever build happens to
/// be open ([`map_frame`]), which keys its own entry rather than claiming a
/// build's.
fn renderer_for(
    build: Option<NonZeroU32>,
    map_name: &str,
    vfs: &VfsPath,
    version: Option<&Version>,
) -> Result<SharedPreviewRenderer, PreviewError> {
    let key = RendererKey { build, map: map_name.to_owned() };
    {
        let mut cache = RENDERERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(at) = cache.iter().position(|held| held.key == key) {
            // Moved to the end, so the least recently asked for is the one
            // dropped when the cache is full.
            let entry = cache.remove(at);
            let renderer = Arc::clone(&entry.renderer);
            cache.push(entry);
            return Ok(renderer);
        }
    }

    // Built outside the lock: reading a build's art takes long enough that
    // holding the cache through it would stall every other preview. Two
    // callers racing on the same map build it twice and the second wins,
    // which costs one extra read rather than a stall.
    let renderer: SharedPreviewRenderer = Arc::new(Mutex::new(PreviewRenderer::new(vfs, version, map_name)?));

    let mut cache = RENDERERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|held| held.key != key);
    cache.push(CachedRenderer { key, renderer: Arc::clone(&renderer) });
    while cache.len() > RENDERER_CACHE_SIZE {
        cache.remove(0);
    }
    Ok(renderer)
}

/// Drops every kept renderer, for a caller that would rather have the memory
/// back than the next preview's speed.
pub fn forget_renderers() {
    RENDERERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
}

/// How long each baked frame is shown. The bake keeps an evenly spaced subset
/// of the battle, so this is a display rate rather than the replay's own
/// clock. Taken from the shared rate the egui popup repaints at, so the two
/// play a track at the same speed.
pub const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_nanos((1_000_000_000.0 / PREVIEW_FPS) as u64);

/// Why a preview could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("the replay could not be read")]
    UnreadableReplay,
    #[error("the replay reports an unreadable client version {raw:?}")]
    UnknownBuild { raw: String },
    #[error("this replay's build is not loaded: {reason}")]
    NoGameData { reason: String },
    #[error("the build ships no map info for {map:?}")]
    NoMapInfo { map: String },
    #[error("the preview was superseded")]
    Cancelled,
    #[error(transparent)]
    Render(#[from] wows_minimap_renderer::preview::PreviewRenderError),
}

/// Reads the replay at `path` and bakes its preview against its own build.
///
/// The build is loaded through the shared cache, so a replay from a build
/// already open costs nothing extra and one from a build that is not
/// installed simply has no preview.
///
/// `on_map` is handed the map on its own, with nothing drawn over it, as soon
/// as the art is loaded and before the battle is walked: the walk is the
/// seconds of this, and the egui popup shows the map for all of them rather
/// than an empty box (`ui/replay_parser/preview_popup.rs`).
pub fn bake_from_file(
    path: &std::path::Path,
    game_data: &crate::replay_inspector::GameDataCache,
    cancel: &AtomicBool,
    on_map: impl FnOnce(PreviewFrames),
    on_frame: impl FnMut(Arc<RenderImage>),
) -> Result<(), PreviewError> {
    let read_at = std::time::Instant::now();
    let replay = ReplayFile::from_file(path).map_err(|_| PreviewError::UnreadableReplay)?;
    tracing::debug!("preview: replay read in {:?}", read_at.elapsed());
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
        .ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;

    // A replay whose header carries no build number names no build to load.
    let build =
        version.build.ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;
    let build_at = std::time::Instant::now();
    let loaded =
        game_data.get_or_load_build(build.get()).map_err(|err| PreviewError::NoGameData { reason: err.to_string() })?;
    tracing::debug!("preview: build {} ready in {:?}", build.get(), build_at.elapsed());

    bake(&replay, loaded.provider(), loaded.base_constants(), loaded.vfs(), Some(&version), cancel, on_map, on_frame)
}

/// A battle walked once: every kept frame's draw commands, the clock each was
/// drawn at, and the renderer they are rasterised through.
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
    /// Where the map's top-left corner sits in these frames. The layout
    /// decides it, and a side panel moves it.
    pub map_origin: (u32, u32),
}

/// The events of `path`'s battle, in the order they happened.
///
/// A second walk of the replay rather than part of the bake: the viewport is
/// useful without it, and a reader who never presses the event controls should
/// not wait for it.
pub fn extract_events(
    path: &std::path::Path,
    game_data: &crate::replay_inspector::GameDataCache,
) -> Result<TimelineExtractionResult, PreviewError> {
    let replay = ReplayFile::from_file(path).map_err(|_| PreviewError::UnreadableReplay)?;
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
        .ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;
    let build =
        version.build.ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;
    let loaded =
        game_data.get_or_load_build(build.get()).map_err(|err| PreviewError::NoGameData { reason: err.to_string() })?;

    Ok(wows_replay_insights::timeline::extract_timeline_events(
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
pub fn bake_track(
    path: &std::path::Path,
    game_data: &crate::replay_inspector::GameDataCache,
    cancel: &AtomicBool,
    budget: usize,
    frame_interval: f32,
    options: RenderOptions,
) -> Result<BakedTrack, PreviewError> {
    let replay = ReplayFile::from_file(path).map_err(|_| PreviewError::UnreadableReplay)?;
    let version = Version::try_from_client_exe(&replay.meta.clientVersionFromExe)
        .ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;
    let build =
        version.build.ok_or_else(|| PreviewError::UnknownBuild { raw: replay.meta.clientVersionFromExe.clone() })?;
    let loaded =
        game_data.get_or_load_build(build.get()).map_err(|err| PreviewError::NoGameData { reason: err.to_string() })?;

    let provider = loaded.provider().as_ref();
    let vfs = loaded.vfs();
    let map_name = replay.meta.mapName.clone();
    let map_info =
        assets::load_map_info(&map_name, vfs).ok_or_else(|| PreviewError::NoMapInfo { map: map_name.clone() })?;

    let session_version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    let mut renderer = MinimapRenderer::new(Some(map_info), provider, session_version, options);
    renderer.set_fonts(assets::load_game_fonts(vfs));
    let target = renderer_for(Some(build), &map_name, vfs, Some(&version))?;

    let mut session =
        MergedReplays::new(provider.entity_specs(), provider, loaded.base_constants(), session_version, &replay, &[])
            .map_err(|_| PreviewError::UnreadableReplay)?;
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

    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }

    let clocks = sink.kept_clocks().to_vec();
    let map_origin = target.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).map_origin();
    Ok(BakedTrack { frames: sink.finish(), clocks, renderer: target, battle_start, battle_end, map_origin })
}

/// The map `map_name` names, with nothing drawn over it.
///
/// Rendered from whichever build is already open rather than the one the
/// replay was recorded on: loading that build is the slow half of a preview,
/// and the map is what the hover needs on screen first. `None` when no build
/// is open yet, or when the open one ships no art for that map -- a preview
/// that was recorded on a build with a map since removed then waits for its
/// own bake, which carries the art it was recorded against.
pub fn map_frame(map_name: &str, game_data: &crate::replay_inspector::GameDataCache) -> Option<PreviewFrames> {
    let loaded = game_data.newest_loaded()?;
    let renderer = renderer_for(None, map_name, loaded.vfs(), None).ok()?;
    let nothing_drawn: Vec<DrawCommand> = Vec::new();
    let mut renderer = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Some(PreviewFrames::render(&mut renderer, std::slice::from_ref(&nothing_drawn)))
}

/// Bakes `replay` into the frames a preview plays, rasterising them one at a
/// time.
///
/// One forward pass over the battle, sampled by the shared [`TrackSink`], so
/// the port shows the same track the egui app does. `cancel` is checked
/// between the phases that cost anything: a preview the pointer has already
/// left should not finish parsing a replay.
///
/// `on_frame` is handed each frame as it is rasterised rather than the track
/// at the end: rasterising a whole track costs seconds, and a preview that
/// waits for all of it shows a still map for the whole time.
#[allow(clippy::too_many_arguments)]
pub fn bake(
    replay: &ReplayFile,
    provider: &GameMetadataProvider,
    constants: &GameConstants,
    vfs: &VfsPath,
    version: Option<&Version>,
    cancel: &AtomicBool,
    on_map: impl FnOnce(PreviewFrames),
    mut on_frame: impl FnMut(Arc<RenderImage>),
) -> Result<(), PreviewError> {
    let cancelled = || cancel.load(std::sync::atomic::Ordering::Relaxed);
    if cancelled() {
        return Err(PreviewError::Cancelled);
    }

    let map_name = replay.meta.mapName.clone();
    let map_info =
        assets::load_map_info(&map_name, vfs).ok_or_else(|| PreviewError::NoMapInfo { map: map_name.clone() })?;

    let session_version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    let started = std::time::Instant::now();
    let mut renderer = MinimapRenderer::new(Some(map_info), provider, session_version, bake_options());
    renderer.set_fonts(assets::load_game_fonts(vfs));
    tracing::debug!("preview: fonts and renderer in {:?}", started.elapsed());

    // Built before the battle is walked so the map can be shown during it; it
    // is the same renderer the track is rasterised with afterwards.
    let assets_at = std::time::Instant::now();
    let preview = renderer_for(version.and_then(|version| version.build), &map_name, vfs, version)?;
    let mut preview = preview.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    tracing::debug!("preview: art for {map_name} in {:?}", assets_at.elapsed());
    let nothing_drawn: Vec<DrawCommand> = Vec::new();
    let map_at = std::time::Instant::now();
    on_map(PreviewFrames::render(&mut preview, std::slice::from_ref(&nothing_drawn)));
    tracing::debug!("preview: map frame in {:?}", map_at.elapsed());

    let mut session = MergedReplays::new(provider.entity_specs(), provider, constants, session_version, replay, &[])
        .map_err(|_| PreviewError::UnreadableReplay)?;
    // Tracked, not Untracked: the tracer commands come from `active_shots()`,
    // which stays empty unless shot recording is on.
    session.world_mut().set_shot_tracking(ShotTracking::Tracked);

    let mut sink = TrackSink::new();
    let walk_at = std::time::Instant::now();
    build_frame_track(&mut session, &mut renderer, 1.0 / SNAPSHOTS_PER_SECOND, cancel, &mut sink);
    session.finish();
    tracing::debug!("preview: battle walked in {:?}", walk_at.elapsed());

    if cancelled() {
        return Err(PreviewError::Cancelled);
    }

    let raster_at = std::time::Instant::now();
    let mut first = true;
    for commands in sink.finish() {
        if cancelled() {
            return Err(PreviewError::Cancelled);
        }
        on_frame(to_image(preview.render(&commands)));
        if first {
            tracing::debug!("preview: first animated frame in {:?}", raster_at.elapsed());
            first = false;
        }
    }
    tracing::debug!("preview: whole track in {:?}", raster_at.elapsed());
    Ok(())
}

/// The edge length a preview's frames are rasterised and drawn at.
///
/// The renderer composes at the minimap's own resolution; a preview is shown
/// far smaller than that, and keeping a whole track at full size is both the
/// memory and the decode cost that makes the playback stutter.
pub const PREVIEW_PX: u32 = 384;

/// A rendered preview: every frame as an image gpui can draw.
///
/// Decoded, not encoded: a `gpui::Image` carries compressed bytes that gpui
/// decodes when it paints and caches by id, and a track is far more frames
/// than that cache holds, so every frame of the loop would decode again --
/// and paint nothing until it finished.
pub struct PreviewFrames {
    frames: Vec<Arc<RenderImage>>,
    /// Whether every frame of the track is here. An unfinished track plays
    /// forward and holds on its newest frame; looping a track that is two
    /// frames long so far reads as a stutter, not as playback.
    complete: bool,
}

impl PreviewFrames {
    /// Rasterises every frame of a baked track.
    ///
    /// Done once per preview rather than per paint: the frames are small and
    /// few, and re-rasterising on every frame of the *UI* would redraw the
    /// whole map for each one.
    pub fn render(renderer: &mut PreviewRenderer, track: &[Vec<DrawCommand>]) -> Self {
        let frames = track.iter().map(|commands| to_image(renderer.render(commands))).collect();
        Self { frames, complete: true }
    }

    /// An empty track, for a preview whose frames are still arriving.
    pub fn streaming() -> Self {
        Self { frames: Vec::new(), complete: false }
    }

    /// Adopts one more rasterised frame.
    pub fn push(&mut self, frame: Arc<RenderImage>) {
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

    /// How many frames the bake produced. Only the tests and the Search
    /// tab's own test accessor read this back; what plays is decided by
    /// [`Self::at`].
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Every frame, for a caller releasing the track's textures.
    pub fn images(self) -> Vec<Arc<RenderImage>> {
        self.frames
    }

    /// The frame to show `elapsed` into the preview, looping.
    ///
    /// `None` only when there are no frames at all, which is a track that
    /// baked nothing.
    pub fn at(&self, elapsed: std::time::Duration) -> Option<Arc<RenderImage>> {
        if self.frames.is_empty() {
            return None;
        }
        let step = (elapsed.as_millis() / FRAME_INTERVAL.as_millis().max(1)) as usize;
        let index = if self.complete { step % self.frames.len() } else { step.min(self.frames.len() - 1) };
        self.frames.get(index).cloned()
    }
}

/// An RGB frame as an image gpui can paint straight from.
///
/// Scaled to the size it is drawn at, then handed over as BGRA, which is the
/// layout `RenderImage` stores. The renderer composes every frame over opaque
/// map art, so the alpha channel added here is fully opaque by construction.
fn to_image(frame: RgbImage) -> Arc<RenderImage> {
    let frame = image::imageops::resize(&frame, PREVIEW_PX, PREVIEW_PX, image::imageops::FilterType::Triangle);
    let mut bgra = Vec::with_capacity((PREVIEW_PX * PREVIEW_PX * 4) as usize);
    for pixel in frame.pixels() {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let buffer = image::RgbaImage::from_raw(PREVIEW_PX, PREVIEW_PX, bgra)
        .expect("the buffer is four bytes per pixel of the size it was built at");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

#[cfg(test)]
mod tests {
    use super::FRAME_INTERVAL;
    use super::PreviewFrames;
    use std::sync::Arc;
    use std::time::Duration;

    fn frames(count: usize) -> PreviewFrames {
        // The images themselves are irrelevant to the timing rule; a one
        // pixel frame stands in for a rendered one.
        PreviewFrames {
            frames: (0..count)
                .map(|_| {
                    let buffer = image::RgbaImage::from_raw(1, 1, vec![0, 0, 0, 255]).expect("one pixel");
                    Arc::new(gpui_kit::RenderImage::new(vec![image::Frame::new(buffer)]))
                })
                .collect(),
            complete: true,
        }
    }

    /// A track still being rasterised plays forward and holds on its newest
    /// frame; a finished one loops.
    #[test]
    fn an_unfinished_track_holds_on_its_newest_frame_and_a_finished_one_loops() {
        let mut track = PreviewFrames::streaming();
        let one = frames(1);
        track.push(one.at(Duration::ZERO).expect("one frame"));
        track.push(frames(1).at(Duration::ZERO).expect("one frame"));

        // Two frames in, well past the end of what has been rasterised.
        let past_the_end = FRAME_INTERVAL * 5;
        assert!(track.at(past_the_end).is_some(), "an unfinished track holds rather than showing nothing");
        assert!(!track.is_complete());

        track.finish();
        assert!(track.is_complete(), "a finished track loops");
    }

    /// A frame is handed over decoded and at the size it is drawn: gpui
    /// decodes an encoded image when it paints and caches it by id, and a
    /// track holds more frames than that cache does, so an encoded track
    /// would decode a frame afresh on every pass of the loop.
    #[test]
    fn a_frame_is_scaled_to_the_drawn_size_and_stored_as_pixels() {
        let rendered = image::RgbImage::from_pixel(super::PREVIEW_PX * 2, super::PREVIEW_PX * 2, image::Rgb([9, 9, 9]));

        let image = super::to_image(rendered);

        let size = image.size(0);
        assert_eq!((size.width.0 as u32, size.height.0 as u32), (super::PREVIEW_PX, super::PREVIEW_PX));
        assert_eq!(
            image.as_bytes(0).expect("frame 0").len(),
            (super::PREVIEW_PX * super::PREVIEW_PX * 4) as usize,
            "four bytes per pixel, ready to upload"
        );
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
        let shown = |elapsed: Duration| {
            let frame = track.at(elapsed).expect("a frame");
            track.frames.iter().position(|candidate| Arc::ptr_eq(candidate, &frame)).expect("one of ours")
        };

        assert_eq!(shown(Duration::ZERO), 0);
        assert_eq!(shown(FRAME_INTERVAL / 2), 0, "a frame is held for the whole interval");
        assert_eq!(shown(FRAME_INTERVAL), 1);
        assert_eq!(shown(FRAME_INTERVAL * 2), 2);
        assert_eq!(shown(FRAME_INTERVAL * 3), 0, "and it loops");
        assert_eq!(shown(FRAME_INTERVAL * 7), 1);
    }
}
