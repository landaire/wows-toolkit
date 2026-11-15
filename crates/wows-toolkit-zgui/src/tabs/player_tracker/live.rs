//! The battle in progress: the poll of the replays directory, the roster scan, the stats lookup
//! and the Twitch chat observations around the battle.
//!
//! The shell owns the model and starts the poll with the app, so battles are detected and their
//! rosters read whether or not the Player Tracker tab has been opened. The stats lookup still
//! waits until the Current Match section is shown.
//!
//! Every write a task makes is keyed on the battle's start time, so a task that outlives its
//! battle is dropped rather than landing on the next one.

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
#[cfg(test)]
use wows_replays::types::ArenaId;
use wows_toolkit_config::index::query;
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

/// How often the chat observations are re-read while a battle is under way. The egui app's chat
/// poll writes them every two minutes, so reading faster only repeats the same rows.
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

/// The tasks belonging to the battle in progress, cancelled with it.
#[derive(Default)]
struct BattleTasks {
    build_load: Option<Task>,
    scan: Option<Task>,
    lookup: Option<Task>,
    chat: Option<Task>,
}

impl BattleTasks {
    fn cancel(&mut self) {
        for task in
            [self.build_load.take(), self.scan.take(), self.lookup.take(), self.chat.take()].into_iter().flatten()
        {
            task.cancel();
        }
    }
}

/// The battle in progress and everything learned about it.
#[derive(Clone, Copy)]
pub struct LiveModel {
    /// `None` until the replays directory and its game data are known.
    pub source: RwSignal<Option<LiveSourceConfig>, LocalStorage>,
    /// Whether the poll has reported at all yet, so "no battle" is not said before the first look.
    pub checked: RwSignal<bool, LocalStorage>,
    pub live_match: RwSignal<Option<LiveMatch>, LocalStorage>,
    /// Ship data for the battle's build, once it has loaded.
    pub metadata: RwSignal<Option<Arc<LoadedGameData>>, LocalStorage>,
    /// The roster's class icons drawn from that build.
    pub class_icons: RwSignal<HashMap<ClassIconKey, ImageBytes>, LocalStorage>,
    class_icons_asked: StoredValue<HashSet<ClassIconKey>>,
    metadata_build: StoredValue<Option<u32>>,
    /// The shell's scope, which the model's work is spawned into so it ends with the shell.
    owner: StoredValue<Owner, LocalStorage>,
    /// Account ids, realms and clans the packet scan recovered.
    pub identities: RwSignal<Option<LiveIdentities>, LocalStorage>,
    pub stats: RwSignal<StatsState, LocalStorage>,
    /// Which of the battle's players the index has met before, keyed by lower-cased name.
    pub met_before: RwSignal<HashMap<String, AccountId>, LocalStorage>,
    chat: StoredValue<Vec<(String, Timestamp)>>,
    /// The chip's candidates per roster name; a name that is absent has no chip.
    pub twitch: RwSignal<HashMap<String, Vec<SniperCandidate>>, LocalStorage>,
    budget: StoredValue<StatsBudget>,
    gate: StoredValue<StatsLookupGate<HeldLookup>, LocalStorage>,
    watch: StoredValue<Option<Task>, LocalStorage>,
    tasks: StoredValue<BattleTasks, LocalStorage>,
    /// Set by [`Self::seed_over`], after which the watch stays off so no poll replaces the seed.
    #[cfg(test)]
    seeded: StoredValue<bool>,
}

impl LiveModel {
    /// Called from the shell's component body.
    pub fn new() -> Self {
        Self {
            source: RwSignal::new_local(None),
            checked: RwSignal::new_local(false),
            live_match: RwSignal::new_local(None),
            metadata: RwSignal::new_local(None),
            class_icons: RwSignal::new_local(HashMap::new()),
            class_icons_asked: StoredValue::new(HashSet::new()),
            metadata_build: StoredValue::new(None),
            owner: StoredValue::new_local(Owner::current().expect("the live model is made inside a component")),
            identities: RwSignal::new_local(None),
            stats: RwSignal::new_local(StatsState::Idle),
            met_before: RwSignal::new_local(HashMap::new()),
            chat: StoredValue::new(Vec::new()),
            twitch: RwSignal::new_local(HashMap::new()),
            budget: StoredValue::new(StatsBudget::default()),
            gate: StoredValue::new_local(StatsLookupGate::default()),
            watch: StoredValue::new_local(None),
            tasks: StoredValue::new_local(BattleTasks::default()),
            #[cfg(test)]
            seeded: StoredValue::new(false),
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

        let build = data.build();
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
            if self.metadata.with_untracked(|data| data.as_ref().map(|data| data.build())) != Some(build) {
                return;
            }
            self.class_icons
                .update(|icons| icons.extend(drawn.into_iter().map(|(key, png)| (key, ImageBytes::new(png)))));
        };
        self.spawn(work);
    }

    /// Spawns `work` into the shell's task set. Battle work is started from inside the watch task,
    /// where no owner is current, so a plain spawn would outlive the shell and read freed signals.
    fn spawn(self, work: impl Future<Output = ()> + 'static) -> Task {
        self.owner.with_value(|owner| owner.with(|| spawn_local(work)))
    }

    /// Watches the replays folder of the install the settings name, for as long as the calling
    /// scope lasts, restarting the watch whenever the directory or its game data changes.
    pub fn follow_install(self, services: Services) {
        // The directory alone, so an edit to any other setting does not restart the watch and
        // drop the battle in progress.
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
        self.tasks.update_value(BattleTasks::cancel);
        self.checked.set(false);
        self.live_match.set(None);
        self.clear_battle();
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

        let task = spawn_local(async move {
            // Resolving the folder reads the install's files.
            let install_dir = config.install_dir.clone();
            let folder = blocking(move || ReplayFolder::from_app_install(&install_dir)).await;
            // Held by the task, so cancelling it ends the poll.
            let (poll, mut changes) = match LivePoll::start_folder(&watchdog, folder) {
                Ok(started) => started,
                Err(err) => {
                    tracing::error!(%err, "live match: the poll did not start");
                    return;
                }
            };
            while let Some(live) = changes.next().await {
                let roster = RosterSource::InProgress(poll.live_source());
                self.adopt(services, &config, live, roster);
            }
        });
        self.watch.set_value(Some(task));
    }

    /// Drops everything that belonged to the previous battle.
    fn clear_battle(self) {
        self.tasks.update_value(BattleTasks::cancel);
        self.met_before.set(HashMap::new());
        self.chat.set_value(Vec::new());
        self.twitch.set(HashMap::new());
        self.identities.set(None);
        self.stats.set(StatsState::Idle);
        self.gate.update_value(StatsLookupGate::clear);
    }

    /// Takes the finished replay at `replay` as the battle in progress, so the roster scan and the
    /// stats lookup can be exercised outside a battle. A debug tool: the next battle the poll
    /// reports replaces it.
    pub fn adopt_finished_replay(self, services: Services, replay: PathBuf) {
        let Some(config) = self.source.get_untracked() else {
            tracing::warn!("live match: no replays directory is known, so no replay is taken as the battle");
            return;
        };
        self.spawn(async move {
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
            self.adopt(services, &config, Some(live), RosterSource::Finished(replay));
        });
    }

    /// Adopts what a poll found, or a replay taken as the battle, and loads the roster's build
    /// when the loaded data does not already cover it. `roster` is where the scan reads it from.
    fn adopt(self, services: Services, config: &LiveSourceConfig, live: Option<LiveMatch>, roster: RosterSource) {
        self.checked.set(true);
        let started_at = live.as_ref().map(|live| live.started_at);
        let build = live.as_ref().and_then(|live| live.build);
        let names: Vec<String> =
            live.iter().flat_map(|live| live.players.iter().map(|player| player.name.clone())).collect();
        self.live_match.set(live);
        self.clear_battle();

        let Some(started_at) = started_at else { return };
        self.look_up_met_before(services, started_at, names);
        self.watch_chat(services, started_at);

        let Some(build) = build else { return };
        if self.metadata_build.get_value() == Some(build) && self.metadata.with_untracked(Option::is_some) {
            self.start_scan(services, config, roster, started_at);
            return;
        }

        self.metadata_build.set_value(Some(build));
        self.metadata.set(None);
        self.class_icons_asked.set_value(HashSet::new());
        self.class_icons.set(HashMap::new());
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
                    self.metadata_build.set_value(None);
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
        self.stats.set(StatsState::Scanning);
        let game_data = config.game_data.clone();
        let task = self.spawn(async move {
            // Dropped with this task, which stops the retries between attempts.
            let cancel = ScanCancel::default();
            let cancelled = Arc::clone(&cancel.0);
            let state = long_job(move || scan_arena_retrying(&source, &metadata, &game_data, &cancelled)).await;
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
    pub fn set_shown(self, services: Services, shown: bool) {
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
        let mut begun = Ok(());
        self.budget.update_value(|budget| begun = budget.begin(arena_id, Instant::now()));
        if let Err(refused) = begun {
            self.stats.set(refused);
            return;
        }
        self.stats.set(StatsState::Fetching);

        // The proxy as the settings hold it; `http::client` decides what an unset one means.
        let proxy_url = services.settings.read_untracked(|settings| settings.proxy_url.clone()).unwrap_or_default();
        let task = self.spawn(async move {
            let fetched = request_stats(services, state, proxy_url).await;
            if self.started_at() != Some(started_at) {
                return;
            }
            let shown = match fetched {
                Ok(fetched) => {
                    let mut shown = StatsState::Idle;
                    self.budget.update_value(|budget| shown = budget.finish(arena_id, fetched, Instant::now()));
                    shown
                }
                Err(err) => StatsState::Failed(err.to_string()),
            };
            self.stats.set(shown);
        });
        self.tasks.update_value(|tasks| tasks.lookup = Some(task));
    }

    /// Seeds a battle as a poll would have found it.
    #[cfg(test)]
    pub fn seed(self, live: LiveMatch) {
        // A source with no install behind it: the roster is seeded rather than polled.
        self.source.set(Some(LiveSourceConfig {
            install_dir: std::env::temp_dir(),
            game_data: GameDataCache::without_install(),
        }));
        self.checked.set(true);
        self.live_match.set(Some(live));
    }

    /// Seeds a battle with its build loaded from `game_data`, so its ships are named. The watch
    /// and the battle's tasks are stopped first and the watch stays off, so a late poll or scan
    /// cannot adopt over the seed.
    #[cfg(test)]
    pub fn seed_over(self, live: LiveMatch, game_data: GameDataCache) {
        self.seeded.set_value(true);
        if let Some(previous) = self.watch.get_value() {
            previous.cancel();
        }
        self.watch.set_value(None);
        self.tasks.update_value(BattleTasks::cancel);

        let build = live.build.expect("the battle names its build");
        let data = game_data.get_or_load_build(build).expect("the battle's build loads");
        self.metadata_build.set_value(Some(build));
        self.metadata.set(Some(data));
        self.source.set(Some(LiveSourceConfig { install_dir: std::env::temp_dir(), game_data }));
        self.checked.set(true);
        self.live_match.set(Some(live));
    }

    /// Seeds what a scan and a lookup would have produced.
    #[cfg(test)]
    pub fn seed_stats(
        self,
        identities: LiveIdentities,
        players: Vec<wows_toolkit_viewmodel::match_stats::PlayerStatsOut>,
    ) {
        self.identities.set(Some(identities));
        self.recompute_twitch();
        self.stats.set(StatsState::ready(players));
    }

    #[cfg(test)]
    pub fn seed_chat(self, observations: Vec<(String, Timestamp)>) {
        self.chat.set_value(observations);
        self.recompute_twitch();
    }

    /// Offers the battle in progress the lookup a roster scan of `state` would have offered. The
    /// battle's own tasks are stopped first, so its scan cannot offer over the seed.
    #[cfg(test)]
    pub fn seed_scan(self, services: Services, state: ArenaState) {
        let started_at = self.started_at().expect("a battle is in progress");
        self.tasks.update_value(BattleTasks::cancel);
        self.identities.set(Some(LiveIdentities::from_player_states(&state.players)));
        self.offer_lookup(services, HeldLookup { started_at, state });
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
