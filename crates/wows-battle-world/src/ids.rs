//! Typed option/enum types for ingest configuration.

use wows_replays::types::TeamId;

/// Whether artillery shot hits accumulate across the replay or are cleared each
/// packet (the renderer wants only the current frame's hits).
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
        Self { shot_tracking: ShotTracking::Untracked, source_team: SourceTeam(None) }
    }
}
