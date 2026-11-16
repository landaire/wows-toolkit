//! Where playback is in the clip, how fast it runs, and whether it runs.

use std::time::Duration;

use wows_replays::types::GameClock;

/// How many game seconds pass per second of wall time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Speed(u32);

impl Speed {
    /// The speeds the desktop viewer offers.
    pub const CHOICES: [Self; 6] = [Self(1), Self(5), Self(10), Self(20), Self(40), Self(60)];
    pub const DEFAULT: Self = Self(20);

    pub const fn new(multiplier: u32) -> Self {
        Self(multiplier)
    }

    pub fn label(self) -> String {
        format!("{}x", self.0)
    }
}

/// A frame's place in the clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameIndex(usize);

impl FrameIndex {
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Playback over the clocks of a non-empty clip.
pub struct Playback {
    clocks: Vec<GameClock>,
    position: GameClock,
    playing: bool,
    speed: Speed,
}

impl Playback {
    /// Playing from the first frame at the default speed. `None` for a clip
    /// with no frames.
    pub fn new(clocks: Vec<GameClock>) -> Option<Self> {
        let first = *clocks.first()?;

        Some(Self { clocks, position: first, playing: true, speed: Speed::DEFAULT })
    }

    pub fn start(&self) -> GameClock {
        self.clocks[0]
    }

    pub fn end(&self) -> GameClock {
        self.clocks[self.clocks.len() - 1]
    }

    pub fn position(&self) -> GameClock {
        self.position
    }

    /// The last frame drawn at or before the current position.
    pub fn frame(&self) -> FrameIndex {
        let after = self.clocks.partition_point(|clock| clock.seconds() <= self.position.seconds());
        FrameIndex(after.saturating_sub(1))
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    pub fn speed(&self) -> Speed {
        self.speed
    }

    pub fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
    }

    /// Moves on by `wall` of real time at the current speed. Running off the
    /// end starts the clip again, so an unattended demo keeps playing.
    pub fn advance(&mut self, wall: Duration) {
        // A tab returning from the background reports the whole time it was
        // hidden; the clip resumes where it was rather than leaping ahead.
        const LONGEST_STEP: Duration = Duration::from_millis(250);

        if !self.playing {
            return;
        }

        let game_seconds = wall.min(LONGEST_STEP).as_secs_f32() * self.speed.0 as f32;
        let next = self.position.seconds() + game_seconds;
        self.position = if next > self.end().seconds() { self.start() } else { GameClock(next) };
    }

    /// Moves to `clock`, held inside the clip.
    pub fn seek(&mut self, clock: GameClock) {
        self.position = GameClock(clock.seconds().clamp(self.start().seconds(), self.end().seconds()));
    }

    /// Pauses and moves `frames` frames forward, or back for a negative count.
    pub fn step(&mut self, frames: isize) {
        self.playing = false;
        let target = self.frame().0.saturating_add_signed(frames).min(self.clocks.len() - 1);
        self.position = self.clocks[target];
    }

    pub fn rewind(&mut self) {
        self.position = self.start();
    }
}

/// `m:ss` of battle time; a moment before the battle began reads negative.
pub fn elapsed_label(seconds: f32) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let whole = seconds.abs() as u32;

    format!("{sign}{}:{:02}", whole / 60, whole % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playback() -> Playback {
        Playback::new((0..10).map(|step| GameClock(100.0 + step as f32 * 0.5)).collect()).expect("frames")
    }

    #[test]
    fn elapsed_time_reads_as_the_battle_clock() {
        assert_eq!(elapsed_label(0.0), "0:00");
        assert_eq!(elapsed_label(125.7), "2:05");
        assert_eq!(elapsed_label(-4.2), "-0:04");
    }

    #[test]
    fn a_clip_with_no_frames_has_no_playback() {
        assert!(Playback::new(Vec::new()).is_none());
    }

    #[test]
    fn playing_moves_by_wall_time_times_the_speed() {
        let mut playback = playback();
        playback.set_speed(Speed::new(10));

        playback.advance(Duration::from_millis(100));

        assert_eq!(playback.position(), GameClock(101.0));
        assert_eq!(playback.frame(), FrameIndex(2));
    }

    #[test]
    fn running_off_the_end_starts_again() {
        let mut playback = playback();
        playback.seek(GameClock(104.4));
        playback.set_speed(Speed::new(5));

        playback.advance(Duration::from_millis(100));

        assert_eq!(playback.position(), playback.start());
    }

    #[test]
    fn a_long_gap_in_wall_time_moves_one_bounded_step() {
        let mut playback = playback();
        playback.set_speed(Speed::new(1));

        playback.advance(Duration::from_secs(30));

        assert_eq!(playback.position(), GameClock(100.25));
    }

    #[test]
    fn stepping_pauses_and_stops_at_either_end() {
        let mut playback = playback();

        playback.step(-3);
        assert!(!playback.is_playing());
        assert_eq!(playback.frame(), FrameIndex(0));

        playback.step(4);
        assert_eq!(playback.position(), GameClock(102.0));

        playback.step(40);
        assert_eq!(playback.position(), playback.end());
    }

    #[test]
    fn seeking_is_held_inside_the_clip() {
        let mut playback = playback();

        playback.seek(GameClock(50.0));
        assert_eq!(playback.position(), playback.start());

        playback.seek(GameClock(500.0));
        assert_eq!(playback.position(), playback.end());
    }
}
