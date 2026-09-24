//! The game-data cache the Settings tab reports on and manages.
//!
//! The cache holds one dumped build per game version, so a replay recorded
//! against a build the live install no longer has can still be opened. The
//! egui app writes it on load; this port reports what is there and runs the
//! same maintenance against the published repository, through
//! `wows_data_mgr::download_repo`, so both apps act on one cache.
//!
//! Every operation here reaches the network or walks a directory of several
//! gigabytes, so all of them run off the UI thread and report back by
//! channel. The egui settings tab measures the cache inline on the UI thread
//! instead, which is what makes its Settings tab stutter on a large cache.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::AsyncApp;
use gpui_kit::Context;
use gpui_kit::Entity;
use wows_data_mgr::download_repo;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_data_mgr::dump::CacheStats;

use crate::http;
use crate::runtime;

/// What the cache tab is doing, when it is doing something.
///
/// One at a time: all three reach the same repository and the same directory,
/// and the controls that start them are refused while one runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheJob {
    /// Comparing each cached build against the published repository.
    Checking,
    /// Verifying every cached object against the repository and its own hash.
    Validating,
    /// Fetching builds, either to update them or to repair them.
    Downloading,
}

/// How far a job has got. Absent while a job reports no steps of its own,
/// which is what the update check does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// The cache as the Settings tab knows it.
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

    fn start(&mut self, job: CacheJob) {
        self.running = Some(job);
        self.progress = None;
        self.failure = None;
    }

    fn finish(&mut self) {
        self.running = None;
        self.progress = None;
    }
}

/// Where the cache is, given the directory setting.
///
/// `None` when there is no storage directory to fall back on, which is the
/// only case in which the section cannot say anything at all.
pub fn base(cache_dir: &str) -> Option<PathBuf> {
    wows_toolkit_config::game_data_dump_base_with_override(cache_dir)
}

/// Measures the cache off the UI thread and hands the result to `apply`.
///
/// The walk is over every stored object, so on an established cache it is
/// thousands of files; doing it inline is what makes the egui tab stutter.
///
/// `generation` comes back with the answer so the caller can drop one taken
/// against a directory that has since been left.
pub fn measure<V: 'static>(
    base: PathBuf,
    generation: u64,
    cx: &mut Context<V>,
    apply: impl FnOnce(&mut V, CacheStats, u64, &mut Context<V>) + 'static,
) {
    cx.spawn(async move |view, cx: &mut AsyncApp| {
        let measured = cx.background_spawn(async move { wows_data_mgr::dump::cache_stats(&base) }).await;
        let _ = view.update(cx, |view, cx| apply(view, measured, generation, cx));
    })
    .detach();
}

/// Reports what a finished job found, for the view to fold into its state.
pub enum CacheOutcome {
    /// The repository tip, and the cached builds whose data has changed.
    Checked { tip: String, updates: Vec<BuildUpdateStatus> },
    /// The repository tip, and the cached builds that need re-fetching.
    Validated { tip: String, repair: Vec<BuildUpdateStatus> },
    /// How many builds were fetched, and which ones could not be.
    Downloaded { fetched: usize, failed: Vec<u32> },
    /// The job did not finish. Carries what to show under the controls.
    Failed(String),
}

/// Runs an operation against the cache and reports back through `apply`.
///
/// `proxy_url` is the setting as stored: empty means direct.
fn run<V, Work, Fut>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    job: CacheJob,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    work: Work,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) where
    V: 'static,
    Work: FnOnce(reqwest::Client, ProgressSink) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<CacheOutcome, String>>,
{
    view.update(cx, |view, cx| {
        state(view).start(job);
        cx.notify();
    });

    let (progress_tx, mut progress_rx) = futures::channel::mpsc::unbounded::<Progress>();
    let view = view.clone();

    // The bar follows the work rather than waiting for it, so a long
    // validation is visibly running rather than looking hung.
    let watched = view.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        use futures::StreamExt as _;
        while let Some(step) = progress_rx.next().await {
            watched.update(cx, |view, cx| {
                state(view).progress = Some(step);
                cx.notify();
            });
        }
    })
    .detach();

    cx.spawn(async move |cx: &mut AsyncApp| {
        let outcome = match http::client(&proxy_url, reqwest::redirect::Policy::default()) {
            Ok(client) => {
                let sink = ProgressSink(progress_tx);
                runtime::block_on(cx, move || work(client, sink)).await.unwrap_or_else(CacheOutcome::Failed)
            }
            Err(err) => CacheOutcome::Failed(err.to_string()),
        };

        view.update(cx, |view, cx| {
            state(view).finish();
            if let CacheOutcome::Failed(reason) = &outcome {
                state(view).failure = Some(reason.clone());
            }
            apply(view, outcome, cx);
            cx.notify();
        });
    })
    .detach();
}

/// The `Fn(u64, u64)` the `wows_data_mgr` calls want, forwarding to the UI.
///
/// Unbounded and non-blocking: a progress report must never hold up the work
/// reporting it, and a report that arrives after the view is gone is dropped.
pub struct ProgressSink(futures::channel::mpsc::UnboundedSender<Progress>);

impl ProgressSink {
    fn report(&self, done: u64, total: u64) {
        let _ = self.0.unbounded_send(Progress { done, total });
    }
}

/// Asks the repository which cached builds have newer data.
pub fn check_for_updates<V: 'static>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    base: PathBuf,
    known_tip: Option<String>,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) {
    run(
        state,
        CacheJob::Checking,
        proxy_url,
        view,
        cx,
        move |client, _progress| async move {
            download_repo::check_for_updates(&client, download_repo::DEFAULT_REPO_BASE_URL, &base, known_tip.as_deref())
                .await
                .map(|checked| CacheOutcome::Checked { tip: checked.tip, updates: checked.updates })
                .map_err(|err| err.to_string())
        },
        apply,
    );
}

/// Checks every cached object against the repository and its own hash.
pub fn validate<V: 'static>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    base: PathBuf,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) {
    run(
        state,
        CacheJob::Validating,
        proxy_url,
        view,
        cx,
        move |client, progress| async move {
            download_repo::validate_cache(&client, download_repo::DEFAULT_REPO_BASE_URL, &base, |done, total| {
                progress.report(done, total)
            })
            .await
            .map(|validated| {
                let repair = validated
                    .needs_repair()
                    .map(|build| BuildUpdateStatus { build: build.build, version: build.version.clone() })
                    .collect();
                CacheOutcome::Validated { tip: validated.tip, repair }
            })
            .map_err(|err| err.to_string())
        },
        apply,
    );
}

/// Re-fetches the named builds, whether they are stale or damaged.
///
/// Forced, as the egui app forces both of its own: the builds named here are
/// exactly those already found to differ from the repository, so the check
/// that would skip an unchanged one has already been made.
pub fn download<V: 'static>(
    state: impl Fn(&mut V) -> &mut CacheState + Copy + 'static,
    base: PathBuf,
    builds: Vec<BuildUpdateStatus>,
    proxy_url: String,
    view: &Entity<V>,
    cx: &mut App,
    apply: impl FnOnce(&mut V, CacheOutcome, &mut Context<V>) + 'static,
) {
    run(
        state,
        CacheJob::Downloading,
        proxy_url,
        view,
        cx,
        move |client, progress| async move {
            let requests: Vec<(u32, Option<String>)> =
                builds.iter().map(|build| (build.build, Some(build.version.clone()))).collect();
            let results = download_repo::download_builds(
                &client,
                download_repo::DEFAULT_REPO_BASE_URL,
                &base,
                &requests,
                true,
                |done, total| progress.report(done, total),
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
            Ok(CacheOutcome::Downloaded { fetched, failed })
        },
        apply,
    );
}
