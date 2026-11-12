//! The game-data cache's maintenance, as the settings drawer starts it: measuring the cache,
//! checking and validating it against the published repository, fetching builds and pruning old
//! ones.
//!
//! The jobs outlive the drawer, which is rebuilt each time it opens, so their state is the shell's
//! and their tasks are detached.

use std::path::PathBuf;
use std::sync::Arc;

use rust_i18n::t;
use tokio::runtime::Runtime;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_toolkit_services::game_data_cache::CacheJob;
use wows_toolkit_services::game_data_cache::CacheOutcome;
use wows_toolkit_services::game_data_cache::CacheState;
use wows_toolkit_services::game_data_cache::Progress as CacheProgress;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::blocking;
use zgui::task::spawn_detached;

use crate::services::Job;
use crate::services::Progress;
use crate::services::progress;
use crate::shell::Shell;

/// What is known about the cache and the job running against it.
#[derive(Clone, Copy)]
pub struct GameDataCacheJobs {
    state: RwSignal<CacheState, LocalStorage>,
    /// Whether old builds are being deleted, which no `CacheJob` names but which must not run
    /// beside one.
    pruning: RwSignal<bool, LocalStorage>,
}

impl GameDataCacheJobs {
    pub fn new() -> Self {
        Self { state: RwSignal::new_local(CacheState::default()), pruning: RwSignal::new_local(false) }
    }

    /// Reads the state, subscribing to every change.
    pub fn with<R>(self, read: impl FnOnce(&CacheState) -> R) -> R {
        self.state.with(read)
    }

    /// Whether a job is under way, which refuses the controls that start another.
    pub fn busy(self) -> bool {
        self.state.with(CacheState::busy) || self.pruning.get()
    }

    fn busy_untracked(self) -> bool {
        self.state.with_untracked(CacheState::busy) || self.pruning.get_untracked()
    }

    /// Measures the cache under `base` off the UI thread, unless it is measured or being measured.
    pub fn measure_once(self, base: PathBuf) {
        // Notifies only when a walk starts: the caller's effect reads the state, and a notification
        // for a refused measure would run it again without end.
        let wanted = self.state.try_maybe_update(|state| {
            let generation = state.wants_measure();
            (generation.is_some(), generation)
        });
        let Some(generation) = wanted.flatten() else { return };

        spawn_detached(async move {
            let measured = blocking(move || wows_toolkit_services::game_data_cache::measure(&base)).await;
            self.state.try_update(|state| state.measured(measured, generation));
        });
    }

    /// Drops everything known about the cache, so the next look measures it again.
    ///
    /// The stale and damaged lists name builds under one directory; carried across a change, they
    /// would offer to repair builds that are not there.
    pub fn forget_findings(self) {
        self.state.update(|state| {
            state.forget_stats();
            state.updates.clear();
            state.repair.clear();
            state.failure = None;
        });
        // A fire-chance section that found no geometry found none in the old directory. Kept, it
        // would stay failed until a restart even once its data is in the new one.
        wows_replay_insights::fire_chance::sections::clear_fire_section_failures();
    }
}

impl Default for GameDataCacheJobs {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Asks the repository which cached builds have newer data.
    pub fn check_cache_for_updates(self, base: PathBuf) {
        let known_tip = self.services().settings.read_untracked(|settings| settings.game_data_repo_commit.clone());
        let known_tip = known_tip.flatten();
        run(self, CacheJob::Checking, base, move |client, _reports, base| async move {
            wows_toolkit_services::game_data_cache::check_for_updates(&client, &base, known_tip.as_deref()).await
        });
    }

    /// Checks every cached object against the repository and its own hash.
    pub fn validate_cache(self, base: PathBuf) {
        run(self, CacheJob::Validating, base, move |client, reports, base| async move {
            wows_toolkit_services::game_data_cache::validate(&client, &base, |step| report(&reports, step)).await
        });
    }

    /// Fetches `builds` again, whether they are stale or damaged.
    pub fn fetch_cache_builds(self, base: PathBuf, builds: Vec<BuildUpdateStatus>) {
        if builds.is_empty() {
            return;
        }

        run(self, CacheJob::Downloading, base, move |client, reports, base| async move {
            wows_toolkit_services::game_data_cache::download(&client, &base, &builds, |step| report(&reports, step))
                .await
        });
    }

    /// Prunes every cached build but the newest, off the UI thread: it removes whole build
    /// directories, which on a large cache is thousands of files.
    pub fn delete_old_cache_versions(self, base: PathBuf) {
        let cache = self.cache;
        if cache.busy_untracked() {
            return;
        }
        cache.pruning.set(true);
        spawn_detached(async move {
            let deleted = blocking(move || wows_data_mgr::dump::delete_old_versions(&base)).await;
            cache.pruning.try_set(false);
            if deleted > 0 {
                cache.forget_findings();
            }
        });
    }

    /// The cache directory the settings name now.
    fn cache_base(self) -> Option<PathBuf> {
        self.services()
            .settings
            .read_untracked(|settings| wows_toolkit_services::game_data_cache::base(&settings.game_data_cache_dir))
            .flatten()
    }

    /// Folds a finished job back into the state and says what it found.
    ///
    /// A clean check or validation is what lets the next check stop at the tip, so the tip is saved
    /// only when nothing needs doing; saved after a run that found work, it would skip the re-check.
    fn cache_job_finished(self, outcome: CacheOutcome) {
        let toasts = self.toasts();
        self.cache.state.update(|state| state.finish(&outcome));

        match outcome {
            CacheOutcome::Checked { tip, updates } => {
                let clean = updates.is_empty();
                if clean {
                    toasts.ok(t!("ui.messages.game_data_up_to_date").into_owned());
                } else {
                    toasts.warn(t!("ui.messages.game_data_updates_available", count = updates.len()).into_owned());
                }
                self.cache.state.update(|state| state.updates = updates);
                if clean {
                    self.remember_cache_tip(tip);
                }
            }
            CacheOutcome::Validated { tip, repair } => {
                let clean = repair.is_empty();
                if clean {
                    toasts.ok(t!("ui.messages.game_data_cache_valid").into_owned());
                } else {
                    toasts.warn(t!("ui.messages.game_data_cache_invalid", count = repair.len()).into_owned());
                }
                self.cache.state.update(|state| state.repair = repair);
                if clean {
                    self.remember_cache_tip(tip);
                }
            }
            CacheOutcome::Downloaded { fetched, failed } => {
                if fetched > 0 {
                    // What was fetched is no longer stale or damaged, and the cache is a different
                    // size than it was.
                    self.cache.state.update(|state| {
                        state.updates.clear();
                        state.repair.clear();
                        state.forget_stats();
                    });
                }
                if failed.is_empty() {
                    toasts.ok(t!("ui.messages.game_data_builds_downloaded", count = fetched).into_owned());
                } else {
                    let said = t!("ui.messages.game_data_download_failed").into_owned();
                    toasts.failed(said.clone());
                    self.cache.state.update(|state| state.failure = Some(said));
                }
            }
            // Only the replays tab's offer for a missing build asks for a plan, and it reads the
            // plan itself.
            CacheOutcome::Planned { .. } => {}
            CacheOutcome::Failed(reason) => {
                toasts.failed(reason);
            }
        }
    }

    fn remember_cache_tip(self, tip: String) {
        self.cache.state.update(|state| state.tip = Some(tip.clone()));
        self.services().settings.edit(keys::GAME_DATA_REPO_COMMIT, |settings| {
            settings.game_data_repo_commit = Some(tip.clone());
            Some(tip)
        });
    }
}

/// Where a job sends its progress. Unbounded: a report must never hold up the work making it, and
/// one that arrives after the shell is gone is dropped.
type Reports = futures::channel::mpsc::UnboundedSender<CacheProgress>;

fn report(reports: &Reports, step: CacheProgress) {
    let _ = reports.unbounded_send(step);
}

/// Runs `work` against the cache under `base`, with the status bar naming it until it finishes.
///
/// `work` builds futures that are not `Send` (some of `wows_data_mgr`'s download futures hold
/// state across an await that cannot cross threads), so it runs on a worker thread of its own with
/// the shared runtime driving it.
fn run<Work, Fut>(shell: Shell, job: CacheJob, base: PathBuf, work: Work)
where
    Work: FnOnce(reqwest::Client, Reports, PathBuf) -> Fut + Send + 'static,
    Fut: Future<Output = CacheOutcome>,
{
    let services = shell.services();
    let cache = shell.cache;
    if cache.busy_untracked() {
        return;
    }
    let Some(proxy_url) = services.settings.read_untracked(|settings| settings.proxy_url.clone()) else { return };
    let runtime: Option<Arc<Runtime>> = services.runtime();

    cache.state.update(|state| state.start(job));
    let named = job_name(job);
    let held = services.jobs.start(Job { name: named.clone(), progress: None });
    let (reports, received) = futures::channel::mpsc::unbounded::<CacheProgress>();

    let asked = base.clone();
    spawn_detached(async move {
        let working = blocking(move || {
            let Some(runtime) = runtime else {
                return CacheOutcome::Failed(crate::services::TokioError::NoRuntime.to_string());
            };
            match wows_toolkit_services::http::client(
                crate::http::USER_AGENT,
                &proxy_url,
                reqwest::redirect::Policy::default(),
            ) {
                Ok(client) => runtime.block_on(work(client, reports, base)),
                Err(err) => CacheOutcome::Failed(err.to_string()),
            }
        });
        let outcome = progress::follow(working, received, |step| {
            cache.state.update(|state| state.progress = Some(step));
            held.report(Job { name: named.clone(), progress: Progress::new(step.done, step.total) });
        })
        .await;

        drop(held);
        // What a job found belongs to the directory it ran against; shown against another, its
        // update and repair lists would be fetched into the wrong cache.
        if shell.cache_base().as_ref() != Some(&asked) {
            cache.state.update(|state| state.finish(&outcome));
            return;
        }
        shell.cache_job_finished(outcome);
    });
}

/// What the status bar calls `job`.
fn job_name(job: CacheJob) -> String {
    match job {
        CacheJob::Checking => t!("ui.app.status_cache_checking"),
        CacheJob::Validating => t!("ui.app.status_cache_validating"),
        CacheJob::Downloading => t!("ui.app.status_cache_downloading"),
        CacheJob::Planning => t!("ui.app.status_cache_planning"),
    }
    .into_owned()
}
