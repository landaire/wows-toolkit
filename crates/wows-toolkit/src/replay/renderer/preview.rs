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
use super::RgbaAsset;
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
    /// The game-data version the track was baked against, which is the replay's
    /// own build and not necessarily the live install's.
    pub version: Option<Version>,
    /// Map art from that same build, carried so the popup paints the map the
    /// replay was recorded on rather than whatever the live install ships under
    /// the same name.
    pub map_image: Option<Arc<RgbaAsset>>,
}

/// Retains a bounded, evenly spaced subset of the frames it is fed.
pub(crate) struct TrackSink {
    frames: Vec<Vec<DrawCommand>>,
    /// The clock each retained frame arrived with. Only the alignment tests
    /// read these back; the popup plays frames at a fixed display rate.
    #[cfg(test)]
    clocks: Vec<GameClock>,
    /// Keep one frame in every `stride`. Doubles each time the budget fills.
    stride: usize,
    /// Source frames seen since the last frame was retained.
    since_kept: usize,
}

impl TrackSink {
    pub(crate) fn new() -> Self {
        Self {
            frames: Vec::with_capacity(PREVIEW_MAX_FRAMES * 2),
            #[cfg(test)]
            clocks: Vec::new(),
            stride: 1,
            since_kept: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn stride(&self) -> usize {
        self.stride
    }

    #[cfg(test)]
    pub(crate) fn kept_clocks(&self) -> &[GameClock] {
        &self.clocks
    }

    pub(crate) fn finish(
        self,
        map_name: String,
        version: Option<Version>,
        map_image: Option<Arc<RgbaAsset>>,
    ) -> PreviewTrack {
        PreviewTrack { frames: self.frames, map_name, version, map_image }
    }

    /// Drop every other retained frame, halving the track in place.
    fn halve(&mut self) {
        let mut keep = false;
        self.frames.retain(|_| {
            keep = !keep;
            keep
        });
        #[cfg(test)]
        {
            let mut keep = false;
            self.clocks.retain(|_| {
                keep = !keep;
                keep
            });
        }
        self.stride *= 2;
    }
}

impl FrameSink for TrackSink {
    fn push(&mut self, _index: usize, clock: GameClock, commands: Vec<DrawCommand>) {
        // Playback runs at a fixed display rate and never consults the clock;
        // only the frame/clock alignment tests read it back.
        #[cfg(not(test))]
        let _ = clock;
        if self.since_kept.is_multiple_of(self.stride) {
            self.frames.push(commands);
            #[cfg(test)]
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
/// a formatted message back apart. `Display` is the log rendering; what the
/// user reads comes from [`Self::key`] and the value the variant carries.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum PreviewError {
    // `Version` has no `Display` impl; `to_path()` is the same
    // "major.minor.patch" rendering used everywhere else in the app that
    // shows a version to a user (see `wows_data.rs`, `video_export.rs`).
    #[error("no game data for client version {}", version.to_path())]
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

impl PreviewError {
    /// Translation key for the message the popup shows.
    ///
    /// Every key but `Cancelled`'s takes one `value` argument, filled from the
    /// field the variant carries. `Cancelled` never reaches the popup: a
    /// superseded bake stores nothing.
    pub(crate) const fn key(&self) -> &'static str {
        match self {
            Self::NoGameDataForVersion { .. } => "ui.replay.preview_no_game_data",
            Self::UnreadableReplay => "ui.replay.preview_unreadable",
            Self::UnknownClientVersion { .. } => "ui.replay.preview_unknown_version",
            Self::NoMapInfo { .. } => "ui.replay.preview_no_map",
            Self::Cancelled => "ui.replay.preview_cancelled",
        }
    }

    /// The `value` argument [`Self::key`]'s message interpolates, if it takes
    /// one.
    pub(crate) fn value(&self) -> Option<String> {
        match self {
            Self::NoGameDataForVersion { version } => Some(version.to_path()),
            Self::UnknownClientVersion { raw } => Some(raw.clone()),
            Self::NoMapInfo { map_name } => Some(map_name.clone()),
            Self::UnreadableReplay | Self::Cancelled => None,
        }
    }
}

/// What a preview track is baked with.
///
/// Wider than [`preview_options`](crate::ui::replay_parser::preview_popup::preview_options),
/// the paint-time preset, so turning one of that preset's switches on does not
/// invalidate cached tracks. It is not everything the renderer can emit: the
/// panel commands (stats, rosters) carry a full roster per frame and position
/// trails carry the whole match's position history per ship per frame, so
/// baking either would cost tens of megabytes for one track. Widening this set
/// changes what a cached track contains, so a track baked before the change
/// cannot serve a preset that asks for the new class; the popup's presets are
/// checked against this set by `paint_preset_asks_for_nothing_unbaked`.
pub(crate) fn bake_options() -> RenderOptions {
    RenderOptions {
        show_kill_feed: true,
        show_chat: true,
        show_armament: true,
        show_stats_panel: false,
        show_team_rosters: false,
        show_ship_config: false,
        show_trails: false,
        show_speed_trails: false,
        ..RenderOptions::default()
    }
}

/// Bake a decimated preview track for `path` in a single forward pass.
///
/// Mirrors the setup `playback_thread` does before its own forward pass, then
/// skips everything else that thread does afterward: damage-event gathering,
/// timeline/shot extraction, salvo flight-time scanning, player-build
/// snapshotting, the live-session rebuild, silhouette loading,
/// `commands.scheme.xml` parsing, and the collab announce. That is the entire
/// reason this finishes cheaply enough to run on hover.
///
/// Cancellation is cooperative, and the checks before `data_map.resolve` matter
/// most: resolving an unloaded build is a full game-data load, so a bake that
/// has already been superseded must not enter one.
pub(crate) fn bake_preview_track(
    path: &Path,
    data_map: &WoWsDataMap,
    asset_cache: &Arc<parking_lot::Mutex<RendererAssetCache>>,
    cancel: &AtomicBool,
) -> Result<PreviewTrack, PreviewError> {
    let cancelled = || cancel.load(Ordering::Relaxed).then_some(PreviewError::Cancelled);

    let replay_file = ReplayFile::from_file(path).map_err(|_| PreviewError::UnreadableReplay)?;
    if let Some(err) = cancelled() {
        return Err(err);
    }
    let raw_client_version = replay_file.meta.clientVersionFromExe.clone();
    let replay_version = Version::try_from_client_exe(&raw_client_version)
        .ok_or(PreviewError::UnknownClientVersion { raw: raw_client_version })?;
    if let Some(err) = cancelled() {
        return Err(err);
    }
    // Holds no other lock across this call: a build load takes seconds, and the
    // UI thread needs `asset_cache` and `preview_cache` every frame.
    let wows_data =
        data_map.resolve(&replay_version).ok_or(PreviewError::NoGameDataForVersion { version: replay_version })?;
    if let Some(err) = cancelled() {
        return Err(err);
    }

    let map_name = replay_file.meta.mapName.clone();
    let (vfs, version, game_metadata, game_constants, dump_dir) = {
        let data = wows_data.read();
        let gm = data.game_metadata.clone().ok_or(PreviewError::NoGameDataForVersion { version: replay_version })?;
        (data.vfs.clone(), data.version().copied(), gm, Arc::clone(&data.game_constants), data.dump_dir.clone())
    };

    let (map_image, map_info) = super::load_map_unlocked(asset_cache, &map_name, &vfs, version.as_ref());
    // `draw_frame` returns an empty command list with no map info, so bailing
    // here is the difference between reporting the gap and silently looping
    // through a track of blank frames.
    let map_info = map_info.ok_or(PreviewError::NoMapInfo { map_name: map_name.clone() })?;
    let game_fonts = super::load_game_fonts_unlocked(asset_cache, &vfs, version.as_ref(), dump_dir.as_deref());
    drop(vfs);

    let session_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let mut renderer = MinimapRenderer::new(Some(map_info), &game_metadata, session_version, bake_options());
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
    // Tracked, not Untracked: `draw_frame` only emits `ShotTracer` commands
    // from `active_shots()`, which stays empty unless shot recording is on.
    // This costs the bake shot and hit recording it would otherwise skip.
    session.world_mut().set_shot_tracking(ShotTracking::Tracked);

    let mut sink = TrackSink::new();
    build_frame_track(&mut session, &mut renderer, 1.0 / SNAPSHOTS_PER_SECOND, cancel, &mut sink);
    session.finish();

    if let Some(err) = cancelled() {
        return Err(err);
    }
    Ok(sink.finish(map_name, version, map_image))
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

/// How long a preview can go undrawn before its next draw is treated as a
/// fresh open rather than a continuation. Must comfortably exceed the
/// `Ready`-state repaint interval (`1.0 / PREVIEW_FPS`, about 67ms) so two
/// consecutive repaints of the same open tooltip never look like a gap; only
/// an actual close-and-reopen (the cursor left and came back) crosses it.
pub(crate) const PREVIEW_ANIM_RESTART_GAP_SECS: f64 = 0.25;

/// One tooltip's play position: which key it is animating and where in wall
/// time its loop started.
struct AnimSession {
    key: PreviewKey,
    start: f64,
    last_seen: f64,
    /// How many frames the track had when the session started. A track that
    /// lands mid-bake replaces a one-frame placeholder, and continuing that
    /// session would enter the real loop however many seconds in the bake took.
    frame_count: usize,
}

/// Identifies one bake attempt, distinct from every other attempt even for
/// the same [`PreviewKey`]. Slot ownership is decided by this, not by key
/// equality: a row can be dwelled onto, away from, and back onto again while
/// an earlier bake for that same row is still winding down after
/// cancellation, and that stale bake must not be mistaken for the new one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct BakeId(u64);

struct InFlightBake {
    key: PreviewKey,
    id: BakeId,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub(crate) struct PreviewCache {
    /// Most recently used first.
    entries: Vec<(PreviewKey, PreviewEntry)>,
    in_flight: Option<InFlightBake>,
    /// The row under the cursor and when it was first seen there.
    hover: Option<(PreviewKey, f64)>,
    next_bake_id: u64,
    /// The single visible tooltip's play position, if any. Only one preview
    /// is ever drawn at a time, so one slot is enough.
    anim_session: Option<AnimSession>,
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

    /// Claim the bake slot for `key`, cancelling whatever held it. At most
    /// one bake owns the slot at a time, but a superseded bake is not
    /// stopped by this call: cancellation is cooperative, so it keeps
    /// running until it next checks its `cancel` flag.
    pub(crate) fn begin_bake(&mut self, key: PreviewKey) -> (BakeId, Arc<AtomicBool>) {
        if let Some(ref prev) = self.in_flight {
            prev.cancel.store(true, Ordering::Relaxed);
        }
        let id = BakeId(self.next_bake_id);
        self.next_bake_id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.in_flight = Some(InFlightBake { key, id, cancel: Arc::clone(&cancel) });
        (id, cancel)
    }

    pub(crate) fn is_baking(&self, key: &PreviewKey) -> bool {
        self.in_flight.as_ref().is_some_and(|b| &b.key == key)
    }

    fn owns_slot(&self, id: BakeId) -> bool {
        self.in_flight.as_ref().is_some_and(|b| b.id == id)
    }

    /// Store a finished bake, unless `id` no longer owns the slot. Keying
    /// this on the bake's identity rather than `key` matters when the same
    /// row is dwelled onto twice: the earlier, still-unwinding bake for that
    /// row must not be confused with the current one.
    pub(crate) fn finish_bake(&mut self, id: BakeId, key: PreviewKey, entry: PreviewEntry) {
        if !self.owns_slot(id) {
            return;
        }
        self.in_flight = None;
        self.insert(key, entry);
    }

    /// Release the bake slot without storing a result, but only if `id`
    /// still owns it. A cancelled bake's `id` has usually already been
    /// replaced by a newer one (same row or not) by the time it notices the
    /// cancellation, in which case this is a no-op; only a bake that never
    /// got superseded (for example one that panicked) actually frees
    /// anything here.
    pub(crate) fn clear_bake(&mut self, id: BakeId) {
        if self.owns_slot(id) {
            self.in_flight = None;
        }
    }

    /// Frame index for `key`, restarting from zero whenever the tooltip
    /// reopens after a gap in being drawn, moves to another row, or the track
    /// it is playing changes length. Only one preview is visible at a time, so
    /// a single session slot is enough.
    pub(crate) fn anim_index(&mut self, key: &PreviewKey, now: f64, frame_count: usize) -> usize {
        if frame_count == 0 {
            return 0;
        }
        let restart = match &self.anim_session {
            Some(session) => {
                &session.key != key
                    || session.frame_count != frame_count
                    || now - session.last_seen > PREVIEW_ANIM_RESTART_GAP_SECS
            }
            None => true,
        };
        if restart {
            self.anim_session = Some(AnimSession { key: key.clone(), start: now, last_seen: now, frame_count });
        } else if let Some(session) = &mut self.anim_session {
            session.last_seen = now;
        }
        let start = self.anim_session.as_ref().map_or(now, |session| session.start);
        (((now - start) * PREVIEW_FPS as f64) as usize) % frame_count
    }
}

/// Releases the cache's bake slot when a bake finishes, whichever way it
/// finishes. `entry` is filled in on a normal, non-cancelled completion; a
/// panic inside the baking closure unwinds through this guard with `entry`
/// still `None`, which still frees the slot instead of wedging it for every
/// later hover on that row.
struct BakeSlotGuard {
    cache: Arc<Mutex<PreviewCache>>,
    id: BakeId,
    key: PreviewKey,
    entry: Option<PreviewEntry>,
}

impl Drop for BakeSlotGuard {
    fn drop(&mut self) {
        let mut guard = self.cache.lock();
        match self.entry.take() {
            Some(entry) => guard.finish_bake(self.id, self.key.clone(), entry),
            None => guard.clear_bake(self.id),
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
    // Recorded before the cache/in-flight checks below so every poll keeps
    // `hover` current, even the ones that return early on a cache hit or a
    // bake already in flight. Otherwise a row that left the cache untouched
    // (hit or `Baking`) never refreshes `hover`, and returning to it later
    // reads a stale `since` and skips the dwell entirely.
    let dwelled = guard.note_hover(&key, now);
    match guard.get(&key) {
        Some(PreviewEntry::Ready(track)) => return PreviewState::Ready(Arc::clone(track)),
        Some(PreviewEntry::Unavailable(err)) => return PreviewState::Unavailable(err.clone()),
        None => {}
    }
    if guard.is_baking(&key) {
        return PreviewState::Baking;
    }
    if !dwelled {
        return PreviewState::Idle;
    }

    let (id, cancel) = guard.begin_bake(key.clone());
    drop(guard);

    let cache = Arc::clone(cache);
    let asset_cache = Arc::clone(asset_cache);
    let data_map = data_map.clone();
    crate::util::thread::spawn_logged("replay-preview-bake", move || {
        let mut slot = BakeSlotGuard { cache, id, key: key.clone(), entry: None };
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
        Arc::new(PreviewTrack {
            frames: vec![Vec::new()],
            map_name: "spaces/test".to_string(),
            version: None,
            map_image: None,
        })
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
        let (_first_id, first_cancel) = cache.begin_bake(key("a"));
        let (_second_id, second_cancel) = cache.begin_bake(key("b"));
        assert!(first_cancel.load(Ordering::Relaxed), "superseded bake was not cancelled");
        assert!(!second_cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn a_cancelled_bakes_result_is_discarded() {
        let mut cache = PreviewCache::default();
        let (first_id, _first_cancel) = cache.begin_bake(key("a"));
        let (_second_id, _second_cancel) = cache.begin_bake(key("b"));
        cache.finish_bake(first_id, key("a"), PreviewEntry::Ready(track()));
        assert!(cache.get(&key("a")).is_none(), "stale bake result landed in the cache");
    }

    #[test]
    fn a_stale_bake_does_not_free_a_newer_same_key_bakes_slot() {
        let mut cache = PreviewCache::default();
        let (first_id, _) = cache.begin_bake(key("a"));
        let (_second_id, _) = cache.begin_bake(key("b"));
        let (_third_id, _) = cache.begin_bake(key("a"));
        cache.clear_bake(first_id);
        assert!(cache.is_baking(&key("a")), "a stale bake freed the newest bake's slot");
    }

    #[test]
    fn a_stale_bakes_result_does_not_land_over_a_newer_same_key_bake() {
        let mut cache = PreviewCache::default();
        let (first_id, _) = cache.begin_bake(key("a"));
        let (_second_id, _) = cache.begin_bake(key("b"));
        let (_third_id, _) = cache.begin_bake(key("a"));
        cache.finish_bake(first_id, key("a"), PreviewEntry::Ready(track()));
        assert!(cache.get(&key("a")).is_none(), "a stale bake's result landed in the cache");
        assert!(cache.is_baking(&key("a")), "a stale bake's finish cleared the newest bake's slot");
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
        let (id, _cancel) = cache.lock().begin_bake(k.clone());

        let cache_for_thread = Arc::clone(&cache);
        let key_for_thread = k.clone();
        let handle = crate::util::thread::spawn_logged("test-panicking-bake", move || {
            let _slot = BakeSlotGuard { cache: cache_for_thread, id, key: key_for_thread, entry: None };
            panic!("simulated bake panic");
        });
        // `spawn_logged` catches the panic; joining guarantees the guard's
        // Drop (which runs during unwind) has already executed.
        handle.join().expect("test thread itself should not panic");

        assert!(!cache.lock().is_baking(&k), "panicking bake left the slot wedged");
    }

    #[test]
    fn a_continuous_sequence_of_polls_keeps_advancing_without_restarting() {
        let mut cache = PreviewCache::default();
        let a = key("a");
        assert_eq!(cache.anim_index(&a, 0.0, 10), 0);
        // Small forward steps, each well under the restart gap: the session
        // keeps its original start, so the index keeps climbing with time
        // rather than resetting on every poll.
        assert_eq!(cache.anim_index(&a, 0.1, 10), 1);
        assert_eq!(cache.anim_index(&a, 0.2, 10), 3);
    }

    #[test]
    fn a_gap_larger_than_the_threshold_restarts_at_zero() {
        let mut cache = PreviewCache::default();
        let a = key("a");
        assert_eq!(cache.anim_index(&a, 5.0, 10), 0);
        cache.anim_index(&a, 5.1, 10);
        let after_gap = 5.1 + PREVIEW_ANIM_RESTART_GAP_SECS + 0.01;
        assert_eq!(cache.anim_index(&a, after_gap, 10), 0, "a gap past the threshold should restart the loop");
    }

    #[test]
    fn switching_keys_restarts_at_zero() {
        let mut cache = PreviewCache::default();
        let a = key("a");
        let b = key("b");
        assert_eq!(cache.anim_index(&a, 5.0, 10), 0);
        cache.anim_index(&a, 5.1, 10);
        assert_eq!(cache.anim_index(&b, 5.15, 10), 0, "a different key must not continue the previous session");
    }

    #[test]
    fn the_index_wraps_at_frame_count() {
        let mut cache = PreviewCache::default();
        let a = key("a");
        assert_eq!(cache.anim_index(&a, 0.0, 4), 0);
        // Steady small steps, each well under the restart gap, keep this one
        // continuous session so the elapsed time (not a fresh start) drives
        // the index past `frame_count`.
        for now in [0.2, 0.4, 0.6, 0.8] {
            cache.anim_index(&a, now, 4);
        }
        // By t=1.03s, frame 15 has elapsed (15.45 truncates to 15); wrapped
        // against a 4-frame track that is 15 % 4 == 3, not 15.
        assert_eq!(cache.anim_index(&a, 1.03, 4), 3);
    }

    #[test]
    fn zero_frame_count_returns_zero_without_dividing() {
        let mut cache = PreviewCache::default();
        let a = key("a");
        assert_eq!(cache.anim_index(&a, 0.0, 0), 0);
    }

    #[test]
    fn a_track_landing_mid_bake_starts_the_loop_at_its_beginning() {
        let mut cache = PreviewCache::default();
        let a = key("a");
        // Baking: repeated polls against the one-frame placeholder.
        let mut t = 0.0;
        while t < 5.0 {
            assert_eq!(cache.anim_index(&a, t, 1), 0);
            t += 0.1;
        }
        // The real track lands; the loop must start at its beginning.
        assert_eq!(cache.anim_index(&a, 5.0, 150), 0);
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
        let track = sink.finish("spaces/test".to_string(), None, None);
        assert_eq!(track.frames.len(), 40);
    }

    #[test]
    fn a_long_replay_is_decimated_to_the_budget() {
        let mut sink = TrackSink::new();
        feed(&mut sink, 1800);
        let track = sink.finish("spaces/test".to_string(), None, None);
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
        let track = sink.finish("spaces/test".to_string(), None, None);
        assert!(track.frames.is_empty());
    }

    #[test]
    fn every_retained_frame_keeps_the_clock_it_arrived_with() {
        let mut sink = TrackSink::new();
        for i in 0..1800 {
            sink.push(i, GameClock(i as f32), tagged(i));
        }
        let clocks: Vec<GameClock> = sink.kept_clocks().to_vec();
        let track = sink.finish("spaces/test".to_string(), None, None);
        assert_eq!(track.frames.len(), clocks.len(), "frames and clocks diverged");
        for (frame, clock) in track.frames.iter().zip(clocks.iter()) {
            let DrawCommand::Timer { time_remaining: Some(index), .. } = frame[0] else {
                panic!("expected a tagged Timer frame");
            };
            assert_eq!(index as f32, clock.0, "frame {index} was paired with clock {}", clock.0);
        }
    }
}
