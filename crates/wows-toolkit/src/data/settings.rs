use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use wows_minimap_renderer::ShipConfigFilter;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_ast::OperatorPreferences;

use crate::data::session_stats::DivisionFilter;
use crate::twitch::Token;

pub use wows_toolkit_config::ReplayGrouping;
pub use wows_toolkit_config::ReplaySettings;

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
    #[serde(default)]
    pub show_player_names: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_ship_names: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_capture_points: bool,
    #[serde(default = "default_bool::<true>")]
    pub show_buildings: bool,
    #[serde(default = "default_bool::<true>", alias = "show_turret_direction")]
    pub show_camera_direction: bool,
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
    #[serde(default = "default_bool::<true>")]
    pub show_team_rosters: bool,
    /// Prefer CPU (software) encoder for video export instead of GPU hardware encoder.
    #[serde(default)]
    pub prefer_cpu_encoder: bool,
    /// Video codec preference. `None` means "best codec for the current backend".
    #[serde(default)]
    pub video_codec: Option<wows_minimap_renderer::VideoCodec>,
    /// Include the pre-battle phase (spawn and countdown) at the start of an
    /// exported video. When false, export begins at battle start.
    #[serde(default)]
    pub include_pre_battle: bool,
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
            show_player_names: false,
            show_ship_names: true,
            show_capture_points: true,
            show_buildings: true,
            show_camera_direction: true,
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
            // Stats panel is the default side-panel; team rosters get swapped
            // in automatically when a merged replay is loaded (handled by the
            // renderer launcher), and the user can flip them manually too.
            show_stats_panel: true,
            show_team_rosters: false,
            prefer_cpu_encoder: false,
            video_codec: None,
            include_pre_battle: false,
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
    pub search: SearchSettings,
}

/// What the Search tab keeps across restarts.
///
/// The query is stored as its canonical text rather than as a serialized tree:
/// it is the same string the user copies to share a search, and it is the form
/// the grammar is the single authority on. A tree would need a second
/// serialization to keep in step with it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchSettings {
    /// Canonical query text, the same string the user copies to share.
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub saved: Vec<SavedSearch>,
    #[serde(default)]
    pub history: std::collections::VecDeque<String>,
    #[serde(default = "ResultColumn::default_columns")]
    pub columns: Vec<ResultColumn>,
    #[serde(default = "default_result_sort")]
    pub sort: (ResultColumn, SortDir),
    /// The operator each field was last filtered with, so a new filter starts
    /// on the comparison the user reached for last.
    #[serde(default)]
    pub op_prefs: OperatorPreferences,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            query: String::new(),
            saved: Vec::new(),
            history: std::collections::VecDeque::new(),
            columns: ResultColumn::default_columns(),
            sort: default_result_sort(),
            op_prefs: OperatorPreferences::default(),
        }
    }
}

/// A query the user named and kept.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSearch {
    pub name: String,
    /// Canonical query text, the same form `SearchSettings::query` holds.
    pub query: String,
}

/// A column of the Search tab's results table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResultColumn {
    Date,
    Map,
    Mode,
    Ship,
    Outcome,
    Damage,
    Kills,
    Pr,
}

impl ResultColumn {
    /// The columns a tab shows before the user has chosen any.
    pub fn default_columns() -> Vec<ResultColumn> {
        vec![
            ResultColumn::Date,
            ResultColumn::Map,
            ResultColumn::Mode,
            ResultColumn::Ship,
            ResultColumn::Outcome,
            ResultColumn::Damage,
            ResultColumn::Kills,
            ResultColumn::Pr,
        ]
    }

    /// The index column whose ordering reproduces what this column displays, or
    /// `None` for a column the index cannot order by.
    ///
    /// `Ship` is the one exclusion. Its cell is composed on the UI thread from
    /// whichever build's game data is loaded, falling back to the name frozen
    /// at index time and then to a bracketed id (see `ship_display_name`). The
    /// index holds only that frozen name, so an `ORDER BY` over it would put
    /// rows in an order the names on screen do not read in.
    pub fn sort_column(self) -> Option<SortColumn> {
        match self {
            ResultColumn::Date => Some(SortColumn::Date),
            ResultColumn::Map => Some(SortColumn::Map),
            ResultColumn::Mode => Some(SortColumn::Mode),
            ResultColumn::Outcome => Some(SortColumn::Outcome),
            ResultColumn::Damage => Some(SortColumn::Damage),
            ResultColumn::Kills => Some(SortColumn::Kills),
            ResultColumn::Pr => Some(SortColumn::Pr),
            ResultColumn::Ship => None,
        }
    }
}

impl From<SortColumn> for ResultColumn {
    fn from(column: SortColumn) -> Self {
        match column {
            SortColumn::Date => ResultColumn::Date,
            SortColumn::Map => ResultColumn::Map,
            SortColumn::Mode => ResultColumn::Mode,
            SortColumn::Outcome => ResultColumn::Outcome,
            SortColumn::Damage => ResultColumn::Damage,
            SortColumn::Kills => ResultColumn::Kills,
            SortColumn::Pr => ResultColumn::Pr,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDir {
    Ascending,
    Descending,
}

impl From<SortDir> for SortDirection {
    fn from(dir: SortDir) -> Self {
        match dir {
            SortDir::Ascending => SortDirection::Ascending,
            SortDir::Descending => SortDirection::Descending,
        }
    }
}

impl From<SortDirection> for SortDir {
    fn from(direction: SortDirection) -> Self {
        match direction {
            SortDirection::Ascending => SortDir::Ascending,
            SortDirection::Descending => SortDir::Descending,
        }
    }
}

impl SearchSettings {
    /// The stored sort as the index understands it.
    ///
    /// A stored column the index cannot order by falls back to the default
    /// rather than being ordered by something adjacent. Nothing in the app
    /// writes one, but a config carried forward from a build whose columns
    /// differed, or edited by hand, can hold one.
    pub fn sort_spec(&self) -> SortSpec {
        let (column, direction) = self.sort;
        match column.sort_column() {
            Some(column) => SortSpec { column, direction: direction.into() },
            None => SortSpec::default(),
        }
    }

    pub fn set_sort_spec(&mut self, spec: SortSpec) {
        self.sort = (spec.column.into(), spec.direction.into());
    }
}

/// Newest match first, which is the order the index query itself returns.
fn default_result_sort() -> (ResultColumn, SortDir) {
    (ResultColumn::Date, SortDir::Descending)
}

/// General application preferences.
pub struct AppPreferences {
    pub check_for_updates: bool,
    pub debug_mode: bool,
    pub enable_logging: bool,
    pub locale: Option<String>,
    pub build_consent_window_shown: bool,
    pub language_selection_shown: bool,
    pub replay_consent_prompt_shown: bool,
    pub suppress_gpu_encoder_warning: bool,
    /// UI zoom factor (default 1.15).
    pub zoom_factor: f32,
    /// Which theme to render in.
    pub theme: ThemeChoice,
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
            replay_consent_prompt_shown: false,
            suppress_gpu_encoder_warning: false,
            zoom_factor: 1.15,
            theme: ThemeChoice::default(),
        }
    }
}

/// Game installation and data paths.
pub struct GameSettings {
    pub wows_dir: String,
    pub current_replay_path: PathBuf,
    pub constants_file_commit: Option<String>,
    pub has_052_game_params_fix: bool,
    /// Automatically dump game data on load so old replays work after a game update.
    pub auto_dump_game_data: bool,
    /// Custom directory for game data cache. When empty, uses the default app data location.
    pub game_data_cache_dir: String,
    /// Commit of the game data repository at the last successful update check.
    /// Used to skip per-build comparisons when nothing has changed upstream.
    pub game_data_repo_commit: Option<String>,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            wows_dir: "C:\\Games\\World_of_Warships".to_string(),
            current_replay_path: Default::default(),
            constants_file_commit: None,
            has_052_game_params_fix: true,
            auto_dump_game_data: false,
            game_data_cache_dir: String::new(),
            game_data_repo_commit: None,
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

/// Which theme the app renders in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum ThemeChoice {
    /// Follow the desktop's light/dark preference.
    #[default]
    System,
    Dark,
    Light,
}

impl From<ThemeChoice> for egui::ThemePreference {
    fn from(choice: ThemeChoice) -> Self {
        match choice {
            ThemeChoice::System => Self::System,
            ThemeChoice::Dark => Self::Dark,
            ThemeChoice::Light => Self::Light,
        }
    }
}

/// How much battle data the user has agreed to share with the server.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSharingMode {
    /// Share nothing.
    #[default]
    Off,
    /// Send per-player build payloads to `/api/ship_builds`.
    BuildData,
    /// Send the raw replay file to `/api/replays`; test-ship battles fall back
    /// to build data.
    Replays,
}

impl DataSharingMode {
    /// Migrate the legacy `send_replay_data` bool. `true` shared build data, so
    /// map to `BuildData`; never escalate to `Replays`.
    pub fn from_send_replay_data_bool(enabled: bool) -> Self {
        if enabled { Self::BuildData } else { Self::Off }
    }

    /// Whether any data is shared. Used to persist a downgrade-compatible bool.
    pub fn shares_anything(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Reconcile a loaded mode against the downgrade-compat `send_replay_data`
    /// bool when both persisted keys exist. An older app build writes only the
    /// bool, so a disagreement means the bool is the newer signal; honor it
    /// without ever escalating to Replays.
    pub fn reconcile_with_compat_bool(self, shared: bool) -> Self {
        match (self, shared) {
            (Self::Off, true) => Self::BuildData,
            (mode, false) if mode.shares_anything() => Self::Off,
            (mode, _) => mode,
        }
    }
}

/// External service integrations.
#[derive(Default)]
pub struct IntegrationSettings {
    pub data_sharing_mode: DataSharingMode,
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

#[cfg(test)]
mod data_sharing_mode_tests {
    use super::DataSharingMode;

    #[test]
    fn serializes_to_stable_snake_case() {
        assert_eq!(serde_json::to_string(&DataSharingMode::Off).unwrap(), "\"off\"");
        assert_eq!(serde_json::to_string(&DataSharingMode::BuildData).unwrap(), "\"build_data\"");
        assert_eq!(serde_json::to_string(&DataSharingMode::Replays).unwrap(), "\"replays\"");
    }

    #[test]
    fn round_trips_through_json() {
        for mode in [DataSharingMode::Off, DataSharingMode::BuildData, DataSharingMode::Replays] {
            let s = serde_json::to_string(&mode).unwrap();
            let back: DataSharingMode = serde_json::from_str(&s).unwrap();
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn legacy_bool_maps_without_escalation() {
        assert_eq!(DataSharingMode::from_send_replay_data_bool(true), DataSharingMode::BuildData);
        assert_eq!(DataSharingMode::from_send_replay_data_bool(false), DataSharingMode::Off);
    }

    #[test]
    fn shares_anything_only_when_not_off() {
        assert!(!DataSharingMode::Off.shares_anything());
        assert!(DataSharingMode::BuildData.shares_anything());
        assert!(DataSharingMode::Replays.shares_anything());
    }

    #[test]
    fn reconcile_is_identity_when_bool_agrees() {
        assert_eq!(DataSharingMode::Off.reconcile_with_compat_bool(false), DataSharingMode::Off);
        assert_eq!(DataSharingMode::BuildData.reconcile_with_compat_bool(true), DataSharingMode::BuildData);
        assert_eq!(DataSharingMode::Replays.reconcile_with_compat_bool(true), DataSharingMode::Replays);
    }

    #[test]
    fn reconcile_honors_opt_out() {
        assert_eq!(DataSharingMode::Replays.reconcile_with_compat_bool(false), DataSharingMode::Off);
        assert_eq!(DataSharingMode::BuildData.reconcile_with_compat_bool(false), DataSharingMode::Off);
    }

    #[test]
    fn reconcile_never_escalates_to_replays() {
        assert_eq!(DataSharingMode::Off.reconcile_with_compat_bool(true), DataSharingMode::BuildData);
    }
}

#[cfg(test)]
mod theme_choice_tests {
    use super::*;

    #[test]
    fn default_follows_the_system() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }

    #[test]
    fn round_trips_through_json() {
        for choice in [ThemeChoice::System, ThemeChoice::Dark, ThemeChoice::Light] {
            let encoded = serde_json::to_string(&choice).expect("serialises");
            let decoded: ThemeChoice = serde_json::from_str(&encoded).expect("deserialises");
            assert_eq!(decoded, choice);
        }
    }

    #[test]
    fn maps_to_egui_theme_preference() {
        assert_eq!(egui::ThemePreference::from(ThemeChoice::System), egui::ThemePreference::System);
        assert_eq!(egui::ThemePreference::from(ThemeChoice::Dark), egui::ThemePreference::Dark);
        assert_eq!(egui::ThemePreference::from(ThemeChoice::Light), egui::ThemePreference::Light);
    }
}

#[cfg(test)]
mod search_sort_tests {
    use super::*;

    /// The whole point of persisting it: the sort the user clicked is what the
    /// next launch reads back.
    #[test]
    fn a_chosen_sort_round_trips_through_the_persisted_settings() {
        for column in SortColumn::ALL {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                let spec = SortSpec { column, direction };
                let mut settings = SearchSettings::default();
                settings.set_sort_spec(spec);

                let encoded = serde_json::to_string(&settings).expect("serialises");
                let decoded: SearchSettings = serde_json::from_str(&encoded).expect("deserialises");
                assert_eq!(decoded.sort_spec(), spec);
            }
        }
    }

    /// The stored pair is what shipped builds already wrote, so the two names
    /// have to keep deserialising or a launch would lose every search setting
    /// at once, not just the sort.
    #[test]
    fn the_shipped_stored_form_still_reads_back() {
        let settings: SearchSettings =
            serde_json::from_str(r#"{"query":"","saved":[],"history":[],"sort":["Date","Descending"]}"#)
                .expect("the form already on disk must still deserialise");
        assert_eq!(settings.sort_spec(), SortSpec::default());
    }

    /// A column the index cannot order by must not be honoured as though it
    /// could. Ordering by something adjacent would be the failure the whole
    /// exclusion exists to avoid, only now invisible.
    #[test]
    fn a_stored_column_the_index_cannot_order_by_falls_back_to_the_default() {
        let settings = SearchSettings { sort: (ResultColumn::Ship, SortDir::Ascending), ..Default::default() };
        assert_eq!(settings.sort_spec(), SortSpec::default());
    }

    /// A remembered operator is worth nothing if it does not outlive the
    /// session that learned it.
    #[test]
    fn remembered_operators_survive_a_settings_round_trip() {
        use wows_toolkit_config::index::query_ast::Op;
        use wows_toolkit_config::index::query_ast::RosterField;

        let mut settings = SearchSettings::default();
        settings.op_prefs.record(RosterField::Damage.name(), Op::Le);
        let encoded = serde_json::to_string(&settings).expect("serialises");
        let decoded: SearchSettings = serde_json::from_str(&encoded).expect("deserialises");
        assert_eq!(
            decoded.op_prefs.preferred(RosterField::Damage.name(), RosterField::Damage.allowed_ops()),
            Some(Op::Le)
        );
        assert!(SearchSettings::default().op_prefs.is_empty(), "a fresh install remembers nothing");
    }

    /// A settings file naming an operator this build does not know must not
    /// cost the user everything else in it.
    #[test]
    fn a_stored_operator_this_build_does_not_know_leaves_the_rest_intact() {
        use wows_toolkit_config::index::query_ast::Op;
        use wows_toolkit_config::index::query_ast::RosterField;

        let settings: SearchSettings = serde_json::from_str(
            r#"{"query":"outcome=win","sort":["Date","Descending"],"op_prefs":{"damage":"between","kills":"le"}}"#,
        )
        .expect("the file must still load");
        assert_eq!(settings.query, "outcome=win");
        assert_eq!(settings.op_prefs.preferred(RosterField::Damage.name(), RosterField::Damage.allowed_ops()), None);
        assert_eq!(
            settings.op_prefs.preferred(RosterField::Kills.name(), RosterField::Kills.allowed_ops()),
            Some(Op::Le),
            "the entry beside the unknown one must survive"
        );
    }
}
