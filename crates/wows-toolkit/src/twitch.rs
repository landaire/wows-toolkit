use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt::Debug;
use std::str::FromStr;

use jiff::Timestamp;
use jiff::Unit;
use serde::Deserialize;
use serde::Serialize;
use twitch_api::HelixClient;
use twitch_api::helix;
use twitch_api::helix::chat::get_chatters;
use twitch_api::twitch_oauth2::TwitchToken;
use twitch_api::twitch_oauth2::UserToken;
use twitch_api::types::UserId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    username: String,
    user_id: u64,
    client_id: String,
    oauth_token: String,
}

// TODO: some features here may be desired.
#[allow(dead_code)]
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

impl FromStr for Token {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.split(';');
        let mut username = None;
        let mut user_id = None;
        let mut client_id = None;
        let mut oauth_token = None;

        for part in parts {
            if part.is_empty() {
                continue;
            }

            let mut split = part.split('=');
            let key = split.next().ok_or(())?;
            let value = split.next().ok_or(())?;
            match key {
                "username" => {
                    username = Some(value.to_string());
                }
                "user_id" => {
                    user_id = Some(value.parse().map_err(|_| ())?);
                }
                "client_id" => {
                    client_id = Some(value.to_string());
                }
                "oauth_token" => {
                    oauth_token = Some(value.to_string());
                }
                _ => {
                    return Err(());
                }
            }
        }

        Ok(Token {
            username: username.ok_or(())?,
            user_id: user_id.ok_or(())?,
            client_id: client_id.ok_or(())?,
            oauth_token: oauth_token.ok_or(())?,
        })
    }
}

#[derive(Debug)]
pub enum TwitchUpdate {
    Token(Token),
    User(String),
}

type ParticipantList = HashMap<String, BTreeSet<Timestamp>>;

#[derive(Default)]
pub struct TwitchState {
    client: HelixClient<'static, reqwest::Client>,
    pub token: Option<UserToken>,
    pub participants: ParticipantList,
    /// Set to true when a token validation attempt fails.
    pub token_validation_failed: bool,
}

/// Pure name-similarity predicate: does a Twitch chat `login` plausibly refer
/// to the same person as in-game name `ign`? Matches if `ign` is longer than
/// 5 characters and within Levenshtein distance 3 of `login`, or if any
/// 5-character chunk of `ign` (also longer than 5 chars, i.e. the final
/// undersized chunk is skipped) is a substring of `login`. Carries no
/// time-window filtering -- callers apply that separately.
pub fn login_matches_ign(login: &str, ign: &str) -> bool {
    let name_chunks =
        ign.chars().collect::<Vec<char>>().chunks(5).map(|c| c.iter().collect::<String>()).collect::<Vec<String>>();

    (ign.len() > 5 && levenshtein::levenshtein(login, ign) <= 3)
        || name_chunks.iter().any(|chunk| if chunk.len() > 5 { login.contains(chunk) } else { false })
}

impl TwitchState {
    pub fn token_is_valid(&self) -> bool {
        if let Some(token) = self.token.as_ref() { token.expires_in().as_secs() > 0 } else { false }
    }

    pub fn client(&self) -> &HelixClient<'static, reqwest::Client> {
        &self.client
    }

    pub fn player_is_potential_stream_sniper(
        &self,
        name: &str,
        match_timestamp: Timestamp,
    ) -> Option<HashMap<String, Vec<Timestamp>>> {
        let mut results = HashMap::new();

        for (viewer_name, viewer_timestamps) in &self.participants {
            if login_matches_ign(viewer_name, name) {
                let timestamps: Vec<_> = viewer_timestamps
                    .iter()
                    .cloned()
                    .filter(|timestamp| {
                        let delta = *timestamp - match_timestamp;
                        let minutes = delta.total(Unit::Minute).unwrap_or(0.0);

                        minutes < 20.0 && minutes > -2.0
                    })
                    .collect();

                if !timestamps.is_empty() {
                    results.insert(viewer_name.to_string(), timestamps);
                }
            }
        }

        if results.is_empty() { None } else { Some(results) }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    /// Helper: create a TwitchState with the given viewer entries.
    fn state_with_viewers(viewers: Vec<(&str, Vec<Timestamp>)>) -> TwitchState {
        let mut state = TwitchState::default();
        for (name, timestamps) in viewers {
            state.participants.insert(name.to_string(), timestamps.into_iter().collect());
        }
        state
    }

    /// Helper: create a timestamp offset by `minutes` from a base.
    fn ts_plus_minutes(base: Timestamp, minutes: i64) -> Timestamp {
        base.checked_add(jiff::SignedDuration::from_mins(minutes)).unwrap()
    }

    #[test]
    fn exact_name_match_within_window() {
        let match_ts = Timestamp::now();
        let viewer_ts = ts_plus_minutes(match_ts, 5);
        let state = state_with_viewers(vec![("Player1", vec![viewer_ts])]);

        // "Player1" has levenshtein distance 0 from "Player1" and len > 5
        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_some());
        let map = result.unwrap();
        assert!(map.contains_key("Player1"));
        assert_eq!(map["Player1"].len(), 1);
    }

    #[test]
    fn levenshtein_match_within_threshold() {
        let match_ts = Timestamp::now();
        let viewer_ts = ts_plus_minutes(match_ts, 3);
        // "PlayerX" vs "Player1" = distance 1 (≤ 3), both len > 5
        let state = state_with_viewers(vec![("PlayerX", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_some());
    }

    #[test]
    fn levenshtein_too_far_no_match() {
        let match_ts = Timestamp::now();
        let viewer_ts = ts_plus_minutes(match_ts, 3);
        // "ABCDEFGH" vs "Player1" = distance well above 3
        let state = state_with_viewers(vec![("ABCDEFGH", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_none());
    }

    #[test]
    fn short_name_ignored_for_levenshtein() {
        let match_ts = Timestamp::now();
        let viewer_ts = ts_plus_minutes(match_ts, 3);
        // name.len() <= 5, so levenshtein branch is skipped
        let state = state_with_viewers(vec![("ABCDE", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("ABCDE", match_ts);
        assert!(result.is_none());
    }

    #[test]
    fn timestamp_before_window_filtered_out() {
        let match_ts = Timestamp::now();
        // 3 minutes before match start (> -2 threshold)
        let viewer_ts = ts_plus_minutes(match_ts, -3);
        let state = state_with_viewers(vec![("Player1", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_none());
    }

    #[test]
    fn timestamp_after_window_filtered_out() {
        let match_ts = Timestamp::now();
        // 25 minutes after match start (> 20 threshold)
        let viewer_ts = ts_plus_minutes(match_ts, 25);
        let state = state_with_viewers(vec![("Player1", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_none());
    }

    #[test]
    fn timestamp_just_inside_lower_bound() {
        let match_ts = Timestamp::now();
        // -1 minute: within the -2..+20 window
        let viewer_ts = ts_plus_minutes(match_ts, -1);
        let state = state_with_viewers(vec![("Player1", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_some());
    }

    #[test]
    fn timestamp_at_boundary_19_minutes() {
        let match_ts = Timestamp::now();
        let viewer_ts = ts_plus_minutes(match_ts, 19);
        let state = state_with_viewers(vec![("Player1", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_some());
    }

    #[test]
    fn multiple_timestamps_partial_filter() {
        let match_ts = Timestamp::now();
        let ts_in = ts_plus_minutes(match_ts, 5);
        let ts_out = ts_plus_minutes(match_ts, 30);
        let state = state_with_viewers(vec![("Player1", vec![ts_in, ts_out])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_some());
        let map = result.unwrap();
        // Only the in-window timestamp should remain
        assert_eq!(map["Player1"].len(), 1);
        assert_eq!(map["Player1"][0], ts_in);
    }

    #[test]
    fn multiple_viewers_mixed_results() {
        let match_ts = Timestamp::now();
        let ts_in = ts_plus_minutes(match_ts, 5);
        let ts_out = ts_plus_minutes(match_ts, 30);
        let state = state_with_viewers(vec![
            ("Player1", vec![ts_in]),  // name match + in window
            ("ZZZZZZZ", vec![ts_in]),  // no name match
            ("Playe_1", vec![ts_out]), // name match but out of window
        ]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_some());
        let map = result.unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("Player1"));
    }

    #[test]
    fn no_participants_returns_none() {
        let state = TwitchState::default();
        let result = state.player_is_potential_stream_sniper("Player1", Timestamp::now());
        assert!(result.is_none());
    }

    #[test]
    fn minutes_calculation_is_correct() {
        // This is the core bug regression test: verify that time deltas
        // are computed as total minutes, not just the minutes component.
        let match_ts = Timestamp::now();
        let viewer_ts = ts_plus_minutes(match_ts, 10);
        let state = state_with_viewers(vec![("Player1", vec![viewer_ts])]);

        let result = state.player_is_potential_stream_sniper("Player1", match_ts);
        assert!(result.is_some(), "10 minutes should be within the -2..+20 window");

        // Verify the delta computes correctly (the original bug: get_minutes() returned 0)
        let delta = viewer_ts - match_ts;
        let total_mins = delta.total(Unit::Minute).unwrap();
        assert!((total_mins - 10.0).abs() < 0.01, "expected ~10.0 minutes, got {total_mins}");
    }

    #[test]
    fn login_matches_ign_exact_match() {
        // Identical strings: distance 0, len > 5.
        assert!(login_matches_ign("Player1", "Player1"));
    }

    #[test]
    fn login_matches_ign_close_name_within_threshold() {
        // "PlayerX" vs "Player1" = distance 1 (<= 3), both len > 5.
        assert!(login_matches_ign("PlayerX", "Player1"));
    }

    #[test]
    fn login_matches_ign_distant_name_no_match() {
        // "ABCDEFGH" vs "Player1" = distance well above 3.
        assert!(!login_matches_ign("ABCDEFGH", "Player1"));
    }

    #[test]
    fn login_matches_ign_short_name_skips_levenshtein_branch() {
        // ign.len() <= 5, so the levenshtein branch is skipped entirely, and
        // the 5-char chunk is not long enough to trigger the chunk branch either.
        assert!(!login_matches_ign("ABCDE", "ABCDE"));
    }

    #[test]
    fn login_matches_ign_no_overlap_no_match() {
        assert!(!login_matches_ign("CompletelyDifferent", "Player1"));
    }

    #[test]
    fn token_parse_roundtrip() {
        let input = "username=testuser;user_id=12345;client_id=abc123;oauth_token=tok456";
        let token: Token = input.parse().unwrap();
        assert_eq!(token.username(), "testuser");
        assert_eq!(token.user_id(), 12345);
        assert_eq!(token.client_id(), "abc123");
        assert_eq!(token.oauth_token(), "tok456");
    }

    #[test]
    fn token_parse_missing_field() {
        let input = "username=testuser;user_id=12345;client_id=abc123";
        assert!(input.parse::<Token>().is_err());
    }

    #[test]
    fn token_parse_unknown_field() {
        let input = "username=testuser;user_id=12345;client_id=abc123;oauth_token=tok456;extra=bad";
        assert!(input.parse::<Token>().is_err());
    }
}

pub async fn fetch_chatters(
    client: &HelixClient<'static, reqwest::Client>,
    user_id: &UserId,
    token: &UserToken,
) -> Result<Vec<String>, twitch_api::helix::ClientRequestError<reqwest::Error>> {
    let request = get_chatters::GetChattersRequest::new(user_id, &token.user_id);
    let response: Vec<helix::chat::Chatter> = client.req_get(request, token).await?.data;
    Ok(response.iter().map(|chatter| chatter.user_login.to_string()).collect())
}
