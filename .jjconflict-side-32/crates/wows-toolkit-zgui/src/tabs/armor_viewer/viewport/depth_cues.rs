//! The ring sent out from a hit as it lands while a battle is followed.

use std::time::Duration;

use wows_replays::types::GameClock;

/// How long a landing hit's ring takes to spread and fade.
pub(super) const PULSE: Duration = Duration::from_millis(700);

/// The radius a ring starts at and spreads to, in CSS pixels.
pub(super) const PULSE_FROM: f32 = 4.0;
pub(super) const PULSE_TO: f32 = 22.0;

/// The hits `now` adds to `before` while a battle plays forward: those after the hits already
/// shown, when the earlier list, by the time of each hit, is where the new one starts. Empty for a
/// seek back or a different set.
pub(super) fn landed<'a, T>(before: &[T], now: &'a [T], clock: impl Fn(&T) -> GameClock) -> &'a [T] {
    let continues =
        now.len() > before.len() && before.iter().zip(now).all(|(earlier, later)| clock(earlier) == clock(later));
    if !continues {
        return &[];
    }

    &now[before.len()..]
}

/// A ring `progress` of the way from a dot to its widest: its radius and its opacity.
pub(super) fn pulse_ring(progress: f32) -> (f32, f32) {
    let progress = progress.clamp(0.0, 1.0);
    (PULSE_FROM + (PULSE_TO - PULSE_FROM) * progress, 0.9 * (1.0 - progress))
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameClock;

    use super::landed;
    use super::pulse_ring;

    /// Only hits after those already shown are new; a seek back, or a list that does not start
    /// with the shown hits, lands nothing.
    #[test]
    fn only_hits_after_the_shown_ones_land() {
        let clocks = |seconds: &[f32]| seconds.iter().copied().map(GameClock).collect::<Vec<_>>();
        let same = |clock: &GameClock| *clock;
        let shown = clocks(&[10., 20.]);

        assert_eq!(landed(&shown, &clocks(&[10., 20., 31., 32.]), same), clocks(&[31., 32.]).as_slice());
        assert_eq!(landed(&[], &clocks(&[5.]), same), clocks(&[5.]).as_slice());
        assert!(landed(&shown, &clocks(&[10.]), same).is_empty(), "a seek back");
        assert!(landed(&shown, &shown, same).is_empty(), "nothing new");
        assert!(landed(&shown, &clocks(&[11., 20., 30.]), same).is_empty(), "another set of hits");
    }

    #[test]
    fn a_ring_spreads_and_fades_out() {
        assert_eq!(pulse_ring(0.0), (4.0, 0.9));
        let (radius, opacity) = pulse_ring(1.0);
        assert_eq!(radius, 22.0);
        assert!(opacity.abs() < f32::EPSILON);
    }
}
