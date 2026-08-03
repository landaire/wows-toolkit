use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

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
