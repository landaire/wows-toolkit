//! The transport: where playback is, whether it is running, and how fast.

use std::time::Duration;

use wows_minimap_renderer::timing::FPS;
use wows_minimap_renderer::timing::PlaybackRate;
use wows_minimap_renderer::timing::SPEED_LADDER;
use wows_replays::types::GameClock;

use crate::replay_render::track::FrameIndex;
use crate::replay_render::track::Track;

/// How often a playing viewport draws, at every speed: the rate a video is written at. Each draw
/// is placed between the two baked frames around its clock, so playback moves at this rate rather
/// than at the bake's.
pub const FRAME_INTERVAL: Duration = Duration::from_nanos((1_000_000_000.0 / FPS) as u64);

/// The speed a viewport opens at. Real time is too slow to watch a battle through.
pub const DEFAULT_SPEED: f32 = PlaybackRate::DEFAULT_SPEED.game_seconds_per_second();

/// How far the skip controls and the left and right keys move, in seconds of
/// game time.
pub const SEEK_STEP: f32 = 10.0;

/// Where playback is on a [`Track`], whether it is running, and how fast.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    at: FrameIndex,
    /// How far past `at`'s clock playback is, in seconds of game time. Zero on a frame sought by
    /// its index, so it is drawn as it was baked.
    past: f32,
    playing: bool,
    speed: f32,
}

/// What one tick of a running transport did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Playback moved on between the same two frames.
    Moved,
    /// Playback moved on to a later frame.
    Advanced,
    /// Playback reached the last frame and stopped there.
    Finished,
    /// Playback was not running.
    Idle,
}

impl Playback {
    pub fn new() -> Self {
        Self { at: FrameIndex::FIRST, past: 0.0, playing: false, speed: DEFAULT_SPEED }
    }

    /// The frame at or before where playback is: what it draws, moved on by [`Self::clock`].
    pub fn at(&self) -> FrameIndex {
        self.at
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Moves playback to `at`, held to the track. Returns whether it moved.
    ///
    /// A viewport with no frame on screen yet draws one even when this
    /// returns `false`.
    pub fn go_to(&mut self, track: &Track, at: FrameIndex) -> bool {
        let at = at.min(track.last());
        if at == self.at && self.past == 0.0 {
            return false;
        }
        self.at = at;
        self.past = 0.0;

        true
    }

    /// Moves playback `delta` seconds of game time from where it is.
    pub fn seek_by(&mut self, track: &Track, delta: f32) -> bool {
        self.go_to_clock(track, GameClock(self.clock(track).seconds() + delta))
    }

    /// Moves playback to the frame a seek bar over the track's frames names.
    ///
    /// The frame playback is already on is no move: a seek bar writes back the frame it shows as
    /// it is built, which must not pull a playing transport back to that frame's start.
    pub fn seek_to_slider(&mut self, track: &Track, value: f32) -> bool {
        let at = FrameIndex::new(value.round().max(0.) as usize);
        if at == self.at {
            return false;
        }

        self.go_to(track, at)
    }

    /// Moves playback to `elapsed` seconds into the battle, which is the
    /// clock a timeline event carries.
    pub fn go_to_elapsed(&mut self, track: &Track, elapsed: f32) -> bool {
        self.go_to_clock(track, GameClock(elapsed + track.battle_start.seconds()))
    }

    /// Moves playback to `clock`, held to the track, between frames if that is where it falls.
    ///
    /// The clock is the battle's own, as the replay's timeline states it, which is what a salvo in
    /// the incoming-fire log is stamped with. Landing on it rather than on the frame before is what
    /// lets the next event after one that was jumped to be found, rather than that one again.
    pub fn go_to_clock(&mut self, track: &Track, clock: GameClock) -> bool {
        let seconds = clock.seconds().clamp(track.seconds_at(FrameIndex::FIRST), track.duration_seconds());
        let at = track.frame_at(seconds);
        let past = seconds - track.seconds_at(at);
        if at == self.at && past == self.past {
            return false;
        }
        self.at = at;
        self.past = past;

        true
    }

    /// Changes the speed. Returns whether it changed.
    pub fn set_speed(&mut self, speed: f32) -> bool {
        if (self.speed - speed).abs() < f32::EPSILON {
            return false;
        }
        self.speed = speed;

        true
    }

    /// Moves one rung along the speed ladder, in the direction of `step`.
    ///
    /// Nothing happens at either end: the ladder is what the transport
    /// offers, and there is no speed past its ends to move to.
    pub fn step_speed(&mut self, step: i32) -> bool {
        let here = SPEED_LADDER.iter().position(|speed| (speed - self.speed).abs() < f32::EPSILON);
        let Some(here) = here else { return false };
        let Some(next) = here.checked_add_signed(step as isize).and_then(|index| SPEED_LADDER.get(index)) else {
            return false;
        };

        self.set_speed(*next)
    }

    /// Starts or stops playback. Returns whether playback moved.
    ///
    /// Playing from the end starts again at the beginning, which is what a
    /// transport with no stop button has to mean.
    pub fn toggle_playing(&mut self, track: &Track) -> bool {
        self.playing = !self.playing;
        if self.playing && self.at.get() + 1 >= track.len() {
            return self.go_to(track, FrameIndex::FIRST);
        }

        false
    }

    /// Stops playback where it is, between frames if that is where it is.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Moves a running transport on by `elapsed` of real time at its speed, stopping at the last
    /// frame.
    pub fn advance(&mut self, track: &Track, elapsed: Duration) -> Tick {
        if !self.playing {
            return Tick::Idle;
        }
        let target = self.clock(track).seconds() + elapsed.as_secs_f32() * self.speed;
        if target >= track.duration_seconds() || self.at >= track.last() {
            self.at = track.last();
            self.past = 0.0;
            self.playing = false;
            return Tick::Finished;
        }

        let at = track.frame_at(target);
        let advanced = at != self.at;
        self.at = at;
        self.past = target - track.seconds_at(at);

        if advanced { Tick::Advanced } else { Tick::Moved }
    }

    /// The game clock where playback is.
    pub fn clock(&self, track: &Track) -> GameClock {
        GameClock(track.seconds_at(self.at) + self.past)
    }

    /// How far into the battle playback is, which is what an event's clock is
    /// measured against.
    pub fn elapsed(&self, track: &Track) -> f32 {
        self.clock(track).seconds() - track.battle_start.seconds()
    }

    /// Where playback has reached, as the clock the game showed.
    ///
    /// Elapsed rather than absolute: a replay records from the loading screen
    /// on, so the raw clock opens at something like 0:40 on a battle that has
    /// not started.
    pub fn clock_label(&self, track: &Track) -> String {
        mmss(self.elapsed(track))
    }

    /// Where playback has reached and how much there is, which is what says
    /// whether the end is near without reading the bar.
    pub fn clock_and_length(&self, track: &Track) -> String {
        format!("{} / {}", self.clock_label(track), track.length_label())
    }
}

impl Default for Playback {
    fn default() -> Self {
        Self::new()
    }
}

/// Where `speed` sits in the ladder.
///
/// A speed off the ladder reads as its first rung, which is what a dropdown
/// over the ladder can show for it.
pub fn speed_index(speed: f32) -> usize {
    SPEED_LADDER.iter().position(|offered| (offered - speed).abs() < f32::EPSILON).unwrap_or_default()
}

/// A speed as the transport labels it: no trailing zero on a whole one.
pub fn speed_label(speed: f32) -> String {
    if (speed - speed.round()).abs() < f32::EPSILON {
        format!("{}x", speed.round() as i32)
    } else {
        format!("{speed}x")
    }
}

/// Seconds of game time as the clock the game shows, zero-padded.
pub fn mmss(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
