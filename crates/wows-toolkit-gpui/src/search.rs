//! The Search tab: a query over the replay index, and its results.
//!
//! Mirrors the egui tab's shape -- a query bar above a sortable results table
//! -- over the same query language. Parsing the query, compiling it to SQL and
//! deciding what a header click sorts by all live in the shared index, so this
//! is the rendering and the wiring.

use std::path::PathBuf;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::calendar::Calendar;
use gpui_kit::component::calendar::CalendarEvent;
use gpui_kit::component::calendar::CalendarState;
use gpui_kit::component::calendar::Date as CalendarDate;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use crate::search_pills::StructuralEdit;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_ast::Expr;
use wows_toolkit_config::index::query_ast::MatchExpr;
use wows_toolkit_config::index::query_ast::OperatorPreferences;
use wows_toolkit_config::index::query_sql::CompileCtx;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_viewmodel::query_bar::select;
use wows_toolkit_viewmodel::query_bar::select::Selection;
use wows_toolkit_viewmodel::query_bar::suggest;
use wows_toolkit_viewmodel::query_bar::suggest::ValueOption;
use wows_toolkit_viewmodel::query_bar::suggest::ValueRequest;
use wows_toolkit_viewmodel::query_bar::tokens::NodePath;

use std::collections::HashMap;

use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::search as search_hint;
use wows_toolkit_viewmodel::search as ship_display;
use wows_toolkit_viewmodel::search::ship_display_name;

use crate::replay_inspector::GameDataCache;
use crate::runtime;
use crate::ui::selectable;

/// A column the results table draws.
///
/// The sortable ones are the shared `SortColumn`; Ship is drawn beside them
/// and carries no sort, since the index has nothing to order it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ResultColumn {
    Sortable(SortColumn),
    Ship,
}

impl ResultColumn {
    /// The order the egui table lists them in: Ship sits after Mode.
    fn all() -> Vec<ResultColumn> {
        let mut columns = Vec::with_capacity(SortColumn::ALL.len() + 1);
        for column in SortColumn::ALL {
            columns.push(ResultColumn::Sortable(column));
            if column == SortColumn::Mode {
                columns.push(ResultColumn::Ship);
            }
        }
        columns
    }

    fn label(self) -> &'static str {
        match self {
            Self::Sortable(column) => column.label(),
            Self::Ship => "Ship",
        }
    }

    /// What the column is drawn at until the reader drags it.
    fn default_width(self) -> Pixels {
        match self {
            Self::Sortable(column) => column_width(column),
            Self::Ship => px(150.),
        }
    }
}

/// The path of `path`'s parent. The root's parent is the root.
fn parent_of(path: &[usize]) -> NodePath {
    path.iter().copied().take(path.len().saturating_sub(1)).collect()
}

/// How many structural edits can be stepped back through. Deep enough to
/// cover a session's worth of reshaping, bounded so a long one does not grow
/// without limit.
const UNDO_DEPTH: usize = 64;

/// A column as the current frame lays it out.
#[derive(Clone, Copy)]
struct DrawnColumn {
    column: ResultColumn,
    width: Pixels,
}

/// A column-width drag in progress.
#[derive(Clone, Copy)]
struct ColumnDrag {
    column: ResultColumn,
    pointer_start: Pixels,
    width_start: Pixels,
}

/// The narrowest a dragged column may be made. Below this a column's own
/// content is unreadable and its header is gone.
const COLUMN_DRAG_MIN: Pixels = px(28.);

/// The width of the strip on a header's trailing edge that starts a drag.
const RESIZE_GRIP: Pixels = px(6.);

/// The settings row the dragged column widths are kept in.
///
/// The port's own row: the egui results table has fixed widths and nothing to
/// share.
const COLUMN_WIDTHS_KEY: &str = "search_column_widths";

/// Raised for the app to act on.
#[derive(Clone, Debug)]
pub enum SearchEvent {
    /// Open this replay in the Replay Inspector.
    OpenReplay(std::path::PathBuf),
    /// Play this replay's battle back on its minimap.
    RenderReplay(std::path::PathBuf),
}

impl EventEmitter<SearchEvent> for SearchView {}

/// Whether replacing the box's text is something the reader may want
/// completions for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Offer {
    /// A fragment was taken and may be continued, so the next render offers
    /// what can follow it.
    WhatFollows,
    /// The text is an answer rather than a fragment: a committed query, a step
    /// through the history, a picked date.
    Nothing,
}

/// The settings row both front ends keep the query bar's state in.
const SEARCH_SETTINGS_KEY: &str = "search";

/// Results asked for per run. The egui table pages the same way rather than
/// pulling an unbounded set into memory.
const RESULT_LIMIT: i64 = 500;

/// The preview's edge length. Square, as the minimap is, and the same figure the
/// listing's own popup draws at so a hovered battle looks the same on either.
const PREVIEW_WIDTH: f32 = 384.;

/// How far to the right of the pointer the popup sits, so it does not cover the
/// row being read. The listing's own offset.
const PREVIEW_CURSOR_OFFSET: Pixels = px(24.);

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
/// The dots at the end of each row, which is what the row reserves in place of
/// the three buttons it used to carry: only the row being pointed at can have
/// its actions used, so only it shows them.
const ACTIONS_COLUMN_WIDTH: Pixels = px(28.);

/// Queries the bar remembers. Older ones fall off the end rather than the
/// row growing without bound.
const HISTORY_DEPTH: usize = 50;

/// How long a typed edit waits before the results follow it.
const RERUN_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// How a taken day is written into the query. The grammar reads a bare
/// `YYYY-MM-DD` as local midnight on that day.
const CALENDAR_DATE_FORMAT: &str = "%Y-%m-%d";

/// Rows a value lookup offers, matching the egui bar's own limit.
const VALUE_LIMIT: i64 = 50;
/// How tall the completions dropdown grows before it scrolls, matching the
/// egui bar's own cap, and the width it is kept between so it neither reads
/// as a strip nor spans the window.
const COMPLETIONS_MAX_HEIGHT: f32 = 260.0;
const COMPLETIONS_MIN_WIDTH: f32 = 260.0;
const COMPLETIONS_MAX_WIDTH: f32 = 460.0;
/// How long the caret sits still before its value lookup is sent.
const VALUE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);

fn sort_from_value(value: &serde_json::Value) -> Option<SortSpec> {
    let pair = value.as_array()?;
    if pair.len() != 2 {
        return None;
    }
    let column = match pair.first()?.as_str()? {
        "Date" => SortColumn::Date,
        "Map" => SortColumn::Map,
        "Mode" => SortColumn::Mode,
        "Outcome" => SortColumn::Outcome,
        "Damage" => SortColumn::Damage,
        "Kills" => SortColumn::Kills,
        "Pr" => SortColumn::Pr,
        _ => return None,
    };
    let direction = match pair.get(1)?.as_str()? {
        "Ascending" => SortDirection::Ascending,
        "Descending" => SortDirection::Descending,
        _ => return None,
    };
    Some(SortSpec { column, direction })
}

fn sort_value(sort: SortSpec) -> serde_json::Value {
    let column = match sort.column {
        SortColumn::Date => "Date",
        SortColumn::Map => "Map",
        SortColumn::Mode => "Mode",
        SortColumn::Outcome => "Outcome",
        SortColumn::Damage => "Damage",
        SortColumn::Kills => "Kills",
        SortColumn::Pr => "Pr",
    };
    let direction = match sort.direction {
        SortDirection::Ascending => "Ascending",
        SortDirection::Descending => "Descending",
    };
    serde_json::json!([column, direction])
}

fn parse_saved_settings(
    stored: Option<serde_json::Value>,
) -> Result<(String, Vec<String>, SortSpec, OperatorPreferences), String> {
    let Some(stored) = stored else {
        return Ok((String::new(), Vec::new(), SortSpec::default(), OperatorPreferences::default()));
    };
    let object = stored.as_object().ok_or_else(|| "Search settings must be a JSON object".to_string())?;
    let query = match object.get("query") {
        None => String::new(),
        Some(value) => value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "Search settings field 'query' must be a string".to_string())?,
    };
    let history = match object.get("history") {
        None => Vec::new(),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|error| format!("Search settings field 'history' is invalid: {error}"))?,
    };
    let sort = match object.get("sort") {
        None => SortSpec::default(),
        Some(value) => sort_from_value(value).ok_or_else(|| "Search settings field 'sort' is invalid".to_string())?,
    };
    let operator_preferences = match object.get("op_prefs") {
        None => OperatorPreferences::default(),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|error| format!("Search settings field 'op_prefs' is invalid: {error}"))?,
    };
    Ok((query, history, sort, operator_preferences))
}

/// The values `request` asks the index for.
///
/// Ships and players are searched by what has been typed so far; maps and
/// sources are whole catalogues, which are small enough to offer entire.
async fn look_up_values(pool: &sqlx::SqlitePool, request: &ValueRequest) -> Vec<ValueOption> {
    match request {
        ValueRequest::Players { needle } => match query::search_players(pool, needle, VALUE_LIMIT).await {
            Ok(rows) => rows
                .iter()
                .map(|facet| ValueOption {
                    label: if facet.clan.is_empty() {
                        facet.latest_name.clone()
                    } else {
                        format!("[{}] {}", facet.clan, facet.latest_name)
                    },
                    token: facet.account_id.raw().to_string(),
                })
                .collect(),
            Err(err) => {
                tracing::warn!("search: the player lookup failed: {err}");
                Vec::new()
            }
        },
        ValueRequest::Ships { needle } => match query::search_ships(pool, needle, VALUE_LIMIT).await {
            Ok(rows) => rows
                .iter()
                .map(|facet| ValueOption { label: facet.ship_name.clone(), token: facet.ship_id.raw().to_string() })
                .collect(),
            Err(err) => {
                tracing::warn!("search: the ship lookup failed: {err}");
                Vec::new()
            }
        },
        ValueRequest::Maps => match query::distinct_maps(pool, VALUE_LIMIT).await {
            Ok(names) => names
                .iter()
                .map(|name| ValueOption { label: name.clone(), token: query_text::quote_if_needed(name) })
                .collect(),
            Err(err) => {
                tracing::warn!("search: the map lookup failed: {err}");
                Vec::new()
            }
        },
        ValueRequest::Sources => match query::list_sources(pool).await {
            Ok(sources) => sources
                .iter()
                .map(|source| ValueOption { label: source.name.clone(), token: source.id.0.to_string() })
                .collect(),
            Err(err) => {
                tracing::warn!("search: the source lookup failed: {err}");
                Vec::new()
            }
        },
    }
}

/// Where the tab is in running a query.
enum SearchState {
    /// Nothing asked for yet.
    Idle,
    /// The query text did not parse. Carries what the parser objected to and
    /// the part of the text it objected at, so the bar can point at it.
    Invalid {
        reason: String,
        span: std::ops::Range<usize>,
        text: String,
    },
    Running,
    Failed(String),
    Done,
}

struct ValueEdit {
    path: NodePath,
    leaf: NodePath,
    seed: MatchExpr,
    tail: NodePath,
    kind: wows_toolkit_config::index::query_ast::ValueKind,
}

pub struct SearchView {
    query_input: Entity<InputState>,
    value_input: Entity<InputState>,
    value_edit: Option<ValueEdit>,
    value_error: bool,
    /// Set by Enter, acted on where a window is at hand: turning the typed
    /// term into a pill clears the box, and clearing a box needs one.
    pending_commit: bool,
    /// The part of the query that has been turned into pills. What is in the
    /// box is the rest: the term still being typed.
    ///
    /// The egui bar keeps the same two apart -- a `MatchExpr` it draws as
    /// pills and a `pending` buffer its caret edits -- which is what makes a
    /// finished term leave the text and become a pill rather than being
    /// mirrored under it.
    committed: String,
    /// What the caret fragment may be completed to, and the bar text they
    /// were built from. Refreshed in `render` when the two disagree rather
    /// than from an input event: a completion taken by click changes the text
    /// too, and one path that notices covers both.
    completions: Vec<crate::search_pills::Completion>,
    completion_source: String,
    /// The committed query, which is what the pills read back. Rewritten by
    /// every edit that lands on a pill and by a term the box commits; typing
    /// does not touch it.
    ///
    /// Separate from `expr`, which is the query the results on screen came
    /// from: editing the bar must not relabel results it has not been run
    /// against.
    reading: Option<wows_toolkit_config::index::query_ast::MatchExpr>,
    /// The calendar a date field is picked from. Held rather than built per
    /// render so the month it was paged to survives a keystroke.
    calendar: Entity<CalendarState>,
    /// The value lookup the caret calls for, and the options it returned.
    /// The index is asked for ships and players; maps and sources are read
    /// once and kept.
    value_request: Option<ValueRequest>,
    value_options: Vec<ValueOption>,
    /// Whether the index is still being asked. A whole-index name search is
    /// seconds on an established index, so the bar says it is looking rather
    /// than showing an empty dropdown that reads as "nothing matches".
    value_lookup_running: bool,
    _value_lookup: Option<Task<()>>,
    /// Which row of the dropdown the keyboard is on. `None` while the caret
    /// is being typed at, so Enter runs the query rather than taking whatever
    /// row happened to be first.
    completion_cursor: Option<usize>,
    /// Whether the dropdown is showing. Closed by Escape, by taking a row, by
    /// a pointer press anywhere off it, and by the box losing focus; reopened
    /// by the next edit, so it does not sit over the results after the query
    /// has been committed.
    completions_open: bool,
    /// Where the query input sits, so the dropdown can be anchored under it.
    /// Recorded during layout; `None` before the bar has been drawn once.
    bar_bounds: Option<Bounds<Pixels>>,
    /// Set when Enter took a completion. The input reports the same Enter
    /// through its own event, and without this the query would run on the
    /// text as it was before the row was taken.
    took_completion_on_enter: bool,
    /// The query the last run was for, so an edit that leaves the text alone
    /// (a caret move, a selection) does not re-query.
    last_run_query: String,
    /// The pending re-query behind a typed edit. Dropped whenever another
    /// edit arrives, which is what collapses a burst of keystrokes into one
    /// query.
    _rerun: Option<Task<()>>,
    /// Queries that were run, newest first. Shared with the egui tab, which
    /// declares the same field and walks it with Up.
    history: Vec<String>,
    /// How far back into the history Up has walked, and the text the walk
    /// started from so Down can put it back.
    history_walk: Option<(usize, String)>,
    /// The pills the reader has selected, which is what a group or a delete
    /// acts on. Cleared whenever the query is rewritten, since a path names
    /// a place in a tree that no longer exists.
    selection: Selection,
    /// The fixed end of a multi-pill selection, and the end an arrow moves.
    /// `None` when the caret is in the text rather than on a pill.
    selection_anchor: Option<Vec<usize>>,
    selection_focus: Option<Vec<usize>>,
    /// Queries a structural edit replaced, newest last.
    ///
    /// Only structural edits are recorded: typing in the bar has the text
    /// field's own undo, and pushing every keystroke here would bury the
    /// edits this stack exists for. Bounded, because a long session should
    /// not grow it without limit.
    undo: Vec<String>,
    /// Queries undone, for redoing. Cleared by the next edit, since redoing
    /// past one would restore a query that no longer follows from what is in
    /// the bar.
    redo: Vec<String>,
    /// Names the pills read ids back as. Filled from the same lookups the
    /// result table uses, so a pill and a row name a ship the same way.
    name_cache: wows_toolkit_viewmodel::query_bar::label::NameCache,
    sort: SortSpec,
    operator_preferences: OperatorPreferences,
    settings_write_lock: std::sync::Arc<futures::lock::Mutex<()>>,
    hits: Vec<MatchHit>,
    state: SearchState,
    /// The query the current results came from, so the game-mode hint knows
    /// whether this search filters on one.
    expr: Option<wows_toolkit_config::index::query_ast::MatchExpr>,
    /// Whether each result's replay is still on disk, positionally against
    /// `hits`. Checked once per result set rather than per row per frame: it
    /// is a syscall, and the answer only changes when the file does.
    on_disk: Vec<bool>,
    /// Hover-to-preview: the same behaviour the replay listing has
    /// (`preview_hover`), over the result rows.
    preview: crate::preview_hover::PreviewHover,
    /// Where the pointer was when it entered the row being previewed, which is
    /// what the popup is anchored to.
    preview_anchor: Point<Pixels>,
    /// Game data for resolving a result's ship name in the current locale,
    /// rather than the one it was indexed in. Shared with the replay
    /// inspector, which already holds it.
    game_data: Option<GameDataCache>,
    /// Ship names resolved for the results on screen, keyed by the build they
    /// were resolved against. Resolved once per search rather than per frame:
    /// each name is a provider lookup, and a build load is expensive.
    resolved_ships: HashMap<(u32, GameParamId), String>,
    /// How many indexed matches carry no game mode. `None` until asked.
    game_mode_gap: Option<i64>,
    /// Whether the last run had more matches than it asked for.
    truncated: bool,
    /// Widths the reader dragged a column to. A column not in here is drawn
    /// at its own [`ResultColumn::default_width`].
    column_widths: HashMap<ResultColumn, Pixels>,
    /// Whether the saved widths have been read back yet. One shot, on the
    /// first frame.
    widths_loaded: bool,
    /// The column being dragged and the width it had when the drag started,
    /// so the new width follows the pointer's total travel rather than
    /// accumulating per-frame deltas.
    resizing: Option<ColumnDrag>,
    /// Whether the saved query has been read and run. The tab opens showing
    /// what the query bar was left holding, the way the egui tab does, rather
    /// than an empty page with an instruction on it.
    opened: bool,
    settings_load_error: Option<SharedString>,
    query_settings_dirty: bool,
    history_settings_dirty: bool,
    sort_settings_dirty: bool,
    operator_preferences_dirty: bool,
    settings_save_error: Option<SharedString>,
    settings_failed_write: Option<(&'static str, serde_json::Value)>,
    /// Whether a count is in flight, so concurrent searches do not each start
    /// another scan.
    gap_lookup_running: bool,
    /// Bumped per run so a slower earlier query cannot overwrite a later one.
    generation: u64,
    list_state: ListState,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SearchView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add filter"));
        let subscription = cx.subscribe(&query_input, Self::on_query_event);
        let value_input = cx.new(|cx| InputState::new(window, cx));
        let value_subscription = cx.subscribe_in(&value_input, window, |this, _input, event, window, cx| match event {
            InputEvent::PressEnter { .. } => {
                this.commit_value_edit(window, cx);
                if this.value_edit.is_none() {
                    this.query_input.read(cx).focus_handle(cx).focus(window, cx);
                }
            }
            InputEvent::Blur => this.commit_value_edit(window, cx),
            InputEvent::Change => {
                this.value_error = false;
                cx.notify();
            }
            _ => {}
        });
        let calendar = cx.new(|cx| CalendarState::new(window, cx));
        let calendar_subscription = cx.subscribe_in(&calendar, window, Self::on_calendar_event);

        Self {
            query_input,
            value_input,
            value_edit: None,
            value_error: false,
            committed: String::new(),
            pending_commit: false,
            calendar,
            completions: Vec::new(),
            completion_source: String::new(),
            reading: None,
            value_request: None,
            value_options: Vec::new(),
            value_lookup_running: false,
            _value_lookup: None,
            completion_cursor: None,
            completions_open: false,
            bar_bounds: None,
            took_completion_on_enter: false,
            last_run_query: String::new(),
            _rerun: None,
            column_widths: HashMap::new(),
            widths_loaded: false,
            resizing: None,
            history: Vec::new(),
            history_walk: None,
            selection: Selection::default(),
            selection_anchor: None,
            selection_focus: None,
            undo: Vec::new(),
            redo: Vec::new(),
            name_cache: Default::default(),
            sort: SortSpec::default(),
            operator_preferences: OperatorPreferences::default(),
            settings_write_lock: std::sync::Arc::new(futures::lock::Mutex::new(())),
            hits: Vec::new(),
            state: SearchState::Idle,
            expr: None,
            on_disk: Vec::new(),
            preview: Default::default(),
            preview_anchor: Point::default(),
            game_data: None,
            resolved_ships: HashMap::new(),
            game_mode_gap: None,
            truncated: false,
            opened: false,
            settings_load_error: None,
            query_settings_dirty: false,
            history_settings_dirty: false,
            sort_settings_dirty: false,
            operator_preferences_dirty: false,
            settings_save_error: None,
            settings_failed_write: None,
            gap_lookup_running: false,
            generation: 0,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription, calendar_subscription, value_subscription],
        }
    }

    /// Puts `replacement` in the bar. What may follow it is re-offered by the
    /// next render, which notices the text changed.
    /// A day taken from the calendar: it replaces the half-typed value under
    /// the caret, the way a completion row does.
    fn on_calendar_event(
        &mut self,
        _calendar: &Entity<CalendarState>,
        event: &CalendarEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let CalendarEvent::Selected(CalendarDate::Single(Some(day))) = event else { return };
        let text = self.query_input.read(cx).value().to_string();
        let replacement = suggest::replace_active_value(&text, &day.format(CALENDAR_DATE_FORMAT).to_string());
        self.take_completion(replacement, Offer::Nothing, window, cx);
    }

    /// Applies one of the pill menu's edits to the query the bar is holding.
    ///
    /// The edits themselves are `query_bar::select`'s, shared with the egui
    /// bar; this applies one, writes the query back as text and re-runs it,
    /// which is how every other edit in this bar lands.
    pub(crate) fn apply_structural_edit(
        &mut self,
        path: NodePath,
        edit: StructuralEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if edit == StructuralEdit::ToggleSelected {
            // An arrow step afterwards carries on from the pill just clicked,
            // which is the one the reader is looking at.
            if self.selection.contains(&path) {
                self.selection_anchor = None;
                self.selection_focus = None;
            } else {
                self.selection_anchor = Some(path.clone());
                self.selection_focus = Some(path.clone());
            }
            self.selection.toggle(path);
            cx.notify();
            return;
        }

        let Some(mut expr) = self.reading.clone() else { return };
        // A menu item acts on the selection where there is one and on the
        // pill it was opened from where there is not, so a right-click on an
        // unselected pill still does the obvious thing.
        let target = if self.selection.is_empty() {
            let mut one = Selection::default();
            one.set_one(path.clone());
            one
        } else {
            self.selection.clone()
        };

        match edit {
            StructuralEdit::ToggleSelected => unreachable!("handled above"),
            StructuralEdit::Group { is_or } => select::group(&mut expr, &target, is_or),
            StructuralEdit::Ungroup => {
                select::ungroup(&mut expr, &path);
            }
            StructuralEdit::Negate => select::negate(&mut expr, &path),
            StructuralEdit::Delete => select::delete(&mut expr, &target),
            StructuralEdit::FlipConnector => {
                let is_or = matches!(select::node_at(&expr, &parent_of(&path)), Some(Expr::Any(_)));
                select::set_connector(&mut expr, &path, !is_or);
            }
        }
        select::canonicalise(&mut expr);

        // Recorded before the rewrite lands, so undo restores what was there.
        self.remember_for_undo(cx);
        // Every path the selection held named a place in the tree that has
        // just been rewritten.
        self.clear_pill_selection();
        self.set_query_text(query_text::print_query(&expr), window, cx);
    }

    /// The whole query, for a test reading the bar back: the pills hold
    /// most of it and the box only the term being typed.
    #[cfg(test)]
    pub(crate) fn query_for_test(&self, cx: &App) -> String {
        self.full_query(cx)
    }

    /// The whole query: what the pills hold and what is being typed after
    /// them.
    fn full_query(&self, cx: &App) -> String {
        let pending = self.query_input.read(cx).value();
        let pending = pending.trim();
        match (self.committed.trim(), pending) {
            ("", rest) => rest.to_string(),
            (pills, "") => pills.to_string(),
            (pills, rest) => format!("{pills} {rest}"),
        }
    }

    /// Turns what has been typed into a pill.
    ///
    /// The egui bar does this on Enter rather than as each character lands:
    /// a term half typed is not a filter, and a pill that appeared at
    /// "map:oce" would take the caret away mid-word.
    fn commit_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let whole = self.full_query(cx);
        if query_text::parse_query(&whole).is_err() {
            // Not a query yet, so there is nothing to make a pill of. It
            // stays in the box with its error showing.
            self.run(cx);
            return;
        }
        self.remember_for_undo(cx);
        self.committed = whole;
        self.set_bar_text("", Offer::Nothing, window, cx);
        self.reading = query_text::parse_query(&self.committed).ok();
        self.run(cx);
        cx.notify();
    }

    /// Puts the query as it stands on the undo stack.
    fn remember_for_undo(&mut self, cx: &Context<Self>) {
        let current = self.full_query(cx);
        if self.undo.last() == Some(&current) {
            return;
        }
        self.undo.push(current);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        // A new edit is a new branch; what was undone no longer follows.
        self.redo.clear();
    }

    /// Whether there is anything to step back to, and anything to step
    /// forward to.
    #[cfg(test)]
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Steps back to the query before the last structural edit.
    pub(crate) fn undo_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.undo.pop() else { return };
        self.redo.push(self.full_query(cx));
        // The paths a selection holds name places in the tree being replaced.
        self.clear_pill_selection();
        self.set_query_text(previous, window, cx);
    }

    /// Steps forward again to a query that was undone.
    pub(crate) fn redo_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.redo.pop() else { return };
        self.undo.push(self.full_query(cx));
        self.clear_pill_selection();
        self.set_query_text(next, window, cx);
    }

    /// Replaces the bar's text with `text` and runs it.
    fn set_query_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.query_settings_dirty = true;
        self.restore_query_text(text, window, cx);
        self.run(cx);
        cx.notify();
    }

    fn begin_value_edit(&mut self, path: NodePath, window: &mut Window, cx: &mut Context<Self>) {
        let Some(expr) = self.reading.as_ref() else { return };
        let Some(term_path) = select::segment_path(expr, &path) else { return };
        let Some((field, _, value)) = select::term_at(expr, &term_path) else { return };
        let kind = field.value_kind();
        let literal = query_text::print_value(value);
        let Some((leaf, seed, tail)) = select::leaf_seed(expr, &term_path) else { return };
        self.value_edit = Some(ValueEdit { path, leaf, seed, tail, kind });
        self.value_error = false;
        self.close_completions(cx);
        self.value_input.update(cx, |input, cx| input.set_value(literal, window, cx));
        self.value_input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn commit_value_edit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.value_edit.as_ref() else { return };
        let literal = self.value_input.read(cx).value();
        let Some(value) = query_text::parse_roster_value(edit.kind, &literal) else {
            self.value_error = true;
            cx.notify();
            return;
        };
        let Some(mut expr) = self.reading.clone() else { return };
        if !select::commit_value(&mut expr, &edit.leaf, &edit.seed, &edit.tail, value) {
            self.value_edit = None;
            cx.notify();
            return;
        }
        self.remember_for_undo(cx);
        self.value_edit = None;
        self.value_error = false;
        self.committed = query_text::print_query(&expr);
        self.reading = Some(expr);
        self.query_settings_dirty = true;
        self.run(cx);
        cx.notify();
    }

    fn restore_query_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.value_edit = None;
        self.value_error = false;
        self.reading = query_text::parse_query(&text).ok();
        if self.reading.is_some() {
            self.committed = text;
            self.set_bar_text("", Offer::Nothing, window, cx);
        } else {
            // Invalid saved queries must remain available for correction.
            self.committed.clear();
            self.set_bar_text(&text, Offer::Nothing, window, cx);
        }
    }

    fn take_completion(&mut self, replacement: String, offer: Offer, window: &mut Window, cx: &mut Context<Self>) {
        self.set_bar_text(&replacement, offer, window, cx);
        self.completion_cursor = None;
        if query_text::parse_query(&self.full_query(cx)).is_ok() {
            self.commit_typed(window, cx);
        }
        cx.notify();
    }

    /// Moves the highlight `delta` rows, out of `offered` rows.
    ///
    /// The first press lands on the first row whichever way it went, so Down
    /// opens the list at the top and Up at the bottom.
    fn move_completion_cursor(&mut self, delta: isize, offered: usize, cx: &mut Context<Self>) {
        if offered == 0 {
            return;
        }
        self.completion_cursor = Some(match self.completion_cursor {
            None if delta > 0 => 0,
            None => offered - 1,
            Some(at) => (at as isize + delta).rem_euclid(offered as isize) as usize,
        });
        cx.notify();
    }

    /// Keyboard on the query bar: the dropdown's rows are walked with the
    /// arrows, taken with Enter, and dismissed with Escape. Enter with no row
    /// highlighted runs the query, which is what the bar does with no
    /// dropdown open at all.
    fn on_bar_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.value_edit.is_some() {
            if event.keystroke.key == "escape" {
                self.value_edit = None;
                self.value_error = false;
                self.query_input.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
            }
            return;
        }
        let offered = self.offered_completions().len();
        let modifiers = event.keystroke.modifiers;
        if modifiers.secondary() {
            match event.keystroke.key.as_str() {
                "z" if !modifiers.shift => {
                    self.undo_edit(window, cx);
                    return;
                }
                "y" | "z" => {
                    self.redo_edit(window, cx);
                    return;
                }
                _ => {}
            }
        }
        match event.keystroke.key.as_str() {
            "down" if self.completions_open => self.move_completion_cursor(1, offered, cx),
            "up" if self.completions_open => self.move_completion_cursor(-1, offered, cx),
            // With no dropdown open the arrows walk what was run before,
            // which is the recall the egui bar offers on Up.
            "up" => self.walk_history(1, window, cx),
            "down" => self.walk_history(-1, window, cx),
            "escape" => {
                self.completions_open = false;
                self.completion_cursor = None;
                cx.notify();
            }
            // Nothing left to erase in the box, so the key means the filter
            // before the caret. The egui bar reads it the same way.
            // Only from the start of the text: anywhere else the key belongs to
            // the box, which is what the reader is typing in. The egui bar
            // gates it on the same caret position.
            "left" if self.caret_at_start(cx) => self.step_pill_selection(true, modifiers.shift, cx),
            // And only once a pill is selected, so the key still walks the text
            // when nothing is.
            "right" if self.caret_at_end(cx) && !self.selection.is_empty() => {
                self.step_pill_selection(false, modifiers.shift, cx)
            }
            "delete" if !self.selection.is_empty() => self.delete_at_caret(window, cx),
            "backspace" if self.query_input.read(cx).value().is_empty() => self.delete_at_caret(window, cx),
            "enter" => {
                let Some(at) = self.completion_cursor else { return };
                let Some(taken) = self.offered_completions().get(at).map(|row| row.replacement.clone()) else {
                    return;
                };
                self.took_completion_on_enter = true;
                self.take_completion(taken, Offer::WhatFollows, window, cx);
            }
            _ => {}
        }
    }

    /// Moves the caret onto the pill beside the one it is on, or onto the last
    /// pill when it is still in the text.
    ///
    /// `extend` grows the selection from wherever it was anchored instead of
    /// replacing it, which is how a run of filters is taken in one gesture. The
    /// egui bar steps the same way (`ui/query_bar/mod.rs`'s `step_selection`).
    fn step_pill_selection(&mut self, back: bool, extend: bool, cx: &mut Context<Self>) {
        let Some(expr) = self.reading.clone() else { return };
        let tokens = wows_toolkit_viewmodel::query_bar::tokens::tokenize(&expr, &self.name_cache);
        let paths = select::selectable_paths(&expr, &tokens);
        let from = self.selection_focus.clone();
        let Some(target) = select::step(&paths, from.as_deref(), back) else {
            // Stepping forward off the last pill puts the caret back in the
            // text, which is where the reader was heading.
            if !back {
                self.selection.clear();
                self.selection_anchor = None;
                self.selection_focus = None;
                cx.notify();
            }
            return;
        };

        if extend {
            let anchor = self.selection_anchor.clone().unwrap_or_else(|| target.clone());
            self.selection.set_many(select::range(&paths, &anchor, &target));
            self.selection_anchor = Some(anchor);
        } else {
            self.selection.set_one(target.clone());
            self.selection_anchor = Some(target.clone());
        }
        self.selection_focus = Some(target);
        cx.notify();
    }

    /// Takes the pill before the caret, or the selection where there is one.
    fn delete_at_caret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut expr) = self.reading.clone() else { return };
        if self.selection.is_empty() {
            let tokens = wows_toolkit_viewmodel::query_bar::tokens::tokenize(&expr, &self.name_cache);
            let Some(target) = select::step(&select::pill_paths(&tokens), None, true) else { return };
            if !select::addresses_node(&expr, &target) {
                return;
            }
            let mut one = Selection::default();
            one.set_one(target);
            select::delete(&mut expr, &one);
        } else {
            select::delete(&mut expr, &self.selection);
        }
        select::canonicalise(&mut expr);
        self.remember_for_undo(cx);
        self.clear_pill_selection();
        self.set_query_text(query_text::print_query(&expr), window, cx);
    }

    /// Puts the caret back in the text, with no pill under it.
    fn clear_pill_selection(&mut self) {
        self.selection.clear();
        self.selection_anchor = None;
        self.selection_focus = None;
    }

    /// Whether the text caret sits before the first character.
    fn caret_at_start(&self, cx: &App) -> bool {
        let input = self.query_input.read(cx);
        input.cursor() == 0 && input.selected_range().is_empty()
    }

    /// Whether it sits after the last one.
    fn caret_at_end(&self, cx: &App) -> bool {
        let input = self.query_input.read(cx);
        input.cursor() == input.value().len() && input.selected_range().is_empty()
    }

    /// Puts an older (positive delta) or newer query in the bar.
    ///
    /// Walking past the newest restores the text the walk started from, so
    /// Up then Down leaves the bar as it was found.
    fn walk_history(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.is_empty() {
            return;
        }
        let (at, started_from) = match self.history_walk.take() {
            Some((at, started_from)) => (at as isize + delta, started_from),
            None if delta > 0 => (0, self.full_query(cx)),
            // Down with no walk in progress is not a recall.
            None => return,
        };

        let text = if at < 0 {
            started_from.clone()
        } else {
            match self.history.get(at as usize) {
                Some(entry) => entry.clone(),
                // Past the oldest: stay where the walk already is.
                None => {
                    self.history_walk = Some(((at - delta) as usize, started_from));
                    return;
                }
            }
        };

        self.committed = text;
        // The bar was not typed at, so the dropdown stays shut: the arrows
        // are walking the history, not a list of completions.
        self.set_bar_text("", Offer::Nothing, window, cx);
        self.reading = query_text::parse_query(&self.committed).ok();
        self.history_walk = (at >= 0).then_some((at as usize, started_from));
        self.run(cx);
    }

    /// Records the bar's text as a query that was run, newest first.
    ///
    /// Only an explicit run is remembered: the results following a typed
    /// edit would otherwise fill the history with every prefix of it.
    fn remember_query(&mut self, cx: &mut Context<Self>) {
        let text = self.full_query(cx);
        if text.is_empty() || self.history.first() == Some(&text) {
            return;
        }
        self.history.retain(|entry| entry != &text);
        self.history.insert(0, text);
        self.history.truncate(HISTORY_DEPTH);
        self.history_settings_dirty = true;
        self.save_history(cx);
    }

    /// Writes the history back into the shared row beside the query.
    fn save_history(&self, cx: &mut Context<Self>) {
        self.save_search_value("history", serde_json::json!(self.history), cx);
    }

    fn save_search_value(&self, field: &'static str, value: serde_json::Value, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let write_lock = self.settings_write_lock.clone();
        let retry_value = value.clone();
        cx.spawn(async move |this, cx| {
            let _guard = write_lock.lock().await;
            let written = runtime::spawn(cx, async move {
                let mut stored = match wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(
                    &pool,
                    SEARCH_SETTINGS_KEY,
                )
                .await
                {
                    Ok(Some(value)) => value,
                    Ok(None) => serde_json::json!({}),
                    Err(error) => {
                        return Err(format!("Could not read Search settings before saving {field}: {error}"));
                    }
                };
                let Some(object) = stored.as_object_mut() else {
                    return Err(format!("Search settings are not an object; {field} was not saved"));
                };
                object.insert(field.to_string(), value);
                let json = stored.to_string();
                wows_toolkit_config::queries::set_setting_raw(&pool, SEARCH_SETTINGS_KEY, &json)
                    .await
                    .map_err(|error| format!("Search {field} could not be saved: {error}"))
            })
            .await;
            let message = match written {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(format!("Search settings update for {field} did not complete: {error}")),
            };
            if let Some(message) = &message {
                tracing::warn!("{message}");
            }
            let _ = this.update(cx, |this, cx| {
                this.settings_save_error = message.map(Into::into);
                this.settings_failed_write = this.settings_save_error.as_ref().map(|_| (field, retry_value));
                cx.notify();
            });
        })
        .detach();
    }

    /// What the dropdown is offering: the values the caret's own field takes
    /// once the index has answered, and otherwise the vocabulary a new term
    /// may start with.
    fn offered_completions(&self) -> Vec<crate::search_pills::Completion> {
        if self.value_options.is_empty() {
            return self.completions.clone();
        }
        let text = self.completion_source.clone();
        self.value_options
            .iter()
            .map(|option| crate::search_pills::Completion {
                label: option.label.clone(),
                context: "value".to_string(),
                replacement: suggest::replace_active_value(&text, &option.token),
            })
            .collect()
    }

    /// Puts `query` in the bar as pills and runs it.
    ///
    /// For a caller outside the tab -- the command palette's seeded searches,
    /// which are a query the reader can then edit rather than a fixed result.
    pub(crate) fn run_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_query_text(query.to_string(), window, cx);
    }

    /// Seeds a bake in flight with nothing to show yet. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_baking_preview_for_test(&mut self, path: PathBuf) {
        self.preview.seed_baking_for_test(path);
    }

    /// What the preview is showing, if anything. Test-only.
    #[cfg(test)]
    pub(crate) fn preview_frame_count(&self) -> Option<usize> {
        self.preview.frame_count()
    }

    /// Whether a row is currently being dwelled on. Test-only.
    #[cfg(test)]
    pub(crate) fn is_dwelling(&self) -> bool {
        self.preview.is_watching()
    }

    /// The pointer settled on `path`'s row: after the shared dwell, its
    /// battle plays back beside the pointer.
    pub(crate) fn hover_row(&mut self, path: PathBuf, position: Point<Pixels>, cx: &mut Context<Self>) {
        // Already on this row: the pointer travelling along it is the same
        // battle, and moving the anchor under it would have the popup chase
        // the pointer across the row.
        if self.preview.watched_path() == Some(path.as_path()) {
            return;
        }
        self.preview_anchor = position;
        let cache = self.game_data.clone();
        self.resolve_hovered_ship(&path, cx);
        // The index records a match's map under the name it displays, not the
        // one the art is stored under, so a result has no map to draw ahead of
        // its bake; the bake's own map stands in.
        self.preview.enter(path, None, cache, cx, |panel| &mut panel.preview);
    }

    /// Loads the hovered row's own build, if it is not open, and names its ship
    /// from it.
    ///
    /// [`Self::resolve_ship_names`] deliberately asks only builds already open,
    /// because a result set can span years. One row under the pointer is the
    /// other case: its build is about to be loaded for the preview anyway, so
    /// the name and class icon beside it need not stay on what the index stored.
    fn resolve_hovered_ship(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else { return };
        let Some(hit) = self.hits.iter().find(|hit| hit.replay_path == path) else { return };
        let Some(key) = hit.version_build.zip(hit.self_ship_id) else { return };
        if self.resolved_ships.contains_key(&key) {
            return;
        }

        let (build, ship_id) = key;
        cx.spawn(async move |this, cx| {
            let named = cx
                .background_spawn(async move {
                    let loaded = game_data.get_or_load_build_for(build, None).ok()?;
                    ship_display::try_resolve_ship_name(ship_id, Some(loaded.provider()))
                })
                .await;

            let Some(name) = named else { return };
            let _ = this.update(cx, |this, cx| {
                this.resolved_ships.insert(key, name);
                cx.notify();
            });
        })
        .detach();
    }

    /// The pointer left the results, which is the one thing that ends a
    /// preview: the rows themselves only start them.
    pub(crate) fn leave_rows(&mut self, cx: &mut Context<Self>) {
        self.preview.leave(cx);
    }

    /// Replaces the box's text.
    ///
    /// `Offer::Nothing` also records the new text as what the completions were
    /// last built for, so the next render does not read the change back as
    /// typing. Without it, committing a query empties the box and an empty box
    /// matches every suggestion there is, which puts the whole list up over
    /// the results that were just asked for.
    fn set_bar_text(&mut self, text: &str, offer: Offer, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.to_string();
        self.query_input.update(cx, |state, cx| state.set_value(text.clone(), window, cx));
        if offer == Offer::Nothing {
            self.completions.clear();
            self.completion_source = text;
            self.completion_cursor = None;
            self.completions_open = false;
        }
    }

    /// Puts the dropdown away, wherever the dismissal came from.
    fn close_completions(&mut self, cx: &mut Context<Self>) {
        if !self.completions_open {
            return;
        }
        self.completions_open = false;
        self.completion_cursor = None;
        cx.notify();
    }

    /// Re-reads what the fragment under the caret may be completed to.
    ///
    /// Only the box: the pills come from the committed query, which typing
    /// leaves alone until a term is finished.
    fn refresh_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.query_input.read(cx).value().to_string();
        if text == self.completion_source {
            return;
        }
        self.completions = crate::search_pills::completions(&text);
        self.completion_source = text.clone();
        // Typing moves the caret off whatever row was highlighted, and an
        // edit made in the box is what reopens a dropdown Escape closed.
        self.completion_cursor = None;
        // Only an edit made in the box offers completions. The text also
        // changes when a saved query is opened or the history is walked, and a
        // dropdown over a bar nobody is typing in stands between the reader
        // and the results. Read off the window rather than mirrored from the
        // focus events, which arrive a frame after the focus itself moves.
        self.completions_open = self.query_input.focus_handle(cx).is_focused(window);
        self.refresh_value_options(&text, cx);
    }

    /// Asks for the values the caret's field takes, when it takes any.
    ///
    /// A fragment that names no field, or one whose values are typed rather
    /// than chosen, leaves the static suggestions showing. The lookup waits
    /// out `VALUE_DEBOUNCE` so typing a ship name does not put one query per
    /// keystroke through the index.
    fn refresh_value_options(&mut self, text: &str, cx: &mut Context<Self>) {
        let request = suggest::value_request_for(text);
        if request == self.value_request {
            return;
        }
        self.value_request = request.clone();
        self.value_options.clear();
        let Some(request) = request else {
            self.value_lookup_running = false;
            self._value_lookup = None;
            return;
        };
        let Some(pool) = crate::settings_store::pool(cx) else { return };

        self.value_lookup_running = true;
        self._value_lookup = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(VALUE_DEBOUNCE).await;
            let asked = request.clone();
            let found = runtime::spawn(cx, async move { look_up_values(&pool, &asked).await }).await;
            let _ = this.update(cx, |this, cx| {
                // The caret has moved on to another field since.
                if this.value_request.as_ref() != Some(&request) {
                    return;
                }
                this.value_lookup_running = false;
                if let Ok(options) = found {
                    this.value_options = options;
                }
                cx.notify();
            });
        }));
    }

    /// The results follow the query as it is typed, and Enter runs it at
    /// once.
    ///
    /// The egui bar re-queries on every change it reports; this waits out a
    /// short pause first, because a keystroke here is a round trip to the
    /// index rather than a frame the egui tab was drawing anyway.
    fn on_query_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        match event {
            InputEvent::Focus => {}
            InputEvent::Blur => self.close_completions(cx),
            InputEvent::PressEnter { .. } => {
                // That Enter was the dropdown's, not the bar's.
                if std::mem::take(&mut self.took_completion_on_enter) {
                    return;
                }
                self.query_settings_dirty = true;
                self._rerun = None;
                // Running is an answer to what the dropdown was offering, so
                // it closes: the arrows then walk the history instead.
                self.completions_open = false;
                self.completion_cursor = None;
                self.remember_query(cx);
                self.pending_commit = true;
                cx.notify();
            }
            InputEvent::Change => {
                self.query_settings_dirty = true;
                let text = self.full_query(cx);
                if text == self.last_run_query {
                    return;
                }
                // Typing is a new query, not a step in the walk. The walk's
                // own edits carry the entry it just recalled, so only text
                // that is not that leaves the walk.
                if let Some((at, _)) = self.history_walk.as_ref()
                    && self.history.get(*at) != Some(&text)
                {
                    self.history_walk = None;
                }
                self._rerun = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(RERUN_DELAY).await;
                    let _ = this.update(cx, |this, cx| this.run(cx));
                }));
            }
        }
    }

    /// Asks the index how many matches carry no game mode, so a query that
    /// filters on one can say what it cannot see.
    ///
    /// Asked per search rather than once: ingest appends to the index while
    /// the app runs, so a latched count would keep showing a hint the user
    /// has already acted on, or keep hiding one that has since applied. One
    /// lookup at a time, since each is a scan.
    fn look_up_game_mode_gap(&mut self, cx: &mut Context<Self>) {
        if self.gap_lookup_running {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        self.gap_lookup_running = true;

        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move { query::matches_missing_game_mode_count(&pool).await }).await;
            let _ = this.update(cx, |this, cx| {
                this.gap_lookup_running = false;
                match found {
                    Ok(Ok(count)) => this.game_mode_gap = Some(count),
                    // The results still stand; only the hint is missing.
                    Ok(Err(err)) => tracing::warn!("search: the game-mode gap lookup failed: {err}"),
                    Err(err) => tracing::warn!("search: the game-mode gap lookup did not complete: {err}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the query the bar was last left holding and runs it.
    ///
    /// The row is the egui tab's own (`search`), so both front ends reopen on
    /// the same query; only that field is written back, leaving the saved
    /// searches, the history and the column set the egui tab keeps there
    /// untouched.
    /// The width `column` is drawn at: what the reader dragged it to, or its
    /// own default.
    fn width_of(&self, column: ResultColumn) -> Pixels {
        self.column_widths.get(&column).copied().unwrap_or_else(|| column.default_width())
    }

    /// Starts a width drag on `column` from the pointer's current position.
    fn start_column_drag(&mut self, column: ResultColumn, at: Pixels, cx: &mut Context<Self>) {
        self.resizing = Some(ColumnDrag { column, pointer_start: at, width_start: self.width_of(column) });
        cx.notify();
    }

    /// Follows a width drag. A no-op when nothing is being dragged, which is
    /// every pointer move outside one.
    fn drag_column(&mut self, at: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = self.resizing else { return };
        let width = (drag.width_start + (at - drag.pointer_start)).max(COLUMN_DRAG_MIN);
        self.column_widths.insert(drag.column, width);
        cx.notify();
    }

    /// Ends a width drag, keeping where it got to.
    fn end_column_drag(&mut self, cx: &mut Context<Self>) {
        if self.resizing.take().is_some() {
            self.save_column_widths(cx);
            cx.notify();
        }
    }

    /// Puts `column` back on its default width.
    fn reset_column_width(&mut self, column: ResultColumn, cx: &mut Context<Self>) {
        self.resizing = None;
        if self.column_widths.remove(&column).is_some() {
            self.save_column_widths(cx);
            cx.notify();
        }
    }

    /// Writes the dragged widths back, in `ResultColumn::all` order, so the
    /// tab opens at them next time.
    fn save_column_widths(&self, cx: &mut Context<Self>) {
        let widths: Vec<Option<f32>> = ResultColumn::all()
            .into_iter()
            .map(|column| self.column_widths.get(&column).copied().map(f32::from))
            .collect();
        crate::settings_store::save(COLUMN_WIDTHS_KEY, &widths, cx);
    }

    /// Reads the dragged widths back on the first frame, once the config
    /// database is open. `new` runs before it is.
    fn load_column_widths(&mut self, cx: &mut Context<Self>) {
        if self.widths_loaded {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        self.widths_loaded = true;
        cx.spawn(async move |this, cx| {
            let stored = runtime::spawn(cx, async move {
                wows_toolkit_config::queries::get_setting::<Vec<Option<f32>>>(&pool, COLUMN_WIDTHS_KEY).await
            })
            .await;
            let Ok(Some(widths)) = stored else { return };
            let _ = this.update(cx, |this, cx| {
                for (column, width) in ResultColumn::all().into_iter().zip(widths) {
                    // A width narrower than the grip would leave a column
                    // that cannot be grabbed to widen again.
                    let Some(width) = width else { continue };
                    this.column_widths.insert(column, px(width).max(COLUMN_DRAG_MIN));
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_saved_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opened {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        self.opened = true;

        cx.spawn(async move |this, cx| {
            let stored = runtime::spawn(cx, async move {
                wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(&pool, SEARCH_SETTINGS_KEY).await
            })
            .await;
            let stored = match stored {
                Ok(Ok(stored)) => stored,
                Ok(Err(error)) => {
                    let message = format!("Could not read Search settings: {error}");
                    let _ = this.update(cx, |this, cx| {
                        this.settings_load_error = Some(message.clone().into());
                        if !this.query_settings_dirty
                            && !this.sort_settings_dirty
                            && !this.history_settings_dirty
                            && !this.operator_preferences_dirty
                        {
                            this.state = SearchState::Failed(message);
                        }
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let message = format!("Search settings read did not complete: {error}");
                    let _ = this.update(cx, |this, cx| {
                        this.settings_load_error = Some(message.clone().into());
                        if !this.query_settings_dirty
                            && !this.sort_settings_dirty
                            && !this.history_settings_dirty
                            && !this.operator_preferences_dirty
                        {
                            this.state = SearchState::Failed(message);
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            let (query, history, sort, operator_preferences) = match parse_saved_settings(stored) {
                Ok(settings) => settings,
                Err(message) => {
                    let _ = this.update(cx, |this, cx| {
                        this.settings_load_error = Some(message.clone().into());
                        if !this.query_settings_dirty
                            && !this.sort_settings_dirty
                            && !this.history_settings_dirty
                            && !this.operator_preferences_dirty
                        {
                            this.state = SearchState::Failed(message);
                        }
                        cx.notify();
                    });
                    return;
                }
            };

            let _ = this.update_in(cx, |this, window, cx| {
                this.settings_load_error = None;
                let restored_sort = !this.sort_settings_dirty;
                if !this.history_settings_dirty {
                    this.history = history;
                }
                if !this.sort_settings_dirty {
                    this.sort = sort;
                }
                if !this.operator_preferences_dirty {
                    this.operator_preferences = operator_preferences;
                }
                if !this.query_settings_dirty {
                    this.restore_query_text(query, window, cx);
                    // An empty query matches everything, which is the page the
                    // egui tab opens on.
                    this.completions_open = false;
                    this.run(cx);
                } else if restored_sort && this.last_run_query == this.full_query(cx) {
                    this.run(cx);
                } else {
                    cx.notify();
                }
            });
        })
        .detach();
        let _ = window;
    }

    /// Writes the query text back into the shared row, keeping every other
    /// field the egui tab stores beside it.
    fn save_query(&self, text: String, cx: &mut Context<Self>) {
        self.save_search_value("query", serde_json::Value::String(text), cx);
    }

    /// Adopts the game data the replay inspector opened, so results can be
    /// named in the current locale.
    pub fn set_game_data(&mut self, game_data: Option<GameDataCache>, cx: &mut Context<Self>) {
        self.game_data = game_data;
        self.resolved_ships.clear();
        cx.notify();
    }

    /// Resolves each result's ship name against its own build.
    ///
    /// Only builds already loaded are asked: a search can span years of
    /// replays, and loading every build they were recorded on would cost more
    /// than the names are worth. A build that is not loaded leaves its rows
    /// on the name stored when they were indexed.
    fn resolve_ship_names(&mut self, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else { return };

        let wanted: Vec<(u32, GameParamId)> = self
            .hits
            .iter()
            .filter_map(|hit| Some((hit.version_build?, hit.self_ship_id?)))
            .filter(|key| !self.resolved_ships.contains_key(key))
            .collect();
        if wanted.is_empty() {
            return;
        }

        cx.spawn(async move |this, cx| {
            let resolved = cx
                .background_spawn(async move {
                    let mut resolved = HashMap::new();
                    for (build, ship_id) in wanted {
                        let Some(loaded) = game_data.loaded_build(build) else { continue };
                        if let Some(name) = ship_display::try_resolve_ship_name(ship_id, Some(loaded.provider())) {
                            resolved.insert((build, ship_id), name);
                        }
                    }
                    resolved
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.resolved_ships.extend(resolved);
                cx.notify();
            });
        })
        .detach();
    }

    /// Parses the query text and searches the index with it.
    ///
    /// An empty query matches everything, which is how the egui bar opens; a
    /// query that does not parse reports where rather than searching for it
    /// literally.
    fn run(&mut self, cx: &mut Context<Self>) {
        self.last_run_query = self.full_query(cx);
        // Bumped first: every exit below changes what is on screen, and a
        // search already in flight must not land over it.
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;

        // Parsed before the index is asked for: reading the query back as
        // pills is something the bar can do with no database open, and an
        // index that is not there says nothing about whether the query is
        // well formed.
        let text = self.full_query(cx);
        let expr = match query_text::parse_query(&text) {
            Ok(expr) => expr,
            Err(err) => {
                // The results on screen came from a query that did parse, so
                // they stay: the error is said above them, pointing at the
                // part of the text it is about, rather than taking the page
                // over.
                self.state =
                    SearchState::Invalid { reason: err.kind.to_string(), span: err.span.clone(), text: text.clone() };
                cx.notify();
                return;
            }
        };
        self.expr = Some(expr.clone());
        let prior_preferences = self.operator_preferences.clone();
        select::record_operators(&expr, &mut self.operator_preferences);
        if self.operator_preferences != prior_preferences {
            self.operator_preferences_dirty = true;
            match serde_json::to_value(&self.operator_preferences) {
                Ok(value) => self.save_search_value("op_prefs", value, cx),
                Err(error) => tracing::warn!("search: operator preferences could not be serialized: {error}"),
            }
        }
        self.save_query(text.clone(), cx);

        let Some(pool) = crate::settings_store::pool(cx) else {
            self.state = SearchState::Failed("the replay index is not open".to_string());
            cx.notify();
            return;
        };

        self.state = SearchState::Running;
        self.look_up_game_mode_gap(cx);
        cx.notify();

        let sort = self.sort;
        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move {
                // The map catalog only resolves friendly map names; an empty
                // one still searches, it just cannot match a map by label.
                let ctx = CompileCtx::default();
                // The query fetches one row past the limit itself, so a set
                // of exactly the limit reads as a complete answer rather than
                // a truncated one. The extra row is dropped below.
                query::search_by_ast(&pool, &expr, &ctx, RESULT_LIMIT, sort).await
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match found {
                    Ok(Ok(mut hits)) => {
                        this.truncated = hits.len() as i64 > RESULT_LIMIT;
                        hits.truncate(RESULT_LIMIT as usize);
                        this.hits = hits;
                        this.state = SearchState::Done;
                    }
                    Ok(Err(err)) => this.state = SearchState::Failed(err.to_string()),
                    Err(err) => this.state = SearchState::Failed(err.to_string()),
                }
                this.on_disk = this.hits.iter().map(|hit| hit.replay_path.is_file()).collect();
                this.sync_rows(cx);
                this.resolve_ship_names(cx);
            });
        })
        .detach();
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        self.list_state.reset(self.hits.len());
        cx.notify();
    }

    /// A header click re-sorts, which means re-running: the ordering is done
    /// by the query, not over the page already fetched.
    fn sort_by(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        self.sort_settings_dirty = true;
        self.sort = self.sort.after_click(column);
        self.save_search_value("sort", sort_value(self.sort), cx);
        if matches!(self.state, SearchState::Done | SearchState::Running) {
            self.run(cx);
        } else {
            cx.notify();
        }
    }
}

impl Focusable for SearchView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Column widths, in the order the header lays them out.
fn column_width(column: SortColumn) -> Pixels {
    match column {
        SortColumn::Date => px(160.),
        SortColumn::Map => px(150.),
        SortColumn::Mode => px(130.),
        SortColumn::Outcome => px(90.),
        SortColumn::Damage => px(100.),
        SortColumn::Kills => px(70.),
        SortColumn::Pr => px(80.),
    }
}

fn outcome_label(outcome: MatchOutcome) -> String {
    match outcome {
        MatchOutcome::Win => t!("ui.search.outcome_win").into_owned(),
        MatchOutcome::Loss => t!("ui.search.outcome_loss").into_owned(),
        MatchOutcome::Draw => t!("ui.search.outcome_draw").into_owned(),
        MatchOutcome::Unknown => "-".to_string(),
    }
}

/// What a hit shows in `column`. An absent value reads as a dash rather than
/// a zero, which would sort and scan as a real result.
/// One result's actions: open the replay in the inspector, and copy its
/// path. A replay the index knows about but that is no longer on disk cannot
/// be opened, and says so rather than failing on click.
///
/// `exists` is checked when the results land rather than here: this runs per
/// row per frame, and the check is a syscall.
/// What a result row offers, built once and used by both the dots and the
/// row's right-click menu so the two cannot drift apart.
///
/// A replay that is no longer on disk keeps its menu: its path is still worth
/// copying. The rest is greyed out under a line that says why, since a
/// disabled item carries no tooltip of its own to explain itself.
fn build_row_menu(menu: PopupMenu, path: &std::path::Path, exists: bool, search: &Entity<SearchView>) -> PopupMenu {
    let open_path = path.to_path_buf();
    let open_search = search.clone();
    let render_path = path.to_path_buf();
    let render_search = search.clone();
    let copy_file = path.to_path_buf();
    let copy_path = path.to_path_buf();
    let reveal_path = path.to_path_buf();

    menu.when(!exists, |menu| menu.item(PopupMenuItem::label(t!("ui.search.open_missing").into_owned())).separator())
        .item(PopupMenuItem::new(t!("ui.search.open").into_owned()).disabled(!exists).on_click(
            move |_event, _window, cx| {
                let path = open_path.clone();
                open_search.update(cx, |_this, cx| cx.emit(SearchEvent::OpenReplay(path)));
            },
        ))
        .item(PopupMenuItem::new(t!("ui.replay.context.render_replay").into_owned()).disabled(!exists).on_click(
            move |_event, _window, cx| {
                let path = render_path.clone();
                render_search.update(cx, |_this, cx| cx.emit(SearchEvent::RenderReplay(path)));
            },
        ))
        .separator()
        .item(PopupMenuItem::new(t!("ui.replay.context.copy_replay").into_owned()).disabled(!exists).on_click(
            move |_event, window, cx| {
                crate::replay_inspector::browser_view::copy_replay_files(std::slice::from_ref(&copy_file), window, cx);
            },
        ))
        .item(PopupMenuItem::new(t!("ui.replay.context.copy_path").into_owned()).on_click(
            move |_event, _window, cx| {
                crate::replay_inspector::browser_view::copy_paths(std::slice::from_ref(&copy_path), cx);
            },
        ))
        .item(PopupMenuItem::new(t!("ui.replay.context.show_in_explorer").into_owned()).disabled(!exists).on_click(
            move |_event, _window, _cx| {
                crate::replay_inspector::browser_view::reveal_in_file_manager(&reveal_path);
            },
        ))
}

/// The dots at the end of a result row, up only while the row is pointed at.
///
/// `ix` keys the trigger so every row's popover state is its own, and `group`
/// ties the reveal to that row's hover, as the Replay Inspector's rows do.
fn row_actions(ix: usize, path: PathBuf, exists: bool, search: Entity<SearchView>, group: SharedString) -> AnyElement {
    let trigger = Button::new(("search-row-actions", ix))
        .ghost()
        .xsmall()
        .icon(IconName::Ellipsis)
        .tooltip(t!("ui.search.row_actions_hint").to_string());

    h_flex()
        .flex_none()
        .w(ACTIONS_COLUMN_WIDTH)
        .items_center()
        .invisible()
        .group_hover(group, |this| this.visible())
        .child(trigger.dropdown_menu(move |menu, _window, _cx| build_row_menu(menu, &path, exists, &search)))
        .into_any_element()
}

fn cell_text(hit: &MatchHit, column: SortColumn) -> String {
    match column {
        SortColumn::Date => hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string(),
        SortColumn::Map => hit.map.clone(),
        SortColumn::Mode => hit.game_mode.clone(),
        SortColumn::Outcome => outcome_label(hit.outcome).to_string(),
        // Grouped: a six-figure damage number is unreadable as a bare run of
        // digits, and every other surface groups it.
        SortColumn::Damage => hit.self_damage.map(|d| separate_number(d, None)).unwrap_or_else(|| "-".into()),
        SortColumn::Kills => hit.self_kills.map(|k| k.to_string()).unwrap_or_else(|| "-".into()),
        SortColumn::Pr => hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_else(|| "-".into()),
    }
}

/// One labelled fact per line, as the listing's own popup draws them.
fn preview_hover_grid(facts: &[wows_toolkit_viewmodel::listing_row::HoverFact]) -> impl IntoElement + use<> {
    let label_color = crate::theme::text_dim();
    v_flex().max_w(px(PREVIEW_WIDTH)).children(
        facts
            .iter()
            .map(|fact| {
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .w(PREVIEW_LABEL_WIDTH)
                            .flex_none()
                            .text_xs()
                            .text_color(label_color)
                            .child(fact.label.clone()),
                    )
                    .child(div().flex_1().min_w(px(0.)).text_xs().child(fact.value.clone()))
            })
            .collect::<Vec<_>>(),
    )
}

/// How wide the labels beside the map are, so the values line up.
const PREVIEW_LABEL_WIDTH: Pixels = px(64.);

/// The tone a battle result is read in, or none for a result the index does
/// not know.
fn outcome_color(outcome: MatchOutcome) -> Option<Hsla> {
    let semantic = crate::theme::semantic();
    let packed = match outcome {
        MatchOutcome::Win => semantic.win,
        MatchOutcome::Loss => semantic.loss,
        MatchOutcome::Draw => semantic.draw,
        MatchOutcome::Unknown => return None,
    };
    Some(rgb(packed).into())
}

/// The parse error, above the results: the message, and the query with the
/// run the parser objected to marked.
///
/// An empty or out-of-range span (the parser reports one past the end for
/// text that simply stops early) marks nothing, and the message stands on
/// its own.
fn parse_error_strip(reason: &str, span: std::ops::Range<usize>, text: &str) -> AnyElement {
    let warn: Hsla = rgb(0xe8a54a).into();
    let mut strip = v_flex()
        .id("search-parse-error")
        .test_support()
        .aria_label(t!("ui.search.parse_failed", reason = reason).into_owned())
        .w_full()
        .gap_px()
        .px(px(20.))
        .child(div().text_xs().text_color(warn).child(t!("ui.search.parse_failed", reason = reason).into_owned()));

    let marked = text.get(span.clone()).filter(|marked| !marked.is_empty());
    if let Some(marked) = marked {
        let before = text[..span.start].to_string();
        let after = text[span.end..].to_string();
        strip = strip.child(
            h_flex()
                .text_xs()
                .font_family("monospace")
                .child(div().text_color(crate::theme::text_dim()).child(before))
                .child(div().text_color(warn).underline().child(marked.to_string()))
                .child(div().text_color(crate::theme::text_dim()).child(after)),
        );
    }

    strip.into_any_element()
}

/// The band a personal rating falls in, in that band's own text tone.
fn rating_color(pr: f64) -> Hsla {
    let category = wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory::from_pr(pr);
    rgb(wows_toolkit_viewmodel::personal_rating::chip_text(category, crate::theme::is_dark_mode())).into()
}

impl Render for SearchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Enter asked for the typed term to become a pill; this is where
        // there is a window to clear the box with.
        if std::mem::take(&mut self.pending_commit) {
            self.commit_typed(window, cx);
        }
        self.preview.release_dropped(window);
        self.open_saved_query(window, cx);
        self.load_column_widths(cx);
        self.refresh_bar(window, cx);
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;

        // The input's own rectangle, recorded as it is laid out, so the
        // dropdown below can be anchored to its left edge and bottom.
        let measure = cx.weak_entity();
        // One bar: the terms that have been finished are pills and the one
        // being typed is a box sitting among them, where the token stream
        // says the caret goes. The egui bar is laid out the same way.
        let entity = cx.entity();
        let caret = div()
            .min_w(px(120.))
            .flex_1()
            .child(Input::new(&self.query_input).id("search-query").small().appearance(false))
            .into_any_element();
        let pills = match self.reading.as_ref() {
            Some(expr) => {
                let entity = entity.clone();
                let structure_entity = entity.clone();
                let edit_entity = entity.clone();
                crate::search_pills::pill_strip(
                    expr,
                    &self.name_cache,
                    &self.selection,
                    &self.operator_preferences,
                    cx,
                    move |taken, window, cx| {
                        entity.update(cx, |this, cx| this.set_query_text(taken, window, cx));
                    },
                    move |path, edit, window, cx| {
                        structure_entity.update(cx, |this, cx| this.apply_structural_edit(path, edit, window, cx));
                    },
                    self.value_edit.as_ref().map(|edit| (&edit.path, &self.value_input)),
                    move |path, window, cx| {
                        edit_entity.update(cx, |this, cx| this.begin_value_edit(path, window, cx));
                    },
                    caret,
                )
            }
            // Nothing finished yet, so the bar is the box on its own.
            None => Some(caret),
        };

        let entry_row = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(Icon::new(IconName::Search))
            .child(
                div()
                    .id("search-pills")
                    .test_support()
                    .flex_1()
                    .relative()
                    .px_2()
                    .py_0p5()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .children(pills)
                    .child(
                        canvas(
                            move |bounds, _window, cx| {
                                let _ = measure.update(cx, |this: &mut Self, cx| {
                                    if this.bar_bounds != Some(bounds) {
                                        this.bar_bounds = Some(bounds);
                                        cx.notify();
                                    }
                                });
                            },
                            |_bounds, _prepaint, _window, _cx| {},
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .child(
                Button::new("search-run")
                    .label(t!("ui.tabs.search").to_string())
                    .compact()
                    .on_click(cx.listener(|this: &mut Self, _event, window, cx| this.commit_typed(window, cx))),
            );

        // The completions hang under the input as a dropdown, the way the
        // egui bar's do, rather than as a row of buttons pushing the results
        // down the page.
        let offered = self.offered_completions();
        let cursor = self.completion_cursor;
        let theme = cx.theme();
        let (surface, muted, accent) = (theme.popover, theme.muted_foreground, theme.accent);
        let rows: Vec<AnyElement> = offered
            .iter()
            .enumerate()
            .map(|(index, completion)| {
                let replacement = completion.replacement.clone();
                let highlighted = cursor == Some(index);
                h_flex()
                    .id(("search-completion", index))
                    .test_support()
                    .aria_label(completion.label.clone())
                    .w_full()
                    .gap_4()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .when(highlighted, |this| this.bg(accent))
                    .hover(|this| this.bg(accent))
                    .child(div().text_sm().child(completion.label.clone()))
                    .child(div().text_xs().text_color(muted).child(completion.context.clone()))
                    .on_click(cx.listener(move |this, _event, window, cx| {
                        this.take_completion(replacement.clone(), Offer::WhatFollows, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        let looking_up = self.value_lookup_running.then(|| {
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .child(div().text_xs().text_color(muted).child(t!("ui.search.looking_up_values").into_owned()))
                .into_any_element()
        });

        // A date field is picked from a calendar rather than from a list of
        // values the index happens to hold.
        let bar_text = self.query_input.read(cx).value().to_string();
        let wants_calendar = suggest::date_value_at_caret(&bar_text).is_some();
        let calendar = (self.completions_open && wants_calendar).then_some(self.bar_bounds).flatten().map(|bounds| {
            deferred(
                anchored()
                    .position(point(bounds.origin.x, bounds.origin.y + bounds.size.height + px(4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .id("search-calendar")
                            .test_support()
                            .occlude()
                            .on_mouse_down_out(
                                cx.listener(|this: &mut Self, _event, _window, cx| this.close_completions(cx)),
                            )
                            .p_1()
                            .bg(surface)
                            .border_1()
                            .border_color(border)
                            .rounded(theme.radius)
                            .shadow_md()
                            .child(Calendar::new(&self.calendar)),
                    ),
            )
            .with_priority(1)
            .into_any_element()
        });

        // What the parser objected to, with the offending run of the query
        // marked where it sits.
        let parse_error = match &self.state {
            SearchState::Invalid { reason, span, text } => Some(parse_error_strip(reason, span.clone(), text)),
            _ => None,
        };

        let dropdown = (self.completions_open && !wants_calendar && (!rows.is_empty() || looking_up.is_some()))
            .then_some(self.bar_bounds)
            .flatten()
            .map(|bounds| {
                deferred(
                    anchored()
                        .position(point(bounds.origin.x, bounds.origin.y + bounds.size.height + px(4.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            v_flex()
                                .id("search-completions")
                                .test_support()
                                .occlude()
                                // A press anywhere off the dropdown puts it
                                // away, as it does for the library's own
                                // popovers (`base::popover`). A press inside
                                // it is the row being taken.
                                .on_mouse_down_out(
                                    cx.listener(|this: &mut Self, _event, _window, cx| this.close_completions(cx)),
                                )
                                // Sized to what it lists, not to the bar: a
                                // dropdown as wide as the window puts its
                                // breadcrumbs an inch from the text they
                                // belong to.
                                .min_w(px(COMPLETIONS_MIN_WIDTH))
                                .max_w(px(COMPLETIONS_MAX_WIDTH))
                                .max_h(px(COMPLETIONS_MAX_HEIGHT))
                                .overflow_scroll()
                                .p_1()
                                .gap_px()
                                .bg(surface)
                                .border_1()
                                .border_color(border)
                                .rounded(theme.radius)
                                .shadow_md()
                                .children(looking_up)
                                .children(rows),
                        ),
                )
                .with_priority(1)
                .into_any_element()
            });

        let query_bar = crate::ui::toolbar(cx)
            .flex_col()
            .items_stretch()
            .gap_1()
            .on_key_down(cx.listener(Self::on_bar_key))
            .child(entry_row)
            .when(self.value_error, |bar| {
                bar.child(
                    div()
                        .text_xs()
                        .text_color(crate::theme::accent())
                        .child("Invalid filter value. Correct the value or press Escape to cancel."),
                )
            })
            .when_some(calendar, |this, calendar| this.child(calendar))
            .when_some(dropdown, |this, rows| this.child(rows))
            .when_some(parse_error, |this, strip| this.child(strip));

        let header =
            h_flex().flex_none().gap_2().items_center().px_2().py_1().border_b_1().border_color(border).children(
                ResultColumn::all().into_iter().enumerate().map(|(ix, column)| {
                    let width = self.width_of(column);
                    // The grip sits on the header's trailing edge, over the
                    // rule between this column and the next, which is where a
                    // reader reaches for it.
                    let grip = div()
                        .id(("search-header-grip", ix))
                        .test_support()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(RESIZE_GRIP)
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                // The grip lies over the header's sort
                                // control. Without this, letting go of a drag
                                // over the header also re-sorts the results.
                                cx.stop_propagation();
                                // Double-clicking a grip puts that column back
                                // on its default, which is the usual way out of
                                // a drag that went too far.
                                if event.click_count >= 2 {
                                    this.reset_column_width(column, cx);
                                    return;
                                }
                                this.start_column_drag(column, event.position.x, cx);
                            }),
                        );

                    let ResultColumn::Sortable(sortable) = column else {
                        // Nothing to sort by, so the header is a label rather
                        // than a control that would refuse every click.
                        return div()
                            .relative()
                            .w(width)
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child(column.label())
                            .child(grip)
                            .into_any_element();
                    };

                    let active = self.sort.column == sortable;
                    div()
                        .relative()
                        .w(width)
                        .child(selectable(
                            ("search-sort", sortable as usize),
                            active,
                            div()
                                .id(("search-sort-button", sortable as usize))
                                .w_full()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .child(h_flex().gap_1().items_center().child(column.label()).when(active, |this| {
                                    this.child(Icon::new(match self.sort.direction {
                                        SortDirection::Ascending => IconName::SortAscending,
                                        SortDirection::Descending => IconName::SortDescending,
                                    }))
                                }))
                                .on_click(cx.listener(move |this, _event, _window, cx| this.sort_by(sortable, cx))),
                        ))
                        .child(grip)
                        .into_any_element()
                }),
            );

        let hits = self.hits.clone();
        // What the header just laid out, so a row cannot disagree with it.
        let drawn_columns: Vec<DrawnColumn> = ResultColumn::all()
            .into_iter()
            .map(|column| DrawnColumn { column, width: self.width_of(column) })
            .collect();
        let on_disk = self.on_disk.clone();
        let resolved = self.resolved_ships.clone();
        let entity = cx.entity();
        let render_row = move |ix: usize, _window: &mut Window, cx: &mut App| {
            let Some(hit) = hits.get(ix) else {
                return div().into_any_element();
            };
            let path = hit.replay_path.clone();
            let panel = entity.clone();
            let exists = on_disk.get(ix).copied().unwrap_or(false);
            let group = SharedString::from(format!("search-row-{ix}"));
            let open_path = path.clone();
            let open_entity = entity.clone();
            let menu_path = path.clone();
            let menu_entity = entity.clone();
            let actions_path = path.clone();
            h_flex()
                .id(ix)
                .test_support()
                .group(group.clone())
                .w_full()
                .h(ROW_HEIGHT)
                .gap_2()
                .items_center()
                .px_2()
                .when_some(crate::ui::stripe(ix, cx), |el, color| el.bg(color))
                .hover(|this| this.bg(hover_bg))
                // Opening the battle is what a reader is after when they
                // double-click one, which is how the Replay Inspector's own
                // listing reads a double-click (`browser_view`).
                .on_click(move |event: &ClickEvent, _window, cx: &mut App| {
                    if event.click_count() < 2 || !exists {
                        return;
                    }
                    let path = open_path.clone();
                    open_entity.update(cx, |_this, cx| cx.emit(SearchEvent::OpenReplay(path)));
                })
                // Every part of the row starts that row's preview, so it
                // does not matter where in the row the pointer came to rest.
                // Nothing here ends one: a leave raised by one part of a row
                // would cancel the preview another part of it had just
                // started. The pointer leaving the results is what ends a
                // preview, which the listing below does.
                .on_mouse_move(move |event: &MouseMoveEvent, _window, cx: &mut App| {
                    let path = path.clone();
                    let at = event.position;
                    panel.update(cx, |this, cx| this.hover_row(path, at, cx));
                })
                .children(drawn_columns.iter().copied().map(|DrawnColumn { column, width }| {
                    // The outcome and the rating carry their meaning in
                    // colour, as they do in the replay table and in the egui
                    // results (`ui/search_tab.rs`).
                    let tint = match column {
                        ResultColumn::Sortable(SortColumn::Outcome) => outcome_color(hit.outcome),
                        ResultColumn::Sortable(SortColumn::Pr) => hit.self_pr.map(rating_color),
                        _ => None,
                    };
                    div().w(width).text_sm().truncate().when_some(tint, |el, color| el.text_color(color)).child(
                        match column {
                            ResultColumn::Sortable(column) => cell_text(hit, column),
                            // The name this match's own build resolves when
                            // that build is loaded, else the one stored at
                            // index time.
                            ResultColumn::Ship => {
                                let live =
                                    hit.version_build.zip(hit.self_ship_id).and_then(|key| resolved.get(&key)).cloned();
                                ship_display_name(hit, live).unwrap_or_else(|| "-".to_string())
                            }
                        },
                    )
                }))
                .child(row_actions(ix, actions_path, exists, entity.clone(), group))
                // Wraps the row, so it goes last: the menu is a container
                // around what it belongs to rather than a style on it.
                .context_menu(move |menu, _window, _cx| build_row_menu(menu, &menu_path, exists, &menu_entity))
                .into_any_element()
        };

        // The dwelled row's battle, played back beside the pointer. The popup
        // goes up as soon as a bake starts, holding open water until the first
        // frame lands, so the map arrives in place rather than the popup growing
        // around it.
        let preview_art: Option<AnyElement> = match self.preview.frame() {
            Some(frame) => Some(img(frame).w(px(PREVIEW_WIDTH)).h(px(PREVIEW_WIDTH)).into_any_element()),
            None if self.preview.awaits_preview() => Some(
                div()
                    .id("search-preview-placeholder")
                    .test_support()
                    .w(px(PREVIEW_WIDTH))
                    .h(px(PREVIEW_WIDTH))
                    .bg(crate::preview_hover::MAP_PLACEHOLDER)
                    .into_any_element(),
            ),
            // Why there is none, where the map would have been: a replay from a
            // build that is not installed is the ordinary case.
            None => self.preview.failure().map(|reason| {
                // A column, clipped: the reason is a sentence rather than a
                // word, and a row flex would run it off the side of the map's
                // square instead of wrapping it inside.
                v_flex()
                    .id("search-preview-failed")
                    .test_support()
                    .aria_label(reason.clone())
                    .w(px(PREVIEW_WIDTH))
                    .h(px(PREVIEW_WIDTH))
                    .bg(crate::preview_hover::MAP_PLACEHOLDER)
                    .overflow_hidden()
                    .items_center()
                    .justify_center()
                    .p_2()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(div().w_full().child(reason.clone()))
                    .into_any_element()
            }),
        };
        // What the hovered row says, beside the map: the detail the columns have
        // no room for, worded by the reading both surfaces share.
        let preview_facts = self.preview.watched_path().and_then(|path| {
            let hit = self.hits.iter().find(|hit| hit.replay_path == path)?;
            let live = hit.version_build.zip(hit.self_ship_id).and_then(|key| self.resolved_ships.get(&key)).cloned();
            let ship = ship_display_name(hit, live).unwrap_or_else(|| "-".to_string());
            Some(wows_toolkit_viewmodel::listing_row::hover_facts_for_match(hit, ship, None))
        });
        // Floating at the pointer rather than in a strip of its own: a battle is
        // read beside the row it belongs to, and a strip at the foot of the tab
        // resized the results under the pointer that asked for it.
        let preview = (preview_art.is_some() || preview_facts.is_some()).then(|| {
            let theme = cx.theme();
            let anchor = point(self.preview_anchor.x + PREVIEW_CURSOR_OFFSET, self.preview_anchor.y);
            deferred(
                anchored().position(anchor).snap_to_window_with_margin(px(8.)).child(
                    v_flex()
                        .id("search-preview")
                        .test_support()
                        .gap_1()
                        .p_1()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .children(preview_art)
                        .when_some(preview_facts, |this, facts| this.child(preview_hover_grid(&facts))),
                ),
            )
            .with_priority(1)
        });

        let status = match &self.state {
            SearchState::Idle => Some(t!("ui.search.type_a_query").into_owned()),
            // Said in its own strip under the bar, not in place of the
            // results.
            SearchState::Invalid { .. } if !self.hits.is_empty() => None,
            SearchState::Invalid { .. } => Some(t!("ui.search.type_a_query").into_owned()),
            SearchState::Running => Some(t!("ui.search.searching").into_owned()),
            SearchState::Failed(reason) => Some(t!("ui.search.failed", reason = reason).into_owned()),
            SearchState::Done if self.hits.is_empty() => Some(t!("ui.search.no_matches").into_owned()),
            SearchState::Done => None,
        };
        let settings_error_needs_banner = status.is_none();
        let search_entity = cx.entity();

        // Singular at exactly one, so it never reads "1 indexed matches".
        let gap_hint = self
            .expr
            .as_ref()
            .zip(self.game_mode_gap)
            .filter(|(expr, missing)| search_hint::game_mode_gap_applies(*missing, expr))
            .map(|(_, missing)| {
                if missing == 1 {
                    "1 indexed match has no recorded game mode and cannot match this query. Re-index to fill it in."
                        .to_string()
                } else {
                    format!(
                        "{missing} indexed matches have no recorded game mode and cannot match this query.                          Re-index to fill them in."
                    )
                }
            });

        let body: AnyElement = match status {
            Some(status) => v_flex()
                .id("search-status")
                .test_support()
                .aria_label(status.clone())
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(status))
                .when_some(self.settings_load_error.clone(), |this, _reason| {
                    let search_entity = search_entity.clone();
                    this.child(
                        Button::new("search-settings-retry")
                            .label(t!("ui.buttons.retry").to_string())
                            .compact()
                            .on_click(move |_event, window, cx: &mut App| {
                                search_entity.update(cx, |this, cx| {
                                    this.settings_load_error = None;
                                    this.opened = false;
                                    this.open_saved_query(window, cx);
                                });
                            }),
                    )
                })
                .into_any_element(),
            None => div()
                .id("search-results")
                .test_support()
                .relative()
                .size_full()
                // The rows start a preview; the pointer leaving all of them
                // ends it, which is what crossing out of the results means.
                .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                    if !*hovered {
                        this.leave_rows(cx);
                    }
                }))
                .child(list(self.list_state.clone(), render_row).size_full())
                .child(Scrollbar::vertical(&self.list_state))
                .into_any_element(),
        };

        // Always there, empty until a search has finished, so a count
        // arriving does not lift the results off the bottom of the tab.
        let counted = match self.state {
            SearchState::Done if self.truncated => {
                t!("ui.search.match_count_truncated", count = RESULT_LIMIT).into_owned()
            }
            SearchState::Done => t!("ui.search.match_count", count = self.hits.len()).into_owned(),
            _ => String::new(),
        };
        let footer = h_flex()
            .flex_none()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(border)
            .child(div().text_xs().text_color(crate::theme::text_dim()).child(counted));

        v_flex()
            .id("search-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.drag_column(event.position.x, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| this.end_column_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| this.end_column_drag(cx)),
            )
            .child(query_bar)
            // The strip holds its line whether or not there is a gap to
            // report, so noticing one does not push the results down.
            .child(
                div()
                    .id("search-game-mode-gap")
                    .test_support()
                    .flex_none()
                    .px_2()
                    .py_1()
                    .text_xs()
                    // The colour the egui hint uses: this is a warning about
                    // results the query cannot reach, not a note.
                    .text_color(rgb(0xe8a54a))
                    .child(gap_hint.unwrap_or_default()),
            )
            .when_some(self.settings_save_error.clone(), |this, error| {
                let search_entity = search_entity.clone();
                this.child(
                    h_flex()
                        .id("search-settings-save-error")
                        .test_support()
                        .flex_none()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(0xe8a54a))
                        .child(error)
                        .child(
                            Button::new("search-settings-save-retry")
                                .label(t!("ui.buttons.retry").to_string())
                                .compact()
                                .on_click(move |_event, _window, cx: &mut App| {
                                    search_entity.update(cx, |this, cx| {
                                        if let Some((field, value)) = this.settings_failed_write.clone() {
                                            this.save_search_value(field, value, cx);
                                        }
                                    });
                                }),
                        ),
                )
            })
            .when(settings_error_needs_banner, |this| {
                this.when_some(self.settings_load_error.clone(), |this, error| {
                    let search_entity = search_entity.clone();
                    this.child(
                        h_flex()
                            .id("search-settings-load-error")
                            .test_support()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(rgb(0xe8a54a))
                            .child(error)
                            .child(
                                Button::new("search-settings-load-retry")
                                    .label(t!("ui.buttons.retry").to_string())
                                    .compact()
                                    .on_click(move |_event, window, cx: &mut App| {
                                        search_entity.update(cx, |this, cx| {
                                            this.settings_load_error = None;
                                            this.opened = false;
                                            this.open_saved_query(window, cx);
                                        });
                                    }),
                            ),
                    )
                })
            })
            .child(header)
            .child(div().flex_1().min_h(px(0.)).child(body))
            .child(footer)
            .when_some(preview, |this, preview| this.child(preview))
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt as _;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use wows_toolkit_config::index::rows::MatchHit;
    use wows_toolkit_config::index::rows::MatchOutcome;

    /// The query box, which is somewhere off the results.
    const SEARCH_QUERY_ID: &str = "search-query";

    use super::SearchEvent;
    use super::SearchState;
    use super::SearchView;
    use wows_toolkit_config::index::query::SortColumn;

    /// One result, on disk, so a row is drawn with every action available.
    fn hit(path: &str) -> MatchHit {
        MatchHit {
            arena_id: wows_replays::types::ArenaId::from(1i64),
            timestamp: jiff::Timestamp::from_second(1_760_000_000).expect("a timestamp in range"),
            map: "Okinawa".to_string(),
            game_mode: "Domination".to_string(),
            game_mode_id: None,
            game_type: "RandomBattle".to_string(),
            match_group: "pvp".to_string(),
            version_build: Some(13_187_581),
            source_id: wows_toolkit_config::index::rows::SourceId(1),
            outcome: MatchOutcome::Win,
            self_account_id: None,
            self_ship_id: None,
            self_ship_name: Some("Thunderer".to_string()),
            self_survived: Some(true),
            self_damage: Some(112_345),
            self_kills: Some(2),
            self_pr: Some(1543.0),
            results_available: true,
            replay_path: PathBuf::from(path),
            file_mtime: None,
        }
    }

    /// A view with one result already in it, so the row can be driven without
    /// an index behind it.
    fn open_with_one_hit(cx: &mut TestAppContext) -> gpui_kit::WindowHandle<SearchView> {
        cx.update(gpui_kit::init);
        cx.open_window(size(px(1200.), px(600.)), |window, cx| {
            let mut view = SearchView::new(window, cx);
            view.hits = vec![hit("C:/replays/one.wowsreplay")];
            view.on_disk = vec![true];
            view.state = SearchState::Done;
            view.list_state.reset(1);
            view
        })
    }

    fn record_events(
        window: gpui_kit::WindowHandle<SearchView>,
        cx: &mut TestAppContext,
    ) -> (Rc<RefCell<Vec<SearchEvent>>>, gpui_kit::Subscription) {
        let seen: Rc<RefCell<Vec<SearchEvent>>> = Rc::new(RefCell::new(Vec::new()));
        let view = window.entity(cx).expect("the window has a root view");
        let recorder = seen.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(&view, move |_view, event: &SearchEvent, _cx| recorder.borrow_mut().push(event.clone()))
        });
        (seen, subscription)
    }

    #[gpui_kit::test]
    fn double_clicking_a_result_opens_it(cx: &mut TestAppContext) {
        let window = open_with_one_hit(cx);
        let (seen, subscription) = record_events(window, cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(0usize, cx);
            assert!(seen.borrow().is_empty(), "one click does not open a battle");

            // The dots sit on the row, and opening their menu is not opening
            // the battle: a double-click that lands on them must not do both.
            window.double_click(("search-row-actions", 0usize), cx);
            assert!(seen.borrow().is_empty(), "the dots are the row's actions, not the row");

            window.double_click(0usize, cx);
        })
        .expect("the window is open");

        let seen = seen.borrow();
        assert!(
            matches!(seen.first(), Some(SearchEvent::OpenReplay(path)) if path.ends_with("one.wowsreplay")),
            "a double-click opens the replay, got {seen:?}"
        );
        drop(subscription);
    }

    /// Widening a column is not a request to re-sort by it. The grip lies
    /// over the header's own sort control here, rather than inside it.
    #[gpui_kit::test]
    fn dragging_a_header_grip_leaves_the_sort_alone(cx: &mut TestAppContext) {
        let window = open_with_one_hit(cx);
        let view = window.entity(cx).expect("the window has a root view");
        let before = cx.update(|cx| view.read(cx).sort);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("search-header-grip", 0usize)).bounds().center();
            window.drag(from, gpui_kit::point(from.x + px(40.), from.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        assert_eq!(cx.update(|cx| view.read(cx).sort), before, "the drag widened the column and nothing else");

        // And the header still sorts when the header itself is clicked.
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("search-sort", SortColumn::Map as usize), cx);
        })
        .expect("the window is open");
        assert_eq!(cx.update(|cx| view.read(cx).sort.column), SortColumn::Map);
    }

    /// A preview belongs to the row, not to the part of it the pointer came to
    /// rest on: it starts anywhere in the row, and only the pointer leaving the
    /// results ends it.
    #[gpui_kit::test]
    fn hovering_anywhere_in_a_row_previews_that_battle(cx: &mut TestAppContext) {
        let window = open_with_one_hit(cx);
        let view = window.entity(cx).expect("the window has a root view");
        let watched = |cx: &mut TestAppContext| {
            cx.update(|cx| view.read(cx).preview.watched_path().map(std::path::Path::to_path_buf))
        };

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.hover(0usize, cx);
            window.render_frame(cx);
        })
        .expect("the window is open");
        let row = watched(cx);
        assert!(row.is_some(), "the row under the pointer is previewed");
        let anchor = cx.update(|cx| view.read(cx).preview_anchor);

        // The dots are part of the row, not a gap in it.
        cx.update_window(window.into(), |_, window, cx| {
            window.hover(("search-row-actions", 0usize), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");
        assert_eq!(watched(cx), row, "the dots are part of the row, so it is still that battle being previewed");
        assert_eq!(
            cx.update(|cx| view.read(cx).preview_anchor),
            anchor,
            "and the popup stays where it went up rather than chasing the pointer along the row"
        );

        // Off the results altogether.
        cx.update_window(window.into(), |_, window, cx| {
            window.hover(SEARCH_QUERY_ID, cx);
            window.render_frame(cx);
        })
        .expect("the window is open");
        assert!(watched(cx).is_none(), "leaving the results ends the preview");
    }

    #[gpui_kit::test]
    fn a_result_row_carries_the_dots_that_open_its_actions(cx: &mut TestAppContext) {
        let window = open_with_one_hit(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find(("search-row-actions", 0usize)).is_some(),
                "the row carries the dots the actions hang off"
            );
        })
        .expect("the window is open");
    }
}
