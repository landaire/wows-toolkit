//! Where the battle in progress is read from.
//!
//! The game writes `tempArenaInfo.json` into its replays directory when a
//! battle starts and removes it when the battle ends, so the file's presence
//! is the signal. Its contents are the same metadata a finished replay
//! carries, which is what [`LiveMatch::from_meta`] reads.
//!
//! **Polled, not watched.** A filesystem watcher reports changes only, so a
//! battle already under way when the app starts raises no event. One poll
//! covers both, and at [`POLL_INTERVAL`] costs a `stat` per tick. The front
//! end owns the timer; [`LiveWatcher`] decides what each tick reports.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use wows_replays::ReplayFile;

use super::live::LiveMatch;

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

/// How many times an unreadable arena info is re-read before it is taken at
/// face value as "no battle this app can read".
///
/// The game flushes the file in one go, so a couple of ticks covers the race;
/// past that the file is malformed rather than incomplete, and re-reading it
/// every two seconds for a whole battle only fills the log.
const READ_ATTEMPTS: usize = 3;

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
    #[error("the arena info is not a replay header")]
    Parse(#[source] wows_replays::ParseError),
    #[error("the arena info carries no date the game's format reads")]
    NoTimestamp,
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

/// What a poll tick decided, once its read (if any) is in.
#[derive(Debug)]
pub enum Settled {
    /// The arena info did not parse yet; the next tick reads it again.
    Retry,
    /// The battle in progress changed: `None` is no battle.
    Report(Option<LiveMatch>),
}

/// Decides, tick by tick, when the battle in progress has changed.
///
/// The caller polls and reads (on whatever thread suits it) and hands both
/// here; nothing is reported until a read settles, and a file that never
/// parses is reported once, as absent, rather than re-read for the whole
/// battle.
#[derive(Debug, Default)]
pub struct LiveWatcher {
    current: Option<ArenaInfoStamp>,
    /// Nothing has been reported yet, so the first tick reports even when it
    /// finds no battle: a front end shows "checking" until it does.
    reported: bool,
    attempts: usize,
}

impl LiveWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this tick's `stamp` needs the arena info read and settled.
    pub fn wants_read(&self, stamp: Option<&ArenaInfoStamp>) -> bool {
        stamp != self.current.as_ref() || !self.reported
    }

    /// Takes the read made for `stamp` (`None` when there was no battle to
    /// read).
    pub fn settle(
        &mut self,
        stamp: Option<ArenaInfoStamp>,
        read: Option<Result<LiveMatch, LiveMatchError>>,
    ) -> Settled {
        let live = match read {
            None => None,
            Some(Ok(live)) => Some(live),
            Some(Err(err)) => {
                self.attempts += 1;
                if self.attempts < READ_ATTEMPTS {
                    tracing::debug!("live match: the arena info is not readable yet: {err}");
                    return Settled::Retry;
                }
                tracing::warn!("live match: the arena info never parsed, giving up on it: {err}");
                None
            }
        };

        self.current = stamp;
        self.reported = true;
        self.attempts = 0;

        Settled::Report(live)
    }

    /// One whole tick against `source`, run where blocking file reads are
    /// acceptable. `None` when nothing changed.
    pub fn tick(&mut self, source: &LiveSource) -> Option<Option<LiveMatch>> {
        let stamp = poll(source);
        if !self.wants_read(stamp.as_ref()) {
            return None;
        }

        let read = stamp.as_ref().map(|_| read(source));
        match self.settle(stamp, read) {
            Settled::Retry => None,
            Settled::Report(live) => Some(live),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wt-viewmodel-live-{name}-{}", std::process::id()));
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

    #[test]
    fn the_watcher_reports_no_battle_once_then_the_battle_then_its_end() {
        let dir = temp_dir("watcher");
        let source = LiveSource::in_dir(&dir);
        let mut watcher = LiveWatcher::new();

        assert!(matches!(watcher.tick(&source), Some(None)), "the first tick reports, even with no battle");
        assert!(watcher.tick(&source).is_none(), "an unchanged directory reports nothing");

        std::fs::write(dir.join(ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is writable");
        let started = watcher.tick(&source).expect("a new battle is a change").expect("and it is a battle");
        assert_eq!(started.players.len(), 2);
        assert!(watcher.tick(&source).is_none(), "the same battle is not reported twice");

        std::fs::remove_file(dir.join(ARENA_INFO_FILE)).expect("the arena info is removable");
        assert!(matches!(watcher.tick(&source), Some(None)), "the battle ending is a change");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_arena_info_that_never_parses_is_reported_once_as_no_battle() {
        let dir = temp_dir("watcher-malformed");
        let source = LiveSource::in_dir(&dir);
        let mut watcher = LiveWatcher::new();
        assert!(matches!(watcher.tick(&source), Some(None)));

        std::fs::write(dir.join(ARENA_INFO_FILE), br#"{"gameMode": 7,"#).expect("the arena info is writable");
        for _ in 1..READ_ATTEMPTS {
            assert!(watcher.tick(&source).is_none(), "a read the game may still be flushing is retried");
        }
        assert!(matches!(watcher.tick(&source), Some(None)), "then given up on, as absent");
        assert!(watcher.tick(&source).is_none(), "and not re-read every tick for the whole battle");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
