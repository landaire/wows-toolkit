//! The battle currently in progress.
//!
//! Where it is read from and when it counts as changed are shared
//! (`wows_toolkit_viewmodel::player_tracker::live_source`); this drives the
//! poll on the app's executor, scans the roster's identities, and asks the
//! stats service about it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::AsyncApp;
use gpui_kit::Task;
use wows_replays::ReplayFile;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::analyzer::arena_scan::scan_arena_state;
use wows_replays::types::ArenaId;
use wows_toolkit_viewmodel::match_stats;
use wows_toolkit_viewmodel::match_stats::MatchStatsError;
use wows_toolkit_viewmodel::match_stats::MatchStatsResponse;
use wows_toolkit_viewmodel::match_stats::RateLimiter;
use wows_toolkit_viewmodel::match_stats::build_request;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveSource;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveWatcher;
use wows_toolkit_viewmodel::player_tracker::live_source::POLL_INTERVAL;
use wows_toolkit_viewmodel::player_tracker::live_source::Settled;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;

/// Polls `replay_dir`, handing each change to `on_change`; `None` means no
/// battle is under way.
///
/// The task runs until the returned handle is dropped, which is what ends it
/// when the tab's directory changes.
pub fn watch(
    replay_dir: PathBuf,
    cx: &App,
    mut on_change: impl FnMut(Option<LiveMatch>, &mut AsyncApp) -> bool + 'static,
) -> Task<()> {
    let source = LiveSource::in_dir(&replay_dir);

    cx.spawn(async move |cx| {
        let mut watcher = LiveWatcher::new();

        loop {
            // `poll` is a blocking `metadata`, which on a network share or a
            // spun-down drive would stall a frame if it ran here.
            let stamp = {
                let source = source.clone();
                cx.background_spawn(async move { wows_toolkit_viewmodel::player_tracker::live_source::poll(&source) })
                    .await
            };

            if watcher.wants_read(stamp.as_ref()) {
                let read_source = source.clone();
                let read = match stamp {
                    Some(_) => Some(
                        cx.background_spawn(async move {
                            wows_toolkit_viewmodel::player_tracker::live_source::read(&read_source)
                        })
                        .await,
                    ),
                    None => None,
                };

                // The view is gone; so is any reason to keep polling.
                if let Settled::Report(live) = watcher.settle(stamp, read)
                    && !on_change(live, cx)
                {
                    return;
                }
            }

            cx.background_executor().timer(POLL_INTERVAL).await;
        }
    })
}

/// How often a battle in progress is re-read while waiting for its roster.
///
/// The game writes the packet stream in flushes, so the scan's target packet
/// may not be in the file yet on the first look.
pub const SCAN_RETRY_INTERVAL: Duration = Duration::from_secs(3);

/// How long to keep retrying before giving up on a battle's roster.
pub const SCAN_RETRY_BUDGET: Duration = Duration::from_secs(90);

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

#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("the roster could not be read from the live replay")]
    NoRoster,
    #[error(transparent)]
    Refused(#[from] MatchStatsError),
    #[error(transparent)]
    Client(#[from] wows_toolkit_replay_data::error::HttpError),
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
pub async fn fetch_stats(state: &ArenaState, proxy_url: &str) -> Result<MatchStatsResponse, StatsError> {
    let request = build_request(state.arena_id, &state.players)?;

    let mut body = Vec::new();
    ciborium::into_writer(&request, &mut body).map_err(|err| MatchStatsError::Encode(err.to_string()))?;

    // No redirects: a 3xx is classified rather than followed, so the API key
    // is never forwarded to whatever a redirect names.
    let client = crate::http::client(proxy_url, reqwest::redirect::Policy::none())?;

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

/// A refusal in the words the tab shows.
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
mod scan_tests {
    use super::*;
    use std::path::Path;
    use wows_toolkit_replay_data::load::GameDataCache;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
    use wows_toolkit_viewmodel::player_tracker::live_source::ARENA_INFO_FILE;
    use wows_toolkit_viewmodel::player_tracker::live_source::LIVE_STREAM_FILE;

    /// Splits a real replay back into the two halves the game writes during a
    /// battle, then reads the roster off them exactly as the live scan does.
    /// Needs a local game install and a replay recorded on an installed
    /// build. Run with:
    ///
    /// ```text
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY="E:\WoWs\World_of_Warships\replays\some.wowsreplay" \
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture the_live_scan_reads_a_real_replays_roster
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

        let dir = std::env::temp_dir().join(format!("wt-gpui-live-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is creatable");
        std::fs::write(dir.join(ARENA_INFO_FILE), replay.raw_meta.as_bytes()).expect("the arena info is writable");
        std::fs::write(dir.join(LIVE_STREAM_FILE), replay.packet_data()).expect("the stream is writable");

        let game_data = GameDataCache::new(PathBuf::from(&wows_dir));
        let loaded = game_data.get_or_load_build(build).expect("the build's game data loads");

        let state = scan_arena(&LiveSource::in_dir(&dir), loaded.provider()).expect("the roster scans");
        let humans = state.players.iter().filter(|player| !player.is_bot()).count();
        println!("arena {:?}: {} players, {humans} of them human", state.arena_id, state.players.len());

        assert!(!state.players.is_empty(), "a real battle has a roster");

        let identities = LiveIdentities::from_player_states(&state.players);
        assert_eq!(identities.by_name.len(), humans, "every human is named");

        let request = build_request(state.arena_id, &state.players);
        match request {
            Ok(request) => println!("{} players are eligible for a stats lookup", request.players.len()),
            Err(err) => println!("no stats lookup for this match: {err}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
