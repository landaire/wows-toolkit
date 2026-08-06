use crate::data::settings::DataSharingMode;

/// Whether a ShipBuilds batch upload consults the sent-replay ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendReplayCachePolicy {
    UseLedger,
    IgnoreLedger,
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

#[cfg(test)]
mod tests {
    use super::*;

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
