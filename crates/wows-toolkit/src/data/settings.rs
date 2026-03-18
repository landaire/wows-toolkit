use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use wows_minimap_renderer::ShipConfigFilter;

use crate::data::session_stats::DivisionFilter;
use crate::task::ReplayExportFormat;
use crate::twitch::Token;

/// Replay grouping strategy in the file browser
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayGrouping {
    #[default]
    Date,
    Ship,
    None,
}

impl ReplayGrouping {
    pub fn label(&self) -> &'static str {
        match self {
            ReplayGrouping::Date => "Date",
            ReplayGrouping::Ship => "Ship",
            ReplayGrouping::None => "None",
        }
    }
}

/// Settings specific to replay parsing and display
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplaySettings {
    pub show_entity_id: bool,
    pub show_observed_damage: bool,
    #[serde(default)]
    pub show_raw_xp: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_fires: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_floods: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_citadels: bool,
    #[serde(default = "default_bool::<false>")]
    pub show_crits: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_received_damage: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_distance_traveled: bool,
    #[serde(default = "default_bool::<false>")]
    pub auto_export_data: bool,
    #[serde(default)]
    pub auto_export_path: String,
    #[serde(default)]
    pub auto_export_format: ReplayExportFormat,
    #[serde(default)]
    pub grouping: ReplayGrouping,
}

impl Default for ReplaySettings {
    fn default() -> Self {
        Self {
            show_entity_id: false,
            show_observed_damage: false,
            show_raw_xp: false,
            show_fires: true,
            show_floods: true,
            show_citadels: true,
            show_crits: false,
            show_received_damage: true,
            show_distance_traveled: true,
            auto_export_data: false,
            auto_export_path: String::new(),
            auto_export_format: ReplayExportFormat::default(),
            grouping: ReplayGrouping::default(),
        }
    }
}

pub const fn default_bool<const V: bool>() -> bool {
    V
}

/// Serializable mirror of minimap_renderer's RenderOptions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedRenderOptions {
    #[serde(default = "default_bool::<true>")]
    pub show_hp_bars: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_tracers: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_torpedoes: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_planes: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_smoke: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_score: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_timer: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_kill_feed: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_player_names: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_ship_names: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_capture_points: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_buildings: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_turret_direction: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_consumables: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_dead_ships: bool,
    #[serde(default)]
    pub show_dead_ship_names: bool,
    #[serde(default)]
    pub show_armament: bool,
    #[serde(default)]
    pub show_trails: bool,
    #[serde(default)]
    pub show_dead_trails: bool,
    #[serde(default)]
    pub show_speed_trails: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_battle_result: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_buffs: bool,
    #[serde(default)]
    pub show_ship_config: bool,
    #[serde(default)]
    pub show_self_detection_range: bool,
    #[serde(default)]
    pub show_self_main_battery_range: bool,
    #[serde(default)]
    pub show_self_secondary_range: bool,
    #[serde(default)]
    pub show_self_torpedo_range: bool,
    #[serde(default)]
    pub show_self_radar_range: bool,
    #[serde(default)]
    pub show_self_hydro_range: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_chat: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_advantage: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_score_timer: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_stats_panel: bool,
    /// Prefer CPU (software) encoder for video export instead of GPU hardware encoder.
    #[serde(default)]
    pub prefer_cpu_encoder: bool,
}

impl Default for SavedRenderOptions {
    fn default() -> Self {
        Self {
            show_hp_bars: true,
            show_tracers: true,
            show_torpedoes: true,
            show_planes: true,
            show_smoke: true,
            show_score: true,
            show_timer: true,
            show_kill_feed: true,
            show_player_names: true,
            show_ship_names: true,
            show_capture_points: true,
            show_buildings: true,
            show_turret_direction: true,
            show_consumables: true,
            show_dead_ships: true,
            show_dead_ship_names: false,
            show_armament: true,
            show_trails: false,
            show_dead_trails: false,
            show_speed_trails: false,
            show_battle_result: true,
            show_buffs: true,
            show_ship_config: false,
            show_self_detection_range: false,
            show_self_main_battery_range: false,
            show_self_secondary_range: false,
            show_self_torpedo_range: false,
            show_self_radar_range: false,
            show_self_hydro_range: false,
            show_chat: false,
            show_advantage: true,
            show_score_timer: true,
            show_stats_panel: true,
            prefer_cpu_encoder: false,
        }
    }
}

impl SavedRenderOptions {
    /// Get self ship range visibility as a `ShipConfigFilter`.
    pub fn self_range_filter(&self) -> ShipConfigFilter {
        ShipConfigFilter {
            detection: self.show_self_detection_range,
            main_battery: self.show_self_main_battery_range,
            secondary_battery: self.show_self_secondary_range,
            torpedo: self.show_self_torpedo_range,
            radar: self.show_self_radar_range,
            hydro: self.show_self_hydro_range,
        }
    }

    /// Update self ship range visibility from a `ShipConfigFilter`.
    pub fn set_self_range_filter(&mut self, filter: &ShipConfigFilter) {
        self.show_self_detection_range = filter.detection;
        self.show_self_main_battery_range = filter.main_battery;
        self.show_self_secondary_range = filter.secondary_battery;
        self.show_self_torpedo_range = filter.torpedo;
        self.show_self_radar_range = filter.radar;
        self.show_self_hydro_range = filter.hydro;
    }

    /// Returns true if any self range is enabled.
    pub fn any_self_range_enabled(&self) -> bool {
        self.self_range_filter().any_enabled()
    }
}

// ---------------------------------------------------------------------------
// New nested AppSettings
// ---------------------------------------------------------------------------

/// Top-level application settings, grouped by concern.
#[derive(Default)]
pub struct AppSettings {
    pub app: AppPreferences,
    pub game: GameSettings,
    pub replay: ReplaySettings,
    pub renderer: SavedRenderOptions,
    pub stats_filters: StatsFilterSettings,
    pub integrations: IntegrationSettings,
    pub collab: CollabSettings,
}

/// General application preferences.
pub struct AppPreferences {
    pub check_for_updates: bool,
    pub debug_mode: bool,
    pub enable_logging: bool,
    pub locale: Option<String>,
    pub build_consent_window_shown: bool,
    pub language_selection_shown: bool,
    pub suppress_gpu_encoder_warning: bool,
    /// UI zoom factor (default 1.15).
    pub zoom_factor: f32,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            check_for_updates: true,
            debug_mode: false,
            enable_logging: true,
            locale: Some("en".to_string()),
            build_consent_window_shown: false,
            language_selection_shown: false,
            suppress_gpu_encoder_warning: false,
            zoom_factor: 1.15,
        }
    }
}

/// Game installation and data paths.
pub struct GameSettings {
    pub wows_dir: String,
    pub current_replay_path: PathBuf,
    pub constants_file_commit: Option<String>,
    pub has_052_game_params_fix: bool,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            wows_dir: "C:\\Games\\World_of_Warships".to_string(),
            current_replay_path: Default::default(),
            constants_file_commit: None,
            has_052_game_params_fix: true,
        }
    }
}

/// Session stats display filters.
pub struct StatsFilterSettings {
    pub limit_enabled: bool,
    pub game_count: usize,
    pub division_filter: DivisionFilter,
    pub game_mode_filter: BTreeSet<String>,
}

impl Default for StatsFilterSettings {
    fn default() -> Self {
        Self {
            limit_enabled: false,
            game_count: 20,
            division_filter: DivisionFilter::default(),
            game_mode_filter: BTreeSet::default(),
        }
    }
}

/// External service integrations.
#[derive(Default)]
pub struct IntegrationSettings {
    pub send_replay_data: bool,
    pub twitch_token: Option<Token>,
    pub twitch_monitored_channel: String,
}

/// Collaborative session settings.
#[derive(Default)]
pub struct CollabSettings {
    pub display_name: String,
    pub suppress_p2p_ip_warning: bool,
    pub disable_auto_open_session_windows: bool,
}
