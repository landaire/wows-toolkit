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
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use jiff::Timestamp;
use sqlx::sqlite::SqlitePool;
use wows_toolkit_viewmodel::player_tracker::ClanRow;

use wows_replays::types::AccountId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::player_tracker::ClanSort;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::Sort;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::SortOrder;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::clan_rows;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live::LiveRosterRow;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wows_toolkit_viewmodel::player_tracker::live::ResolvedRoster;
use wows_toolkit_viewmodel::player_tracker::live::build_name_index;
use wows_toolkit_viewmodel::player_tracker::live::resolve_roster;
use wows_toolkit_viewmodel::player_tracker::visible_players;

use crate::replay_inspector::GameDataCache;
use crate::replay_inspector::LoadedGameData;
use crate::replay_inspector::columns::ColorRole;
use crate::replay_inspector::columns::PlayerColorKind;
use crate::replay_inspector::table::resolve_color;
use crate::runtime;
use crate::ui::selectable;

const ROW_HEIGHT: Pixels = px(24.);
const LIST_OVERDRAW: Pixels = px(200.);
const NAME_COLUMN_WIDTH: Pixels = px(220.);
const CLAN_TAG_COLUMN_WIDTH: Pixels = px(160.);
const MEMBERS_COLUMN_WIDTH: Pixels = px(120.);
const SHIP_COLUMN_WIDTH: Pixels = px(130.);
const MET_COLUMN_WIDTH: Pixels = px(80.);
const STAT_COLUMN_WIDTH: Pixels = px(64.);

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

/// Where the stats lookup for the battle in progress has got to.
///
/// The service's budget is small, so each battle is asked about once and the
/// answer held for its duration; a failure is reported rather than retried.
enum StatsState {
    /// No battle, or one whose roster has not been read yet.
    Idle,
    /// Waiting for the game to flush the packet that names the roster.
    Scanning,
    Fetching,
    Ready(HashMap<AccountId, PlayerStatsOut>),
    Failed(String),
}

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
    /// Identities the packet scan recovered for the battle in progress:
    /// account ids, realms and clans, none of which `tempArenaInfo` carries.
    live_identities: Option<LiveIdentities>,
    /// Where the stats lookup for the battle in progress has got to.
    stats: StatsState,
    /// The replays directory being watched, so the scan knows where the live
    /// packet stream is.
    replay_dir: Option<PathBuf>,
    /// The proxy the stats lookup goes through, as the settings hold it.
    /// Normalized and interpreted by `http::client`, which is where an unset
    /// or malformed value is decided.
    proxy_url: String,
    _live_scan: Option<Task<()>>,
    _live_build_load: Option<Task<()>>,
    /// What this session has already asked the stats service, so a battle is
    /// asked about once and the service's budget is respected locally.
    stats_budget: live::StatsBudget,
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
    /// Everyone the index has ever seen, for the live roster's "met before"
    /// join. Loaded once: it must not follow the period selector.
    all_time_players: Vec<PlayerFacet>,
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
            live_identities: None,
            stats: StatsState::Idle,
            replay_dir: None,
            proxy_url: String::new(),
            _live_scan: None,
            _live_build_load: None,
            stats_budget: live::StatsBudget::default(),
            _live_watch: None,
            period: TimePeriod::default(),
            sort: Sort::default(),
            clan_sort: ClanSort::default(),
            filter_text: String::new(),
            filter_input,
            players: Vec::new(),
            all_time_players: Vec::new(),
            state: LoadState::Idle,
            generation: 0,
            list_state: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription],
        }
    }

    /// Queries the index for the current period. Called once the config
    /// database is open, and again whenever the period changes.
    /// Loads every account the index has ever seen, for the live roster's
    /// "met before" column. Runs once per session: the set only grows, and a
    /// player met for the first time this battle was not "met before" anyway.
    fn load_all_time_players(&mut self, pool: SqlitePool, cx: &mut Context<Self>) {
        if !self.all_time_players.is_empty() {
            return;
        }

        let filter = TimePeriod::AllTime.match_filter(Timestamp::now());
        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move { query::distinct_players(&pool, &filter).await }).await;
            let _ = this.update(cx, |this, cx| {
                match found {
                    Ok(Ok(players)) => this.all_time_players = players,
                    // The roster still lists everyone; only "met before"
                    // stays empty.
                    Ok(Err(err)) => tracing::warn!("player tracker: the all-time join did not load: {err}"),
                    Err(err) => tracing::warn!("player tracker: the all-time join did not complete: {err}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn refresh(&mut self, pool: SqlitePool, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.state = LoadState::Loading;
        self.load_all_time_players(pool.clone(), cx);
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
        // Every account ever indexed, not the period in view: "met before"
        // must not change when the period selector does. The index carries
        // only the name each account currently goes by, so a rename misses.
        let name_index = build_name_index(
            self.all_time_players.len(),
            std::iter::empty(),
            self.all_time_players.iter().map(|player| (player.account_id, player.latest_name.as_str())),
        );
        Some(resolve_roster(
            live,
            &name_index,
            self.live_identities.as_ref(),
            self.live_metadata.as_deref().map(|data| data.provider().as_ref()),
        ))
    }

    /// Starts watching `replay_dir` for a battle in progress, replacing any
    /// watch already running. Called when the WoWs directory becomes known
    /// and whenever it changes.
    pub fn watch_live_matches(
        &mut self,
        replay_dir: PathBuf,
        game_data: GameDataCache,
        proxy_url: String,
        cx: &mut Context<Self>,
    ) {
        self.game_data = Some(game_data);
        self.replay_dir = Some(replay_dir.clone());
        self.proxy_url = proxy_url;
        self.live_checked = false;
        self.live_match = None;
        self.clear_live_match_data();

        // Weak: the task is owned by this view, and a strong handle here
        // would be a cycle that keeps the whole tab alive for the process.
        let entity = cx.entity().downgrade();
        self._live_watch = Some(live::watch(replay_dir, cx, move |live, cx| {
            entity.update(cx, |this, cx| this.adopt_live_match(live, cx)).is_ok()
        }));
        cx.notify();
    }

    /// Adopts what a poll found, and loads the roster's build if it is one
    /// the resolved data does not already cover.
    fn adopt_live_match(&mut self, live: Option<LiveMatch>, cx: &mut Context<Self>) {
        self.live_checked = true;
        let started_at = live.as_ref().map(|live| live.started_at);
        let build = live.as_ref().and_then(|live| live.build);
        self.live_match = live;
        // A new battle carries its own roster and its own stats; the previous
        // battle's must not show under it.
        self.clear_live_match_data();
        cx.notify();

        let (Some(started_at), Some(build)) = (started_at, build) else { return };
        let Some(game_data) = self.game_data.clone() else { return };

        if self.live_metadata_build == Some(build) && self.live_metadata.is_some() {
            self.start_live_scan(started_at, cx);
            return;
        }

        self.live_metadata_build = Some(build);
        self.live_metadata = None;
        self.stats = StatsState::Scanning;
        // Held rather than detached: a battle that ends while its build is
        // still loading drops this with the rest of that battle's state.
        self._live_build_load = Some(cx.spawn(async move |this, cx| {
            let loaded = cx.background_spawn(async move { game_data.get_or_load_build(build) }).await;
            let _ = this.update(cx, |this, cx| {
                if this.live_started_at() != Some(started_at) {
                    return;
                }
                match loaded {
                    Ok(data) => {
                        this.live_metadata = Some(data);
                        this.start_live_scan(started_at, cx);
                    }
                    // The roster still lists names and relations; the ship
                    // columns and the statistics have nothing to resolve
                    // against, which is a failure rather than a wait.
                    Err(err) => {
                        tracing::warn!("live match: build {build} did not load: {err}");
                        this.live_metadata_build = None;
                        this.stats = StatsState::Failed(format!("this battle's game data did not load: {err}"));
                    }
                }
                cx.notify();
            });
        }));
    }

    /// When the battle in progress started, which keys every write a scan or
    /// a lookup makes: one that outlives its battle must not land on the next.
    fn live_started_at(&self) -> Option<Timestamp> {
        self.live_match.as_ref().map(|live| live.started_at)
    }

    /// Drops everything that belonged to the previous battle, and stops its
    /// scan if one was still running.
    fn clear_live_match_data(&mut self) {
        self.live_identities = None;
        self.stats = StatsState::Idle;
        self._live_scan = None;
        self._live_build_load = None;
    }

    /// Reads the battle's roster off the live packet stream, then asks the
    /// stats service about it.
    ///
    /// The stream arrives in flushes, so the scan retries on a timer until
    /// the packet naming the roster lands or the budget runs out. Every write
    /// is keyed on `started_at`, so a scan that outlives its battle is
    /// dropped rather than landing on the next one.
    fn start_live_scan(&mut self, started_at: Timestamp, cx: &mut Context<Self>) {
        let (Some(replay_dir), Some(metadata)) = (self.replay_dir.clone(), self.live_metadata.clone()) else {
            return;
        };
        self.stats = StatsState::Scanning;
        let proxy_url = self.proxy_url.clone();
        cx.notify();

        self._live_scan = Some(cx.spawn(async move |this, cx| {
            let source = live::LiveSource::in_dir(&replay_dir);
            let deadline = Instant::now() + live::SCAN_RETRY_BUDGET;

            let state = loop {
                let attempt = {
                    let source = source.clone();
                    let metadata = metadata.clone();
                    cx.background_spawn(async move { live::scan_arena(&source, metadata.provider()) }).await
                };
                if let Some(state) = attempt {
                    break Some(state);
                }
                if Instant::now() >= deadline {
                    break None;
                }
                cx.background_executor().timer(live::SCAN_RETRY_INTERVAL).await;
            };

            let Some(state) = state else {
                let _ = this.update(cx, |this, cx| {
                    if this.live_started_at() != Some(started_at) {
                        return;
                    }
                    this.stats = StatsState::Failed(live::StatsError::NoRoster.to_string());
                    cx.notify();
                });
                return;
            };

            // Written before the lookup, so the clan tags and the account
            // join light up even when the lookup then fails or is refused.
            let identities = LiveIdentities::from_player_states(&state.players);
            let arena_id = state.arena_id;
            let cleared = this
                .update(cx, |this, cx| {
                    if this.live_started_at() != Some(started_at) {
                        return false;
                    }
                    this.live_identities = Some(identities);

                    // An answer this session already has costs no request.
                    if let Some(answered) = this.stats_budget.answered(arena_id) {
                        this.stats = StatsState::Ready(index_by_account(answered.players.clone()));
                        cx.notify();
                        return false;
                    }

                    match this.stats_budget.check(arena_id, Instant::now()) {
                        Ok(()) => {
                            this.stats_budget.record(Instant::now());
                            this.stats = StatsState::Fetching;
                            cx.notify();
                            true
                        }
                        Err(refusal) => {
                            this.stats = StatsState::Failed(live::refusal_text(&refusal.into()));
                            cx.notify();
                            false
                        }
                    }
                })
                .unwrap_or(false);
            if !cleared {
                return;
            }

            let fetched = runtime::spawn(cx, async move { live::fetch_stats(&state, &proxy_url).await }).await;

            let _ = this.update(cx, |this, cx| {
                if this.live_started_at() != Some(started_at) {
                    return;
                }
                this.stats = match fetched {
                    Ok(Ok(response)) => {
                        let players = index_by_account(response.players.clone());
                        this.stats_budget.remember(response);
                        StatsState::Ready(players)
                    }
                    Ok(Err(err)) => {
                        this.stats_budget.note_failure(arena_id, &err, Instant::now());
                        StatsState::Failed(live::refusal_text(&err))
                    }
                    Err(err) => StatsState::Failed(err.to_string()),
                };
                cx.notify();
            });
        }));
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

    /// Seeds what a scan and a lookup would have produced. Test-only: in
    /// production both arrive through `start_live_scan`.
    #[cfg(test)]
    pub(crate) fn seed_live_stats(
        &mut self,
        identities: LiveIdentities,
        players: Vec<PlayerStatsOut>,
        cx: &mut Context<Self>,
    ) {
        self.live_identities = Some(identities);
        self.stats = StatsState::Ready(index_by_account(players));
        cx.notify();
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

        let note = match &self.stats {
            // A failure is reported even while the ships are unresolved: a
            // build that did not load is why they are unresolved, and saying
            // "loading" for it would never stop being wrong.
            StatsState::Failed(reason) => Some(format!("Player statistics are unavailable: {reason}")),
            _ if !roster.ships_resolved => {
                Some("Loading this build's ship data; names and classes fill in when it lands.".to_string())
            }
            StatsState::Scanning => Some("Reading the roster off the battle in progress...".to_string()),
            StatsState::Fetching => Some("Looking up player statistics...".to_string()),
            StatsState::Idle | StatsState::Ready(_) => None,
        };
        let stats = match &self.stats {
            StatsState::Ready(players) => Some(players),
            _ => None,
        };

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
                    .child(team_column("Allies", "ally", &roster.friendly, stats, border))
                    .child(div().w(px(1.)).h_full().bg(border))
                    .child(team_column("Enemies", "enemy", &roster.enemy, stats, border)),
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

/// The service's answer keyed by account, which is how a row looks its own
/// statistics up.
fn index_by_account(players: Vec<PlayerStatsOut>) -> HashMap<AccountId, PlayerStatsOut> {
    players.into_iter().map(|player| (player.account_id, player)).collect()
}

/// A live roster row's colour. The same table the replay inspector's player
/// names use, so one person reads the same in both tabs.
fn tint_color(tint: PlayerTint) -> Hsla {
    let kind = match tint {
        PlayerTint::SelfPlayer => PlayerColorKind::SelfPlayer,
        PlayerTint::Ally => PlayerColorKind::Ally,
        PlayerTint::Enemy => PlayerColorKind::Enemy,
        PlayerTint::DivisionMate => PlayerColorKind::DivisionMate,
        PlayerTint::Abuser => PlayerColorKind::Abuser,
    };
    resolve_color(ColorRole::Player(kind))
}

/// One team's roster column, with its own header row.
fn team_column(
    title: &'static str,
    side: &'static str,
    rows: &[LiveRosterRow],
    stats: Option<&HashMap<AccountId, PlayerStatsOut>>,
    border: Hsla,
) -> AnyElement {
    let header = h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(border)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .child(div().flex_1().min_w(px(0.)).child(title))
        .child(div().w(SHIP_COLUMN_WIDTH).child("Ship"))
        .child(div().w(STAT_COLUMN_WIDTH).child("Win rate"))
        .child(div().w(STAT_COLUMN_WIDTH).child("PR"))
        .child(div().w(STAT_COLUMN_WIDTH).child("Battles"))
        .child(div().w(MET_COLUMN_WIDTH).child("Seen"));

    v_flex()
        .flex_1()
        .min_w(px(0.))
        .child(header)
        .children(rows.iter().enumerate().map(|(index, row)| roster_row(side, index, row, stats)))
        .into_any_element()
}

/// A rating's colour band, the same one the replay inspector's PR column
/// uses. Absent when the service returned no rating for this player.
fn rating_color(pr: Option<f64>) -> Option<Hsla> {
    let category = PersonalRatingCategory::from_pr(pr?);
    Some(rgb(personal_rating::chip_text(category, true)).into())
}

/// A stats cell: the value when the service answered with one, a dash when it
/// did not, and nothing at all while the lookup is still running.
fn stat_cell(text: Option<String>, color: Option<Hsla>, pending: bool) -> AnyElement {
    let text = match (text, pending) {
        (Some(text), _) => text,
        (None, true) => String::new(),
        (None, false) => "-".to_string(),
    };

    div()
        .w(STAT_COLUMN_WIDTH)
        .text_xs()
        .when_some(color, |this, color| this.text_color(color))
        .when(color.is_none(), |this| this.opacity(0.6))
        .child(text)
        .into_any_element()
}

/// One live roster entry: the player, their ship, and whether they have been
/// met before.
fn roster_row(
    side: &'static str,
    index: usize,
    row: &LiveRosterRow,
    stats: Option<&HashMap<AccountId, PlayerStatsOut>>,
) -> AnyElement {
    let name = match row.clan.as_deref() {
        Some(clan) => format!("[{clan}] {}", row.name),
        None => row.name.clone(),
    };

    // A row whose account the scan never named cannot be looked up at all,
    // which is a different absence from a player the service had no data for.
    let player = row.account_id.and_then(|id| stats?.get(&id));
    let pending = stats.is_none();
    let hidden = player.is_some_and(|player| player.status != PlayerStatsStatus::Ok);
    let win_rate = player.and_then(|player| player.overall_win_rate).map(|rate| format!("{rate:.1}%"));
    let pr = player.and_then(|player| player.pr);
    let battles = player.and_then(|player| player.battles).map(|battles| battles.to_string());

    h_flex()
        // Keyed by position as well as name: bots repeat names within a team.
        .id(SharedString::from(format!("tracker-roster-{side}-{index}")))
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
        .child(stat_cell(if hidden { Some("hidden".to_string()) } else { win_rate }, None, pending))
        .child(stat_cell(pr.map(|pr| format!("{pr:.0}")), rating_color(pr), pending))
        .child(stat_cell(battles, None, pending))
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
    use wows_replays::types::AccountId;
    use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
    use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
    use wows_toolkit_viewmodel::match_stats::Region;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentity;

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

    /// The two halves a scan produces must actually reach the cells: the
    /// identities give a row its account, and only an account can look a
    /// player's statistics up.
    #[gpui_kit::test]
    async fn a_scanned_identity_joins_its_player_to_the_looked_up_statistics(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = temp_dir("stats-join");
        let window = cx.open_window(size(px(1000.), px(700.)), PlayerTrackerView::new);

        window
            .update(cx, |tracker, _window, cx| {
                tracker.set_sub_tab(SubTab::CurrentMatch, cx);
                tracker.watch_live_matches(dir.clone(), GameDataCache::new(dir.join("game")), String::new(), cx);
            })
            .expect("the window is open");

        std::fs::write(dir.join(super::live::ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is writable");
        cx.wait_for(window.into(), Duration::from_secs(30), |window, cx| {
            window.render_frame(cx);
            window.try_find("tracker-roster-ally-0").is_some()
        })
        .await;

        // Before the scan lands, no row has an account, so no row has stats.
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("tracker-roster-ally-0").label(), Some("Me"), "no clan tag without identities");
        })
        .expect("the window is open");

        let account = AccountId(4242);
        let identity = LiveIdentity {
            account_id: account,
            region: Some(Region::Eu),
            clan: Some("WTK".to_string()),
            clan_color: None,
        };
        let identities = LiveIdentities { by_name: [("me".to_string(), identity)].into_iter().collect() };
        let stats = vec![PlayerStatsOut {
            account_id: account,
            region: "eu".to_string(),
            ship_id: 100u64.into(),
            status: PlayerStatsStatus::Ok,
            battles: Some(1234),
            overall_win_rate: Some(54.25),
            overall_avg_damage: None,
            ship_win_rate: None,
            ship_battles: None,
            ship_avg_damage: None,
            ship_pr: None,
            pr: Some(1650.0),
        }];

        window
            .update(cx, |tracker, _window, cx| tracker.seed_live_stats(identities, stats, cx))
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("tracker-roster-ally-0").label(),
                Some("[WTK] Me"),
                "the scan's clan tag reaches the name"
            );
        })
        .expect("the window is open");

        let _ = std::fs::remove_dir_all(&dir);
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
                tracker.watch_live_matches(dir.clone(), GameDataCache::new(dir.join("game")), String::new(), cx);
            })
            .expect("the window is open");

        std::fs::write(dir.join(super::live::ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is writable");

        cx.wait_for(window.into(), Duration::from_secs(30), |window, cx| {
            window.render_frame(cx);
            window.try_find("tracker-roster-ally-0").is_some()
        })
        .await;

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("tracker-roster-ally-0").label(), Some("Me"), "the recording player is listed");
            assert_eq!(window.find("tracker-roster-enemy-0").label(), Some("Foe"), "so is the other team");
        })
        .expect("the window is open");

        // The battle ending removes the file, and the roster goes with it.
        std::fs::remove_file(dir.join(super::live::ARENA_INFO_FILE)).expect("the arena info is removable");
        cx.wait_for(window.into(), Duration::from_secs(30), |window, cx| {
            window.render_frame(cx);
            window.try_find("tracker-roster-ally-0").is_none()
        })
        .await;

        let _ = std::fs::remove_dir_all(&dir);
    }
}
