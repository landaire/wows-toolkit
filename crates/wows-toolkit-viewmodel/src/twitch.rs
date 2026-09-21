//! Matching a Twitch chat login to an in-game name.
//!
//! Someone who was in a streamer's chat minutes before meeting you in a
//! battle may have been watching your stream. Deciding which logins plausibly
//! name which player, and which observations are close enough in time to
//! count, is the same rule in both front ends; where the observations come
//! from is not.

use std::collections::HashMap;

use jiff::Timestamp;
use jiff::Unit;

/// How long after a battle started a chat observation still counts.
pub const WINDOW_AFTER_MINUTES: f64 = 20.0;

/// How long before it started one counts. Negative because the observation
/// precedes the battle: someone in chat just before queueing is the case this
/// exists for.
pub const WINDOW_BEFORE_MINUTES: f64 = -2.0;

/// Whether a Twitch chat `login` plausibly refers to the same person as
/// in-game name `ign`.
///
/// Matches if `ign` is longer than 5 bytes and within Levenshtein distance 3
/// of `login`, or if a 5-character chunk of `ign` is a substring of `login`.
///
/// The chunk arm compares `chunk.len() > 5`, a byte count against a chunk of
/// at most 5 characters: a chunk clears it only if one of those characters is
/// multi-byte, so an all-ASCII name never matches on a chunk while a name
/// carrying one accent or one Cyrillic letter can. That asymmetry is the rule
/// as the egui app has always applied it, and changing it here would change
/// which players it chips.
///
/// Carries no time-window filtering; callers apply that separately.
pub fn login_matches_ign(login: &str, ign: &str) -> bool {
    let name_chunks =
        ign.chars().collect::<Vec<char>>().chunks(5).map(|c| c.iter().collect::<String>()).collect::<Vec<String>>();

    (ign.len() > 5 && levenshtein::levenshtein(login, ign) <= 3)
        || name_chunks.iter().any(|chunk| if chunk.len() > 5 { login.contains(chunk) } else { false })
}

/// The logins that plausibly name `ign`, each with the observations close
/// enough in time to this battle to count.
///
/// `observations` is every login seen in chat and when, which is what both
/// the live poll and the persisted index carry. `None` when nothing matched,
/// which is what leaves a roster row without a chip.
pub fn potential_stream_snipers<'a>(
    observations: impl Iterator<Item = (&'a str, Timestamp)>,
    ign: &str,
    match_timestamp: Timestamp,
) -> Option<HashMap<String, Vec<Timestamp>>> {
    let mut results: HashMap<String, Vec<Timestamp>> = HashMap::new();

    for (login, seen_at) in observations {
        if !login_matches_ign(login, ign) {
            continue;
        }
        if !in_window(seen_at, match_timestamp) {
            continue;
        }
        results.entry(login.to_string()).or_default().push(seen_at);
    }

    if results.is_empty() { None } else { Some(results) }
}

/// Whether an observation at `seen_at` is close enough to a battle that
/// started at `match_timestamp` to count.
///
/// Inclusive at both ends: an observation is stored with its seconds
/// truncated, so one recorded a fraction inside the window lands exactly on
/// the boundary, and excluding the boundary would drop it.
pub fn in_window(seen_at: Timestamp, match_timestamp: Timestamp) -> bool {
    // A span this app cannot measure in minutes is not an observation it can
    // place against the battle, so it does not count.
    let Ok(minutes) = (seen_at - match_timestamp).total(Unit::Minute) else {
        return false;
    };
    (WINDOW_BEFORE_MINUTES..=WINDOW_AFTER_MINUTES).contains(&minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> Timestamp {
        Timestamp::from_second(1_700_000_000 + minutes * 60).expect("a valid timestamp")
    }

    #[test]
    fn a_login_within_three_edits_of_a_long_name_matches() {
        assert!(login_matches_ign("harvey635", "harvey635"));
        assert!(login_matches_ign("harvey_635", "harvey635"));
        assert!(!login_matches_ign("someoneelse", "harvey635"));
    }

    /// A short ASCII name clears neither arm: too short for the edit
    /// distance, and its chunks are never over five bytes.
    #[test]
    fn a_short_name_matches_nothing() {
        assert!(!login_matches_ign("abc", "abc"));
        assert!(!login_matches_ign("abcde", "abcde"));
    }

    /// The chunk arm needs a chunk over five *bytes*: an all-ASCII chunk is
    /// exactly five and never clears it, while a single multi-byte character
    /// inside one does.
    #[test]
    fn the_chunk_rule_reaches_only_multi_byte_names() {
        assert!(!login_matches_ign("xx_longplayername_yy", "longplayername"), "ASCII chunks are five bytes");
        // A Cyrillic name, written as escapes to keep the source ASCII.
        let name = "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}\u{438}\u{43a}";
        assert!(login_matches_ign(&format!("xx_{name}_yy"), name));

        // One accented character in an otherwise ASCII name is enough.
        let mixed = "J\u{fc}rgen_Klopp";
        assert!(login_matches_ign(&format!("xx_{mixed}_yy"), mixed), "one multi-byte character clears the arm");
    }

    #[test]
    fn the_window_opens_shortly_before_the_battle_and_closes_twenty_minutes_in() {
        let start = at(0);
        assert!(in_window(at(-1), start), "just before queueing counts");
        assert!(in_window(at(19), start));
        assert!(!in_window(at(-3), start), "too long before");
        assert!(!in_window(at(21), start), "after the battle would have ended");
        // The boundaries themselves count: a stored timestamp has its
        // seconds truncated onto them.
        assert!(in_window(at(-2), start));
        assert!(in_window(at(20), start));
    }

    #[test]
    fn only_matching_logins_inside_the_window_are_reported() {
        let start = at(0);
        let observations = [("harvey635", at(5)), ("harvey635", at(90)), ("stranger", at(5))];

        let found = potential_stream_snipers(observations.iter().map(|(l, t)| (*l, *t)), "harvey635", start)
            .expect("the login matches");

        assert_eq!(found.len(), 1, "only the matching login is reported");
        assert_eq!(found["harvey635"], vec![at(5)], "only the observation inside the window counts");
    }

    #[test]
    fn a_player_nobody_matched_gets_no_chip() {
        let observations = [("stranger", at(1))];
        assert!(potential_stream_snipers(observations.iter().map(|(l, t)| (*l, *t)), "harvey635", at(0)).is_none());
    }
}
