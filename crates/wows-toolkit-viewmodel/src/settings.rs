//! Settings both front ends read and write.
//!
//! The keys live here rather than as string literals at each call site, so a
//! setting the egui app writes is the one the GPUI port reads, and renaming
//! one is a single edit that the compiler checks.

use serde::Deserialize;
use serde::Serialize;

/// Keys in the shared config database's settings table.
pub mod keys {
    pub const WOWS_DIR: &str = "wows_dir";
    pub const CURRENT_REPLAY_PATH: &str = "current_replay_path";
    pub const ZOOM_FACTOR: &str = "zoom_factor";
    pub const CHECK_FOR_UPDATES: &str = "check_for_updates";
    pub const ENABLE_LOGGING: &str = "enable_logging";
    pub const DEBUG_MODE: &str = "debug_mode";
    pub const DATA_SHARING_MODE: &str = "data_sharing_mode";
    pub const PROXY_URL: &str = "proxy_url";
    pub const LOCALE: &str = "locale";
    pub const REPLAY_SETTINGS: &str = "replay_settings";
    pub const AUTO_LOAD_LATEST_REPLAY: &str = "auto_load_latest_replay";
    /// Where the unpacker writes extracted files. Empty until the user picks
    /// one, which is why the Extract control stays disabled.
    pub const OUTPUT_DIR: &str = "output_dir";
}

/// What the app is allowed to send upstream.
///
/// Stored snake_case, which is what is already in the database; reading it as
/// the variant names would silently fall back to `Off` and misreport what the
/// user agreed to share. `Replays` never implies `BuildData`: they are
/// different payloads to different endpoints, not escalating levels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSharingMode {
    /// Share nothing.
    #[default]
    Off,
    /// Send per-player build payloads.
    BuildData,
    /// Send the raw replay file once results are in it.
    Replays,
}

impl DataSharingMode {
    pub const ALL: [DataSharingMode; 3] = [Self::Off, Self::BuildData, Self::Replays];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::BuildData => "Ship builds",
            Self::Replays => "Replays",
        }
    }

    /// One line on what this mode sends, for the control's description.
    pub fn description(self) -> &'static str {
        match self {
            Self::Off => "Nothing is sent.",
            Self::BuildData => "Per-player ship builds are sent after a battle.",
            Self::Replays => "The replay file is sent once results are in it.",
        }
    }

    pub fn shares_anything(self) -> bool {
        !matches!(self, Self::Off)
    }
}

#[cfg(test)]
mod tests {
    use super::DataSharingMode;

    /// Pinned against real stored values: the database holds snake_case, and
    /// reading it as the variant names would quietly report sharing as off.
    #[test]
    fn the_stored_shape_is_snake_case_as_written_to_the_database() {
        assert_eq!(serde_json::to_string(&DataSharingMode::Off).unwrap(), "\"off\"");
        assert_eq!(serde_json::to_string(&DataSharingMode::BuildData).unwrap(), "\"build_data\"");
        assert_eq!(serde_json::to_string(&DataSharingMode::Replays).unwrap(), "\"replays\"");
        assert_eq!(serde_json::from_str::<DataSharingMode>("\"replays\"").unwrap(), DataSharingMode::Replays);
        assert!(
            serde_json::from_str::<DataSharingMode>("\"Replays\"").is_err(),
            "the variant spelling is not what is stored"
        );
    }

    #[test]
    fn only_off_shares_nothing() {
        assert!(!DataSharingMode::Off.shares_anything());
        assert!(DataSharingMode::BuildData.shares_anything());
        assert!(DataSharingMode::Replays.shares_anything());
    }

    #[test]
    fn every_mode_is_offered_and_described() {
        assert_eq!(DataSharingMode::ALL.len(), 3);
        for mode in DataSharingMode::ALL {
            assert!(!mode.label().is_empty());
            assert!(!mode.description().is_empty());
        }
    }
}
