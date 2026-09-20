//! The Player Tracker tab: everyone met in indexed battles, over a chosen
//! window of time.
//!
//! Mirrors the egui tab's historical table: a period selector and a name
//! filter above a sortable table of players, their clan and how often they
//! have been met. The rows come from the shared replay index.

use gpui_kit::component::ActiveTheme;
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
use jiff::Timestamp;
use sqlx::sqlite::SqlitePool;

use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::player_tracker::Sort;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::SortOrder;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::visible_players;

use crate::runtime;
use crate::ui::selectable;

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
const NAME_COLUMN_WIDTH: Pixels = px(220.);
const CLAN_COLUMN_WIDTH: Pixels = px(120.);
const COUNT_COLUMN_WIDTH: Pixels = px(110.);

/// Where the tab is in loading the index.
enum LoadState {
    /// Before the config database is available.
    Idle,
    Loading,
    Failed(String),
    Loaded,
}

pub struct PlayerTrackerView {
    period: TimePeriod,
    sort: Sort,
    filter_text: String,
    filter_input: Entity<InputState>,
    /// Everyone the index returned for the current period, unfiltered. The
    /// filter and sort are applied per render over this.
    players: Vec<PlayerFacet>,
    state: LoadState,
    /// Bumped per query so a slower earlier period cannot overwrite a later.
    generation: u64,
    list_state: ListState,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PlayerTrackerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by player or clan..."));
        let subscription = cx.subscribe(&filter_input, Self::on_filter_event);

        Self {
            period: TimePeriod::default(),
            sort: Sort::default(),
            filter_text: String::new(),
            filter_input,
            players: Vec::new(),
            state: LoadState::Idle,
            generation: 0,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    /// Queries the index for the current period. Called once the config
    /// database is open, and again whenever the period changes.
    pub fn refresh(&mut self, pool: SqlitePool, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.state = LoadState::Loading;
        cx.notify();

        let filter = self.period.match_filter(Timestamp::now());
        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move { query::distinct_players(&pool, &filter).await }).await;

            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match found {
                    Ok(Ok(players)) => {
                        this.players = players;
                        this.state = LoadState::Loaded;
                    }
                    Ok(Err(err)) => this.state = LoadState::Failed(err.to_string()),
                    Err(err) => this.state = LoadState::Failed(err.to_string()),
                }
                this.sync_rows(cx);
            });
        })
        .detach();
    }

    fn on_filter_event(&mut self, _state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        let InputEvent::Change = event else { return };
        let text = self.filter_input.read(cx).value().to_string();
        if text == self.filter_text {
            return;
        }
        self.filter_text = text;
        self.sync_rows(cx);
    }

    fn rows(&self) -> Vec<PlayerFacet> {
        visible_players(&self.players, &self.filter_text, self.sort)
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        self.list_state.reset(self.rows().len());
        cx.notify();
    }

    fn set_period(&mut self, period: TimePeriod, pool: Option<SqlitePool>, cx: &mut Context<Self>) {
        if self.period == period {
            return;
        }
        self.period = period;
        match pool {
            // A different window of time is a different query, not a filter
            // over what is already loaded.
            Some(pool) => self.refresh(pool, cx),
            None => self.sync_rows(cx),
        }
    }

    fn sort_by(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        self.sort = self.sort.toggled(column);
        self.sync_rows(cx);
    }
}

impl Focusable for PlayerTrackerView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PlayerTrackerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;
        let pool = crate::settings_store::pool(cx);

        let period_buttons = TimePeriod::ALL.map(|period| {
            let chosen = self.period == period;
            let pool = pool.clone();
            selectable(
                ("tracker-period", period as usize),
                chosen,
                Button::new(("tracker-period-button", period as usize))
                    .label(period.label())
                    .compact()
                    .selected(chosen)
                    .on_click(cx.listener(move |this, _event, _window, cx| this.set_period(period, pool.clone(), cx))),
            )
        });

        let toolbar = h_flex()
            .flex_none()
            .flex_wrap()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .children(period_buttons)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(Icon::new(IconName::Search))
                    .child(div().w(px(220.)).child(Input::new(&self.filter_input).id("tracker-filter").small())),
            );

        let header =
            h_flex().flex_none().gap_2().items_center().px_2().py_1().border_b_1().border_color(border).children(
                SortColumn::ALL.map(|column| {
                    let width = match column {
                        SortColumn::Name => NAME_COLUMN_WIDTH,
                        SortColumn::Clan => CLAN_COLUMN_WIDTH,
                        SortColumn::Encounters => COUNT_COLUMN_WIDTH,
                    };
                    let active = self.sort.column == column;
                    selectable(
                        ("tracker-sort", column as usize),
                        active,
                        div()
                            .id(("tracker-sort-button", column as usize))
                            .w(width)
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child(h_flex().gap_1().items_center().child(column.label()).when(active, |this| {
                                this.child(Icon::new(match self.sort.order {
                                    SortOrder::Ascending => IconName::SortAscending,
                                    SortOrder::Descending => IconName::SortDescending,
                                }))
                            }))
                            .on_click(cx.listener(move |this, _event, _window, cx| this.sort_by(column, cx))),
                    )
                }),
            );

        let rows = self.rows();
        let render_row = move |ix: usize, _window: &mut Window, _cx: &mut App| {
            let Some(row) = rows.get(ix) else {
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
                .child(div().w(NAME_COLUMN_WIDTH).text_sm().child(row.latest_name.clone()))
                .child(div().w(CLAN_COLUMN_WIDTH).text_sm().opacity(0.8).child(row.clan.clone()))
                .child(div().w(COUNT_COLUMN_WIDTH).text_sm().child(row.match_count.to_string()))
                .into_any_element()
        };

        let status = match &self.state {
            LoadState::Idle => Some("Waiting for the replay index".to_string()),
            LoadState::Loading => Some("Loading players...".to_string()),
            LoadState::Failed(reason) => Some(format!("Could not read the index: {reason}")),
            LoadState::Loaded if self.players.is_empty() => Some("No players indexed for this period".to_string()),
            LoadState::Loaded => None,
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

        v_flex()
            .id("tracker-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(toolbar)
            .child(header)
            .child(div().flex_1().min_h(px(0.)).child(body))
    }
}
