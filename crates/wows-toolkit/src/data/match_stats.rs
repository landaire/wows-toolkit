//! Client for the shipbuilds `match_stats` API, which answers a match roster
//! with each player's win rates, PR and battle counts.

use serde::Deserialize;
use serde::Serialize;
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

/// One player's stats. `pr_tier` is deliberately absent: the band is derived
/// from `pr` through `PersonalRatingCategory::from_pr`, so the chips here and
/// the ones in the replay inspector cannot disagree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatsOut {
    pub account_id: AccountId,
    pub region: String,
    pub ship_id: GameParamId,
    pub status: PlayerStatsStatus,
    pub battles: Option<i64>,
    pub overall_win_rate: Option<f64>,
    pub ship_win_rate: Option<f64>,
    pub ship_battles: Option<i64>,
    pub pr: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchStatsResponse {
    pub arena_id: ArenaId,
    pub players: Vec<PlayerStatsOut>,
}

use std::collections::HashMap;
use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

const ENDPOINT: &str = "https://shipbuilds.com/api/match_stats";
const API_KEY: &str = "WTK-PleaseDontAbuseMyServer";
const CONTENT_TYPE: &str = "application/cbor";

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
    #[error("the stats service answered {status}")]
    Http { status: u16 },
    #[error("could not reach the stats service: {0}")]
    Transport(String),
    #[error("could not encode the request: {0}")]
    Encode(String),
    #[error("could not decode the response: {0}")]
    Decode(String),
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

/// Fetches match stats, at most once per arena and within the service's
/// budget.
pub struct MatchStatsClient {
    http: reqwest::blocking::Client,
    limiter: RateLimiter,
    cache: HashMap<ArenaId, MatchStatsResponse>,
}

impl MatchStatsClient {
    pub fn new(http: reqwest::blocking::Client) -> Self {
        Self { http, limiter: RateLimiter::new(), cache: HashMap::new() }
    }

    pub fn fetch(&mut self, request: &MatchStatsRequest) -> Result<MatchStatsResponse, MatchStatsError> {
        if let Some(cached) = self.cache.get(&request.arena_id) {
            return Ok(cached.clone());
        }
        request.validate()?;

        let now = Instant::now();
        if let Err(retry_after) = self.limiter.check(now) {
            return Err(MatchStatsError::RateLimited { retry_after });
        }

        let mut body = Vec::new();
        ciborium::into_writer(request, &mut body).map_err(|e| MatchStatsError::Encode(e.to_string()))?;

        self.limiter.record(now);
        let response = self
            .http
            .post(ENDPOINT)
            .header("X-API-Key", API_KEY)
            .header(reqwest::header::CONTENT_TYPE, CONTENT_TYPE)
            .body(body)
            .send()
            .map_err(|e| MatchStatsError::Transport(crate::util::http::error_chain(&e)))?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // The service does not guarantee Retry-After on a 429; falling back to
            // the full window is the safe assumption when it is absent or malformed.
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(RATE_LIMIT_WINDOW);
            return Err(MatchStatsError::RateLimited { retry_after });
        }
        if !status.is_success() {
            return Err(MatchStatsError::Http { status: status.as_u16() });
        }

        let bytes = response.bytes().map_err(|e| MatchStatsError::Transport(crate::util::http::error_chain(&e)))?;
        let decoded: MatchStatsResponse =
            ciborium::from_reader(bytes.as_ref()).map_err(|e| MatchStatsError::Decode(e.to_string()))?;

        self.cache.insert(request.arena_id, decoded.clone());
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                ship_win_rate: None,
                ship_battles: None,
                pr: None,
            }],
        };

        let mut bytes = Vec::new();
        ciborium::into_writer(&response, &mut bytes).expect("response encodes");
        let decoded: MatchStatsResponse = ciborium::from_reader(bytes.as_slice()).expect("response decodes");

        assert_eq!(decoded.players[0].status, PlayerStatsStatus::Hidden);
        assert_eq!(decoded.players[0].battles, None);
        assert_eq!(decoded.players[0].pr, None);
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
