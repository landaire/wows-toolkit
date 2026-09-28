//! The Player Tracker tab: everyone met in indexed battles, over a chosen
//! window of time.
//!
//! Mirrors the egui tab's historical table: a period selector and a name
//! filter above a sortable table of players, their clan and how often they
//! have been met. The rows come from the shared replay index.

mod live;
mod panels;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::dock::DockSkin;
use gpui_kit::component::dock::PanelId;
use gpui_kit::component::dock::panel_handle;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use jiff::Timestamp;
use sqlx::sqlite::SqlitePool;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::player_tracker::clans::ClanRow;

use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::PopupMenuItem;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query_text;
use wows_toolkit_config::index::rows::ClanCorrection;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::player_tracker::ClanSort;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::PlayerRow;
use wows_toolkit_viewmodel::player_tracker::Sort;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::SortOrder;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::player_tracker::clans::build_clan_breakdown;
use wows_toolkit_viewmodel::player_tracker::clans::visible_clans;
use wows_toolkit_viewmodel::player_tracker::history;
use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live::LiveRosterRow;
use wows_toolkit_viewmodel::player_tracker::live::PlayerTint;
use wows_toolkit_viewmodel::player_tracker::live::ResolvedRoster;
use wows_toolkit_viewmodel::player_tracker::live::RowStats;
use wows_toolkit_viewmodel::player_tracker::live::TrackedIndex;
use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;
use wows_toolkit_viewmodel::player_tracker::live::resolve_roster;
use wows_toolkit_viewmodel::player_tracker::live::row_stats;
use wows_toolkit_viewmodel::player_tracker::live::visible_stat_modes;
use wows_toolkit_viewmodel::player_tracker::shipbuilds_player_url;
use wows_toolkit_viewmodel::player_tracker::store;
use wows_toolkit_viewmodel::player_tracker::tracked;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::player_tracker::visible_player_rows;
use wows_toolkit_viewmodel::player_tracker::wows_numbers_player_url;
use wows_toolkit_viewmodel::query_bar::seed;
use wows_toolkit_viewmodel::twitch;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use wowsunpack::game_params::types::Species;

use crate::replay_inspector::GameDataCache;
use crate::replay_inspector::IconCache;
use crate::replay_inspector::LoadedGameData;
use crate::replay_inspector::columns::ColorRole;
use crate::replay_inspector::columns::PlayerColorKind;
use crate::replay_inspector::columns::player_color_kind_rgb;
use crate::replay_inspector::table::resolve_color;
use crate::runtime;
use crate::ui::selectable;

const ROW_HEIGHT: Pixels = px(24.);
const NAME_COLUMN_WIDTH: Pixels = px(220.);
const CLAN_TAG_COLUMN_WIDTH: Pixels = px(160.);
const MEMBERS_COLUMN_WIDTH: Pixels = px(120.);
const SHIP_COLUMN_WIDTH: Pixels = px(130.);
const MET_COLUMN_WIDTH: Pixels = px(80.);
const STAT_COLUMN_WIDTH: Pixels = px(64.);
const CLASS_COLUMN_WIDTH: Pixels = px(16.);
const TWITCH_MENU_WIDTH: Pixels = px(180.);
const CHIP_COLUMN_WIDTH: Pixels = px(24.);
/// How often the chat observations are re-read while a battle is under way.
///
/// The egui app's chat poll writes them every two minutes, so reading faster
/// than that only repeats the same rows.
const CHAT_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Which table the tab is showing.
///
/// Both read the same loaded players, so switching is a re-render rather than
/// another query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubTab {
    Players,
    /// The battle in progress, which is a different source entirely: the
    /// game's own `tempArenaInfo.json` rather than the index.
    CurrentMatch,
    Clans,
}

impl SubTab {
    /// The order the egui app lists them in.
    pub(crate) const ALL: [SubTab; 3] = [SubTab::Players, SubTab::CurrentMatch, SubTab::Clans];

    pub(crate) const fn label_key(self) -> &'static str {
        match self {
            Self::Players => "ui.player_tracker.subtab_players",
            Self::CurrentMatch => "ui.player_tracker.subtab_current_match",
            Self::Clans => "ui.player_tracker.subtab_clans",
        }
    }
}
const CLAN_COLUMN_WIDTH: Pixels = px(120.);
const COUNT_COLUMN_WIDTH: Pixels = px(110.);
/// Wide enough for "Encounters in Time Range" and the figure under it.
const RANGE_COUNT_COLUMN_WIDTH: Pixels = px(170.);
/// Wide enough for a relative age such as "2 months, 3 days ago".
const LAST_SEEN_COLUMN_WIDTH: Pixels = px(200.);

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

/// Raised for the app to act on.
#[derive(Clone, Debug)]
pub enum PlayerTrackerEvent {
    /// Show the Search tab, holding this query. Raised by the "find matches"
    /// buttons, which look a player or a clan up in the replay index rather
    /// than in the tracker's own tables.
    SearchFor(String),
}

impl EventEmitter<PlayerTrackerEvent> for PlayerTrackerView {}

/// The tone a number of encounters is read in, or none for a number not
/// worth marking (`wows_toolkit_viewmodel::player_tracker::
/// encounter_severity`).
fn severity_color(times: usize) -> Option<Hsla> {
    let semantic = crate::theme::semantic();
    let packed = match wows_toolkit_viewmodel::player_tracker::encounter_severity(times) {
        wows_toolkit_viewmodel::player_tracker::EncounterSeverity::None => return None,
        wows_toolkit_viewmodel::player_tracker::EncounterSeverity::Noted => semantic.division,
        wows_toolkit_viewmodel::player_tracker::EncounterSeverity::Warned => semantic.warn,
        wows_toolkit_viewmodel::player_tracker::EncounterSeverity::Heavy => semantic.loss,
    };
    Some(rgb(packed).into())
}

/// What the name filter is given: enough to read a name in.
const FILTER_WIDTH: Pixels = px(220.);

/// The period combo, and the menu under it.
const PERIOD_COMBO_WIDTH: Pixels = px(150.);

/// One entry in the period combo. A local newtype: `TimePeriod` is shared
/// with the egui app and `SearchableListItem` is the component library's.
#[derive(Clone)]
struct PeriodItem(TimePeriod);

impl SearchableListItem for PeriodItem {
    type Value = TimePeriod;

    fn title(&self) -> SharedString {
        SharedString::from(t!(self.0.label_key()).into_owned())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }
}

fn period_index(period: TimePeriod) -> usize {
    TimePeriod::ALL.iter().position(|offered| *offered == period).expect("TimePeriod::ALL lists every period")
}

/// Where the tab is in loading the index.
enum LoadState {
    /// Before the config database is available.
    Idle,
    Loading,
    Failed(String),
    Loaded,
}

/// The settings row the tracker keeps its view in.
const TRACKER_SETTINGS_KEY: &str = "player_tracker";

/// What the tracker remembers between sessions: the window of time it looks
/// back over, how each table is ordered, and what the filter box holds.
///
/// The egui tracker keeps none of this, so this row is the port's own rather
/// than one the two apps share.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct TrackerSettings {
    #[serde(default)]
    period: TimePeriod,
    #[serde(default)]
    sort: Sort,
    #[serde(default)]
    clan_sort: ClanSort,
    #[serde(default)]
    filter: String,
}

pub struct PlayerTrackerView {
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
    /// Twitch logins seen in chat around the battle in progress, and when.
    /// Read from the shared index, which the egui app's chat poll fills: a
    /// user who has connected Twitch there gets the chips here too.
    chat_observations: Vec<(String, Timestamp)>,
    /// The chip's candidates per roster name, rebuilt when the chat or the
    /// roster changes. Matching is a Levenshtein pass over every observation
    /// for every row, which is too much to redo on every frame.
    twitch_candidates: HashMap<String, Vec<SniperCandidate>>,
    _chat_watch: Option<Task<()>>,
    /// Notes kept against the players met, keyed by account. Shared with the
    /// egui app through the `player_tracker_data` blob, so a note written in
    /// either is the note both show.
    tracked: HashMap<AccountId, TrackedPlayer>,
    /// The index's latest clan per account, which wins over the tracker's
    /// wherever the index knows the account, and the per-encounter
    /// corrections for accounts whose clan at the time differed.
    clan_latest: HashMap<AccountId, String>,
    clan_corrections: Vec<ClanCorrection>,
    /// Whether the clans table counts battles the user arranged. Stored in
    /// the shared tracker blob, so the egui app opens on the same answer.
    show_division_mates: bool,
    /// The account whose note is open for editing, and the field holding it.
    editing_note: Option<AccountId>,
    /// Historical rows opened to show what the tracker recorded beyond the
    /// columns. Keyed by account so re-sorting carries the open state with
    /// the player rather than with the row position.
    expanded_players: HashSet<AccountId>,
    /// Clans rows opened to show the members met from them, keyed by tag for
    /// the same reason the players are keyed by account.
    expanded_clans: HashSet<String>,
    /// Why the last note did not save, shown beside the editor. `None` when
    /// the last write succeeded, which is also the state before any write.
    note_error: Option<String>,
    /// Bumped per save so a slower earlier one cannot land over a later.
    note_generation: u64,
    _note_save: Option<Task<()>>,
    note_input: Entity<InputState>,
    /// Ship-class icons for the roster, decoded from the battle's own build.
    /// Empty until that build's data loads; a row without one falls back to
    /// its ship name alone, which is what an older client with no icon does.
    icons: IconCache,
    /// How much of each player the roster shows, and which scope its figures
    /// come from when it shows one.
    view_mode: CurrentMatchViewMode,
    win_rate_mode: WinRateMode,
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
    /// Whether the saved view has been read back yet. One shot, on the first
    /// frame.
    view_loaded: bool,
    period_select: Entity<SelectState<SearchableVec<PeriodItem>>>,
    sort: Sort,
    clan_sort: ClanSort,
    filter_text: String,
    filter_input: Entity<InputState>,
    /// Everyone the index returned for the current period, unfiltered. The
    /// filter and sort are applied per render over this.
    players: Vec<PlayerFacet>,
    /// Which of the battle's players the index has met before, keyed by
    /// lower-cased name. Looked up per battle, all-time.
    met_before: HashMap<String, AccountId>,
    state: LoadState,
    /// Where the Clans table is in reading its own two all-time aggregates,
    /// which are far heavier than the period-filtered player query and so are
    /// read only once that table is shown (see [`Self::load_clan_inputs`]).
    clan_state: LoadState,
    /// Bumped per query so a slower earlier period cannot overwrite a later.
    generation: u64,
    /// The dock the three sections live in, so they can be split and docked.
    dock_area: Entity<DockArea>,
    /// The panels in it, in [`SubTab::ALL`] order, so a row count can be
    /// pushed to the one it belongs to.
    panels: Vec<Entity<panels::TrackerPanel>>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl PlayerTrackerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.player_tracker.filter_hint").to_string()));
        let periods = SearchableVec::new(TimePeriod::ALL.map(PeriodItem).to_vec());
        let period_select = cx.new(|cx| {
            SelectState::new(periods, Some(IndexPath::new(period_index(TimePeriod::default()))), window, cx)
                .searchable(false)
        });
        // `Confirm(None)` is the cleared-selection case, which this combo
        // cannot produce: it is not `.cleanable()` and always holds a period.
        let period_chosen = cx.subscribe(&period_select, |this, _state, event, cx| {
            let SelectEvent::Confirm(Some(period)) = event else { return };
            let pool = crate::settings_store::pool(cx);
            this.set_period(*period, pool, cx);
        });
        let note_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.player_tracker.notes_hint").to_string()));
        let subscription = cx.subscribe(&filter_input, Self::on_filter_event);
        let note_edited = cx.subscribe(&note_input, Self::on_note_edited);

        // The skinned area is what draws a tab bar over a group holding more
        // than one panel, which is what the three sections read as until one
        // is dragged out.
        let (dock_area, _) = DockSkin::dock_area("tracker-dock", None, window, cx);
        let this = cx.weak_entity();
        let panels: Vec<Entity<panels::TrackerPanel>> = SubTab::ALL
            .map(|section| {
                let panel = cx.new(|cx| panels::TrackerPanel::new(section, this.clone(), cx));
                dock_area.update(cx, |dock, cx| {
                    dock.add_panel_view(panel_handle(panel.clone()), DockPlacement::Center, None, window, cx);
                });
                panel
            })
            .to_vec();
        // Each add shows what it added, so the last would be on top; the tab
        // opens on its players, as the egui tracker does.
        if let Some(first) = panels.first() {
            let id = PanelId::from(first.entity_id());
            dock_area.update(cx, |dock, cx| dock.select_panel(id, window, cx));
        }

        Self {
            live_match: None,
            live_checked: false,
            game_data: None,
            live_metadata: None,
            live_metadata_build: None,
            live_identities: None,
            stats: StatsState::Idle,
            replay_dir: None,
            chat_observations: Vec::new(),
            twitch_candidates: HashMap::new(),
            _chat_watch: None,
            tracked: HashMap::new(),
            clan_latest: HashMap::new(),
            clan_corrections: Vec::new(),
            show_division_mates: false,
            editing_note: None,
            expanded_players: HashSet::new(),
            expanded_clans: HashSet::new(),
            note_error: None,
            note_generation: 0,
            _note_save: None,
            note_input,
            icons: IconCache::new(),
            view_mode: CurrentMatchViewMode::default(),
            win_rate_mode: WinRateMode::default(),
            proxy_url: String::new(),
            _live_scan: None,
            _live_build_load: None,
            stats_budget: live::StatsBudget::default(),
            _live_watch: None,
            period: TimePeriod::default(),
            view_loaded: false,
            period_select,
            sort: Sort::default(),
            clan_sort: ClanSort::default(),
            filter_text: String::new(),
            filter_input,
            players: Vec::new(),
            met_before: HashMap::new(),
            state: LoadState::Idle,
            clan_state: LoadState::Idle,
            generation: 0,
            dock_area,
            panels,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![subscription, note_edited, period_chosen],
        }
    }

    /// Queries the index for the current period. Called once the config
    /// database is open, and again whenever the period changes.
    /// Reads the Twitch chat observations around this battle, and keeps
    /// reading while the battle's window is still open.
    ///
    /// The writer is the egui app's chat poll, which records who was in chat
    /// every couple of minutes; almost all of this battle's window is still
    /// in the future when the battle starts, so a single read at that moment
    /// would find nothing. The task ends with the battle, or when the window
    /// closes.
    fn watch_chat_observations(&mut self, started_at: Timestamp, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };

        let start = started_at.as_second() + (twitch::WINDOW_BEFORE_MINUTES * 60.0) as i64;
        let end = started_at.as_second() + (twitch::WINDOW_AFTER_MINUTES * 60.0) as i64;

        self._chat_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                let pool = pool.clone();
                let found =
                    runtime::spawn(cx, async move { query::observations_in_window(&pool, start, end).await }).await;

                let still_current = this
                    .update(cx, |this, cx| {
                        if this.live_started_at() != Some(started_at) {
                            return false;
                        }
                        match found {
                            Ok(Ok(rows)) => {
                                this.chat_observations = rows
                                    .into_iter()
                                    .filter_map(|(login, seen_at)| {
                                        Timestamp::from_second(seen_at).ok().map(|seen_at| (login, seen_at))
                                    })
                                    .collect();
                                this.recompute_twitch_candidates();
                            }
                            // The roster still lists everyone; only the chips
                            // are missing.
                            Ok(Err(err)) => tracing::warn!("player tracker: the chat lookup failed: {err}"),
                            Err(err) => tracing::warn!("player tracker: the chat lookup did not complete: {err}"),
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);

                // Past the window there is nothing further to find, and the
                // battle it belonged to is over.
                if !still_current || Timestamp::now().as_second() > end {
                    return;
                }
                cx.background_executor().timer(CHAT_POLL_INTERVAL).await;
            }
        }));
    }

    /// Reads the players the two apps share. Called once the config database
    /// is open.
    fn load_tracked_players(&mut self, pool: SqlitePool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let read = runtime::spawn(cx, async move {
                // A database written before the tracker had tables still holds
                // it as one settings blob; whichever app opens first imports it.
                if let Err(err) = store::import_blob_once(&pool).await {
                    tracing::warn!("player tracker: the stored blob was not imported: {err}");
                }
                let players = store::load(&pool).await;
                let modes = store::load_view_modes(&pool).await;
                players.map(|players| (players, modes))
            })
            .await;

            match read {
                Ok(Ok((players, modes))) => {
                    let _ = this.update(cx, |this, cx| {
                        this.tracked = players;
                        this.win_rate_mode = modes.win_rate_mode;
                        this.view_mode = modes.current_match_view_mode;
                        this.show_division_mates = modes.show_division_mates;
                        cx.notify();
                    });
                }
                // Left unloaded rather than shown as empty: a note written from
                // here would then be the only row an account has.
                Ok(Err(err)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.note_error = Some(err.to_string());
                        cx.notify();
                    });
                    tracing::warn!("player tracker: the stored players could not be read: {err}");
                }
                Err(err) => tracing::warn!("player tracker: the read did not run: {err}"),
            }
        })
        .detach();
    }

    /// Seeds the chat observations a lookup would have returned. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_chat_observations(&mut self, observations: Vec<(String, Timestamp)>, cx: &mut Context<Self>) {
        self.chat_observations = observations;
        self.recompute_twitch_candidates();
        cx.notify();
    }

    /// Seeds the players the index returned and the notes kept against them.
    /// Test-only: production loads both from the database.
    #[cfg(test)]
    pub(crate) fn seed_players_and_notes(
        &mut self,
        players: Vec<PlayerFacet>,
        tracked: HashMap<AccountId, TrackedPlayer>,
        cx: &mut Context<Self>,
    ) {
        self.players = players;
        self.tracked = tracked;
        self.state = LoadState::Loaded;
        // The clans table aggregates the same tracked players, so seeding
        // them is all it is waiting on too.
        self.clan_state = LoadState::Loaded;
        self.sync_rows(cx);
    }

    /// The note kept against `account`, as the tab holds it. Test-only.
    #[cfg(test)]
    pub(crate) fn note_for(&self, account: AccountId) -> Option<&str> {
        self.tracked.get(&account).map(|player| player.notes.as_str())
    }

    /// Opens `account`'s note for editing, seeding the field with what is
    /// stored.
    fn edit_note(&mut self, account: AccountId, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.tracked.get(&account).map(|player| player.notes.clone()).unwrap_or_default();
        self.note_input.update(cx, |state, cx| state.set_value(note, window, cx));
        self.editing_note = Some(account);
        // The editor lives in the row's own detail block, so writing a note
        // opens the row it belongs to.
        self.expanded_players.insert(account);
        self.remeasure_section(SubTab::Players, cx);
    }

    /// Opens or closes a historical row's detail block.
    fn toggle_player_expanded(&mut self, account: AccountId, cx: &mut Context<Self>) {
        if !self.expanded_players.remove(&account) {
            self.expanded_players.insert(account);
        } else if self.editing_note == Some(account) {
            self.editing_note = None;
        }
        self.remeasure_section(SubTab::Players, cx);
    }

    /// Opens or closes a clans row's member list.
    fn toggle_clan_expanded(&mut self, clan: String, cx: &mut Context<Self>) {
        if !self.expanded_clans.remove(&clan) {
            self.expanded_clans.insert(clan);
        }
        self.remeasure_section(SubTab::Clans, cx);
    }

    /// Tells one section's list that a row's height changed under it.
    ///
    /// Only the section holding the opened row is remeasured: a row opening
    /// in the players table moves nothing in the clans table, and remeasuring
    /// a list also costs it its scroll position.
    fn remeasure_section(&mut self, section: SubTab, cx: &mut Context<Self>) {
        if let Some(panel) = self.panels.iter().find(|panel| panel.read(cx).section() == section).cloned() {
            panel.update(cx, |panel, cx| panel.remeasure(cx));
        }
        cx.notify();
    }

    /// Saved on blur or Enter rather than per keystroke, so a half-typed note
    /// never reaches the database.
    fn on_note_edited(&mut self, state: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>) {
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let Some(account) = self.editing_note else { return };
        let note = state.read(cx).value().to_string();
        self.store_note(account, note, cx);
    }

    fn store_note(&mut self, account: AccountId, note: String, cx: &mut Context<Self>) {
        // An empty note about a player nothing else is recorded for is not
        // an entry worth creating.
        if note.is_empty() && !self.tracked.contains_key(&account) {
            return;
        }

        // The name the index knows, so a note written here is legible in the
        // egui tab's own list, which keys its rows on the stored name.
        let facet = self.players.iter().find(|facet| facet.account_id == account);
        let entry = self.tracked.entry(account).or_insert_with(|| TrackedPlayer {
            db_id: account,
            last_name: facet.map(|facet| facet.latest_name.clone()).unwrap_or_default(),
            clan: facet.map(|facet| facet.clan.clone()).unwrap_or_default(),
            ..TrackedPlayer::default()
        });
        if entry.notes == note {
            return;
        }
        entry.notes = note;

        let Some(pool) = crate::settings_store::pool(cx) else { return };
        // One account's row, which is all a note changes. The whole map used to
        // go out on every keystroke's save.
        let mut pending = tracked::Pending::default();
        pending.note_changed(account);
        let players: HashMap<AccountId, TrackedPlayer> =
            self.tracked.get(&account).map(|player| (account, player.clone())).into_iter().collect();
        // Bumped per write so a slower earlier save cannot land over a later
        // one.
        self.note_generation = self.note_generation.wrapping_add(1);
        let generation = self.note_generation;

        self._note_save =
            Some(cx.spawn(async move |this, cx| {
                let written = runtime::spawn(cx, async move {
                    store::save(&pool, &pending, &players).await.map_err(NoteError::Write)
                })
                .await;

                let failed = match written {
                    Ok(Ok(())) => None,
                    Ok(Err(err)) => Some(err.to_string()),
                    Err(err) => Some(err.to_string()),
                };
                let _ = this.update(cx, |this, cx| {
                    if this.note_generation != generation {
                        return;
                    }
                    if let Some(reason) = failed {
                        tracing::warn!("player tracker: the note was not saved: {reason}");
                        this.note_error = Some(reason);
                    } else {
                        this.note_error = None;
                    }
                    cx.notify();
                });
            }));
    }

    /// Looks up which of this battle's players the index has met before.
    ///
    /// Asks about the two dozen names in hand rather than reading every
    /// account the index holds: the answer is a boolean per row, and the
    /// whole-index read is hundreds of thousands of rows on an established
    /// install. All-time by design, so the column does not follow the period
    /// selector the way the tables above it do.
    fn look_up_met_before(&mut self, cx: &mut Context<Self>) {
        let Some(live) = self.live_match.as_ref() else { return };
        let Some(pool) = crate::settings_store::pool(cx) else { return };

        let started_at = live.started_at;
        let names: Vec<String> = live.players.iter().map(|player| player.name.clone()).collect();
        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move { query::accounts_named(&pool, &names).await }).await;
            let _ = this.update(cx, |this, cx| {
                if this.live_started_at() != Some(started_at) {
                    return;
                }
                match found {
                    Ok(Ok(accounts)) => this.met_before = accounts,
                    // The roster still lists everyone; only "met before"
                    // stays empty.
                    Ok(Err(err)) => tracing::warn!("player tracker: the met-before lookup failed: {err}"),
                    Err(err) => tracing::warn!("player tracker: the met-before lookup did not complete: {err}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads what the clans table needs from the replay index: the latest
    /// clan per account, and the encounters whose clan at the time differed.
    ///
    /// Neither answer depends on the period or the division toggle, so this
    /// runs per refresh rather than per table rebuild. A failure leaves the
    /// table counting on the tracker's own clan per player, which is the same
    /// answer wherever the index has nothing fresher.
    ///
    /// Both queries are all-time whole-index scans -- tens of seconds on an
    /// established install -- and the config pool hands out one connection at
    /// a time, so running them with every refresh queued the Players table's
    /// own (period-filtered, near-instant) query behind them. They run when
    /// the Clans table is drawn instead: see [`Self::load_clan_inputs_if_idle`].
    fn load_clan_inputs(&mut self, pool: SqlitePool, cx: &mut Context<Self>) {
        let generation = self.generation;
        self.clan_state = LoadState::Loading;
        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move {
                let filter = wows_toolkit_config::index::rows::MatchFilter::default();
                let latest = query::distinct_players(&pool, &filter).await?;
                let corrections = query::clan_history_corrections(&pool, &filter).await?;
                Ok::<_, wows_toolkit_config::index::rows::IndexError>((latest, corrections))
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match found {
                    Ok(Ok((latest, corrections))) => {
                        this.clan_latest = latest.into_iter().map(|facet| (facet.account_id, facet.clan)).collect();
                        this.clan_corrections = corrections;
                        this.clan_state = LoadState::Loaded;
                        this.sync_rows(cx);
                    }
                    Ok(Err(err)) => {
                        tracing::warn!("player tracker: the clan inputs could not be read: {err}");
                        this.clan_state = LoadState::Failed(err.to_string());
                    }
                    Err(err) => {
                        tracing::warn!("player tracker: the clan inputs did not load: {err}");
                        this.clan_state = LoadState::Failed(err.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the notes without touching the replay index: the shared notes
    /// are what the replay inspector colours its rows from, so they are worth
    /// reading at startup, while the index aggregates behind [`refresh`] are
    /// not (see [`Self::load_index_once`]).
    pub fn load_notes(&mut self, pool: SqlitePool, cx: &mut Context<Self>) {
        self.load_tracked_players(pool, cx);
    }

    /// Runs the index queries the first time the tab is shown.
    ///
    /// They scan every indexed vehicle row, which is tens of seconds on a
    /// large index, and they hold a connection from the shared pool while
    /// they do. The egui app runs the same aggregate only on the Player
    /// Tracker's own button press; running it at startup delayed everything
    /// else the pool serves, up to the point of timing out the Twitch poll.
    pub fn load_index_once(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.state, LoadState::Idle) {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        self.refresh(pool, cx);
    }

    pub fn refresh(&mut self, pool: SqlitePool, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.state = LoadState::Loading;
        self.clan_state = LoadState::Idle;
        self.load_tracked_players(pool.clone(), cx);
        cx.notify();

        let filter = self.period.match_filter(Timestamp::now());
        cx.spawn(async move |this, cx| {
            let found = runtime::spawn(cx, async move {
                let players = query::distinct_players(&pool, &filter).await?;
                // The perspective player of a replay is not someone you met:
                // the egui tracker drops those accounts too
                // (`player_tracker/model.rs`'s `populate_from_index`), and
                // left in they lead the table, since they were in every
                // battle their own replay recorded.
                let own = query::self_account_ids(&pool, &filter).await?;
                Ok::<_, wows_toolkit_config::index::rows::IndexError>((players, own))
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match found {
                    Ok(Ok((players, own))) => {
                        this.players = players.into_iter().filter(|facet| !own.contains(&facet.account_id)).collect();
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
        self.save_view(cx);
        self.sync_rows(cx);
    }

    /// The historical table's rows: the indexed players, joined to what the
    /// tracker recorded about the same accounts, so the counts and the
    /// division-mate toggle read the way the egui tracker's do.
    fn rows(&self) -> Vec<PlayerRow> {
        visible_player_rows(
            &self.players,
            &self.tracked,
            &self.filter_text,
            self.period.earliest(Timestamp::now()),
            self.show_division_mates,
            self.sort,
        )
    }

    /// The clans table, counted over the tracked encounters.
    ///
    /// The index facets cannot answer this: they count rows in the replay
    /// index, which carries no record of which battles the user arranged, so
    /// a table built from them would ignore the division-mate toggle and
    /// disagree with the egui app's figures.
    fn clans(&self) -> Vec<ClanRow> {
        let since = self.period.earliest(Timestamp::now());
        let rows = build_clan_breakdown(
            &self.tracked,
            &self.clan_latest,
            &self.clan_corrections,
            since,
            self.show_division_mates,
        );
        visible_clans(rows, &self.filter_text, self.clan_sort)
    }

    /// Hands the Search tab a query for every match this player was in.
    fn find_player_matches(&mut self, account: AccountId, cx: &mut Context<Self>) {
        let query = query_text::print_query(&seed::matches_with_player(account));
        cx.emit(PlayerTrackerEvent::SearchFor(query));
    }

    /// The same for a clan tag. The index has no clan-only field, so this
    /// matches a tag appearing in a name too, which is what the button's
    /// hover says.
    fn find_clan_matches(&mut self, clan: String, cx: &mut Context<Self>) {
        let query = query_text::print_query(&seed::matches_mentioning_clan(&clan));
        cx.emit(PlayerTrackerEvent::SearchFor(query));
    }

    fn set_show_division_mates(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_division_mates == show {
            return;
        }
        self.show_division_mates = show;
        self.store_view_modes(cx);
        self.sync_rows(cx);
    }

    /// The live roster joined to game data and tracked history.
    ///
    /// Recomputed per render rather than cached: it is at most 24 rows, and
    /// caching it would need invalidating on every one of the four inputs.
    fn live_roster(&self) -> Option<ResolvedRoster> {
        let live = self.live_match.as_ref()?;
        // The lookup is already keyed by lower-cased name and scoped to this
        // battle, which is the shape the index takes.
        let name_index = TrackedIndex { by_name: self.met_before.clone(), players: self.met_before.len() };
        Some(resolve_roster(
            live,
            &name_index,
            self.live_identities.as_ref(),
            self.live_metadata.as_deref().map(|data| data.provider().as_ref()),
        ))
    }

    /// Rebuilds the chip candidates for everyone currently on the roster.
    ///
    /// Called when the chat observations change and when a scan replaces the
    /// roster; a row whose name matched nothing is left out of the map, which
    /// is a row with no chip.
    fn recompute_twitch_candidates(&mut self) {
        let Some(roster) = self.live_roster() else {
            self.twitch_candidates.clear();
            return;
        };

        let mut candidates = HashMap::new();
        for row in roster.friendly.iter().chain(roster.enemy.iter()) {
            let observations = self.chat_observations.iter().map(|(login, seen_at)| (login.as_str(), *seen_at));
            let found = twitch::sniper_candidates(observations, &row.name, roster.started_at);
            if !found.is_empty() {
                candidates.insert(row.name.clone(), found);
            }
        }
        self.twitch_candidates = candidates;
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

        self.look_up_met_before(cx);

        if let Some(started_at) = started_at {
            self.watch_chat_observations(started_at, cx);
        }

        let (Some(started_at), Some(build)) = (started_at, build) else { return };
        let Some(game_data) = self.game_data.clone() else { return };

        if self.live_metadata_build == Some(build) && self.live_metadata.is_some() {
            // The build is loaded, but this battle's roster can carry a class
            // the last one did not.
            let svg_renderer = cx.svg_renderer();
            self.load_roster_icons(svg_renderer, cx);
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
                        let svg_renderer = cx.svg_renderer();
                        this.load_roster_icons(svg_renderer, cx);
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
        self.met_before.clear();
        self.chat_observations.clear();
        self.twitch_candidates.clear();
        self._chat_watch = None;
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
                    // The roster's names can change with the identities, and
                    // the chip is keyed by them.
                    this.recompute_twitch_candidates();

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

    /// Says the rows have changed. Each section reconciles its own list as
    /// it draws, where the rows are already built, so this only has to ask
    /// for a redraw; the panels watch this tab for it.
    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    /// Brings one section forward in the dock.
    ///
    /// The app switches sections through the dock's own tab bar, so nothing
    /// in it calls this; it is how a test asks for a section.
    #[cfg(test)]
    pub(crate) fn set_sub_tab(&mut self, sub_tab: SubTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.panels.iter().find(|panel| panel.read(cx).section() == sub_tab) {
            let id = PanelId::from(panel.entity_id());
            self.dock_area.update(cx, |dock, cx| dock.select_panel(id, window, cx));
        }
        if sub_tab == SubTab::Clans {
            self.load_clan_inputs_if_idle(cx);
        }
        self.sync_rows(cx);
    }

    /// Starts the clans table's own index reads, unless they have already
    /// been read for this refresh.
    ///
    /// Asked for by the clans section as it draws rather than by whatever
    /// switched to it: the sections are dock panels, so one can be brought on
    /// screen by a drag this tab never hears about.
    fn load_clan_inputs_if_idle(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.clan_state, LoadState::Idle) {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        self.load_clan_inputs(pool, cx);
    }

    fn sort_clans_by(&mut self, column: ClanSortColumn, cx: &mut Context<Self>) {
        self.clan_sort = self.clan_sort.toggled(column);
        self.save_view(cx);
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
        self.recompute_twitch_candidates();
        self.stats = StatsState::Ready(index_by_account(players));
        cx.notify();
    }

    /// Decodes a ship-class icon for every class in the roster, tinted by the
    /// relation the row is drawn in. One decode per class and tint, not one
    /// per row.
    fn load_roster_icons(&mut self, svg_renderer: gpui_kit::SvgRenderer, cx: &mut Context<Self>) {
        let Some(metadata) = self.live_metadata.clone() else { return };
        let Some(roster) = self.live_roster() else { return };

        let vfs = metadata.vfs().clone();
        let mut seen: HashSet<(Species, u32)> = HashSet::new();
        for row in roster.friendly.iter().chain(roster.enemy.iter()) {
            let Some(species) = row.species else { continue };
            let tint = tint_rgb(row.tint);
            if seen.insert((species, tint)) {
                self.icons.load_ship_class(species, tint, &vfs, &svg_renderer);
            }
        }
        cx.notify();
    }

    fn set_view_mode(&mut self, view_mode: CurrentMatchViewMode, cx: &mut Context<Self>) {
        self.view_mode = view_mode;
        self.store_view_modes(cx);
        cx.notify();
    }

    fn set_win_rate_mode(&mut self, win_rate_mode: WinRateMode, cx: &mut Context<Self>) {
        self.win_rate_mode = win_rate_mode;
        self.store_view_modes(cx);
        cx.notify();
    }

    /// Saves the roster's view settings where the egui app keeps its own, so
    /// the mode chosen in one is the mode the other opens on.
    fn store_view_modes(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let modes = tracked::ViewModes {
            win_rate_mode: self.win_rate_mode,
            current_match_view_mode: self.view_mode,
            show_division_mates: self.show_division_mates,
        };

        cx.spawn(async move |_this, cx| {
            let written = runtime::spawn(cx, async move { store::store_view_modes(&pool, modes).await }).await;

            if let Ok(Err(err)) = written {
                tracing::warn!("player tracker: the view settings were not saved: {err}");
            }
        })
        .detach();
    }

    /// One section's own table: its header and the rows under it.
    ///
    /// Takes the section to draw and the list it scrolls rather than reading
    /// the tab's own, so two sections docked beside each other each scroll
    /// their own rows.
    pub(crate) fn render_section(
        &mut self,
        sub_tab: SubTab,
        list_state: &ListState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let border = cx.theme().border;
        let hover_bg = cx.theme().accent;

        // Drawing the clans table is what says it is wanted, whether it was
        // brought forward by its tab or dragged into view.
        if sub_tab == SubTab::Clans {
            self.load_clan_inputs_if_idle(cx);
        }

        // The Current Match roster is its own layout: two teams side by
        // side, each with its own header, so the shared table chrome below
        // does not apply to it.
        if sub_tab == SubTab::CurrentMatch {
            return self.render_current_match(cx);
        }

        let header_cells: Vec<AnyElement> = match sub_tab {
            SubTab::Players => SortColumn::ALL
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    let column = *column;
                    let width = player_column_width(column);
                    sort_header(
                        HeaderCell {
                            id_prefix: "tracker-sort",
                            index,
                            label: t!(column.label_key()).into_owned().into(),
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
                    let width = clan_column_width(column);
                    sort_header(
                        HeaderCell {
                            id_prefix: "tracker-clan-sort",
                            index,
                            label: t!(column.label_key()).into_owned().into(),
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
        let row_count = match sub_tab {
            SubTab::Players => players.len(),
            SubTab::Clans => clans.len(),
            SubTab::CurrentMatch => unreachable!("the roster returns above"),
        };
        // Reconciled here, where the rows are already built. Resetting also
        // returns the list to the top, so it is done only when the count
        // actually moved and only to this section's own list.
        if list_state.item_count() != row_count {
            list_state.reset(row_count);
        }
        let notes: HashMap<AccountId, String> = self
            .tracked
            .iter()
            .filter(|(_, player)| !player.notes.is_empty())
            .map(|(id, player)| (*id, player.notes.clone()))
            .collect();
        let tracker = cx.entity();
        // What an opened row shows beneath itself, taken here rather than in
        // the row closure, which cannot reach back into the tab.
        let expanded = self.expanded_players.clone();
        let details: HashMap<AccountId, PlayerDetail> = expanded
            .iter()
            .map(|account| (*account, player_detail(self.tracked.get(account), self.show_division_mates)))
            .collect();
        let expanded_clans = self.expanded_clans.clone();
        // The name each member is known by, which the clan aggregate holds
        // only account ids for.
        let member_names: HashMap<AccountId, String> =
            self.tracked.iter().map(|(id, player)| (*id, player.last_name.clone())).collect();
        let editing_note = self.editing_note;
        let note_input = self.note_input.clone();
        let note_error = self.note_error.clone();
        let render_row = move |ix: usize, _window: &mut Window, cx: &mut App| match sub_tab {
            SubTab::Players => {
                let Some(row) = players.get(ix) else {
                    return div().into_any_element();
                };
                let account = row.facet.account_id;
                let open = expanded.contains(&account);
                let cells = h_flex()
                    .id(ix)
                    .w_full()
                    .h(ROW_HEIGHT)
                    .gap_2()
                    .items_center()
                    .px_2()
                    .hover(|this| this.bg(hover_bg))
                    .child(
                        h_flex()
                            .w(NAME_COLUMN_WIDTH)
                            .gap_1()
                            .items_center()
                            .child(expand_caret(("tracker-player-expand", ix), open, {
                                let tracker = tracker.clone();
                                move |cx: &mut App| {
                                    tracker.update(cx, |this, cx| this.toggle_player_expanded(account, cx));
                                }
                            }))
                            .child(div().flex_1().min_w(px(0.)).text_sm().child(row.facet.latest_name.clone())),
                    )
                    .child(
                        div()
                            .w(CLAN_COLUMN_WIDTH)
                            .text_sm()
                            .text_color(crate::theme::text_dim())
                            .child(row.facet.clan.clone()),
                    )
                    // Both counts are read in the tone the in-range one
                    // deserves: how often you have met someone lately is what
                    // the ramp is warning about.
                    .child(
                        div()
                            .w(COUNT_COLUMN_WIDTH)
                            .text_sm()
                            .when_some(severity_color(row.encounters_in_range), |el, color| el.text_color(color))
                            .child(count_text(row.total_encounters)),
                    )
                    .child(
                        div()
                            .w(RANGE_COUNT_COLUMN_WIDTH)
                            .text_sm()
                            .when_some(severity_color(row.encounters_in_range), |el, color| el.text_color(color))
                            .child(separate_number(row.encounters_in_range as i64, None)),
                    )
                    .child(last_seen_cell(ix, row.last_seen))
                    .child(find_matches_cell(
                        ("tracker-find-player", ix),
                        t!("ui.player_tracker.find_matches").into_owned(),
                        {
                            let tracker = tracker.clone();
                            let account = row.facet.account_id;
                            move |cx: &mut App| {
                                tracker.update(cx, |this, cx| this.find_player_matches(account, cx));
                            }
                        },
                    ))
                    .child(note_cell(ix, row.facet.account_id, notes.get(&row.facet.account_id), tracker.clone()));

                let detail = open.then(|| {
                    player_detail_block(
                        ix,
                        account,
                        details.get(&account),
                        (editing_note == Some(account))
                            .then(|| OpenNote { input: note_input.clone(), error: note_error.clone() }),
                    )
                });

                v_flex()
                    .w_full()
                    .when_some(crate::ui::stripe(ix, cx), |el, color| el.bg(color))
                    .child(cells)
                    .children(detail)
                    .into_any_element()
            }
            SubTab::CurrentMatch => unreachable!("the roster returns above"),
            SubTab::Clans => {
                let Some(row) = clans.get(ix) else {
                    return div().into_any_element();
                };
                let open = expanded_clans.contains(&row.clan);
                let cells = h_flex()
                    .id(ix)
                    .w_full()
                    .h(ROW_HEIGHT)
                    .gap_2()
                    .items_center()
                    .px_2()
                    .hover(|this| this.bg(hover_bg))
                    // The tag and both battle counts read in the tone the
                    // in-range count deserves, as the egui table's do.
                    .child(
                        h_flex()
                            .w(CLAN_TAG_COLUMN_WIDTH)
                            .gap_1()
                            .items_center()
                            .child(expand_caret(("tracker-clan-expand", ix), open, {
                                let tracker = tracker.clone();
                                let clan = row.clan.clone();
                                move |cx: &mut App| {
                                    let clan = clan.clone();
                                    tracker.update(cx, |this, cx| this.toggle_clan_expanded(clan, cx));
                                }
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_sm()
                                    .when_some(severity_color(row.matches_in_range), |el, color| el.text_color(color))
                                    .child(row.clan.clone()),
                            ),
                    )
                    .child(div().w(MEMBERS_COLUMN_WIDTH).text_sm().child(row.members.len().to_string()))
                    .child(div().w(COUNT_COLUMN_WIDTH).text_sm().child(separate_number(row.matches as i64, None)))
                    .child(
                        div()
                            .w(RANGE_COUNT_COLUMN_WIDTH)
                            .text_sm()
                            .when_some(severity_color(row.matches_in_range), |el, color| el.text_color(color))
                            .child(separate_number(row.matches_in_range as i64, None)),
                    )
                    .child(sightings_cell(ix, row.sightings, row.sightings_in_range))
                    .child(last_seen_cell(ix, Some(row.last_seen)))
                    .child(find_matches_cell(
                        ("tracker-find-clan", ix),
                        t!("ui.player_tracker.find_clan_matches").into_owned(),
                        {
                            let tracker = tracker.clone();
                            let clan = row.clan.clone();
                            move |cx: &mut App| {
                                let clan = clan.clone();
                                tracker.update(cx, |this, cx| this.find_clan_matches(clan, cx));
                            }
                        },
                    ));

                let members = open.then(|| clan_member_list(ix, &row.members, &member_names, tracker.clone()));

                v_flex()
                    .w_full()
                    .when_some(crate::ui::stripe(ix, cx), |el, color| el.bg(color))
                    .child(cells)
                    .children(members)
                    .into_any_element()
            }
        };

        // The clans table reads its own aggregates, so it reports on those
        // rather than on the player query it does not draw from.
        let load_state = match sub_tab {
            SubTab::Clans => &self.clan_state,
            _ => &self.state,
        };
        let status = match load_state {
            LoadState::Idle => Some(t!("ui.player_tracker.waiting_for_index").into_owned()),
            LoadState::Loading => match sub_tab {
                SubTab::Clans => Some(t!("ui.player_tracker.loading_clans").into_owned()),
                _ => Some(t!("ui.player_tracker.loading_players").into_owned()),
            },
            LoadState::Failed(reason) => Some(t!("ui.player_tracker.index_failed", reason = reason).to_string()),
            LoadState::Loaded if row_count == 0 => match sub_tab {
                SubTab::Players => Some(t!("ui.player_tracker.no_players").into_owned()),
                SubTab::Clans => Some(t!("ui.player_tracker.clan_no_data").into_owned()),
                SubTab::CurrentMatch => unreachable!("the roster returns above"),
            },
            LoadState::Loaded => None,
        };

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
                .child(list(list_state.clone(), render_row).size_full())
                .child(Scrollbar::vertical(list_state))
                .into_any_element(),
        };

        v_flex().size_full().child(header).child(div().flex_1().min_h(px(0.)).child(body)).into_any_element()
    }

    /// The Current Match body: the roster when a battle is under way, and
    /// what is missing when it is not.
    fn render_current_match(&self, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;

        let status = match (&self.game_data, self.live_checked, &self.live_match) {
            (None, _, _) => Some(t!("ui.player_tracker.live_no_directory").into_owned()),
            (_, false, _) => Some(t!("ui.player_tracker.live_checking").into_owned()),
            (_, true, None) => Some(t!("ui.player_tracker.live_none").into_owned()),
            (_, true, Some(_)) => None,
        };

        // The mode selectors sit above the roster rather than in the tab's
        // own toolbar: they say nothing about the tables beside it.
        let entity = cx.entity();
        let view_modes = CurrentMatchViewMode::ALL.map(|mode| {
            let chosen = self.view_mode == mode;
            let entity = entity.clone();
            selectable(
                ("tracker-view-mode", mode as usize),
                chosen,
                Button::new(("tracker-view-mode-button", mode as usize))
                    .label(t!(mode.label_key()).into_owned())
                    .compact()
                    .selected(chosen)
                    .on_click(move |_event, _window, cx: &mut App| {
                        entity.update(cx, |this, cx| this.set_view_mode(mode, cx));
                    }),
            )
        });
        // Which scope the compact roster shows. Detailed shows both, so the
        // selector would say nothing there.
        let scope_modes = (self.view_mode == CurrentMatchViewMode::Compact).then(|| {
            WinRateMode::ALL.map(|mode| {
                let chosen = self.win_rate_mode == mode;
                let entity = entity.clone();
                selectable(
                    ("tracker-win-rate-mode", mode as usize),
                    chosen,
                    Button::new(("tracker-win-rate-mode-button", mode as usize))
                        .label(t!(mode.label_key()).into_owned())
                        .compact()
                        .selected(chosen)
                        .on_click(move |_event, _window, cx: &mut App| {
                            entity.update(cx, |this, cx| this.set_win_rate_mode(mode, cx));
                        }),
                )
            })
        });

        let mode_bar = h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .children(view_modes)
            .when_some(scope_modes, |this, modes| this.child(div().w(px(8.))).children(modes));

        if let Some(status) = status {
            return v_flex()
                .size_full()
                .child(mode_bar)
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .child(div().text_sm().text_color(crate::theme::text_dim()).child(status)),
                )
                .into_any_element();
        }

        let Some(roster) = self.live_roster() else {
            return div().into_any_element();
        };

        let note = match &self.stats {
            // A failure is reported even while the ships are unresolved: a
            // build that did not load is why they are unresolved, and saying
            // "loading" for it would never stop being wrong.
            StatsState::Failed(reason) => Some(t!("ui.player_tracker.stats_unavailable", reason = reason).to_string()),
            _ if !roster.ships_resolved => Some(t!("ui.player_tracker.live_loading_build").into_owned()),
            StatsState::Scanning => Some(t!("ui.player_tracker.stats_resolving").into_owned()),
            StatsState::Fetching => Some(t!("ui.player_tracker.stats_fetching").into_owned()),
            StatsState::Idle | StatsState::Ready(_) => None,
        };
        let stats = match &self.stats {
            StatsState::Ready(players) => Some(players),
            _ => None,
        };
        let modes = visible_stat_modes(self.view_mode, self.win_rate_mode);
        let tracker = cx.entity();
        let layout = RosterLayout {
            stats,
            met: &self.tracked,
            count_division_mates: self.show_division_mates,
            icons: &self.icons,
            twitch: &self.twitch_candidates,
            modes: &modes,
            border,
            tracker: &tracker,
        };

        v_flex()
            .size_full()
            .child(mode_bar)
            .when_some(note, |this, note| {
                this.child(div().flex_none().px_2().py_1().text_xs().text_color(crate::theme::text_dim()).child(note))
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_start()
                    .child(team_column(t!("ui.player_tracker.allies").into_owned(), "ally", &roster.friendly, layout))
                    .child(div().w(px(1.)).h_full().bg(border))
                    .child(team_column(t!("ui.player_tracker.enemies").into_owned(), "enemy", &roster.enemy, layout)),
            )
            .into_any_element()
    }

    /// Writes the view back to the settings row.
    fn save_view(&self, cx: &mut Context<Self>) {
        let settings = TrackerSettings {
            period: self.period,
            sort: self.sort,
            clan_sort: self.clan_sort,
            filter: self.filter_text.clone(),
        };
        crate::settings_store::save(TRACKER_SETTINGS_KEY, &settings, cx);
    }

    /// Reads the view back at startup, so the tracker opens where it was
    /// left rather than on its defaults.
    fn load_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let stored = runtime::spawn(cx, async move {
                wows_toolkit_config::queries::get_setting::<TrackerSettings>(&pool, TRACKER_SETTINGS_KEY).await
            })
            .await;
            let Ok(Some(settings)) = stored else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                this.period = settings.period;
                this.sort = settings.sort;
                this.clan_sort = settings.clan_sort;
                this.filter_text = settings.filter.clone();
                this.period_select.update(cx, |select, cx| {
                    select.set_selected_index(Some(IndexPath::new(period_index(settings.period))), window, cx)
                });
                if !settings.filter.is_empty() {
                    this.filter_input.update(cx, |state, cx| state.set_value(settings.filter, window, cx));
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_period(&mut self, period: TimePeriod, pool: Option<SqlitePool>, cx: &mut Context<Self>) {
        if self.period == period {
            return;
        }
        self.period = period;
        self.save_view(cx);
        match pool {
            // A different window of time is a different query, not a filter
            // over what is already loaded.
            Some(pool) => self.refresh(pool, cx),
            None => self.sync_rows(cx),
        }
    }

    fn sort_by(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        self.sort = self.sort.toggled(column);
        self.save_view(cx);
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
    label: SharedString,
    width: Pixels,
    /// Whether this is the column the table is currently ordered by.
    active: bool,
    order: SortOrder,
}

/// What each historical column is drawn at.
fn player_column_width(column: SortColumn) -> Pixels {
    match column {
        SortColumn::Name => NAME_COLUMN_WIDTH,
        SortColumn::Clan => CLAN_COLUMN_WIDTH,
        SortColumn::TotalEncounters => COUNT_COLUMN_WIDTH,
        SortColumn::Encounters => RANGE_COUNT_COLUMN_WIDTH,
        SortColumn::LastEncountered => LAST_SEEN_COLUMN_WIDTH,
    }
}

/// A count the tracker has no record of reads as a dash rather than as zero,
/// which would claim the player was never met.
fn count_text(count: Option<usize>) -> String {
    match count {
        Some(count) => separate_number(count as i64, None),
        None => "-".to_string(),
    }
}

/// When the player was last met, worded as an age with the exact local time
/// behind it.
fn last_seen_cell(ix: usize, last_seen: Option<jiff::Timestamp>) -> AnyElement {
    let relative = history::last_seen_text(last_seen, Timestamp::now());
    let exact = history::last_seen_timestamp_text(last_seen);
    div()
        .id(("tracker-last-seen", ix))
        .w(LAST_SEEN_COLUMN_WIDTH)
        .text_sm()
        .text_color(crate::theme::text_dim())
        .when(!exact.is_empty(), |this| {
            let exact = SharedString::from(exact);
            this.tooltip(move |window, cx| Tooltip::new(exact.clone()).build(window, cx))
        })
        .child(relative)
        .into_any_element()
}

/// The note editor, when the row being drawn is the one being written
/// about.
struct OpenNote {
    input: Entity<InputState>,
    /// Why the last write did not land, if it did not.
    error: Option<String>,
}

/// What a historical row shows once it is opened.
struct PlayerDetail {
    /// Every other name this account has been seen under, sorted.
    aliases: Vec<String>,
    /// When the player was last met, spelled out in local time. Empty when
    /// the tracker has no encounter recorded.
    last_seen_exact: String,
    /// Battles the player was met in, all time.
    total_encounters: usize,
}

/// Reads one player's detail out of the tracker.
fn player_detail(player: Option<&TrackedPlayer>, show_division_mates: bool) -> PlayerDetail {
    let Some(player) = player else {
        return PlayerDetail { aliases: Vec::new(), last_seen_exact: String::new(), total_encounters: 0 };
    };
    let mut aliases: Vec<String> = player.names.iter().filter(|name| **name != player.last_name).cloned().collect();
    aliases.sort();
    PlayerDetail {
        aliases,
        last_seen_exact: history::last_seen_timestamp_text(player.last_visible_timestamp(show_division_mates)),
        total_encounters: player.visible_arena_ids(show_division_mates).count(),
    }
}

/// The block under an opened historical row: what the tracker knows beyond
/// the columns, and the note editor when this is the row being written about.
fn player_detail_block(
    ix: usize,
    account: AccountId,
    detail: Option<&PlayerDetail>,
    note: Option<OpenNote>,
) -> AnyElement {
    let dim = crate::theme::text_dim();
    let mut block = v_flex()
        .id(("tracker-player-detail", ix))
        .test_support()
        .w_full()
        .gap_1()
        .px_2()
        .py_1()
        .pl(NAME_COLUMN_WIDTH)
        .child(
            Button::new(("tracker-account-id", ix))
                .label(account.0.to_string())
                .compact()
                .ghost()
                .xsmall()
                .tooltip(t!("ui.player_tracker.copy_wg_id").into_owned())
                .on_click(move |_event, window, cx: &mut App| {
                    let id = account.0.to_string();
                    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(id.clone()));
                    crate::toast::ok(t!("ui.player_tracker.copied_wg_id", id = id).into_owned(), window, cx);
                }),
        );

    if let Some(detail) = detail {
        if !detail.aliases.is_empty() {
            block = block.child(
                div()
                    .text_xs()
                    .text_color(dim)
                    .child(t!("ui.player_tracker.aliases_hover", names = detail.aliases.join(", ")).into_owned()),
            );
        }
        if !detail.last_seen_exact.is_empty() {
            block = block.child(div().text_xs().text_color(dim).child(
                t!("ui.player_tracker.last_encountered_exact", timestamp = detail.last_seen_exact).into_owned(),
            ));
        }
        block = block.child(
            div()
                .text_xs()
                .text_color(dim)
                .child(t!("ui.player_tracker.arena_count", count = detail.total_encounters).into_owned()),
        );
    }

    match note {
        None => block.child(div().text_xs().text_color(dim).child(t!("ui.player_tracker.notes_hint").into_owned())),
        Some(OpenNote { input, error }) => block
            .child(Input::new(&input).id("tracker-note-input").small().w_full())
            .when_some(error, |this, reason| {
                this.child(
                    div()
                        .id("tracker-note-error")
                        .test_support()
                        .text_xs()
                        .text_color(rgb(0xff8080))
                        .child(t!("ui.player_tracker.note_not_saved", reason = reason).into_owned()),
                )
            }),
    }
    .into_any_element()
}

/// The members met from a clan, most-met first, each offering to look up
/// the matches they were in.
fn clan_member_list(
    ix: usize,
    members: &[(AccountId, usize)],
    names: &HashMap<AccountId, String>,
    tracker: Entity<PlayerTrackerView>,
) -> AnyElement {
    let dim = crate::theme::text_dim();
    v_flex()
        .id(("tracker-clan-members", ix))
        .test_support()
        .w_full()
        .gap_px()
        .px_2()
        .py_1()
        .pl(CLAN_TAG_COLUMN_WIDTH)
        .children(members.iter().enumerate().map(|(member_ix, (account, matches))| {
            // A member the tracker recorded under no name is still worth a
            // row: the account id is what the search needs.
            let name = names.get(account).cloned().unwrap_or_else(|| account.0.to_string());
            let tracker = tracker.clone();
            let account = *account;
            h_flex()
                .gap_2()
                .items_center()
                .child(find_matches_cell(
                    ("tracker-find-clan-member", ix * MEMBER_ID_STRIDE + member_ix),
                    t!("ui.player_tracker.find_matches").into_owned(),
                    move |cx: &mut App| {
                        tracker.update(cx, |this, cx| this.find_player_matches(account, cx));
                    },
                ))
                .child(div().text_xs().child(name))
                .child(
                    div()
                        .text_xs()
                        .text_color(dim)
                        .child(t!("ui.player_tracker.clan_member_matches", count = matches).into_owned()),
                )
        }))
        .into_any_element()
}

/// Spreads the member buttons' ids apart so two clans' lists cannot collide.
const MEMBER_ID_STRIDE: usize = 1024;

/// The triangle that opens a row.
fn expand_caret(id: (&'static str, usize), open: bool, on_click: impl Fn(&mut App) + 'static) -> AnyElement {
    Button::new(id)
        .child(crate::icons::icon(if open { crate::icons::CARET_DOWN } else { crate::icons::CARET_RIGHT }))
        .ghost()
        .xsmall()
        .on_click(move |_event, _window, cx: &mut App| on_click(cx))
        .into_any_element()
}

/// The button that looks a row up in the replay index.
fn find_matches_cell(id: (&'static str, usize), hover: String, on_click: impl Fn(&mut App) + 'static) -> AnyElement {
    Button::new(id)
        .child(crate::icons::icon(crate::icons::MAGNIFYING_GLASS))
        .compact()
        .tooltip(hover)
        .on_click(move |_event, _window, cx: &mut App| on_click(cx))
        .into_any_element()
}

/// What each clans column is drawn at.
fn clan_column_width(column: ClanSortColumn) -> Pixels {
    match column {
        ClanSortColumn::Clan => CLAN_TAG_COLUMN_WIDTH,
        ClanSortColumn::Members => MEMBERS_COLUMN_WIDTH,
        ClanSortColumn::Encounters => COUNT_COLUMN_WIDTH,
        ClanSortColumn::EncountersInRange => RANGE_COUNT_COLUMN_WIDTH,
        ClanSortColumn::Sightings => COUNT_COLUMN_WIDTH,
        ClanSortColumn::LastEncountered => LAST_SEEN_COLUMN_WIDTH,
    }
}

/// How many of the clan's players were met, counted per battle. The hover
/// says how many of those fall in the period on screen.
fn sightings_cell(ix: usize, sightings: usize, in_range: usize) -> AnyElement {
    let hover = t!("ui.player_tracker.clan_sightings_hover", range = in_range).into_owned();
    div()
        .id(("tracker-clan-sightings", ix))
        .w(COUNT_COLUMN_WIDTH)
        .text_sm()
        .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx))
        .child(separate_number(sightings as i64, None))
        .into_any_element()
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

/// A live roster row's colour kind. The same table the replay inspector's
/// player names use, so one person reads the same in both tabs.
fn tint_kind(tint: PlayerTint) -> PlayerColorKind {
    match tint {
        PlayerTint::SelfPlayer => PlayerColorKind::SelfPlayer,
        PlayerTint::Ally => PlayerColorKind::Ally,
        PlayerTint::Enemy => PlayerColorKind::Enemy,
        PlayerTint::DivisionMate => PlayerColorKind::DivisionMate,
        PlayerTint::Abuser => PlayerColorKind::Abuser,
    }
}

fn tint_color(tint: PlayerTint) -> Hsla {
    resolve_color(ColorRole::Player(tint_kind(tint)))
}

/// The same colour packed, which is how an icon is tinted.
fn tint_rgb(tint: PlayerTint) -> u32 {
    player_color_kind_rgb(tint_kind(tint))
}

/// Why a note could not be saved.
#[derive(Debug, thiserror::Error)]
enum NoteError {
    #[error("the players could not be written")]
    Write(#[source] sqlx::Error),
}

/// One player row's note affordance: the note itself as a tooltip when there
/// is one, and a button that opens the editor either way.
fn note_cell(ix: usize, account: AccountId, note: Option<&String>, tracker: Entity<PlayerTrackerView>) -> AnyElement {
    let has_note = note.is_some();
    let tooltip = note.cloned();

    Button::new(("tracker-note", ix))
        .child(
            crate::icons::icon(crate::icons::NOTE_PENCIL)
                .when(!has_note, |this| this.text_color(crate::theme::text_faint())),
        )
        .compact()
        .when_some(tooltip, |this, note| this.tooltip(SharedString::from(note)))
        .when(!has_note, |this| this.tooltip(t!("ui.player_tracker.notes_hint").into_owned()))
        .on_click(move |_event, window, cx: &mut App| {
            tracker.update(cx, |this, cx| this.edit_note(account, window, cx));
        })
        .into_any_element()
}

/// What a roster column draws, bundled so the row and header helpers stay
/// under clippy's argument-count limit.
#[derive(Clone, Copy)]
struct RosterLayout<'a> {
    stats: Option<&'a HashMap<AccountId, PlayerStatsOut>>,
    /// Everyone met before, so a roster row can say how often rather than
    /// only that it happened.
    met: &'a HashMap<AccountId, TrackedPlayer>,
    /// Whether battles the user arranged count towards that number.
    count_division_mates: bool,
    icons: &'a IconCache,
    /// The chip's candidates per roster name; a name that is absent has no
    /// chip.
    twitch: &'a HashMap<String, Vec<SniperCandidate>>,
    /// The scopes each row shows, left to right.
    modes: &'a [WinRateMode],
    border: Hsla,
    /// The tab itself, which a row's menu asks to look a player up.
    tracker: &'a Entity<PlayerTrackerView>,
}

/// One team's heading: its name, how many players are on it, and what they
/// average.
///
/// The averages are the shared ones, over the scope the rows are showing, so
/// the heading and the cells under it cannot disagree. A team nobody has
/// stats for shows no average rather than a zero.
fn team_heading(title: String, rows: &[LiveRosterRow], layout: RosterLayout) -> impl IntoElement + use<> {
    let mode = layout.modes.first().copied().unwrap_or(WinRateMode::Overall);
    let stats = layout.stats;
    let win_rate =
        stats.and_then(|stats| wows_toolkit_viewmodel::player_tracker::live::team_average_win_rate(rows, stats, mode));
    let rating = stats.and_then(|stats| {
        wows_toolkit_viewmodel::player_tracker::live::team_average_personal_rating(rows, stats, mode)
    });

    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        .child(div().text_sm().font_weight(FontWeight::BOLD).child(title))
        .child(
            div()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.player_tracker.team_players", count = rows.len()).into_owned()),
        )
        .when_some(win_rate, |row, rate| {
            row.child(
                div()
                    .text_xs()
                    .when_some(band_color(Some(PersonalRatingCategory::from_win_rate(rate))), |cell, color| {
                        cell.text_color(color)
                    })
                    .child(t!("ui.player_tracker.team_average_win_rate", rate = format!("{rate:.1}%")).into_owned()),
            )
        })
        .when_some(rating, |row, pr| {
            row.child(
                div()
                    .text_xs()
                    .when_some(band_color(Some(PersonalRatingCategory::from_pr(pr))), |cell, color| {
                        cell.text_color(color)
                    })
                    .child(format!("{}: {pr:.0}", t!("stat.avg_pr"))),
            )
        })
}

/// A scoped column heading: the scope, then the column, both from the catalogue.
///
/// Composed rather than a key per pair, because the pairs are the product of two
/// lists and the egui app composes the same two halves for its cell hovers
/// (`ui/player_tracker/current_match.rs:782`).
fn scoped_heading(scope: &str, column_key: &str) -> String {
    format!("{scope} {}", t!(column_key))
}

/// One team's roster column, with its own header row.
fn team_column(title: String, side: &'static str, rows: &[LiveRosterRow], layout: RosterLayout) -> AnyElement {
    let heading = team_heading(title, rows, layout);
    let mut header = h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(layout.border)
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .child(div().flex_none().w(CLASS_COLUMN_WIDTH))
        .child(div().flex_none().w(CHIP_COLUMN_WIDTH))
        .child(div().flex_1().min_w(px(0.)).child(t!("ui.player_tracker.column.player_name").to_string()))
        .child(div().w(SHIP_COLUMN_WIDTH).child(t!("ui.player_tracker.win_rate_ship").to_string()));

    // One group of columns per scope, so a detailed row reads
    // "overall, then this ship" rather than interleaving the two.
    for mode in layout.modes {
        let scope = t!(mode.label_key()).into_owned();
        header = header
            .child(div().w(STAT_COLUMN_WIDTH).child(scoped_heading(&scope, "ui.player_tracker.column.win_rate")))
            .child(div().w(STAT_COLUMN_WIDTH).child(scoped_heading(&scope, "ui.player_tracker.column.personal_rating")))
            .child(div().w(STAT_COLUMN_WIDTH).child(scoped_heading(&scope, "ui.player_tracker.column.avg_damage")))
            .child(div().w(STAT_COLUMN_WIDTH).child(scoped_heading(&scope, "ui.player_tracker.column.battles")));
    }

    v_flex()
        .flex_1()
        .min_w(px(0.))
        .child(heading)
        .child(
            header
                .child(div().w(MET_COLUMN_WIDTH).child(t!("ui.player_tracker.column.encounters").to_string()))
                .child(div().flex_none().w(ACTIONS_COLUMN_WIDTH)),
        )
        .children(rows.iter().enumerate().map(|(index, row)| roster_row(side, index, row, layout)))
        .into_any_element()
}

/// A rating's colour band, the same one the replay inspector's PR column
/// uses. Absent when the service returned no rating for this player.
fn rating_color(pr: Option<f64>) -> Option<Hsla> {
    let category = PersonalRatingCategory::from_pr(pr?);
    Some(rgb(personal_rating::chip_text(category, crate::theme::is_dark_mode())).into())
}

/// A win rate's colour, from the band the rate itself falls in, so the
/// number and its colour cannot disagree.
fn band_color(band: Option<PersonalRatingCategory>) -> Option<Hsla> {
    Some(rgb(personal_rating::chip_text(band?, crate::theme::is_dark_mode())).into())
}

/// A roster row's own menu: where to read more about this player.
///
/// Both links need an account id and a region, which only the identity scan
/// supplies; a row it never named has nothing to link to and shows no menu.
fn roster_row_menu(
    side: &'static str,
    index: usize,
    row: &LiveRosterRow,
    tracker: &Entity<PlayerTrackerView>,
) -> impl IntoElement + use<> {
    let Some((account_id, region)) = row.account_id.zip(row.region) else {
        // The column still holds its width, so a row with no menu does not
        // pull the ones around it out of line.
        return div().flex_none().w(ACTIONS_COLUMN_WIDTH).into_any_element();
    };

    let numbers = wows_numbers_player_url(region, account_id, &row.name);
    let builds = shipbuilds_player_url(region, account_id, &row.name);
    let tracker = tracker.clone();

    div()
        .flex_none()
        .w(ACTIONS_COLUMN_WIDTH)
        .child(
            Button::new(SharedString::from(format!("tracker-roster-actions-{side}-{index}")))
                .icon(IconName::Ellipsis)
                .ghost()
                .xsmall()
                .dropdown_menu(move |menu, _window, _cx| {
                    menu.item(
                        PopupMenuItem::link(t!("ui.player_tracker.open_wows_numbers").into_owned(), numbers.clone())
                            .icon(IconName::ExternalLink),
                    )
                    .item(
                        PopupMenuItem::link(t!("ui.player_tracker.open_shipbuilds").into_owned(), builds.clone())
                            .icon(IconName::ExternalLink),
                    )
                    .item({
                        let tracker = tracker.clone();
                        PopupMenuItem::new(t!("ui.player_tracker.find_matches").into_owned())
                            .icon(IconName::Search)
                            .on_click(move |_event, _window, cx| {
                                tracker.update(cx, |this, cx| this.find_player_matches(account_id, cx));
                            })
                    })
                }),
        )
        .into_any_element()
}

/// Room for the row menu's trigger, held even by a row that has none.
const ACTIONS_COLUMN_WIDTH: Pixels = px(24.);

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
        .when(color.is_none(), |this| this.text_color(crate::theme::text_dim()))
        .child(text)
        .into_any_element()
}

/// The cells one scope contributes to a row.
fn scope_cells(stats: RowStats, status: PlayerStatsStatus, pending: bool) -> Vec<AnyElement> {
    // A player who hid their statistics is a different answer from one the
    // service had nothing for, and both differ from one it could not reach.
    let note = match status {
        PlayerStatsStatus::Ok => None,
        PlayerStatsStatus::Hidden => Some("hidden"),
        PlayerStatsStatus::Unavailable => Some("n/a"),
        PlayerStatsStatus::Unknown => Some("?"),
    };
    if let Some(note) = note {
        let mut cells = vec![stat_cell(Some(note.to_string()), None, false)];
        cells.extend((0..3).map(|_| stat_cell(None, None, false)));
        return cells;
    }

    vec![
        stat_cell(stats.win_rate.map(|rate| format!("{rate:.1}%")), band_color(stats.band), pending),
        stat_cell(stats.pr.map(|pr| format!("{pr:.0}")), rating_color(stats.pr), pending),
        // Grouped, as every other figure this size in the app is.
        stat_cell(stats.avg_damage.map(|damage| separate_number(damage, None)), None, pending),
        stat_cell(stats.battles.map(|battles| separate_number(battles, None)), None, pending),
    ]
}

/// The possible-stream-sniper chip: shown when a Twitch login that
/// plausibly names this player was in chat around this battle.
///
/// One candidate copies on click, as the egui chip does; several open a
/// picker, because copying an arbitrary one of them would be a guess.
fn twitch_chip(side: &'static str, index: usize, row: &LiveRosterRow, layout: RosterLayout) -> AnyElement {
    let slot = div().flex_none().w(CHIP_COLUMN_WIDTH);
    let Some(candidates) = layout.twitch.get(&row.name) else {
        return slot.into_any_element();
    };
    let Some(first) = candidates.first() else {
        return slot.into_any_element();
    };

    let hover = SharedString::from(sniper_hover_text(candidates));
    let trigger = Button::new(SharedString::from(format!("tracker-twitch-{side}-{index}")))
        .child(crate::icons::icon(crate::icons::TWITCH_LOGO))
        .compact()
        .tooltip(hover);

    if candidates.len() == 1 {
        let login = first.login.clone();
        return slot
            .child(trigger.on_click(move |_event, window, cx: &mut App| {
                cx.write_to_clipboard(ClipboardItem::new_string(login.clone()));
                crate::toast::ok(t!("ui.twitch.copied", name = login).into_owned(), window, cx);
            }))
            .into_any_element();
    }

    let logins: Vec<String> = candidates.iter().map(|candidate| candidate.login.clone()).collect();
    slot.child(
        Popover::new(SharedString::from(format!("tracker-twitch-menu-{side}-{index}"))).trigger(trigger).content(
            move |_state, _window, _cx| {
                let logins = logins.clone();
                v_flex().w(TWITCH_MENU_WIDTH).gap_1().p_1().children(logins.into_iter().enumerate().map(
                    |(slot_index, login)| {
                        let copied = login.clone();
                        Button::new(SharedString::from(format!("tracker-twitch-pick-{side}-{index}-{slot_index}")))
                            .label(login)
                            .compact()
                            .on_click(move |_event, window, cx: &mut App| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                                crate::toast::ok(t!("ui.twitch.copied", name = copied).into_owned(), window, cx);
                            })
                    },
                ))
            },
        ),
    )
    .into_any_element()
}

/// The chip's hover text: each candidate login and how many minutes from the
/// battle start it was seen, matching what the egui chip says.
fn sniper_hover_text(candidates: &[SniperCandidate]) -> String {
    let mut out = String::new();
    for candidate in candidates {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let minutes: Vec<String> = candidate.minutes.iter().map(|minute| minute.to_string()).collect();
        out.push_str(&t!("ui.twitch.possible_name", name = candidate.login));
        out.push('\n');
        out.push_str(&t!("ui.twitch.seen_minutes", minutes = minutes.join(", ")));
    }
    out.push_str("\n\n");
    out.push_str(&t!("ui.twitch.click_to_copy"));
    out
}

/// The row's ship-class glyph, tinted like its name. A fixed-width slot
/// either way, so the names below it stay aligned while the icons load.
fn class_icon(row: &LiveRosterRow, icons: &IconCache) -> AnyElement {
    let slot = div().flex_none().w(CLASS_COLUMN_WIDTH).h(CLASS_COLUMN_WIDTH);
    let Some(species) = row.species else {
        return slot.into_any_element();
    };
    match icons.get(species, tint_rgb(row.tint)) {
        Some(image) => {
            let class = SharedString::from(row.species_text.clone().unwrap_or_else(|| format!("{species:?}")));
            slot.id(SharedString::from(format!("tracker-class-{}", row.name)))
                .child(img(image).size_full())
                .tooltip(move |window, cx| Tooltip::new(class.clone()).build(window, cx))
                .into_any_element()
        }
        None => slot.into_any_element(),
    }
}

/// One live roster entry: the player, their ship, their figures in each
/// visible scope, and whether they have been met before.
fn roster_row(side: &'static str, index: usize, row: &LiveRosterRow, layout: RosterLayout) -> AnyElement {
    let name = match row.clan.as_deref() {
        Some(clan) => format!("[{clan}] {}", row.name),
        None => row.name.clone(),
    };

    // A row whose account the scan never named cannot be looked up at all,
    // which is a different absence from a player the service had no data for.
    let player = row.account_id.and_then(|id| layout.stats?.get(&id));
    let pending = layout.stats.is_none();
    let status = player.map_or(PlayerStatsStatus::Ok, |player| player.status);

    let cells: Vec<AnyElement> =
        layout.modes.iter().flat_map(|mode| scope_cells(row_stats(player, *mode), status, pending)).collect();

    // How many battles this player has been met in, counted the way the
    // tables beside it count: the division toggle hides the ones the user
    // arranged.
    let met = row
        .tracked
        .and_then(|account| layout.met.get(&account))
        .map(|tracked| tracked.visible_arena_ids(layout.count_division_mates).count())
        .unwrap_or_default();

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
        .child(class_icon(row, layout.icons))
        .child(twitch_chip(side, index, row, layout))
        .child(div().flex_1().min_w(px(0.)).text_sm().text_color(tint_color(row.tint)).truncate().child(name))
        .child(
            div()
                .w(SHIP_COLUMN_WIDTH)
                .text_xs()
                .text_color(crate::theme::text_dim())
                .truncate()
                .child(row.ship_name.clone().unwrap_or_else(|| "-".to_string())),
        )
        .children(cells)
        // How often this player has been met, in the tone that number
        // deserves -- the egui roster's own column, where the port had a
        // static "met before" string.
        .child(
            div()
                .w(MET_COLUMN_WIDTH)
                .text_xs()
                .when_some(severity_color(met), |el, color| el.text_color(color))
                .when(met == 0, |el| el.text_color(crate::theme::text_dim()))
                .child(if met == 0 { String::new() } else { separate_number(met as i64, None) }),
        )
        .child(roster_row_menu(side, index, row, layout.tracker))
        .into_any_element()
}

impl Render for PlayerTrackerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The settings row is read on the first frame rather than in `new`,
        // which runs before the config database is open.
        if !std::mem::replace(&mut self.view_loaded, true) {
            self.load_view(window, cx);
        }
        let border = cx.theme().border;

        // Not wrapping: a wrapped row does not grow its own height in this
        // layout, so a second line would be drawn over the table header
        // under it. The filter gives up width instead.
        let toolbar =
            h_flex()
                .flex_none()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(border)
                .child(
                    crate::ui::boxed(PERIOD_COMBO_WIDTH, crate::ui::SELECT_SMALL_HEIGHT).child(
                        Select::new(&self.period_select)
                            .id("tracker-period")
                            .accessibility_label(t!("ui.player_tracker.time_period").to_string())
                            .small()
                            // The menu takes the width of the element the popup
                            // is anchored to, which is the box this combo sits
                            // in rather than the combo itself.
                            .menu_width(PERIOD_COMBO_WIDTH),
                    ),
                )
                .child(crate::ui::rule_v(cx))
                .child(
                    h_flex().gap_1().items_center().flex_none().child(Icon::new(IconName::Search)).child(
                        div().w(FILTER_WIDTH).child(Input::new(&self.filter_input).id("tracker-filter").small()),
                    ),
                )
                // The checkbox sits at the far end rather than beside the filter,
                // so neither moves when the other changes size.
                .child(div().flex_1().min_w(px(0.)))
                // Offered whatever is docked: the tables count it and the
                // roster's own Seen column follows it too.
                .child(crate::ui::rule_v(cx))
                .child({
                    let show = self.show_division_mates;
                    Checkbox::new("tracker-show-division-mates")
                        .label(t!("ui.player_tracker.show_division_mates").to_string())
                        .checked(show)
                        .tooltip(t!("ui.player_tracker.show_division_mates_hover").to_string())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.set_show_division_mates(!show, cx);
                        }))
                });

        v_flex()
            .id("tracker-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(toolbar)
            // The sections are dock panels rather than a tab strip, so they
            // can be split and docked the way the egui tracker's are; the
            // dock draws its own tab bar over them.
            .child(div().flex_1().min_h(px(0.)).child(self.dock_area.clone()))
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
    use jiff::Timestamp;
    use wows_replays::types::AccountId;
    use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
    use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
    use wows_toolkit_viewmodel::match_stats::Region;
    use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentity;
    use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;

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

    /// The same shape as `ARENA_INFO` with names long enough for the Twitch
    /// matching rule, which ignores anything five bytes or shorter.
    const ARENA_INFO_LONG_NAMES: &str = r#"{
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
        "vehicles": [{"shipId": 100, "relation": 0, "id": 1, "name": "Harvey635"},
                     {"shipId": 200, "relation": 2, "id": 2, "name": "Stranger99"}],
        "playersPerTeam": 12,
        "dateTime": "28.12.2023 00:52:26",
        "mapName": "spaces/00_CO_ocean",
        "playerName": "Harvey635",
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
            .update(cx, |tracker, window, cx| {
                tracker.set_sub_tab(SubTab::CurrentMatch, window, cx);
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

    /// A player who was in chat around the battle gets a chip; a player who
    /// was not gets nothing.
    #[gpui_kit::test]
    async fn a_chat_login_that_names_a_player_puts_a_chip_on_their_row(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = temp_dir("twitch-chip");
        let window = cx.open_window(size(px(1200.), px(700.)), PlayerTrackerView::new);

        window
            .update(cx, |tracker, window, cx| {
                tracker.set_sub_tab(SubTab::CurrentMatch, window, cx);
                tracker.watch_live_matches(dir.clone(), GameDataCache::new(dir.join("game")), String::new(), cx);
            })
            .expect("the window is open");

        std::fs::write(dir.join(super::live::ARENA_INFO_FILE), ARENA_INFO_LONG_NAMES)
            .expect("the arena info is writable");
        cx.wait_for(window.into(), Duration::from_secs(30), |window, cx| {
            window.render_frame(cx);
            window.try_find("tracker-roster-ally-0").is_some()
        })
        .await;

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("tracker-twitch-ally-0").is_none(), "no observations, no chip");
        })
        .expect("the window is open");

        let started_at = window
            .update(cx, |tracker, _window, _cx| tracker.live_started_at().expect("a battle is under way"))
            .expect("the window is open");

        // One login names the ally, and one names nobody in this battle.
        window
            .update(cx, |tracker, _window, cx| {
                tracker.seed_chat_observations(
                    vec![("harvey_635".to_string(), started_at), ("someoneelse".to_string(), started_at)],
                    cx,
                );
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("tracker-twitch-ally-0").is_some(), "the login that names the ally chips them");
            assert!(window.try_find("tracker-twitch-enemy-0").is_none(), "the enemy nobody named keeps no chip");
        })
        .expect("the window is open");

        // An observation outside the window says nothing about this battle.
        let long_after = Timestamp::from_second(started_at.as_second() + 60 * 60).expect("a valid timestamp");
        window
            .update(cx, |tracker, _window, cx| {
                tracker.seed_chat_observations(vec![("harvey_635".to_string(), long_after)], cx);
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("tracker-twitch-ally-0").is_none(), "an hour later is not this battle");
        })
        .expect("the window is open");

        // Two logins that both name the ally: copying one of them would be a
        // guess, so the chip opens a picker instead.
        window
            .update(cx, |tracker, _window, cx| {
                tracker.seed_chat_observations(
                    vec![("harvey_635".to_string(), started_at), ("harvey635x".to_string(), started_at)],
                    cx,
                );
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("tracker-twitch-pick-ally-0-0").is_none(), "the picker opens from the chip");
            window.click("tracker-twitch-ally-0", cx);
        })
        .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("tracker-twitch-pick-ally-0-0").label(),
                Some("harvey635x"),
                "the picker lists the candidates alphabetically"
            );
            assert_eq!(window.find("tracker-twitch-pick-ally-0-1").label(), Some("harvey_635"));
        })
        .expect("the window is open");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The hover names every candidate and when each was seen, which is what
    /// the egui chip says.
    #[test]
    fn the_chip_hover_names_each_login_and_its_sightings() {
        use wows_toolkit_viewmodel::twitch::SniperCandidate;

        let hover = super::sniper_hover_text(&[
            SniperCandidate { login: "harvey635".to_string(), minutes: vec![-1, 5] },
            SniperCandidate { login: "harvey_635".to_string(), minutes: vec![3] },
        ]);

        // The catalogue's wording, which is what the replay table's own chip
        // says for the same candidate (`replay_inspector/table.rs`).
        assert!(hover.contains("Possible stream name: harvey635"), "got {hover:?}");
        assert!(hover.contains("Seen: -1, 5 minutes after match start"), "got {hover:?}");
        assert!(hover.contains("Possible stream name: harvey_635"), "got {hover:?}");
        assert!(hover.ends_with(&rust_i18n::t!("ui.twitch.click_to_copy").into_owned()), "got {hover:?}");
    }

    /// An opened row shows what the tracker knows beyond the columns.
    #[gpui_kit::test]
    fn a_historical_row_opens_on_its_aliases_and_account_id(cx: &mut TestAppContext) {
        use std::collections::HashMap;
        use wows_toolkit_config::index::rows::PlayerFacet;
        use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1000.), px(700.)), PlayerTrackerView::new);

        let account = AccountId(7);
        let players = vec![PlayerFacet {
            account_id: account,
            latest_name: "Harvey635".to_string(),
            clan: "WTK".to_string(),
            match_count: 3,
        }];
        let tracked: HashMap<AccountId, TrackedPlayer> = [(
            account,
            TrackedPlayer {
                last_name: "Harvey635".to_string(),
                names: ["Harvey635".to_string(), "Harvey42".to_string()].into_iter().collect(),
                ..TrackedPlayer::default()
            },
        )]
        .into_iter()
        .collect();

        window
            .update(cx, |tracker, _window, cx| tracker.seed_players_and_notes(players, tracked, cx))
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("tracker-player-detail", 0usize)).is_none(), "a row opens on request");

            window.click(("tracker-player-expand", 0usize), cx);
            window.render_frame(cx);
            assert!(window.try_find(("tracker-player-detail", 0usize)).is_some(), "the block is under the row");

            window.click(("tracker-player-expand", 0usize), cx);
            window.render_frame(cx);
            assert!(window.try_find(("tracker-player-detail", 0usize)).is_none(), "and closes again");
        })
        .expect("the window is open");
    }

    /// A row's magnifying glass asks the Search tab for every match the
    /// player was in.
    #[gpui_kit::test]
    fn finding_a_players_matches_names_a_query_the_search_tab_can_run(cx: &mut TestAppContext) {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::rc::Rc;
        use wows_toolkit_config::index::rows::PlayerFacet;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1000.), px(700.)), PlayerTrackerView::new);

        let account = AccountId(7);
        let players = vec![PlayerFacet {
            account_id: account,
            latest_name: "Harvey635".to_string(),
            clan: "WTK".to_string(),
            match_count: 3,
        }];
        window
            .update(cx, |tracker, _window, cx| tracker.seed_players_and_notes(players, HashMap::new(), cx))
            .expect("the window is open");

        let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let tracker = window.entity(cx).expect("the window has a root view");
        let recorder = seen.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(&tracker, move |_tracker, event: &super::PlayerTrackerEvent, _cx| {
                let super::PlayerTrackerEvent::SearchFor(query) = event;
                recorder.borrow_mut().push(query.clone());
            })
        });

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("tracker-find-player", 0usize), cx);
        })
        .expect("the window is open");

        let seen = seen.borrow();
        let query = seen.first().expect("the button asked for a search");
        assert!(query.contains("7"), "the query names the account, got {query:?}");
        assert!(
            wows_toolkit_config::index::query_text::parse_query(query).is_ok(),
            "the query parses back, got {query:?}"
        );
        drop(subscription);
    }

    /// A note opens for editing from its row, and what is typed is what the
    /// tab holds afterwards.
    #[gpui_kit::test]
    fn a_players_note_opens_from_its_row_and_keeps_what_is_typed(cx: &mut TestAppContext) {
        use wows_toolkit_config::index::rows::PlayerFacet;
        use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1000.), px(700.)), PlayerTrackerView::new);

        let account = AccountId(7);
        let players = vec![PlayerFacet {
            account_id: account,
            latest_name: "Harvey635".to_string(),
            clan: "WTK".to_string(),
            match_count: 3,
        }];
        let tracked = [(account, TrackedPlayer { notes: "camps the spawn".to_string(), ..TrackedPlayer::default() })]
            .into_iter()
            .collect();

        window
            .update(cx, |tracker, _window, cx| tracker.seed_players_and_notes(players, tracked, cx))
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("tracker-note-input").is_none(), "the editor opens from a row");
            window.click(("tracker-note", 0usize), cx);
            assert_eq!(
                window.find("tracker-note-input").value(),
                Some("camps the spawn"),
                "the editor opens on the stored note"
            );
        })
        .expect("the window is open");

        window
            .update(cx, |tracker, _window, cx| tracker.store_note(account, "divisions with a CV".to_string(), cx))
            .expect("the window is open");

        window
            .update(cx, |tracker, _window, _cx| {
                assert_eq!(tracker.note_for(account), Some("divisions with a CV"));
            })
            .expect("the window is open");
    }

    /// The roster's ship-class icons come out of the battle's own build, so
    /// this checks them against a real install rather than a fixture VFS.
    /// Run with:
    ///
    /// ```text
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture a_real_build_decodes_a_ship_class_icon
    /// ```
    #[gpui_kit::test]
    #[ignore = "needs a local game install"]
    fn a_real_build_decodes_a_ship_class_icon(cx: &mut TestAppContext) {
        use wowsunpack::game_params::types::Species;

        let wows_dir = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR to a WoWs install directory");
        let dir = std::path::PathBuf::from(&wows_dir);
        let build = wowsunpack::game_data::list_available_builds(&dir)
            .expect("the install lists its builds")
            .into_iter()
            .max()
            .expect("the install has at least one build");

        let game_data = GameDataCache::new(dir);
        let loaded = game_data.get_or_load_build(build).expect("the build's game data loads");

        cx.update(gpui_kit::init);
        let renderer = cx.update(|cx| cx.svg_renderer());
        let mut icons = crate::replay_inspector::IconCache::new();
        let tint = super::tint_rgb(super::PlayerTint::Ally);
        for species in [Species::Destroyer, Species::Cruiser, Species::Battleship, Species::AirCarrier] {
            icons.load_ship_class(species, tint, loaded.vfs(), &renderer);
            assert!(icons.get(species, tint).is_some(), "{species:?} has no class icon in build {build}");
        }
        println!("decoded {} tinted class icons from build {build}", icons.ship_class_count());
    }

    /// Compact shows one scope and offers to choose it; Detailed shows both
    /// and so has nothing to choose.
    #[gpui_kit::test]
    async fn the_view_modes_change_which_scopes_the_roster_shows(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = temp_dir("view-modes");
        let window = cx.open_window(size(px(1200.), px(700.)), PlayerTrackerView::new);

        window
            .update(cx, |tracker, window, cx| {
                tracker.set_sub_tab(SubTab::CurrentMatch, window, cx);
                tracker.watch_live_matches(dir.clone(), GameDataCache::new(dir.join("game")), String::new(), cx);
            })
            .expect("the window is open");

        std::fs::write(dir.join(super::live::ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is writable");
        cx.wait_for(window.into(), Duration::from_secs(30), |window, cx| {
            window.render_frame(cx);
            window.try_find("tracker-roster-ally-0").is_some()
        })
        .await;

        // Detailed is the default, and shows both scopes at once.
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let detailed = ("tracker-view-mode", CurrentMatchViewMode::Detailed as usize);
            assert_eq!(window.find(detailed).selected(), Some(true), "the roster opens detailed");
            assert!(
                window.try_find(("tracker-win-rate-mode", WinRateMode::Ship as usize)).is_none(),
                "showing both scopes leaves nothing to choose between"
            );
        })
        .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("tracker-view-mode", CurrentMatchViewMode::Compact as usize), cx);

            let overall = ("tracker-win-rate-mode", WinRateMode::Overall as usize);
            let ship = ("tracker-win-rate-mode", WinRateMode::Ship as usize);
            assert_eq!(window.find(overall).selected(), Some(true), "compact opens on the account scope");

            window.click(ship, cx);
            assert_eq!(window.find(ship).selected(), Some(true));
            assert_eq!(window.find(overall).selected(), Some(false), "one scope at a time");
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
            .update(cx, |tracker, window, cx| {
                tracker.set_sub_tab(SubTab::CurrentMatch, window, cx);
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

    /// The view the tracker was left in comes back, including a period that
    /// is not the default.
    #[test]
    fn the_saved_view_round_trips_through_the_settings_row() {
        use super::ClanSort;
        use super::Sort;
        use super::TrackerSettings;
        use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
        use wows_toolkit_viewmodel::player_tracker::SortColumn;
        use wows_toolkit_viewmodel::player_tracker::TimePeriod;

        let saved = TrackerSettings {
            period: TimePeriod::LastWeek,
            sort: Sort::default().toggled(SortColumn::Name),
            clan_sort: ClanSort::default().toggled(ClanSortColumn::Members),
            filter: "RAIN".to_string(),
        };
        let json = serde_json::to_string(&saved).expect("the view serializes");
        let read: TrackerSettings = serde_json::from_str(&json).expect("and reads back");
        assert_eq!(read, saved);

        // A row written before a field existed still reads, on the defaults.
        let older: TrackerSettings = serde_json::from_str("{}").expect("an empty row reads");
        assert_eq!(older, TrackerSettings::default());
    }
}

#[cfg(test)]
mod clan_table_tests {
    use gpui_kit::AppContext;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;

    use super::PlayerTrackerView;
    use super::SubTab;
    use jiff::Timestamp;
    use std::collections::HashMap;
    use wows_replays::types::AccountId;
    use wows_replays::types::ArenaId;
    use wows_toolkit_config::index::rows::PlayerFacet;
    use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;

    fn at(minute: i64) -> Timestamp {
        Timestamp::from_second(1_700_000_000 + minute * 60).expect("a valid timestamp")
    }

    /// The clans table counts the tracked encounters, so the division-mate
    /// toggle changes what it shows. Counting the replay index's rows instead
    /// would ignore the toggle and disagree with the egui app.
    #[gpui_kit::test]
    fn the_division_toggle_changes_what_the_clans_table_counts(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1000.), px(700.)), PlayerTrackerView::new);

        let mut player = TrackedPlayer { clan: "WTK".to_string(), ..TrackedPlayer::default() };
        for (arena, minute) in [(1i64, 10i64), (2, 20)] {
            player.arena_ids.insert(ArenaId::from(arena));
            player.timestamps.insert(at(minute));
        }
        // One of the two was a battle the user arranged.
        player.division_encounters.mark(ArenaId::from(2i64), at(20));
        let tracked = HashMap::from([(AccountId(7), player)]);

        window
            .update(cx, |tracker, window, cx| {
                tracker.set_sub_tab(SubTab::Clans, window, cx);
                tracker.seed_players_and_notes(Vec::new(), tracked, cx);

                let rows = tracker.clans();
                assert_eq!(rows.len(), 1, "the clan is listed from the tracked history");
                assert_eq!(rows[0].matches, 1, "the arranged battle is left out by default");
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("tracker-show-division-mates").checked(), Some(false));
            window.click("tracker-show-division-mates", cx);
        })
        .expect("the window is open");

        window
            .update(cx, |tracker, _window, _cx| {
                assert!(tracker.show_division_mates, "the toggle flipped");
                assert_eq!(tracker.clans()[0].matches, 2, "counting it back in adds the battle");
            })
            .expect("the window is open");
    }

    /// Two sections can be on screen at once, each with its own rows.
    ///
    /// The point of the dock: while the sections shared one list state, only
    /// one of them could be scrolled, so only one could usefully be shown.
    #[gpui_kit::test]
    fn two_sections_can_be_docked_side_by_side(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1400.), px(700.)), PlayerTrackerView::new);

        let mut player =
            TrackedPlayer { clan: "WTK".to_string(), last_name: "Harvey635".to_string(), ..TrackedPlayer::default() };
        player.arena_ids.insert(ArenaId::from(1i64));
        player.timestamps.insert(at(10));
        let tracked = HashMap::from([(AccountId(7), player)]);
        let players = vec![PlayerFacet {
            account_id: AccountId(7),
            latest_name: "Harvey635".to_string(),
            clan: "WTK".to_string(),
            match_count: 1,
        }];

        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");

        window
            .update(cx, |tracker, _window, cx| {
                let sections: Vec<SubTab> = tracker.panels.iter().map(|panel| panel.read(cx).section()).collect();
                assert_eq!(sections, SubTab::ALL.to_vec(), "one panel per section, in the egui tab's order");
                assert_eq!(tracker.panels[0].read(cx).list_len(), 0, "nothing is tracked yet");

                tracker.seed_players_and_notes(players, tracked, cx);
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");

        window
            .update(cx, |tracker, _window, cx| {
                let expected = tracker.rows().len();
                assert_eq!(expected, 1, "the seeded player is one row");
                assert_eq!(
                    tracker.panels[0].read(cx).list_len(),
                    expected,
                    "the drawn section reconciled its own list"
                );
                // The clans section has not been drawn, so it holds nothing
                // yet; its own list is what it reconciles, not this one's.
                assert_eq!(tracker.panels[2].read(cx).list_len(), 0);
            })
            .expect("the window is open");
    }

    /// A clans row opens on the members met from that clan.
    #[gpui_kit::test]
    fn a_clans_row_opens_on_the_members_met_from_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1000.), px(700.)), PlayerTrackerView::new);

        let mut player =
            TrackedPlayer { clan: "WTK".to_string(), last_name: "Harvey635".to_string(), ..TrackedPlayer::default() };
        player.arena_ids.insert(ArenaId::from(1i64));
        player.timestamps.insert(at(10));
        let tracked = HashMap::from([(AccountId(7), player)]);

        window
            .update(cx, |tracker, window, cx| {
                tracker.set_sub_tab(SubTab::Clans, window, cx);
                tracker.seed_players_and_notes(Vec::new(), tracked, cx);
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(("tracker-clan-members", 0usize)).is_none(), "the list opens on request");

            window.click(("tracker-clan-expand", 0usize), cx);
            window.render_frame(cx);
            assert!(window.try_find(("tracker-clan-members", 0usize)).is_some(), "the members are under the row");

            window.click(("tracker-clan-expand", 0usize), cx);
            window.render_frame(cx);
            assert!(window.try_find(("tracker-clan-members", 0usize)).is_none(), "and it closes again");
        })
        .expect("the window is open");
    }
}
