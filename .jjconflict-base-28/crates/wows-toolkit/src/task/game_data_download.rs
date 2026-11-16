use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

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

/// Download game data for `requests` from the wows-replay-data repository into
/// `output_base`, one build after another so a build's missing-object set is
/// computed against what the ones before it in the selection already stored.
/// `runtime` runs the downloads and the CAS writes they spawn onto blocking
/// threads; it must be multi-threaded. When `force` is true an existing copy
/// is rebuilt to pick up newer remote data.
///
/// `follow_up` rides on the task rather than on the app so the thing to redo is
/// released with the download that asked for it, whatever the download's
/// outcome. `None` for downloads nothing is waiting on.
pub fn start_game_data_download_task(
    output_base: PathBuf,
    requests: Vec<crate::task::BuildRequest>,
    runtime: Arc<tokio::runtime::Runtime>,
    force: bool,
    follow_up: Option<GameDataFollowUp>,
    egui_ctx: egui::Context,
    proxy: Option<crate::util::proxy::ProxyConfig>,
) -> BackgroundTask {
    let (tx, rx) = crate::task::completion_channel();
    // Throttled: progress reports per downloaded chunk.
    let (progress_tx, progress_rx) =
        crate::ui_channel::throttled_channel(egui_ctx, std::time::Duration::from_millis(100));

    crate::util::thread::spawn_logged("download-game-data", move || {
        let _ = tx.send(download(output_base, requests, &runtime, force, &progress_tx, proxy.as_ref()));
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
    proxy: Option<crate::util::proxy::ProxyConfig>,
) -> BackgroundTask {
    let (tx, rx) = crate::task::completion_channel();

    crate::util::thread::spawn_logged("plan-game-data-download", move || {
        let _ = tx.send(plan(output_base, builds, ticket, proxy.as_ref()));
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::PlanningGameDataDownload { ticket } }
}

/// Check the repository for updates to builds already cached in `output_base`.
/// `known_tip` is the repository commit recorded at the last check; when it is
/// unchanged the check returns immediately with no per-build requests.
pub fn start_game_data_update_check_task(
    output_base: PathBuf,
    known_tip: Option<String>,
    proxy: Option<crate::util::proxy::ProxyConfig>,
) -> BackgroundTask {
    let (tx, rx) = crate::task::completion_channel();

    crate::util::thread::spawn_logged("check-game-data-updates", move || {
        let _ = tx.send(check_for_updates(output_base, known_tip, proxy.as_ref()));
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::CheckingGameDataUpdates }
}

/// Validate every cached build in `output_base` against the remote repository,
/// the source of truth. Reports missing, corrupt, or stale builds so the user
/// can re-download them.
pub fn start_game_data_validation_task(
    output_base: PathBuf,
    egui_ctx: egui::Context,
    proxy: Option<crate::util::proxy::ProxyConfig>,
) -> BackgroundTask {
    let (tx, rx) = crate::task::completion_channel();
    // Throttled: progress reports per validated object.
    let (progress_tx, progress_rx) =
        crate::ui_channel::throttled_channel(egui_ctx, std::time::Duration::from_millis(100));

    crate::util::thread::spawn_logged("validate-game-data", move || {
        let _ = tx.send(validate(output_base, &progress_tx, proxy.as_ref()));
    });

    BackgroundTask {
        receiver: Some(rx),
        kind: BackgroundTaskKind::ValidatingGameData { rx: progress_rx, last_progress: None },
    }
}

fn build_client(proxy: Option<&crate::util::proxy::ProxyConfig>) -> Result<&'static reqwest::Client, Report> {
    crate::util::http::shared_async_client(proxy, reqwest::redirect::Policy::default())
        .ok_or_else(|| report!("failed to build HTTP client"))
}

fn download(
    output_base: PathBuf,
    requests: Vec<crate::task::BuildRequest>,
    runtime: &tokio::runtime::Runtime,
    force: bool,
    progress_tx: &crate::ui_channel::ThrottledSender<DownloadProgress>,
    proxy: Option<&crate::util::proxy::ProxyConfig>,
) -> Result<BackgroundTaskCompletion, Report> {
    let client = build_client(proxy)?;

    let plain: Vec<(u32, Option<String>)> =
        requests.iter().map(|r| (r.build_u32(), Some(r.friendly_version()))).collect();

    let results = runtime.block_on(wows_data_mgr::download_repo::download_builds(
        client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &output_base,
        &plain,
        force,
        |downloaded, total| {
            let _ = progress_tx.send(DownloadProgress { downloaded, total });
        },
    ));

    let mut downloaded = Vec::new();
    let mut failures = Vec::new();
    for (request, result) in requests.iter().zip(results) {
        match result {
            Ok(build) => downloaded.push((request.build_u32(), build)),
            Err(e) => {
                tracing::warn!("failed to download build {}: {e}", request.build_u32());
                failures.push(request.build_u32());
            }
        }
    }

    Ok(BackgroundTaskCompletion::GameDataDownloaded { downloaded, failures })
}

fn plan(
    output_base: PathBuf,
    builds: Vec<(u32, Option<String>)>,
    ticket: PlanTicket,
    proxy: Option<&crate::util::proxy::ProxyConfig>,
) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client(proxy)?;
    let cas_root = wows_data_mgr::cas::cas_root(&output_base);

    let plan = runtime.block_on(wows_data_mgr::download_repo::plan_download(
        client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &cas_root,
        &builds,
    ))?;

    Ok(BackgroundTaskCompletion::GameDataDownloadPlanned { ticket, plan })
}

fn check_for_updates(
    output_base: PathBuf,
    known_tip: Option<String>,
    proxy: Option<&crate::util::proxy::ProxyConfig>,
) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client(proxy)?;

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
    progress_tx: &crate::ui_channel::ThrottledSender<DownloadProgress>,
    proxy: Option<&crate::util::proxy::ProxyConfig>,
) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client(proxy)?;

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
