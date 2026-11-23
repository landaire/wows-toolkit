//! The time scale shared by live playback and video export.
//!
//! Both are measured in game seconds per displayed second ([`PlaybackRate`]).
//! Live playback picks a rate from [`SPEED_LADDER`]. An export either picks a
//! rate as well ([`ExportSpeed`]) or fits its window into a [`VideoLength`],
//! which yields one.

use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::GameClock;

use crate::error::TimingError;

/// Frames per second of an exported video.
pub const FPS: f64 = 30.0;

/// Speeds offered for live playback and speed-mode export, in game seconds per
/// second. A battle runs twenty minutes, so the useful range is well above real
/// time.
pub const SPEED_LADDER: [f32; 6] = [1.0, 5.0, 10.0, 20.0, 40.0, 60.0];

/// Preset export lengths, in seconds of output video.
pub const VIDEO_LENGTH_LADDER: [u32; 5] = [30, 60, 120, 180, 300];

/// Game seconds that pass per second of displayed time.
///
/// Video export and playback speed-ups run the game clock faster than the
/// viewer's, and the reflow animation is paced in the viewer's time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackRate(f32);

impl PlaybackRate {
    pub const REAL_TIME: Self = Self(1.0);

    /// The speed live playback opens at, and the speed a speed-mode export
    /// starts from.
    pub const DEFAULT_SPEED: Self = Self(20.0);

    /// `None` unless the rate is finite and positive.
    pub const fn new(game_seconds_per_second: f32) -> Option<Self> {
        if game_seconds_per_second.is_finite() && game_seconds_per_second > 0.0 {
            Some(Self(game_seconds_per_second))
        } else {
            None
        }
    }

    pub const fn game_seconds_per_second(self) -> f32 {
        self.0
    }
}

/// The rate of a speed-mode export.
///
/// Never slower than real time: the encoder holds every frame until it writes
/// the file, so an export may not run longer than the stretch of battle it
/// shows.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct ExportSpeed(PlaybackRate);

impl ExportSpeed {
    /// The slowest export speed, the lowest rung of [`SPEED_LADDER`].
    pub const MIN: f32 = SPEED_LADDER[0];

    pub const DEFAULT: Self = Self(PlaybackRate::DEFAULT_SPEED);

    /// `None` unless the speed is finite and at least [`Self::MIN`].
    pub const fn new(game_seconds_per_second: f32) -> Option<Self> {
        if game_seconds_per_second.is_finite() && game_seconds_per_second >= Self::MIN {
            Some(Self(PlaybackRate(game_seconds_per_second)))
        } else {
            None
        }
    }

    pub const fn rate(self) -> PlaybackRate {
        self.0
    }

    pub const fn game_seconds_per_second(self) -> f32 {
        self.0.0
    }
}

impl TryFrom<f32> for ExportSpeed {
    type Error = TimingError;

    fn try_from(multiplier: f32) -> Result<Self, TimingError> {
        Self::new(multiplier).ok_or(TimingError::SpeedOutOfRange { multiplier, min: Self::MIN })
    }
}

impl From<ExportSpeed> for f32 {
    fn from(speed: ExportSpeed) -> f32 {
        speed.game_seconds_per_second()
    }
}

/// Seconds of output video an export is fitted into.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct VideoLength(f32);

impl VideoLength {
    /// The length an export is fitted into unless the reader picks another.
    pub const DEFAULT: Self = Self(60.0);

    /// The shortest length, a second of video.
    pub const MIN: f32 = 1.0;

    /// The longest length, twice the longest preset. The encoder holds every
    /// frame until it writes the file, so this bounds what an export holds.
    pub const MAX: f32 = 600.0;

    /// `None` unless the length is within [`Self::MIN`]..=[`Self::MAX`].
    pub const fn new(seconds: f32) -> Option<Self> {
        if seconds >= Self::MIN && seconds <= Self::MAX { Some(Self(seconds)) } else { None }
    }

    pub fn seconds(self) -> f32 {
        self.0
    }
}

impl TryFrom<f32> for VideoLength {
    type Error = TimingError;

    fn try_from(seconds: f32) -> Result<Self, TimingError> {
        Self::new(seconds).ok_or(TimingError::VideoLengthOutOfRange { seconds, min: Self::MIN, max: Self::MAX })
    }
}

impl From<VideoLength> for f32 {
    fn from(length: VideoLength) -> f32 {
        length.0
    }
}

/// How an export maps game time onto video time.
///
/// Every method takes the exported window in game seconds. Frame `n` of the
/// video shows the game clock `window start + n * frame_step(window)`. A
/// negative window counts as empty.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum ExportTiming {
    /// Fit the window into a video of this length.
    Length(VideoLength),
    /// Play the window at this speed, however long the video comes out.
    Speed(ExportSpeed),
}

impl ExportTiming {
    /// Frames the video holds. Always at least one, so an empty window still
    /// produces a picture.
    pub fn frame_count(self, window: f32) -> usize {
        let frames = match self {
            Self::Length(length) => (f64::from(length.seconds()) * FPS).round(),
            // Exact for every rate on the ladder, so a window that is a whole
            // number of frames long does not gain one from rounding.
            Self::Speed(speed) => {
                (f64::from(window.max(0.0)) * FPS / f64::from(speed.game_seconds_per_second())).ceil()
            }
        };

        (frames as usize).max(1)
    }

    /// Game seconds between consecutive frames.
    pub fn frame_step(self, window: f32) -> f32 {
        match self {
            Self::Length(_) => window.max(0.0) / self.frame_count(window) as f32,
            Self::Speed(speed) => (f64::from(speed.game_seconds_per_second()) / FPS) as f32,
        }
    }

    /// Game seconds each second of the video covers.
    pub fn rate(self, window: f32) -> PlaybackRate {
        match self {
            Self::Speed(speed) => speed.rate(),
            // An empty window plays no game time at all, so there is no rate to
            // report; real time keeps the animations paced by it well defined.
            Self::Length(_) => {
                PlaybackRate::new(self.frame_step(window) * FPS as f32).unwrap_or(PlaybackRate::REAL_TIME)
            }
        }
    }

    /// Seconds of video the export produces.
    pub fn output_seconds(self, window: f32) -> f32 {
        (self.frame_count(window) as f64 / FPS) as f32
    }
}

impl Default for ExportTiming {
    fn default() -> Self {
        Self::Length(VideoLength::DEFAULT)
    }
}

/// The stretch of a replay an export covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportWindow {
    start: GameClock,
    end: GameClock,
}

impl ExportWindow {
    /// `None` unless both ends are finite and `end` is not before `start`.
    pub fn new(start: GameClock, end: GameClock) -> Option<Self> {
        let finite = start.seconds().is_finite() && end.seconds().is_finite();
        if finite && end >= start { Some(Self { start, end }) } else { None }
    }

    pub fn start(self) -> GameClock {
        self.start
    }

    pub fn end(self) -> GameClock {
        self.end
    }

    /// Game seconds the window spans.
    pub fn seconds(self) -> f32 {
        self.end - self.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speed(multiplier: f32) -> ExportTiming {
        ExportTiming::Speed(ExportSpeed::new(multiplier).expect("an export speed"))
    }

    fn length(seconds: f32) -> ExportTiming {
        ExportTiming::Length(VideoLength::new(seconds).expect("a video length"))
    }

    #[test]
    fn length_mode_holds_its_frame_count_for_any_window() {
        let timing = length(30.0);

        for window in [0.0, 1.0, 480.0, 1200.0] {
            assert_eq!(timing.frame_count(window), 900);
            assert_eq!(timing.output_seconds(window), 30.0);
        }

        assert_eq!(length(45.5).frame_count(600.0), 1365);
    }

    #[test]
    fn length_mode_spreads_frames_over_the_window() {
        let timing = ExportTiming::default();

        assert_eq!(timing.frame_count(1200.0), 1800);
        assert_eq!(timing.frame_step(1200.0) * 1800.0, 1200.0);
        assert_eq!(timing.rate(1200.0).game_seconds_per_second(), 20.0);
    }

    #[test]
    fn a_tiny_window_still_fills_the_length() {
        let timing = length(60.0);

        assert_eq!(timing.frame_count(0.001), 1800);
        assert!(timing.frame_step(0.001) > 0.0);
        assert!(timing.rate(0.001).game_seconds_per_second() < 0.001);
    }

    #[test]
    fn an_empty_window_in_length_mode_plays_at_real_time() {
        let timing = length(60.0);

        assert_eq!(timing.frame_step(0.0), 0.0);
        assert_eq!(timing.rate(0.0), PlaybackRate::REAL_TIME);
    }

    #[test]
    fn speed_mode_on_a_long_battle() {
        let timing = speed(20.0);

        assert_eq!(timing.frame_count(1200.0), 1800);
        assert_eq!(timing.output_seconds(1200.0), 60.0);
        assert_eq!(timing.rate(1200.0).game_seconds_per_second(), 20.0);
        // A partial frame step at the end still gets its frame.
        assert_eq!(timing.frame_count(1187.3), 1781);
        assert_eq!(speed(60.0).frame_count(1200.0), 600);
    }

    #[test]
    fn a_window_shorter_than_one_frame_step_is_one_frame() {
        let timing = speed(20.0);
        let step = timing.frame_step(0.5);

        assert!(step > 0.5, "one frame covers {step} game seconds");
        assert_eq!(timing.frame_count(0.5), 1);
        assert_eq!(timing.frame_count(0.0), 1);
        assert_eq!(timing.frame_count(-3.0), 1);
        assert_eq!(timing.frame_count(0.7), 2);
    }

    #[test]
    fn stored_forms() {
        let written = serde_json::to_string(&ExportTiming::default()).expect("serializes");
        insta::assert_snapshot!(written, @r#"{"mode":"length","value":60.0}"#);

        let written = serde_json::to_string(&speed(20.0)).expect("serializes");
        insta::assert_snapshot!(written, @r#"{"mode":"speed","value":20.0}"#);

        let read: ExportTiming = serde_json::from_str(r#"{"mode":"speed","value":40}"#).expect("parses");
        assert_eq!(read, speed(40.0));
    }

    #[test]
    fn a_stored_timing_out_of_range_is_refused() {
        for stored in [
            r#"{"mode":"speed","value":0}"#,
            r#"{"mode":"speed","value":0.5}"#,
            r#"{"mode":"length","value":-5}"#,
            r#"{"mode":"length","value":0.5}"#,
            r#"{"mode":"length","value":601}"#,
        ] {
            assert!(serde_json::from_str::<ExportTiming>(stored).is_err(), "{stored}");
        }
    }

    /// An export runs no slower than real time and no longer than ten minutes
    /// in length mode, and a refused value is reported with the bound it broke.
    #[test]
    fn timing_values_are_held_to_their_bounds() {
        assert_eq!(ExportSpeed::try_from(1.0).map(f32::from), Ok(1.0));
        assert_eq!(ExportSpeed::try_from(0.5), Err(TimingError::SpeedOutOfRange { multiplier: 0.5, min: 1.0 }));
        assert!(ExportSpeed::try_from(f32::INFINITY).is_err());

        assert_eq!(VideoLength::try_from(1.0).map(f32::from), Ok(1.0));
        assert_eq!(VideoLength::try_from(600.0).map(f32::from), Ok(600.0));
        assert_eq!(
            VideoLength::try_from(0.01),
            Err(TimingError::VideoLengthOutOfRange { seconds: 0.01, min: 1.0, max: 600.0 })
        );
        assert!(VideoLength::try_from(601.0).is_err());
        assert!(VideoLength::try_from(f32::NAN).is_err());

        assert_eq!(speed(ExportSpeed::MIN).frame_count(3600.0), 108_000, "an hour at real time");
        assert_eq!(length(VideoLength::MAX).frame_count(3600.0), 18_000);
    }

    #[test]
    fn a_window_cannot_end_before_it_starts() {
        assert!(ExportWindow::new(GameClock(10.0), GameClock(9.0)).is_none());
        assert!(ExportWindow::new(GameClock(10.0), GameClock(f32::NAN)).is_none());

        let window = ExportWindow::new(GameClock(30.0), GameClock(1230.0)).expect("a window");
        assert_eq!(window.seconds(), 1200.0);
    }
}
