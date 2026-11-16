//! A battle baked for playback: the draw commands of every kept frame and
//! when each was drawn.

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_replays::types::GameClock;
use wowsunpack::data::Version;

use crate::load::GameDataCache;
use crate::minimap_preview::PreviewError;
use crate::minimap_preview::SharedPreviewRenderer;
use crate::replay_render::options::playback_options;
use crate::replay_render::playback::mmss;

/// How many frames of a battle the track keeps.
///
/// Past this the sink halves what it holds and samples half as often, so a
/// twenty-minute battle costs what a five-minute one does. Twelve hundred
/// frames is two minutes of game time at the bake rate below, and a whole
/// twenty-minute battle once the stride has doubled four times.
pub const TRACK_BUDGET: usize = 1200;

/// How much game time one baked frame covers, in seconds.
pub const BAKE_INTERVAL: f32 = 0.5;

/// Where the map's corner sits before a track says otherwise.
///
/// The renderer reserves a strip above the map for the score bar and the
/// timer; a side panel would also move the map right, which is why a baked
/// track carries its own origin rather than every caller assuming this one.
pub const DEFAULT_MAP_ORIGIN: (f32, f32) = (0.0, HUD_HEIGHT as f32);

/// A frame's place in a [`Track`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameIndex(usize);

impl FrameIndex {
    pub const FIRST: Self = Self(0);

    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

/// A baked battle: the commands of every kept frame, and when each was drawn.
pub struct Track {
    pub frames: Vec<Vec<DrawCommand>>,
    pub clocks: Vec<GameClock>,
    /// When the battle proper began. Recording starts at the loading screen,
    /// so the clock a frame carries runs ahead of the one the game showed by
    /// this much.
    pub battle_start: GameClock,
    /// When the battle ended, if the replay ran that far.
    pub battle_end: Option<GameClock>,
    /// How wide the map is in the game's own world units, which is what
    /// turns a ship's range in metres into a distance on the minimap.
    pub space_size: f32,
    /// The build this was recorded on, which some of a ship's ranges are
    /// gated on.
    pub version: Version,
    /// Where the map's top-left corner sits in these frames.
    pub map_origin: (f32, f32),
    /// The map's space name, which a peer watching this playback loads art by.
    pub space: String,
}

impl Track {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The last frame, or the first on an empty track.
    pub fn last(&self) -> FrameIndex {
        FrameIndex(self.len().saturating_sub(1))
    }

    /// What frame `at` draws.
    pub fn commands(&self, at: FrameIndex) -> Option<&[DrawCommand]> {
        self.frames.get(at.0).map(Vec::as_slice)
    }

    /// The game time `index` was drawn at, in seconds.
    ///
    /// A frame past the end reads as clock zero, which is where the transport
    /// of an empty track sits.
    pub fn seconds_at(&self, index: FrameIndex) -> f32 {
        self.clocks.get(index.0).map(|clock| clock.seconds()).unwrap_or(0.0)
    }

    /// How far into the battle `index` is, which is the clock the game showed.
    pub fn elapsed_at(&self, index: FrameIndex) -> f32 {
        self.seconds_at(index) - self.battle_start.seconds()
    }

    /// The last frame drawn at or before `seconds` of game time.
    ///
    /// Clocks ascend, so this is a partition point. A time before the first
    /// frame lands on that frame rather than nowhere.
    pub fn frame_at(&self, seconds: f32) -> FrameIndex {
        FrameIndex(self.clocks.partition_point(|clock| clock.seconds() <= seconds).saturating_sub(1))
    }

    /// Where a clock sits along the track, as a fraction of its length.
    ///
    /// `None` when the track is too short to have a length to sit along.
    pub fn position_of(&self, clock: GameClock) -> Option<f32> {
        let last = self.len().checked_sub(1).filter(|last| *last > 0)?;
        Some((self.frame_at(clock.seconds()).0 as f32 / last as f32).clamp(0.0, 1.0))
    }

    /// Where the seek bar marks the battle's start and end, as fractions of
    /// its length.
    ///
    /// A replay records from the loading screen on and often past the last
    /// shot, so the battle proper is a stretch inside the track rather than
    /// the whole of it. An end the replay never recorded has no mark.
    pub fn battle_marks(&self) -> Vec<f32> {
        [Some(self.battle_start), self.battle_end]
            .into_iter()
            .flatten()
            .filter_map(|clock| self.position_of(clock))
            .collect()
    }

    /// The frames an export covers.
    ///
    /// A replay records the loading screen and the countdown before the battle
    /// proper; unless the reader asked for them, an export starts where the
    /// battle did.
    pub fn frames_to_export(&self, include_pre_battle: bool) -> &[Vec<DrawCommand>] {
        if include_pre_battle {
            return &self.frames;
        }
        let from = self.frame_at(self.battle_start.seconds());
        self.frames.get(from.0..).unwrap_or(&self.frames)
    }

    /// The game time of the last frame, in seconds.
    pub fn duration_seconds(&self) -> f32 {
        self.seconds_at(self.last())
    }

    /// How long the battle runs for, as the clock would read at its end.
    pub fn length_label(&self) -> String {
        mmss(self.elapsed_at(self.last()))
    }
}

/// Why a battle could not be played back.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Preview(#[from] PreviewError),
}

/// Walks `path`'s battle once, keeping every frame's draw commands and the
/// renderer they are drawn through.
///
/// `alts` are other recordings of the same battle, read alongside it: the map
/// then shows what the primary's team could not see.
pub fn bake(
    path: &Path,
    alts: &[PathBuf],
    game_data: &GameDataCache,
    cancel: &AtomicBool,
) -> Result<(Track, SharedPreviewRenderer), RenderError> {
    let baked = crate::minimap_preview::bake_track(
        path,
        alts,
        game_data,
        cancel,
        TRACK_BUDGET,
        BAKE_INTERVAL,
        playback_options(),
    )?;

    let track = Track {
        frames: baked.frames,
        clocks: baked.clocks,
        battle_start: baked.battle_start,
        battle_end: baked.battle_end,
        space_size: baked.space_size,
        version: baked.version,
        map_origin: (baked.map_origin.0 as f32, baked.map_origin.1 as f32),
        space: baked.map_name,
    };

    Ok((track, baked.renderer))
}
