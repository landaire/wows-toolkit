//! The Search tab's native half: running a query over the shared replay
//! index, the values a caret field offers, and the saved bar state.

mod bar;
mod palette;

pub use bar::QueryBar;
pub use palette::Palette;
pub use palette::ShipSearch;

use std::path::PathBuf;
use std::sync::Mutex;

use nsh_core::JsonValue;
use nsh_core::prelude::*;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_sql::CompileCtx;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::queries::SettingError;
use wows_toolkit_viewmodel::listing_row;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::query_bar::pills;
use wows_toolkit_viewmodel::query_bar::suggest;
use wows_toolkit_viewmodel::search;
use wows_toolkit_viewmodel::search_results;
use wows_toolkit_viewmodel::search_results::RESULT_LIMIT;
use wows_toolkit_viewmodel::search_results::ValueLookupError;
use wows_toolkit_viewmodel::search_settings;
use wows_toolkit_viewmodel::search_settings::COLUMN_WIDTHS_KEY;
use wows_toolkit_viewmodel::search_settings::SEARCH_SETTINGS_KEY;
use wows_toolkit_viewmodel::search_settings::SavedSearch;
use wows_toolkit_viewmodel::search_settings::SavedSearchError;
use wows_toolkit_viewmodel::search_settings::SearchField;
use wows_toolkit_viewmodel::translate;

use crate::runtime::RuntimeError;
use bar::CompletionView;

/// Serializes read-modify-writes of the saved row, so two fields saved at
/// once do not drop each other.
static SETTINGS_WRITE: Mutex<()> = Mutex::new(());

/// A results column the index can order by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum SortKey {
    Date,
    Map,
    Mode,
    Outcome,
    Damage,
    Kills,
    Pr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// The order a search returns its rows in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SortState {
    pub column: SortKey,
    pub direction: SortOrder,
}

/// A column the results table draws, in the order it draws them.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct ResultColumn {
    pub key: String,
    pub title: String,
    /// What a press on its header sorts by; Ship carries none, since the
    /// index has nothing to order it by.
    pub sort: Option<SortKey>,
    pub default_width: f64,
}

/// How a battle ended, which the outcome cell is tinted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum OutcomeTone {
    Win,
    Loss,
    Draw,
    Unknown,
}

/// A color for each appearance, `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct Tone {
    pub dark: String,
    pub light: String,
}

/// One labelled fact a hovered result shows.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct HoverFact {
    pub label: String,
    pub value: String,
}

/// One result row.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct SearchRow {
    pub replay_path: String,
    /// Whether the replay is still on disk; one that is not cannot be opened.
    pub exists: bool,
    pub date: String,
    pub map: String,
    pub mode: String,
    pub ship: String,
    pub outcome: String,
    pub damage: String,
    pub kills: String,
    pub pr: String,
    pub outcome_tone: OutcomeTone,
    /// The rating's band color, when the battle has a rating.
    pub pr_tone: Option<Tone>,
    pub facts: Vec<HoverFact>,
}

/// One page of results.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct SearchPage {
    pub rows: Vec<SearchRow>,
    /// More matches than the page holds.
    pub truncated: bool,
    /// The footer's count.
    pub count: String,
    /// Indexed matches with no game mode cannot match a query on one; said
    /// when this query filters on it.
    pub game_mode_gap: Option<String>,
}

/// What the saved row holds.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct SavedSearchState {
    pub query: String,
    pub history: Vec<String>,
    pub sort: SortState,
    pub operator_preferences: JsonValue,
}

/// The Search tab's words, in the shared catalogue's current language.
/// `%{reason}` in the templates is filled by the caller.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SearchText {
    pub type_a_query: String,
    pub searching: String,
    pub no_matches: String,
    pub looking_up_values: String,
    pub parse_failed: String,
    pub failed: String,
    pub open: String,
    pub open_missing: String,
    pub row_actions_hint: String,
    pub render_replay: String,
    pub render_to_clipboard: String,
    pub copy_replay: String,
    pub copy_path: String,
    pub path_copied: String,
    pub show_in_explorer: String,
    pub retry: String,
    pub undo: String,
    pub redo: String,
    pub pill_select: String,
    pub pill_deselect: String,
    pub pill_group_all: String,
    pub pill_group_any: String,
    pub pill_ungroup: String,
    pub pill_negate: String,
    pub pill_flip_connector: String,
    pub pill_delete: String,
}

/// Why a search or a saved-state read or write failed.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("{source}")]
    Runtime { source: RuntimeError },
    #[error("could not copy the replay to the clipboard: {source}")]
    Clipboard { source: arboard::Error },
    #[error("the {task} task did not complete: {source}")]
    Task { task: &'static str, source: tokio::task::JoinError },
    #[error("the replay index is not open: {source}")]
    Open { source: sqlx::Error },
    #[error("the query did not parse: {reason}")]
    Parse { reason: String },
    #[error("{source}")]
    Search { source: IndexError },
    #[error("{source}")]
    Lookup { source: ValueLookupError },
    #[error("Could not read Search settings: {source}")]
    ReadSettings { source: SettingError },
    #[error("{source}")]
    Saved { source: SavedSearchError },
    #[error("Search {field} could not be saved: {source}")]
    WriteSettings { field: &'static str, source: sqlx::Error },
    #[error("Search {field} could not be written: {source}")]
    Encode { field: &'static str, source: serde_json::Error },
}

/// Searches the replay index and keeps the Search tab's saved state.
#[module]
pub struct ReplaySearch;

#[module]
impl ReplaySearch {
    /// The tab's words.
    pub fn text() -> SearchText {
        SearchText {
            type_a_query: translate("ui.search.type_a_query"),
            searching: translate("ui.search.searching"),
            no_matches: translate("ui.search.no_matches"),
            looking_up_values: translate("ui.search.looking_up_values"),
            parse_failed: translate("ui.search.parse_failed"),
            failed: translate("ui.search.failed"),
            open: translate("ui.search.open"),
            open_missing: translate("ui.search.open_missing"),
            row_actions_hint: translate("ui.search.row_actions_hint"),
            render_replay: translate("ui.replay.context.render_replay"),
            render_to_clipboard: translate("ui.replay.context.render_to_clipboard"),
            copy_replay: translate("ui.replay.context.copy_replay"),
            copy_path: translate("ui.replay.context.copy_path"),
            path_copied: translate("ui.search.path_copied"),
            show_in_explorer: translate("ui.replay.context.show_in_explorer"),
            retry: translate("ui.buttons.retry"),
            undo: translate("ui.search.undo"),
            redo: translate("ui.search.redo"),
            pill_select: translate("ui.search.pill_select"),
            pill_deselect: translate("ui.search.pill_deselect"),
            pill_group_all: translate("ui.search.pill_group_all"),
            pill_group_any: translate("ui.search.pill_group_any"),
            pill_ungroup: translate("ui.search.pill_ungroup"),
            pill_negate: translate("ui.search.pill_negate"),
            pill_flip_connector: translate("ui.search.pill_flip_connector"),
            pill_delete: translate("ui.search.pill_delete"),
        }
    }

    /// The columns the results table draws, in order.
    pub fn columns() -> Vec<ResultColumn> {
        let mut columns = Vec::with_capacity(SortColumn::ALL.len() + 1);
        for column in SortColumn::ALL {
            columns.push(ResultColumn {
                key: search_settings::sort_column_name(column).to_ascii_lowercase(),
                title: column.label().to_owned(),
                sort: Some(sort_key(column)),
                default_width: default_width(column),
            });
            // Ship sits after Mode, as the egui table lists it.
            if column == SortColumn::Mode {
                columns.push(ResultColumn {
                    key: "ship".to_owned(),
                    title: "Ship".to_owned(),
                    sort: None,
                    default_width: 150.0,
                });
            }
        }

        columns
    }

    /// The sort the results open in: newest first.
    pub fn default_sort() -> SortState {
        sort_state(SortSpec::default())
    }

    /// The sort a press on `column`'s header makes of `current`: another
    /// column starts at its natural direction, a second press reverses it,
    /// a third puts the opening order back.
    pub fn sort_after_press(current: SortState, column: SortKey) -> SortState {
        sort_state(sort_spec(current).after_click(sort_column(column)))
    }

    /// Searches the index for `query`, ordered by `sort`.
    pub async fn search(query: String, sort: SortState) -> Result<SearchPage, String> {
        blocking(move || run_search(&query, sort_spec(sort)))
            .await
            .map_err(|source| SearchError::Task { task: "search", source }.to_string())?
            .map_err(|error| error.to_string())
    }

    /// The values the caret field in `pending` takes, as completions of it.
    pub async fn lookup_values(pending: String) -> Result<Vec<CompletionView>, String> {
        blocking(move || look_up_values(&pending))
            .await
            .map_err(|source| SearchError::Task { task: "value lookup", source }.to_string())?
            .map_err(|error| error.to_string())
    }

    /// Reads the saved row.
    pub async fn load_settings() -> Result<SavedSearchState, String> {
        blocking(load_settings)
            .await
            .map_err(|source| SearchError::Task { task: "settings read", source }.to_string())?
            .map_err(|error| error.to_string())
    }

    pub async fn save_query(query: String) -> Result<(), String> {
        save_field(SearchField::Query, JsonValue::String(query)).await
    }

    pub async fn save_history(history: Vec<String>) -> Result<(), String> {
        save_field(SearchField::History, serde_json::json!(history)).await
    }

    pub async fn save_sort(sort: SortState) -> Result<(), String> {
        save_field(SearchField::Sort, search_settings::sort_value(sort_spec(sort))).await
    }

    pub async fn save_operator_preferences(preferences: JsonValue) -> Result<(), String> {
        save_field(SearchField::OperatorPreferences, preferences).await
    }

    /// The widths the reader dragged each column to, in [`Self::columns`]
    /// order; `null` for a column left at its default.
    pub async fn load_column_widths() -> Result<Vec<Option<f64>>, String> {
        blocking(load_column_widths)
            .await
            .map_err(|source| SearchError::Task { task: "column width read", source }.to_string())?
            .map_err(|error| error.to_string())
    }

    pub async fn save_column_widths(widths: Vec<Option<f64>>) -> Result<(), String> {
        blocking(move || save_column_widths(&widths))
            .await
            .map_err(|source| SearchError::Task { task: "column width write", source }.to_string())?
            .map_err(|error| error.to_string())
    }

    /// Shows the replay selected in the file manager.
    pub async fn reveal_replay(path: String) -> Result<(), String> {
        reveal_path(PathBuf::from(path)).await
    }

    /// Puts the replay at `path` on the clipboard as a file, so a file
    /// manager or chat client pastes the replay itself.
    pub async fn copy_replay(path: String) -> Result<(), String> {
        blocking(move || copy_replay_file(PathBuf::from(path)))
            .await
            .map_err(|source| SearchError::Task { task: "replay copy", source }.to_string())?
            .map_err(|error| error.to_string())
    }
}

fn copy_replay_file(path: PathBuf) -> Result<(), SearchError> {
    crate::clipboard::copy_files(&[path]).map_err(|source| SearchError::Clipboard { source })
}

fn run_search(text: &str, sort: SortSpec) -> Result<SearchPage, SearchError> {
    let expr = query_text::parse_query(text).map_err(|error| SearchError::Parse { reason: error.kind.to_string() })?;
    let runtime = crate::runtime::shared().map_err(|source| SearchError::Runtime { source })?;

    let (mut hits, missing) = runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| SearchError::Open { source })?;
        // The query fetches one row past the limit itself, so a set of
        // exactly the limit reads as a complete answer.
        let hits = query::search_by_ast(&pool, &expr, &CompileCtx::default(), RESULT_LIMIT, sort)
            .await
            .map_err(|source| SearchError::Search { source })?;
        // Counted only for a query that filters on the mode: it is a scan,
        // and the hint is about nothing else.
        let missing = if search::references_game_mode(&expr) {
            match query::matches_missing_game_mode_count(&pool).await {
                Ok(count) => Some(count),
                Err(error) => {
                    // The results still stand; only the hint is missing.
                    log::warn!("search: the game-mode gap lookup failed: {error}");
                    None
                }
            }
        } else {
            None
        };

        Ok::<_, SearchError>((hits, missing))
    })?;

    let truncated = hits.len() as i64 > RESULT_LIMIT;
    hits.truncate(RESULT_LIMIT as usize);
    let count = search_results::match_count(hits.len(), truncated);
    let game_mode_gap = missing
        .filter(|missing| search::game_mode_gap_applies(*missing, &expr))
        .map(search_results::game_mode_gap_hint);

    Ok(SearchPage { rows: hits.iter().map(search_row).collect(), truncated, count, game_mode_gap })
}

fn search_row(hit: &MatchHit) -> SearchRow {
    // The name stored at index time: a live name needs the match's own build
    // loaded, which is the game data service's to provide.
    let ship = search::ship_display_name(hit, None).unwrap_or_else(|| "-".to_owned());
    let facts = listing_row::hover_facts_for_match(hit, ship.clone(), None)
        .into_iter()
        .map(|fact| HoverFact { label: fact.label, value: fact.value })
        .collect();
    let pr_tone = hit.self_pr.map(|pr| {
        let category = PersonalRatingCategory::from_pr(pr);
        Tone {
            dark: hex(personal_rating::chip_text(category, true)),
            light: hex(personal_rating::chip_text(category, false)),
        }
    });

    SearchRow {
        replay_path: hit.replay_path.to_string_lossy().into_owned(),
        exists: hit.replay_path.is_file(),
        date: search_results::cell_text(hit, SortColumn::Date),
        map: search_results::cell_text(hit, SortColumn::Map),
        mode: search_results::cell_text(hit, SortColumn::Mode),
        ship,
        outcome: search_results::cell_text(hit, SortColumn::Outcome),
        damage: search_results::cell_text(hit, SortColumn::Damage),
        kills: search_results::cell_text(hit, SortColumn::Kills),
        pr: search_results::cell_text(hit, SortColumn::Pr),
        outcome_tone: match hit.outcome {
            MatchOutcome::Win => OutcomeTone::Win,
            MatchOutcome::Loss => OutcomeTone::Loss,
            MatchOutcome::Draw => OutcomeTone::Draw,
            MatchOutcome::Unknown => OutcomeTone::Unknown,
        },
        pr_tone,
        facts,
    }
}

fn look_up_values(pending: &str) -> Result<Vec<CompletionView>, SearchError> {
    let Some(request) = suggest::value_request_for(pending) else {
        return Ok(Vec::new());
    };
    let runtime = crate::runtime::shared().map_err(|source| SearchError::Runtime { source })?;

    let options = runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| SearchError::Open { source })?;
        search_results::look_up_values(&pool, &request).await.map_err(|source| SearchError::Lookup { source })
    })?;

    Ok(pills::value_completions(pending, &options)
        .into_iter()
        .map(|completion| CompletionView {
            label: completion.label,
            context: completion.context,
            replacement: completion.replacement,
        })
        .collect())
}

fn load_settings() -> Result<SavedSearchState, SearchError> {
    let runtime = crate::runtime::shared().map_err(|source| SearchError::Runtime { source })?;
    let stored = runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| SearchError::Open { source })?;
        wows_toolkit_config::queries::try_get_setting::<JsonValue>(&pool, SEARCH_SETTINGS_KEY)
            .await
            .map_err(|source| SearchError::ReadSettings { source })
    })?;
    let saved = SavedSearch::parse(stored).map_err(|source| SearchError::Saved { source })?;
    let operator_preferences = serde_json::to_value(&saved.operator_preferences)
        .map_err(|source| SearchError::Encode { field: SearchField::OperatorPreferences.key(), source })?;

    Ok(SavedSearchState {
        query: saved.query,
        history: saved.history,
        sort: sort_state(saved.sort),
        operator_preferences,
    })
}

async fn save_field(field: SearchField, value: JsonValue) -> Result<(), String> {
    blocking(move || write_field(field, value))
        .await
        .map_err(|source| SearchError::Task { task: "settings write", source }.to_string())?
        .map_err(|error| error.to_string())
}

fn write_field(field: SearchField, value: JsonValue) -> Result<(), SearchError> {
    let _guard = SETTINGS_WRITE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let runtime = crate::runtime::shared().map_err(|source| SearchError::Runtime { source })?;

    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| SearchError::Open { source })?;
        let stored = wows_toolkit_config::queries::try_get_setting::<JsonValue>(&pool, SEARCH_SETTINGS_KEY)
            .await
            .map_err(|source| SearchError::ReadSettings { source })?;
        let stored = search_settings::with_field(stored, field, value).map_err(|source| SearchError::Saved { source })?;
        wows_toolkit_config::queries::set_setting_raw(&pool, SEARCH_SETTINGS_KEY, &stored.to_string())
            .await
            .map_err(|source| SearchError::WriteSettings { field: field.key(), source })
    })
}

fn load_column_widths() -> Result<Vec<Option<f64>>, SearchError> {
    let runtime = crate::runtime::shared().map_err(|source| SearchError::Runtime { source })?;
    let stored = runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| SearchError::Open { source })?;
        wows_toolkit_config::queries::try_get_setting::<Vec<Option<f32>>>(&pool, COLUMN_WIDTHS_KEY)
            .await
            .map_err(|source| SearchError::ReadSettings { source })
    })?;

    // Nothing saved is every column at its default.
    Ok(stored.unwrap_or_default().into_iter().map(|width| width.map(f64::from)).collect())
}

fn save_column_widths(widths: &[Option<f64>]) -> Result<(), SearchError> {
    let widths: Vec<Option<f32>> = widths.iter().map(|width| width.map(|width| width as f32)).collect();
    let runtime = crate::runtime::shared().map_err(|source| SearchError::Runtime { source })?;

    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| SearchError::Open { source })?;
        wows_toolkit_config::queries::set_setting(&pool, COLUMN_WIDTHS_KEY, &widths)
            .await
            .map_err(|source| SearchError::WriteSettings { field: "column widths", source })
    })
}

fn default_width(column: SortColumn) -> f64 {
    match column {
        SortColumn::Date => 160.0,
        SortColumn::Map => 150.0,
        SortColumn::Mode => 130.0,
        SortColumn::Outcome => 90.0,
        SortColumn::Damage => 100.0,
        SortColumn::Kills => 70.0,
        SortColumn::Pr => 80.0,
    }
}

fn sort_key(column: SortColumn) -> SortKey {
    match column {
        SortColumn::Date => SortKey::Date,
        SortColumn::Map => SortKey::Map,
        SortColumn::Mode => SortKey::Mode,
        SortColumn::Outcome => SortKey::Outcome,
        SortColumn::Damage => SortKey::Damage,
        SortColumn::Kills => SortKey::Kills,
        SortColumn::Pr => SortKey::Pr,
    }
}

fn sort_column(key: SortKey) -> SortColumn {
    match key {
        SortKey::Date => SortColumn::Date,
        SortKey::Map => SortColumn::Map,
        SortKey::Mode => SortColumn::Mode,
        SortKey::Outcome => SortColumn::Outcome,
        SortKey::Damage => SortColumn::Damage,
        SortKey::Kills => SortColumn::Kills,
        SortKey::Pr => SortColumn::Pr,
    }
}

fn sort_state(sort: SortSpec) -> SortState {
    SortState {
        column: sort_key(sort.column),
        direction: match sort.direction {
            SortDirection::Ascending => SortOrder::Ascending,
            SortDirection::Descending => SortOrder::Descending,
        },
    }
}

fn sort_spec(sort: SortState) -> SortSpec {
    SortSpec {
        column: sort_column(sort.column),
        direction: match sort.direction {
            SortOrder::Ascending => SortDirection::Ascending,
            SortOrder::Descending => SortDirection::Descending,
        },
    }
}

fn hex(rgb: u32) -> String {
    format!("#{rgb:06x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_press_cycles_its_column() {
        let start = ReplaySearch::default_sort();
        assert_eq!(start, SortState { column: SortKey::Date, direction: SortOrder::Descending });

        let map = ReplaySearch::sort_after_press(start, SortKey::Map);
        assert_eq!(map, SortState { column: SortKey::Map, direction: SortOrder::Ascending });
        let reversed = ReplaySearch::sort_after_press(map, SortKey::Map);
        assert_eq!(reversed.direction, SortOrder::Descending);
        assert_eq!(ReplaySearch::sort_after_press(reversed, SortKey::Map), start, "a third press resets");
    }

    #[test]
    fn the_columns_follow_the_egui_order_with_ship_after_mode() {
        let keys: Vec<String> = ReplaySearch::columns().into_iter().map(|column| column.key).collect();

        assert_eq!(keys, ["date", "map", "mode", "ship", "outcome", "damage", "kills", "pr"]);
    }

    #[test]
    fn every_sort_converts_both_ways() {
        for column in SortColumn::ALL {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                let spec = SortSpec { column, direction };
                assert_eq!(sort_spec(sort_state(spec)), spec);
            }
        }
    }
}
