use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;

use crate::task::ReplayExportFormat;
use crate::twitch::Token;
use crate::ui::player_tracker::PlayerTracker;

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
#[derive(Serialize, Deserialize)]
pub struct ReplaySettings {
    pub show_game_chat: bool,
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
            show_game_chat: true,
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

fn default_session_stats_game_count() -> usize {
    20
}

pub fn default_sent_replays() -> Arc<RwLock<HashSet<String>>> {
    Default::default()
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
    #[serde(default = "default_bool::<true>")]
    pub show_chat: bool,
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
            show_armament: false,
            show_trails: false,
            show_dead_trails: false,
            show_speed_trails: false,
            show_battle_result: true,
            show_buffs: true,
            show_ship_config: false,
            show_chat: true,
        }
    }
}

/// Global application settings
#[derive(Serialize, Deserialize)]
pub struct Settings {
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
    #[serde(default = "default_bool::<false>")]
    pub has_default_value_fix_015: bool,
    #[serde(default = "default_sent_replays")]
    pub sent_replays: Arc<RwLock<HashSet<String>>>,
    #[serde(default = "default_bool::<false>")]
    pub has_019_game_params_update: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_037_crew_skills_fix: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_038_game_params_fix: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_041_game_params_fix: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_047_game_params_fix: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_047_ship_config_fix: bool,
    #[serde(default = "default_bool::<false>")]
    pub has_047_game_params_fix_for_buffs: bool,
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
    pub renderer_options: SavedRenderOptions,
    /// Whether to limit session stats to the N most recent games.
    #[serde(default = "default_bool::<false>")]
    pub session_stats_limit_enabled: bool,
    /// Number of most recent games to show when limit is enabled.
    #[serde(default = "default_session_stats_game_count")]
    pub session_stats_game_count: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            current_replay_path: Default::default(),
            wows_dir: "C:\\Games\\World_of_Warships".to_string(),
            replays_dir: Default::default(),
            locale: Some("en".to_string()),
            replay_settings: Default::default(),
            check_for_updates: true,
            send_replay_data: false,
            has_default_value_fix_015: true,
            sent_replays: Default::default(),
            has_019_game_params_update: true,
            player_tracker: Default::default(),
            twitch_token: Default::default(),
            twitch_monitored_channel: Default::default(),
            constants_file_commit: None,
            debug_mode: false,
            build_consent_window_shown: false,
            has_037_crew_skills_fix: true,
            has_038_game_params_fix: true,
            has_041_game_params_fix: true,
            has_047_game_params_fix: true,
            has_047_ship_config_fix: true,
            has_047_game_params_fix_for_buffs: true,
            renderer_options: Default::default(),
            session_stats_limit_enabled: false,
            session_stats_game_count: 20,
        }
    }
}
