//! Matching a Twitch chat login to an in-game name.
//!
//! Someone who was in a streamer's chat minutes before meeting you in a
//! battle may have been watching your stream. Deciding which logins plausibly
//! name which player, and which observations are close enough in time to
//! count, is the same rule in both front ends; where the observations come
//! from is not.

use std::collections::HashMap;
use std::str::FromStr;

use jiff::Timestamp;
use jiff::Unit;
use serde::Deserialize;
use serde::Serialize;

/// How long after a battle started a chat observation still counts.
pub const WINDOW_AFTER_MINUTES: f64 = 20.0;

/// How long before it started one counts. Negative because the observation
/// precedes the battle: someone in chat just before queueing is the case this
/// exists for.
pub const WINDOW_BEFORE_MINUTES: f64 = -2.0;

/// A Twitch credential, as the helper that produces it writes it out.
///
/// Stored as JSON under [`keys::TOKEN`], which is what the egui app already
/// wrote, so both front ends read one credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    username: String,
    user_id: u64,
    client_id: String,
    oauth_token: String,
}

impl Token {
    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn user_id(&self) -> u64 {
        self.user_id
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn oauth_token(&self) -> &str {
        &self.oauth_token
    }
}

/// Why a pasted credential is not one.
///
/// Carries the offending field rather than a message, so a front end can say
/// which part of the paste was wrong instead of showing the whole string
/// back.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenParseError {
    #[error("the credential has no {field}")]
    Missing { field: &'static str },
    #[error("the credential carries an unknown field {key:?}")]
    UnknownField { key: String },
    #[error("{key:?} has no value")]
    ValueMissing { key: String },
    #[error("the user id {value:?} is not a number")]
    UserIdNotANumber { value: String },
}

impl FromStr for Token {
    type Err = TokenParseError;

    /// Parses the `key=value;key=value` form the credential helper emits.
    ///
    /// Every field is required: a credential missing one cannot be used, and
    /// accepting it would fail later against Twitch with nothing to point at.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut username = None;
        let mut user_id = None;
        let mut client_id = None;
        let mut oauth_token = None;

        for part in s.split(';') {
            if part.is_empty() {
                continue;
            }

            let mut split = part.split('=');
            let key = split.next().unwrap_or_default();
            let value = split.next().ok_or_else(|| TokenParseError::ValueMissing { key: key.to_string() })?;
            match key {
                "username" => username = Some(value.to_string()),
                "user_id" => {
                    user_id = Some(
                        value.parse().map_err(|_| TokenParseError::UserIdNotANumber { value: value.to_string() })?,
                    )
                }
                "client_id" => client_id = Some(value.to_string()),
                "oauth_token" => oauth_token = Some(value.to_string()),
                key => return Err(TokenParseError::UnknownField { key: key.to_string() }),
            }
        }

        Ok(Token {
            username: username.ok_or(TokenParseError::Missing { field: "username" })?,
            user_id: user_id.ok_or(TokenParseError::Missing { field: "user_id" })?,
            client_id: client_id.ok_or(TokenParseError::Missing { field: "client_id" })?,
            oauth_token: oauth_token.ok_or(TokenParseError::Missing { field: "oauth_token" })?,
        })
    }
}

/// Where the credential and the watched channel live in the shared config
/// database.
pub mod keys {
    /// The credential, stored as JSON.
    pub const TOKEN: &str = "twitch_token";
    /// The channel whose chat is polled. Empty means the credential's own.
    pub const MONITORED_CHANNEL: &str = "twitch_monitored_channel";
}

/// How often the chat is polled for its current viewers.
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(120);

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

/// One chat login that plausibly names a player, and when it was seen.
///
/// `minutes` is each sighting measured from the battle start, so a negative
/// figure is someone who was in chat before the battle began. Both front ends
/// show these in the chip's hover text; the wording is each one's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniperCandidate {
    pub login: String,
    pub minutes: Vec<i64>,
}

/// The chip's candidates for `ign`, alphabetical by login.
///
/// Alphabetical because the underlying match is a map with no stable order,
/// and the chip names one login: without an order it would name a different
/// one between frames. Empty when nothing matched, which is a row with no
/// chip.
pub fn sniper_candidates<'a>(
    observations: impl Iterator<Item = (&'a str, Timestamp)>,
    ign: &str,
    match_timestamp: Timestamp,
) -> Vec<SniperCandidate> {
    let Some(found) = potential_stream_snipers(observations, ign, match_timestamp) else {
        return Vec::new();
    };

    let mut candidates: Vec<SniperCandidate> = found
        .into_iter()
        .map(|(login, seen)| SniperCandidate {
            login,
            // A sighting this app cannot measure in minutes is reported as
            // the battle start rather than dropped: it was still a sighting.
            minutes: seen
                .into_iter()
                .map(|at| (at - match_timestamp).total(Unit::Minute).unwrap_or(0.0) as i64)
                .collect(),
        })
        .collect();
    candidates.sort_by(|a, b| a.login.cmp(&b.login));
    candidates
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

    #[test]
    fn a_credential_round_trips_through_its_pasted_form() {
        let token: Token = "username=harvey;user_id=42;client_id=abc;oauth_token=def".parse().expect("it parses");
        assert_eq!(token.username(), "harvey");
        assert_eq!(token.user_id(), 42);
        assert_eq!(token.client_id(), "abc");
        assert_eq!(token.oauth_token(), "def");
    }

    /// Each rejection names what was wrong, so the settings tab can say which
    /// part of the paste to fix rather than showing the paste back.
    #[test]
    fn a_malformed_credential_names_what_is_wrong() {
        assert_eq!(
            "user_id=42;client_id=abc;oauth_token=def".parse::<Token>(),
            Err(TokenParseError::Missing { field: "username" })
        );
        assert_eq!(
            "username=harvey;user_id=nope;client_id=abc;oauth_token=def".parse::<Token>(),
            Err(TokenParseError::UserIdNotANumber { value: "nope".to_string() })
        );
        assert_eq!(
            "username=harvey;surprise=1".parse::<Token>(),
            Err(TokenParseError::UnknownField { key: "surprise".to_string() })
        );
        assert_eq!("username".parse::<Token>(), Err(TokenParseError::ValueMissing { key: "username".to_string() }));
    }

    /// Pinned against what the egui app already stored.
    #[test]
    fn the_stored_credential_is_json_with_the_field_names() {
        let token: Token = "username=harvey;user_id=42;client_id=abc;oauth_token=def".parse().expect("it parses");
        let json = serde_json::to_string(&token).expect("it encodes");
        assert_eq!(json, r#"{"username":"harvey","user_id":42,"client_id":"abc","oauth_token":"def"}"#);
        assert_eq!(serde_json::from_str::<Token>(&json).expect("it decodes"), token);
    }

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
    fn candidates_are_alphabetical_and_carry_each_sighting() {
        let start = at(0);
        let observations = [
            ("harvey_635", at(3)),
            ("harvey635", at(-1)),
            ("harvey635", at(5)),
            ("stranger", at(2)),
            // Outside the window, so it is not a sighting at all.
            ("harvey635", at(90)),
        ];

        let candidates = sniper_candidates(observations.iter().map(|(l, t)| (*l, *t)), "harvey635", start);

        let logins: Vec<&str> = candidates.iter().map(|c| c.login.as_str()).collect();
        assert_eq!(logins, vec!["harvey635", "harvey_635"], "alphabetical, and the stranger is left out");

        let mut minutes = candidates[0].minutes.clone();
        minutes.sort();
        assert_eq!(minutes, vec![-1, 5], "each sighting inside the window, measured from the battle start");
    }

    #[test]
    fn a_player_nobody_matched_gets_no_candidates() {
        assert!(sniper_candidates([("stranger", at(1))].iter().map(|(l, t)| (*l, *t)), "harvey635", at(0)).is_empty());
    }

    #[test]
    fn a_player_nobody_matched_gets_no_chip() {
        let observations = [("stranger", at(1))];
        assert!(potential_stream_snipers(observations.iter().map(|(l, t)| (*l, *t)), "harvey635", at(0)).is_none());
    }
}
