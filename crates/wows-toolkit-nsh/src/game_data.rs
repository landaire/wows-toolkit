//! The process-wide game-data service: the loaded game data bound to the WoWs
//! directory and locale in the shared settings database, the startup preload of
//! the installed build, replay parsing, the replay index pass, the game-data
//! cache maintenance, missing-build detection and the replay directory
//! registry.
//!
//! The work itself is `wows_toolkit_replay_data`, shared with the GPUI app.
//! What is here is the state one process keeps of it, and the `GameData`
//! module the TSX interface reads that state through.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_data_mgr::download_repo::RemoteAvailability;
use wows_toolkit_replay_data::cache;
use wows_toolkit_replay_data::cache::CacheJob;
use wows_toolkit_replay_data::cache::CacheOutcome;
use wows_toolkit_replay_data::cache::CacheState;
use wows_toolkit_replay_data::directories::CloseRefusal;
use wows_toolkit_replay_data::directories::DirectoryRegistry;
use wows_toolkit_replay_data::error::PreloadError;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::load::ParsedReplay;
use wows_toolkit_replay_data::replay_index;
use wows_toolkit_replay_data::replay_index::IndexMode;
use wows_toolkit_replay_data::replay_index::IndexProgress;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::settings::keys;
use wowsunpack::data::Version;

/// Identifies this front end to the services it reaches.
pub const USER_AGENT: &str = concat!("wows-toolkit-nsh/", env!("CARGO_PKG_VERSION"));

/// Where the game data stands.
#[derive(Clone)]
pub enum GameDataState {
    /// The settings name no World of Warships directory.
    NotConfigured,
    /// The installed build is loading. `version` is what the install says it
    /// is, `None` when its `preferences.xml` could not be read.
    Loading {
        version: Option<String>,
    },
    Ready(Arc<LoadedGameData>),
    Failed {
        reason: String,
    },
}

/// Why a game-data request could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum GameDataError {
    #[error("the World of Warships directory is not set")]
    NotConfigured,
    #[error(transparent)]
    Load(#[from] ReplayLoadError),
    #[error(transparent)]
    Preload(#[from] PreloadError),
    #[error("the settings database could not be read: {reason}")]
    Settings { reason: String },
}

/// The settings the service is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundSettings {
    wows_dir: Option<PathBuf>,
    locale: String,
    auto_dump: bool,
    cache_dir: String,
    proxy_url: String,
    repo_commit: Option<String>,
}

impl BoundSettings {
    /// Whether a change from `self` to `other` needs a new cache: the install,
    /// the dump directory and the auto-dump flag are all fixed per cache.
    fn rebinds(&self, other: &Self) -> bool {
        self.wows_dir != other.wows_dir || self.auto_dump != other.auto_dump || self.cache_dir != other.cache_dir
    }
}

struct Binding {
    settings: BoundSettings,
    cache: GameDataCache,
    /// Bumped on each rebind, so a preload that finishes after a newer one
    /// started does not overwrite its status.
    generation: u64,
}

#[derive(Default)]
struct IndexState {
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<IndexProgress>,
    outcome: Option<IndexOutcome>,
    /// Directories waiting for a pass while one runs.
    queued: Vec<(PathBuf, IndexMode)>,
}

#[derive(Clone)]
struct IndexOutcome {
    text: String,
    failed: bool,
}

/// One build a listing of replays needs and nothing on this machine has.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MissingBuild {
    build: u32,
    /// `major.minor.patch`, the hint the repository resolves other servers'
    /// builds by. `None` when the replay's version did not parse that far.
    version: Option<String>,
    replays: u32,
}

#[derive(Default)]
struct OfferState {
    planning: bool,
    builds: Vec<MissingBuild>,
    availability: BTreeMap<u32, RemoteAvailability>,
    objects: Option<usize>,
    failure: Option<String>,
    open: bool,
}

struct State {
    revision: u64,
    started: bool,
    binding: Option<Binding>,
    status: GameDataState,
    settings_error: Option<String>,
    personal_rating: Option<Arc<PersonalRatingData>>,
    index: IndexState,
    cache: CacheState,
    cache_message: Option<String>,
    directories: DirectoryRegistry,
    directories_error: Option<String>,
    offered_builds: BTreeSet<u32>,
    offer: OfferState,
}

struct Service {
    state: Mutex<State>,
    changed: tokio::sync::watch::Sender<u64>,
}

static SERVICE: LazyLock<Service> = LazyLock::new(|| Service {
    state: Mutex::new(State {
        revision: 0,
        started: false,
        binding: None,
        status: GameDataState::NotConfigured,
        settings_error: None,
        personal_rating: None,
        index: IndexState::default(),
        cache: CacheState::default(),
        cache_message: None,
        directories: DirectoryRegistry::new(),
        directories_error: None,
        offered_builds: BTreeSet::new(),
        offer: OfferState::default(),
    }),
    changed: tokio::sync::watch::channel(0).0,
});

fn state() -> MutexGuard<'static, State> {
    SERVICE.state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Applies `change` and tells every watcher.
fn update<R>(change: impl FnOnce(&mut State) -> R) -> R {
    let (result, revision) = {
        let mut state = state();
        let result = change(&mut state);
        state.revision += 1;
        (result, state.revision)
    };
    SERVICE.changed.send_replace(revision);

    result
}

/// The loaded game data's cache, bound to the settings. `None` when the
/// settings name no WoWs directory. The first call of the process reads the
/// settings database and starts the preload; it never waits for the load.
pub fn cache() -> Option<GameDataCache> {
    ensure_started();
    let state = state();
    let binding = state.binding.as_ref()?;
    binding.settings.wows_dir.as_ref()?;

    Some(binding.cache.clone())
}

/// The newest build already loaded, without loading anything.
pub fn newest_loaded() -> Option<Arc<LoadedGameData>> {
    cache()?.newest_loaded()
}

/// Where the game data stands.
pub fn status() -> GameDataState {
    ensure_started();
    state().status.clone()
}

/// The configured World of Warships directory.
pub fn wows_dir() -> Option<PathBuf> {
    ensure_started();
    state().binding.as_ref()?.settings.wows_dir.clone()
}

/// The directory the game records replays into.
pub fn replays_dir() -> Option<PathBuf> {
    wows_dir().map(|dir| wows_toolkit_replay_data::load::resolve_replays_dir(&dir))
}

/// The personal-rating expected values, once they have been loaded.
pub fn personal_rating() -> Option<Arc<PersonalRatingData>> {
    state().personal_rating.clone()
}

/// The installed build's game data, waiting for the load when it is still
/// running. Blocking: run it off the UI thread.
pub fn wait_current_build() -> Result<Arc<LoadedGameData>, GameDataError> {
    let cache = cache().ok_or(GameDataError::NotConfigured)?;
    let wows_dir = wows_dir().ok_or(GameDataError::NotConfigured)?;

    Ok(wows_toolkit_replay_data::load::preload_current_build(&wows_dir, &cache)?)
}

/// Parses one replay against the build it was recorded on. Blocking.
pub fn parse_replay(path: &Path) -> Result<ParsedReplay, GameDataError> {
    let cache = cache().ok_or(GameDataError::NotConfigured)?;
    let rating = personal_rating();

    Ok(wows_toolkit_replay_data::load::parse_replay(path, &cache, rating.as_deref())?)
}

/// Reads the settings again and rebinds what they changed: the WoWs
/// directory, the language, the cache directory, the auto-dump flag and the
/// proxy. Blocking.
pub fn reload_settings() -> Result<(), GameDataError> {
    rebind().map_err(|reason| GameDataError::Settings { reason })
}

/// Starts an index pass over the game's replay directory. `false` when the
/// settings name no WoWs directory.
pub fn start_indexing(mode: IndexMode) -> bool {
    let Some(wows_dir) = wows_dir() else { return false };
    queue_index(wows_dir.join("replays"), mode);

    true
}

/// Reads the settings and binds to them, once per process.
fn ensure_started() {
    if state().started {
        return;
    }
    if let Err(reason) = rebind() {
        log::warn!("game data: the settings could not be read: {reason}");
    }
}

/// Reads the settings and rebinds whatever they changed.
fn rebind() -> Result<(), String> {
    let read = read_settings();
    let settings = match read {
        Ok(settings) => settings,
        Err(reason) => {
            update(|state| {
                state.started = true;
                state.settings_error = Some(reason.clone());
            });
            return Err(reason);
        }
    };

    let first_start = !state().started;
    let restart = update(|state| {
        state.started = true;
        state.settings_error = None;
        match &mut state.binding {
            Some(binding) if !binding.settings.rebinds(&settings) => {
                if binding.settings.locale != settings.locale {
                    binding.cache.set_locale(&settings.locale);
                }
                binding.settings = settings.clone();
                None
            }
            current => {
                let generation = current.as_ref().map_or(0, |binding| binding.generation + 1);
                let cache = match &settings.wows_dir {
                    Some(dir) => GameDataCache::new(dir.clone()),
                    None => GameDataCache::without_install(),
                }
                .with_locale(&settings.locale)
                .with_auto_dump(settings.auto_dump)
                .with_cache_dir(&settings.cache_dir);
                *current = Some(Binding { settings: settings.clone(), cache: cache.clone(), generation });
                state.cache.forget_stats();
                state.cache.updates.clear();
                state.cache.repair.clear();
                state.status = match &settings.wows_dir {
                    Some(dir) => GameDataState::Loading {
                        version: wows_toolkit_replay_data::load::installed_version(dir)
                            .map(|version| version.to_path()),
                    },
                    None => GameDataState::NotConfigured,
                };
                Some((cache, generation))
            }
        }
    });

    if let Some((cache, generation)) = restart
        && let Some(wows_dir) = settings.wows_dir.clone()
    {
        spawn("game-data-preload", move || preload(wows_dir, cache, generation));
    }
    if first_start {
        spawn("game-data-startup", startup_work);
    }

    Ok(())
}

/// Loads the installed build and folds the outcome into the status, then runs
/// the work that waits for it: pruning the per-build caches nothing opens and
/// the index pass a stale index asks for.
fn preload(wows_dir: PathBuf, cache: GameDataCache, generation: u64) {
    let outcome = wows_toolkit_replay_data::load::preload_current_build(&wows_dir, &cache);
    let current = update(|state| {
        let current = state.binding.as_ref().is_some_and(|binding| binding.generation == generation);
        if current {
            state.status = match &outcome {
                Ok(loaded) => GameDataState::Ready(Arc::clone(loaded)),
                Err(error) => GameDataState::Failed { reason: error.to_string() },
            };
        }
        current
    });
    if !current || outcome.is_err() {
        return;
    }

    let dump_base = state().binding.as_ref().and_then(|binding| cache::base(&binding.settings.cache_dir));
    wows_toolkit_replay_data::load::prune_stale_caches(&wows_dir, dump_base.as_deref());

    // Rows written by an older parse than this one are read again, as the
    // GPUI app does on the same check.
    let root = wows_dir.join("replays");
    let stale = with_database(|runtime, pool| {
        runtime
            .block_on(wows_toolkit_config::index::query::has_stale_replay_records(pool, &root))
            .map_err(|error| error.to_string())
    });
    match stale {
        Ok(true) => queue_index(root, IndexMode::FillGaps),
        Ok(false) => {}
        Err(reason) => log::warn!("replay index revision could not be checked: {reason}"),
    }
}

/// Once per process: tidy the game-data cache, load the personal-rating table
/// and restore the pinned replay directories.
fn startup_work() {
    let (base, proxy) = {
        let state = state();
        let Some(binding) = state.binding.as_ref() else { return };
        (cache::base(&binding.settings.cache_dir), binding.settings.proxy_url.clone())
    };
    if let Some(base) = &base {
        cache::maintain(base);
    }

    let pinned = with_database(|runtime, pool| {
        runtime.block_on(wows_toolkit_replay_data::directories::load_pinned(pool)).map_err(|error| error.to_string())
    });
    update(|state| match pinned {
        Ok(pinned) => {
            for root in pinned.unwrap_or_default() {
                state.directories.open(root, true);
            }
        }
        Err(reason) => state.directories_error = Some(format!("Pinned replay directories could not be read: {reason}")),
    });

    let rating = job_runtime().ok().and_then(|runtime| {
        runtime.block_on(wows_toolkit_replay_data::personal_rating::load_refreshed(USER_AGENT, &proxy))
    });
    update(|state| state.personal_rating = rating.map(Arc::new));
}

/// Queues a pass over `root`, starting it now when nothing else runs.
fn queue_index(root: PathBuf, mode: IndexMode) {
    let start = update(|state| {
        if state.index.cancel.is_some() {
            if !state.index.queued.iter().any(|(queued, _)| *queued == root) {
                state.index.queued.push((root.clone(), mode));
            }
            return None;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        state.index.cancel = Some(Arc::clone(&cancel));
        state.index.progress = Some(IndexProgress::default());
        state.index.outcome = None;
        Some(cancel)
    });
    let Some(cancel) = start else { return };

    spawn("replay-index", move || run_index(root, mode, cancel));
}

fn run_index(root: PathBuf, mode: IndexMode, cancel: Arc<AtomicBool>) {
    let cache = state().binding.as_ref().map(|binding| binding.cache.clone());
    let built = match cache {
        Some(cache) => with_database(|runtime, pool| {
            replay_index::build_index(runtime, pool, &root, &cache, &cancel, mode, |step| {
                update(|state| state.index.progress = Some(step));
            })
            .map_err(|error| error.to_string())
        }),
        None => Err("the settings database could not be read".to_owned()),
    };

    let next = update(|state| {
        state.index.cancel = None;
        state.index.progress = None;
        state.index.outcome = Some(match &built {
            Ok(progress) if progress.skipped > 0 => IndexOutcome {
                text: format!(
                    "Indexed {} replays, skipped {} already indexed, {} could not be read.",
                    progress.indexed, progress.skipped, progress.failed
                ),
                failed: progress.failed > 0,
            },
            Ok(progress) => IndexOutcome {
                text: format!("Indexed {} replays, {} could not be read.", progress.indexed, progress.failed),
                failed: progress.failed > 0,
            },
            Err(reason) => IndexOutcome { text: format!("The index could not be built: {reason}"), failed: true },
        });
        if cancel.load(Ordering::Relaxed) {
            state.index.queued.clear();
            return None;
        }
        (!state.index.queued.is_empty()).then(|| state.index.queued.remove(0))
    });
    if let Some((root, mode)) = next {
        queue_index(root, mode);
    }
}

/// Reads the settings the service binds to from the shared database.
fn read_settings() -> Result<BoundSettings, String> {
    with_database(|runtime, pool| {
        runtime.block_on(async {
            let wows_dir: String = optional(pool, keys::WOWS_DIR).await?.unwrap_or_default();
            let locale: Option<String> = optional::<Option<String>>(pool, keys::LOCALE).await?.flatten();
            let auto_dump: bool = optional(pool, keys::AUTO_DUMP_GAME_DATA).await?.unwrap_or(false);
            let cache_dir: String = optional(pool, keys::GAME_DATA_CACHE_DIR).await?.unwrap_or_default();
            let proxy_url: Option<String> = optional::<Option<String>>(pool, keys::PROXY_URL).await?.flatten();
            let repo_commit: Option<String> =
                optional::<Option<String>>(pool, keys::GAME_DATA_REPO_COMMIT).await?.flatten();

            Ok(BoundSettings {
                wows_dir: (!wows_dir.trim().is_empty()).then(|| PathBuf::from(wows_dir.trim())),
                // Game text is read in the chosen language, then the system's;
                // the catalog lookup itself falls back to English.
                locale: locale
                    .filter(|locale| !locale.is_empty())
                    .or_else(sys_locale::get_locale)
                    .unwrap_or_else(|| "en".to_owned()),
                // Off is the stored default too: a dump is gigabytes.
                auto_dump,
                cache_dir,
                proxy_url: proxy_url.unwrap_or_default(),
                repo_commit,
            })
        })
    })
}

async fn optional<T>(pool: &sqlx::SqlitePool, key: &str) -> Result<Option<T>, String>
where
    T: serde::de::DeserializeOwned,
{
    wows_toolkit_config::queries::try_get_setting(pool, key)
        .await
        .map_err(|error| format!("could not read setting {key:?}: {error}"))
}

/// Saves one setting. Blocking.
fn save_setting<T: serde::Serialize>(key: &str, value: &T) -> Result<(), String> {
    with_database(|runtime, pool| {
        runtime
            .block_on(wows_toolkit_config::queries::set_setting(pool, key, value))
            .map_err(|error| format!("could not save setting {key:?}: {error}"))
    })
}

fn job_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start a runtime: {error}"))
}

/// Runs `work` against the shared settings database on a runtime of its own.
fn with_database<T>(
    work: impl FnOnce(&tokio::runtime::Runtime, &sqlx::SqlitePool) -> Result<T, String>,
) -> Result<T, String> {
    let runtime = job_runtime()?;
    let pool = runtime
        .block_on(wows_toolkit_config::open_db())
        .map_err(|error| format!("could not open the settings database: {error}"))?;
    let result = work(&runtime, &pool);
    runtime.block_on(pool.close());

    result
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.to_owned()).spawn(work) {
        log::warn!("game data: no thread for {name}: {error}");
    }
}

/// The cache directory and proxy the cache jobs run against.
fn cache_target() -> Result<(PathBuf, String, Option<String>), String> {
    ensure_started();
    let state = state();
    let binding = state.binding.as_ref().ok_or_else(|| "the settings database could not be read".to_owned())?;
    let base = cache::base(&binding.settings.cache_dir)
        .ok_or_else(|| "there is no storage directory for the game data cache".to_owned())?;

    Ok((base, binding.settings.proxy_url.clone(), binding.settings.repo_commit.clone()))
}

/// Runs one cache job on a thread of its own, reporting progress into the
/// state. Refused while another job runs.
fn run_cache_job<F, Fut>(job: CacheJob, work: F) -> Result<(), String>
where
    F: FnOnce(reqwest::Client, PathBuf, Option<String>, Box<dyn Fn(cache::Progress) + Send>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<CacheOutcome, String>>,
{
    let (base, proxy, tip) = cache_target()?;
    let started = update(|state| {
        if state.cache.busy() {
            return false;
        }
        state.cache.start(job);
        state.cache_message = None;
        true
    });
    if !started {
        return Err("another game data cache job is running".to_owned());
    }

    spawn("game-data-cache", move || {
        let outcome = match (
            wows_toolkit_viewmodel::http::client(USER_AGENT, &proxy, reqwest::redirect::Policy::default()),
            job_runtime(),
        ) {
            (Ok(client), Ok(runtime)) => {
                let report: Box<dyn Fn(cache::Progress) + Send> =
                    Box::new(|step| update(|state| state.cache.progress = Some(step)));
                runtime.block_on(work(client, base, tip, report)).unwrap_or_else(CacheOutcome::Failed)
            }
            (Err(error), _) => CacheOutcome::Failed(error.to_string()),
            (_, Err(reason)) => CacheOutcome::Failed(reason),
        };
        cache_job_finished(outcome);
    });

    Ok(())
}

/// Folds a finished job into the state. A clean check or validation saves the
/// repository tip, so the next check can stop there; a run that found work
/// does not, or the next check would skip it.
fn cache_job_finished(outcome: CacheOutcome) {
    let remember = update(|state| {
        state.cache.finish();
        match outcome {
            CacheOutcome::Checked { tip, updates } => {
                let clean = updates.is_empty();
                state.cache_message = Some(if clean {
                    "Game data is up to date.".to_owned()
                } else {
                    format!("Updates are available for {} cached builds.", updates.len())
                });
                state.cache.updates = updates;
                clean.then_some(tip)
            }
            CacheOutcome::Validated { tip, repair } => {
                let clean = repair.is_empty();
                state.cache_message = Some(if clean {
                    "The game data cache is valid.".to_owned()
                } else {
                    format!("{} cached builds need repair.", repair.len())
                });
                state.cache.repair = repair;
                clean.then_some(tip)
            }
            CacheOutcome::Downloaded { fetched, failed } => {
                if fetched > 0 {
                    state.cache.updates.clear();
                    state.cache.repair.clear();
                    state.cache.forget_stats();
                }
                if failed.is_empty() {
                    state.cache_message = Some(format!("Downloaded {fetched} builds."));
                } else {
                    let failed: Vec<String> = failed.iter().map(u32::to_string).collect();
                    state.cache.failure = Some(format!("Some builds could not be downloaded: {}", failed.join(", ")));
                }
                None
            }
            CacheOutcome::Planned { plan } => {
                state.offer.planning = false;
                state.offer.objects = Some(plan.unique_missing_objects);
                state.offer.availability = plan
                    .resolved
                    .into_iter()
                    .map(|resolved| (resolved.requested_build, resolved.availability))
                    .collect();
                None
            }
            CacheOutcome::Failed(reason) => {
                if state.offer.planning {
                    state.offer.planning = false;
                    state.offer.failure = Some(format!("What the repository publishes could not be read: {reason}"));
                }
                state.cache.failure = Some(reason);
                None
            }
        }
    });

    if let Some(tip) = remember {
        update(|state| {
            state.cache.tip = Some(tip.clone());
            if let Some(binding) = state.binding.as_mut() {
                binding.settings.repo_commit = Some(tip.clone());
            }
        });
        if let Err(reason) = save_setting(keys::GAME_DATA_REPO_COMMIT, &Some(tip)) {
            log::warn!("{reason}");
        }
    }
}

/// Measures the cache when nothing has, on a thread of its own.
fn measure_cache_if_wanted() {
    let Ok((base, _, _)) = cache_target() else { return };
    // Claimed without telling the watchers: nothing they show changed yet,
    // and a notice here would wake `GameData.watch`, which measures again.
    let Some(generation) = state().cache.wants_measure() else { return };

    spawn("game-data-cache-measure", move || {
        let stats = cache::measure(&base);
        update(|state| state.cache.measured(stats, generation));
    });
}

/// Every replay in the game's replay directory and in the open directories,
/// grouped by the builds nothing on this machine can read. Blocking: reads
/// every replay's header.
fn find_missing_builds() -> Result<Vec<MissingBuild>, GameDataError> {
    let cache = cache().ok_or(GameDataError::NotConfigured)?;
    let mut roots: Vec<PathBuf> = replays_dir().into_iter().collect();
    roots.extend(state().directories.directories().iter().map(|directory| directory.root.clone()));

    let mut missing: BTreeMap<u32, MissingBuild> = BTreeMap::new();
    for root in roots {
        for path in replay_index::replays_under(&root) {
            let Ok(meta) = wows_replays::ReplayFile::meta_from_file(&path) else { continue };
            let Some(version) = Version::try_from_client_exe(&meta.clientVersionFromExe) else { continue };
            let Some(build) = version.build_number() else { continue };
            let hint = format!("{}.{}.{}", version.major, version.minor, version.patch);
            if cache.can_read_build(build, Some(&hint)) {
                continue;
            }
            missing.entry(build).or_insert(MissingBuild { build, version: Some(hint), replays: 0 }).replays += 1;
        }
    }

    Ok(missing.into_values().collect())
}

fn availability_text(availability: Option<&RemoteAvailability>) -> String {
    match availability {
        Some(RemoteAvailability::Exact) => "available".to_owned(),
        Some(RemoteAvailability::Nearest { version, build }) => format!("nearest available: {version} ({build})"),
        Some(RemoteAvailability::Unpublished) => "not published".to_owned(),
        Some(RemoteAvailability::Unreachable) => "repository unreachable".to_owned(),
        None => "checking".to_owned(),
    }
}

fn eligible(availability: Option<&RemoteAvailability>) -> bool {
    matches!(availability, Some(RemoteAvailability::Exact | RemoteAvailability::Nearest { .. }))
}

/// Where the game data stands, as the status indicator shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum GameDataPhase {
    #[default]
    NotConfigured,
    Loading,
    Ready,
    Failed,
}

/// How much of a replay directory an index pass re-reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum ReplayIndexMode {
    /// Read what is not indexed yet.
    #[default]
    FillGaps,
    /// Read everything again.
    RefreshAll,
}

impl From<ReplayIndexMode> for IndexMode {
    fn from(mode: ReplayIndexMode) -> Self {
        match mode {
            ReplayIndexMode::FillGaps => IndexMode::FillGaps,
            ReplayIndexMode::RefreshAll => IndexMode::RefreshAll,
        }
    }
}

/// The index pass, as the Settings section shows it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct IndexSnapshot {
    pub running: bool,
    pub done: u64,
    pub total: u64,
    pub indexed: u64,
    pub failed: u64,
    pub skipped: u64,
    /// What the last pass reported.
    pub outcome: Option<String>,
    /// Whether that pass left replays unread.
    pub outcome_failed: bool,
}

/// One cached build a job named.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CacheBuild {
    pub build: u32,
    pub version: String,
}

/// The game-data cache, as the Settings section shows it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct CacheSnapshot {
    /// Where the cache is. `None` when there is no storage directory at all.
    pub directory: Option<String>,
    /// The directory setting as stored; empty is the default location.
    pub directory_setting: String,
    pub auto_dump: bool,
    pub size_bytes: Option<u64>,
    pub build_count: Option<u64>,
    /// The job running: "checking", "validating", "downloading" or "planning".
    pub job: Option<String>,
    pub progress_done: Option<u64>,
    pub progress_total: Option<u64>,
    pub updates: Vec<CacheBuild>,
    pub repair: Vec<CacheBuild>,
    pub failure: Option<String>,
    pub message: Option<String>,
}

/// One open replay directory.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct DirectoryEntry {
    pub path: String,
    pub label: String,
    pub pinned: bool,
}

/// One build the download offer names.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct MissingBuildRow {
    pub build: u32,
    pub version: Option<String>,
    pub replays: u32,
    pub availability: String,
    /// Whether the repository publishes something for it, which is what lets
    /// it be ticked.
    pub eligible: bool,
}

/// The offer to download the game data replays need.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct MissingBuildOffer {
    pub open: bool,
    pub planning: bool,
    pub rows: Vec<MissingBuildRow>,
    pub objects: Option<u64>,
    pub failure: Option<String>,
}

/// Everything the game-data views show.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct GameDataSnapshot {
    pub phase: GameDataPhase,
    /// The loading or loaded version.
    pub version: Option<String>,
    pub build: Option<u32>,
    /// Why the load failed, or why the settings could not be read.
    pub reason: Option<String>,
    pub wows_dir: Option<String>,
    pub index: IndexSnapshot,
    pub cache: CacheSnapshot,
    pub directories: Vec<DirectoryEntry>,
    pub directories_error: Option<String>,
    pub offer: MissingBuildOffer,
}

fn snapshot() -> GameDataSnapshot {
    let state = state();
    let (phase, version, build, reason) = match &state.status {
        GameDataState::NotConfigured => (GameDataPhase::NotConfigured, None, None, state.settings_error.clone()),
        GameDataState::Loading { version } => (GameDataPhase::Loading, version.clone(), None, None),
        GameDataState::Ready(loaded) => {
            (GameDataPhase::Ready, loaded.version().map(|version| version.to_path()), Some(loaded.build()), None)
        }
        GameDataState::Failed { reason } => (GameDataPhase::Failed, None, None, Some(reason.clone())),
    };
    let settings = state.binding.as_ref().map(|binding| &binding.settings);
    let progress = state.index.progress.unwrap_or_default();
    let cache_builds = |builds: &[BuildUpdateStatus]| {
        builds.iter().map(|build| CacheBuild { build: build.build, version: build.version.clone() }).collect()
    };

    GameDataSnapshot {
        phase,
        version,
        build,
        reason,
        wows_dir: settings.and_then(|settings| settings.wows_dir.as_ref()).map(|dir| dir.display().to_string()),
        index: IndexSnapshot {
            running: state.index.cancel.is_some(),
            done: progress.done,
            total: progress.total,
            indexed: progress.indexed,
            failed: progress.failed,
            skipped: progress.skipped,
            outcome: state.index.outcome.as_ref().map(|outcome| outcome.text.clone()),
            outcome_failed: state.index.outcome.as_ref().is_some_and(|outcome| outcome.failed),
        },
        cache: CacheSnapshot {
            directory: settings
                .and_then(|settings| cache::base(&settings.cache_dir))
                .map(|dir| dir.display().to_string()),
            directory_setting: settings.map(|settings| settings.cache_dir.clone()).unwrap_or_default(),
            auto_dump: settings.is_some_and(|settings| settings.auto_dump),
            size_bytes: state.cache.stats.map(|stats| stats.total_bytes),
            build_count: state.cache.stats.map(|stats| stats.version_count as u64),
            job: state.cache.running.map(|job| {
                match job {
                    CacheJob::Checking => "checking",
                    CacheJob::Validating => "validating",
                    CacheJob::Downloading => "downloading",
                    CacheJob::Planning => "planning",
                }
                .to_owned()
            }),
            progress_done: state.cache.progress.map(|step| step.done),
            progress_total: state.cache.progress.map(|step| step.total),
            updates: cache_builds(&state.cache.updates),
            repair: cache_builds(&state.cache.repair),
            failure: state.cache.failure.clone(),
            message: state.cache_message.clone(),
        },
        directories: state
            .directories
            .directories()
            .iter()
            .map(|directory| DirectoryEntry {
                path: directory.root.display().to_string(),
                label: wows_toolkit_replay_data::directories::shortened_path(&directory.root),
                pinned: directory.pinned,
            })
            .collect(),
        directories_error: state.directories_error.clone(),
        offer: MissingBuildOffer {
            open: state.offer.open,
            planning: state.offer.planning,
            rows: state
                .offer
                .builds
                .iter()
                .map(|missing| {
                    let availability = state.offer.availability.get(&missing.build);
                    MissingBuildRow {
                        build: missing.build,
                        version: missing.version.clone(),
                        replays: missing.replays,
                        availability: if state.offer.planning || state.offer.failure.is_none() {
                            availability_text(availability)
                        } else {
                            "unknown".to_owned()
                        },
                        eligible: eligible(availability),
                    }
                })
                .collect(),
            objects: state.offer.objects.map(|count| count as u64),
            failure: state.offer.failure.clone(),
        },
    }
}

/// Saves the pinned directories. Blocking.
fn save_pinned() -> Result<(), String> {
    let pinned = state().directories.pinned_roots();
    with_database(|runtime, pool| {
        runtime
            .block_on(wows_toolkit_replay_data::directories::save_pinned(pool, &pinned))
            .map_err(|error| format!("Pinned replay directories could not be saved: {error}"))
    })
}

async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    blocking(work).await.map_err(|error| format!("game data task failed: {error}"))?
}

/// Game data services exposed to the TSX interface.
#[module]
pub struct GameData;

#[module]
impl GameData {
    /// Binds to the settings, once per process, and returns the state.
    pub async fn start() -> Result<GameDataSnapshot, String> {
        run_blocking(|| {
            ensure_started();
            measure_cache_if_wanted();
            Ok(snapshot())
        })
        .await
    }

    /// Reads the settings again and rebinds what they changed. Called after
    /// the WoWs directory, the language, the cache directory, the auto-dump
    /// flag or the proxy is saved.
    pub async fn reload() -> Result<GameDataSnapshot, String> {
        run_blocking(|| {
            rebind()?;
            measure_cache_if_wanted();
            Ok(snapshot())
        })
        .await
    }

    /// Calls `on_change` with the state now and after every change, for as
    /// long as the interface runs.
    pub async fn watch(on_change: Callback<GameDataSnapshot>) -> Result<(), String> {
        let mut changes = SERVICE.changed.subscribe();
        loop {
            measure_cache_if_wanted();
            // Awaited rather than posted: a callback whose engine is gone
            // fails here, which ends the feed.
            if on_change.call_async(snapshot()).await.is_err() {
                return Ok(());
            }
            if changes.changed().await.is_err() {
                return Ok(());
            }
        }
    }

    /// Indexes the game's replay directory. `false` when no WoWs directory is
    /// set.
    pub async fn index_replays(mode: ReplayIndexMode) -> Result<bool, String> {
        run_blocking(move || Ok(start_indexing(mode.into()))).await
    }

    /// Stops the running index pass after the replay it is reading.
    pub fn cancel_indexing() {
        if let Some(cancel) = state().index.cancel.as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Asks the repository which cached builds have newer data.
    pub async fn check_cache_updates() -> Result<(), String> {
        run_blocking(|| {
            run_cache_job(CacheJob::Checking, |client, base, tip, _report| async move {
                cache::check_for_updates(&client, &base, tip.as_deref()).await
            })
        })
        .await
    }

    /// Checks every cached object against the repository and its own hash.
    pub async fn validate_cache() -> Result<(), String> {
        run_blocking(|| {
            run_cache_job(CacheJob::Validating, |client, base, _tip, report| async move {
                cache::validate(&client, &base, report).await
            })
        })
        .await
    }

    /// Fetches every build the last check found stale.
    pub async fn update_cache_builds() -> Result<(), String> {
        run_blocking(|| {
            let builds = state().cache.updates.clone();
            download_builds(builds)
        })
        .await
    }

    /// Fetches every build the last validation found damaged.
    pub async fn repair_cache_builds() -> Result<(), String> {
        run_blocking(|| {
            let builds = state().cache.repair.clone();
            download_builds(builds)
        })
        .await
    }

    /// Removes every cached build but the newest.
    pub async fn delete_old_cache_builds() -> Result<u32, String> {
        run_blocking(|| {
            let (base, _, _) = cache_target()?;
            let deleted = wows_data_mgr::dump::delete_old_versions(&base);
            if deleted > 0 {
                update(|state| {
                    state.cache.forget_stats();
                    state.cache_message = Some(format!("Removed {deleted} old builds."));
                });
            }
            Ok(deleted as u32)
        })
        .await
    }

    /// Shows the cache directory in the file manager.
    pub async fn open_cache_folder() -> Result<(), String> {
        let (base, _, _) = run_blocking(cache_target).await?;
        std::fs::create_dir_all(&base).map_err(|error| format!("{}: {error}", base.display()))?;
        open_path(base).await
    }

    /// Saves whether a build loaded from the install is also written to the
    /// cache, and rebinds.
    pub async fn set_auto_dump(enabled: bool) -> Result<GameDataSnapshot, String> {
        run_blocking(move || {
            save_setting(keys::AUTO_DUMP_GAME_DATA, &enabled)?;
            rebind()?;
            Ok(snapshot())
        })
        .await
    }

    /// Asks for a cache directory and saves it. `None` when the dialog was
    /// cancelled.
    pub async fn pick_cache_directory() -> Result<Option<GameDataSnapshot>, String> {
        let picked = pick_paths(OpenDialog::folders().title("Game Data Cache Directory").button_label("Select"))
            .await
            .map_err(|error| format!("could not show the directory dialog: {error}"))?;
        let Some(directory) = picked.and_then(|mut paths| paths.pop()) else { return Ok(None) };

        run_blocking(move || {
            save_setting(keys::GAME_DATA_CACHE_DIR, &directory.display().to_string())?;
            rebind()?;
            measure_cache_if_wanted();
            Ok(Some(snapshot()))
        })
        .await
    }

    /// Lists the builds the replays on this machine need and nothing here
    /// has, and asks the repository what it publishes for each. Builds already
    /// offered this session are left out.
    pub async fn check_missing_builds() -> Result<MissingBuildOffer, String> {
        run_blocking(|| {
            let missing = find_missing_builds().map_err(|error| error.to_string())?;
            let missing: Vec<MissingBuild> = {
                let state = state();
                missing.into_iter().filter(|build| !state.offered_builds.contains(&build.build)).collect()
            };
            if missing.is_empty() {
                update(|state| state.offer = OfferState::default());
                return Ok(snapshot().offer);
            }

            let requests: Vec<(u32, Option<String>)> =
                missing.iter().map(|build| (build.build, build.version.clone())).collect();
            update(|state| {
                state.offer = OfferState { planning: true, builds: missing, open: true, ..OfferState::default() };
            });
            let planned = run_cache_job(CacheJob::Planning, move |client, base, _tip, _report| async move {
                cache::plan(&client, &base, &requests).await
            });
            if let Err(reason) = planned {
                update(|state| {
                    state.offer.planning = false;
                    state.offer.failure = Some(reason);
                });
            }
            Ok(snapshot().offer)
        })
        .await
    }

    /// Downloads the offered builds named in `builds` and closes the offer.
    /// Every offered build is answered for, ticked or not, so a later check
    /// does not offer it again.
    pub async fn download_missing_builds(builds: Vec<u32>) -> Result<(), String> {
        run_blocking(move || {
            let wanted: Vec<BuildUpdateStatus> = update(|state| {
                let offer = std::mem::take(&mut state.offer);
                state.offered_builds.extend(offer.builds.iter().map(|build| build.build));
                offer
                    .builds
                    .into_iter()
                    .filter(|missing| builds.contains(&missing.build))
                    .filter(|missing| eligible(offer.availability.get(&missing.build)))
                    .map(|missing| BuildUpdateStatus {
                        build: missing.build,
                        version: missing.version.unwrap_or_default(),
                    })
                    .collect()
            });
            if wanted.is_empty() {
                return Ok(());
            }
            download_builds(wanted)
        })
        .await
    }

    /// Closes the offer without downloading.
    pub fn dismiss_missing_builds() {
        update(|state| {
            let offered = std::mem::take(&mut state.offer).builds;
            state.offered_builds.extend(offered.iter().map(|build| build.build));
        });
    }

    /// Opens a replay directory and queues an index pass over it.
    pub async fn open_directory(path: String, pinned: bool) -> Result<(), String> {
        run_blocking(move || open_registered_directory(PathBuf::from(path), pinned)).await
    }

    /// Asks for a replay directory and opens it. `false` when the dialog was
    /// cancelled.
    pub async fn pick_directory() -> Result<bool, String> {
        let picked = pick_paths(OpenDialog::folders().title("Open Replay Directory").button_label("Open"))
            .await
            .map_err(|error| format!("could not show the directory dialog: {error}"))?;
        let Some(directory) = picked.and_then(|mut paths| paths.pop()) else { return Ok(false) };

        run_blocking(move || open_registered_directory(directory, false).map(|()| true)).await
    }

    /// Closes a replay directory. A pinned one stays open.
    pub fn close_directory(path: String) -> Result<(), String> {
        update(|state| state.directories.close(Path::new(&path))).map_err(|refusal| match refusal {
            CloseRefusal::Pinned { .. } | CloseRefusal::NotOpen { .. } => refusal.to_string(),
        })
    }

    /// Pins or unpins a replay directory and saves the pinned set.
    pub async fn pin_directory(path: String, pinned: bool) -> Result<(), String> {
        run_blocking(move || {
            if !update(|state| state.directories.set_pinned(Path::new(&path), pinned)) {
                return Err(format!("{path} is not open"));
            }
            save_pinned()
        })
        .await
    }

    /// Shows a replay directory in the file manager.
    pub async fn reveal_directory(path: String) -> Result<(), String> {
        open_path(PathBuf::from(path)).await
    }
}

fn open_registered_directory(root: PathBuf, pinned: bool) -> Result<(), String> {
    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let root = update(|state| {
        let opened = state.directories.open(root, pinned);
        let index = match opened {
            wows_toolkit_replay_data::directories::Opened::Added(index)
            | wows_toolkit_replay_data::directories::Opened::Existing(index) => index,
        };
        state.directories.directories()[index].root.clone()
    });
    if pinned {
        save_pinned()?;
    }
    if cache().is_some() {
        queue_index(root, IndexMode::FillGaps);
    }

    Ok(())
}

fn download_builds(builds: Vec<BuildUpdateStatus>) -> Result<(), String> {
    if builds.is_empty() {
        return Ok(());
    }
    run_cache_job(CacheJob::Downloading, move |client, base, _tip, report| async move {
        cache::download(&client, &base, &builds, report).await
    })
}
