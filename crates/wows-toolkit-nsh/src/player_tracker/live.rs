//! The battle in progress: a watch over the replays directory, the roster it
//! reads resolved against the battle's own build, the identities and figures
//! the stats service has for it, and the Twitch chat seen around it.
//!
//! The watch and each battle's lookup run on threads of their own and bump
//! [`revision`] whenever what the Current Match section shows has changed,
//! which the view polls for.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;
use std::time::Instant;

use base64::Engine as _;
use jiff::Timestamp;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::types::AccountId;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::error::StatsError;
use wows_toolkit_replay_data::live_match;
use wows_toolkit_replay_data::live_match::StatsBudget;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_viewmodel::formatting::separate_number;
use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
use wows_toolkit_viewmodel::personal_rating;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::player_tracker::encounter_severity;
use wows_toolkit_viewmodel::player_tracker::history;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live::LiveRosterRow;
use wows_toolkit_viewmodel::player_tracker::live::RowStats;
use wows_toolkit_viewmodel::player_tracker::live::StatsLookupGate;
use wows_toolkit_viewmodel::player_tracker::live::TrackedIndex;
use wows_toolkit_viewmodel::player_tracker::live::WinRateMode;
use wows_toolkit_viewmodel::player_tracker::live::resolve_roster;
use wows_toolkit_viewmodel::player_tracker::live::row_stats;
use wows_toolkit_viewmodel::player_tracker::live::team_average_personal_rating;
use wows_toolkit_viewmodel::player_tracker::live::team_average_win_rate;
use wows_toolkit_viewmodel::player_tracker::live::visible_stat_modes;
use wows_toolkit_viewmodel::player_tracker::live::win_rate_hover;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveSource;
use wows_toolkit_viewmodel::player_tracker::live_source::LiveWatcher;
use wows_toolkit_viewmodel::player_tracker::live_source::POLL_INTERVAL;
use wows_toolkit_viewmodel::player_tracker::shipbuilds_player_url;
use wows_toolkit_viewmodel::player_tracker::wows_numbers_player_url;
use wows_toolkit_viewmodel::twitch;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

use crate::js_newtype::js_newtype;
use crate::replay_inspector::constants::BuildNumber;
use crate::text::text;
use crate::text::text_with;

use super::database;
use super::state::Tracker;
use super::view::AccountIdText;
use super::view::BandedText;
use super::view::LiveView;
use super::view::RatingBand;
use super::view::RosterRowView;
use super::view::ScopeCellsView;
use super::view::ScopeHeadings;
use super::view::StatsMark;
use super::view::TeamView;

/// How often the chat observations are re-read while a battle is under way.
///
/// The chat poll writes them every two minutes, so reading faster than that
/// only repeats the same rows.
const CHAT_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Counts changes to what the Current Match section shows; the view polls it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveRevision(u32);

js_newtype!(LiveRevision, u32);

impl LiveRevision {
    fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Counts watches of the replays directory, so a thread whose directory was
/// replaced stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WatchGeneration(u64);

impl WatchGeneration {
    const FIRST: Self = Self(1);

    fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Why the roster's ships or figures are missing.
#[derive(Debug, thiserror::Error)]
pub enum LiveDataError {
    #[error("this battle's game data did not load: no World of Warships directory is bound")]
    NotConfigured,
    #[error("this battle's game data did not load: {0}")]
    Build(ReplayLoadError),
    #[error("{}", live_match::refusal_text(.0))]
    Stats(StatsError),
    #[error("the stats request could not start: {0}")]
    Runtime(std::io::Error),
}

/// Where the stats lookup for the battle in progress has got to.
#[derive(Debug, Default)]
enum StatsState {
    /// No battle, or one whose build has not been asked for.
    #[default]
    Idle,
    /// Loading the build, or waiting for the game to flush the packet that
    /// names the roster.
    Scanning,
    /// Ready to send, waiting for the Current Match section to be shown.
    Held,
    Fetching,
    Ready(HashMap<AccountId, PlayerStatsOut>),
    /// Reported rather than retried.
    Failed(LiveDataError),
}

/// A battle's stats lookup, held until the Current Match section is shown.
struct HeldLookup {
    generation: WatchGeneration,
    started_at: Timestamp,
    state: ArenaState,
}

/// What the session has asked the stats service, kept across battles and
/// directory changes so its budget is respected.
#[derive(Default)]
struct Lookups {
    budget: StatsBudget,
    gate: StatsLookupGate<HeldLookup>,
}

/// The game data a battle's roster resolves against.
struct BuildShips {
    build: BuildNumber,
    provider: Arc<GameMetadataProvider>,
}

/// The battle in progress, as the watch last saw it.
#[derive(Default)]
pub struct LiveState {
    /// The directory being watched; `None` while no WoWs directory is set.
    replay_dir: Option<PathBuf>,
    /// Whether the watch has looked at the directory yet.
    checked: bool,
    live_match: Option<LiveMatch>,
    /// Which of the battle's players the index has met, keyed by lower-cased
    /// name. All time, so it does not follow the period selector.
    met_before: HashMap<String, AccountId>,
    /// Twitch logins seen in chat around the battle, and when.
    chat: Vec<(String, Timestamp)>,

    /// The battle's build, which resolves ships and classes.
    ships: Option<BuildShips>,
    /// Each roster class's icon from that build.
    class_icons: HashMap<Species, Arc<str>>,
    /// Account ids, realms and clans the scan recovered; `tempArenaInfo`
    /// carries none of them.
    identities: Option<LiveIdentities>,
    stats: StatsState,
    lookups: Lookups,

    revision: LiveRevision,
    /// The watch running, `None` before the first.
    watch: Option<WatchGeneration>,
}

impl LiveState {
    /// Whether `generation`'s battle starting at `started_at` is still the
    /// one shown, so work that outlived its battle does not land on the next.
    fn is_current(&self, generation: WatchGeneration, started_at: Timestamp) -> bool {
        self.watch == Some(generation) && started_at_of(self) == Some(started_at)
    }

    fn bump(&mut self) {
        self.revision = self.revision.next();
    }
}

static LIVE: LazyLock<Mutex<LiveState>> = LazyLock::new(|| Mutex::new(LiveState::default()));

fn live() -> MutexGuard<'static, LiveState> {
    // A panicking watch thread leaves the state as it was last written, which
    // is still a consistent snapshot to draw.
    LIVE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Bumped whenever what [`view`] returns has changed.
pub fn revision() -> LiveRevision {
    live().revision
}

/// Watches the replays directory under `wows_dir`, replacing the watch
/// already running when the directory differs. `None` stops watching.
pub fn watch(wows_dir: Option<&Path>) {
    let replay_dir = wows_dir.map(|dir| dir.join("replays"));

    let generation = {
        let mut state = live();
        if state.replay_dir == replay_dir && (replay_dir.is_none() || state.watch.is_some()) {
            return;
        }
        let watch = state.watch.map_or(WatchGeneration::FIRST, WatchGeneration::next);
        let revision = state.revision.next();
        let mut lookups = std::mem::take(&mut state.lookups);
        lookups.gate.clear();
        *state =
            LiveState { replay_dir: replay_dir.clone(), lookups, watch: Some(watch), revision, ..LiveState::default() };
        watch
    };

    let Some(replay_dir) = replay_dir else { return };
    spawn("player-tracker-live", move || run_watch(generation, &replay_dir));
}

/// Records whether the Current Match section is on screen, sending the
/// lookup held for it once it is.
pub fn set_shown(shown: bool) {
    let sending = {
        let mut state = live();
        let held = state.lookups.gate.set_shown(shown);
        held.and_then(|lookup| send_lookup(&mut state, lookup))
    };

    if let Some(lookup) = sending {
        spawn("player-tracker-stats", move || fetch(lookup));
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(err) = std::thread::Builder::new().name(name.to_owned()).spawn(work) {
        log::error!("player tracker: {name} could not start: {err}");
    }
}

/// The chat read for the battle in progress, while its window is open.
struct ChatPoll {
    started_at: Timestamp,
    start: Timestamp,
    end: Timestamp,
    next: Instant,
}

impl ChatPoll {
    /// The poll for a battle that started at `started_at`, due now. A window
    /// edge past the range a timestamp can hold is held at that range's end.
    fn new(started_at: Timestamp) -> Self {
        // Whole seconds, as the observations are stored.
        let offset = |minutes: f64| {
            let second = started_at.as_second().saturating_add((minutes * 60.0) as i64);
            let second = second.clamp(Timestamp::MIN.as_second(), Timestamp::MAX.as_second());
            Timestamp::from_second(second).expect("the second is clamped to the representable range")
        };

        Self {
            started_at,
            start: offset(twitch::WINDOW_BEFORE_MINUTES),
            end: offset(twitch::WINDOW_AFTER_MINUTES),
            next: Instant::now(),
        }
    }
}

/// A battle the watch has just adopted, as its lookups need it.
struct AdoptedBattle {
    started_at: Timestamp,
    names: Vec<String>,
    build: Option<BuildNumber>,
}

fn run_watch(generation: WatchGeneration, replay_dir: &Path) {
    let source = LiveSource::in_dir(replay_dir);
    let mut watcher = LiveWatcher::new();
    let mut chat: Option<ChatPoll> = None;

    while live().watch == Some(generation) {
        if let Some(found) = watcher.tick(&source) {
            chat = found.as_ref().map(|found| ChatPoll::new(found.started_at));
            adopt(generation, replay_dir, found);
        }

        if let Some(poll) = chat.as_mut()
            && Instant::now() >= poll.next
        {
            read_chat(generation, poll);
            poll.next = Instant::now() + CHAT_POLL_INTERVAL;
            // Past the window there is nothing further to find.
            if Timestamp::now().as_second() > poll.end.as_second() {
                chat = None;
            }
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Takes what a poll found, asks the index which of its players were met
/// before, and starts resolving the battle against its build.
fn adopt(generation: WatchGeneration, replay_dir: &Path, found: Option<LiveMatch>) {
    let battle = {
        let mut state = live();
        if state.watch != Some(generation) {
            return;
        }
        state.checked = true;
        state.met_before.clear();
        state.chat.clear();
        // A new battle carries its own roster and its own figures; the
        // previous battle's must not show under it.
        state.identities = None;
        state.stats = StatsState::Idle;
        state.lookups.gate.clear();
        let build = found.as_ref().and_then(|found| found.build).map(BuildNumber);
        // The same build keeps resolving the ships while it is asked for
        // again, rather than flashing the roster back to unresolved.
        if state.ships.as_ref().map(|ships| ships.build) != build {
            state.ships = None;
            state.class_icons.clear();
        }
        let battle = found.as_ref().map(|found| AdoptedBattle {
            started_at: found.started_at,
            names: found.players.iter().map(|player| player.name.clone()).collect(),
            build,
        });
        if build.is_some() {
            state.stats = StatsState::Scanning;
        }
        state.live_match = found;
        state.bump();
        battle
    };

    let Some(AdoptedBattle { started_at, names, build }) = battle else { return };
    if let Some(build) = build {
        let replay_dir = replay_dir.to_owned();
        spawn("player-tracker-battle", move || resolve_battle(generation, started_at, build, &replay_dir));
    }

    match database::run(async |pool| database::accounts_named(pool, &names).await) {
        Ok(accounts) => {
            let mut state = live();
            if state.is_current(generation, started_at) {
                state.met_before = accounts;
                state.bump();
            }
        }
        // The roster still lists everyone; only the met counts stay empty.
        Err(err) => log::warn!("player tracker: the met-before lookup failed: {}", err.chain()),
    }
}

/// Loads the battle's build, reads the roster's identities off the packet
/// stream, and offers the stats lookup.
///
/// The stream arrives in flushes, so the scan retries on a timer until the
/// packet naming the roster lands or the budget runs out.
fn resolve_battle(generation: WatchGeneration, started_at: Timestamp, build: BuildNumber, replay_dir: &Path) {
    let metadata = match load_build(crate::game_data::cache(), build) {
        Ok(metadata) => metadata,
        // The roster still lists names and relations; the ship columns and
        // the figures have nothing to resolve against, which is a failure
        // rather than a wait.
        Err(err) => {
            log::warn!("player tracker: build {build} did not load: {err}");
            let mut state = live();
            if state.is_current(generation, started_at) {
                state.stats = StatsState::Failed(err);
                state.bump();
            }
            return;
        }
    };

    let ship_ids: Vec<_> = {
        let state = live();
        if !state.is_current(generation, started_at) {
            return;
        }
        state.live_match.iter().flat_map(|live| live.players.iter().map(|player| player.ship_id)).collect()
    };
    let icons = class_icons(&metadata, &ship_ids);
    {
        let mut state = live();
        if !state.is_current(generation, started_at) {
            return;
        }
        state.ships = Some(BuildShips { build, provider: Arc::clone(metadata.provider()) });
        state.class_icons = icons;
        state.bump();
    }

    let source = LiveSource::in_dir(replay_dir);
    let deadline = Instant::now() + live_match::SCAN_RETRY_BUDGET;
    let scanned = loop {
        if !live().is_current(generation, started_at) {
            return;
        }
        if let Some(scanned) = live_match::scan_arena(&source, metadata.provider()) {
            break Some(scanned);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(live_match::SCAN_RETRY_INTERVAL);
    };

    let sending = {
        let mut state = live();
        if !state.is_current(generation, started_at) {
            return;
        }
        state.bump();
        let Some(scanned) = scanned else {
            state.stats = StatsState::Failed(LiveDataError::Stats(StatsError::NoRoster));
            return;
        };
        // Written before the lookup, so the clan tags and the account join
        // show even when the lookup then fails, is refused, or waits.
        state.identities = Some(LiveIdentities::from_player_states(&scanned.players));
        offer_lookup(&mut state, HeldLookup { generation, started_at, state: scanned })
    };

    if let Some(lookup) = sending {
        fetch(lookup);
    }
}

/// `build`'s game data from the cache bound to the WoWs directory, which
/// keeps a build once it has loaded.
fn load_build(cache: Option<GameDataCache>, build: BuildNumber) -> Result<Arc<LoadedGameData>, LiveDataError> {
    let cache = cache.ok_or(LiveDataError::NotConfigured)?;

    cache.get_or_load_build(build.0).map_err(LiveDataError::Build)
}

/// Each class of `ships` with its icon in `metadata`'s build, as an SVG
/// `data:` URL. A class the build has no icon for is left out.
fn class_icons(metadata: &LoadedGameData, ships: &[wows_replays::types::GameParamId]) -> HashMap<Species, Arc<str>> {
    let provider = metadata.provider();
    let mut icons = HashMap::new();
    for ship in ships {
        let Some(species) = provider.game_param_by_id(*ship).and_then(|param| param.species()?.known().copied()) else {
            continue;
        };
        if icons.contains_key(&species) {
            continue;
        }
        if let Some(icon) = class_icon(metadata.vfs(), species) {
            icons.insert(species, icon);
        }
    }

    icons
}

fn class_icon(vfs: &wowsunpack::vfs::VfsPath, species: Species) -> Option<Arc<str>> {
    let bytes = GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }.read(vfs, None)?;

    Some(Arc::from(format!("data:image/svg+xml;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

/// Answers `lookup` from what this session already has, returns it to be
/// sent while the Current Match section is shown, or holds it until it is.
fn offer_lookup(state: &mut LiveState, lookup: HeldLookup) -> Option<HeldLookup> {
    // An answer this session already has costs no request.
    if let Some(answered) = state.lookups.budget.answered(lookup.state.arena_id) {
        state.stats = StatsState::Ready(index_by_account(answered.players.clone()));
        return None;
    }

    match state.lookups.gate.offer(lookup) {
        Some(lookup) => send_lookup(state, lookup),
        None => {
            state.stats = StatsState::Held;
            None
        }
    }
}

/// Spends the budget on `lookup`, returning it for the caller to send
/// outside the lock; `None` when the budget refuses it or its battle is over.
fn send_lookup(state: &mut LiveState, lookup: HeldLookup) -> Option<HeldLookup> {
    if !state.is_current(lookup.generation, lookup.started_at) {
        return None;
    }
    state.bump();

    if let Err(refusal) = state.lookups.budget.check(lookup.state.arena_id, Instant::now()) {
        state.stats = StatsState::Failed(LiveDataError::Stats(refusal.into()));
        return None;
    }
    state.lookups.budget.record(Instant::now());
    state.stats = StatsState::Fetching;

    Some(lookup)
}

/// Asks the stats service about `lookup`'s roster. Blocking.
fn fetch(lookup: HeldLookup) {
    let proxy_url = crate::game_data::proxy_url();
    let fetched = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime
            .block_on(live_match::fetch_stats(crate::game_data::USER_AGENT, &lookup.state, proxy_url.as_deref()))
            .map_err(LiveDataError::Stats),
        Err(err) => Err(LiveDataError::Runtime(err)),
    };

    let mut state = live();
    if !state.is_current(lookup.generation, lookup.started_at) {
        return;
    }
    state.stats = match fetched {
        Ok(response) => {
            let players = index_by_account(response.players.clone());
            state.lookups.budget.remember(response);
            StatsState::Ready(players)
        }
        Err(err) => {
            if let LiveDataError::Stats(stats) = &err {
                state.lookups.budget.note_failure(lookup.state.arena_id, stats, Instant::now());
            }
            StatsState::Failed(err)
        }
    };
    state.bump();
}

/// The service's answer keyed by the account each row joins on.
fn index_by_account(players: Vec<PlayerStatsOut>) -> HashMap<AccountId, PlayerStatsOut> {
    players.into_iter().map(|player| (player.account_id, player)).collect()
}

fn read_chat(generation: WatchGeneration, poll: &ChatPoll) {
    let start = poll.start;
    let end = poll.end;
    match database::run(async move |pool| database::chat_observations(pool, start, end).await) {
        Ok(observations) => {
            let mut state = live();
            if state.is_current(generation, poll.started_at) {
                state.chat = observations;
                state.bump();
            }
        }
        // The roster still lists everyone; only the chips are missing.
        Err(err) => log::warn!("player tracker: the chat lookup failed: {}", err.chain()),
    }
}

fn started_at_of(state: &LiveState) -> Option<Timestamp> {
    state.live_match.as_ref().map(|live| live.started_at)
}

/// The Current Match section as it stands.
pub fn view(tracker: &Tracker, now: Timestamp) -> LiveView {
    let state = live();
    state_view(&state, tracker, now)
}

/// What a roster draws, bundled so the row and team helpers take one value.
struct RosterContext<'a> {
    state: &'a LiveState,
    tracker: &'a Tracker,
    stats: Option<&'a HashMap<AccountId, PlayerStatsOut>>,
    modes: &'a [WinRateMode],
    started_at: Timestamp,
    now: Timestamp,
}

fn state_view(state: &LiveState, tracker: &Tracker, now: Timestamp) -> LiveView {
    let modes = visible_stat_modes(tracker.modes.current_match_view_mode, tracker.modes.win_rate_mode);
    let scope_headings = modes
        .iter()
        .map(|mode| {
            let scope = text(mode.label_key());
            let heading = |key: &str| format!("{scope} {}", text(key));
            ScopeHeadings {
                win_rate: heading("ui.player_tracker.column.win_rate"),
                personal_rating: heading("ui.player_tracker.column.personal_rating"),
                avg_damage: heading("ui.player_tracker.column.avg_damage"),
                battles: heading("ui.player_tracker.column.battles"),
            }
        })
        .collect();

    let status = match (&state.replay_dir, state.checked, &state.live_match) {
        (None, _, _) => Some(text("ui.player_tracker.live_no_directory")),
        (Some(_), false, _) => Some(text("ui.player_tracker.live_checking")),
        (Some(_), true, None) => Some(text("ui.player_tracker.live_none")),
        (Some(_), true, Some(_)) => None,
    };

    let stats = match &state.stats {
        StatsState::Ready(players) => Some(players),
        _ => None,
    };

    let Some(live) = state.live_match.as_ref() else {
        // No row reads `started_at` from an empty roster.
        let context = RosterContext { state, tracker, stats, modes: &modes, started_at: now, now };
        return LiveView {
            status,
            note: None,
            scope_headings,
            friendly: team_view("ui.player_tracker.allies", &[], &context),
            enemy: team_view("ui.player_tracker.enemies", &[], &context),
        };
    };

    let index = TrackedIndex { by_name: state.met_before.clone(), players: state.met_before.len() };
    let provider = state.ships.as_ref().map(|ships| ships.provider.as_ref());
    let roster = resolve_roster(live, &index, state.identities.as_ref(), provider);
    let note = match &state.stats {
        // A failure is reported even while the ships are unresolved: a build
        // that did not load is why they are unresolved.
        StatsState::Failed(err) => {
            Some(text_with("ui.player_tracker.stats_unavailable", &[("reason", &err.to_string())]))
        }
        _ if !roster.ships_resolved => Some(text("ui.player_tracker.live_loading_build")),
        StatsState::Scanning => Some(text("ui.player_tracker.stats_resolving")),
        StatsState::Held | StatsState::Fetching => Some(text("ui.player_tracker.stats_fetching")),
        StatsState::Idle | StatsState::Ready(_) => None,
    };

    let context = RosterContext { state, tracker, stats, modes: &modes, started_at: roster.started_at, now };
    LiveView {
        status,
        note,
        scope_headings,
        friendly: team_view("ui.player_tracker.allies", &roster.friendly, &context),
        enemy: team_view("ui.player_tracker.enemies", &roster.enemy, &context),
    }
}

/// One team: its name, its size, what it averages over the scope its rows
/// lead with, and its rows. A team nobody has figures for shows no average
/// rather than a zero.
fn team_view(title_key: &str, rows: &[LiveRosterRow], context: &RosterContext) -> TeamView {
    // `visible_stat_modes` names at least one scope; the fallback is unused.
    let lead = context.modes.first().copied().unwrap_or(WinRateMode::Overall);
    let average_win_rate = context.stats.and_then(|stats| team_average_win_rate(rows, stats, lead)).map(|rate| {
        BandedText {
            text: text_with("ui.player_tracker.team_average_win_rate", &[("rate", &format!("{rate:.1}%"))]),
            band: band(PersonalRatingCategory::from_win_rate(rate)),
        }
    });
    let average_rating =
        context.stats.and_then(|stats| team_average_personal_rating(rows, stats, lead)).map(|rating| BandedText {
            text: format!("{}: {rating:.0}", text("stat.avg_pr")),
            band: band(PersonalRatingCategory::from_pr(rating)),
        });

    TeamView {
        title: text(title_key),
        players: text_with("ui.player_tracker.team_players", &[("count", &rows.len().to_string())]),
        average_win_rate,
        average_rating,
        rows: rows.iter().map(|row| roster_row(row, context)).collect(),
    }
}

fn roster_row(row: &LiveRosterRow, context: &RosterContext) -> RosterRowView {
    let show_division_mates = context.tracker.modes.show_division_mates;

    // How many battles this player has been met in, counted the way the
    // tables beside it count.
    let tracked = row.tracked.and_then(|account| context.tracker.tracked.get(&account));
    // A player never recorded was met in no battle.
    let met = tracked.map_or(0, |tracked| tracked.visible_arena_ids(show_division_mates).count());
    let met_hover = match tracked {
        Some(tracked) if met > 0 => {
            let total = tracked.arena_ids.len().to_string();
            let last = tracked
                .last_visible_timestamp(show_division_mates)
                .map(|at| history::last_seen_text(Some(at), context.now))
                .unwrap_or_else(|| text("ui.player_tracker.never_encountered"));
            text_with(
                "ui.player_tracker.encounters_hover",
                &[("total", &total), ("range", &met.to_string()), ("last", &last)],
            )
        }
        _ => text("ui.player_tracker.never_encountered"),
    };

    let observations = context.state.chat.iter().map(|(login, seen_at)| (login.as_str(), *seen_at));
    let candidates = twitch::sniper_candidates(observations, &row.name, context.started_at);
    let twitch_hover = if candidates.is_empty() { String::new() } else { twitch::sniper_hover_text(&candidates) };

    // A row whose account the scan never named cannot be looked up at all,
    // which is a different absence from a player the service had no data for.
    let player = row.account_id.and_then(|id| context.stats?.get(&id));
    let pending = context.stats.is_none();
    // Without an answer row the cells show dashes or wait, not a mark.
    let status = player.map_or(PlayerStatsStatus::Ok, |player| player.status);
    let hover_both_scopes = win_rate_hover(player, None);
    let scopes = context
        .modes
        .iter()
        .map(|mode| scope_cells(row_stats(player, *mode), status, pending, *mode, hover_both_scopes.clone()))
        .collect();
    // `visible_stat_modes` names at least one scope; the fallback is unused.
    let lead = context.modes.first().copied().unwrap_or(WinRateMode::Overall);

    let links = row.account_id.zip(row.region);
    let class_icon = row.species.and_then(|species| context.state.class_icons.get(&species));

    RosterRowView {
        name: match row.clan.as_deref() {
            Some(clan) => format!("[{clan}] {}", row.name),
            None => row.name.clone(),
        },
        tint: row.tint.into(),
        class_icon: class_icon.map(|icon| icon.to_string()),
        class_name: class_icon.and(row.species_text.clone()),
        // A ship the build has not resolved reads as a dash.
        ship: row.ship_name.clone().unwrap_or_else(|| "-".to_string()),
        band: row_stats(player, lead).band.map(band),
        scopes,
        met: if met == 0 { String::new() } else { separate_number(met as i64, None) },
        met_severity: encounter_severity(met).into(),
        met_hover,
        note: tracked.map(|tracked| tracked.notes.clone()).filter(|notes| !notes.trim().is_empty()),
        twitch_logins: candidates.into_iter().map(|candidate| candidate.login).collect(),
        twitch_hover,
        wows_numbers_url: links.map(|(account, region)| wows_numbers_player_url(region, account, &row.name)),
        shipbuilds_url: links.map(|(account, region)| shipbuilds_player_url(region, account, &row.name)),
        account_id: links.map(|(account, _)| AccountIdText::from(account)),
    }
}

/// The figures one scope contributes to a row.
fn scope_cells(
    stats: RowStats,
    status: PlayerStatsStatus,
    pending: bool,
    mode: WinRateMode,
    win_rate_hover: Option<String>,
) -> ScopeCellsView {
    // A player who hid their statistics is a different answer from one the
    // service had nothing for, and both differ from one it could not reach.
    if status != PlayerStatsStatus::Ok {
        let (mark, hover) = match status {
            PlayerStatsStatus::Hidden => (StatsMark::Hidden, "ui.player_tracker.stats_hidden"),
            PlayerStatsStatus::Unavailable => (StatsMark::Unavailable, "ui.player_tracker.stats_no_data"),
            PlayerStatsStatus::Unknown | PlayerStatsStatus::Ok => (StatsMark::Unknown, "ui.player_tracker.stats_no_data"),
        };
        return ScopeCellsView {
            mark: Some(mark),
            mark_hover: Some(text(hover)),
            win_rate: String::new(),
            win_rate_band: None,
            win_rate_hover: None,
            personal_rating: cell(None, false),
            personal_rating_band: None,
            personal_rating_hover: String::new(),
            avg_damage: cell(None, false),
            battles: cell(None, false),
        };
    }

    ScopeCellsView {
        mark: None,
        mark_hover: None,
        win_rate: cell(stats.win_rate.map(|rate| format!("{rate:.1}%")), pending),
        win_rate_band: stats.band.map(band),
        win_rate_hover: win_rate_hover.filter(|hover| !hover.is_empty()),
        personal_rating: cell(stats.pr.map(|pr| format!("{pr:.0}")), pending),
        personal_rating_band: stats.pr.map(|pr| band(PersonalRatingCategory::from_pr(pr))),
        personal_rating_hover: text(match mode {
            WinRateMode::Overall => "ui.player_tracker.pr_overall_hover",
            WinRateMode::Ship => "ui.player_tracker.pr_ship_hover",
        }),
        // Grouped, as every other figure this size in the app is.
        avg_damage: cell(stats.avg_damage.map(|damage| separate_number(damage, None)), pending),
        battles: cell(stats.battles.map(|battles| separate_number(battles, None)), pending),
    }
}

/// A figure, a dash when the service gave none, and nothing while the lookup
/// is still running.
fn cell(value: Option<String>, pending: bool) -> String {
    match (value, pending) {
        (Some(value), _) => value,
        (None, true) => String::new(),
        (None, false) => "-".to_string(),
    }
}

fn band(category: PersonalRatingCategory) -> RatingBand {
    let hex = |color: u32| format!("#{color:06x}");
    RatingBand {
        hue: hex(personal_rating::chip_hue(category)),
        dark_text: hex(personal_rating::chip_text(category, true)),
        light_text: hex(personal_rating::chip_text(category, false)),
    }
}

#[cfg(test)]
mod tests {
    use jiff::ToSpan as _;
    use wows_replays::types::ArenaId;
    use wows_replays::types::GameParamId;
    use wows_replays::types::Relation;
    use wows_toolkit_viewmodel::match_stats::MatchStatsResponse;
    use wows_toolkit_viewmodel::match_stats::Region;
    use wows_toolkit_viewmodel::player_tracker::live::LiveIdentity;
    use wows_toolkit_viewmodel::player_tracker::live::LiveMatchPlayer;
    use wows_toolkit_viewmodel::player_tracker::live_source::ARENA_INFO_FILE;
    use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;

    use super::*;

    /// A `tempArenaInfo.json` naming the reader, an ally and an enemy.
    const ARENA_INFO: &str = r#"{
        "gameMode": 7, "clientVersionFromExe": "13, 11, 0, 12668706", "mapDisplayName": "ocean",
        "mapId": 1, "clientVersionFromXml": "13, 11, 0, 12668706", "duration": 1200, "gameLogic": null,
        "name": "12x12", "scenario": "Domination", "playerID": 0,
        "vehicles": [{"shipId": 100, "relation": 0, "id": 1, "name": "Harvey635"},
                     {"shipId": 100, "relation": 1, "id": 2, "name": "Friendly42"},
                     {"shipId": 200, "relation": 2, "id": 3, "name": "Stranger99"}],
        "playersPerTeam": 12, "dateTime": "28.12.2023 00:52:26", "mapName": "spaces/00_CO_ocean",
        "playerName": "Harvey635", "scenarioConfigId": 1, "teamsCount": 2, "logic": null,
        "playerVehicle": "PFSD110-Kleber"
    }"#;

    fn battle(started_at: Timestamp) -> LiveMatch {
        let player = |name: &str, relation: u32| LiveMatchPlayer {
            name: name.to_string(),
            ship_id: GameParamId::from(100u64),
            relation: Relation::new(relation),
        };
        LiveMatch {
            started_at,
            build: Some(1),
            players: vec![player("Harvey635", 0), player("Stranger99", 2), player("Friendly42", 1)],
        }
    }

    fn watching(started_at: Timestamp) -> LiveState {
        LiveState {
            replay_dir: Some(PathBuf::from("/replays")),
            checked: true,
            live_match: Some(battle(started_at)),
            stats: StatsState::Failed(LiveDataError::NotConfigured),
            watch: Some(WatchGeneration(1)),
            ..LiveState::default()
        }
    }

    /// A build that resolved, with no ships of its own to name.
    fn resolved() -> Option<BuildShips> {
        let provider = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider");
        Some(BuildShips { build: BuildNumber(1), provider: Arc::new(provider) })
    }

    fn figures(account: i64, status: PlayerStatsStatus) -> PlayerStatsOut {
        PlayerStatsOut {
            account_id: AccountId(account),
            region: "na".to_string(),
            ship_id: GameParamId::from(100u64),
            status,
            battles: Some(1234),
            overall_win_rate: Some(54.25),
            overall_avg_damage: Some(61_500),
            ship_win_rate: Some(48.0),
            ship_battles: Some(87),
            ship_avg_damage: None,
            ship_pr: None,
            pr: Some(1650.0),
        }
    }

    fn identities() -> LiveIdentities {
        let identity = |account| LiveIdentity {
            account_id: AccountId(account),
            region: Some(Region::Na),
            clan: None,
            clan_color: None,
        };
        LiveIdentities {
            by_name: [("harvey635".to_string(), identity(1)), ("stranger99".to_string(), identity(2))].into(),
        }
    }

    fn lookup(started_at: Timestamp, arena: i64) -> HeldLookup {
        HeldLookup { generation: WatchGeneration(1), started_at, state: ArenaState { arena_id: ArenaId::from(arena), players: Vec::new() } }
    }

    #[test]
    fn no_directory_no_look_and_no_battle_each_say_so() {
        let tracker = Tracker::new();
        let now = Timestamp::now();

        let unset = LiveState::default();
        assert_eq!(state_view(&unset, &tracker, now).status, Some(text("ui.player_tracker.live_no_directory")));

        let checking = LiveState { replay_dir: Some(PathBuf::from("/replays")), ..LiveState::default() };
        assert_eq!(state_view(&checking, &tracker, now).status, Some(text("ui.player_tracker.live_checking")));

        let idle = LiveState { checked: true, ..checking };
        assert_eq!(state_view(&idle, &tracker, now).status, Some("No battle in progress".to_string()));
    }

    /// The roster a `tempArenaInfo.json` names, before its build has
    /// resolved any ship: every player listed by side, ships as dashes.
    #[test]
    fn a_fixture_arena_info_lists_the_roster_by_side() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(dir.path().join(ARENA_INFO_FILE), ARENA_INFO).expect("the arena info is written");
        let found = wows_toolkit_viewmodel::player_tracker::live_source::read(&LiveSource::in_dir(dir.path()))
            .expect("the arena info reads as a battle");
        assert_eq!(found.build, Some(12_668_706));

        let state = LiveState {
            replay_dir: Some(dir.path().to_owned()),
            checked: true,
            live_match: Some(found),
            stats: StatsState::Scanning,
            ..LiveState::default()
        };
        let view = state_view(&state, &Tracker::new(), Timestamp::now());

        let names = |team: &TeamView| team.rows.iter().map(|row| row.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&view.friendly).len(), 2);
        assert!(names(&view.friendly).contains(&"Harvey635".to_string()));
        assert_eq!(names(&view.enemy), vec!["Stranger99".to_string()]);
        assert!(view.friendly.rows.iter().all(|row| row.ship == "-" && row.class_icon.is_none()));
        assert_eq!(view.note, Some(text("ui.player_tracker.live_loading_build")));
    }

    /// A WoWs directory with no game in it has no build to resolve against,
    /// which the roster reports rather than waiting on.
    #[test]
    fn a_build_the_install_lacks_is_reported_as_unavailable() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        // The cache keeps its dumps under the app's storage, never the user's.
        wows_toolkit_config::storage_override::set(
            std::env::temp_dir().join(format!("wows-toolkit-nsh-tracker-storage-{}", std::process::id())),
        );

        let missing = load_build(None, BuildNumber(12_668_706)).err().expect("no cache, no build");
        assert!(matches!(missing, LiveDataError::NotConfigured));

        let err = load_build(Some(GameDataCache::new(dir.path().to_owned())), BuildNumber(12_668_706)).err().expect("no install");
        assert!(matches!(err, LiveDataError::Build(_)));
        assert!(err.to_string().starts_with("this battle's game data did not load: "), "got {err}");
    }

    #[test]
    fn the_roster_splits_by_team_and_counts_who_was_met_before() {
        let now = Timestamp::now();
        let mut tracker = Tracker::new();
        let mut met = TrackedPlayer { last_name: "Stranger99".to_string(), notes: "rushes B".to_string(), ..TrackedPlayer::default() };
        for arena in 1..=4 {
            met.arena_ids.insert(ArenaId::from(arena));
            met.timestamps.insert(now - arena.hours());
        }
        tracker.tracked.insert(AccountId(5), met);

        let mut state = watching(now);
        state.met_before.insert("stranger99".to_string(), AccountId(5));

        let view = state_view(&state, &tracker, now);
        assert!(view.status.is_none());
        assert_eq!(view.friendly.rows.len(), 2);
        assert_eq!(view.enemy.rows.len(), 1);
        assert_eq!(view.enemy.players, "1 players");

        let foe = &view.enemy.rows[0];
        assert_eq!(foe.met, "4");
        assert_eq!(foe.met_severity, super::super::view::Severity::Warned);
        assert_eq!(foe.note.as_deref(), Some("rushes B"));
        assert!(foe.met_hover.starts_with("Seen 4 times total"), "got {:?}", foe.met_hover);
        assert_eq!(foe.ship, "-", "no game data resolves the ship");
        assert!(view.note.as_deref().is_some_and(|note| note.starts_with("Stats unavailable")));
        assert!(foe.scopes.iter().all(|scope| scope.win_rate.is_empty()), "a failed lookup has no figures to show");
    }

    #[test]
    fn a_chat_login_that_names_a_player_puts_a_chip_on_their_row() {
        let now = Timestamp::now();
        let tracker = Tracker::new();
        let mut state = watching(now);
        state.chat = vec![("harvey635".to_string(), now + 1.minute())];

        let view = state_view(&state, &tracker, now);
        let me = view.friendly.rows.iter().find(|row| row.name == "Harvey635").expect("the reader is listed");
        assert_eq!(me.twitch_logins, vec!["harvey635".to_string()]);
        assert!(me.twitch_hover.contains("harvey635"));
        let other = view.enemy.rows.first().expect("the enemy is listed");
        assert!(other.twitch_logins.is_empty());
    }

    #[test]
    fn the_detailed_roster_heads_both_scopes() {
        let tracker = Tracker::new();
        let view = state_view(&LiveState::default(), &tracker, Timestamp::now());
        assert_eq!(view.scope_headings.len(), 2);
        assert_eq!(view.scope_headings[0].win_rate, "Overall WR");
        assert_eq!(view.scope_headings[1].battles, "Ship Battles");
    }

    /// The service's answer fills each scope's figures, coloured by band,
    /// and the team heading averages them; a hidden profile is marked.
    #[test]
    fn the_answered_figures_fill_the_rows_by_scope() {
        wows_toolkit_viewmodel::set_locale("en");
        let now = Timestamp::now();
        let tracker = Tracker::new();
        let mut state = watching(now);
        state.ships = resolved();
        state.identities = Some(identities());
        state.stats = StatsState::Ready(index_by_account(vec![
            figures(1, PlayerStatsStatus::Ok),
            // The service sends no figures for a profile its owner hid.
            PlayerStatsOut {
                battles: None,
                overall_win_rate: None,
                overall_avg_damage: None,
                ship_win_rate: None,
                ship_battles: None,
                pr: None,
                ..figures(2, PlayerStatsStatus::Hidden)
            },
        ]));

        let view = state_view(&state, &tracker, now);
        assert!(view.note.is_none(), "an answer leaves nothing to say");

        let me = view.friendly.rows.iter().find(|row| row.name == "Harvey635").expect("the reader is listed");
        assert_eq!(me.scopes.len(), 2, "the detailed roster shows both scopes");
        let overall = &me.scopes[0];
        assert_eq!(overall.win_rate, "54.2%");
        assert_eq!(overall.personal_rating, "1650");
        assert_eq!(overall.avg_damage, "61,500");
        assert_eq!(overall.battles, "1,234");
        assert!(overall.win_rate_band.is_some() && overall.personal_rating_band.is_some());
        assert_eq!(overall.win_rate_hover.as_deref().map(|hover| hover.lines().count()), Some(2));
        let ship = &me.scopes[1];
        assert_eq!(ship.win_rate, "48.0%");
        assert_eq!(ship.personal_rating, "-", "the service gave no ship rating");
        assert!(me.band.is_some(), "the row is tinted by its lead scope");
        assert!(me.account_id == Some(AccountIdText::from(AccountId(1))) && me.wows_numbers_url.is_some());

        let friend = view.friendly.rows.iter().find(|row| row.name == "Friendly42").expect("the ally is listed");
        assert_eq!(friend.scopes[0].win_rate, "-", "a player the scan never named has no figures");

        let foe = &view.enemy.rows[0];
        assert_eq!(foe.scopes[0].mark, Some(StatsMark::Hidden));
        assert_eq!(foe.scopes[0].battles, "-");

        let average = view.friendly.average_win_rate.expect("the reader's rate averages the team");
        assert_eq!(average.text, text_with("ui.player_tracker.team_average_win_rate", &[("rate", "54.2%")]));
        assert!(view.enemy.average_win_rate.is_none(), "a team nobody has a rate for has no average");
    }

    #[test]
    fn a_lookup_in_flight_leaves_the_cells_empty() {
        let now = Timestamp::now();
        let mut state = watching(now);
        state.ships = resolved();
        state.identities = Some(identities());
        state.stats = StatsState::Fetching;

        let view = state_view(&state, &Tracker::new(), now);
        assert_eq!(view.note, Some(text("ui.player_tracker.stats_fetching")));
        let me = view.friendly.rows.iter().find(|row| row.name == "Harvey635").expect("the reader is listed");
        assert_eq!(me.scopes[0].win_rate, "", "nothing yet rather than a dash");
    }

    /// A lookup waits while the section is hidden, goes once it is shown,
    /// and a battle the session already has an answer for costs nothing.
    #[test]
    fn the_lookup_waits_for_the_section_and_spends_the_budget_once() {
        let now = Timestamp::now();
        let mut state = watching(now);

        assert!(offer_lookup(&mut state, lookup(now, 9)).is_none());
        assert!(matches!(state.stats, StatsState::Held));

        let sent = state.lookups.gate.set_shown(true).and_then(|held| send_lookup(&mut state, held));
        assert!(sent.is_some(), "showing the section sends the held lookup");
        assert!(matches!(state.stats, StatsState::Fetching));

        state.lookups.budget.remember(MatchStatsResponse { arena_id: ArenaId::from(9), players: vec![figures(1, PlayerStatsStatus::Ok)] });
        assert!(offer_lookup(&mut state, lookup(now, 9)).is_none());
        assert!(matches!(&state.stats, StatsState::Ready(players) if players.contains_key(&AccountId(1))));

        let stale = HeldLookup { started_at: now - 1.hour(), ..lookup(now, 10) };
        assert!(offer_lookup(&mut state, stale).is_none(), "a lookup for a finished battle is dropped");
    }

    #[test]
    fn a_spent_budget_is_reported_in_minutes() {
        let now = Timestamp::now();
        let mut state = watching(now);
        state.lookups.gate.set_shown(true);
        for _ in 0..wows_toolkit_viewmodel::match_stats::MAX_REQUESTS_PER_WINDOW {
            state.lookups.budget.record(Instant::now());
        }

        assert!(offer_lookup(&mut state, lookup(now, 11)).is_none());
        let view = state_view(&state, &Tracker::new(), now);
        assert_eq!(
            view.note,
            Some(text_with("ui.player_tracker.stats_unavailable", &[("reason", "rate limited, try again in 20 minutes")]))
        );
    }

    #[test]
    fn a_class_icon_is_read_from_the_build_as_svg() {
        use std::io::Write as _;

        let vfs: wowsunpack::vfs::VfsPath = wowsunpack::vfs::MemoryFS::new().into();
        let dir = vfs.join("gui/fla/minimap/ship_icons").expect("a path");
        dir.create_dir_all().expect("the icon directory");
        dir.join("minimap_destroyer.svg")
            .expect("a path")
            .create_file()
            .expect("the icon file")
            .write_all(b"<svg/>")
            .expect("the icon is written");

        let icon = class_icon(&vfs, Species::Destroyer).expect("the destroyer icon is there");
        assert_eq!(&*icon, "data:image/svg+xml;base64,PHN2Zy8+");
        assert!(class_icon(&vfs, Species::Cruiser).is_none(), "a class the build lacks has no icon");
    }
}
