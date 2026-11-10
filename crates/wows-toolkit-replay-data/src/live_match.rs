//! The battle in progress: its roster's identities read off the live packet
//! stream, and the stats service asked about them.
//!
//! Where the battle is read from and when it counts as changed live in
//! `wows_toolkit_viewmodel::player_tracker::live_source`; each front end drives
//! the scan's retries and the request on its own executor.

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use wows_replays::ReplayFile;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::analyzer::arena_scan::scan_arena_state;
use wows_replays::types::ArenaId;
use wows_toolkit_viewmodel::match_stats;
use wows_toolkit_viewmodel::match_stats::MatchStatsError;
use wows_toolkit_viewmodel::match_stats::MatchStatsResponse;
use wows_toolkit_viewmodel::match_stats::RateLimiter;
use wows_toolkit_viewmodel::match_stats::build_request;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveSource;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::error::StatsError;

/// How often a battle in progress is re-read while waiting for its roster.
///
/// The game writes the packet stream in flushes, so the scan's target packet
/// may not be in the file yet on the first look.
pub const SCAN_RETRY_INTERVAL: Duration = Duration::from_secs(3);

/// How long to keep retrying before giving up on a battle's roster.
pub const SCAN_RETRY_BUDGET: Duration = Duration::from_secs(90);

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

/// Reads the arena roster off the live packet stream.
///
/// `None` means "not yet": the stream is short, still being written, or the
/// packet carrying the roster has not been flushed. Retried by the caller.
pub fn scan_arena(source: &LiveSource, provider: &GameMetadataProvider) -> Option<ArenaState> {
    let meta = std::fs::read(&source.arena_info).ok()?;
    let packets = std::fs::read(&source.stream).ok()?;
    // Both halves are re-read per attempt, so an attempt that raced the game
    // mid-write is corrected by the next one.
    let replay = ReplayFile::from_decrypted_parts(meta, packets).ok()?;

    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    scan_arena_state(provider.entity_specs(), version, &replay)
}

/// Asks the stats service about this match's roster.
///
/// The caller holds the budget (see [`StatsBudget`]): this sends the request
/// it was cleared for. `user_agent` names the front end asking; `proxy_url`
/// is `None` for a direct connection.
pub async fn fetch_stats(
    user_agent: &str,
    state: &ArenaState,
    proxy_url: Option<&str>,
) -> Result<MatchStatsResponse, StatsError> {
    let request = build_request(state.arena_id, &state.players)?;

    let mut body = Vec::new();
    ciborium::into_writer(&request, &mut body).map_err(|err| MatchStatsError::Encode(err.to_string()))?;

    // No redirects: a 3xx is classified rather than followed, so the API key
    // is never forwarded to whatever a redirect names.
    let client = wows_toolkit_viewmodel::http::client(user_agent, proxy_url, reqwest::redirect::Policy::none())?;

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

/// A refusal in the words the tracker shows.
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
    use wows_toolkit_viewmodel::player_tracker::live_source::ARENA_INFO_FILE;
    use wows_toolkit_viewmodel::player_tracker::live_source::LIVE_STREAM_FILE;

    use super::*;
    use crate::load::GameDataCache;

    fn response(arena: i64) -> MatchStatsResponse {
        MatchStatsResponse { arena_id: ArenaId::from(arena), players: Vec::new() }
    }

    #[test]
    fn a_session_answer_is_kept_per_arena() {
        let mut budget = StatsBudget::default();
        budget.remember(response(7));

        assert!(budget.answered(ArenaId::from(7)).is_some());
        assert!(budget.answered(ArenaId::from(8)).is_none());
    }

    #[test]
    fn the_budget_refuses_once_the_window_is_spent() {
        let mut budget = StatsBudget::default();
        let start = Instant::now();
        for _ in 0..match_stats::MAX_REQUESTS_PER_WINDOW {
            assert!(budget.check(ArenaId::from(1), start).is_ok());
            budget.record(start);
        }

        let refused = budget.check(ArenaId::from(1), start).expect_err("the window is spent");
        assert!(
            matches!(refused, MatchStatsError::RateLimited { retry_after } if retry_after == match_stats::RATE_LIMIT_WINDOW)
        );
        assert!(budget.check(ArenaId::from(1), start + match_stats::RATE_LIMIT_WINDOW).is_ok(), "the window ages out");
    }

    #[test]
    fn only_a_failure_the_service_answered_starts_a_cooldown() {
        let mut budget = StatsBudget::default();
        let now = Instant::now();
        let arena = ArenaId::from(3);

        budget.note_failure(arena, &StatsError::NoRoster, now);
        budget.note_failure(
            arena,
            &StatsError::Refused(MatchStatsError::RateLimited { retry_after: Duration::ZERO }),
            now,
        );
        budget.note_failure(arena, &StatsError::Refused(MatchStatsError::NoEligiblePlayers), now);
        assert!(budget.check(arena, now).is_ok(), "nothing the service answered failed");

        budget.note_failure(arena, &StatsError::Refused(MatchStatsError::Http { status: 500 }), now);
        let later = now + Duration::from_secs(60);
        let refused = budget.check(arena, later).expect_err("the arena cools down");
        assert!(matches!(
            refused,
            MatchStatsError::RecentlyFailed { retry_after } if retry_after == match_stats::RATE_LIMIT_WINDOW - Duration::from_secs(60)
        ));
        assert!(budget.check(ArenaId::from(4), later).is_ok(), "another arena is not held back");
    }

    #[test]
    fn a_wait_is_worded_in_whole_minutes() {
        let wait = |secs| StatsError::Refused(MatchStatsError::RateLimited { retry_after: Duration::from_secs(secs) });

        assert_eq!(refusal_text(&wait(0)), "rate limited, try again in 1 minute");
        assert_eq!(refusal_text(&wait(60)), "rate limited, try again in 1 minute");
        assert_eq!(refusal_text(&wait(61)), "rate limited, try again in 2 minutes");
        let failed = StatsError::Refused(MatchStatsError::RecentlyFailed { retry_after: Duration::from_secs(1200) });
        assert_eq!(refusal_text(&failed), "rate limited, try again in 20 minutes");
        assert_eq!(refusal_text(&StatsError::NoRoster), "the roster could not be read from the live replay");
    }

    /// A battle the game has not written a packet stream for yet, or whose
    /// stream does not hold the roster yet, is "not yet" rather than an error.
    #[test]
    fn a_battle_without_its_roster_packet_is_not_scanned_yet() {
        const ARENA_INFO: &str = r#"{
            "gameMode": 7, "clientVersionFromExe": "13, 11, 0, 12668706", "mapDisplayName": "ocean",
            "mapId": 1, "clientVersionFromXml": "13, 11, 0, 12668706", "duration": 1200, "gameLogic": null,
            "name": "12x12", "scenario": "Domination", "playerID": 0,
            "vehicles": [{"shipId": 100, "relation": 0, "id": 1, "name": "Harvey635"}],
            "playersPerTeam": 12, "dateTime": "28.12.2023 00:52:26", "mapName": "spaces/00_CO_ocean",
            "playerName": "Harvey635", "scenarioConfigId": 1, "teamsCount": 2, "logic": null,
            "playerVehicle": "PFSD110-Kleber"
        }"#;

        let dir = tempfile::tempdir().expect("a temporary directory");
        let provider = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider");
        let source = LiveSource::in_dir(dir.path());

        std::fs::write(dir.path().join(ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is written");
        assert!(scan_arena(&source, &provider).is_none(), "no stream yet");

        std::fs::write(dir.path().join(LIVE_STREAM_FILE), [0u8; 16]).expect("the stream is written");
        assert!(scan_arena(&source, &provider).is_none(), "the stream holds no roster");
    }

    /// Splits a real replay back into the two halves the game writes during a
    /// battle, then reads the roster off them exactly as the live scan does.
    /// Needs a local game install and a replay recorded on an installed
    /// build. Run with:
    ///
    /// ```text
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY="E:\WoWs\World_of_Warships\replays\some.wowsreplay" \
    /// cargo test -p wows-toolkit-replay-data -- --ignored --nocapture the_live_scan_reads_a_real_replays_roster
    /// ```
    #[test]
    #[ignore = "needs a local game install + a replay recorded on an installed build"]
    fn the_live_scan_reads_a_real_replays_roster() {
        let wows_dir = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR to a WoWs install directory");
        let replay_path = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY to a .wowsreplay path");

        let replay = ReplayFile::from_file(Path::new(&replay_path)).expect("the replay parses");
        let build = Version::from_client_exe(&replay.meta.clientVersionFromExe)
            .build_number()
            .expect("the replay names its build");

        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(dir.path().join(ARENA_INFO_FILE), replay.raw_meta.as_bytes())
            .expect("the arena info is writable");
        std::fs::write(dir.path().join(LIVE_STREAM_FILE), replay.packet_data()).expect("the stream is writable");

        let game_data = GameDataCache::new(PathBuf::from(&wows_dir));
        let loaded = game_data.get_or_load_build(build).expect("the build's game data loads");

        let state = scan_arena(&LiveSource::in_dir(dir.path()), loaded.provider()).expect("the roster scans");
        let humans = state.players.iter().filter(|player| !player.is_bot()).count();
        println!("arena {:?}: {} players, {humans} of them human", state.arena_id, state.players.len());

        assert!(!state.players.is_empty(), "a real battle has a roster");

        let identities = LiveIdentities::from_player_states(&state.players);
        assert_eq!(identities.by_name.len(), humans, "every human is named");

        match build_request(state.arena_id, &state.players) {
            Ok(request) => println!("{} players are eligible for a stats lookup", request.players.len()),
            Err(err) => println!("no stats lookup for this match: {err}"),
        }
    }
}
