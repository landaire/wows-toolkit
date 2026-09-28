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
    /// The older, coarser form of the same consent: whether anything is shared at
    /// all. A build that knew only this row still reads it, so both apps write it
    /// beside the mode and reconcile the two on load
    /// (`DataSharingMode::reconcile_with_compat_bool`).
    pub const SEND_REPLAY_DATA: &str = "send_replay_data";
    pub const PROXY_URL: &str = "proxy_url";
    pub const LOCALE: &str = "locale";
    pub const REPLAY_SETTINGS: &str = "replay_settings";
    pub const AUTO_LOAD_LATEST_REPLAY: &str = "auto_load_latest_replay";
    /// Where the unpacker writes extracted files. Empty until the user picks
    /// one, which is why the Extract control stays disabled.
    pub const OUTPUT_DIR: &str = "output_dir";
    /// Which theme the app renders in; a [`super::ThemeChoice`].
    pub const THEME: &str = "theme";
    /// Whether game data is cached on load so old replays still open after a
    /// game update.
    pub const AUTO_DUMP_GAME_DATA: &str = "auto_dump_game_data";
    /// Where that cache is kept. Empty means the default location.
    pub const GAME_DATA_CACHE_DIR: &str = "game_data_cache_dir";
    /// The game-data repository commit the cache was last checked against.
    pub const GAME_DATA_REPO_COMMIT: &str = "game_data_repo_commit";
    /// The name this app appears under to the peers in a session.
    pub const COLLAB_DISPLAY_NAME: &str = "collab_display_name";
    /// Whether the warning that a session reveals an address is suppressed.
    pub const SUPPRESS_P2P_IP_WARNING: &str = "suppress_p2p_ip_warning";
    /// Whether windows a session opens are left for the reader to open.
    pub const DISABLE_AUTO_OPEN_SESSION_WINDOWS: &str = "disable_auto_open_session_windows";
    /// Whether the reader has been asked what battle data to share.
    pub const BUILD_CONSENT_SHOWN: &str = "build_consent_window_shown";
    /// Whether the reader has been offered sharing whole replays, which was
    /// added after build data.
    pub const REPLAY_CONSENT_SHOWN: &str = "replay_consent_prompt_shown";
    /// Whether the reader has been asked which language to read in.
    pub const LANGUAGE_SELECTION_SHOWN: &str = "language_selection_shown";
    /// Whether the notice that an export fell back to software encoding is
    /// suppressed.
    pub const SUPPRESS_GPU_ENCODER_WARNING: &str = "suppress_gpu_encoder_warning";
    /// The commit the cached result mappings were last read at, which is what
    /// makes the next check one small request.
    pub const CONSTANTS_FILE_COMMIT: &str = "constants_file_commit";
    /// Whether Code Integrity Guard is applied: a
    /// `wows_toolkit_hardening::CodeIntegrityPreference`. The one process
    /// mitigation with a real compatibility cost, so the one the reader chooses.
    pub const CODE_INTEGRITY: &str = "code_integrity";
}

/// A proxy setting as a URL a client can take.
///
/// `None` for an unset proxy. A bare `host:port` gets the `http://` scheme a
/// client requires: that is what the Windows proxy dialog stores, and
/// rejecting it would silently send direct.
pub fn normalize_proxy_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.contains("://") { Some(raw.to_string()) } else { Some(format!("http://{raw}")) }
}

/// Which theme the app renders in.
///
/// Stored as the variant name, which is what the egui app already wrote to
/// the database; renaming a variant would silently read back as `System`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    /// Follow the desktop's light/dark preference.
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [Self::System, Self::Dark, Self::Light];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    /// Whether this choice renders dark, given what the desktop reports.
    ///
    /// `system_is_dark` is consulted only for [`Self::System`]; an explicit
    /// choice ignores the desktop, which is the point of making one.
    pub fn is_dark(self, system_is_dark: bool) -> bool {
        match self {
            Self::System => system_is_dark,
            Self::Dark => true,
            Self::Light => false,
        }
    }
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

    /// Reads the `send_replay_data` bool an older build wrote on its own.
    ///
    /// That bool meant build data, so it never resolves to `Replays`: sharing the
    /// file itself is a choice that build only offered as sharing builds.
    pub fn from_send_replay_data_bool(enabled: bool) -> Self {
        if enabled { Self::BuildData } else { Self::Off }
    }

    /// Settles a disagreement between this mode and the `send_replay_data` bool
    /// beside it.
    ///
    /// Both rows exist because an older build writes only the bool, so a
    /// disagreement means the bool is the newer word on it. As with the bool
    /// itself, agreeing with it never escalates to `Replays`.
    pub fn reconcile_with_compat_bool(self, shared: bool) -> Self {
        match (self, shared) {
            (Self::Off, true) => Self::BuildData,
            (mode, false) if mode.shares_anything() => Self::Off,
            (mode, _) => mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DataSharingMode;
    use super::ThemeChoice;
    use super::normalize_proxy_url;

    /// The stored form is what is already in the database, so reading it as
    /// anything else would misreport what the reader agreed to.
    #[test]
    fn the_mode_is_stored_snake_case() {
        assert_eq!(serde_json::to_string(&DataSharingMode::Off).expect("it serializes"), "\"off\"");
        assert_eq!(serde_json::to_string(&DataSharingMode::BuildData).expect("it serializes"), "\"build_data\"");
        assert_eq!(serde_json::to_string(&DataSharingMode::Replays).expect("it serializes"), "\"replays\"");
    }

    /// The bool an older build wrote means build data, both ways round.
    #[test]
    fn the_legacy_bool_never_escalates_to_replays() {
        assert_eq!(DataSharingMode::from_send_replay_data_bool(true), DataSharingMode::BuildData);
        assert_eq!(DataSharingMode::from_send_replay_data_bool(false), DataSharingMode::Off);

        assert_eq!(DataSharingMode::Off.reconcile_with_compat_bool(true), DataSharingMode::BuildData);
        assert_eq!(DataSharingMode::Replays.reconcile_with_compat_bool(false), DataSharingMode::Off);
        assert_eq!(DataSharingMode::Replays.reconcile_with_compat_bool(true), DataSharingMode::Replays);
    }

    #[test]
    fn an_unset_proxy_is_absent_rather_than_an_empty_url() {
        assert_eq!(normalize_proxy_url(""), None);
        assert_eq!(normalize_proxy_url("   "), None);
    }

    /// The Windows proxy dialog stores a bare host and port.
    #[test]
    fn a_scheme_less_proxy_gets_one() {
        assert_eq!(normalize_proxy_url("proxy.corp:8080"), Some("http://proxy.corp:8080".to_string()));
        assert_eq!(normalize_proxy_url("https://proxy.corp:8080"), Some("https://proxy.corp:8080".to_string()));
    }

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

    /// Pinned against what the egui app already wrote: the database holds
    /// the variant names, and reading anything else would quietly put every
    /// user back on the system theme.
    #[test]
    fn the_stored_theme_is_the_variant_name() {
        assert_eq!(serde_json::to_string(&ThemeChoice::System).unwrap(), "\"System\"");
        assert_eq!(serde_json::to_string(&ThemeChoice::Dark).unwrap(), "\"Dark\"");
        assert_eq!(serde_json::to_string(&ThemeChoice::Light).unwrap(), "\"Light\"");
        assert_eq!(serde_json::from_str::<ThemeChoice>("\"Light\"").unwrap(), ThemeChoice::Light);
    }

    /// Only the system choice asks the desktop; an explicit one is explicit.
    #[test]
    fn an_explicit_theme_ignores_what_the_desktop_reports() {
        assert!(ThemeChoice::System.is_dark(true));
        assert!(!ThemeChoice::System.is_dark(false));
        assert!(ThemeChoice::Dark.is_dark(false));
        assert!(!ThemeChoice::Light.is_dark(true));
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
