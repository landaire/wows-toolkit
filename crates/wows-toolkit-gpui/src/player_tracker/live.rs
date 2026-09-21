//! The battle currently in progress.
//!
//! The game writes `tempArenaInfo.json` into its replays directory when a
//! battle starts and removes it when the battle ends, so the file's presence
//! is the signal. Its contents are the same metadata a finished replay
//! carries, which is what `LiveMatch::from_meta` reads.
//!
//! **Polled, not watched.** A filesystem watcher reports changes only, so a
//! battle already under way when the app starts raises no event; the egui app
//! covers that with a separate startup check. One poll covers both, and at
//! this interval costs a `stat` per tick.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::AsyncApp;
use gpui_kit::Task;
use wows_replays::ReplayFile;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::analyzer::arena_scan::scan_arena_state;
use wows_toolkit_viewmodel::match_stats;
use wows_toolkit_viewmodel::match_stats::MatchStatsError;
use wows_toolkit_viewmodel::match_stats::MatchStatsResponse;
use wows_toolkit_viewmodel::match_stats::build_request;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;

/// How often the replays directory is checked for a battle in progress.
///
/// A battle lasts twenty minutes, so noticing one a couple of seconds late
/// costs nothing; polling faster only spends more syscalls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The file the game writes for the duration of a battle.
pub const ARENA_INFO_FILE: &str = "tempArenaInfo.json";

/// The bare packet stream the game writes beside it. Wrapped into a replay
/// file only once the battle ends.
pub const LIVE_STREAM_FILE: &str = "temp.wowsreplay";

/// Where a battle in progress keeps its two halves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSource {
    pub arena_info: PathBuf,
    pub stream: PathBuf,
}

impl LiveSource {
    /// The pair inside `replay_dir`, whether or not a battle is under way.
    pub fn in_dir(replay_dir: &Path) -> Self {
        Self { arena_info: replay_dir.join(ARENA_INFO_FILE), stream: replay_dir.join(LIVE_STREAM_FILE) }
    }
}

/// What one poll found.
///
/// Size and modified time together distinguish one battle from the next: the
/// game rewrites the file per battle, and a player who returns to port and
/// queues again produces a second file where the first had been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArenaInfoStamp {
    pub modified: SystemTime,
    pub len: u64,
}

/// The stamp of the battle in progress, or `None` when there is none.
pub fn poll(source: &LiveSource) -> Option<ArenaInfoStamp> {
    let metadata = std::fs::metadata(&source.arena_info).ok()?;
    if !metadata.is_file() {
        return None;
    }
    Some(ArenaInfoStamp { modified: metadata.modified().ok()?, len: metadata.len() })
}

#[derive(Debug, thiserror::Error)]
pub enum LiveMatchError {
    #[error("the arena info could not be read")]
    Read(#[source] std::io::Error),
    #[error("the arena info is not a replay header: {0}")]
    Parse(String),
}

/// Reads the battle in progress.
///
/// The game flushes this file at match start and may not have finished, so a
/// parse failure is an ordinary "not yet" for the next poll to retry rather
/// than a permanent error.
pub fn read(source: &LiveSource) -> Result<LiveMatch, LiveMatchError> {
    let bytes = std::fs::read(&source.arena_info).map_err(LiveMatchError::Read)?;
    let replay =
        ReplayFile::from_decrypted_parts(bytes, Vec::new()).map_err(|err| LiveMatchError::Parse(format!("{err:?}")))?;
    Ok(LiveMatch::from_meta(&replay.meta))
}

/// Polls `replay_dir`, handing each change to `on_change`; `None` means no
/// battle is under way.
///
/// The task runs until the returned handle is dropped, which is what ends it
/// when the tab's directory changes.
pub fn watch(
    replay_dir: PathBuf,
    cx: &App,
    mut on_change: impl FnMut(Option<LiveMatch>, &mut AsyncApp) + 'static,
) -> Task<()> {
    let source = LiveSource::in_dir(&replay_dir);

    cx.spawn(async move |cx| {
        let mut current: Option<ArenaInfoStamp> = None;
        // Nothing has been reported yet, so the first poll reports even when
        // it finds no battle: the tab shows "checking" until it does.
        let mut reported = false;

        loop {
            let stamp = poll(&source);
            if stamp != current || !reported {
                let read_source = source.clone();
                let live = match stamp {
                    Some(_) => match cx.background_spawn(async move { read(&read_source) }).await {
                        Ok(live) => Some(live),
                        Err(err) => {
                            tracing::debug!("live match: the arena info is not readable yet: {err}");
                            None
                        }
                    },
                    None => None,
                };

                // A half-written file leaves `current` alone, so the next
                // poll sees the same stamp as a change and retries it.
                let readable = stamp.is_none() || live.is_some();
                if readable {
                    current = stamp;
                    reported = true;
                    on_change(live, cx);
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
    #[error("the stats client could not be built")]
    Client(#[source] reqwest::Error),
    #[error("could not reach the stats service")]
    Transport(#[source] reqwest::Error),
}

/// Asks the stats service about this match's roster.
///
/// The service's own budget is small, so a match is asked about once: the
/// caller holds the answer for the battle's duration rather than polling.
pub async fn fetch_stats(state: &ArenaState, proxy_url: &str) -> Result<MatchStatsResponse, StatsError> {
    let request = build_request(state.arena_id, &state.players)?;

    let mut body = Vec::new();
    ciborium::into_writer(&request, &mut body).map_err(|err| MatchStatsError::Encode(err.to_string()))?;

    let mut builder = reqwest::Client::builder().user_agent(concat!("wows-toolkit/", env!("CARGO_PKG_VERSION")));
    if !proxy_url.is_empty() {
        match reqwest::Proxy::all(proxy_url) {
            Ok(proxy) => builder = builder.proxy(proxy),
            Err(err) => tracing::warn!("live match stats: ignoring a malformed proxy URL: {err}"),
        }
    }
    let client = builder.build().map_err(StatsError::Client)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wt-gpui-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is creatable");
        dir
    }

    /// The shape of a `tempArenaInfo.json`: the same metadata a finished
    /// replay carries, with every key `ReplayMeta` has no default for.
    const ARENA_INFO: &str = r#"{
        "gameMode": 7,
        "clientVersionFromExe": "13, 11, 0, 12668706",
        "mapDisplayName": "ocean",
        "mapId": 1,
        "clientVersionFromXml": "13, 11, 0, 12668706",
        "duration": 1200,
        "gameLogic": null,
        "name": "12x12",
        "scenario": "Domination",
        "playerID": 0,
        "vehicles": [{"shipId": 100, "relation": 0, "id": 1, "name": "Me"},
                     {"shipId": 200, "relation": 2, "id": 2, "name": "Foe"}],
        "playersPerTeam": 12,
        "dateTime": "28.12.2023 00:52:26",
        "mapName": "spaces/00_CO_ocean",
        "playerName": "Me",
        "scenarioConfigId": 1,
        "teamsCount": 2,
        "logic": null,
        "playerVehicle": "PFSD110-Kleber"
    }"#;

    #[test]
    fn an_empty_replay_directory_reports_no_battle() {
        let dir = temp_dir("idle");
        assert!(poll(&LiveSource::in_dir(&dir)).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_written_arena_info_reports_a_battle_and_reads_its_roster() {
        let dir = temp_dir("in-match");
        std::fs::write(dir.join(ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is writable");
        let source = LiveSource::in_dir(&dir);

        assert!(poll(&source).is_some(), "the file's presence is the signal");
        let live = read(&source).expect("the roster reads");
        assert_eq!(live.players.len(), 2);
        assert_eq!(live.build, Some(12668706));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_half_written_arena_info_is_reported_rather_than_read_as_an_empty_roster() {
        let dir = temp_dir("half-written");
        std::fs::write(dir.join(ARENA_INFO_FILE), br#"{"gameMode": 7,"#).expect("the arena info is writable");

        let error = read(&LiveSource::in_dir(&dir)).expect_err("a truncated header is not a roster");
        assert!(matches!(error, LiveMatchError::Parse(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_stream_is_the_sibling_the_game_writes() {
        let source = LiveSource::in_dir(Path::new("/replays"));
        assert!(source.arena_info.ends_with(ARENA_INFO_FILE));
        assert!(source.stream.ends_with(LIVE_STREAM_FILE));
    }
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use crate::replay_inspector::GameDataCache;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;

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
