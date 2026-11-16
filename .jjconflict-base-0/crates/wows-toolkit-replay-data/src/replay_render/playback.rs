//! The transport: where playback is, whether it is running, and how fast.

use std::time::Duration;

use wows_replays::types::GameClock;

use crate::replay_render::track::FrameIndex;
use crate::replay_render::track::Track;

/// How often the viewport advances while playing, before the speed
/// multiplier. One frame of game time per tick at 1x.
pub const TICK: Duration = Duration::from_millis(500);

/// The speeds the transport offers, which are the egui renderer's own
/// (`PLAYBACK_SPEEDS`). A battle runs twenty minutes, so the useful range is
/// well above real time.
pub const SPEEDS: [f32; 6] = [1.0, 5.0, 10.0, 20.0, 40.0, 60.0];

/// The speed a viewport opens at, as the egui renderer opens at
/// (`SharedRendererState::speed`). Real time is too slow to watch a battle
/// through.
pub const DEFAULT_SPEED: f32 = 20.0;

/// How far the skip controls and the left and right keys move, in seconds of
/// game time. The egui renderer's own step.
pub const SEEK_STEP: f32 = 10.0;

/// Where playback is on a [`Track`], whether it is running, and how fast.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    at: FrameIndex,
    playing: bool,
    speed: f32,
}

/// What one tick of a running transport did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Playback moved one frame on.
    Advanced,
    /// Playback had reached the last frame and stopped there.
    Finished,
    /// Playback was not running.
    Idle,
}

impl Playback {
    pub fn new() -> Self {
        Self { at: FrameIndex::FIRST, playing: false, speed: DEFAULT_SPEED }
    }

    /// Which frame of the track is on screen, or is being drawn for.
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
    /// A front end with no frame on screen yet draws one even when this
    /// returns `false`.
    pub fn go_to(&mut self, track: &Track, at: FrameIndex) -> bool {
        let at = at.min(track.last());
        if at == self.at {
            return false;
        }
        self.at = at;

        true
    }

    /// Moves playback `delta` seconds of game time from where it is.
    pub fn seek_by(&mut self, track: &Track, delta: f32) -> bool {
        let at = track.frame_at(track.seconds_at(self.at) + delta);
        self.go_to(track, at)
    }

    /// Moves playback to the frame a seek bar over the track's frames names.
    pub fn seek_to_slider(&mut self, track: &Track, value: f32) -> bool {
        self.go_to(track, FrameIndex::new(value.round().max(0.) as usize))
    }

    /// Moves playback to `elapsed` seconds into the battle, which is the
    /// clock a timeline event carries.
    pub fn go_to_elapsed(&mut self, track: &Track, elapsed: f32) -> bool {
        let frame = track.frame_at(elapsed + track.battle_start.seconds());
        self.go_to(track, frame)
    }

    /// Moves playback to the frame `clock` falls in.
    ///
    /// The clock is the battle's own, as the replay's timeline states it, which
    /// is what a salvo in the incoming-fire log is stamped with.
    pub fn go_to_clock(&mut self, track: &Track, clock: GameClock) -> bool {
        let frame = track.frame_at(clock.seconds());
        self.go_to(track, frame)
    }

    /// Changes the speed. Returns whether it changed.
    ///
    /// A ticker's interval is fixed when it starts, so a front end restarts
    /// one that is running when this returns `true`.
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
        let here = SPEEDS.iter().position(|speed| (speed - self.speed).abs() < f32::EPSILON);
        let Some(here) = here else { return false };
        let Some(next) = here.checked_add_signed(step as isize).and_then(|index| SPEEDS.get(index)) else {
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

    /// Stops playback where it is.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// How long a running transport waits between ticks at this speed.
    pub fn tick_interval(&self) -> Duration {
        Duration::from_secs_f32(TICK.as_secs_f32() / self.speed.max(0.1))
    }

    /// Advances a running transport by one frame, stopping at the last.
    pub fn advance(&mut self, track: &Track) -> Tick {
        if !self.playing {
            return Tick::Idle;
        }
        let next = self.at.get() + 1;
        if next >= track.len() {
            self.playing = false;
            return Tick::Finished;
        }
        self.go_to(track, FrameIndex::new(next));

        Tick::Advanced
    }

    /// The game clock where playback is, in seconds.
    pub fn seconds(&self, track: &Track) -> f32 {
        track.seconds_at(self.at)
    }

    /// How far into the battle playback is, which is what an event's clock is
    /// measured against.
    pub fn elapsed(&self, track: &Track) -> f32 {
        track.elapsed_at(self.at)
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
    SPEEDS.iter().position(|offered| (offered - speed).abs() < f32::EPSILON).unwrap_or_default()
}

/// A speed as the transport labels it: no trailing zero on a whole one.
pub fn speed_label(speed: f32) -> String {
    if (speed - speed.round()).abs() < f32::EPSILON {
        format!("{}x", speed.round() as i32)
    } else {
        format!("{speed}x")
    }
}

/// Seconds of game time as the clock the game shows, zero-padded as the egui
/// renderer pads it.
pub fn mmss(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
