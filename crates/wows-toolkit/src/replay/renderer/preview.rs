use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;

use wows_battle_world::ids::ShotTracking;
use wows_battle_world::merged::MergedReplays;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::renderer::RenderOptions;
use wows_replays::ReplayFile;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use super::RendererAssetCache;
use super::SNAPSHOTS_PER_SECOND;
use super::frame_pass::FrameSink;
use super::frame_pass::build_frame_track;
use crate::data::wows_data::WoWsDataMap;

/// Wall-clock length of one full preview loop for a replay long enough to
/// fill the frame budget.
pub(crate) const PREVIEW_LOOP_SECS: f32 = 10.0;
/// Display rate the popup advances the track at.
pub(crate) const PREVIEW_FPS: f32 = 15.0;
/// Frames retained per track. Battles shorter than
/// `PREVIEW_MAX_FRAMES / SNAPSHOTS_PER_SECOND` seconds keep fewer and loop
/// proportionally sooner, which is correct: a one-minute battle should not be
/// stretched to ten seconds.
pub(crate) const PREVIEW_MAX_FRAMES: usize = (PREVIEW_LOOP_SECS * PREVIEW_FPS) as usize;

/// A decimated command track for one replay, played on loop by the inspector
/// hover popup.
pub(crate) struct PreviewTrack {
    pub frames: Vec<Vec<DrawCommand>>,
    pub map_name: String,
}

/// Retains a bounded, evenly spaced subset of the frames it is fed.
pub(crate) struct TrackSink {
    frames: Vec<Vec<DrawCommand>>,
    clocks: Vec<GameClock>,
    /// Keep one frame in every `stride`. Doubles each time the budget fills.
    stride: usize,
    /// Source frames seen since the last frame was retained.
    since_kept: usize,
}

impl TrackSink {
    pub(crate) fn new() -> Self {
        Self { frames: Vec::with_capacity(PREVIEW_MAX_FRAMES * 2), clocks: Vec::new(), stride: 1, since_kept: 0 }
    }

    pub(crate) fn stride(&self) -> usize {
        self.stride
    }

    pub(crate) fn kept_clocks(&self) -> &[GameClock] {
        &self.clocks
    }

    pub(crate) fn finish(self, map_name: String) -> PreviewTrack {
        PreviewTrack { frames: self.frames, map_name }
    }

    /// Drop every other retained frame, halving the track in place.
    fn halve(&mut self) {
        let mut keep = false;
        self.frames.retain(|_| {
            keep = !keep;
            keep
        });
        let mut keep = false;
        self.clocks.retain(|_| {
            keep = !keep;
            keep
        });
        self.stride *= 2;
    }
}

impl FrameSink for TrackSink {
    fn push(&mut self, _index: usize, clock: GameClock, commands: Vec<DrawCommand>) {
        if self.since_kept % self.stride == 0 {
            self.frames.push(commands);
            self.clocks.push(clock);
            if self.frames.len() > PREVIEW_MAX_FRAMES {
                self.halve();
            }
        }
        self.since_kept += 1;
    }
}

/// Why a replay has no preview.
///
/// Each variant carries what the popup needs to say, so the popup never parses
/// a formatted message back apart.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum PreviewError {
    // `Version` is not guaranteed to implement `Display`; format it via Debug
    // rather than adding an impl to `wowsunpack` for one message.
    #[error("no game data for client version {version:?}")]
    NoGameDataForVersion { version: Version },
    #[error("replay could not be read")]
    UnreadableReplay,
    #[error("replay reports an unreadable client version {raw:?}")]
    UnknownClientVersion { raw: String },
    #[error("no map info for {map_name}")]
    NoMapInfo { map_name: String },
    #[error("preview bake was superseded")]
    Cancelled,
}

/// Bake a decimated preview track for `path` in a single forward pass.
///
/// Mirrors the setup `playback_thread` does before its own forward pass, then
/// skips everything else that thread does afterward: damage-event gathering,
/// timeline/shot extraction, salvo flight-time scanning, player-build
/// snapshotting, the live-session rebuild, silhouette loading,
/// `commands.scheme.xml` parsing, and the collab announce. That is the entire
/// reason this finishes cheaply enough to run on hover.
pub(crate) fn bake_preview_track(
    path: &Path,
    data_map: &WoWsDataMap,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    cancel: &AtomicBool,
) -> Result<PreviewTrack, PreviewError> {
    let replay_file = ReplayFile::from_file(path).map_err(|_| PreviewError::UnreadableReplay)?;
    let raw_client_version = replay_file.meta.clientVersionFromExe.clone();
    let replay_version = Version::try_from_client_exe(&raw_client_version)
        .ok_or(PreviewError::UnknownClientVersion { raw: raw_client_version })?;
    let wows_data =
        data_map.resolve(&replay_version).ok_or(PreviewError::NoGameDataForVersion { version: replay_version })?;

    let map_name = replay_file.meta.mapName.clone();
    let (vfs, version, game_metadata, game_constants, dump_dir) = {
        let data = wows_data.read();
        let gm = data.game_metadata.clone().ok_or(PreviewError::NoGameDataForVersion { version: replay_version })?;
        (data.vfs.clone(), data.version().copied(), gm, Arc::clone(&data.game_constants), data.dump_dir.clone())
    };

    let (_map_image, map_info) = asset_cache.lock().get_or_load_map(&map_name, &vfs, version.as_ref());
    // `draw_frame` returns an empty command list with no map info, so bailing
    // here is the difference between reporting the gap and silently looping
    // through a track of blank frames.
    let map_info = map_info.ok_or(PreviewError::NoMapInfo { map_name: map_name.clone() })?;
    let game_fonts = asset_cache.lock().get_or_load_game_fonts(&vfs, version.as_ref(), dump_dir.as_deref());
    drop(vfs);

    let session_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    // Emit the full command set; the popup filters at paint time, so the
    // preset can change later without invalidating a cached track.
    let mut renderer = MinimapRenderer::new(Some(map_info), &game_metadata, session_version, RenderOptions::default());
    renderer.set_fonts(game_fonts);

    let mut session = MergedReplays::new(
        game_metadata.entity_specs(),
        &*game_metadata,
        &game_constants,
        session_version,
        &replay_file,
        &[],
    )
    .map_err(|_| PreviewError::UnreadableReplay)?;
    session.world_mut().set_shot_tracking(ShotTracking::Untracked);

    let mut sink = TrackSink::new();
    build_frame_track(&mut session, &mut renderer, 1.0 / SNAPSHOTS_PER_SECOND, cancel, &mut sink);
    session.finish();

    if cancel.load(Ordering::Relaxed) {
        return Err(PreviewError::Cancelled);
    }
    Ok(sink.finish(map_name))
}

/// Identifies a replay's preview. The mtime is part of the key so a replaced
/// file gets a fresh bake rather than a stale track.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct PreviewKey {
    pub path: PathBuf,
    pub mtime_secs: i64,
}

pub(crate) enum PreviewEntry {
    Ready(Arc<PreviewTrack>),
    Unavailable(PreviewError),
}

/// What the popup should draw this frame.
pub(crate) enum PreviewState {
    Ready(Arc<PreviewTrack>),
    Baking,
    Unavailable(PreviewError),
    Idle,
}

pub(crate) const PREVIEW_DWELL_SECS: f64 = 0.3;
/// Tracks held in memory. Small enough that a linear scan with move-to-front
/// beats taking on an LRU dependency.
pub(crate) const PREVIEW_CACHE_CAPACITY: usize = 8;

struct InFlightBake {
    key: PreviewKey,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub(crate) struct PreviewCache {
    /// Most recently used first.
    entries: Vec<(PreviewKey, PreviewEntry)>,
    in_flight: Option<InFlightBake>,
    /// The row under the cursor and when it was first seen there.
    hover: Option<(PreviewKey, f64)>,
}

impl PreviewCache {
    /// Record that `key` is hovered at `now`. True once the cursor has rested
    /// on that one row for `PREVIEW_DWELL_SECS`, so sweeping a list costs
    /// nothing.
    pub(crate) fn note_hover(&mut self, key: &PreviewKey, now: f64) -> bool {
        match self.hover {
            Some((ref hovered, since)) if hovered == key => now - since >= PREVIEW_DWELL_SECS,
            _ => {
                self.hover = Some((key.clone(), now));
                false
            }
        }
    }

    pub(crate) fn get(&mut self, key: &PreviewKey) -> Option<&PreviewEntry> {
        let idx = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(idx);
        self.entries.insert(0, entry);
        self.entries.first().map(|(_, e)| e)
    }

    pub(crate) fn insert(&mut self, key: PreviewKey, entry: PreviewEntry) {
        self.entries.retain(|(k, _)| k != &key);
        self.entries.insert(0, (key, entry));
        self.entries.truncate(PREVIEW_CACHE_CAPACITY);
    }

    /// Claim the single bake slot for `key`, cancelling whatever held it.
    pub(crate) fn begin_bake(&mut self, key: PreviewKey) -> Arc<AtomicBool> {
        if let Some(ref prev) = self.in_flight {
            prev.cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.in_flight = Some(InFlightBake { key, cancel: Arc::clone(&cancel) });
        cancel
    }

    pub(crate) fn is_baking(&self, key: &PreviewKey) -> bool {
        self.in_flight.as_ref().is_some_and(|b| &b.key == key)
    }

    /// Store a finished bake, unless it was superseded while running.
    pub(crate) fn finish_bake(&mut self, key: PreviewKey, entry: PreviewEntry) {
        if !self.is_baking(&key) {
            return;
        }
        self.in_flight = None;
        self.insert(key, entry);
    }

    /// Release the bake slot for `key` without storing a result, unless it
    /// was already superseded (in which case the slot belongs to a newer
    /// bake and must be left alone).
    pub(crate) fn clear_bake(&mut self, key: &PreviewKey) {
        if self.is_baking(key) {
            self.in_flight = None;
        }
    }
}

/// Releases the cache's bake slot when a bake finishes, whichever way it
/// finishes. `entry` is filled in on a normal, non-cancelled completion; a
/// panic inside the baking closure unwinds through this guard with `entry`
/// still `None`, which still frees the slot instead of wedging it for every
/// later hover on that row.
struct BakeSlotGuard {
    cache: Arc<Mutex<PreviewCache>>,
    key: PreviewKey,
    entry: Option<PreviewEntry>,
}

impl Drop for BakeSlotGuard {
    fn drop(&mut self) {
        let mut guard = self.cache.lock();
        match self.entry.take() {
            Some(entry) => guard.finish_bake(self.key.clone(), entry),
            None => guard.clear_bake(&self.key),
        }
    }
}

/// Resolve what the popup should draw for `key`, starting a bake when the
/// cursor has dwelled long enough and nothing is cached.
pub(crate) fn poll_preview(
    cache: &Arc<Mutex<PreviewCache>>,
    key: PreviewKey,
    now: f64,
    data_map: &WoWsDataMap,
    asset_cache: &Arc<Mutex<RendererAssetCache>>,
) -> PreviewState {
    let mut guard = cache.lock();
    match guard.get(&key) {
        Some(PreviewEntry::Ready(track)) => return PreviewState::Ready(Arc::clone(track)),
        Some(PreviewEntry::Unavailable(err)) => return PreviewState::Unavailable(err.clone()),
        None => {}
    }
    if guard.is_baking(&key) {
        return PreviewState::Baking;
    }
    if !guard.note_hover(&key, now) {
        return PreviewState::Idle;
    }

    let cancel = guard.begin_bake(key.clone());
    drop(guard);

    let cache = Arc::clone(cache);
    let asset_cache = Arc::clone(asset_cache);
    let data_map = data_map.clone();
    crate::util::thread::spawn_logged("replay-preview-bake", move || {
        let mut slot = BakeSlotGuard { cache, key: key.clone(), entry: None };
        let result = bake_preview_track(&key.path, &data_map, &asset_cache, &cancel);
        slot.entry = match result {
            Ok(track) => Some(PreviewEntry::Ready(Arc::new(track))),
            // A superseded bake stores nothing: the row it belonged to is no
            // longer hovered, and caching the cancellation would poison it.
            Err(PreviewError::Cancelled) => None,
            Err(err) => Some(PreviewEntry::Unavailable(err)),
        };
    });

    PreviewState::Baking
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    fn key(name: &str) -> PreviewKey {
        PreviewKey { path: PathBuf::from(name), mtime_secs: 1 }
    }

    fn track() -> Arc<PreviewTrack> {
        Arc::new(PreviewTrack { frames: vec![Vec::new()], map_name: "spaces/test".to_string() })
    }

    #[test]
    fn dwell_is_not_met_until_the_threshold_elapses_on_one_row() {
        let mut cache = PreviewCache::default();
        assert!(!cache.note_hover(&key("a"), 0.0));
        assert!(!cache.note_hover(&key("a"), PREVIEW_DWELL_SECS - 0.05));
        assert!(cache.note_hover(&key("a"), PREVIEW_DWELL_SECS + 0.01));
    }

    #[test]
    fn moving_to_another_row_restarts_the_dwell() {
        let mut cache = PreviewCache::default();
        assert!(!cache.note_hover(&key("a"), 0.0));
        assert!(!cache.note_hover(&key("b"), PREVIEW_DWELL_SECS + 0.01));
        assert!(cache.note_hover(&key("b"), PREVIEW_DWELL_SECS * 2.0 + 0.02));
    }

    #[test]
    fn the_cache_evicts_the_least_recently_used_entry() {
        let mut cache = PreviewCache::default();
        for i in 0..PREVIEW_CACHE_CAPACITY {
            cache.insert(key(&format!("r{i}")), PreviewEntry::Ready(track()));
        }
        // Touch the oldest so it is no longer the eviction candidate.
        assert!(matches!(cache.get(&key("r0")), Some(PreviewEntry::Ready(_))));
        cache.insert(key("overflow"), PreviewEntry::Ready(track()));
        assert!(cache.get(&key("r0")).is_some(), "recently used entry was evicted");
        assert!(cache.get(&key("r1")).is_none(), "least recently used entry survived");
    }

    #[test]
    fn a_new_bake_cancels_the_one_in_flight() {
        let mut cache = PreviewCache::default();
        let first = cache.begin_bake(key("a"));
        let second = cache.begin_bake(key("b"));
        assert!(first.load(Ordering::Relaxed), "superseded bake was not cancelled");
        assert!(!second.load(Ordering::Relaxed));
    }

    #[test]
    fn a_cancelled_bakes_result_is_discarded() {
        let mut cache = PreviewCache::default();
        let _first = cache.begin_bake(key("a"));
        let _second = cache.begin_bake(key("b"));
        cache.finish_bake(key("a"), PreviewEntry::Ready(track()));
        assert!(cache.get(&key("a")).is_none(), "stale bake result landed in the cache");
    }

    #[test]
    fn an_unavailable_entry_is_sticky() {
        let mut cache = PreviewCache::default();
        cache.insert(key("a"), PreviewEntry::Unavailable(PreviewError::UnreadableReplay));
        assert!(matches!(cache.get(&key("a")), Some(PreviewEntry::Unavailable(_))));
        assert!(matches!(cache.get(&key("a")), Some(PreviewEntry::Unavailable(_))));
    }

    #[test]
    fn a_panicking_bake_releases_the_slot_instead_of_wedging_it() {
        let cache = Arc::new(Mutex::new(PreviewCache::default()));
        let k = key("a");
        cache.lock().begin_bake(k.clone());

        let cache_for_thread = Arc::clone(&cache);
        let key_for_thread = k.clone();
        let handle = crate::util::thread::spawn_logged("test-panicking-bake", move || {
            let _slot = BakeSlotGuard { cache: cache_for_thread, key: key_for_thread, entry: None };
            panic!("simulated bake panic");
        });
        // `spawn_logged` catches the panic; joining guarantees the guard's
        // Drop (which runs during unwind) has already executed.
        handle.join().expect("test thread itself should not panic");

        assert!(!cache.lock().is_baking(&k), "panicking bake left the slot wedged");
    }
}

#[cfg(test)]
mod track_tests {
    use super::*;
    use wows_minimap_renderer::draw_command::DrawCommand;
    use wows_replays::types::ElapsedClock;

    fn feed(sink: &mut TrackSink, count: usize) {
        for i in 0..count {
            sink.push(i, GameClock(i as f32), Vec::new());
        }
    }

    fn tagged(index: usize) -> Vec<DrawCommand> {
        vec![DrawCommand::Timer { time_remaining: Some(index as i64), elapsed: ElapsedClock(index as f32) }]
    }

    #[test]
    fn a_short_replay_keeps_every_frame() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 40);
        let track = sink.finish("spaces/test".to_string());
        assert_eq!(track.frames.len(), 40);
    }

    #[test]
    fn a_long_replay_is_decimated_to_the_budget() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 1800);
        let track = sink.finish("spaces/test".to_string());
        assert!(track.frames.len() <= PREVIEW_MAX_FRAMES, "kept {}", track.frames.len());
        assert!(track.frames.len() > PREVIEW_MAX_FRAMES / 2, "kept too few: {}", track.frames.len());
    }

    #[test]
    fn decimation_keeps_the_first_frame_and_reaches_the_end() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 1800);
        assert_eq!(sink.kept_clocks()[0], GameClock(0.0));
        let last = *sink.kept_clocks().last().expect("a kept frame");
        assert!(last.0 >= 1800.0 - sink.stride() as f32, "last kept clock {last:?}");
    }

    #[test]
    fn an_empty_replay_yields_an_empty_track() {
        let sink = TrackSink::new();
        let track = sink.finish("spaces/test".to_string());
        assert!(track.frames.is_empty());
    }

    #[test]
    fn every_retained_frame_keeps_the_clock_it_arrived_with() {
        let mut sink = TrackSink::new();
        for i in 0..1800 {
            sink.push(i, GameClock(i as f32), tagged(i));
        }
        let clocks: Vec<GameClock> = sink.kept_clocks().to_vec();
        let track = sink.finish("spaces/test".to_string());
        assert_eq!(track.frames.len(), clocks.len(), "frames and clocks diverged");
        for (frame, clock) in track.frames.iter().zip(clocks.iter()) {
            let DrawCommand::Timer { time_remaining: Some(index), .. } = frame[0] else {
                panic!("expected a tagged Timer frame");
            };
            assert_eq!(index as f32, clock.0, "frame {index} was paired with clock {}", clock.0);
        }
    }
}
