//! One ship's health over a battle, as a strip to draw beside its armor.
//!
//! The shape only: where the line goes, where the hits landed, and what a press
//! along it means in battle time. Both armor viewers draw the same strip from
//! this (`replay/realtime_armor_viewer.rs`'s `draw_health_timeline`).

use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replays::types::GameClock;

/// A health line, laid out across a strip.
///
/// Every position is a fraction of the strip's width or height, from zero at the
/// left or bottom, so the caller scales it to whatever room it has.
#[derive(Clone, Debug, PartialEq)]
pub struct HealthStrip {
    /// The battle clock at each end, which is what the strip is labelled with.
    pub first: GameClock,
    pub last: GameClock,
    /// The health line: `(along, height)` pairs, in the order they are joined.
    pub line: Vec<(f32, f32)>,
    /// Where each shell landed, along the strip.
    pub hits: Vec<f32>,
}

impl HealthStrip {
    /// Where `clock` falls along the strip, held to its ends.
    pub fn along(&self, clock: GameClock) -> f32 {
        let span = self.last.seconds() - self.first.seconds();
        if span <= 0.0 {
            return 0.0;
        }
        ((clock.seconds() - self.first.seconds()) / span).clamp(0.0, 1.0)
    }

    /// What battle clock a press `along` the strip names.
    pub fn clock_at(&self, along: f32) -> GameClock {
        let span = self.last.seconds() - self.first.seconds();
        GameClock(self.first.seconds() + along.clamp(0.0, 1.0) * span)
    }
}

/// The strip for one ship's battle.
///
/// `None` when nothing recorded its health, which is a ship the viewer has
/// nothing to draw a line for rather than a ship that stayed whole.
pub fn health_strip(timeline: &ShipShotTimeline) -> Option<HealthStrip> {
    let first = *timeline.health_history.keys().next()?;
    let last = *timeline.health_history.keys().next_back()?;
    // A battle recorded at one moment has no span to lay anything out along.
    let span = last.seconds() - first.seconds();
    if span <= 0.0 {
        return None;
    }

    let along = |clock: GameClock| ((clock.seconds() - first.seconds()) / span).clamp(0.0, 1.0);
    let line = timeline
        .health_history
        .iter()
        .map(|(clock, snapshot)| {
            // A ship whose maximum was never recorded reads as whole, which is
            // what it was before anything hit it.
            let ratio =
                if snapshot.max_health > 0.0 { (snapshot.health / snapshot.max_health).clamp(0.0, 1.0) } else { 1.0 };
            (along(*clock), ratio)
        })
        .collect();
    let hits = timeline.hits.iter().map(|hit| along(hit.clock)).collect();

    Some(HealthStrip { first, last, line, hits })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use wows_replay_insights::timeline::HealthSnapshot;

    use super::*;

    fn timeline(points: &[(f32, f32, f32)]) -> ShipShotTimeline {
        let health_history: BTreeMap<GameClock, HealthSnapshot> = points
            .iter()
            .map(|(at, health, max)| (GameClock(*at), HealthSnapshot { health: *health, max_health: *max }))
            .collect();
        ShipShotTimeline { hits: Vec::new(), health_history }
    }

    #[test]
    fn the_line_runs_from_one_end_of_the_strip_to_the_other() {
        let strip = health_strip(&timeline(&[(60.0, 40000.0, 40000.0), (180.0, 10000.0, 40000.0)]))
            .expect("two recorded moments are a strip");

        assert_eq!(strip.first, GameClock(60.0));
        assert_eq!(strip.last, GameClock(180.0));
        assert_eq!(strip.line, [(0.0, 1.0), (1.0, 0.25)]);
    }

    /// A ship recorded at one moment has no span to lay a line along.
    #[test]
    fn one_recorded_moment_is_not_a_strip() {
        assert_eq!(health_strip(&timeline(&[(60.0, 40000.0, 40000.0)])), None);
        assert_eq!(health_strip(&timeline(&[])), None);
    }

    /// A press along the strip names the battle clock under it, and back again.
    #[test]
    fn a_press_along_the_strip_names_a_moment_in_the_battle() {
        let strip = health_strip(&timeline(&[(60.0, 100.0, 100.0), (180.0, 50.0, 100.0)])).expect("a strip");

        assert_eq!(strip.clock_at(0.0), GameClock(60.0));
        assert_eq!(strip.clock_at(0.5), GameClock(120.0));
        assert_eq!(strip.clock_at(1.0), GameClock(180.0));
        // Past either end is held to it: a press in the margin is not a seek
        // past the battle.
        assert_eq!(strip.clock_at(-1.0), GameClock(60.0));
        assert_eq!(strip.clock_at(2.0), GameClock(180.0));

        assert_eq!(strip.along(GameClock(120.0)), 0.5);
        assert_eq!(strip.along(GameClock(0.0)), 0.0);
    }
}
