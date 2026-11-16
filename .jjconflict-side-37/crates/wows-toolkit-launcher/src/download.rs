use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use wgc_catalog::update::LatestBuild;

use crate::CatalogCheckState;
use crate::DownloadState;
use crate::LauncherError;

/// Where a running download is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadPhase {
    Starting,
    FetchingManifests,
    Downloading,
    Unpacking,
}

impl std::fmt::Display for DownloadPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DownloadPhase::Starting => "Starting",
            DownloadPhase::FetchingManifests => "Fetching torrent manifests",
            DownloadPhase::Downloading => "Downloading",
            DownloadPhase::Unpacking => "Unpacking",
        })
    }
}

/// Everything a download run needs from a catalog build.
pub(crate) struct DownloadPlan {
    torrent_urls: Vec<String>,
    web_seed_base: String,
    /// Under the destination so a resumed run finds its own sha1-verified
    /// partial files rather than starting cold.
    staging_dir: PathBuf,
    install_dir: PathBuf,
    concurrency: usize,
}

impl DownloadPlan {
    pub(crate) fn of(latest: &LatestBuild, install_dir: PathBuf) -> Result<DownloadPlan, LauncherError> {
        if latest.downloads.is_empty() {
            return Err(LauncherError::NothingToDownload);
        }
        // WGUS advertises one web seed base per response, shared by every
        // part, so any part's value is the base.
        let web_seed_base =
            latest.downloads.iter().find_map(|d| d.web_seed_base.clone()).ok_or(LauncherError::NoWebSeed)?;
        // Parts can share a torrent (client and hotfix packaged together);
        // fetch each distinct torrent once.
        let mut torrent_urls: Vec<String> = Vec::new();
        for url in latest.downloads.iter().flat_map(|d| &d.torrent_urls) {
            if !torrent_urls.contains(url) {
                torrent_urls.push(url.clone());
            }
        }
        if torrent_urls.is_empty() {
            return Err(LauncherError::NoTorrentUrls);
        }
        Ok(DownloadPlan {
            torrent_urls,
            web_seed_base,
            staging_dir: install_dir.join(".wows-toolkit-download-staging"),
            install_dir,
            // WGUS's suggested concurrency, clamped so a full install's files
            // across every part saturate a fast link without an unreasonable
            // number of sockets.
            concurrency: wgc_download::download_concurrency(latest.web_seed_threads),
        })
    }
}

/// A plain client for `wgc_download`, honoring the proxy the same way
/// `wgc_catalog::CatalogClient::with_proxy` does.
pub(crate) fn http_client(proxy_url: Option<&str>) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder();
    if let Some(proxy_url) = proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
    }
    builder.build()
}

/// Read and parse `game_info_path`, then fetch and diff the newest catalog
/// chain against it. `game_info.xml` is a small file WGC writes once per
/// install, so a synchronous read on this tokio task is fine.
pub(crate) async fn check_install_catalog(game_info_path: &Path, proxy_url: Option<&str>) -> CatalogCheckState {
    let Ok(xml) = std::fs::read_to_string(game_info_path) else {
        return CatalogCheckState::NoUpdateHost;
    };
    let game_info = match wgc_catalog::game_info::parse_game_info(&xml) {
        Ok(info) => info,
        Err(e) => return CatalogCheckState::Error(e.to_string()),
    };
    let client = match wgc_catalog::CatalogClient::with_proxy(proxy_url) {
        Ok(client) => client,
        Err(e) => return CatalogCheckState::Error(e.to_string()),
    };
    match client.check_updates(&game_info).await {
        Ok(build) => CatalogCheckState::Ready(build),
        Err(e) => CatalogCheckState::Error(e.to_string()),
    }
}

/// Fetch every torrent's manifest up front, so the progress total is right
/// from the start, then download every file of every torrent together and
/// 7z-extract the FULL packages into the install dir. `control` is checked
/// between manifest fetches and inside the download and unpack calls, so a
/// multi-GB file in flight stops promptly. `progress` gets every `Running`
/// update; the final state is the return value.
///
/// `done_bytes` never decreases: it is one counter shared across every
/// torrent's files, and unpacking starts only after every download finished,
/// so the `Unpacking` phase reports the full total.
///
/// Driven by `Handle::block_on` from a blocking-pool thread, so nothing here
/// needs to be `Send`.
pub(crate) async fn run(
    client: &reqwest::Client,
    plan: &DownloadPlan,
    control: &wgc_download::DownloadControl,
    progress: impl Fn(DownloadState),
) -> DownloadState {
    progress(DownloadState::Running {
        phase: DownloadPhase::FetchingManifests,
        done_bytes: 0,
        total_bytes: 0,
        current: String::new(),
    });

    let mut torrents = Vec::with_capacity(plan.torrent_urls.len());
    for url in &plan.torrent_urls {
        if control.cancel.load(Ordering::SeqCst) {
            return DownloadState::Cancelled;
        }
        if control.pause.load(Ordering::SeqCst) {
            return DownloadState::Paused;
        }
        match wgc_download::fetch_torrent(client, url).await {
            Ok(meta) => torrents.push(meta),
            Err(e) => return DownloadState::Failed(format!("fetching torrent manifest {url}: {e}")),
        }
    }

    let total_bytes: u64 = torrents.iter().map(|t| t.total_length).sum();
    let reqs: Vec<wgc_download::InstallRequest> = torrents
        .into_iter()
        .map(|torrent| wgc_download::InstallRequest {
            staging_dir: plan.staging_dir.clone(),
            install_dir: plan.install_dir.clone(),
            torrent,
            web_seed_base: plan.web_seed_base.clone(),
            is_steam_install: false,
        })
        .collect();

    let result = wgc_download::download_and_install_all(client, &reqs, control, plan.concurrency, |update| {
        let (phase, current, done_bytes) = match &update {
            wgc_download::InstallProgress::Downloading(dp) => (
                DownloadPhase::Downloading,
                format!("{} (file {}/{})", dp.file, dp.files_done + 1, dp.files_total),
                dp.downloaded_bytes,
            ),
            wgc_download::InstallProgress::Unpacking { file }
            | wgc_download::InstallProgress::Unpacked { file, .. } => {
                (DownloadPhase::Unpacking, file.clone(), total_bytes)
            }
        };
        progress(DownloadState::Running { phase, done_bytes, total_bytes, current });
    })
    .await;

    match result {
        Ok(outcomes) => DownloadState::Done { skipped_deltas: outcomes.iter().map(|o| o.skipped_deltas.len()).sum() },
        Err(wgc_download::DownloadError::Cancelled) => DownloadState::Cancelled,
        Err(wgc_download::DownloadError::Paused) => DownloadState::Paused,
        Err(e) => DownloadState::Failed(e.to_string()),
    }
}
