//! The match countdown, fitted to the whole-second `timeLeft` samples the
//! server sends.

use wows_replays::types::GameClock;
use wows_replays::types::TimeRemaining;

use crate::units::SecondsRemaining;

/// A BattleLogic `timeLeft` value and the clock it arrived at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeLeftSample {
    pub remaining: SecondsRemaining,
    pub clock: GameClock,
}

/// Whether the count runs at game rate or stands still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pace {
    Running,
    Held,
}

/// One stretch of the count, from `start` until the next segment starts.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    start: GameClock,
    remaining: SecondsRemaining,
    pace: Pace,
}

impl Segment {
    fn running_from(sample: TimeLeftSample) -> Self {
        Self { start: sample.clock, remaining: sample.remaining, pace: Pace::Running }
    }

    /// The count at `clock`. A clock before `start` reads as `start`, so a
    /// segment never runs backwards into time it did not cover.
    fn at(self, clock: GameClock) -> TimeRemaining {
        let from = TimeRemaining(self.remaining.0 as f32);
        match self.pace {
            Pace::Running => from.counted_down((clock - self.start).max(0.0)),
            Pace::Held => from,
        }
    }
}

/// The match countdown as one continuous count at game rate.
///
/// `timeLeft` arrives in whole seconds at irregular packet clocks. The count
/// runs down from an anchor sample, so it loses exactly one second per game
/// second whatever the packet timing. A later sample moves the anchor only
/// when it disagrees with the count by more than [`Countdown::TOLERANCE`], so
/// jitter leaves the tick phase alone and a real change, such as a new time
/// limit, is followed. The same value sent again a second or more later
/// means the server's count is held, as in a pause; the count stands at that
/// value until a sample shows a different one.
///
/// The segment before the current one is kept, so a frame drawn for a clock
/// shortly before the latest change still reads the count it had then.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Countdown {
    first: GameClock,
    previous: Option<Segment>,
    current: Segment,
    latest: TimeLeftSample,
}

impl Countdown {
    /// Seconds a sample may differ from the count and still agree with it:
    /// up to half a second of packet jitter, plus the second lost to rounding
    /// to whole seconds.
    pub const TOLERANCE: f32 = 1.5;

    /// Game seconds after which a repeated value means a held count. A
    /// running count changes its whole-second value once per second, so the
    /// server has no reason to resend an unchanged value sooner.
    pub const HOLD_AFTER: f32 = 1.0;

    pub fn new(sample: TimeLeftSample) -> Self {
        Self { first: sample.clock, previous: None, current: Segment::running_from(sample), latest: sample }
    }

    pub fn observe(&mut self, sample: TimeLeftSample) {
        // Packet clocks can step backwards; an older sample says nothing about
        // the count after the newer one.
        if sample.clock < self.latest.clock {
            return;
        }

        let last = std::mem::replace(&mut self.latest, sample);
        let repeated = sample.remaining == last.remaining;

        match self.current.pace {
            Pace::Held if repeated => {}
            Pace::Held => self.begin(Segment::running_from(sample)),
            Pace::Running if repeated && sample.clock - last.clock >= Self::HOLD_AFTER => {
                // Held from when the value was first seen.
                self.begin(Segment { start: last.clock, remaining: sample.remaining, pace: Pace::Held });
            }
            Pace::Running => {
                // Against the clamped count, so a zero sent after the count has
                // run out agrees with it.
                let drift = sample.remaining.0 as f32 - self.current.at(sample.clock).seconds();
                if drift.abs() > Self::TOLERANCE {
                    self.begin(Segment::running_from(sample));
                }
            }
        }
    }

    fn begin(&mut self, segment: Segment) {
        // A segment that would cover no time is replaced rather than kept as
        // the history.
        if segment.start > self.current.start {
            self.previous = Some(self.current);
        }
        self.current = segment;
    }

    /// The most recent sample, as the server sent it.
    pub fn latest(&self) -> TimeLeftSample {
        self.latest
    }

    /// The count at `clock`, which may fall between samples. `None` before
    /// the first sample. A clock before the kept history reads as the start
    /// of the oldest kept segment.
    pub fn remaining_at(&self, clock: GameClock) -> Option<TimeRemaining> {
        if clock < self.first {
            return None;
        }

        let segment = match self.previous {
            Some(previous) if clock < self.current.start => previous,
            _ => self.current,
        };
        Some(segment.at(clock))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(remaining: i64, clock: f32) -> TimeLeftSample {
        TimeLeftSample { remaining: SecondsRemaining(remaining), clock: GameClock(clock) }
    }

    fn shown(countdown: &Countdown, clock: f32) -> i64 {
        countdown.remaining_at(GameClock(clock)).expect("after the first sample").shown_seconds()
    }

    fn seconds(countdown: &Countdown, clock: f32) -> f32 {
        countdown.remaining_at(GameClock(clock)).expect("after the first sample").seconds()
    }

    /// A sample older than the latest one leaves the count and the held state
    /// alone.
    #[test]
    fn older_samples_are_ignored() {
        let mut countdown = Countdown::new(sample(300, 10.0));
        countdown.observe(sample(299, 11.0));
        countdown.observe(sample(500, 9.0));
        assert_eq!(shown(&countdown, 11.0), 299);

        // Taken as the latest, a stale 299 would make the resend at 11.2 look
        // a full second after it, and so a held count.
        countdown.observe(sample(299, 10.0));
        countdown.observe(sample(299, 11.2));
        assert_eq!(shown(&countdown, 14.0), 296);
    }

    /// Samples one per second, each late or early by up to 0.4 s, do not move
    /// the ticks: as a viewer sees it, with each sample taken in when its
    /// clock is reached, every tick lands exactly one game second after the
    /// last.
    #[test]
    fn jittered_samples_tick_once_per_game_second() {
        const JITTER: [f32; 8] = [0.0, 0.3, -0.25, 0.4, -0.4, 0.1, 0.35, -0.3];
        let samples: Vec<TimeLeftSample> =
            (1..=40).map(|index| sample(600 - index as i64, 100.0 + index as f32 + JITTER[index % 8])).collect();
        let mut countdown = Countdown::new(sample(600, 100.0));
        let mut pending = samples.iter().peekable();

        // Clocks in hundredths of a second, which f32 holds exactly at whole
        // seconds.
        let mut showing = 600;
        let mut ticks = Vec::new();
        for hundredths in 10_001..=14_000_u32 {
            let now = hundredths as f32 / 100.0;
            while let Some(next) = pending.next_if(|next| next.clock <= GameClock(now)) {
                countdown.observe(*next);
            }

            let now_showing = shown(&countdown, now);
            if now_showing != showing {
                ticks.push(hundredths);
                showing = now_showing;
            }
        }

        assert_eq!(ticks.len(), 40);
        assert_eq!(ticks[0], 10_100, "in phase with the first sample");
        assert!(ticks.windows(2).all(|pair| pair[1] - pair[0] == 100), "ticks at {ticks:?}");
        assert_eq!(showing, 560);
    }

    /// A sample far from the count is a real change, and the count restarts
    /// from it.
    #[test]
    fn a_jump_in_time_left_reanchors() {
        let mut countdown = Countdown::new(sample(600, 100.0));
        countdown.observe(sample(599, 101.2));

        countdown.observe(sample(900, 102.5));

        assert_eq!(seconds(&countdown, 102.5), 900.0);
        assert_eq!(shown(&countdown, 103.5), 899);
        assert_eq!(countdown.latest(), sample(900, 102.5));
    }

    /// A count the server holds, as in a pause, stands still at the held
    /// value for as long as it is resent, then runs again from the first
    /// sample that differs.
    #[test]
    fn a_held_count_stands_still_until_it_resumes() {
        let mut countdown = Countdown::new(sample(300, 50.0));
        let mut showing = Vec::new();
        for clock in 51..=54 {
            countdown.observe(sample(300, clock as f32));
            showing.push(shown(&countdown, clock as f32));
            showing.push(shown(&countdown, clock as f32 + 0.5));
        }
        assert_eq!(showing, [300; 8]);

        countdown.observe(sample(299, 60.0));

        assert_eq!(seconds(&countdown, 60.0), 299.0);
        assert_eq!(seconds(&countdown, 62.5), 296.5, "running again from the resumed sample");
        assert_eq!(shown(&countdown, 59.5), 300, "still held before the resumed sample");
    }

    #[test]
    fn the_count_stops_at_zero() {
        let mut countdown = Countdown::new(sample(2, 10.0));
        assert_eq!(seconds(&countdown, 15.0), 0.0);

        countdown.observe(sample(0, 14.0));

        assert_eq!(seconds(&countdown, 14.0), 0.0, "a late zero agrees with the count");
        assert_eq!(shown(&countdown, 11.5), 1);
    }

    #[test]
    fn there_is_no_count_before_the_first_sample() {
        let countdown = Countdown::new(sample(1200, 30.0));

        assert_eq!(countdown.remaining_at(GameClock(29.9)), None);
        assert_eq!(seconds(&countdown, 30.0), 1200.0);
    }

    /// A frame drawn for a clock before a new time limit arrived, as a bake
    /// does after taking the packet that carried it, reads the old count.
    #[test]
    fn a_clock_before_the_latest_change_reads_the_count_it_had_then() {
        let mut countdown = Countdown::new(sample(60, 0.0));

        countdown.observe(sample(1200, 30.0));

        assert_eq!(seconds(&countdown, 29.5), 30.5, "the pre-battle count");
        assert_eq!(seconds(&countdown, 31.0), 1199.0);
    }

    /// Only one change back is kept; anything older reads as the start of the
    /// oldest kept stretch rather than running a newer count backwards.
    #[test]
    fn a_clock_before_the_kept_history_reads_its_start() {
        let mut countdown = Countdown::new(sample(60, 0.0));
        countdown.observe(sample(1200, 30.0));

        countdown.observe(sample(900, 40.0));

        assert_eq!(seconds(&countdown, 10.0), 1200.0);
        assert_eq!(seconds(&countdown, 35.0), 1195.0);
        assert_eq!(seconds(&countdown, 41.0), 899.0);
    }
}
