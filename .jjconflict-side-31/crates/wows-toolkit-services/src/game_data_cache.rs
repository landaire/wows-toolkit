//! The game-data cache a settings page reports on and manages.
//!
//! The cache holds one dumped build per game version, so a replay recorded
//! against a build the live install no longer has can still be opened. The
//! operations here run the same maintenance as the egui app against the
//! published repository, through `wows_data_mgr::download_repo`, so both apps
//! act on one cache.
//!
//! Every operation reaches the network or walks a directory of several
//! gigabytes. The jobs are futures that are not `Send`: some of
//! `wows_data_mgr`'s download futures hold state across an await that cannot
//! cross threads, so a front end builds and drives each one on a single worker
//! thread with the tokio reactor entered. [`maintain`] blocks.

use std::path::Path;
use std::path::PathBuf;

use wows_data_mgr::download_repo;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_data_mgr::dump::CacheStats;

/// What the cache page is doing, when it is doing something.
///
/// One at a time: all of them reach the same repository and the same directory,
/// and the controls that start them are refused while one runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheJob {
    /// Comparing each cached build against the published repository.
    Checking,
    /// Verifying every cached object against the repository and its own hash.
    Validating,
    /// Fetching builds, either to update them or to repair them.
    Downloading,
    /// Asking the repository what it publishes for a selection of builds, and
    /// how much fetching them would come to.
    Planning,
}

/// How far a job has got. Absent while a job reports no steps of its own,
/// which is what the update check does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// The cache as the settings page knows it.
#[derive(Debug, Default)]
pub struct CacheState {
    /// What the cache occupies. `None` until it has been measured, which is
    /// also what a directory change returns it to.
    pub stats: Option<CacheStats>,
    /// Whether a measurement is already under way. The section asks for one
    /// as it draws, so without this every frame would start another walk of
    /// the same directory.
    measuring: bool,
    /// Cached builds whose published data has since changed.
    pub updates: Vec<BuildUpdateStatus>,
    /// Cached builds that are out of date or damaged.
    pub repair: Vec<BuildUpdateStatus>,
    /// The job under way, and how far it has got.
    pub running: Option<CacheJob>,
    pub progress: Option<Progress>,
    /// What went wrong, for the line under the controls. Cleared when the
    /// next job starts.
    pub failure: Option<String>,
    /// The repository tip the cache was last found to match, to be written
    /// back so the next check can stop at the tip.
    pub tip: Option<String>,
    /// Bumped whenever the directory changes, so a measurement that started
    /// against the old one is discarded rather than shown against the new.
    generation: u64,
}

impl CacheState {
    /// Whether a job is under way, which is what refuses the controls.
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Forgets the measurement, so the next draw takes a fresh one. Called
    /// when the directory changes or a build is deleted under it.
    ///
    /// A walk already running is left to finish; it is told to discard its
    /// answer, which belongs to the directory that has just been left.
    pub fn forget_stats(&mut self) {
        self.stats = None;
        self.measuring = false;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Whether a measurement should be started now, and which directory it
    /// would belong to.
    ///
    /// `None` when the answer is already in hand or a walk is already
    /// running. The section asks as it draws, so without that second
    /// condition every frame would start another walk of the same directory.
    pub fn wants_measure(&mut self) -> Option<u64> {
        if self.stats.is_some() || self.measuring {
            return None;
        }
        self.measuring = true;
        Some(self.generation)
    }

    /// Takes a finished measurement, unless the directory has moved on.
    pub fn measured(&mut self, stats: CacheStats, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.measuring = false;
        self.stats = Some(stats);
    }

    /// Marks `job` as under way, clearing what the last one left.
    pub fn start(&mut self, job: CacheJob) {
        self.running = Some(job);
        self.progress = None;
        self.failure = None;
    }

    /// Folds a finished job's outcome into the state.
    pub fn finish(&mut self, outcome: &CacheOutcome) {
        self.running = None;
        self.progress = None;
        if let CacheOutcome::Failed(reason) = outcome {
            self.failure = Some(reason.clone());
        }
    }
}

/// Reports what a finished job found, for the view to fold into its state.
pub enum CacheOutcome {
    /// The repository tip, and the cached builds whose data has changed.
    Checked { tip: String, updates: Vec<BuildUpdateStatus> },
    /// The repository tip, and the cached builds that need re-fetching.
    Validated { tip: String, repair: Vec<BuildUpdateStatus> },
    /// How many builds were fetched, and which ones could not be.
    Downloaded { fetched: usize, failed: Vec<u32> },
    /// What the repository publishes for each requested build, and how many CAS
    /// objects the whole selection would fetch.
    Planned { plan: download_repo::DownloadPlan },
    /// The job did not finish. Carries what to show under the controls.
    Failed(String),
}

/// Where the cache is, given the directory setting.
///
/// `None` when there is no storage directory to fall back on, which is the
/// only case in which the section cannot say anything at all.
pub fn base(cache_dir: &str) -> Option<PathBuf> {
    wows_toolkit_config::game_data_dump_base_with_override(cache_dir)
}

/// What the cache occupies. Blocks for the walk over every stored object,
/// which on an established cache is thousands of files.
pub fn measure(base: &Path) -> CacheStats {
    wows_data_mgr::dump::cache_stats(base)
}

/// Brings an older cache layout up to date and drops what nothing references.
///
/// Three passes the egui app runs at startup for the same reasons
/// (`app.rs`'s game-data-cache-maintenance thread): a cache written before the
/// content store was renamed still has its old directory, a cache written before
/// the store existed holds whole build directories that can be deduplicated into
/// it, and a build directory deleted by hand leaves objects nothing points at.
///
/// Blocks, and reports only to the log: nothing the reader asked for is waiting
/// on it, and a cache that cannot be tidied still reads.
pub fn maintain(base: &Path) {
    if !base.exists() {
        return;
    }

    match wows_data_mgr::dump::migrate_cas_dir_name(base) {
        Ok(true) => tracing::info!("game data cache: the content store was moved to its current name"),
        Ok(false) => {}
        Err(err) => tracing::warn!("game data cache: the content store was not moved: {err}"),
    }
    match wows_data_mgr::dump::migrate_to_cas(base) {
        Ok(migrated) if migrated > 0 => {
            tracing::info!(migrated, "game data cache: builds were moved into the content store")
        }
        Ok(_) => {}
        Err(err) => tracing::warn!("game data cache: builds were not moved into the content store: {err}"),
    }
    match wows_data_mgr::dump::gc_unreferenced(base) {
        Ok(dropped) if dropped > 0 => {
            tracing::info!(dropped, "game data cache: objects nothing referenced were dropped")
        }
        Ok(_) => {}
        Err(err) => tracing::warn!("game data cache: unreferenced objects were not dropped: {err}"),
    }
}

/// Asks the repository which cached builds have newer data.
pub async fn check_for_updates(client: &reqwest::Client, base: &Path, known_tip: Option<&str>) -> CacheOutcome {
    download_repo::check_for_updates(client, download_repo::DEFAULT_REPO_BASE_URL, base, known_tip).await.map_or_else(
        |err| CacheOutcome::Failed(err.to_string()),
        |checked| CacheOutcome::Checked { tip: checked.tip, updates: checked.updates },
    )
}

/// Checks every cached object against the repository and its own hash.
pub async fn validate(client: &reqwest::Client, base: &Path, progress: impl Fn(Progress)) -> CacheOutcome {
    let validated = download_repo::validate_cache(client, download_repo::DEFAULT_REPO_BASE_URL, base, |done, total| {
        progress(Progress { done, total })
    })
    .await;

    match validated {
        Ok(validated) => {
            let repair = validated
                .needs_repair()
                .map(|build| BuildUpdateStatus { build: build.build, version: build.version.clone() })
                .collect();
            CacheOutcome::Validated { tip: validated.tip, repair }
        }
        Err(err) => CacheOutcome::Failed(err.to_string()),
    }
}

/// Asks the repository what it has for each of `builds`, and what fetching them
/// all would come to.
///
/// `builds` pairs each build with the version hint the replays that need it
/// reported, which is what lets the repository answer with a nearest published
/// build rather than nothing at all.
pub async fn plan(client: &reqwest::Client, base: &Path, builds: &[(u32, Option<String>)]) -> CacheOutcome {
    let cas_root = wows_data_mgr::cas::cas_root(base);

    download_repo::plan_download(client, download_repo::DEFAULT_REPO_BASE_URL, &cas_root, builds)
        .await
        .map_or_else(|err| CacheOutcome::Failed(err.to_string()), |plan| CacheOutcome::Planned { plan })
}

/// Re-fetches the named builds, whether they are stale or damaged.
///
/// Forced, as the egui app forces both of its own: the builds named here are
/// exactly those already found to differ from the repository, so the check
/// that would skip an unchanged one has already been made.
pub async fn download(
    client: &reqwest::Client,
    base: &Path,
    builds: &[BuildUpdateStatus],
    progress: impl Fn(Progress),
) -> CacheOutcome {
    let requests: Vec<(u32, Option<String>)> =
        builds.iter().map(|build| (build.build, Some(build.version.clone()))).collect();
    let results = download_repo::download_builds(
        client,
        download_repo::DEFAULT_REPO_BASE_URL,
        base,
        &requests,
        true,
        |done, total| progress(Progress { done, total }),
    )
    .await;

    let mut fetched = 0;
    let mut failed = Vec::new();
    for (request, result) in requests.iter().zip(results) {
        match result {
            Ok(_) => fetched += 1,
            Err(err) => {
                tracing::warn!("game data cache: build {} could not be fetched: {err}", request.0);
                failed.push(request.0);
            }
        }
    }

    CacheOutcome::Downloaded { fetched, failed }
}
