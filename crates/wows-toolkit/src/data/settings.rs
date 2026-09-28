use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_ast::OperatorPreferences;

use crate::data::session_stats::DivisionFilter;
use crate::hardening::CodeIntegrityPreference;
use crate::twitch::Token;

pub use wows_toolkit_config::ReplayGrouping;
pub use wows_toolkit_config::ReplaySettings;

pub const fn default_bool<const V: bool>() -> bool {
    V
}

/// The persisted form of the renderer's options, shared with the renderer
/// crate so both front ends store the same row.
pub use wows_minimap_renderer::SavedRenderOptions;

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
    /// Whether to load only Microsoft-signed code into this process. Read at
    /// startup, before the app exists, because the policy cannot be applied or
    /// lifted once a window is up.
    pub code_integrity: CodeIntegrityPreference,
    /// Manual proxy override (`host:port` or a full URL). Takes precedence over
    /// environment variables and the Windows proxy configuration. `None` means
    /// auto-detect; applied once at startup.
    pub proxy_url: Option<String>,
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
            code_integrity: CodeIntegrityPreference::default(),
            proxy_url: None,
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

/// Which theme the app renders in. Shared with the GPUI port, which reads the
/// same stored value.
pub use wows_toolkit_viewmodel::settings::ThemeChoice;

/// What egui calls the same choice. A free function rather than a `From`
/// impl: both types are foreign to this crate now that the choice is shared.
pub fn egui_theme_preference(choice: ThemeChoice) -> egui::ThemePreference {
    match choice {
        ThemeChoice::System => egui::ThemePreference::System,
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
    }
}

/// How much battle data the reader has agreed to share, shared with the port so
/// both apps read the row the same way and share under the same rules
/// (`wows_toolkit_viewmodel::upload`).
pub use wows_toolkit_viewmodel::settings::DataSharingMode;

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
        assert_eq!(egui_theme_preference(ThemeChoice::System), egui::ThemePreference::System);
        assert_eq!(egui_theme_preference(ThemeChoice::Dark), egui::ThemePreference::Dark);
        assert_eq!(egui_theme_preference(ThemeChoice::Light), egui::ThemePreference::Light);
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
