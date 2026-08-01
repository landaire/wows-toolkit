use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use rootcause::Report;
use rootcause::prelude::*;
use rootcause::report;

use super::BackgroundTask;
use super::BackgroundTaskCompletion;
use super::BackgroundTaskKind;
use super::DownloadProgress;

/// What the toolkit should redo once a game data download has been tried.
///
/// A download carries at most one of these, so it cannot be waiting to reopen a
/// replay and to walk a directory at the same time.
#[derive(Clone)]
pub enum GameDataFollowUp {
    /// One replay was opened directly; reopen it.
    Replay(PathBuf),
    /// A directory listing was short of the replays this build covers; walk it
    /// again.
    Directory(crate::db::index::rows::WorkspaceId),
}

/// Download game data for `target_build` from the wows-replay-data repository
/// into `output_base`. `version_hint` (the replay's `major.minor.patch` string)
/// allows falling back to a different build of the same version when no exact
/// match is published. When `force` is true an existing copy is rebuilt to pick
/// up newer remote data.
///
/// `follow_up` rides on the task rather than on the app so the thing to redo is
/// released with the download that asked for it, whatever the download's
/// outcome. `None` for downloads nothing is waiting on.
pub fn start_game_data_download_task(
    output_base: PathBuf,
    target_build: u32,
    version_hint: Option<String>,
    force: bool,
    follow_up: Option<GameDataFollowUp>,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    let (progress_tx, progress_rx) = mpsc::channel();

    crate::util::thread::spawn_logged("download-game-data", move || {
        let _ = tx.send(download(output_base, target_build, version_hint, force, &progress_tx));
    });

    BackgroundTask {
        receiver: Some(rx),
        kind: BackgroundTaskKind::DownloadingGameData { rx: progress_rx, last_progress: None, follow_up },
    }
}

/// Identifies one run of the planner. Both a delivered plan and the reaping of
/// the task that produced it are matched back to the request through this, so
/// two offers over the same builds are told apart where comparing the builds
/// they asked about cannot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanTicket(u64);

impl PlanTicket {
    /// Hand out the next ticket. Monotonic for the life of the process.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Resolve `builds` against the remote repository and count the CAS objects the
/// whole selection would have to fetch. Runs off the UI thread: it makes one
/// index request plus one metadata request per distinct resolved build.
///
/// `ticket` is the caller's handle on this run; it comes back on both the task
/// kind and the completion.
pub fn start_game_data_plan_task(
    output_base: PathBuf,
    builds: Vec<(u32, Option<String>)>,
    ticket: PlanTicket,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();

    crate::util::thread::spawn_logged("plan-game-data-download", move || {
        let _ = tx.send(plan(output_base, builds, ticket));
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::PlanningGameDataDownload { ticket } }
}

/// Check the repository for updates to builds already cached in `output_base`.
/// `known_tip` is the repository commit recorded at the last check; when it is
/// unchanged the check returns immediately with no per-build requests.
pub fn start_game_data_update_check_task(output_base: PathBuf, known_tip: Option<String>) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();

    crate::util::thread::spawn_logged("check-game-data-updates", move || {
        let _ = tx.send(check_for_updates(output_base, known_tip));
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::CheckingGameDataUpdates }
}

/// Validate every cached build in `output_base` against the remote repository,
/// the source of truth. Reports missing, corrupt, or stale builds so the user
/// can re-download them.
pub fn start_game_data_validation_task(output_base: PathBuf) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    let (progress_tx, progress_rx) = mpsc::channel();

    crate::util::thread::spawn_logged("validate-game-data", move || {
        let _ = tx.send(validate(output_base, &progress_tx));
    });

    BackgroundTask {
        receiver: Some(rx),
        kind: BackgroundTaskKind::ValidatingGameData { rx: progress_rx, last_progress: None },
    }
}

fn build_client() -> Result<&'static reqwest::Client, Report> {
    crate::util::http::shared_async_client().ok_or_else(|| report!("failed to build HTTP client"))
}

fn download(
    output_base: PathBuf,
    target_build: u32,
    version_hint: Option<String>,
    force: bool,
    progress_tx: &mpsc::Sender<DownloadProgress>,
) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client()?;

    let build = runtime.block_on(wows_data_mgr::download_repo::download_build(
        client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &output_base,
        target_build,
        version_hint.as_deref(),
        force,
        |downloaded, total| {
            let _ = progress_tx.send(DownloadProgress { downloaded, total });
        },
    ))?;

    Ok(BackgroundTaskCompletion::GameDataDownloaded { requested_build: target_build, build })
}

fn plan(
    output_base: PathBuf,
    builds: Vec<(u32, Option<String>)>,
    ticket: PlanTicket,
) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client()?;
    let cas_root = wows_data_mgr::cas::cas_root(&output_base);

    let plan = runtime.block_on(wows_data_mgr::download_repo::plan_download(
        client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &cas_root,
        &builds,
    ))?;

    Ok(BackgroundTaskCompletion::GameDataDownloadPlanned { ticket, plan })
}

fn check_for_updates(output_base: PathBuf, known_tip: Option<String>) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client()?;

    let result = runtime.block_on(wows_data_mgr::download_repo::check_for_updates(
        client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &output_base,
        known_tip.as_deref(),
    ))?;

    Ok(BackgroundTaskCompletion::GameDataUpdatesChecked { tip: result.tip, updates: result.updates })
}

fn validate(
    output_base: PathBuf,
    progress_tx: &mpsc::Sender<DownloadProgress>,
) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client()?;

    let result = runtime.block_on(wows_data_mgr::download_repo::validate_cache(
        client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &output_base,
        |downloaded, total| {
            let _ = progress_tx.send(DownloadProgress { downloaded, total });
        },
    ))?;

    Ok(BackgroundTaskCompletion::GameDataValidated { tip: result.tip, builds: result.builds })
}
