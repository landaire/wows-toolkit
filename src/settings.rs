use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

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

pub fn default_sent_replays() -> Arc<RwLock<HashSet<String>>> {
    Default::default()
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
        }
    }
}
