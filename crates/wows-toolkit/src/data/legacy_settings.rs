//! Legacy `Settings` struct preserved verbatim for deserializing `app.ron`
//! during the one-time migration to SQLite + the new `AppSettings` structure.
//!
//! **Do not use this module for anything other than the RON migration path.**

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;

use crate::armor_viewer::state::ArmorViewerDefaults;
use crate::data::session_stats::DivisionFilter;
use crate::data::session_stats::SessionStats;
use crate::data::settings::AppPreferences;
use crate::data::settings::AppSettings;
use crate::data::settings::CollabSettings;
use crate::data::settings::GameSettings;
use crate::data::settings::IntegrationSettings;
use crate::data::settings::ReplaySettings;
use crate::data::settings::SavedRenderOptions;
use crate::data::settings::StatsFilterSettings;
use crate::data::settings::default_bool;
use crate::tab_state::PersistedState;
use crate::tab_state::SessionStatsChartConfig;
use crate::tab_state::StatsSubTab;
use crate::twitch::Token;
use crate::ui::mod_manager::ModManagerInfo;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::SortOrder;

fn default_session_stats_game_count() -> usize {
    20
}

fn default_sent_replays() -> Arc<RwLock<HashSet<String>>> {
    Default::default()
}

fn default_stats_dock_state() -> egui_dock::DockState<StatsSubTab> {
    egui_dock::DockState::new([StatsSubTab::Overview].to_vec())
}

/// The old flat `Settings` struct as it existed in `app.ron`.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct LegacySettings {
    pub current_replay_path: PathBuf,
    pub wows_dir: String,
    #[serde(skip)]
    pub replays_dir: Option<PathBuf>,
    pub locale: Option<String>,
    #[serde(default)]
    pub replay_settings: ReplaySettings,
    #[serde(default = "default_bool::<true>")]
    pub check_for_updates: bool,
    #[serde(default = "default_bool::<false>")]
    pub send_replay_data: bool,
    #[serde(default = "default_sent_replays")]
    pub sent_replays: Arc<RwLock<HashSet<String>>>,
    #[serde(default = "default_bool::<false>")]
    pub has_052_game_params_fix: bool,
    #[serde(default)]
    pub player_tracker: Arc<RwLock<PlayerTracker>>,
    #[serde(default)]
    pub twitch_token: Option<Token>,
    #[serde(default)]
    pub twitch_monitored_channel: String,
    #[serde(default)]
    pub constants_file_commit: Option<String>,
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub build_consent_window_shown: bool,
    #[serde(default)]
    pub language_selection_shown: bool,
    #[serde(default)]
    pub renderer_options: SavedRenderOptions,
    #[serde(default = "default_bool::<false>")]
    pub session_stats_limit_enabled: bool,
    #[serde(default = "default_session_stats_game_count")]
    pub session_stats_game_count: usize,
    #[serde(default)]
    pub session_stats_division_filter: DivisionFilter,
    #[serde(default)]
    pub session_stats_game_mode_filter: BTreeSet<String>,
    #[serde(default)]
    pub session_stats: SessionStats,
    #[serde(default)]
    pub suppress_gpu_encoder_warning: bool,
    #[serde(default = "default_bool::<true>")]
    pub enable_logging: bool,
    #[serde(default)]
    pub collab_display_name: String,
    #[serde(default)]
    pub suppress_p2p_ip_warning: bool,
    #[serde(default)]
    pub disable_auto_open_session_windows: bool,
}

impl Default for LegacySettings {
    fn default() -> Self {
        Self {
            current_replay_path: Default::default(),
            wows_dir: "C:\\Games\\World_of_Warships".to_string(),
            replays_dir: Default::default(),
            locale: Some("en".to_string()),
            replay_settings: Default::default(),
            check_for_updates: true,
            send_replay_data: false,
            sent_replays: Default::default(),
            player_tracker: Default::default(),
            twitch_token: Default::default(),
            twitch_monitored_channel: Default::default(),
            constants_file_commit: None,
            debug_mode: false,
            build_consent_window_shown: false,
            language_selection_shown: false,
            has_052_game_params_fix: true,
            renderer_options: Default::default(),
            session_stats_limit_enabled: false,
            session_stats_game_count: 20,
            session_stats_division_filter: DivisionFilter::default(),
            session_stats_game_mode_filter: BTreeSet::default(),
            session_stats: SessionStats::default(),
            suppress_gpu_encoder_warning: false,
            enable_logging: true,
            collab_display_name: String::new(),
            suppress_p2p_ip_warning: false,
            disable_auto_open_session_windows: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy deserialization wrappers for the one-time RON migration
// ---------------------------------------------------------------------------

/// Only the serialized (non-skip) fields from the old `TabState`.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct LegacyTabState {
    pub settings: LegacySettings,
    pub output_dir: String,
    #[serde(default = "default_bool::<true>")]
    pub auto_load_latest_replay: bool,
    #[serde(default)]
    pub replay_sort: Arc<Mutex<SortOrder>>,
    #[serde(default)]
    pub mod_manager_info: ModManagerInfo,
    #[serde(default = "default_stats_dock_state")]
    pub stats_dock_state: egui_dock::DockState<StatsSubTab>,
    #[serde(default)]
    pub next_chart_tab_id: u64,
    #[serde(default)]
    pub chart_configs: HashMap<u64, SessionStatsChartConfig>,
    #[serde(default)]
    pub armor_viewer_defaults: ArmorViewerDefaults,
}

impl Default for LegacyTabState {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            output_dir: String::new(),
            auto_load_latest_replay: true,
            replay_sort: Default::default(),
            mod_manager_info: Default::default(),
            stats_dock_state: default_stats_dock_state(),
            next_chart_tab_id: 0,
            chart_configs: Default::default(),
            armor_viewer_defaults: Default::default(),
        }
    }
}

/// Top-level legacy app struct matching the old `WowsToolkitApp` RON layout.
/// Only `tab_state` was non-skip in practice.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LegacyWowsToolkitApp {
    pub tab_state: LegacyTabState,
}

impl LegacyWowsToolkitApp {
    /// Convert the deserialized legacy state into the new structures.
    ///
    /// Returns `(persisted, player_tracker, sent_replays, replay_sort)`.
    #[allow(clippy::type_complexity)]
    pub fn into_new_state(
        self,
    ) -> (PersistedState, Arc<RwLock<PlayerTracker>>, Arc<RwLock<HashSet<String>>>, Arc<Mutex<SortOrder>>) {
        let ts = self.tab_state;
        let s = ts.settings;

        // Extract data stores before consuming settings fields.
        let player_tracker = s.player_tracker;
        let sent_replays = s.sent_replays;
        let session_stats = s.session_stats;

        let settings = AppSettings {
            app: AppPreferences {
                check_for_updates: s.check_for_updates,
                debug_mode: s.debug_mode,
                enable_logging: s.enable_logging,
                locale: s.locale,
                build_consent_window_shown: s.build_consent_window_shown,
                language_selection_shown: s.language_selection_shown,
                suppress_gpu_encoder_warning: s.suppress_gpu_encoder_warning,
                zoom_factor: 1.15,
            },
            game: GameSettings {
                wows_dir: s.wows_dir,
                current_replay_path: s.current_replay_path,
                constants_file_commit: s.constants_file_commit,
                has_052_game_params_fix: s.has_052_game_params_fix,
            },
            replay: s.replay_settings,
            renderer: s.renderer_options,
            stats_filters: StatsFilterSettings {
                limit_enabled: s.session_stats_limit_enabled,
                game_count: s.session_stats_game_count,
                division_filter: s.session_stats_division_filter,
                game_mode_filter: s.session_stats_game_mode_filter,
            },
            integrations: IntegrationSettings {
                send_replay_data: s.send_replay_data,
                twitch_token: s.twitch_token,
                twitch_monitored_channel: s.twitch_monitored_channel,
            },
            collab: CollabSettings {
                display_name: s.collab_display_name,
                suppress_p2p_ip_warning: s.suppress_p2p_ip_warning,
                disable_auto_open_session_windows: s.disable_auto_open_session_windows,
            },
        };

        let persisted = PersistedState {
            settings,
            output_dir: ts.output_dir,
            auto_load_latest_replay: ts.auto_load_latest_replay,
            mod_manager_info: ts.mod_manager_info,
            stats_dock_state: ts.stats_dock_state,
            next_chart_tab_id: ts.next_chart_tab_id,
            chart_configs: ts.chart_configs,
            armor_viewer_defaults: ts.armor_viewer_defaults,
            session_stats,
        };

        (persisted, player_tracker, sent_replays, ts.replay_sort)
    }
}
