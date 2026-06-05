//! Typed option/enum types for ingest configuration.

use wows_replays::types::TeamId;

/// Controls shot and hit recording.
///
/// `Tracked`: record active_shots and shot_hits, and clear shot_hits each packet
/// so callers see only the current frame's hits (matches BattleController default).
/// `Untracked`: skip all shot/hit recording entirely (memory optimization for
/// passes that do not need shot data, e.g. cap_layout / replayshark).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotTracking {
    Tracked,
    Untracked,
}

/// Merge-mode tag identifying which perspective the current packet came from.
#[derive(Debug, Clone, Copy, Default)]
pub struct SourceTeam(pub Option<TeamId>);

/// Options controlling how a packet stream is ingested.
#[derive(Debug, Clone, Copy)]
pub struct IngestOptions {
    pub shot_tracking: ShotTracking,
    pub source_team: SourceTeam,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self { shot_tracking: ShotTracking::Tracked, source_team: SourceTeam(None) }
    }
}
