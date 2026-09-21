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

use crate::runtime;
use crate::ui::selectable;

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
    sort: SortSpec,
    hits: Vec<MatchHit>,
    state: SearchState,
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
            sort: SortSpec::default(),
            hits: Vec::new(),
            state: SearchState::Idle,
            generation: 0,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    /// Enter runs the query, as it does in the egui query bar.
    fn on_query_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.run(cx);
        }
    }

    /// Parses the query text and searches the index with it.
    ///
    /// An empty query matches everything, which is how the egui bar opens; a
    /// query that does not parse reports where rather than searching for it
    /// literally.
    fn run(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else {
            self.state = SearchState::Failed("the replay index is not open".to_string());
            cx.notify();
            return;
        };

        let text = self.query_input.read(cx).value().trim().to_string();
        let expr = match query_text::parse_query(&text) {
            Ok(expr) => expr,
            Err(err) => {
                self.state = SearchState::Invalid(err.to_string());
                self.hits.clear();
                self.sync_rows(cx);
                return;
            }
        };

        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.state = SearchState::Running;
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
                this.sync_rows(cx);
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
fn row_actions(ix: usize, hit: &MatchHit, search: Entity<SearchView>) -> AnyElement {
    let path = hit.replay_path.clone();
    let exists = path.exists();
    let open_path = path.clone();
    let copy_path = path.clone();

    h_flex()
        .flex_none()
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
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;

        let query_bar = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(Icon::new(IconName::Search))
            .child(div().flex_1().child(Input::new(&self.query_input).id("search-query").small().w_full()))
            .child(
                Button::new("search-run")
                    .label("Search")
                    .compact()
                    .on_click(cx.listener(|this, _event, _window, cx| this.run(cx))),
            );

        let header =
            h_flex().flex_none().gap_2().items_center().px_2().py_1().border_b_1().border_color(border).children(
                SortColumn::ALL.map(|column| {
                    let active = self.sort.column == column;
                    selectable(
                        ("search-sort", column as usize),
                        active,
                        div()
                            .id(("search-sort-button", column as usize))
                            .w(column_width(column))
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child(h_flex().gap_1().items_center().child(column.label()).when(active, |this| {
                                this.child(Icon::new(match self.sort.direction {
                                    SortDirection::Ascending => IconName::SortAscending,
                                    SortDirection::Descending => IconName::SortDescending,
                                }))
                            }))
                            .on_click(cx.listener(move |this, _event, _window, cx| this.sort_by(column, cx))),
                    )
                }),
            );

        let hits = self.hits.clone();
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
                .children(
                    SortColumn::ALL.map(|column| div().w(column_width(column)).text_sm().child(cell_text(hit, column))),
                )
                .child(row_actions(ix, hit, entity.clone()))
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
            .child(header)
            .child(div().flex_1().min_h(px(0.)).child(body))
            .when_some(footer, |this, footer| this.child(footer))
    }
}
