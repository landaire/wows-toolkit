//! The Search tab: a query over the replay index, and its results.
//!
//! Mirrors the egui tab's shape -- a query bar above a sortable results table
//! -- over the same query language. Parsing the query, compiling it to SQL and
//! deciding what a header click sorts by all live in the shared index, so this
//! is the rendering and the wiring.

use std::path::PathBuf;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use crate::search_pills::EditablePart;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_sql::CompileCtx;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    fn width(self) -> Pixels {
        match self {
            Self::Sortable(column) => column_width(column),
            Self::Ship => px(150.),
        }
    }
}

/// Raised for the app to act on.
#[derive(Clone, Debug)]
pub enum SearchEvent {
    /// Open this replay in the Replay Inspector.
    OpenReplay(std::path::PathBuf),
}

impl EventEmitter<SearchEvent> for SearchView {}

/// The settings row both front ends keep the query bar's state in.
const SEARCH_SETTINGS_KEY: &str = "search";

/// Results asked for per run. The egui table pages the same way rather than
/// pulling an unbounded set into memory.
const RESULT_LIMIT: i64 = 500;

/// The preview's edge length. Square, as the minimap is.
const PREVIEW_WIDTH: f32 = 240.;

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
/// The open/copy pair at the end of each row, which the header reserves.
const ACTIONS_COLUMN_WIDTH: Pixels = px(72.);

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
    /// The query text did not parse. Carries what the parser objected to.
    Invalid(String),
    Running,
    Failed(String),
    Done,
}

pub struct SearchView {
    query_input: Entity<InputState>,
    /// What the caret fragment may be completed to, and the bar text they
    /// were built from. Refreshed in `render` when the two disagree rather
    /// than from an input event: a completion taken by click changes the text
    /// too, and one path that notices covers both.
    completions: Vec<crate::search_pills::Completion>,
    completion_source: String,
    /// What the query says as it is typed, which is what the pills read back.
    /// Separate from `expr`, which is the query the results on screen came
    /// from: editing the text must not relabel results it has not been run
    /// against.
    reading: Option<wows_toolkit_config::index::query_ast::MatchExpr>,
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
    /// Whether the dropdown is showing. Closed by Escape and by taking a row,
    /// and reopened by the next edit, so it does not sit over the results
    /// after the query has been committed.
    completions_open: bool,
    /// Where the query input sits, so the dropdown can be anchored under it.
    /// Recorded during layout; `None` before the bar has been drawn once.
    bar_bounds: Option<Bounds<Pixels>>,
    /// Set when Enter took a completion. The input reports the same Enter
    /// through its own event, and without this the query would run on the
    /// text as it was before the row was taken.
    took_completion_on_enter: bool,
    /// The pill segment whose picker is open, and which part of it. `None`
    /// when none is.
    editing: Option<(NodePath, EditablePart)>,
    /// Names the pills read ids back as. Filled from the same lookups the
    /// result table uses, so a pill and a row name a ship the same way.
    name_cache: wows_toolkit_viewmodel::query_bar::label::NameCache,
    sort: SortSpec,
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
    /// Whether the saved query has been read and run. The tab opens showing
    /// what the query bar was left holding, the way the egui tab does, rather
    /// than an empty page with an instruction on it.
    opened: bool,
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
        let query_input = cx.new(|cx| InputState::new(window, cx).placeholder("outcome=win and map:ocean"));
        let subscription = cx.subscribe(&query_input, Self::on_query_event);

        Self {
            query_input,
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
            editing: None,
            name_cache: Default::default(),
            sort: SortSpec::default(),
            hits: Vec::new(),
            state: SearchState::Idle,
            expr: None,
            on_disk: Vec::new(),
            preview: Default::default(),
            game_data: None,
            resolved_ships: HashMap::new(),
            game_mode_gap: None,
            truncated: false,
            opened: false,
            gap_lookup_running: false,
            generation: 0,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    /// Puts `replacement` in the bar. What may follow it is re-offered by the
    /// next render, which notices the text changed.
    fn take_completion(&mut self, replacement: String, window: &mut Window, cx: &mut Context<Self>) {
        self.query_input.update(cx, |state, cx| state.set_value(replacement, window, cx));
        self.completion_cursor = None;
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
        let offered = self.offered_completions().len();
        match event.keystroke.key.as_str() {
            "down" if self.completions_open => self.move_completion_cursor(1, offered, cx),
            "up" if self.completions_open => self.move_completion_cursor(-1, offered, cx),
            "escape" => {
                self.completions_open = false;
                self.completion_cursor = None;
                cx.notify();
            }
            "enter" => {
                let Some(at) = self.completion_cursor else { return };
                let Some(taken) = self.offered_completions().get(at).map(|row| row.replacement.clone()) else {
                    return;
                };
                self.took_completion_on_enter = true;
                self.take_completion(taken, window, cx);
            }
            _ => {}
        }
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

    /// Opens the picker for a pill segment, or closes it when that segment's
    /// is the one already open.
    fn toggle_picker(&mut self, path: NodePath, part: EditablePart, cx: &mut Context<Self>) {
        let same = self.editing.as_ref().is_some_and(|(open, open_part)| open == &path && *open_part == part);
        self.editing = if same { None } else { Some((path, part)) };
        cx.notify();
    }

    /// Puts `query` in the bar and runs it: the bar exists to show matches,
    /// and leaving the old ones under an edited query would be showing the
    /// wrong ones.
    fn take_edit(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.query_input.update(cx, |state, cx| state.set_value(query, window, cx));
        self.run(cx);
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
    /// battle plays back under the results.
    pub(crate) fn hover_row(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let cache = self.game_data.clone();
        // The index records a match's map under the name it displays, not the
        // one the art is stored under, so a result has no map to draw ahead of
        // its bake; the bake's own map stands in.
        self.preview.enter(path, None, cache, cx, |panel| &mut panel.preview);
    }

    /// The pointer left the results.
    pub(crate) fn leave_rows(&mut self, cx: &mut Context<Self>) {
        self.preview.leave(cx);
    }

    /// Re-reads the bar when its text has changed: what the query says, and
    /// what the fragment under the caret may be completed to.
    ///
    /// The query is parsed on every edit, not only when it is run, so the
    /// pills read back what is being typed the way the egui bar's do.
    fn refresh_bar(&mut self, cx: &mut Context<Self>) {
        let text = self.query_input.read(cx).value().to_string();
        if text == self.completion_source {
            return;
        }
        self.reading = query_text::parse_query(&text).ok();
        self.completions = crate::search_pills::completions(&text);
        self.completion_source = text.clone();
        // Typing moves the caret off whatever row was highlighted, and an
        // edit is what reopens a dropdown Escape closed.
        self.completion_cursor = None;
        self.completions_open = true;
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

    /// Enter runs the query, as it does in the egui query bar.
    fn on_query_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. }) {
            return;
        }
        // That Enter was the dropdown's, not the bar's.
        if std::mem::take(&mut self.took_completion_on_enter) {
            return;
        }
        self.run(cx);
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
    fn open_saved_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opened {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        self.opened = true;

        cx.spawn(async move |this, cx| {
            let stored = runtime::spawn(cx, async move {
                wows_toolkit_config::queries::get_setting::<serde_json::Value>(&pool, SEARCH_SETTINGS_KEY).await
            })
            .await;
            let query = stored
                .ok()
                .flatten()
                .and_then(|value| value.get("query").and_then(|query| query.as_str()).map(str::to_owned))
                .unwrap_or_default();

            let _ = this.update_in(cx, |this, window, cx| {
                if !query.is_empty() {
                    this.query_input.update(cx, |state, cx| state.set_value(query, window, cx));
                }
                // An empty query matches everything, which is the page the
                // egui tab opens on.
                this.completions_open = false;
                this.run(cx);
            });
        })
        .detach();
        let _ = window;
    }

    /// Writes the query text back into the shared row, keeping every other
    /// field the egui tab stores beside it.
    fn save_query(&self, text: String, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        cx.spawn(async move |_this, cx| {
            let _ = runtime::spawn(cx, async move {
                let mut stored =
                    wows_toolkit_config::queries::get_setting::<serde_json::Value>(&pool, SEARCH_SETTINGS_KEY)
                        .await
                        .unwrap_or_else(|| serde_json::json!({}));
                if let Some(object) = stored.as_object_mut() {
                    object.insert("query".to_string(), serde_json::Value::String(text));
                }
                let json = stored.to_string();
                wows_toolkit_config::queries::set_setting_raw(&pool, SEARCH_SETTINGS_KEY, &json).await
            })
            .await;
        })
        .detach();
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
        // Bumped first: every exit below changes what is on screen, and a
        // search already in flight must not land over it.
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;

        // Parsed before the index is asked for: reading the query back as
        // pills is something the bar can do with no database open, and an
        // index that is not there says nothing about whether the query is
        // well formed.
        let text = self.query_input.read(cx).value().trim().to_string();
        let expr = match query_text::parse_query(&text) {
            Ok(expr) => expr,
            Err(err) => {
                self.state = SearchState::Invalid(err.to_string());
                // The hint above the table reads this; a query that did not
                // parse filters on nothing.
                self.expr = None;
                self.hits.clear();
                self.on_disk.clear();
                self.sync_rows(cx);
                return;
            }
        };
        self.expr = Some(expr.clone());
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
                // One more than the limit: a set of exactly the limit is a
                // complete answer, and reporting it as truncated would be a
                // lie. The extra row is dropped below.
                query::search_by_ast(&pool, &expr, &ctx, RESULT_LIMIT + 1, sort).await
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
        self.sort = self.sort.after_click(column);
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
fn row_actions(ix: usize, hit: &MatchHit, exists: bool, search: Entity<SearchView>) -> AnyElement {
    let path = hit.replay_path.clone();
    let open_path = path.clone();
    let copy_path = path.clone();

    h_flex()
        .flex_none()
        .w(ACTIONS_COLUMN_WIDTH)
        .gap_1()
        .items_center()
        .child(
            Button::new(("search-open", ix))
                .icon(IconName::FolderOpen)
                .compact()
                .disabled(!exists)
                .tooltip(if exists {
                    t!("ui.search.open").into_owned()
                } else {
                    t!("ui.search.open_missing").into_owned()
                })
                .on_click(move |_event, _window, cx: &mut App| {
                    let open_path = open_path.clone();
                    search.update(cx, |_this, cx| cx.emit(SearchEvent::OpenReplay(open_path)));
                }),
        )
        .child(
            Button::new(("search-copy", ix))
                .icon(IconName::Copy)
                .compact()
                .tooltip(t!("ui.search.copy_path").to_string())
                .on_click(move |_event, window, cx: &mut App| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_path.to_string_lossy().into_owned()));
                    crate::toast::ok(t!("ui.search.path_copied").to_string(), window, cx);
                }),
        )
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

/// The band a personal rating falls in, in that band's own text tone.
fn rating_color(pr: f64) -> Hsla {
    let category = wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory::from_pr(pr);
    rgb(wows_toolkit_viewmodel::personal_rating::chip_text(category, crate::theme::is_dark_mode())).into()
}

impl Render for SearchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.preview.release_dropped(window);
        self.open_saved_query(window, cx);
        self.refresh_bar(cx);
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;

        // The input's own rectangle, recorded as it is laid out, so the
        // dropdown below can be anchored to its left edge and bottom.
        let measure = cx.weak_entity();
        let entry_row = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(Icon::new(IconName::Search))
            .child(
                div()
                    .flex_1()
                    .relative()
                    .child(Input::new(&self.query_input).id("search-query").small().w_full())
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
                    .on_click(cx.listener(|this, _event, _window, cx| this.run(cx))),
            );

        // What the query the user typed actually says, read back through the
        // same rules the egui bar draws its pills with.
        let entity = cx.entity();
        let pills = self
            .reading
            .as_ref()
            .and_then(|expr| {
                let entity = entity.clone();
                crate::search_pills::pill_strip(expr, &self.name_cache, cx, move |path, part, cx| {
                    entity.update(cx, |this, cx| this.toggle_picker(path, part, cx));
                })
            })
            .map(|strip| div().id("search-pills").test_support().w_full().px(px(20.)).child(strip));

        // The picker for whichever pill segment was clicked.
        let picker = self.editing.as_ref().and_then(|(path, part)| {
            let offered = crate::search_pills::choices(self.reading.as_ref()?, path, *part);
            (!offered.is_empty()).then(|| {
                h_flex().w_full().flex_wrap().gap_1().px(px(20.)).children(offered.into_iter().enumerate().map(
                    |(index, choice)| {
                        let taken = choice.taken.clone();
                        selectable(
                            ("search-choice", index),
                            choice.current,
                            Button::new(("search-choice-button", index))
                                .label(choice.label)
                                .compact()
                                .selected(choice.current)
                                .on_click(cx.listener(move |this, _event, window, cx| {
                                    this.take_edit(taken.clone(), window, cx)
                                })),
                        )
                    },
                ))
            })
        });

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
                        this.completions_open = false;
                        this.take_completion(replacement.clone(), window, cx);
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

        let dropdown = (self.completions_open && (!rows.is_empty() || looking_up.is_some()))
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
                                .occlude()
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

        let query_bar = v_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .on_key_down(cx.listener(Self::on_bar_key))
            .child(entry_row)
            .when_some(pills, |this, pills| this.child(pills))
            .when_some(picker, |this, rows| this.child(rows))
            .when_some(dropdown, |this, rows| this.child(rows));

        let header =
            h_flex().flex_none().gap_2().items_center().px_2().py_1().border_b_1().border_color(border).children(
                ResultColumn::all().into_iter().map(|column| {
                    let ResultColumn::Sortable(sortable) = column else {
                        // Nothing to sort by, so the header is a label rather
                        // than a control that would refuse every click.
                        return div()
                            .w(column.width())
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child(column.label())
                            .into_any_element();
                    };

                    let active = self.sort.column == sortable;
                    selectable(
                        ("search-sort", sortable as usize),
                        active,
                        div()
                            .id(("search-sort-button", sortable as usize))
                            .w(column.width())
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child(h_flex().gap_1().items_center().child(column.label()).when(active, |this| {
                                this.child(Icon::new(match self.sort.direction {
                                    SortDirection::Ascending => IconName::SortAscending,
                                    SortDirection::Descending => IconName::SortDescending,
                                }))
                            }))
                            .on_click(cx.listener(move |this, _event, _window, cx| this.sort_by(sortable, cx))),
                    )
                    .into_any_element()
                }),
            );

        let hits = self.hits.clone();
        let on_disk = self.on_disk.clone();
        let resolved = self.resolved_ships.clone();
        let entity = cx.entity();
        let render_row = move |ix: usize, _window: &mut Window, cx: &mut App| {
            let Some(hit) = hits.get(ix) else {
                return div().into_any_element();
            };
            let path = hit.replay_path.clone();
            let panel = entity.clone();
            h_flex()
                .id(ix)
                .w_full()
                .h(ROW_HEIGHT)
                .gap_2()
                .items_center()
                .px_2()
                .when_some(crate::ui::stripe(ix, cx), |el, color| el.bg(color))
                .hover(|this| this.bg(hover_bg))
                .on_hover(move |hovered, _window, cx| {
                    let path = path.clone();
                    let hovered = *hovered;
                    panel.update(cx, |this, cx| {
                        if hovered {
                            this.hover_row(path, cx);
                        } else {
                            this.leave_rows(cx);
                        }
                    });
                })
                .children(ResultColumn::all().into_iter().map(|column| {
                    // The outcome and the rating carry their meaning in
                    // colour, as they do in the replay table and in the egui
                    // results (`ui/search_tab.rs`).
                    let tint = match column {
                        ResultColumn::Sortable(SortColumn::Outcome) => outcome_color(hit.outcome),
                        ResultColumn::Sortable(SortColumn::Pr) => hit.self_pr.map(rating_color),
                        _ => None,
                    };
                    div()
                        .w(column.width())
                        .text_sm()
                        .truncate()
                        .when_some(tint, |el, color| el.text_color(color))
                        .child(match column {
                            ResultColumn::Sortable(column) => cell_text(hit, column),
                            // The name this match's own build resolves when
                            // that build is loaded, else the one stored at
                            // index time.
                            ResultColumn::Ship => {
                                let live =
                                    hit.version_build.zip(hit.self_ship_id).and_then(|key| resolved.get(&key)).cloned();
                                ship_display_name(hit, live).unwrap_or_else(|| "-".to_string())
                            }
                        })
                }))
                .child(row_actions(ix, hit, on_disk.get(ix).copied().unwrap_or(false), entity.clone()))
                .into_any_element()
        };

        // The dwelled row's battle, played back. Absent until the pointer has
        // rested on a row whose build is loaded.
        let preview = self.preview.frame().map(|frame| {
            div()
                .id("search-preview")
                .test_support()
                .flex_none()
                .p_1()
                .border_t_1()
                .border_color(border)
                .child(img(frame).w(px(PREVIEW_WIDTH)).h(px(PREVIEW_WIDTH)))
        });

        let status = match &self.state {
            SearchState::Idle => Some(t!("ui.search.type_a_query").into_owned()),
            SearchState::Invalid(reason) => Some(t!("ui.search.parse_failed", reason = reason).into_owned()),
            SearchState::Running => Some(t!("ui.search.searching").into_owned()),
            SearchState::Failed(reason) => Some(t!("ui.search.failed", reason = reason).into_owned()),
            SearchState::Done if self.hits.is_empty() => Some(t!("ui.search.no_matches").into_owned()),
            SearchState::Done => None,
        };

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
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(status))
                .into_any_element(),
            None => div()
                .relative()
                .size_full()
                .child(list(self.list_state.clone(), render_row).size_full())
                .child(Scrollbar::vertical(&self.list_state))
                .into_any_element(),
        };

        let footer = matches!(self.state, SearchState::Done).then(|| {
            let capped = self.truncated;
            h_flex().flex_none().px_2().py_1().border_t_1().border_color(border).child(
                div().text_xs().text_color(crate::theme::text_dim()).child(if capped {
                    t!("ui.search.match_count_truncated", count = RESULT_LIMIT).into_owned()
                } else {
                    t!("ui.search.match_count", count = self.hits.len()).into_owned()
                }),
            )
        });

        v_flex()
            .id("search-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(query_bar)
            .when_some(gap_hint, |this, hint| {
                this.child(
                    div()
                        .id("search-game-mode-gap")
                        .test_support()
                        .flex_none()
                        .px_2()
                        .py_1()
                        .text_xs()
                        // The colour the egui hint uses: this is a warning
                        // about results the query cannot reach, not a note.
                        .text_color(rgb(0xe8a54a))
                        .child(hint),
                )
            })
            .child(header)
            .child(div().flex_1().min_h(px(0.)).child(body))
            .when_some(preview, |this, preview| this.child(preview))
            .when_some(footer, |this, footer| this.child(footer))
    }
}
