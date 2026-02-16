use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self};
use std::time::Duration;
use std::time::Instant;

use http_body::Body;
use http_body_util::BodyExt;
use image::EncodableLayout;
use octocrab::models::repos::Asset;
use octocrab::models::repos::Release;
use octocrab::params::repos::Reference;
use reqwest::Url;
use rootcause::Report;
use rootcause::prelude::ResultExt;
use tokio::runtime::Runtime;
use tracing::debug;
use tracing::error;
use tracing::instrument;
use zip::ZipArchive;

use crate::error::ToolkitError;

use super::BackgroundTask;
use super::BackgroundTaskCompletion;
use super::BackgroundTaskKind;
use super::DownloadProgress;

/// A job that can be sent to the networking thread.
pub enum NetworkJob {
    /// Check for app updates on GitHub.
    CheckForAppUpdates,
    /// Fetch latest constants from wows-constants repo.
    FetchLatestConstants { current_commit: Option<String> },
    /// Fetch PR expected values from wows-numbers.com.
    FetchPersonalRatingData,
    /// Fetch versioned constants for a specific game build from GitHub.
    FetchVersionedConstants { build: u32 },
}

/// A result sent back from the networking thread to the UI.
pub enum NetworkResult {
    /// App update available.
    AppUpdateAvailable(Box<Release>),
    /// App is up to date.
    AppUpToDate,
    /// App update check failed.
    AppUpdateCheckFailed(String),
    /// Constants fetched successfully.
    ConstantsFetched { data: Vec<u8>, commit: Option<String> },
    /// Constants already up to date.
    ConstantsUpToDate,
    /// Constants fetch failed.
    ConstantsFetchFailed(String),
    /// PR data fetched successfully.
    PersonalRatingDataFetched(Vec<u8>),
    /// PR data fetch failed.
    PersonalRatingDataFetchFailed(String),
    /// Versioned constants fetched and saved to disk for a specific build.
    VersionedConstantsFetched { build: u32 },
    /// Versioned constants fetch failed.
    VersionedConstantsFetchFailed { build: u32, msg: String },
}

/// State for the background networking thread.
struct NetworkingThread {
    job_rx: mpsc::Receiver<NetworkJob>,
    result_tx: mpsc::Sender<NetworkResult>,
    runtime: Runtime,
    last_constants_check: Option<Instant>,
}

/// Start the background networking thread.
///
/// Returns the sender for submitting jobs and the receiver for collecting results.
pub fn start_networking_thread() -> (mpsc::Sender<NetworkJob>, mpsc::Receiver<NetworkResult>) {
    let (job_tx, job_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();

    std::thread::Builder::new()
        .name("networking".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    error!("Failed to create tokio runtime for networking thread: {:?}", e);
                    return;
                }
            };

            let mut thread = NetworkingThread { job_rx, result_tx, runtime, last_constants_check: None };

            thread.run();
        })
        .expect("failed to spawn networking thread");

    (job_tx, result_rx)
}

impl NetworkingThread {
    fn run(&mut self) {
        debug!("Networking thread started");

        loop {
            // Wait for a job, with a timeout for periodic checks
            match self.job_rx.recv_timeout(Duration::from_secs(60)) {
                Ok(job) => self.handle_job(job),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Periodic check: if constants were requested but throttled,
                    // we could re-attempt here. For now, the UI drives retries
                    // by sending new FetchLatestConstants jobs.
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    debug!("Networking thread: job channel disconnected, exiting");
                    break;
                }
            }
        }
    }

    fn handle_job(&mut self, job: NetworkJob) {
        match job {
            NetworkJob::CheckForAppUpdates => self.check_for_app_updates(),
            NetworkJob::FetchLatestConstants { current_commit } => {
                self.fetch_latest_constants(current_commit);
            }
            NetworkJob::FetchPersonalRatingData => self.fetch_personal_rating_data(),
            NetworkJob::FetchVersionedConstants { build } => self.fetch_versioned_constants(build),
        }
    }

    #[instrument(skip(self))]
    fn check_for_app_updates(&mut self) {
        let result = self
            .runtime
            .block_on(async { octocrab::instance().repos("landaire", "wows-toolkit").releases().get_latest().await });

        match result {
            Ok(latest_release) => match semver::Version::parse(&latest_release.tag_name[1..]) {
                Ok(version) => {
                    let app_version = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
                    if app_version < version {
                        let _ = self.result_tx.send(NetworkResult::AppUpdateAvailable(Box::new(latest_release)));
                    } else {
                        let _ = self.result_tx.send(NetworkResult::AppUpToDate);
                    }
                }
                Err(e) => {
                    let _ = self.result_tx.send(NetworkResult::AppUpdateCheckFailed(format!(
                        "failed to parse release version '{}': {e}",
                        latest_release.tag_name
                    )));
                }
            },
            Err(e) => {
                let _ = self
                    .result_tx
                    .send(NetworkResult::AppUpdateCheckFailed(format!("failed to check GitHub releases: {e}")));
            }
        }
    }

    #[instrument(skip(self))]
    fn fetch_latest_constants(&mut self, current_commit: Option<String>) {
        // Throttle: don't check more often than every 30 minutes
        let now = Instant::now();
        if let Some(last_check) = self.last_constants_check
            && now.duration_since(last_check).as_secs() < 30 * 60
        {
            debug!("Constants check throttled");
            return;
        }
        self.last_constants_check = Some(now);
        let result = self.runtime.block_on(async {
            let octocrab = octocrab::instance();

            let latest_commit = octocrab
                .repos("padtrack", "wows-constants")
                .list_commits()
                .per_page(1)
                .send()
                .await
                .ok()
                .and_then(|mut list| list.take_items().pop())
                .map(|commit| commit.sha);

            if current_commit == latest_commit || latest_commit.is_none() {
                return Ok(None);
            }

            match octocrab
                .repos("padtrack", "wows-constants")
                .raw_file(Reference::Branch("main".to_string()), "data/latest.json")
                .await
            {
                Ok(response) => {
                    let mut body = response.into_body();
                    let mut data = Vec::with_capacity(body.size_hint().exact().unwrap_or_default() as usize);

                    while let Some(frame) = body.frame().await {
                        match frame {
                            Ok(frame) => {
                                if let Some(chunk) = frame.data_ref() {
                                    data.extend_from_slice(chunk);
                                }
                            }
                            Err(e) => return Err(format!("failed to read constants response body: {e}")),
                        }
                    }

                    Ok(Some((data, latest_commit)))
                }
                Err(e) => Err(format!("failed to fetch constants from GitHub: {e}")),
            }
        });

        match result {
            Ok(Some((data, commit))) => {
                let _ = self.result_tx.send(NetworkResult::ConstantsFetched { data, commit });
            }
            Ok(None) => {
                let _ = self.result_tx.send(NetworkResult::ConstantsUpToDate);
            }
            Err(msg) => {
                let _ = self.result_tx.send(NetworkResult::ConstantsFetchFailed(msg));
            }
        }
    }

    #[instrument(skip(self))]
    fn fetch_personal_rating_data(&mut self) {
        let result = self.runtime.block_on(crate::personal_rating::fetch_expected_values());

        match result {
            Ok(data) => {
                let _ = self.result_tx.send(NetworkResult::PersonalRatingDataFetched(data));
            }
            Err(e) => {
                let _ = self
                    .result_tx
                    .send(NetworkResult::PersonalRatingDataFetchFailed(format!("failed to fetch PR data: {e}")));
            }
        }
    }

    #[instrument(skip(self))]
    fn fetch_versioned_constants(&mut self, target_build: u32) {
        // If already cached on disk, no need to download
        if load_versioned_constants_from_disk(target_build).is_some() {
            debug!("already on disk, skipping fetch");
            let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetched { build: target_build });
            return;
        }

        // List available builds from GitHub
        let available_builds = match self.list_available_constants_builds() {
            Some(builds) => builds,
            None => {
                let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetchFailed {
                    build: target_build,
                    msg: "Failed to list available builds from GitHub".into(),
                });
                return;
            }
        };

        // Try exact build first
        if available_builds.contains(&target_build)
            && let Some(data) = self.fetch_constants_for_build(target_build)
        {
            save_versioned_constants(target_build, &data);
            debug!("fetched exact match from GitHub");
            let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetched { build: target_build });
            return;
        }

        // Walk down to the nearest previous build
        for &available_build in available_builds.iter().rev() {
            if available_build >= target_build {
                continue;
            }
            // Check disk for this fallback
            if let Some(data) = load_versioned_constants_from_disk(available_build) {
                debug!(available_build, "using cached fallback from disk");
                save_versioned_constants(target_build, &data);
                let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetched { build: target_build });
                return;
            }
            // Fetch from GitHub
            if let Some(data) = self.fetch_constants_for_build(available_build) {
                debug!(available_build, "fetched fallback from GitHub");
                save_versioned_constants(available_build, &data);
                save_versioned_constants(target_build, &data);
                let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetched { build: target_build });
                return;
            }
        }

        let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetchFailed {
            build: target_build,
            msg: "No matching build found on GitHub".into(),
        });
    }

    /// List available versioned constants builds from GitHub (data/versions/ directory).
    fn list_available_constants_builds(&self) -> Option<Vec<u32>> {
        self.runtime.block_on(async {
            let items = octocrab::instance()
                .repos("padtrack", "wows-constants")
                .get_content()
                .path("data/versions")
                .r#ref("main")
                .send()
                .await
                .ok()?;
            let mut builds: Vec<u32> =
                items.items.iter().filter_map(|item| item.name.strip_suffix(".json")?.parse::<u32>().ok()).collect();
            builds.sort();
            Some(builds)
        })
    }

    /// Fetch a specific build's constants JSON from GitHub.
    fn fetch_constants_for_build(&self, build: u32) -> Option<serde_json::Value> {
        self.runtime.block_on(async {
            let path = format!("data/versions/{build}.json");
            let response = octocrab::instance()
                .repos("padtrack", "wows-constants")
                .raw_file(Reference::Branch("main".to_string()), &path)
                .await
                .ok()?;

            let mut body = response.into_body();
            let mut result = Vec::with_capacity(body.size_hint().exact().unwrap_or_default() as usize);

            while let Some(frame) = body.frame().await {
                match frame {
                    Ok(frame) => {
                        if let Some(data) = frame.data_ref() {
                            result.extend_from_slice(data);
                        }
                    }
                    Err(_) => return None,
                }
            }

            serde_json::from_slice(&result).ok()
        })
    }
}

/// Save versioned constants to `constants_{build}.json` on disk.
#[instrument(skip(data))]
fn save_versioned_constants(build: u32, data: &serde_json::Value) {
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        let filename = format!("constants_{build}.json");
        let path = storage_dir.join(filename);
        if let Ok(bytes) = serde_json::to_vec(data) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

// --- Versioned constants (used by replay loading, runs in background threads) ---

/// Try to load versioned constants from `constants_{build}.json` on disk.
#[instrument]
pub(crate) fn load_versioned_constants_from_disk(build: u32) -> Option<serde_json::Value> {
    let filename = format!("constants_{build}.json");
    let storage_dir = eframe::storage_dir(crate::APP_NAME)?;
    let path = storage_dir.join(filename);
    if path.exists() {
        let data = std::fs::read(&path).ok()?;
        serde_json::from_slice(&data).ok()
    } else {
        None
    }
}

/// Check disk cache only for versioned constants. Does NOT perform any network I/O.
/// Used during initial game data loading to avoid blocking on network calls.
///
/// Returns `(constants_data, is_exact_match)` if found on disk, or None.
#[instrument]
pub fn load_versioned_constants_from_disk_with_fallback(target_build: u32) -> Option<(serde_json::Value, bool)> {
    if let Some(data) = load_versioned_constants_from_disk(target_build) {
        debug!("exact match found on disk");
        return Some((data, true));
    }

    debug!("no cached constants on disk");
    None
}

// --- Download update task (stays here, already async with progress) ---

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
async fn download_update(tx: mpsc::Sender<DownloadProgress>, file: Url) -> Result<PathBuf, Report> {
    let mut body = reqwest::get(file)
        .await
        .context("failed to get HTTP response for update file")?
        .error_for_status()
        .context("HTTP error status for update file")?;

    let total = body.content_length().expect("body has no content-length");
    let mut downloaded = 0;

    const NEW_FILE_NAME: &str = "wows_toolkit.tmp.exe";
    let new_exe_path = std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.parent()?.join(NEW_FILE_NAME)))
        .unwrap_or_else(|| PathBuf::from(NEW_FILE_NAME));

    // We're going to be blocking here on I/O but it shouldn't matter since this
    // application doesn't really use async
    let mut zip_data = Vec::new();

    while let Some(chunk) = body.chunk().await.context("failed to get update body chunk")? {
        downloaded += chunk.len();
        let _ = tx.send(DownloadProgress { downloaded: downloaded as u64, total });

        zip_data.extend_from_slice(chunk.as_bytes());
    }

    let cursor = Cursor::new(zip_data.as_slice());

    let mut zip = ZipArchive::new(cursor).context("failed to create ZipArchive reader")?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).context("failed to get zip inner file by index")?;
        if file.name().ends_with(".exe") {
            let mut out_file = std::fs::File::create(&new_exe_path)
                .context("failed to create update tmp file")
                .attach_with(|| format!("{new_exe_path:?}"))?;
            std::io::copy(&mut file, &mut out_file).context("failed to decompress update file to disk")?;
            break;
        }
    }

    Ok(new_exe_path)
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn start_download_update_task(runtime: &Runtime, release: &Asset) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();

    let (progress_tx, progress_rx) = mpsc::channel();
    let url = release.browser_download_url.clone();

    runtime.spawn(async move {
        let result = download_update(progress_tx, url).await.map(BackgroundTaskCompletion::UpdateDownloaded);

        let _ = tx.send(result);
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::Updating { rx: progress_rx, last_progress: None } }
}

// --- Constants/PR loading tasks (deserialize JSON in background thread) ---

pub fn load_constants(constants: Vec<u8>) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result: Result<BackgroundTaskCompletion, Report> = serde_json::from_slice(&constants)
            .map(BackgroundTaskCompletion::ConstantsLoaded)
            .map_err(|err| Report::from(ToolkitError::from(err)));

        tx.send(result).expect("tx closed");
    });
    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingConstants }
}

pub fn load_personal_rating_data(data: Vec<u8>) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result: Result<BackgroundTaskCompletion, Report> = serde_json::from_slice(&data)
            .map(BackgroundTaskCompletion::PersonalRatingDataLoaded)
            .map_err(|err| Report::from(ToolkitError::from(err)));

        tx.send(result).expect("tx closed");
    });
    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingPersonalRatingData }
}

// --- Twitch task ---

use crate::twitch::Token;
use crate::twitch::TwitchState;
use crate::twitch::TwitchUpdate;
use crate::twitch::{self};
use jiff::Timestamp;
use parking_lot::RwLock;
use twitch_api::twitch_oauth2::AccessToken;
use twitch_api::twitch_oauth2::UserToken;

async fn update_twitch_token(twitch_state: &RwLock<TwitchState>, token: &Token) {
    let client = twitch_state.read().client().clone();
    match UserToken::from_token(&client, AccessToken::from(token.oauth_token())).await {
        Ok(token) => {
            let mut state = twitch_state.write();
            state.token = Some(token);
            state.token_validation_failed = false;
        }
        Err(_e) => {
            let mut state = twitch_state.write();
            state.token_validation_failed = true;
        }
    }
}

pub fn start_twitch_task(
    runtime: &Runtime,
    twitch_state: Arc<RwLock<TwitchState>>,
    monitored_channel: String,
    token: Option<Token>,
    mut token_rx: tokio::sync::mpsc::Receiver<TwitchUpdate>,
) {
    runtime.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60 * 2));

        // Set the initial twitch token
        if let Some(token) = token {
            update_twitch_token(&twitch_state, &token).await;
        }

        let (client, token) = {
            let state = twitch_state.read();
            (state.client().clone(), state.token.clone())
        };
        let mut monitored_user_id = token.as_ref().map(|token| token.user_id.clone());
        if !monitored_channel.is_empty()
            && let Some(token) = token
            && let Ok(Some(user)) = client.get_user_from_login(&monitored_channel, &token).await
        {
            monitored_user_id = Some(user.id)
        }

        loop {
            let token_receive = token_rx.recv();

            tokio::select! {
                // Every 2 minutes we attempt to get the participants list
                _ = interval.tick() => {
                    let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                    if let Some(token) = token
                        && let Some(monitored_user) = &monitored_user_id
                            && let Ok(chatters) = twitch::fetch_chatters(&client, monitored_user, &token).await {
                                let now = Timestamp::now();
                                let mut state = twitch_state.write();
                                for chatter in chatters {
                                    state.participants.entry(chatter).or_default().insert(now);
                                }
                            }
                }

                update = token_receive => {
                    if let Some(update) = update {
                        match update {
                            TwitchUpdate::Token(token) => {
                                let had_previous_token = { twitch_state.read().token_is_valid() };
                                update_twitch_token(&twitch_state, &token).await;

                                let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                                if let Some(token) = &token
                                    && let Some(monitored_user) = &monitored_user_id
                                        && let Ok(chatters) = twitch::fetch_chatters(&client, monitored_user, token).await {
                                            let now = Timestamp::now();
                                            let mut state = twitch_state.write();
                                            for chatter in chatters {
                                                state.participants.entry(chatter).or_default().insert(now);
                                            }
                                        }

                                if !had_previous_token {
                                    // If we didn't have a previous token, but we did have a username to watch, update the username
                                    monitored_user_id = token.as_ref().map(|token| token.user_id.clone());
                                    if !monitored_channel.is_empty()
                                        && let Some(token) = token
                                            && let Ok(Some(user)) = client.get_user_from_login(&monitored_channel, &token).await {
                                                monitored_user_id = Some(user.id)
                                            }
                                }
                            },
                            TwitchUpdate::User(user_name) => {
                                let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                                if let Some(token) = token
                                    && let Ok(Some(user)) = client.get_user_from_login(&user_name, &token).await {
                                        monitored_user_id = Some(user.id);
                                    }
                            },
                        }
                    }
                }
            }

            // Do a period cleanup of old viewers
            let mut state = twitch_state.write();
            let now = Timestamp::now();
            for timestamps in state.participants.values_mut() {
                // Retain only timestamps within the last 30 minutes
                timestamps.retain(|ts| *ts > (now - Duration::from_secs(60 * 30)));
            }
        }
    });
}
