//! How a tracked player's encounter history reads: how many of them fall in
//! the period on screen, when the last one was, and how long ago that is.
//!
//! Both front ends draw these, so they are counted and worded here rather
//! than twice.

use jiff::Timestamp;
use jiff::Unit;
use jiff::ZonedDifference;
use jiff::tz::TimeZone;

use super::tracked::TrackedPlayer;

/// Human-readable "how long ago" for a past timestamp.
pub fn relative_age_text(timestamp: Timestamp, now: Timestamp) -> String {
    let timestamp = timestamp.to_zoned(TimeZone::system());
    let now = now.to_zoned(TimeZone::system());
    let delta = now
        .since(
            ZonedDifference::new(&timestamp)
                .smallest(Unit::Minute)
                .largest(Unit::Year)
                .mode(jiff::RoundMode::HalfExpand),
        )
        .expect("failed to calculate the age of an encounter timestamp");

    format!("{delta:#}")
}

/// Absolute local-time rendering of a timestamp, for the hover behind a
/// relative age.
pub fn exact_timestamp_text(timestamp: Timestamp) -> String {
    timestamp.to_zoned(TimeZone::system()).strftime("%Y-%m-%d %H:%M:%S").to_string()
}

/// Human-readable "how long ago" for an encounter timestamp. Empty when there
/// is no encounter to describe, which is the right degradation for a hover.
pub fn last_seen_text(last_seen: Option<Timestamp>, now: Timestamp) -> String {
    match last_seen {
        Some(last) => relative_age_text(last, now),
        None => String::new(),
    }
}

/// Absolute local-time stamp behind the relative "last seen" text. Empty for
/// the same reason [`last_seen_text`] is.
pub fn last_seen_timestamp_text(last_seen: Option<Timestamp>) -> String {
    last_seen.map(exact_timestamp_text).unwrap_or_default()
}

/// How many of a player's encounters fall inside the active period, counting
/// only the ones the division-mate toggle leaves visible. `since` is the
/// period's resolved boundary; `None` is the all-time period, where every
/// visible encounter counts.
pub fn encounters_in_range(player: &TrackedPlayer, since: Option<Timestamp>, show_division_mates: bool) -> usize {
    player.visible_timestamps(show_division_mates).filter(|ts| since.is_none_or(|since| *ts > since)).count()
}

/// Whether a player met inside the period was met only as a division mate,
/// which is what takes their row off the table while the toggle is off.
pub fn met_only_in_division(player: &TrackedPlayer, since: Option<Timestamp>) -> bool {
    encounters_in_range(player, since, false) == 0 && encounters_in_range(player, since, true) > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_tracker::tracked::TrackedPlayer;
    use jiff::ToSpan;
    use wows_replays::types::ArenaId;

    fn player(stamps: &[(Timestamp, bool)]) -> TrackedPlayer {
        let mut player = TrackedPlayer::default();
        for (ix, (timestamp, in_division)) in stamps.iter().enumerate() {
            let arena = ArenaId::from(ix as i64);
            player.timestamps.insert(*timestamp);
            player.arena_ids.insert(arena);
            if *in_division {
                player.division_encounters.mark(arena, *timestamp);
            }
        }
        player
    }

    #[test]
    fn the_all_time_period_counts_every_visible_encounter() {
        let now = Timestamp::now();
        let player = player(&[(now - 1.hour(), false), (now - (24 * 400).hours(), false)]);
        assert_eq!(encounters_in_range(&player, None, false), 2);
    }

    #[test]
    fn a_boundary_drops_the_encounters_before_it() {
        let now = Timestamp::now();
        let player = player(&[(now - 1.hour(), false), (now - (24 * 400).hours(), false)]);
        assert_eq!(encounters_in_range(&player, Some(now - 24.hours()), false), 1);
    }

    #[test]
    fn division_encounters_count_only_while_the_toggle_is_on() {
        let now = Timestamp::now();
        let player = player(&[(now - 1.hour(), true)]);
        assert_eq!(encounters_in_range(&player, None, false), 0);
        assert_eq!(encounters_in_range(&player, None, true), 1);
    }

    #[test]
    fn a_player_met_only_in_division_is_named_as_such() {
        let now = Timestamp::now();
        assert!(met_only_in_division(&player(&[(now - 1.hour(), true)]), None));
    }

    #[test]
    fn a_player_met_outside_division_is_not() {
        let now = Timestamp::now();
        assert!(!met_only_in_division(&player(&[(now - 1.hour(), true), (now - 2.hours(), false)]), None));
    }

    #[test]
    fn a_player_never_met_in_the_period_is_not_hidden_by_the_toggle() {
        let now = Timestamp::now();
        let player = player(&[(now - (24 * 400).hours(), true)]);
        assert!(!met_only_in_division(&player, Some(now - 24.hours())));
    }

    #[test]
    fn an_absent_last_encounter_reads_as_nothing_rather_than_a_guess() {
        assert!(last_seen_text(None, Timestamp::now()).is_empty());
        assert!(last_seen_timestamp_text(None).is_empty());
    }
}
