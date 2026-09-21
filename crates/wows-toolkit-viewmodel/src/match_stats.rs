//! The shipbuilds `match_stats` protocol: the request and response shapes,
//! the service's own limits, and the budget tracked against them.
//!
//! Both front ends speak this; each supplies its own HTTP client.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;
use wows_replays::analyzer::decoder::PlayerStateData;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_replays::types::GameParamId;

/// A region the stats service covers. The service holds no data for any other
/// realm, so a roster from one is never sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Eu,
    Na,
    Asia,
}

impl Region {
    /// The region a replay's realm string names, or `None` where the service
    /// has no data for it.
    pub fn from_realm(realm: &str) -> Option<Self> {
        match realm.to_ascii_lowercase().as_str() {
            "eu" => Some(Self::Eu),
            "na" => Some(Self::Na),
            "asia" => Some(Self::Asia),
            _ => None,
        }
    }

    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Eu => "eu",
            Self::Na => "na",
            Self::Asia => "asia",
        }
    }

    /// The form the region takes in a shipbuilds player URL.
    pub fn as_url_segment(self) -> &'static str {
        match self {
            Self::Eu => "EU",
            Self::Na => "NA",
            Self::Asia => "ASIA",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerRef {
    pub account_id: AccountId,
    pub region: Region,
    pub ship_id: GameParamId,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchStatsRequest {
    pub arena_id: ArenaId,
    pub players: Vec<PlayerRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerStatsStatus {
    Ok,
    Hidden,
    Unavailable,
    /// A status this client does not know. Kept so one unexpected value costs
    /// a single row rather than the whole response.
    #[serde(other)]
    Unknown,
}

/// One player's stats. `pr_tier` and `ship_pr_tier` are deliberately absent:
/// both bands are derived from their number through
/// `PersonalRatingCategory::from_pr`, so the chips here and the ones in the
/// replay inspector cannot disagree.
///
/// The fields carrying `#[serde(default)]` postdate the rest of the shape. A
/// server that has not deployed them yet omits the key entirely, and absent is
/// what `None` already means for every field here, so defaulting costs that one
/// cell rather than the whole roster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatsOut {
    pub account_id: AccountId,
    pub region: String,
    pub ship_id: GameParamId,
    pub status: PlayerStatsStatus,
    pub battles: Option<i64>,
    pub overall_win_rate: Option<f64>,
    #[serde(default)]
    pub overall_avg_damage: Option<i64>,
    pub ship_win_rate: Option<f64>,
    pub ship_battles: Option<i64>,
    #[serde(default)]
    pub ship_avg_damage: Option<i64>,
    #[serde(default)]
    pub ship_pr: Option<f64>,
    pub pr: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchStatsResponse {
    pub arena_id: ArenaId,
    pub players: Vec<PlayerStatsOut>,
}

/// Where the service answers, and how a request identifies itself. Part of
/// the protocol rather than of either front end's client.
pub const ENDPOINT: &str = "https://shipbuilds.com/api/match_stats";
pub const API_KEY: &str = "WTK-PleaseDontAbuseMyServer";
pub const CONTENT_TYPE: &str = "application/cbor";

/// The service's cap on one request's roster.
pub const MAX_PLAYERS: usize = 24;
/// The service's published budget, enforced here so a refusal costs no request.
pub const MAX_REQUESTS_PER_WINDOW: usize = 5;
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(20 * 60);

#[derive(Debug, thiserror::Error)]
pub enum MatchStatsError {
    #[error("the stats service has no data for realm {realm}")]
    UnsupportedRegion { realm: String },
    #[error("no players in this match can be looked up")]
    NoEligiblePlayers,
    #[error("{count} players is over the {MAX_PLAYERS} the service accepts")]
    TooManyPlayers { count: usize },
    #[error("rate limited, retry in {}s", retry_after.as_secs())]
    RateLimited { retry_after: Duration },
    #[error("this match was already tried and the service failed; retry in {}s", retry_after.as_secs())]
    RecentlyFailed { retry_after: Duration },
    #[error("the stats service answered {status}")]
    Http { status: u16 },
    #[error("could not reach the stats service: {0}")]
    Transport(String),
    #[error("could not encode the request: {0}")]
    Encode(String),
    #[error("could not decode the response: {0}")]
    Decode(String),
}

/// Turn a scanned roster into a request, or say why it cannot be one.
///
/// Every human in a match shares its realm, so an unsupported one rejects the
/// whole request rather than dropping players until the roster is empty.
pub fn build_request(arena_id: ArenaId, players: &[PlayerStateData]) -> Result<MatchStatsRequest, MatchStatsError> {
    let mut refs = Vec::new();
    for player in players.iter().filter(|player| !player.is_bot()) {
        let Some(realm) = player.realm() else {
            continue;
        };
        let Some(region) = Region::from_realm(realm) else {
            return Err(MatchStatsError::UnsupportedRegion { realm: realm.to_string() });
        };
        let Some(ship_id) = player.ship_params_id() else {
            continue;
        };
        refs.push(PlayerRef { account_id: player.db_id(), region, ship_id });
    }

    let request = MatchStatsRequest { arena_id, players: refs };
    request.validate()?;
    Ok(request)
}

impl MatchStatsRequest {
    /// Reject a roster the service would reject, before spending a request on
    /// finding that out.
    pub fn validate(&self) -> Result<(), MatchStatsError> {
        if self.players.is_empty() {
            return Err(MatchStatsError::NoEligiblePlayers);
        }
        if self.players.len() > MAX_PLAYERS {
            return Err(MatchStatsError::TooManyPlayers { count: self.players.len() });
        }
        Ok(())
    }
}

/// The service's budget, tracked locally. `now` is a parameter so the window
/// can be tested without waiting on the clock.
#[derive(Debug, Default)]
pub struct RateLimiter {
    sent: VecDeque<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// `Ok` when a request may be sent, `Err(wait)` with how long until the
    /// oldest request in the window ages out.
    pub fn check(&self, now: Instant) -> Result<(), Duration> {
        // `self.sent` is ordered oldest-first (`record` only pushes to the back),
        // so the first entry the window filter keeps is the oldest live one.
        // `front()` alone is not safe here: it can be a stale entry that already
        // aged out, which would understate the wait.
        let mut in_window = 0usize;
        let mut oldest_live = None;
        for sent in self.sent.iter().copied() {
            if now.duration_since(sent) < RATE_LIMIT_WINDOW {
                in_window += 1;
                oldest_live.get_or_insert(sent);
            }
        }
        if in_window < MAX_REQUESTS_PER_WINDOW {
            return Ok(());
        }
        // in_window >= MAX_REQUESTS_PER_WINDOW, which is > 0, so the loop above
        // saw at least one live entry and oldest_live is always Some here.
        let oldest = oldest_live.expect("in_window counted at least one live entry");
        Err(RATE_LIMIT_WINDOW.saturating_sub(now.duration_since(oldest)))
    }

    pub fn record(&mut self, now: Instant) {
        self.sent.push_back(now);
        while self.sent.front().is_some_and(|sent| now.duration_since(*sent) >= RATE_LIMIT_WINDOW) {
            self.sent.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Looks a key up in a decoded CBOR map without assuming key order.
    fn map_get<'a>(map: &'a [(ciborium::Value, ciborium::Value)], key: &str) -> Option<&'a ciborium::Value> {
        map.iter().find(|(k, _)| k.as_text() == Some(key)).map(|(_, v)| v)
    }

    #[test]
    fn a_request_round_trips_with_integer_ids() {
        let request = MatchStatsRequest {
            arena_id: ArenaId::from(9_876_543_210i64),
            players: vec![PlayerRef {
                account_id: AccountId(1_003_924_023),
                region: Region::Na,
                ship_id: GameParamId::from(4_179_539_664u64),
            }],
        };

        let mut bytes = Vec::new();
        ciborium::into_writer(&request, &mut bytes).expect("request encodes");
        // Decoded as a raw `Value`, not back into `MatchStatsRequest`, so this
        // pins the actual wire shape rather than only proving serde round-trips
        // with itself.
        let value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).expect("bytes are a value");

        let map = value.as_map().expect("request is a map");
        let arena_id = map_get(map, "arena_id").expect("arena_id present");
        assert_eq!(arena_id.as_integer().and_then(|i| i64::try_from(i).ok()), Some(9_876_543_210));

        let players = map_get(map, "players").expect("players present").as_array().expect("players is an array");
        let player = players[0].as_map().expect("a player is a map");
        let account_id = map_get(player, "account_id").expect("account_id present");
        assert_eq!(account_id.as_integer().and_then(|i| i64::try_from(i).ok()), Some(1_003_924_023));
        let ship_id = map_get(player, "ship_id").expect("ship_id present");
        assert_eq!(ship_id.as_integer().and_then(|i| u64::try_from(i).ok()), Some(4_179_539_664));
        let region = map_get(player, "region").expect("region present");
        assert_eq!(region.as_text(), Some("na"));
    }

    #[test]
    fn a_response_round_trips_and_keeps_nulls_as_none() {
        let response = MatchStatsResponse {
            arena_id: ArenaId::from(7i64),
            players: vec![PlayerStatsOut {
                account_id: AccountId(1),
                region: "eu".to_string(),
                ship_id: GameParamId::from(2u64),
                status: PlayerStatsStatus::Hidden,
                battles: None,
                overall_win_rate: None,
                overall_avg_damage: None,
                ship_win_rate: None,
                ship_battles: None,
                ship_avg_damage: None,
                ship_pr: None,
                pr: None,
            }],
        };

        let mut bytes = Vec::new();
        ciborium::into_writer(&response, &mut bytes).expect("response encodes");
        let decoded: MatchStatsResponse = ciborium::from_reader(bytes.as_slice()).expect("response decodes");

        assert_eq!(decoded.players[0].status, PlayerStatsStatus::Hidden);
        assert_eq!(decoded.players[0].battles, None);
        assert_eq!(decoded.players[0].pr, None);
        assert_eq!(decoded.players[0].ship_pr, None);
        assert_eq!(decoded.players[0].overall_avg_damage, None);
        assert_eq!(decoded.players[0].ship_avg_damage, None);
    }

    /// The full current shape decodes into every field, including the ones that
    /// postdate the original response.
    #[test]
    fn the_current_response_shape_decodes_every_field() {
        use ciborium::Value;

        let player = Value::Map(vec![
            (Value::Text("account_id".into()), Value::Integer(503_278_143i64.into())),
            (Value::Text("region".into()), Value::Text("eu".into())),
            (Value::Text("ship_id".into()), Value::Integer(4_273_911_792i64.into())),
            (Value::Text("status".into()), Value::Text("ok".into())),
            (Value::Text("battles".into()), Value::Integer(30_550i64.into())),
            (Value::Text("overall_win_rate".into()), Value::Float(63.5)),
            (Value::Text("overall_avg_damage".into()), Value::Integer(96_047i64.into())),
            (Value::Text("ship_win_rate".into()), Value::Float(58.0)),
            (Value::Text("ship_battles".into()), Value::Integer(816i64.into())),
            (Value::Text("ship_avg_damage".into()), Value::Integer(83_266i64.into())),
            (Value::Text("ship_pr".into()), Value::Float(2834.3)),
            (Value::Text("ship_pr_tier".into()), Value::Text("Super Unicum".into())),
            (Value::Text("pr".into()), Value::Float(2057.6)),
            (Value::Text("pr_tier".into()), Value::Text("Great".into())),
        ]);
        let document = Value::Map(vec![
            (Value::Text("arena_id".into()), Value::Integer(1_234_567_890i64.into())),
            (Value::Text("players".into()), Value::Array(vec![player])),
        ]);

        let mut bytes = Vec::new();
        ciborium::into_writer(&document, &mut bytes).expect("document encodes");

        let decoded: MatchStatsResponse = ciborium::from_reader(bytes.as_slice()).expect("response decodes");

        let player = &decoded.players[0];
        assert_eq!(player.battles, Some(30_550));
        assert_eq!(player.overall_avg_damage, Some(96_047));
        assert_eq!(player.ship_battles, Some(816));
        assert_eq!(player.ship_avg_damage, Some(83_266));
        assert_eq!(player.ship_pr, Some(2834.3));
        assert_eq!(player.pr, Some(2057.6));
    }

    /// The tier strings the server sends are ignored: both bands are derived
    /// from their number so the chips cannot disagree with the replay
    /// inspector's. They must not fail the decode either.
    #[test]
    fn the_server_supplied_tier_strings_are_ignored_not_rejected() {
        use ciborium::Value;

        let player = Value::Map(vec![
            (Value::Text("account_id".into()), Value::Integer(1i64.into())),
            (Value::Text("region".into()), Value::Text("eu".into())),
            (Value::Text("ship_id".into()), Value::Integer(2i64.into())),
            (Value::Text("status".into()), Value::Text("ok".into())),
            (Value::Text("battles".into()), Value::Integer(10i64.into())),
            (Value::Text("overall_win_rate".into()), Value::Float(50.0)),
            (Value::Text("ship_win_rate".into()), Value::Float(50.0)),
            (Value::Text("ship_battles".into()), Value::Integer(5i64.into())),
            (Value::Text("pr".into()), Value::Float(1500.0)),
            (Value::Text("pr_tier".into()), Value::Text("Good".into())),
            (Value::Text("ship_pr_tier".into()), Value::Text("Unicum".into())),
        ]);
        let document = Value::Map(vec![
            (Value::Text("arena_id".into()), Value::Integer(1i64.into())),
            (Value::Text("players".into()), Value::Array(vec![player])),
        ]);

        let mut bytes = Vec::new();
        ciborium::into_writer(&document, &mut bytes).expect("document encodes");

        let decoded: MatchStatsResponse = ciborium::from_reader(bytes.as_slice()).expect("response decodes");

        assert_eq!(decoded.players[0].pr, Some(1500.0));
    }

    /// A server still answering the previous shape omits the newer keys
    /// entirely. That must cost those cells, not the whole roster.
    #[test]
    fn a_response_without_the_newer_fields_still_decodes() {
        use ciborium::Value;

        let player = Value::Map(vec![
            (Value::Text("account_id".into()), Value::Integer(1i64.into())),
            (Value::Text("region".into()), Value::Text("eu".into())),
            (Value::Text("ship_id".into()), Value::Integer(2i64.into())),
            (Value::Text("status".into()), Value::Text("ok".into())),
            (Value::Text("battles".into()), Value::Integer(9000i64.into())),
            (Value::Text("overall_win_rate".into()), Value::Float(52.5)),
            (Value::Text("ship_win_rate".into()), Value::Float(48.0)),
            (Value::Text("ship_battles".into()), Value::Integer(120i64.into())),
            (Value::Text("pr".into()), Value::Float(1800.0)),
        ]);
        let document = Value::Map(vec![
            (Value::Text("arena_id".into()), Value::Integer(1i64.into())),
            (Value::Text("players".into()), Value::Array(vec![player])),
        ]);

        let mut bytes = Vec::new();
        ciborium::into_writer(&document, &mut bytes).expect("document encodes");

        let decoded: MatchStatsResponse = ciborium::from_reader(bytes.as_slice()).expect("response decodes");

        let player = &decoded.players[0];
        assert_eq!(player.battles, Some(9000), "the fields the old shape does carry still arrive");
        assert_eq!(player.overall_avg_damage, None);
        assert_eq!(player.ship_avg_damage, None);
        assert_eq!(player.ship_pr, None);
    }

    /// A status the server adds later must degrade one player's row, not fail
    /// the whole response and lose the other 23 players with it.
    #[test]
    fn an_unrecognised_status_decodes_as_unknown() {
        use ciborium::Value;

        let player = Value::Map(vec![
            (Value::Text("account_id".into()), Value::Integer(1i64.into())),
            (Value::Text("region".into()), Value::Text("eu".into())),
            (Value::Text("ship_id".into()), Value::Integer(2i64.into())),
            (Value::Text("status".into()), Value::Text("throttled".into())),
            (Value::Text("battles".into()), Value::Null),
            (Value::Text("overall_win_rate".into()), Value::Null),
            (Value::Text("ship_win_rate".into()), Value::Null),
            (Value::Text("ship_battles".into()), Value::Null),
            (Value::Text("pr".into()), Value::Null),
        ]);
        let document = Value::Map(vec![
            (Value::Text("arena_id".into()), Value::Integer(1i64.into())),
            (Value::Text("players".into()), Value::Array(vec![player])),
        ]);

        let mut bytes = Vec::new();
        ciborium::into_writer(&document, &mut bytes).expect("document encodes");

        let decoded: MatchStatsResponse = ciborium::from_reader(bytes.as_slice()).expect("response decodes");

        assert_eq!(decoded.players[0].status, PlayerStatsStatus::Unknown);
    }

    #[test]
    fn only_eu_na_and_asia_are_supported_regions() {
        assert_eq!(Region::from_realm("eu"), Some(Region::Eu));
        assert_eq!(Region::from_realm("NA"), Some(Region::Na));
        assert_eq!(Region::from_realm("asia"), Some(Region::Asia));
        assert_eq!(Region::from_realm("ru"), None);
        assert_eq!(Region::from_realm(""), None);
    }

    #[test]
    fn a_regions_wire_form_is_lowercase_and_its_url_form_uppercase() {
        assert_eq!(Region::Asia.as_wire(), "asia");
        assert_eq!(Region::Asia.as_url_segment(), "ASIA");
        assert_eq!(Region::Eu.as_wire(), "eu");
        assert_eq!(Region::Eu.as_url_segment(), "EU");
    }

    #[test]
    fn the_limiter_allows_the_first_five_requests_in_a_window() {
        let start = Instant::now();
        let mut limiter = RateLimiter::new();

        for i in 0..MAX_REQUESTS_PER_WINDOW {
            assert!(limiter.check(start).is_ok(), "request {i} must be allowed");
            limiter.record(start);
        }

        let Err(wait) = limiter.check(start) else {
            panic!("a sixth request inside the window must be refused");
        };
        assert!(wait <= RATE_LIMIT_WINDOW && !wait.is_zero(), "the reported wait must be inside the window");
    }

    /// Once every recorded request has aged out of the window, `check` must
    /// allow again. This does not on its own prove pruning happens correctly
    /// for a MIXED fresh/stale deque; see
    /// `the_limiter_reports_a_wait_from_the_oldest_live_request_not_a_stale_one`
    /// for that.
    #[test]
    fn check_allows_a_request_once_the_whole_window_has_elapsed() {
        let start = Instant::now();
        let mut limiter = RateLimiter::new();
        for _ in 0..MAX_REQUESTS_PER_WINDOW {
            limiter.record(start);
        }

        let later = start + RATE_LIMIT_WINDOW + Duration::from_secs(1);

        assert!(limiter.check(later).is_ok());
    }

    /// A deque can hold both a stale entry (older than the window) and enough
    /// live ones to be at cap. The reported wait must come from the oldest
    /// LIVE entry, not from `front()`, which may be the stale one and would
    /// wrongly report a zero wait while the limiter is genuinely full.
    #[test]
    fn the_limiter_reports_a_wait_from_the_oldest_live_request_not_a_stale_one() {
        let base = Instant::now();
        let mut limiter = RateLimiter::new();
        for _ in 0..MAX_REQUESTS_PER_WINDOW + 1 {
            limiter.record(base);
        }
        let fresh = base + Duration::from_secs(19 * 60);
        for _ in 0..MAX_REQUESTS_PER_WINDOW {
            limiter.record(fresh);
        }

        let later = base + Duration::from_secs(21 * 60);
        let Err(wait) = limiter.check(later) else {
            panic!("5 live requests from `fresh` must still refuse a 6th");
        };
        assert!(!wait.is_zero(), "the oldest live request has not aged out yet, so the wait must not be zero");
        assert!(wait <= RATE_LIMIT_WINDOW, "the reported wait must be inside the window");
    }

    /// One human roster entry, deserialized because `PlayerStateData`'s fields
    /// are crate-private to the parser. `raw_with_names` (which backs
    /// `ship_params_id()`) is `#[serde(skip_deserializing)]`, so the ship id
    /// is added afterward through the parser's public `update_from_dict`.
    fn player_state(name: &str, db_id: i64, realm: &str, ship_id: i64) -> PlayerStateData {
        let mut player: PlayerStateData = serde_json::from_value(serde_json::json!({
            "username": name,
            "clan": "RAIN",
            "clan_id": 7,
            "clan_color": 0,
            "db_id": db_id,
            "realm": realm,
            "player_id": 0,
            "entity_id": 0,
            "team_id": 0,
            "max_health": 40_000,
            "is_abuser": false,
            "is_hidden": false,
            "is_bot": false,
            "human_properties": {
                "avatar_id": 0,
                "prebattle_id": 0,
                "is_client_loaded": true,
                "is_connected": true,
            },
        }))
        .expect("the roster fixture matches PlayerStateData's shape");

        let mut ship_fields = HashMap::new();
        ship_fields.insert("shipParamsId", pickled::Value::I64(ship_id));
        player.update_from_dict(&ship_fields);
        player
    }

    /// One bot roster entry, with a realm and ship id that would pass the rest
    /// of `build_request`'s checks. This is deliberate: only `is_bot()` may be
    /// the reason a bot is dropped, so the fixture must not also fail the
    /// realm or ship-id checks, or a deleted `is_bot()` filter would still
    /// drop it for the wrong reason and the test would not catch the loss.
    fn bot_state(name: &str, db_id: i64, ship_id: i64) -> PlayerStateData {
        let mut player: PlayerStateData = serde_json::from_value(serde_json::json!({
            "username": name,
            "clan": "",
            "clan_id": 0,
            "clan_color": 0,
            "db_id": db_id,
            "realm": "na",
            "player_id": 0,
            "entity_id": 0,
            "team_id": 0,
            "max_health": 40_000,
            "is_abuser": false,
            "is_hidden": false,
            "is_bot": true,
            "human_properties": {
                "avatar_id": 0,
                "prebattle_id": 0,
                "is_client_loaded": true,
                "is_connected": true,
            },
        }))
        .expect("the bot fixture matches PlayerStateData's shape");

        let mut ship_fields = HashMap::new();
        ship_fields.insert("shipParamsId", pickled::Value::I64(ship_id));
        player.update_from_dict(&ship_fields);
        player
    }

    /// A realm the service does not cover must be refused here, not by a 400.
    #[test]
    fn an_unsupported_realm_stops_the_request() {
        let players = vec![player_state("Someone", 1, "ru", 100)];

        let error = build_request(ArenaId::from(1i64), &players).expect_err("ru is unsupported");

        assert!(matches!(error, MatchStatsError::UnsupportedRegion { .. }));
    }

    #[test]
    fn bots_are_left_out_of_the_request() {
        let players = vec![player_state("Human", 1, "eu", 100), bot_state("Bot", 200, 300)];

        let request = build_request(ArenaId::from(1i64), &players).expect("one human is enough");

        assert_eq!(request.players.len(), 1);
        assert_eq!(request.players[0].account_id, AccountId(1));
    }

    #[test]
    fn a_roster_of_only_bots_sends_nothing() {
        let players = vec![bot_state("Bot", 200, 300)];

        let error = build_request(ArenaId::from(1i64), &players).expect_err("bots are not lookups");

        assert!(matches!(error, MatchStatsError::NoEligiblePlayers));
    }

    #[test]
    fn a_roster_over_the_player_cap_is_refused_before_it_is_sent() {
        let players: Vec<PlayerRef> = (0..25)
            .map(|i| PlayerRef { account_id: AccountId(i), region: Region::Eu, ship_id: GameParamId::from(1u64) })
            .collect();
        let request = MatchStatsRequest { arena_id: ArenaId::from(1i64), players };

        assert!(matches!(request.validate(), Err(MatchStatsError::TooManyPlayers { count: 25 })));
    }

    #[test]
    fn an_empty_roster_is_refused_before_it_is_sent() {
        let request = MatchStatsRequest { arena_id: ArenaId::from(1i64), players: Vec::new() };

        assert!(matches!(request.validate(), Err(MatchStatsError::NoEligiblePlayers)));
    }
}
