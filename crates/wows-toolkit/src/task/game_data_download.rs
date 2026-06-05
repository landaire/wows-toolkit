use std::path::PathBuf;
use std::sync::mpsc;

use rootcause::Report;
use rootcause::prelude::*;

use super::BackgroundTask;
use super::BackgroundTaskCompletion;
use super::BackgroundTaskKind;
use super::DownloadProgress;

/// Download game data for `target_build` from the wows-replay-data repository
/// into `output_base`. `version_hint` (the replay's `major.minor.patch` string)
/// allows falling back to a different build of the same version when no exact
/// match is published. When `force` is true an existing copy is rebuilt to pick
/// up newer remote data.
pub fn start_game_data_download_task(
    output_base: PathBuf,
    target_build: u32,
    version_hint: Option<String>,
    force: bool,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    let (progress_tx, progress_rx) = mpsc::channel();

    crate::util::thread::spawn_logged("download-game-data", move || {
        let _ = tx.send(download(output_base, target_build, version_hint, force, &progress_tx));
    });

    BackgroundTask {
        receiver: Some(rx),
        kind: BackgroundTaskKind::DownloadingGameData { rx: progress_rx, last_progress: None },
    }
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

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::ValidatingGameData { rx: progress_rx, last_progress: None } }
}

fn build_client() -> Result<reqwest::Client, Report> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("wows-toolkit/", env!("CARGO_PKG_VERSION")))
        .build()
        .attach_with(|| "failed to build HTTP client")?)
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
        &client,
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

fn check_for_updates(output_base: PathBuf, known_tip: Option<String>) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client()?;

    let result = runtime.block_on(wows_data_mgr::download_repo::check_for_updates(
        &client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &output_base,
        known_tip.as_deref(),
    ))?;

    Ok(BackgroundTaskCompletion::GameDataUpdatesChecked { tip: result.tip, updates: result.updates })
}

fn validate(output_base: PathBuf, progress_tx: &mpsc::Sender<DownloadProgress>) -> Result<BackgroundTaskCompletion, Report> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .attach_with(|| "failed to create download runtime")?;
    let client = build_client()?;

    let result = runtime.block_on(wows_data_mgr::download_repo::validate_cache(
        &client,
        wows_data_mgr::download_repo::DEFAULT_REPO_BASE_URL,
        &output_base,
        |downloaded, total| {
            let _ = progress_tx.send(DownloadProgress { downloaded, total });
        },
    ))?;

    Ok(BackgroundTaskCompletion::GameDataValidated { tip: result.tip, builds: result.builds })
}
