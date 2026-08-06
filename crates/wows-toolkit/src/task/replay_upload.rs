use std::path::Path;

use tracing::debug;
use tracing::error;
use wows_battle_world::report::BattleReport;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::data::settings::DataSharingMode;
use crate::data::shipbuilds::ShipBuildsClient;
use crate::ui::replay_parser::Replay;
use crate::util::build_tracker;

/// Whether a ShipBuilds batch upload consults the sent-replay ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendReplayCachePolicy {
    UseLedger,
    IgnoreLedger,
}

impl SendReplayCachePolicy {
    pub fn should_attempt(&self, ledger_contains: bool) -> bool {
        matches!(self, Self::IgnoreLedger) || !ledger_contains
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReplayCount(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendAllReplaysProgress {
    pub completed: ReplayCount,
    pub total: ReplayCount,
}

impl SendAllReplaysProgress {
    pub fn new(completed: ReplayCount, total: ReplayCount) -> Self {
        Self { completed, total }
    }

    pub fn fraction(&self) -> f32 {
        if self.total.0 == 0 { 0.0 } else { (self.completed.0 as f32 / self.total.0 as f32).clamp(0.0, 1.0) }
    }
}

/// What the background parser should upload for a freshly parsed replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayUploadAction {
    /// Upload nothing.
    Skip,
    /// Send the per-player build payloads to `/api/ship_builds`.
    BuildData,
    /// Send the raw replay file to `/api/replays`.
    RawReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShipBuildsUploadOutcome {
    Skipped,
    Sent,
    TransientFailure,
}

/// Decide what to upload. `self_confirmed_non_test` must be `true` only when the
/// self player's ship is positively known not to be a test ship; any
/// uncertainty is `false`, which keeps a possible test-ship replay off
/// `/api/replays` (hard rule) by falling back to build data.
pub fn decide_upload_action(
    mode: DataSharingMode,
    is_valid_game_type: bool,
    self_confirmed_non_test: bool,
) -> ReplayUploadAction {
    match mode {
        DataSharingMode::Off => ReplayUploadAction::Skip,
        DataSharingMode::BuildData if is_valid_game_type => ReplayUploadAction::BuildData,
        DataSharingMode::Replays if is_valid_game_type => {
            if self_confirmed_non_test {
                ReplayUploadAction::RawReplay
            } else {
                ReplayUploadAction::BuildData
            }
        }
        _ => ReplayUploadAction::Skip,
    }
}

pub(crate) fn upload_parsed_replay(
    path: &Path,
    replay: &Replay,
    report: &BattleReport,
    metadata: &GameMetadataProvider,
    mode: DataSharingMode,
    client: &ShipBuildsClient,
) -> ShipBuildsUploadOutcome {
    // A missing game type remains ineligible because the empty value is unknown.
    let game_type = replay.replay_file.meta.gameType.clone().unwrap_or_default();
    let replay_version = wowsunpack::data::Version::from_client_exe(&replay.replay_file.meta.clientVersionFromExe);
    let battle_type = wowsunpack::game_types::BattleType::from_value(&game_type, replay_version);
    let is_valid_game_type = matches!(
        battle_type.known(),
        Some(wowsunpack::game_types::BattleType::Random | wowsunpack::game_types::BattleType::Ranked)
    );
    if !is_valid_game_type {
        debug!("game type is: {}", &game_type);
    }

    let self_confirmed_non_test = report
        .players()
        .iter()
        .find(|player| player.relation().is_self())
        .and_then(|player| player.vehicle().vehicle())
        .map(|vehicle| !vehicle.is_test_ship())
        .unwrap_or(false);

    match decide_upload_action(mode, is_valid_game_type, self_confirmed_non_test) {
        ReplayUploadAction::Skip => ShipBuildsUploadOutcome::Skipped,
        ReplayUploadAction::BuildData => {
            for player in report.players().iter().filter(|player| !player.is_bot()) {
                let Some(realm) = player.initial_state().realm() else {
                    continue;
                };
                #[cfg(not(feature = "shipbuilds_debugging"))]
                let url = "https://shipbuilds.com/api/ship_builds";
                #[cfg(feature = "shipbuilds_debugging")]
                let url = "http://192.168.1.215:3000/api/ship_builds";

                if let Some(payload) = build_tracker::BuildTrackerPayload::build_from(
                    player,
                    realm.to_string(),
                    report.version(),
                    game_type.to_string(),
                    metadata,
                ) {
                    if let Err(error) = client.http().post(url).json(&payload).send() {
                        error!("error sending request for replay {:?}: {:?}", path, error);
                        if error.is_connect() {
                            return ShipBuildsUploadOutcome::TransientFailure;
                        }
                    }
                } else {
                    error!("no vehicle entity for player?");
                }
            }
            debug!("Successfully sent all builds");
            ShipBuildsUploadOutcome::Sent
        }
        ReplayUploadAction::RawReplay => {
            #[cfg(not(feature = "shipbuilds_debugging"))]
            let url = "https://shipbuilds.com/api/replays";
            #[cfg(feature = "shipbuilds_debugging")]
            let url = "http://192.168.1.215:3000/api/replays";

            match std::fs::read(path) {
                Ok(bytes) => {
                    if let Err(error) = client
                        .http()
                        .post(url)
                        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                        .body(bytes)
                        .send()
                    {
                        error!("error sending replay {:?}: {:?}", path, error);
                        if error.is_connect() {
                            return ShipBuildsUploadOutcome::TransientFailure;
                        }
                    }
                }
                Err(error) => {
                    error!("failed to read replay file for upload {:?}: {:?}", path, error);
                    return ShipBuildsUploadOutcome::TransientFailure;
                }
            }
            ShipBuildsUploadOutcome::Sent
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_policy_skips_ledger_entries_but_debug_policy_does_not() {
        assert!(!SendReplayCachePolicy::UseLedger.should_attempt(true));
        assert!(SendReplayCachePolicy::UseLedger.should_attempt(false));
        assert!(SendReplayCachePolicy::IgnoreLedger.should_attempt(true));
    }

    #[test]
    fn empty_progress_has_a_safe_fraction() {
        let progress = SendAllReplaysProgress::new(ReplayCount(0), ReplayCount(0));
        assert_eq!(progress.fraction(), 0.0);
    }

    #[test]
    fn off_never_uploads() {
        for valid in [true, false] {
            for non_test in [true, false] {
                assert_eq!(decide_upload_action(DataSharingMode::Off, valid, non_test), ReplayUploadAction::Skip);
            }
        }
    }

    #[test]
    fn invalid_game_type_never_uploads() {
        assert_eq!(decide_upload_action(DataSharingMode::BuildData, false, true), ReplayUploadAction::Skip);
        assert_eq!(decide_upload_action(DataSharingMode::Replays, false, true), ReplayUploadAction::Skip);
    }

    #[test]
    fn build_data_mode_sends_build_data() {
        assert_eq!(decide_upload_action(DataSharingMode::BuildData, true, true), ReplayUploadAction::BuildData);
        assert_eq!(decide_upload_action(DataSharingMode::BuildData, true, false), ReplayUploadAction::BuildData);
    }

    #[test]
    fn replays_mode_sends_raw_only_when_confirmed_non_test() {
        assert_eq!(decide_upload_action(DataSharingMode::Replays, true, true), ReplayUploadAction::RawReplay);
    }

    #[test]
    fn replays_mode_falls_back_to_build_data_when_test_or_indeterminate() {
        assert_eq!(decide_upload_action(DataSharingMode::Replays, true, false), ReplayUploadAction::BuildData);
    }
}
