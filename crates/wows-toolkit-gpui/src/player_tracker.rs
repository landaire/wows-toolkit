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
use wows_toolkit_viewmodel::player_tracker::ClanRow;

use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::player_tracker::ClanSort;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::Sort;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::SortOrder;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::clan_rows;
use wows_toolkit_viewmodel::player_tracker::visible_players;

use crate::runtime;
use crate::ui::selectable;

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
const NAME_COLUMN_WIDTH: Pixels = px(220.);
const CLAN_TAG_COLUMN_WIDTH: Pixels = px(160.);
const MEMBERS_COLUMN_WIDTH: Pixels = px(120.);

/// Which table the tab is showing.
///
/// Both read the same loaded players, so switching is a re-render rather than
/// another query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubTab {
    Players,
    Clans,
}

impl SubTab {
    const ALL: [SubTab; 2] = [SubTab::Players, SubTab::Clans];

    fn label(self) -> &'static str {
        match self {
            Self::Players => "Players",
            Self::Clans => "Clans",
        }
    }
}
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
    sub_tab: SubTab,
    period: TimePeriod,
    sort: Sort,
    clan_sort: ClanSort,
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
            sub_tab: SubTab::Players,
            period: TimePeriod::default(),
            sort: Sort::default(),
            clan_sort: ClanSort::default(),
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

    fn clans(&self) -> Vec<ClanRow> {
        clan_rows(&self.players, &self.filter_text, self.clan_sort)
    }

    fn visible_len(&self) -> usize {
        match self.sub_tab {
            SubTab::Players => self.rows().len(),
            SubTab::Clans => self.clans().len(),
        }
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        self.list_state.reset(self.visible_len());
        cx.notify();
    }

    fn set_sub_tab(&mut self, sub_tab: SubTab, cx: &mut Context<Self>) {
        if self.sub_tab == sub_tab {
            return;
        }
        self.sub_tab = sub_tab;
        self.sync_rows(cx);
    }

    fn sort_clans_by(&mut self, column: ClanSortColumn, cx: &mut Context<Self>) {
        self.clan_sort = self.clan_sort.toggled(column);
        self.sync_rows(cx);
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

/// One column of a sortable header.
struct HeaderCell {
    /// Distinguishes the two tables' headers, which are never on screen at
    /// once but must not share ids.
    id_prefix: &'static str,
    index: usize,
    label: &'static str,
    width: Pixels,
    /// Whether this is the column the table is currently ordered by.
    active: bool,
    order: SortOrder,
}

/// Renders a header cell, with the arrow on whichever column is active.
///
/// Shared by both tables: they sort different columns, but a header behaves
/// the same either way.
fn sort_header(
    cell: HeaderCell,
    on_click: impl Fn(&mut PlayerTrackerView, &mut Context<PlayerTrackerView>) + 'static,
    cx: &mut Context<PlayerTrackerView>,
) -> AnyElement {
    let HeaderCell { id_prefix, index, label, width, active, order } = cell;
    selectable(
        (id_prefix, index),
        active,
        div()
            .id((id_prefix, index + 1000))
            .w(width)
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .child(h_flex().gap_1().items_center().child(label).when(active, |this| {
                this.child(Icon::new(match order {
                    SortOrder::Ascending => IconName::SortAscending,
                    SortOrder::Descending => IconName::SortDescending,
                }))
            }))
            .on_click(cx.listener(move |this, _event, _window, cx| on_click(this, cx))),
    )
    .into_any_element()
}

impl Render for PlayerTrackerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;
        let pool = crate::settings_store::pool(cx);

        let sub_tabs = SubTab::ALL.map(|sub_tab| {
            let chosen = self.sub_tab == sub_tab;
            selectable(
                ("tracker-subtab", sub_tab as usize),
                chosen,
                Button::new(("tracker-subtab-button", sub_tab as usize))
                    .label(sub_tab.label())
                    .compact()
                    .selected(chosen)
                    .on_click(cx.listener(move |this, _event, _window, cx| this.set_sub_tab(sub_tab, cx))),
            )
        });

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
            .children(sub_tabs)
            .child(div().w(px(8.)))
            .children(period_buttons)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(Icon::new(IconName::Search))
                    .child(div().w(px(220.)).child(Input::new(&self.filter_input).id("tracker-filter").small())),
            );

        let header_cells: Vec<AnyElement> = match self.sub_tab {
            SubTab::Players => SortColumn::ALL
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    let column = *column;
                    let width = match column {
                        SortColumn::Name => NAME_COLUMN_WIDTH,
                        SortColumn::Clan => CLAN_COLUMN_WIDTH,
                        SortColumn::Encounters => COUNT_COLUMN_WIDTH,
                    };
                    sort_header(
                        HeaderCell {
                            id_prefix: "tracker-sort",
                            index,
                            label: column.label(),
                            width,
                            active: self.sort.column == column,
                            order: self.sort.order,
                        },
                        move |this, cx| this.sort_by(column, cx),
                        cx,
                    )
                })
                .collect(),
            SubTab::Clans => ClanSortColumn::ALL
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    let column = *column;
                    let width = match column {
                        ClanSortColumn::Clan => CLAN_TAG_COLUMN_WIDTH,
                        ClanSortColumn::Members => MEMBERS_COLUMN_WIDTH,
                        ClanSortColumn::Encounters => COUNT_COLUMN_WIDTH,
                    };
                    sort_header(
                        HeaderCell {
                            id_prefix: "tracker-clan-sort",
                            index,
                            label: column.label(),
                            width,
                            active: self.clan_sort.column == column,
                            order: self.clan_sort.order,
                        },
                        move |this, cx| this.sort_clans_by(column, cx),
                        cx,
                    )
                })
                .collect(),
        };

        let header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .children(header_cells);

        let players = self.rows();
        let clans = self.clans();
        let sub_tab = self.sub_tab;
        let render_row = move |ix: usize, _window: &mut Window, _cx: &mut App| match sub_tab {
            SubTab::Players => {
                let Some(row) = players.get(ix) else {
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
            }
            SubTab::Clans => {
                let Some(row) = clans.get(ix) else {
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
                    .child(div().w(CLAN_TAG_COLUMN_WIDTH).text_sm().child(row.clan.clone()))
                    .child(div().w(MEMBERS_COLUMN_WIDTH).text_sm().child(row.members_met.to_string()))
                    .child(div().w(COUNT_COLUMN_WIDTH).text_sm().child(row.encounters.to_string()))
                    .into_any_element()
            }
        };

        let status = match &self.state {
            LoadState::Idle => Some("Waiting for the replay index".to_string()),
            LoadState::Loading => Some("Loading players...".to_string()),
            LoadState::Failed(reason) => Some(format!("Could not read the index: {reason}")),
            LoadState::Loaded if self.visible_len() == 0 => match self.sub_tab {
                SubTab::Players => Some("No players indexed for this period".to_string()),
                SubTab::Clans => Some("No clans met in this period".to_string()),
            },
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
