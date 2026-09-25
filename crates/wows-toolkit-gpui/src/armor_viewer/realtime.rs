//! What a ship had taken by the moment a viewport is showing.
//!
//! The armor viewer draws hits on a hull. Driven from a replay, which hits it
//! should be drawing depends on where playback has reached, and playback can
//! be scrubbed in either direction. This is that bookkeeping, kept away from
//! the drawing so it can be reasoned about on its own: the egui app does the
//! same thing inside `RealtimeArmorBridge`, where it is tangled with the
//! viewer's own state.

use wows_replay_insights::timeline::PreExtractedHit;
use wows_replay_insights::timeline::ShipShotTimeline;
use wows_replays::types::GameClock;

/// One ship's hits, as far as playback has reached.
pub struct RealtimeArmorFeed {
    timeline: ShipShotTimeline,
    /// Where playback was when this was last moved. `None` before it has been
    /// moved at all, which is not the same as clock zero: a viewport that
    /// opens mid-battle has hits behind it already.
    at: Option<GameClock>,
}

/// What moving the clock turned up.
#[derive(Debug, PartialEq, Eq)]
pub enum Advance {
    /// Playback moved forward and these hits are new since the last move.
    Gained(std::ops::Range<usize>),
    /// Playback moved backwards, so what was drawn is no longer right and the
    /// hull has to be drawn again from nothing.
    Rewound,
}

impl RealtimeArmorFeed {
    pub fn new(timeline: ShipShotTimeline) -> Self {
        Self { timeline, at: None }
    }

    /// Every hit this ship had taken by where playback has reached.
    pub fn taken(&self) -> &[PreExtractedHit] {
        match self.at {
            Some(clock) => self.timeline.hits_through(clock),
            None => &[],
        }
    }

    /// What this ship's health was at that moment.
    pub fn health(&self) -> Option<f32> {
        self.timeline.health_at(self.at?).map(|snapshot| snapshot.health)
    }

    /// Where playback has reached, if it has been said.
    pub fn at(&self) -> Option<GameClock> {
        self.at
    }

    /// Moves to `clock` and says what changed.
    ///
    /// A step forward is the hits it gained, as a range into [`Self::taken`],
    /// so a caller draws those rather than all of them again. A step
    /// backwards cannot be undrawn hit by hit, so it says so and the caller
    /// starts the hull again.
    pub fn advance_to(&mut self, clock: GameClock) -> Advance {
        let was = self.at;
        self.at = Some(clock);
        match was {
            Some(was) if clock.seconds() < was.seconds() => Advance::Rewound,
            Some(was) => {
                let from = self.timeline.hits_through(was).len();
                let to = self.timeline.hits_through(clock).len();
                Advance::Gained(from..to)
            }
            // The first move is everything behind it, which is a rewind's
            // answer: draw the hull from nothing.
            None => Advance::Rewound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use wows_replay_insights::timeline::HealthSnapshot;

    fn feed() -> RealtimeArmorFeed {
        let mut health_history = BTreeMap::new();
        health_history.insert(GameClock(0.0), HealthSnapshot { health: 15400.0, max_health: 15400.0 });
        health_history.insert(GameClock(30.0), HealthSnapshot { health: 12000.0, max_health: 15400.0 });
        RealtimeArmorFeed::new(ShipShotTimeline { hits: Vec::new(), health_history })
    }

    /// The first move draws the hull from nothing, because a viewport can
    /// open anywhere in the battle and there may already be hits behind it.
    #[test]
    fn the_first_move_starts_the_hull_again() {
        let mut feed = feed();
        assert!(feed.at().is_none());
        assert_eq!(feed.advance_to(GameClock(60.0)), Advance::Rewound);
        assert_eq!(feed.at(), Some(GameClock(60.0)));
    }

    /// Stepping forward reports what it gained rather than everything.
    #[test]
    fn a_step_forward_reports_only_what_it_gained() {
        let mut feed = feed();
        feed.advance_to(GameClock(10.0));
        assert_eq!(feed.advance_to(GameClock(40.0)), Advance::Gained(0..0), "no hits in this timeline");
    }

    /// Scrubbing backwards cannot be undrawn hit by hit.
    #[test]
    fn a_step_backwards_asks_for_the_hull_again() {
        let mut feed = feed();
        feed.advance_to(GameClock(60.0));
        assert_eq!(feed.advance_to(GameClock(20.0)), Advance::Rewound);
    }

    /// Health follows the clock, holding its last reading.
    #[test]
    fn health_follows_where_playback_reached() {
        let mut feed = feed();
        assert!(feed.health().is_none(), "before playback has been placed");

        feed.advance_to(GameClock(10.0));
        assert_eq!(feed.health(), Some(15400.0));

        feed.advance_to(GameClock(45.0));
        assert_eq!(feed.health(), Some(12000.0));

        feed.advance_to(GameClock(5.0));
        assert_eq!(feed.health(), Some(15400.0), "and it follows a scrub backwards too");
    }
}
