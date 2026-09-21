//! The Search tab: a query over the replay index, and its results.
//!
//! Mirrors the egui tab's shape -- a query bar above a sortable results table
//! -- over the same query language. Parsing the query, compiling it to SQL and
//! deciding what a header click sorts by all live in the shared index, so this
//! is the rendering and the wiring.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
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

use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query::SortColumn;
use wows_toolkit_config::index::query::SortDirection;
use wows_toolkit_config::index::query::SortSpec;
use wows_toolkit_config::index::query_sql::CompileCtx;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_config::index::rows::MatchOutcome;

use std::collections::HashMap;

use wows_replays::types::GameParamId;
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

/// Results asked for per run. The egui table pages the same way rather than
/// pulling an unbounded set into memory.
const RESULT_LIMIT: i64 = 500;

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
/// The open/copy pair at the end of each row, which the header reserves.
const ACTIONS_COLUMN_WIDTH: Pixels = px(72.);

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
            name_cache: Default::default(),
            sort: SortSpec::default(),
            hits: Vec::new(),
            state: SearchState::Idle,
            expr: None,
            on_disk: Vec::new(),
            game_data: None,
            resolved_ships: HashMap::new(),
            game_mode_gap: None,
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
        cx.notify();
    }

    /// Re-offers the completions when the bar text has changed since they
    /// were built.
    fn refresh_completions(&mut self, cx: &mut Context<Self>) {
        let text = self.query_input.read(cx).value().to_string();
        if text == self.completion_source {
            return;
        }
        self.completions = crate::search_pills::completions(&text);
        self.completion_source = text;
    }

    /// Enter runs the query, as it does in the egui query bar.
    fn on_query_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.run(cx);
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

        let Some(pool) = crate::settings_store::pool(cx) else {
            self.state = SearchState::Failed("the replay index is not open".to_string());
            self.expr = None;
            cx.notify();
            return;
        };

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

        self.state = SearchState::Running;
        self.expr = Some(expr.clone());
        self.look_up_game_mode_gap(cx);
        cx.notify();

        let sort = self.sort;
        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move {
                // The map catalog only resolves friendly map names; an empty
                // one still searches, it just cannot match a map by label.
                let ctx = CompileCtx::default();
                query::search_by_ast(&pool, &expr, &ctx, RESULT_LIMIT, sort).await
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match found {
                    Ok(Ok(hits)) => {
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

fn outcome_label(outcome: MatchOutcome) -> &'static str {
    match outcome {
        MatchOutcome::Win => "Win",
        MatchOutcome::Loss => "Loss",
        MatchOutcome::Draw => "Draw",
        MatchOutcome::Unknown => "-",
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
                .tooltip(if exists { "Open this replay" } else { "This replay is no longer on disk" })
                .on_click(move |_event, _window, cx: &mut App| {
                    let open_path = open_path.clone();
                    search.update(cx, |_this, cx| cx.emit(SearchEvent::OpenReplay(open_path)));
                }),
        )
        .child(
            Button::new(("search-copy", ix)).icon(IconName::Copy).compact().tooltip("Copy the replay path").on_click(
                move |_event, _window, cx: &mut App| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_path.to_string_lossy().into_owned()));
                },
            ),
        )
        .into_any_element()
}

fn cell_text(hit: &MatchHit, column: SortColumn) -> String {
    match column {
        SortColumn::Date => hit.timestamp.strftime("%Y-%m-%d %H:%M").to_string(),
        SortColumn::Map => hit.map.clone(),
        SortColumn::Mode => hit.game_mode.clone(),
        SortColumn::Outcome => outcome_label(hit.outcome).to_string(),
        SortColumn::Damage => hit.self_damage.map(|d| d.to_string()).unwrap_or_else(|| "-".into()),
        SortColumn::Kills => hit.self_kills.map(|k| k.to_string()).unwrap_or_else(|| "-".into()),
        SortColumn::Pr => hit.self_pr.map(|pr| format!("{pr:.0}")).unwrap_or_else(|| "-".into()),
    }
}

impl Render for SearchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_completions(cx);
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;

        let entry_row = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(Icon::new(IconName::Search))
            .child(div().flex_1().child(Input::new(&self.query_input).id("search-query").small().w_full()))
            .child(
                Button::new("search-run")
                    .label("Search")
                    .compact()
                    .on_click(cx.listener(|this, _event, _window, cx| this.run(cx))),
            );

        // What the query the user typed actually says, read back through the
        // same rules the egui bar draws its pills with.
        let pills = self
            .expr
            .as_ref()
            .and_then(|expr| crate::search_pills::pill_strip(expr, &self.name_cache, cx))
            .map(|strip| div().id("search-pills").w_full().px(px(20.)).child(strip));

        let completions = (!self.completions.is_empty()).then(|| {
            h_flex().w_full().flex_wrap().gap_1().px(px(20.)).children(self.completions.iter().enumerate().map(
                |(index, completion)| {
                    let replacement = completion.replacement.clone();
                    Button::new(("search-completion", index))
                        .label(completion.label.clone())
                        .compact()
                        .tooltip(completion.context.clone())
                        .on_click(cx.listener(move |this, _event, window, cx| {
                            this.take_completion(replacement.clone(), window, cx)
                        }))
                },
            ))
        });

        let query_bar = v_flex()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(entry_row)
            .when_some(pills, |this, pills| this.child(pills))
            .when_some(completions, |this, rows| this.child(rows));

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
        let render_row = move |ix: usize, _window: &mut Window, _cx: &mut App| {
            let Some(hit) = hits.get(ix) else {
                return div().into_any_element();
            };
            h_flex()
                .id(ix)
                .w_full()
                .h(ROW_HEIGHT)
                .gap_2()
                .items_center()
                .px_2()
                .hover(|this| this.bg(hover_bg))
                .children(ResultColumn::all().into_iter().map(|column| {
                    div().w(column.width()).text_sm().truncate().child(match column {
                        ResultColumn::Sortable(column) => cell_text(hit, column),
                        // The name this match's own build resolves when that
                        // build is loaded, else the one stored at index time.
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

        let status = match &self.state {
            SearchState::Idle => Some("Type a query and press Enter".to_string()),
            SearchState::Invalid(reason) => Some(format!("That query did not parse: {reason}")),
            SearchState::Running => Some("Searching...".to_string()),
            SearchState::Failed(reason) => Some(format!("The search failed: {reason}")),
            SearchState::Done if self.hits.is_empty() => Some("No matches".to_string()),
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
                .child(div().text_sm().opacity(0.6).child(status))
                .into_any_element(),
            None => div()
                .relative()
                .size_full()
                .child(list(self.list_state.clone(), render_row).size_full())
                .child(Scrollbar::vertical(&self.list_state))
                .into_any_element(),
        };

        let footer = matches!(self.state, SearchState::Done).then(|| {
            let capped = self.hits.len() as i64 == RESULT_LIMIT;
            h_flex().flex_none().px_2().py_1().border_t_1().border_color(border).child(
                div().text_xs().opacity(0.6).child(if capped {
                    format!("First {RESULT_LIMIT} matches")
                } else {
                    format!("{} matches", self.hits.len())
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
            .when_some(footer, |this, footer| this.child(footer))
    }
}
