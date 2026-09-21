//! The Player Tracker tab: everyone met in indexed battles, over a chosen
//! window of time.
//!
//! Mirrors the egui tab's historical table: a period selector and a name
//! filter above a sortable table of players, their clan and how often they
//! have been met. The rows come from the shared replay index.

mod live;

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
use std::path::PathBuf;
use std::sync::Arc;

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
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live::LiveRosterRow;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wows_toolkit_viewmodel::player_tracker::live::ResolvedRoster;
use wows_toolkit_viewmodel::player_tracker::live::build_name_index;
use wows_toolkit_viewmodel::player_tracker::live::resolve_roster;
use wows_toolkit_viewmodel::player_tracker::visible_players;

use crate::replay_inspector::GameDataCache;
use crate::replay_inspector::LoadedGameData;
use crate::runtime;
use crate::ui::selectable;

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
const NAME_COLUMN_WIDTH: Pixels = px(220.);
const CLAN_TAG_COLUMN_WIDTH: Pixels = px(160.);
const MEMBERS_COLUMN_WIDTH: Pixels = px(120.);
const SHIP_COLUMN_WIDTH: Pixels = px(130.);
const MET_COLUMN_WIDTH: Pixels = px(80.);

/// Which table the tab is showing.
///
/// Both read the same loaded players, so switching is a re-render rather than
/// another query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubTab {
    Players,
    /// The battle in progress, which is a different source entirely: the
    /// game's own `tempArenaInfo.json` rather than the index.
    CurrentMatch,
    Clans,
}

impl SubTab {
    /// The order the egui app lists them in.
    const ALL: [SubTab; 3] = [SubTab::Players, SubTab::CurrentMatch, SubTab::Clans];

    fn label(self) -> &'static str {
        match self {
            Self::Players => "Players",
            Self::CurrentMatch => "Current Match",
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
    /// The battle in progress, `None` when there is none. Filled in by the
    /// poll in `live`, which starts once the replays directory is known.
    live_match: Option<LiveMatch>,
    /// Whether the poll has reported at all yet, so "no battle" is not shown
    /// before the first look at the directory.
    live_checked: bool,
    /// Game data for the roster's build, so ship names and classes resolve.
    /// Shared with the replay inspector rather than loaded a second time.
    game_data: Option<GameDataCache>,
    /// Resolved ship data for `live_match`'s build, once it has loaded.
    live_metadata: Option<Arc<LoadedGameData>>,
    /// The build `live_metadata` was loaded for, so a new battle on another
    /// build reloads rather than resolving against the wrong one.
    live_metadata_build: Option<u32>,
    _live_watch: Option<Task<()>>,
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
            live_match: None,
            live_checked: false,
            game_data: None,
            live_metadata: None,
            live_metadata_build: None,
            _live_watch: None,
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
            // The roster is two short teams drawn side by side, not a
            // virtualized list, so it contributes no rows to `list_state`.
            SubTab::CurrentMatch => 0,
            SubTab::Clans => self.clans().len(),
        }
    }

    /// The live roster joined to game data and tracked history.
    ///
    /// Recomputed per render rather than cached: it is at most 24 rows, and
    /// caching it would need invalidating on every one of the four inputs.
    fn live_roster(&self) -> Option<ResolvedRoster> {
        let live = self.live_match.as_ref()?;
        // The index carries only the name each account currently goes by:
        // the replay index records no aliases, so a rename simply misses.
        let name_index = build_name_index(
            std::iter::empty(),
            self.players.iter().map(|player| (player.account_id, player.latest_name.as_str())),
        );
        Some(resolve_roster(
            live,
            &name_index,
            None,
            self.live_metadata.as_deref().map(|data| data.provider().as_ref()),
        ))
    }

    /// Starts watching `replay_dir` for a battle in progress, replacing any
    /// watch already running. Called when the WoWs directory becomes known
    /// and whenever it changes.
    pub fn watch_live_matches(&mut self, replay_dir: PathBuf, game_data: GameDataCache, cx: &mut Context<Self>) {
        self.game_data = Some(game_data);
        self.live_checked = false;
        self.live_match = None;

        let entity = cx.entity();
        self._live_watch = Some(live::watch(replay_dir, cx, move |live, cx| {
            entity.update(cx, |this, cx| this.adopt_live_match(live, cx));
        }));
        cx.notify();
    }

    /// Adopts what a poll found, and loads the roster's build if it is one
    /// the resolved data does not already cover.
    fn adopt_live_match(&mut self, live: Option<LiveMatch>, cx: &mut Context<Self>) {
        self.live_checked = true;
        let build = live.as_ref().and_then(|live| live.build);
        self.live_match = live;
        cx.notify();

        let Some(build) = build else { return };
        if self.live_metadata_build == Some(build) {
            return;
        }
        let Some(game_data) = self.game_data.clone() else { return };

        self.live_metadata_build = Some(build);
        self.live_metadata = None;
        cx.spawn(async move |this, cx| {
            let loaded = cx.background_spawn(async move { game_data.get_or_load_build(build) }).await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(data) => this.live_metadata = Some(data),
                    // The roster still lists names and relations; only the
                    // ship columns stay empty.
                    Err(err) => tracing::warn!("live match: build {build} did not load: {err}"),
                }
                cx.notify();
            });
        })
        .detach();
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

    /// The Current Match body: the roster when a battle is under way, and
    /// what is missing when it is not.
    fn render_current_match(&self, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;

        let status = match (&self.game_data, self.live_checked, &self.live_match) {
            (None, _, _) => Some("Set the World of Warships directory to watch for battles"),
            (_, false, _) => Some("Checking for a battle in progress..."),
            (_, true, None) => Some("No battle in progress"),
            (_, true, Some(_)) => None,
        };

        if let Some(status) = status {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().opacity(0.6).child(status))
                .into_any_element();
        }

        let Some(roster) = self.live_roster() else {
            return div().into_any_element();
        };

        let note = (!roster.ships_resolved)
            .then_some("Loading this build's ship data; names and classes fill in when it lands.");

        v_flex()
            .size_full()
            .when_some(note, |this, note| {
                this.child(div().flex_none().px_2().py_1().text_xs().opacity(0.6).child(note))
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_start()
                    .child(team_column("Allies", &roster.friendly, border))
                    .child(div().w(px(1.)).h_full().bg(border))
                    .child(team_column("Enemies", &roster.enemy, border)),
            )
            .into_any_element()
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

/// Colours for a live roster row, matching the replay inspector's player
/// colours so the same person reads the same in both tabs.
fn tint_color(tint: PlayerTint) -> Hsla {
    let packed = match tint {
        PlayerTint::SelfPlayer => 0xffd700,
        PlayerTint::Ally => 0x90ee90,
        PlayerTint::Enemy => 0xff8080,
        PlayerTint::DivisionMate => 0x00bfff,
        PlayerTint::Abuser => 0xff00ff,
    };
    rgb(packed).into()
}

/// One team's roster column.
fn team_column(title: &'static str, rows: &[LiveRosterRow], border: Hsla) -> AnyElement {
    v_flex()
        .flex_1()
        .min_w(px(0.))
        .child(
            div().px_2().py_1().border_b_1().border_color(border).text_xs().font_weight(FontWeight::BOLD).child(title),
        )
        .children(rows.iter().map(roster_row))
        .into_any_element()
}

/// One live roster entry: the player, their ship, and whether they have been
/// met before.
fn roster_row(row: &LiveRosterRow) -> AnyElement {
    let name = match row.clan.as_deref() {
        Some(clan) => format!("[{clan}] {}", row.name),
        None => row.name.clone(),
    };

    h_flex()
        .id(SharedString::from(format!("tracker-roster-{}", row.name)))
        .test_support()
        .aria_label(name.clone())
        .w_full()
        .h(ROW_HEIGHT)
        .gap_2()
        .items_center()
        .px_2()
        .child(div().flex_1().min_w(px(0.)).text_sm().text_color(tint_color(row.tint)).truncate().child(name))
        .child(
            div()
                .w(SHIP_COLUMN_WIDTH)
                .text_xs()
                .opacity(0.8)
                .truncate()
                .child(row.ship_name.clone().unwrap_or_else(|| "-".to_string())),
        )
        .child(div().w(MET_COLUMN_WIDTH).text_xs().opacity(0.6).child(if row.tracked.is_some() {
            "met before"
        } else {
            ""
        }))
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

        // The Current Match roster is its own layout: two teams side by
        // side, each with its own header, so the shared table chrome below
        // does not apply to it.
        if self.sub_tab == SubTab::CurrentMatch {
            return v_flex()
                .id("tracker-root")
                .track_focus(&self.focus_handle)
                .size_full()
                .child(toolbar)
                .child(div().flex_1().min_h(px(0.)).child(self.render_current_match(cx)))
                .into_any_element();
        }

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
            SubTab::CurrentMatch => unreachable!("the roster returns above"),
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
            SubTab::CurrentMatch => unreachable!("the roster returns above"),
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
                SubTab::CurrentMatch => unreachable!("the roster returns above"),
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
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestAppContextExt;
    use gpui_kit::test::TestWindowExt;
    use std::time::Duration;

    use super::PlayerTrackerView;
    use super::SubTab;
    use crate::replay_inspector::GameDataCache;

    /// A `tempArenaInfo.json` naming two players on opposite teams.
    const ARENA_INFO: &str = r#"{
        "gameMode": 7,
        "clientVersionFromExe": "13, 11, 0, 12668706",
        "mapDisplayName": "ocean",
        "mapId": 1,
        "clientVersionFromXml": "13, 11, 0, 12668706",
        "duration": 1200,
        "gameLogic": null,
        "name": "12x12",
        "scenario": "Domination",
        "playerID": 0,
        "vehicles": [{"shipId": 100, "relation": 0, "id": 1, "name": "Me"},
                     {"shipId": 200, "relation": 2, "id": 2, "name": "Foe"}],
        "playersPerTeam": 12,
        "dateTime": "28.12.2023 00:52:26",
        "mapName": "spaces/00_CO_ocean",
        "playerName": "Me",
        "scenarioConfigId": 1,
        "teamsCount": 2,
        "logic": null,
        "playerVehicle": "PFSD110-Kleber"
    }"#;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wt-gpui-tracker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is creatable");
        dir
    }

    /// Drives the real poll against a directory the test writes into, so the
    /// tab's "a battle started" path is exercised end to end rather than by
    /// calling the setter directly. The game data directory is deliberately
    /// bogus: ship names stay unresolved, which the roster already handles,
    /// and the test stays off any real install.
    #[gpui_kit::test]
    async fn a_battle_appearing_in_the_replays_directory_fills_the_roster(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = temp_dir("live");
        let window = cx.open_window(size(px(1000.), px(700.)), PlayerTrackerView::new);

        window
            .update(cx, |tracker, _window, cx| {
                tracker.set_sub_tab(SubTab::CurrentMatch, cx);
                tracker.watch_live_matches(dir.clone(), GameDataCache::new(dir.join("game")), cx);
            })
            .expect("the window is open");

        std::fs::write(dir.join(super::live::ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is writable");

        cx.wait_for(window.into(), Duration::from_secs(30), |window, cx| {
            window.render_frame(cx);
            window.try_find("tracker-roster-Me").is_some()
        })
        .await;

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("tracker-roster-Me").label(), Some("Me"), "the recording player is listed");
            assert!(window.try_find("tracker-roster-Foe").is_some(), "so is the other team");
        })
        .expect("the window is open");

        // The battle ending removes the file, and the roster goes with it.
        std::fs::remove_file(dir.join(super::live::ARENA_INFO_FILE)).expect("the arena info is removable");
        cx.wait_for(window.into(), Duration::from_secs(30), |window, cx| {
            window.render_frame(cx);
            window.try_find("tracker-roster-Me").is_none()
        })
        .await;

        let _ = std::fs::remove_dir_all(&dir);
    }
}
