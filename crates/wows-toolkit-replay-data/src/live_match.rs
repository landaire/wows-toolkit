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
//! [`POLL_INTERVAL`] costs a `stat` per tick. Every function here blocks.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use wows_replays::ReplayFile;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::analyzer::arena_scan::scan_arena_state;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::load::GameDataCache;
use crate::load::LoadedGameData;
use crate::load::decode_constants;

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

/// How often a battle in progress is re-read while waiting for its roster.
///
/// The game writes the packet stream in flushes, so the scan's target packet
/// may not be in the file yet on the first look.
pub const SCAN_RETRY_INTERVAL: Duration = Duration::from_secs(3);

/// How long to keep retrying before giving up on a battle's roster.
pub const SCAN_RETRY_BUDGET: Duration = Duration::from_secs(90);

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

#[derive(Debug, thiserror::Error)]
pub enum LiveMatchError {
    #[error("the arena info could not be read")]
    Read(#[source] std::io::Error),
    #[error("the arena info is not a replay header")]
    Parse(#[source] wows_replays::ParseError),
    #[error("the arena info carries no date the game's format reads")]
    NoTimestamp,
}

/// What one [`LiveWatch::step`] found.
#[derive(Debug)]
pub enum Observed {
    /// Nothing to report since the last step.
    Unchanged,
    /// The battle in progress changed. `None` is no battle, or one whose arena
    /// info never parsed.
    Changed(Option<LiveMatch>),
}

/// Polls one replays directory, reporting each change of battle once.
///
/// The caller drives it: one [`step`](Self::step) per [`POLL_INTERVAL`], on a
/// thread that may block.
#[derive(Debug)]
pub struct LiveWatch {
    source: LiveSource,
    current: Option<ArenaInfoStamp>,
    /// Nothing has been reported yet, so the first step reports even when it
    /// finds no battle: a view shows "checking" until it does.
    reported: bool,
    /// Reading a file the game is still flushing fails; the next step retries
    /// it. A file that never parses is reported once, as absent, rather than
    /// re-read every tick for the whole battle.
    attempts: usize,
}

impl LiveWatch {
    pub fn new(replay_dir: &Path) -> Self {
        Self { source: LiveSource::in_dir(replay_dir), current: None, reported: false, attempts: 0 }
    }

    /// Polls once, reading the arena info when it changed.
    pub fn step(&mut self) -> Observed {
        // How many times an unreadable arena info is re-read before it is taken
        // at face value as "no battle this app can read". The game flushes the
        // file in one go, so a couple of ticks covers the race; past that the
        // file is malformed rather than incomplete.
        const READ_ATTEMPTS: usize = 3;

        let stamp = poll(&self.source);
        if stamp == self.current && self.reported {
            return Observed::Unchanged;
        }

        let read = stamp.as_ref().map(|_| read(&self.source));
        if let Some(Err(err)) = &read {
            self.attempts += 1;
            if self.attempts < READ_ATTEMPTS {
                tracing::debug!("live match: the arena info is not readable yet: {err}");
                return Observed::Unchanged;
            }
            tracing::warn!("live match: the arena info never parsed, giving up on it: {err}");
        }

        self.current = stamp;
        self.reported = true;
        self.attempts = 0;

        Observed::Changed(read.and_then(Result::ok))
    }
}

/// The stamp of the battle in progress, or `None` when there is none.
pub fn poll(source: &LiveSource) -> Option<ArenaInfoStamp> {
    let metadata = std::fs::metadata(&source.arena_info).ok()?;
    if !metadata.is_file() {
        return None;
    }

    Some(ArenaInfoStamp { modified: metadata.modified().ok()?, len: metadata.len() })
}

/// Reads the battle in progress.
///
/// The game flushes this file at match start and may not have finished, so a
/// parse failure is an ordinary "not yet" for the next poll to retry rather
/// than a permanent error.
pub fn read(source: &LiveSource) -> Result<LiveMatch, LiveMatchError> {
    let bytes = std::fs::read(&source.arena_info).map_err(LiveMatchError::Read)?;
    let replay = ReplayFile::from_decrypted_parts(bytes, Vec::new()).map_err(LiveMatchError::Parse)?;

    LiveMatch::from_meta(&replay.meta).ok_or(LiveMatchError::NoTimestamp)
}

/// Reads the arena roster off the live packet stream.
///
/// `None` means "not yet": the stream is short, still being written, or the
/// packet carrying the roster has not been flushed. Retried by the caller.
pub fn scan_arena(source: &LiveSource, loaded: &LoadedGameData, game_data: &GameDataCache) -> Option<ArenaState> {
    let meta = std::fs::read(&source.arena_info).ok()?;
    let packets = std::fs::read(&source.stream).ok()?;
    // Both halves are re-read per attempt, so an attempt that raced the game
    // mid-write is corrected by the next one.
    let replay = ReplayFile::from_decrypted_parts(meta, packets).ok()?;

    let version = Version::from_client_exe(&replay.meta.clientVersionFromExe);
    // Constants are versioned by build; without one the parser's built-in layout applies.
    let constants = match version.build_number() {
        Some(build) => decode_constants(loaded, build, version, game_data),
        None => loaded.base_constants().clone(),
    };

    scan_arena_state(loaded.provider().entity_specs(), version, &replay, &constants)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wt-replay-data-live-{name}-{}", std::process::id()));
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

    /// The first step reports even an idle directory, a battle is reported
    /// once, and a file that never parses is reported as no battle after the
    /// retries rather than on every step.
    #[test]
    fn a_watch_reports_each_change_once() {
        let dir = temp_dir("watch");
        let mut watch = LiveWatch::new(&dir);

        assert!(matches!(watch.step(), Observed::Changed(None)), "the first step reports");
        assert!(matches!(watch.step(), Observed::Unchanged));

        std::fs::write(dir.join(ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is writable");
        assert!(matches!(watch.step(), Observed::Changed(Some(_))));
        assert!(matches!(watch.step(), Observed::Unchanged));

        std::fs::write(dir.join(ARENA_INFO_FILE), br#"{"gameMode": 7,"#).expect("the arena info is writable");
        assert!(matches!(watch.step(), Observed::Unchanged), "a first failed read is retried");
        assert!(matches!(watch.step(), Observed::Unchanged), "a second failed read is retried");
        assert!(matches!(watch.step(), Observed::Changed(None)), "the third is taken as no battle");
        assert!(matches!(watch.step(), Observed::Unchanged));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod scan_tests {
    use wows_toolkit_viewmodel::match_stats::build_request;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;

    use super::*;

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

        let dir = std::env::temp_dir().join(format!("wt-replay-data-live-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is creatable");
        std::fs::write(dir.join(ARENA_INFO_FILE), replay.raw_meta.as_bytes()).expect("the arena info is writable");
        std::fs::write(dir.join(LIVE_STREAM_FILE), replay.packet_data()).expect("the stream is writable");

        let game_data = GameDataCache::new(PathBuf::from(&wows_dir));
        let loaded = game_data.get_or_load_build(build).expect("the build's game data loads");

        let state = scan_arena(&LiveSource::in_dir(&dir), &loaded, &game_data).expect("the roster scans");
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
