//! The stats-service lookup for the battle in progress.
//!
//! The request rules are `wows_toolkit_viewmodel::match_stats`; what is here is
//! the budget a session spends and the request itself, a future for the tokio
//! runtime.

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::types::ArenaId;
use wows_toolkit_viewmodel::match_stats;
use wows_toolkit_viewmodel::match_stats::MatchStatsError;
use wows_toolkit_viewmodel::match_stats::MatchStatsResponse;
use wows_toolkit_viewmodel::match_stats::RateLimiter;
use wows_toolkit_viewmodel::match_stats::build_request;

use crate::http::UserAgent;

#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("the roster could not be read from the live replay")]
    NoRoster,
    #[error(transparent)]
    Refused(#[from] MatchStatsError),
    #[error(transparent)]
    Client(#[from] crate::http::HttpError),
    #[error("could not reach the stats service")]
    Transport(#[source] reqwest::Error),
}

/// The stats lookups this session has made.
///
/// Holds what the service's own client would: one answer per arena, the
/// budget spent so far, and which arenas recently failed. Consulted before a
/// request is sent, so a refusal costs nothing rather than a request the
/// service would reject.
#[derive(Default)]
pub struct StatsBudget {
    limiter: RateLimiter,
    answered: HashMap<ArenaId, MatchStatsResponse>,
    /// Arenas the service answered a failure for, and when. Checked before
    /// the limiter so an outage does not spend the whole budget
    /// rediscovering itself.
    failed: HashMap<ArenaId, Instant>,
}

impl StatsBudget {
    /// What this session already knows about `arena_id`, if anything.
    pub fn answered(&self, arena_id: ArenaId) -> Option<&MatchStatsResponse> {
        self.answered.get(&arena_id)
    }

    /// Whether a request for `arena_id` may be sent, and why not when it may
    /// not. Does not spend the budget; `record` does that.
    pub fn check(&self, arena_id: ArenaId, now: Instant) -> Result<(), MatchStatsError> {
        if let Some(failed_at) = self.failed.get(&arena_id).copied() {
            let elapsed = now.duration_since(failed_at);
            if elapsed < match_stats::RATE_LIMIT_WINDOW {
                return Err(MatchStatsError::RecentlyFailed { retry_after: match_stats::RATE_LIMIT_WINDOW - elapsed });
            }
        }

        self.limiter.check(now).map_err(|retry_after| MatchStatsError::RateLimited { retry_after })
    }

    /// Spends one request against the budget.
    pub fn record(&mut self, now: Instant) {
        self.limiter.record(now);
    }

    pub fn remember(&mut self, response: MatchStatsResponse) {
        self.answered.insert(response.arena_id, response);
    }

    /// Starts a cooldown when the service actually answered a failure.
    /// A transport error never reached it and is likely transient, a refusal
    /// cost no request either way, and a validation error was never sent.
    pub fn note_failure(&mut self, arena_id: ArenaId, error: &StatsError, now: Instant) {
        if matches!(error, StatsError::Refused(MatchStatsError::Http { .. } | MatchStatsError::Decode(_))) {
            self.failed.insert(arena_id, now);
        }
    }
}

/// Asks the stats service about this match's roster.
///
/// The caller holds the budget (see [`StatsBudget`]): this sends the request
/// it was cleared for.
pub async fn fetch_stats(
    agent: UserAgent,
    state: &ArenaState,
    proxy_url: &str,
) -> Result<MatchStatsResponse, StatsError> {
    let request = build_request(state.arena_id, &state.players)?;

    let mut body = Vec::new();
    ciborium::into_writer(&request, &mut body).map_err(|err| MatchStatsError::Encode(err.to_string()))?;

    // No redirects: a 3xx is classified rather than followed, so the API key
    // is never forwarded to whatever a redirect names.
    let client = crate::http::client(agent, proxy_url, reqwest::redirect::Policy::none())?;

    let response = client
        .post(match_stats::ENDPOINT)
        .header("X-API-Key", match_stats::API_KEY)
        .header(reqwest::header::CONTENT_TYPE, match_stats::CONTENT_TYPE)
        .body(body)
        .send()
        .await
        .map_err(StatsError::Transport)?;

    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        // The service does not guarantee Retry-After on a 429; the full
        // window is the safe assumption when it is absent or malformed.
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(match_stats::RATE_LIMIT_WINDOW);
        return Err(MatchStatsError::RateLimited { retry_after }.into());
    }
    if !status.is_success() {
        return Err(MatchStatsError::Http { status: status.as_u16() }.into());
    }

    let bytes = response.bytes().await.map_err(StatsError::Transport)?;
    ciborium::from_reader(bytes.as_ref()).map_err(|err| MatchStatsError::Decode(err.to_string()).into())
}

/// A refusal in the words a view shows.
///
/// A wait is reported in whole minutes rather than the raw seconds the error
/// carries, which is what the egui app tells the user too.
pub fn refusal_text(error: &StatsError) -> String {
    match error {
        StatsError::Refused(MatchStatsError::RateLimited { retry_after })
        | StatsError::Refused(MatchStatsError::RecentlyFailed { retry_after }) => {
            let minutes = retry_after.as_secs().div_ceil(60).max(1);
            if minutes == 1 {
                "rate limited, try again in 1 minute".to_string()
            } else {
                format!("rate limited, try again in {minutes} minutes")
            }
        }
        other => other.to_string(),
    }
}
