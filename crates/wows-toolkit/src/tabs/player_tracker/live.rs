//! The matches observed during this app session, their roster scans, statistics and Twitch data.
//!
//! The shell watches the replays directory whether or not Player Tracker is open. Each match
//! owns its asynchronous work and retained results. Only the selected match may start a stats
//! lookup while Current Match is shown.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use futures::StreamExt;
use jiff::Timestamp;
use wows_replays::ReplayFile;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_toolkit_config::index::query;
use wows_toolkit_replay_data::live_match::LiveBattle;
use wows_toolkit_replay_data::live_match::LivePoll;
use wows_toolkit_replay_data::live_match::RosterSource;
use wows_toolkit_replay_data::live_match::scan_arena_retrying;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::replay_folder::ReplayFolder;
use wows_toolkit_services::match_stats::StatsBudget;
use wows_toolkit_services::match_stats::StatsError;
#[cfg(not(test))]
use wows_toolkit_services::match_stats::fetch_stats;
use wows_toolkit_viewmodel::match_stats;
use wows_toolkit_viewmodel::match_stats::MatchStatsResponse;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live::ResolvedRoster;
use wows_toolkit_viewmodel::player_tracker::live::StatsLookupGate;
use wows_toolkit_viewmodel::player_tracker::live::StatsState;
use wows_toolkit_viewmodel::player_tracker::live::TrackedIndex;
use wows_toolkit_viewmodel::player_tracker::live::resolve_roster;
use wows_toolkit_viewmodel::twitch;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::Owner;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::Task;
use zgui::reactive::blocking;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::spawn_local;
use zgui_image::ImageBytes;

use crate::game_art;
use crate::game_art::ClassIconKey;
#[cfg(not(test))]
use crate::http::USER_AGENT;
use crate::long_job::long_job;
use crate::services::Services;
use crate::services::TokioError;

/// How often the chat observations are re-read while a battle is under way. The chat poll
/// writes them every two minutes, so reading faster only repeats the same rows.
const CHAT_POLL_INTERVAL: Duration = Duration::from_secs(60);

#[cfg(test)]
thread_local! {
    /// The arenas a stats lookup was sent for on this thread.
    static STATS_LOOKUPS: std::cell::RefCell<Vec<ArenaId>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A battle's stats lookup, held until the Current Match section is shown.
pub struct HeldLookup {
    started_at: Timestamp,
    state: ArenaState,
}

/// Sets its flag when dropped, which ends a roster scan whose task was cancelled.
#[derive(Default)]
struct ScanCancel(Arc<AtomicBool>);

impl Drop for ScanCancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Which install the live watch follows, and what it resolves the roster against.
#[derive(Clone)]
pub struct LiveSourceConfig {
    pub install_dir: PathBuf,
    pub game_data: GameDataCache,
}

/// A match's work, retained independently of the displayed match.
#[derive(Default)]
struct BattleTasks {
    build_load: Option<Task>,
    scan: Option<Task>,
    chat: Option<Task>,
}

impl BattleTasks {
    fn cancel(&mut self) {
        for task in [self.build_load.take(), self.scan.take(), self.chat.take()].into_iter().flatten() {
            task.cancel();
        }
    }
}

#[derive(Clone, Copy)]
struct BuildIcons {
    images: RwSignal<HashMap<ClassIconKey, ImageBytes>, LocalStorage>,
    asked: StoredValue<HashSet<ClassIconKey>>,
}

impl BuildIcons {
    fn new() -> Self {
        Self { images: RwSignal::new_local(HashMap::new()), asked: StoredValue::new(HashSet::new()) }
    }
}

type StatsWaiters = StoredValue<HashMap<ArenaId, Vec<RwSignal<StatsState, LocalStorage>>>, LocalStorage>;

/// The live watch and the matches observed during this app session.
#[derive(Clone, Copy)]
pub struct LiveModel {
    pub source: RwSignal<Option<LiveSourceConfig>, LocalStorage>,
    pub checked: RwSignal<bool, LocalStorage>,
    /// Whether the poll currently reports a battle, independently of retained matches.
    pub in_battle: RwSignal<bool, LocalStorage>,
    pub matches: RwSignal<Vec<MatchModel>, LocalStorage>,
    /// A real match's start time, or `None` while a debug preview is selected.
    pub selection: RwSignal<Option<Timestamp>, LocalStorage>,
    preview: RwSignal<Option<MatchModel>, LocalStorage>,
    owner: StoredValue<Owner, LocalStorage>,
    budget: StoredValue<StatsBudget>,
    pending_stats: StatsWaiters,
    build_icons: StoredValue<HashMap<u32, BuildIcons>, LocalStorage>,
    shown: StoredValue<bool>,
    watch: StoredValue<Option<Task>, LocalStorage>,
    preview_read: StoredValue<Option<Task>, LocalStorage>,
    #[cfg(test)]
    seeded: StoredValue<bool>,
}

impl LiveModel {
    pub fn new() -> Self {
        Self {
            source: RwSignal::new_local(None),
            checked: RwSignal::new_local(false),
            in_battle: RwSignal::new_local(false),
            matches: RwSignal::new_local(Vec::new()),
            selection: RwSignal::new_local(None),
            preview: RwSignal::new_local(None),
            owner: StoredValue::new_local(Owner::current().expect("the live model is made inside a component")),
            budget: StoredValue::new(StatsBudget::default()),
            pending_stats: StoredValue::new_local(HashMap::new()),
            build_icons: StoredValue::new_local(HashMap::new()),
            shown: StoredValue::new(false),
            watch: StoredValue::new_local(None),
            preview_read: StoredValue::new_local(None),
            #[cfg(test)]
            seeded: StoredValue::new(false),
        }
    }

    pub fn selected_match(self) -> Option<MatchModel> {
        self.preview.get().or_else(|| {
            let selected = self.selection.get()?;
            self.matches.with(|matches| matches.iter().copied().find(|model| model.started_at() == Some(selected)))
        })
    }

    /// The latest real match observed, even after its arena info disappears.
    pub fn current_match(self) -> Option<MatchModel> {
        self.matches.with_untracked(|matches| matches.last().copied())
    }

    pub fn select_match(self, services: Services, started_at: Timestamp) {
        let selected = self
            .matches
            .with_untracked(|matches| matches.iter().copied().find(|model| model.started_at() == Some(started_at)));
        let Some(selected) = selected else { return };
        self.hide_selected();
        self.clear_preview();
        self.selection.set(Some(started_at));
        selected.set_shown(services, self.shown.get_value());
    }

    pub fn set_shown(self, services: Services, shown: bool) {
        self.shown.set_value(shown);
        if let Some(selected) = self.selected_untracked() {
            selected.set_shown(services, shown);
        }
    }

    fn selected_untracked(self) -> Option<MatchModel> {
        self.preview.get_untracked().or_else(|| {
            let selected = self.selection.get_untracked()?;
            self.matches
                .with_untracked(|matches| matches.iter().copied().find(|model| model.started_at() == Some(selected)))
        })
    }

    fn hide_selected(self) {
        if let Some(selected) = self.selected_untracked() {
            selected.gate.update_value(|gate| {
                gate.set_shown(false);
            });
        }
    }

    fn clear_preview(self) {
        if let Some(task) = self.preview_read.get_value() {
            task.cancel();
        }
        self.preview_read.set_value(None);
        if let Some(preview) = self.preview.get_untracked() {
            let owner = preview.owner.get_value();
            self.preview.set(None);
            preview.tasks.update_value(BattleTasks::cancel);
            owner.cleanup();
        }
    }

    fn stop_live_scans(self) {
        if let Some(model) = self.current_match() {
            model.stop_live_scan();
        }
    }

    /// Returns only newly observed matches; repeated polls must not reset cached state.
    fn cache_match(self, live: LiveMatch, in_progress: bool) -> Option<MatchModel> {
        let started_at = live.started_at;
        if self.matches.with_untracked(|matches| matches.iter().any(|model| model.started_at() == Some(started_at))) {
            return None;
        }
        self.stop_live_scans();
        self.hide_selected();
        self.clear_preview();
        let model = self.make_match(live, in_progress);
        self.matches.update(|matches| matches.push(model));
        self.selection.set(Some(started_at));
        Some(model)
    }

    fn make_match(self, live: LiveMatch, in_progress: bool) -> MatchModel {
        let icons = live.build.and_then(|build| self.build_icons.with_value(|icons| icons.get(&build).copied()));
        let icons = icons.unwrap_or_else(|| {
            let icons = self.owner.with_value(|owner| owner.with(BuildIcons::new));
            if let Some(build) = live.build {
                self.build_icons.update_value(|builds| {
                    builds.insert(build, icons);
                });
            }
            icons
        });
        let owner = self.owner.with_value(Owner::child);
        owner.with(|| MatchModel::new(self, live, in_progress, icons))
    }

    fn spawn(self, work: impl Future<Output = ()> + 'static) -> Task {
        self.owner.with_value(|owner| owner.with(|| spawn_local(work)))
    }
}

/// One match and everything learned about it.
#[derive(Clone, Copy)]
pub struct MatchModel {
    pub live_match: RwSignal<Option<LiveMatch>, LocalStorage>,
    pub metadata: RwSignal<Option<Arc<LoadedGameData>>, LocalStorage>,
    pub class_icons: RwSignal<HashMap<ClassIconKey, ImageBytes>, LocalStorage>,
    class_icons_asked: StoredValue<HashSet<ClassIconKey>>,
    session_owner: StoredValue<Owner, LocalStorage>,
    owner: StoredValue<Owner, LocalStorage>,
    pub identities: RwSignal<Option<LiveIdentities>, LocalStorage>,
    pub stats: RwSignal<StatsState, LocalStorage>,
    pub met_before: RwSignal<HashMap<String, AccountId>, LocalStorage>,
    chat: StoredValue<Vec<(String, Timestamp)>>,
    pub twitch: RwSignal<HashMap<String, Vec<SniperCandidate>>, LocalStorage>,
    budget: StoredValue<StatsBudget>,
    pending_stats: StatsWaiters,
    gate: StoredValue<StatsLookupGate<HeldLookup>, LocalStorage>,
    tasks: StoredValue<BattleTasks, LocalStorage>,
    live_scan: StoredValue<Option<Arc<AtomicBool>>>,
}

impl MatchModel {
    fn new(session: LiveModel, live: LiveMatch, in_progress: bool, icons: BuildIcons) -> Self {
        let mut gate = StatsLookupGate::default();
        gate.set_shown(session.shown.get_value());
        Self {
            live_match: RwSignal::new_local(Some(live)),
            metadata: RwSignal::new_local(None),
            class_icons: icons.images,
            class_icons_asked: icons.asked,
            session_owner: session.owner,
            owner: StoredValue::new_local(Owner::current().expect("a match is made inside its owner")),
            identities: RwSignal::new_local(None),
            stats: RwSignal::new_local(StatsState::Idle),
            met_before: RwSignal::new_local(HashMap::new()),
            chat: StoredValue::new(Vec::new()),
            twitch: RwSignal::new_local(HashMap::new()),
            budget: session.budget,
            pending_stats: session.pending_stats,
            gate: StoredValue::new_local(gate),
            tasks: StoredValue::new_local(BattleTasks::default()),
            live_scan: StoredValue::new(in_progress.then(|| Arc::new(AtomicBool::new(false)))),
        }
    }

    fn stop_live_scan(self) {
        let stopped = self.live_scan.with_value(|cancelled| {
            let Some(cancelled) = cancelled else { return false };
            cancelled.store(true, Ordering::Relaxed);
            true
        });
        if !stopped {
            return;
        }
        self.tasks.update_value(|tasks| {
            if let Some(task) = tasks.scan.take() {
                task.cancel();
            }
        });
        if self.stats.with_untracked(|stats| matches!(stats, StatsState::Scanning)) {
            self.stats.set(StatsState::Failed(StatsError::NoRoster.to_string()));
        }
    }

    fn started_at(self) -> Option<Timestamp> {
        self.live_match.with_untracked(|live| live.as_ref().map(|live| live.started_at))
    }

    /// The live roster joined to game data and the battle's identities. At most 24 rows, so it is
    /// built per read rather than cached.
    pub fn roster(self) -> Option<ResolvedRoster> {
        self.live_match.with(|live| {
            let live = live.as_ref()?;
            let index = self.met_before.with(|met| TrackedIndex { by_name: met.clone(), players: met.len() });
            let metadata = self.metadata.with(Clone::clone);
            Some(self.identities.with(|identities| {
                resolve_roster(
                    live,
                    &index,
                    identities.as_ref(),
                    metadata.as_deref().map(|data| data.provider().as_ref()),
                )
            }))
        })
    }

    /// Draws the class icons in `keys` that have not been asked of the loaded build yet.
    pub fn want_class_icons(self, keys: impl IntoIterator<Item = ClassIconKey>) {
        let Some(data) = self.metadata.get_untracked() else { return };
        let fresh: HashSet<ClassIconKey> =
            self.class_icons_asked.with_value(|asked| keys.into_iter().filter(|key| !asked.contains(key)).collect());
        if fresh.is_empty() {
            return;
        }
        self.class_icons_asked.update_value(|asked| asked.extend(fresh.iter().copied()));

        let icons = self.class_icons;
        let work = async move {
            let drawn: Vec<(ClassIconKey, Vec<u8>)> = blocking(move || {
                fresh
                    .into_iter()
                    .filter_map(|key| match game_art::class_icon_png(data.vfs(), key) {
                        Ok(png) => Some((key, png)),
                        Err(err) => {
                            tracing::warn!("live match: {err}");
                            None
                        }
                    })
                    .collect()
            })
            .await;
            icons.update(|icons| icons.extend(drawn.into_iter().map(|(key, png)| (key, ImageBytes::new(png)))));
        };
        self.session_owner.with_value(|owner| owner.with(|| spawn_local(work)));
    }

    /// Spawns `work` into the shell's task set. Battle work is started from inside the watch task,
    /// where no owner is current, so a plain spawn would outlive the shell and read freed signals.
    fn spawn(self, work: impl Future<Output = ()> + 'static) -> Task {
        self.owner.with_value(|owner| owner.with(|| spawn_local(work)))
    }
}

impl LiveModel {
    /// Watches the replays folder of the install the settings name, for as long as the calling
    /// scope lasts, restarting the watch whenever the directory or its game data changes.
    pub fn follow_install(self, services: Services) {
        // Settings unrelated to the install do not restart its watch.
        let wows_dir = Memo::new(move |_| {
            services.settings.read(|settings| settings.wows_dir.clone()).filter(|dir| !dir.is_empty())
        });
        let following = RenderEffect::new(move |_| {
            let config = wows_dir
                .get()
                .zip(services.game_data.cache())
                .map(|(dir, game_data)| LiveSourceConfig { install_dir: PathBuf::from(dir), game_data });
            self.watch(services, config);
        });
        on_cleanup_local(move || drop(following));
    }

    /// Starts watching `config`'s replays folder, replacing any watch already running.
    pub fn watch(self, services: Services, config: Option<LiveSourceConfig>) {
        #[cfg(test)]
        if self.seeded.get_value() {
            return;
        }
        if let Some(previous) = self.watch.get_value() {
            previous.cancel();
        }
        self.stop_live_scans();
        self.checked.set(false);
        self.in_battle.set(false);
        self.source.set(config.clone());
        let Some(config) = config else {
            self.watch.set_value(None);
            return;
        };
        let Some(watchdog) = services.watchdog() else {
            tracing::error!("live match: the watchdog is not running, so no battle is looked for");
            self.watch.set_value(None);
            return;
        };

        let task = self.spawn(async move {
            // Resolving the folder reads the install's files.
            let install_dir = config.install_dir.clone();
            let folder = blocking(move || ReplayFolder::from_app_install(&install_dir)).await;
            // Held by the task, so cancelling it ends the poll.
            let (_poll, mut changes) = match LivePoll::start_folder(&watchdog, folder) {
                Ok(started) => started,
                Err(err) => {
                    tracing::error!(%err, "live match: the poll did not start");
                    return;
                }
            };
            while let Some(live) = changes.next().await {
                self.adopt(services, &config, live);
            }
        });
        self.watch.set_value(Some(task));
    }

    /// Opens a debug preview without adding it to the observed session matches.
    pub fn adopt_finished_replay(self, services: Services, replay: PathBuf) {
        let Some(config) = self.source.get_untracked() else {
            tracing::warn!("live match: no replays directory is known, so no replay is taken as the battle");
            return;
        };
        if let Some(previous) = self.preview_read.get_value() {
            previous.cancel();
        }
        let task = self.spawn(async move {
            let read = replay.clone();
            let meta = blocking(move || ReplayFile::meta_from_file(&read)).await;
            let live = match meta {
                Ok(meta) => LiveMatch::from_meta(&meta),
                Err(err) => {
                    tracing::warn!(path = %replay.display(), "live match: the replay's header did not read: {err}");
                    return;
                }
            };
            let Some(live) = live else {
                tracing::warn!(path = %replay.display(), "live match: the replay's header carries no date");
                return;
            };
            self.preview_read.set_value(None);
            self.hide_selected();
            self.clear_preview();
            let preview = self.make_match(live, false);
            self.selection.set(None);
            self.preview.set(Some(preview));
            self.checked.set(true);
            preview.start(services, &config, RosterSource::Finished(replay));
        });
        self.preview_read.set_value(Some(task));
    }

    /// Records a poll result without clearing retained matches when the game returns to port.
    fn adopt(self, services: Services, config: &LiveSourceConfig, live: Option<LiveBattle>) {
        self.checked.set(true);
        let was_in_battle = self.in_battle.get_untracked();
        self.in_battle.set(live.is_some());
        let Some(LiveBattle { battle: live, roster }) = live else {
            self.stop_live_scans();
            return;
        };
        if let Some(current) = self.current_match().filter(|current| current.started_at() == Some(live.started_at)) {
            if !was_in_battle {
                current.resume_scan(services, config, roster);
            }
            return;
        }
        let Some(model) = self.cache_match(live, true) else { return };
        model.start(services, config, roster);
    }
}

impl MatchModel {
    fn start(self, services: Services, config: &LiveSourceConfig, roster: RosterSource) {
        let started_at = self.started_at().expect("a cached match has its header");
        let names = self.live_match.with_untracked(|live| {
            live.as_ref()
                .expect("a cached match has its header")
                .players
                .iter()
                .map(|player| player.name.clone())
                .collect()
        });
        self.look_up_met_before(services, started_at, names);
        self.watch_chat(services, started_at);
        self.load_and_scan(services, config, roster, started_at);
    }

    fn resume_scan(self, services: Services, config: &LiveSourceConfig, roster: RosterSource) {
        if self.identities.with_untracked(Option::is_some) {
            return;
        }
        let started_at = self.started_at().expect("a cached match has its header");
        self.tasks.update_value(|tasks| {
            if let Some(previous) = tasks.build_load.take() {
                previous.cancel();
            }
        });
        self.live_scan.set_value(Some(Arc::new(AtomicBool::new(false))));
        self.load_and_scan(services, config, roster, started_at);
    }

    fn load_and_scan(self, services: Services, config: &LiveSourceConfig, roster: RosterSource, started_at: Timestamp) {
        if self.metadata.with_untracked(Option::is_some) {
            self.start_scan(services, config, roster, started_at);
            return;
        }

        let build = self.live_match.with_untracked(|live| live.as_ref().and_then(|live| live.build));

        let Some(build) = build else { return };
        self.stats.set(StatsState::Scanning);
        let game_data = config.game_data.clone();
        let config = config.clone();
        let task = self.spawn(async move {
            let loaded = long_job(move || game_data.get_or_load_build(build)).await;
            if self.started_at() != Some(started_at) {
                return;
            }
            match loaded {
                Ok(data) => {
                    self.metadata.set(Some(data));
                    self.start_scan(services, &config, roster, started_at);
                }
                // The roster still lists names and relations; the ship columns and the statistics
                // have nothing to resolve against, which is a failure rather than a wait.
                Err(err) => {
                    tracing::warn!("live match: build {build} did not load: {err}");
                    self.stats.set(StatsState::Failed(format!("this battle's game data did not load: {err}")));
                }
            }
        });
        self.tasks.update_value(|tasks| tasks.build_load = Some(task));
    }

    /// Looks up which of the battle's players the index has met, all-time, by name.
    fn look_up_met_before(self, services: Services, started_at: Timestamp, names: Vec<String>) {
        let Some(pool) = services.settings.pool() else { return };
        self.spawn(async move {
            let found = services.on_tokio(async move { query::accounts_named(&pool, &names).await }).await;
            if self.started_at() != Some(started_at) {
                return;
            }
            match found {
                Ok(Ok(accounts)) => {
                    self.met_before.set(accounts);
                    self.recompute_twitch();
                }
                // The roster still lists everyone; only "met before" stays empty.
                Ok(Err(err)) => tracing::warn!("player tracker: the met-before lookup failed: {err}"),
                Err(err) => tracing::warn!("player tracker: the met-before lookup did not complete: {err}"),
            }
        });
    }

    /// Reads the Twitch chat observations around the battle, and keeps reading while its window
    /// is open: most of the window is still in the future when the battle starts.
    fn watch_chat(self, services: Services, started_at: Timestamp) {
        let Some(pool) = services.settings.pool() else { return };
        let start = started_at.as_second() + (twitch::WINDOW_BEFORE_MINUTES * 60.0) as i64;
        let end = started_at.as_second() + (twitch::WINDOW_AFTER_MINUTES * 60.0) as i64;
        let task = self.spawn(async move {
            let mut first = true;
            loop {
                if !std::mem::replace(&mut first, false) {
                    // Built inside the runtime: a tokio timer made outside one panics.
                    let waited = services.on_tokio(async { tokio::time::sleep(CHAT_POLL_INTERVAL).await }).await;
                    if waited.is_err() {
                        return;
                    }
                }
                let read = pool.clone();
                let found =
                    services.on_tokio(async move { query::observations_in_window(&read, start, end).await }).await;
                if self.started_at() != Some(started_at) {
                    return;
                }
                match found {
                    Ok(Ok(rows)) => {
                        let observations = rows
                            .into_iter()
                            .filter_map(|(login, seen_at)| Timestamp::from_second(seen_at).ok().map(|at| (login, at)))
                            .collect();
                        self.chat.set_value(observations);
                        self.recompute_twitch();
                    }
                    // The roster still lists everyone; only the chips are missing.
                    Ok(Err(err)) => tracing::warn!("player tracker: the chat lookup failed: {err}"),
                    Err(err) => tracing::warn!("player tracker: the chat lookup did not complete: {err}"),
                }
                // Past the window there is nothing further to find.
                if Timestamp::now().as_second() > end {
                    return;
                }
            }
        });
        self.tasks.update_value(|tasks| tasks.chat = Some(task));
    }

    /// Rebuilds the chip candidates for everyone on the roster. Matching is a Levenshtein pass over
    /// every observation for every row, too much to redo per frame.
    fn recompute_twitch(self) {
        let Some(roster) = self.roster_untracked() else {
            self.twitch.set(HashMap::new());
            return;
        };
        let candidates = self.chat.with_value(|chat| {
            roster
                .friendly
                .iter()
                .chain(roster.enemy.iter())
                .filter_map(|row| {
                    let observations = chat.iter().map(|(login, seen_at)| (login.as_str(), *seen_at));
                    let found = twitch::sniper_candidates(observations, &row.name, roster.started_at);
                    (!found.is_empty()).then(|| (row.name.clone(), found))
                })
                .collect()
        });
        self.twitch.set(candidates);
    }

    fn roster_untracked(self) -> Option<ResolvedRoster> {
        self.live_match.with_untracked(|live| {
            let live = live.as_ref()?;
            let index = self.met_before.with_untracked(|met| TrackedIndex { by_name: met.clone(), players: met.len() });
            let metadata = self.metadata.get_untracked();
            Some(self.identities.with_untracked(|identities| {
                resolve_roster(
                    live,
                    &index,
                    identities.as_ref(),
                    metadata.as_deref().map(|data| data.provider().as_ref()),
                )
            }))
        })
    }

    /// Reads the roster off `source`, retrying while the game flushes a battle in progress, then
    /// offers the stats lookup.
    fn start_scan(self, services: Services, config: &LiveSourceConfig, source: RosterSource, started_at: Timestamp) {
        let Some(metadata) = self.metadata.get_untracked() else { return };
        if self
            .live_scan
            .with_value(|cancelled| cancelled.as_ref().is_some_and(|cancelled| cancelled.load(Ordering::Relaxed)))
        {
            self.stats.set(StatsState::Failed(StatsError::NoRoster.to_string()));
            return;
        }
        self.stats.set(StatsState::Scanning);
        let game_data = config.game_data.clone();
        // A finished replay needs only a task-local cancellation flag.
        let cancel = self.live_scan.with_value(|cancelled| {
            cancelled.as_ref().map(|cancelled| ScanCancel(Arc::clone(cancelled))).unwrap_or_default()
        });
        let task = self.spawn(async move {
            // Dropped with this task, which stops the retries between attempts.
            let cancelled = Arc::clone(&cancel.0);
            let state =
                long_job(move || scan_arena_retrying(&source, &metadata, &game_data, &cancelled, started_at)).await;
            drop(cancel);
            if self.started_at() != Some(started_at) {
                return;
            }
            let Some(state) = state else {
                self.stats.set(StatsState::Failed(StatsError::NoRoster.to_string()));
                return;
            };
            // Written before the lookup, so the clan tags and the account join show even when the
            // lookup fails, is refused, or waits for the section to be shown.
            self.identities.set(Some(LiveIdentities::from_player_states(&state.players)));
            self.recompute_twitch();
            self.offer_lookup(services, HeldLookup { started_at, state });
        });
        self.tasks.update_value(|tasks| tasks.scan = Some(task));
    }

    /// Answers `lookup` from this session's earlier answers, sends it while the Current Match
    /// section is shown, or holds it until it is.
    fn offer_lookup(self, services: Services, lookup: HeldLookup) {
        let answered = self.budget.with_value(|budget| budget.answered_state(lookup.state.arena_id));
        if let Some(answered) = answered {
            self.stats.set(answered);
            return;
        }
        let mut send = None;
        self.gate.update_value(|gate| send = gate.offer(lookup));
        match send {
            Some(lookup) => self.send_lookup(services, lookup),
            None => self.stats.set(StatsState::Held),
        }
    }

    /// Records whether the Current Match section is on screen, sending a held lookup once it is.
    fn set_shown(self, services: Services, shown: bool) {
        let mut send = None;
        self.gate.update_value(|gate| send = gate.set_shown(shown));
        if let Some(lookup) = send {
            self.send_lookup(services, lookup);
        }
    }

    fn send_lookup(self, services: Services, lookup: HeldLookup) {
        let HeldLookup { started_at, state } = lookup;
        if self.started_at() != Some(started_at) {
            return;
        }
        let arena_id = state.arena_id;
        if let Some(answered) = self.budget.with_value(|budget| budget.answered_state(arena_id)) {
            self.stats.set(answered);
            return;
        }
        let pending = self.pending_stats;
        let joined =
            pending.try_update_value(|requests| requests.get_mut(&arena_id).map(|waiters| waiters.push(self.stats)));
        if joined.flatten().is_some() {
            self.stats.set(StatsState::Fetching);
            return;
        }

        let mut begun = Ok(());
        self.budget.update_value(|budget| begun = budget.begin(arena_id, Instant::now()));
        if let Err(refused) = begun {
            self.stats.set(refused);
            return;
        }
        pending.update_value(|requests| {
            requests.insert(arena_id, vec![self.stats]);
        });
        self.stats.set(StatsState::Fetching);

        // The proxy as the settings hold it; `http::client` decides what an unset one means.
        let proxy_url = services.settings.read_untracked(|settings| settings.proxy_url.clone()).unwrap_or_default();
        let budget = self.budget;
        let work = async move {
            let mut retries_left = match_stats::TIMEOUT_RETRIES;
            let shown = loop {
                let fetched = request_stats(services, state.clone(), proxy_url.clone()).await;
                match fetched {
                    // The service may have counted the request that timed out, so the retry is
                    // cleared against the budget like the first.
                    Ok(Err(err)) if err.timed_out() && retries_left > 0 => {
                        retries_left -= 1;
                        let mut begun = Ok(());
                        budget.update_value(|budget| begun = budget.begin(arena_id, Instant::now()));
                        if let Err(refused) = begun {
                            break refused;
                        }
                    }
                    Ok(fetched) => {
                        let mut shown = StatsState::Idle;
                        budget.update_value(|budget| shown = budget.finish(arena_id, fetched, Instant::now()));
                        break shown;
                    }
                    Err(err) => break StatsState::Failed(err.to_string()),
                }
            };
            let waiters = pending.try_update_value(|requests| requests.remove(&arena_id)).flatten();
            if let Some(waiters) = waiters
                && let Some((last, others)) = waiters.split_last()
            {
                for waiter in others {
                    let _ = waiter.try_set(shown.clone());
                }
                let _ = last.try_set(shown);
            }
        };
        self.session_owner.with_value(|owner| owner.with(|| spawn_local(work)));
    }
}

#[cfg(test)]
impl LiveModel {
    /// Seeds a battle as a poll would have found it.
    pub fn seed(self, live: LiveMatch) {
        // A source with no install behind it: the roster is seeded rather than polled.
        self.source.set(Some(LiveSourceConfig {
            install_dir: std::env::temp_dir(),
            game_data: GameDataCache::without_install(),
        }));
        self.checked.set(true);
        self.in_battle.set(true);
        self.cache_match(live, false);
    }

    /// Seeds a battle with its build loaded, and stops the watch from replacing it.
    pub fn seed_over(self, live: LiveMatch, game_data: GameDataCache) {
        self.seeded.set_value(true);
        if let Some(previous) = self.watch.get_value() {
            previous.cancel();
        }
        self.watch.set_value(None);
        self.stop_live_scans();

        let build = live.build.expect("the battle names its build");
        let data = game_data.get_or_load_build(build).expect("the battle's build loads");
        self.seed(live);
        let model = self.current_match().expect("the seeded battle is cached");
        model.tasks.update_value(BattleTasks::cancel);
        model.metadata.set(Some(data));
        self.source.set(Some(LiveSourceConfig { install_dir: std::env::temp_dir(), game_data }));
    }

    /// Seeds what a scan and a lookup would have produced.
    #[cfg(test)]
    pub fn seed_stats(
        self,
        identities: LiveIdentities,
        players: Vec<wows_toolkit_viewmodel::match_stats::PlayerStatsOut>,
    ) {
        let model = self.current_match().expect("a battle is seeded");
        model.identities.set(Some(identities));
        model.recompute_twitch();
        model.stats.set(StatsState::ready(players));
    }

    #[cfg(test)]
    pub fn seed_chat(self, observations: Vec<(String, Timestamp)>) {
        let model = self.current_match().expect("a battle is seeded");
        model.chat.set_value(observations);
        model.recompute_twitch();
    }

    /// Offers the battle in progress the lookup a roster scan of `state` would have offered. The
    /// battle's own tasks are stopped first, so its scan cannot offer over the seed.
    #[cfg(test)]
    pub fn seed_scan(self, services: Services, state: ArenaState) {
        let model = self.current_match().expect("a battle is seeded");
        let started_at = model.started_at().expect("a cached battle has its header");
        model.tasks.update_value(BattleTasks::cancel);
        model.identities.set(Some(LiveIdentities::from_player_states(&state.players)));
        model.recompute_twitch();
        model.offer_lookup(services, HeldLookup { started_at, state });
    }
}

/// Asks the stats service about `state`'s roster.
#[cfg(not(test))]
async fn request_stats(
    services: Services,
    state: ArenaState,
    proxy_url: String,
) -> Result<Result<MatchStatsResponse, StatsError>, TokioError> {
    services.on_tokio(async move { fetch_stats(USER_AGENT, &state, &proxy_url).await }).await
}

/// Notes the lookup and answers it with no players, so a test stays off the network.
#[cfg(test)]
async fn request_stats(
    _services: Services,
    state: ArenaState,
    _proxy_url: String,
) -> Result<Result<MatchStatsResponse, StatsError>, TokioError> {
    STATS_LOOKUPS.with_borrow_mut(|lookups| lookups.push(state.arena_id));

    Ok(Ok(MatchStatsResponse { arena_id: state.arena_id, players: Vec::new() }))
}

/// Takes the arenas a stats lookup was sent for on this thread since the last take, oldest first.
/// A test takes them first, since the harness can run another test on the same thread before it.
#[cfg(test)]
pub fn take_stats_lookups() -> Vec<ArenaId> {
    STATS_LOOKUPS.take()
}
