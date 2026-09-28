//! What a finished battle contributes to ShipBuilds, and when.
//!
//! Shared, so both front ends decide the same thing about the same replay: the
//! data-sharing setting is one row, the sent-replay ledger is one table, and a
//! replay the reader agreed to share must reach the service once, whichever app
//! read it.
//!
//! The rules here are the egui app's (`task/replay_upload.rs`), moved so the port
//! is not a second opinion on them. What stays in each app is the HTTP: one
//! blocking client, one async.

use jiff::SignedDuration;
use jiff::Timestamp;

use crate::settings::DataSharingMode;

pub mod build_tracker;

/// How long a raw upload waits for the battle's results before going anyway.
///
/// Measured from when the replay was first seen, not from its modification time:
/// an archived file carries a time from months ago and would be past due the
/// moment it is first read.
pub const RAW_UPLOAD_GRACE: SignedDuration = SignedDuration::from_secs(30 * 60);

/// When a raw upload that is still waiting for results gives up waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawUploadDeadline(pub Timestamp);

/// Whether a replay's packet stream carries an end-of-battle marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultsScan {
    /// A marker was observed.
    Present,
    /// The whole stream was read and no marker was in it.
    Absent,
    /// The read stopped early, so the tail where the markers live was never
    /// seen. Presence is unknown, which is not the same as absent.
    Truncated,
}

impl ResultsScan {
    /// True only where a marker was positively seen. `Truncated` is not evidence
    /// of absence and must never be read as it.
    pub fn is_present(&self) -> bool {
        matches!(self, Self::Present)
    }
}

/// Whether the file on disk is the whole battle yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawReplaySnapshotState {
    Complete,
    IncompleteWithinGrace { deadline: RawUploadDeadline },
    IncompleteGraceLapsed,
}

pub fn raw_replay_snapshot_state(
    results: ResultsScan,
    first_seen: Timestamp,
    now: Timestamp,
) -> RawReplaySnapshotState {
    if results.is_present() {
        return RawReplaySnapshotState::Complete;
    }
    // Saturate: an anchor close enough to the range edge to overflow is
    // garbage, and holding the upload is the conservative reading.
    let deadline = first_seen.checked_add(RAW_UPLOAD_GRACE).unwrap_or(Timestamp::MAX);
    if deadline <= now {
        RawReplaySnapshotState::IncompleteGraceLapsed
    } else {
        RawReplaySnapshotState::IncompleteWithinGrace { deadline: RawUploadDeadline(deadline) }
    }
}

/// What a freshly read replay should contribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayUploadAction {
    Skip(ReplayUploadSkipReason),
    /// The per-player build payloads, to `/api/ship_builds`.
    BuildData,
    /// The replay file itself, to `/api/replays`.
    RawReplay,
    /// Nothing yet: ask again when the results arrive or the deadline fires.
    AwaitResults {
        deadline: RawUploadDeadline,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayUploadSkipReason {
    SharingDisabled,
    IneligibleGameType,
    /// Replays mode shares the whole file, and a battle that may be in a test
    /// ship must stay off `/api/replays`.
    PossibleTestShip,
}

/// Decides what to send.
///
/// `self_confirmed_non_test` is `true` only where the recording player's ship is
/// positively known not to be a test ship; any uncertainty is `false`, which
/// keeps a possible test-ship replay off `/api/replays`. Replays mode never falls
/// back to build data: the two payloads are exclusive.
pub fn decide_upload_action(
    mode: DataSharingMode,
    is_valid_game_type: bool,
    self_confirmed_non_test: bool,
    raw_snapshot: RawReplaySnapshotState,
) -> ReplayUploadAction {
    if !is_valid_game_type {
        return ReplayUploadAction::Skip(ReplayUploadSkipReason::IneligibleGameType);
    }

    match mode {
        DataSharingMode::Off => ReplayUploadAction::Skip(ReplayUploadSkipReason::SharingDisabled),
        DataSharingMode::BuildData => ReplayUploadAction::BuildData,
        DataSharingMode::Replays => {
            if !self_confirmed_non_test {
                return ReplayUploadAction::Skip(ReplayUploadSkipReason::PossibleTestShip);
            }
            match raw_snapshot {
                RawReplaySnapshotState::Complete | RawReplaySnapshotState::IncompleteGraceLapsed => {
                    ReplayUploadAction::RawReplay
                }
                RawReplaySnapshotState::IncompleteWithinGrace { deadline } => {
                    ReplayUploadAction::AwaitResults { deadline }
                }
            }
        }
    }
}

/// Where each payload goes.
pub const SHIP_BUILDS_URL: &str = "https://shipbuilds.com/api/ship_builds";
pub const RAW_REPLAYS_URL: &str = "https://shipbuilds.com/api/replays";

/// Whether the battle type is one the service takes.
///
/// Random and ranked only, as the egui app gates it: a training room or an
/// operation says nothing about a build people play.
pub fn is_eligible_game_type(game_type: &str, version: wowsunpack::data::Version) -> bool {
    let battle_type = wowsunpack::game_types::BattleType::from_value(game_type, version);
    matches!(
        battle_type.known(),
        Some(wowsunpack::game_types::BattleType::Random | wowsunpack::game_types::BattleType::Ranked)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::from_second(1_800_000_000).expect("a timestamp in range")
    }

    /// Sharing off sends nothing, whatever the battle was.
    #[test]
    fn nothing_is_sent_while_sharing_is_off() {
        let action = decide_upload_action(DataSharingMode::Off, true, true, RawReplaySnapshotState::Complete);
        assert_eq!(action, ReplayUploadAction::Skip(ReplayUploadSkipReason::SharingDisabled));
    }

    /// A battle type the service does not take is refused before the mode is
    /// even read.
    #[test]
    fn an_ineligible_battle_is_refused_in_every_mode() {
        for mode in [DataSharingMode::Off, DataSharingMode::BuildData, DataSharingMode::Replays] {
            let action = decide_upload_action(mode, false, true, RawReplaySnapshotState::Complete);
            assert_eq!(action, ReplayUploadAction::Skip(ReplayUploadSkipReason::IneligibleGameType));
        }
    }

    /// A ship that cannot be proven not to be a test ship keeps the whole file
    /// off the service. Build data is not a fallback for it.
    #[test]
    fn a_possible_test_ship_is_not_shared_as_a_replay() {
        let action = decide_upload_action(DataSharingMode::Replays, true, false, RawReplaySnapshotState::Complete);
        assert_eq!(action, ReplayUploadAction::Skip(ReplayUploadSkipReason::PossibleTestShip));
    }

    /// A replay with no results yet waits, and stops waiting once the grace has
    /// lapsed.
    #[test]
    fn a_replay_without_results_waits_until_the_grace_lapses() {
        let waiting = raw_replay_snapshot_state(ResultsScan::Absent, now(), now());
        let deadline = match waiting {
            RawReplaySnapshotState::IncompleteWithinGrace { deadline } => deadline,
            other => panic!("a fresh replay waits, got {other:?}"),
        };
        assert_eq!(
            decide_upload_action(DataSharingMode::Replays, true, true, waiting),
            ReplayUploadAction::AwaitResults { deadline }
        );

        let lapsed = raw_replay_snapshot_state(ResultsScan::Absent, now(), deadline.0);
        assert_eq!(lapsed, RawReplaySnapshotState::IncompleteGraceLapsed);
        assert_eq!(decide_upload_action(DataSharingMode::Replays, true, true, lapsed), ReplayUploadAction::RawReplay);
    }

    /// A truncated read is not an absent marker: it waits, rather than being
    /// sent as though the battle had no end.
    #[test]
    fn a_truncated_read_is_not_evidence_of_a_missing_end() {
        assert!(!ResultsScan::Truncated.is_present());
        let state = raw_replay_snapshot_state(ResultsScan::Truncated, now(), now());
        assert!(matches!(state, RawReplaySnapshotState::IncompleteWithinGrace { .. }));
    }
}
