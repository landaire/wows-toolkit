//! Client for the shipbuilds `match_stats` API, which answers a match roster
//! with each player's win rates, PR and battle counts.
//!
//! The protocol itself is `wows_toolkit_viewmodel::match_stats`, which the
//! GPUI port speaks too; this adds the HTTP client and its per-arena cache.

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;
use tracing::debug;
use wows_replays::types::ArenaId;

pub use wows_toolkit_viewmodel::match_stats::API_KEY;
pub use wows_toolkit_viewmodel::match_stats::CONTENT_TYPE;
pub use wows_toolkit_viewmodel::match_stats::ENDPOINT;
pub use wows_toolkit_viewmodel::match_stats::MatchStatsError;
pub use wows_toolkit_viewmodel::match_stats::MatchStatsRequest;
pub use wows_toolkit_viewmodel::match_stats::MatchStatsResponse;
pub use wows_toolkit_viewmodel::match_stats::PlayerRef;
pub use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
pub use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
pub use wows_toolkit_viewmodel::match_stats::RATE_LIMIT_WINDOW;
pub use wows_toolkit_viewmodel::match_stats::RateLimiter;
pub use wows_toolkit_viewmodel::match_stats::Region;

/// Fetches match stats, at most once per arena and within the service's
/// budget.
pub struct MatchStatsClient {
    http: crate::data::shipbuilds::ShipBuildsClient,
    limiter: RateLimiter,
    cache: HashMap<ArenaId, MatchStatsResponse>,
    /// Arenas that recently got a server-answered failure, and when. Consulted
    /// before the limiter so an outage does not spend the whole budget
    /// rediscovering itself on every retry.
    failed: HashMap<ArenaId, Instant>,
}

impl MatchStatsClient {
    pub fn new(http: crate::data::shipbuilds::ShipBuildsClient) -> Self {
        Self { http, limiter: RateLimiter::new(), cache: HashMap::new(), failed: HashMap::new() }
    }

    /// How long `arena_id`'s failure cooldown has left, or `None` if it never
    /// failed or the cooldown window has passed.
    fn cooldown_remaining(&self, arena_id: ArenaId, now: Instant) -> Option<Duration> {
        let failed_at = *self.failed.get(&arena_id)?;
        let elapsed = now.duration_since(failed_at);
        (elapsed < RATE_LIMIT_WINDOW).then(|| RATE_LIMIT_WINDOW - elapsed)
    }

    /// Starts a cooldown for `arena_id` if `err` came from a server that
    /// actually answered. `Transport` never reached the server and is likely
    /// transient, `RateLimited` costs no request either way, and the
    /// validation errors were never sent, so none of those cool down.
    fn note_failure(&mut self, arena_id: ArenaId, err: &MatchStatsError, now: Instant) {
        if matches!(err, MatchStatsError::Http { .. } | MatchStatsError::Decode(_)) {
            self.failed.insert(arena_id, now);
        }
    }

    pub fn fetch(&mut self, request: &MatchStatsRequest) -> Result<MatchStatsResponse, MatchStatsError> {
        if let Some(cached) = self.cache.get(&request.arena_id) {
            debug!(arena_id = ?request.arena_id, "match_stats cache hit, no request sent");
            return Ok(cached.clone());
        }

        let now = Instant::now();
        if let Some(retry_after) = self.cooldown_remaining(request.arena_id, now) {
            debug!(arena_id = ?request.arena_id, retry_after_secs = retry_after.as_secs(), "match_stats refused: recent failure cooldown");
            return Err(MatchStatsError::RecentlyFailed { retry_after });
        }

        request.validate()?;

        if let Err(retry_after) = self.limiter.check(now) {
            debug!(retry_after_secs = retry_after.as_secs(), "match_stats refused: rate limited");
            return Err(MatchStatsError::RateLimited { retry_after });
        }

        let mut body = Vec::new();
        ciborium::into_writer(request, &mut body).map_err(|e| MatchStatsError::Encode(e.to_string()))?;

        self.limiter.record(now);
        let response = self
            .http
            .http()
            .post(ENDPOINT)
            .header("X-API-Key", API_KEY)
            .header(reqwest::header::CONTENT_TYPE, CONTENT_TYPE)
            .body(body)
            .send()
            .map_err(|e| MatchStatsError::Transport(crate::util::http::error_chain(&e)))?;

        let status = response.status();
        debug!(status = status.as_u16(), body_len = ?response.content_length(), "match_stats response received");
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
            let err = MatchStatsError::Http { status: status.as_u16() };
            self.note_failure(request.arena_id, &err, now);
            return Err(err);
        }

        let bytes = response.bytes().map_err(|e| MatchStatsError::Transport(crate::util::http::error_chain(&e)))?;
        let decoded: MatchStatsResponse = match ciborium::from_reader(bytes.as_ref()) {
            Ok(decoded) => decoded,
            Err(e) => {
                let err = MatchStatsError::Decode(e.to_string());
                self.note_failure(request.arena_id, &err, now);
                return Err(err);
            }
        };

        self.cache.insert(request.arena_id, decoded.clone());
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> MatchStatsClient {
        MatchStatsClient::new(crate::data::shipbuilds::ShipBuildsClient::new(None).expect("test HTTP client"))
    }

    #[test]
    fn a_cooldown_within_the_window_refuses_without_a_request() {
        let start = Instant::now();
        let arena_id = ArenaId::from(1i64);
        let mut client = test_client();

        client.note_failure(arena_id, &MatchStatsError::Http { status: 500 }, start);

        let later = start + Duration::from_secs(60);
        let wait = client.cooldown_remaining(arena_id, later).expect("still within the cooldown window");
        assert!(!wait.is_zero() && wait <= RATE_LIMIT_WINDOW, "the reported wait must be inside the window");
    }

    #[test]
    fn a_cooldown_older_than_the_window_no_longer_refuses() {
        let start = Instant::now();
        let arena_id = ArenaId::from(1i64);
        let mut client = test_client();

        client.note_failure(arena_id, &MatchStatsError::Decode("bad cbor".to_string()), start);

        let later = start + RATE_LIMIT_WINDOW + Duration::from_secs(1);
        assert_eq!(client.cooldown_remaining(arena_id, later), None);
    }

    #[test]
    fn a_transport_failure_never_starts_a_cooldown() {
        let start = Instant::now();
        let arena_id = ArenaId::from(1i64);
        let mut client = test_client();

        client.note_failure(arena_id, &MatchStatsError::Transport("connection reset".to_string()), start);

        assert_eq!(client.cooldown_remaining(arena_id, start), None);
    }

    #[test]
    fn a_rate_limited_failure_never_starts_a_cooldown() {
        let start = Instant::now();
        let arena_id = ArenaId::from(1i64);
        let mut client = test_client();

        client.note_failure(arena_id, &MatchStatsError::RateLimited { retry_after: RATE_LIMIT_WINDOW }, start);

        assert_eq!(client.cooldown_remaining(arena_id, start), None);
    }

    #[test]
    fn validation_errors_never_start_a_cooldown() {
        let start = Instant::now();
        let arena_id = ArenaId::from(1i64);
        let mut client = test_client();

        client.note_failure(arena_id, &MatchStatsError::NoEligiblePlayers, start);
        client.note_failure(arena_id, &MatchStatsError::TooManyPlayers { count: 25 }, start);
        client.note_failure(arena_id, &MatchStatsError::UnsupportedRegion { realm: "ru".to_string() }, start);

        assert_eq!(client.cooldown_remaining(arena_id, start), None);
    }
}
