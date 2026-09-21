//! Reads the roster off the replay a live battle is writing, then fetches
//! that roster's stats. Runs on the background replay-parser thread.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use jiff::Timestamp;
use parking_lot::RwLock;
use rootcause::prelude::*;
use rust_i18n::t;
use tracing::debug;
use wows_replays::ReplayFile;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::analyzer::arena_scan::scan_arena_state;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::data::match_stats::MatchStatsClient;
use crate::data::match_stats::MatchStatsError;
use crate::data::wows_data::BuildDataCache;
use crate::ui::player_tracker::MatchStatsState;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::player_tracker::live::LiveIdentities;
pub(crate) use wows_toolkit_viewmodel::match_stats::build_request;

/// Where a live-roster scan reads its match from.
///
/// Which file holds the packets and where the metadata comes from are the same
/// distinction, so one enum carries both.
#[derive(Debug, Clone)]
pub enum LiveRosterSource {
    /// A battle in progress. The game writes the packets to `temp.wowsreplay`
    /// as a bare stream and the metadata to its sibling `tempArenaInfo.json`,
    /// wrapping the two into a replay file only once the battle ends. Read
    /// tolerantly and retry: the stream arrives in flushes and
    /// `onArenaStateReceived` may not be in one yet.
    InProgress { arena_info: PathBuf, stream: PathBuf },
    /// A finished replay, carrying both halves itself. Read strictly and try
    /// once.
    Complete { replay: PathBuf },
}

/// How often a battle in progress is re-read while waiting for the roster.
const RETRY_INTERVAL: Duration = Duration::from_secs(3);
/// How long to keep waiting before giving up on a battle's roster.
const RETRY_BUDGET: Duration = Duration::from_secs(90);

/// Read the live roster off `replay`, then fetch its stats.
///
/// Every failure lands on the tracker as a `MatchStatsState`; none of them
/// toast, because a match starting is not a moment to interrupt.
///
/// `started_at` is the match this scan was queued for. Every write to the
/// tracker goes through a setter keyed on it, so a scan that outlives its
/// match (the player left to port and queued into a new one while this scan
/// was retrying or waiting on the HTTP call) is refused rather than landing
/// its stale identities and stats on the new match's roster.
pub(crate) fn resolve_and_fetch(
    source: &LiveRosterSource,
    build: Option<u32>,
    started_at: Timestamp,
    build_cache: &BuildDataCache,
    tracker: &Arc<RwLock<PlayerTracker>>,
    client: &mut MatchStatsClient,
) {
    if !tracker.write().set_match_stats_for(started_at, MatchStatsState::Resolving) {
        debug!("live roster scan: abandoned before it started, the match already changed");
        return;
    }

    // No build means no entity specs, and no later attempt can produce one.
    let Some(build) = build else {
        tracker.write().set_match_stats_for(
            started_at,
            MatchStatsState::Failed(t!("ui.player_tracker.roster_unavailable").into()),
        );
        return;
    };

    let deadline = Instant::now() + RETRY_BUDGET;
    let state = loop {
        if let Some(state) = try_scan(source, build, build_cache) {
            break Some(state);
        }
        let retryable = matches!(source, LiveRosterSource::InProgress { .. });
        if !retryable || Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(RETRY_INTERVAL);
    };

    let Some(state) = state else {
        tracker.write().set_match_stats_for(
            started_at,
            MatchStatsState::Failed(t!("ui.player_tracker.roster_unavailable").into()),
        );
        return;
    };

    // Written before the fetch so the links and the identity join light up
    // even when the fetch then fails.
    if !tracker.write().set_live_identities_for(started_at, LiveIdentities::from_player_states(&state.players)) {
        debug!("live roster scan: identities dropped, the match already changed");
        return;
    }
    if !tracker.write().set_match_stats_for(started_at, MatchStatsState::Fetching) {
        debug!("live roster scan: abandoned before the fetch, the match already changed");
        return;
    }

    let outcome = build_request(state.arena_id, &state.players).and_then(|request| client.fetch(&request));
    let next = match outcome {
        Ok(response) => {
            let by_account = response.players.into_iter().map(|player| (player.account_id, player)).collect();
            MatchStatsState::Ready(by_account)
        }
        Err(error) => MatchStatsState::Failed(failure_reason(error)),
    };
    if !tracker.write().set_match_stats_for(started_at, next) {
        debug!("live roster scan: result dropped, the match already changed");
    }
}

/// The text a `MatchStatsError` shows on the Current Match status line.
/// `RateLimited` and `RecentlyFailed` both mean "the service refused this
/// without answering the roster", so both get the translated rate-limit
/// message rather than their raw English `Display` text.
fn failure_reason(error: MatchStatsError) -> String {
    match error {
        MatchStatsError::RateLimited { retry_after } | MatchStatsError::RecentlyFailed { retry_after } => {
            t!("ui.player_tracker.stats_rate_limited", wait = format_wait_minutes(retry_after)).into()
        }
        other => other.to_string(),
    }
}

/// `retry_after` rounded up to whole minutes, never zero: a sub-minute wait
/// is still worth telling the user to wait a minute for.
fn format_wait_minutes(retry_after: Duration) -> String {
    let minutes = retry_after.as_secs().div_ceil(60).max(1);
    if minutes == 1 { "1 minute".to_string() } else { format!("{minutes} minutes") }
}

/// One attempt at reading the roster. `None` means "not yet": a file is
/// absent, the flushed prefix is too short, game data has not loaded, or
/// `onArenaStateReceived` has not been written.
fn try_scan(source: &LiveRosterSource, build: u32, build_cache: &BuildDataCache) -> Option<ArenaState> {
    let replay_file = match read_replay(source) {
        Ok(file) => file,
        Err(e) => {
            debug!("live roster scan: replay not readable yet: {e:?}");
            return None;
        }
    };

    let shared = build_cache.get(build)?;
    let guard = shared.read();
    let metadata = guard.game_metadata.as_ref()?;
    let version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);

    scan_arena_state(metadata.entity_specs(), version, &replay_file)
}

/// Read the match as one replay, however its two halves are stored.
///
/// A battle in progress has them in separate files, which is what
/// `from_decrypted_parts` is for: `temp.wowsreplay` is the packet stream
/// already, needing neither decryption nor inflation, and its metadata is the
/// `tempArenaInfo.json` beside it. Both are re-read on every attempt, so an
/// attempt that raced the game mid-write is corrected by the next one.
fn read_replay(source: &LiveRosterSource) -> rootcause::Result<ReplayFile> {
    match source {
        LiveRosterSource::InProgress { arena_info, stream } => {
            let meta = read_file(arena_info, "failed to read tempArenaInfo.json")?;
            let packets = read_file(stream, "failed to read the live replay")?;
            ReplayFile::from_decrypted_parts(meta, packets)
                .context("failed to assemble the live replay from its parts")
                .map_err(|e| e.into_dynamic())
        }
        LiveRosterSource::Complete { replay } => ReplayFile::from_file(replay).map_err(|e| e.into_dynamic()),
    }
}

fn read_file(path: &Path, what: &'static str) -> rootcause::Result<Vec<u8>> {
    std::fs::read(path).context(what).attach_with(|| format!("path: {}", path.display())).map_err(|e| e.into_dynamic())
}

#[cfg(test)]
mod tests {

    use super::*;

    /// A wait under a minute still reads as "1 minute": rounding down to zero
    /// would tell the user the retry is available immediately when it is not.
    #[test]
    fn format_wait_minutes_rounds_up_and_never_reports_zero() {
        assert_eq!(format_wait_minutes(Duration::from_secs(1)), "1 minute");
        assert_eq!(format_wait_minutes(Duration::from_secs(60)), "1 minute");
        assert_eq!(format_wait_minutes(Duration::from_secs(61)), "2 minutes");
        assert_eq!(format_wait_minutes(Duration::from_secs(0)), "1 minute");
    }

    /// `RateLimited` and `RecentlyFailed` both reach the UI as the translated
    /// rate-limit message rather than their raw English `Display` text.
    #[test]
    fn rate_limited_and_recently_failed_get_the_translated_reason() {
        let rate_limited = failure_reason(MatchStatsError::RateLimited { retry_after: Duration::from_secs(90) });
        assert!(rate_limited.contains("2 minutes"), "got: {rate_limited}");

        let recently_failed = failure_reason(MatchStatsError::RecentlyFailed { retry_after: Duration::from_secs(30) });
        assert!(recently_failed.contains("1 minute"), "got: {recently_failed}");
    }

    /// Every other variant still falls through to its own `Display` text.
    #[test]
    fn other_errors_keep_their_own_text() {
        let reason = failure_reason(MatchStatsError::Http { status: 500 });
        assert_eq!(reason, MatchStatsError::Http { status: 500 }.to_string());
    }
}
