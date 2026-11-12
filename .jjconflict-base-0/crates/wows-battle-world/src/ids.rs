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
    /// Accumulate a whole-match hit history in `HitHistoryLog`. Off by default:
    /// renderers only need the current frame's hits and should not pay the memory.
    pub record_hit_history: bool,
    /// Accumulate every artillery salvo fired in `SalvoLog`. Off by default:
    /// only a shots-fired statistic needs it, and a renderer draws shells from
    /// the active-shot list instead.
    pub record_salvo_history: bool,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            shot_tracking: ShotTracking::Tracked,
            source_team: SourceTeam(None),
            record_hit_history: false,
            record_salvo_history: false,
        }
    }
}
