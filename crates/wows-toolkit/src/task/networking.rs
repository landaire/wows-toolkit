use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use image::EncodableLayout;
use octocrab::models::repos::Asset;
use octocrab::models::repos::Release;
use reqwest::Url;
use rootcause::Report;
use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use rootcause::prelude::ResultExt;
use tokio::runtime::Runtime;
use tower::Layer;
use tracing::debug;
use tracing::error;
use tracing::instrument;
use zip::ZipArchive;

use crate::util::error::ToolkitError;
use crate::util::proxy::ProxyConfig;

use super::BackgroundTask;
use super::BackgroundTaskCompletion;
use super::BackgroundTaskKind;
use super::DownloadProgress;

/// Connect and read timeouts for the global GitHub client, applied whether or
/// not a proxy is configured.
const GITHUB_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const GITHUB_READ_TIMEOUT: Duration = Duration::from_secs(30);

const GITHUB_BASE_URI: &str = "https://api.github.com";

/// Errors from assembling the proxy-aware GitHub client. The caller falls
/// back to a direct connection on any of these, so this only needs to carry
/// enough detail for the warn! log.
#[derive(Debug, thiserror::Error)]
enum ProxiedOctocrabClientError {
    #[error("proxy URL is not a valid URI: {0}")]
    InvalidProxyUri(http::uri::InvalidUri),
    #[error("failed to build the rustls native-roots HTTPS connector: {0}")]
    HttpsConnector(std::io::Error),
    #[error("failed to build the proxy connector: {0}")]
    ProxyConnector(std::io::Error),
}

/// Strips a `[...]` IPv6-literal wrapper if present. `hyper_http_proxy`'s
/// `Intercept` calls back with `http::Uri::host()`, which keeps the brackets
/// for an IPv6 host (`http` crate's `authority.rs`); the bypass entries this
/// app produces never have them (`parse_bypass`'s `<local>` expansion pushes
/// bare `"::1"`), so leaving them in place would make every IPv6 bypass entry
/// silently never match.
fn strip_ipv6_brackets(host: &str) -> &str {
    host.strip_prefix('[').and_then(|host| host.strip_suffix(']')).unwrap_or(host)
}

/// Matches `host` the way the Windows proxy dialog and `NO_PROXY` entries do:
/// a leading or trailing `*` wildcard (`*.corp.example`, `10.*`); otherwise a
/// bare pattern (`localhost`, `127.0.0.1`, `::1`, or a domain from `NO_PROXY`)
/// matches itself and any subdomain, mirroring `reqwest::NoProxy`'s
/// documented handling of the same `ProxyConfig.bypass` entries for this
/// app's other clients ("a domain name... would match both that domain AND
/// all subdomains"). A pattern with wildcards on both ends is not supported;
/// none of `util::proxy`'s bypass sources produce one.
fn host_matches_bypass_pattern(pattern: &str, host: &str) -> bool {
    let pattern = strip_ipv6_brackets(pattern).to_ascii_lowercase();
    let host = strip_ipv6_brackets(host).to_ascii_lowercase();
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(suffix), _) => host.ends_with(suffix),
        (None, Some(prefix)) => host.starts_with(prefix),
        (None, None) => host == pattern || host.ends_with(&format!(".{pattern}")),
    }
}

/// Builds the global GitHub client with no proxy: octocrab's own default
/// build (feature-rich; see `build_proxied_octocrab_client`'s doc comment),
/// just with connect/read timeouts. This is the unchanged, pre-proxy-support
/// path, kept as its own function so it is reused verbatim as the fallback
/// when a configured proxy cannot be honored.
fn build_direct_octocrab_client() -> Result<octocrab::Octocrab, octocrab::Error> {
    octocrab::Octocrab::builder()
        .set_connect_timeout(Some(GITHUB_CONNECT_TIMEOUT))
        .set_read_timeout(Some(GITHUB_READ_TIMEOUT))
        .build()
}

/// Builds the global GitHub client dialing through `config`'s proxy.
///
/// octocrab 0.49.9's feature-rich `build()` (base URI resolution, the
/// mandatory GitHub User-Agent header, the default retry policy; lib.rs:729)
/// is only reachable from its `NoSvc` builder state. Calling `with_service`
/// (lib.rs:473) moves to a builder state whose `build()` (lib.rs:563) only
/// boxes the response body and constructs `Octocrab::new` from the supplied
/// service directly -- none of those defaults apply there. They are
/// re-applied below as explicit tower layers, in the same relative order
/// octocrab's own `build()` applies them (retry closest to the transport,
/// then the User-Agent header, then base-URI resolution outermost so it can
/// rewrite the relative paths octocrab's request builders produce before
/// anything else sees the request).
///
/// The inner connector is the same rustls native-roots HTTPS connector
/// octocrab's default build uses (lib.rs:734-749), so a bypassed host still
/// gets TLS: `ProxyConnector` only applies its own TLS for the CONNECT-tunnel
/// path, and calls the inner connector directly for hosts its intercept
/// excludes.
fn build_proxied_octocrab_client(config: &ProxyConfig) -> Result<octocrab::Octocrab, ProxiedOctocrabClientError> {
    let proxy_uri: http::Uri = config.url.parse().map_err(ProxiedOctocrabClientError::InvalidProxyUri)?;

    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(ProxiedOctocrabClientError::HttpsConnector)?
        .https_or_http()
        .enable_http1()
        .build();

    let bypass = config.bypass.clone();
    let intercept: hyper_http_proxy::Intercept =
        (move |_scheme: Option<&str>, host: Option<&str>, _port: Option<u16>| {
            host.is_none_or(|host| !bypass.iter().any(|pattern| host_matches_bypass_pattern(pattern, host)))
        })
        .into();

    // `Proxy::new` picks up userinfo embedded in `proxy_uri`
    // (`http://user:pass@proxy:8080`) and sets it as proxy Basic auth.
    let proxy = hyper_http_proxy::Proxy::new(intercept, proxy_uri);
    let proxy_connector = hyper_http_proxy::ProxyConnector::from_proxy(https, proxy)
        .map_err(ProxiedOctocrabClientError::ProxyConnector)?;

    // Applied to the underlying connector, not through the builder's
    // `set_connect_timeout`/`set_read_timeout`: those configure the
    // `NoSvc`-state build only and do not apply once a service is supplied.
    let mut timeout_connector = hyper_timeout::TimeoutConnector::new(proxy_connector);
    timeout_connector.set_connect_timeout(Some(GITHUB_CONNECT_TIMEOUT));
    timeout_connector.set_read_timeout(Some(GITHUB_READ_TIMEOUT));

    let client: hyper_util::client::legacy::Client<_, octocrab::OctoBody> =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build(timeout_connector);

    // octocrab's own default retry policy (`RetryConfig::Simple(3)`,
    // lib.rs:932). Applied directly to the raw legacy client, where octocrab
    // itself applies it: `RetryConfig`'s `Policy` impl is keyed to
    // `hyper_util::client::legacy::Error`, which later layers replace.
    let client =
        tower::retry::RetryLayer::new(octocrab::service::middleware::retry::RetryConfig::Simple(3)).layer(client);

    // octocrab enables this by default (lib.rs:815-816, `follow-redirect`
    // feature). `raw_file` (used by `fetch_latest_constants`) does not
    // compensate for its absence: it calls `execute` directly rather than
    // routing through `follow_location_to_data` the way `download_tarball`
    // does, so a 3xx here would otherwise drain an empty body and surface as
    // a confusing JSON parse error instead of a followed redirect.
    let client = tower_http::follow_redirect::FollowRedirectLayer::new().layer(client);

    // GitHub rejects requests with no User-Agent header; this is the same
    // value and the same layer octocrab's default build applies (lib.rs:821).
    let user_agent_headers = Arc::new(vec![(http::header::USER_AGENT, http::HeaderValue::from_static("octocrab"))]);
    let client = octocrab::service::middleware::extra_headers::ExtraHeadersLayer::new(user_agent_headers).layer(client);

    // octocrab's request builders (e.g. `repos(...).releases().get_latest()`)
    // only ever produce relative paths; without this layer they are not
    // valid request targets for a bare hyper client.
    let base_uri: http::Uri = GITHUB_BASE_URI.parse().expect("GITHUB_BASE_URI is a valid URI");
    let client = octocrab::service::middleware::base_uri::BaseUriLayer::new(base_uri).layer(client);

    // Known gap versus octocrab's default build, accepted for now: no
    // `AuthHeaderLayer`. This app never authenticates to GitHub today
    // (`AuthState::None`, no `.personal_token()`/`.oauth()`/etc. call
    // anywhere); if that changes, it needs adding here too.
    octocrab::OctocrabBuilder::new_empty()
        .with_service(client)
        .with_auth(octocrab::AuthState::None)
        .build()
        .map_err(|infallible: std::convert::Infallible| match infallible {})
}

/// A job that can be sent to the networking thread.
pub enum NetworkJob {
    /// Check for app updates on GitHub.
    CheckForAppUpdates,
    /// Fetch latest constants from wows-constants repo.
    FetchLatestConstants { current_commit: Option<String> },
    /// Fetch PR expected values from wows-numbers.com.
    FetchPersonalRatingData,
    /// Fetch versioned constants for a specific game build from GitHub.
    /// `version` is the replay's friendly version (major.minor.patch) used to
    /// resolve constants across regions when the build number differs.
    FetchVersionedConstants { build: u32, version: Option<String> },
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
    /// Versioned constants are on disk for a specific build, either just
    /// downloaded or already cached there.
    VersionedConstantsFetched { build: u32, source: VersionedConstantsSource },
    /// Versioned constants fetch failed.
    VersionedConstantsFetchFailed { build: u32, msg: String },
}

/// Where a build's versioned constants came from when a fetch job completed.
/// Only `Downloaded` means the file's content changed on this run; a rebuild
/// is wasted work otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionedConstantsSource {
    Downloaded,
    AlreadyOnDisk,
}

/// State for the background networking thread.
struct NetworkingThread {
    job_rx: mpsc::Receiver<NetworkJob>,
    result_tx: egui_inbox::UiInboxSender<NetworkResult>,
    runtime: Runtime,
    last_constants_check: Option<Instant>,
    proxy: Option<crate::util::proxy::ProxyConfig>,
}

/// Start the background networking thread.
///
/// Returns the sender for submitting jobs and the inbox results arrive on.
/// `proxy` is the value resolved once at startup; this thread outlives the
/// job loop, so it is captured here rather than re-resolved per call.
pub fn start_networking_thread(
    proxy: Option<crate::util::proxy::ProxyConfig>,
) -> (mpsc::Sender<NetworkJob>, egui_inbox::UiInbox<NetworkResult>) {
    let (job_tx, job_rx) = mpsc::channel();
    let (result_tx, result_rx) = egui_inbox::UiInbox::channel();

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

            let mut thread = NetworkingThread { job_rx, result_tx, runtime, last_constants_check: None, proxy };

            thread.run();
        })
        .expect("failed to spawn networking thread");

    (job_tx, result_rx)
}

impl NetworkingThread {
    fn run(&mut self) {
        debug!("Networking thread started");

        // Configure the global GitHub client (app-update and constants checks) with
        // connect/read timeouts so a stalled network fails fast instead of hanging,
        // and route it through the resolved proxy when one is configured. Built
        // inside the runtime because octocrab's tower Buffer spawns a worker task;
        // it keeps octocrab's default transient-failure retry either way.
        self.runtime.block_on(async {
            let client = match &self.proxy {
                Some(config) => match build_proxied_octocrab_client(config) {
                    Ok(client) => {
                        debug!("GitHub client is proxied through {}", config.redacted_url());
                        Ok(client)
                    }
                    Err(e) => {
                        tracing::warn!(
                            "failed to build proxied GitHub client, falling back to a direct connection: {}",
                            crate::util::http::error_chain(&e)
                        );
                        build_direct_octocrab_client()
                    }
                },
                None => {
                    debug!("GitHub client is not proxied");
                    build_direct_octocrab_client()
                }
            };

            match client {
                Ok(client) => {
                    octocrab::initialise(client);
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to configure GitHub client timeouts: {}",
                        crate::util::http::error_chain(&e)
                    );
                }
            }
        });

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
            NetworkJob::FetchVersionedConstants { build, version } => self.fetch_versioned_constants(build, version),
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
                let chain = crate::util::http::error_chain(&e);
                tracing::warn!("failed to check GitHub releases: {chain}");
                let _ = self
                    .result_tx
                    .send(NetworkResult::AppUpdateCheckFailed(format!("failed to check GitHub releases: {chain}")));
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
        // The fetch itself is shared with the GPUI port
        // (`wows_data_mgr::constants::fetch_latest_constants`); what stays here is
        // the throttle and the reply channel.
        let result = self
            .runtime
            .block_on(wows_data_mgr::constants::fetch_latest_constants(current_commit.as_deref()))
            .map(|latest| latest.map(|latest| (latest.data, latest.commit)))
            .map_err(|err| format!("failed to fetch constants from GitHub: {err}"));

        match result {
            Ok(Some((data, commit))) => {
                let _ = self.result_tx.send(NetworkResult::ConstantsFetched { data, commit });
            }
            Ok(None) => {
                let _ = self.result_tx.send(NetworkResult::ConstantsUpToDate);
            }
            Err(msg) => {
                tracing::warn!("{msg}");
                let _ = self.result_tx.send(NetworkResult::ConstantsFetchFailed(msg));
            }
        }
    }

    #[instrument(skip(self))]
    fn fetch_personal_rating_data(&mut self) {
        let result = self.runtime.block_on(crate::util::personal_rating::fetch_expected_values(self.proxy.as_ref()));

        match result {
            Ok(data) => {
                let _ = self.result_tx.send(NetworkResult::PersonalRatingDataFetched(data));
            }
            Err(e) => {
                let chain = crate::util::http::error_chain(&e);
                tracing::warn!("failed to fetch PR data: {chain}");
                let _ = self
                    .result_tx
                    .send(NetworkResult::PersonalRatingDataFetchFailed(format!("failed to fetch PR data: {chain}")));
            }
        }
    }

    #[instrument(skip(self))]
    fn fetch_versioned_constants(&mut self, target_build: u32, version: Option<String>) {
        // If already cached on disk, no need to download
        if load_versioned_constants_from_disk(target_build).is_some() {
            debug!("already on disk, skipping fetch");
            let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetched {
                build: target_build,
                source: VersionedConstantsSource::AlreadyOnDisk,
            });
            return;
        }

        // Delegate to the shared constants module in wows-data-mgr
        match self
            .runtime
            .block_on(wows_data_mgr::constants::fetch_versioned_constants(target_build, version.as_deref()))
        {
            Ok((data, actual_build)) => {
                // Cache under the replay's build so later lookups by target_build hit,
                // even when constants were resolved from a different (e.g. cross-region) build.
                save_versioned_constants(target_build, &data);
                if actual_build != target_build {
                    debug!(actual_build, "fetched fallback from GitHub");
                } else {
                    debug!("fetched exact match from GitHub");
                }
                let _ = self.result_tx.send(NetworkResult::VersionedConstantsFetched {
                    build: target_build,
                    source: VersionedConstantsSource::Downloaded,
                });
            }
            Err(e) => {
                // rootcause::Report doesn't implement std::error::Error, so error_chain
                // doesn't apply here. ASCII formatting keeps the log file, which users
                // paste into bug reports, free of Unicode box-drawing characters.
                let formatted = e.format_with(&DefaultReportFormatter::ASCII);
                tracing::warn!(build = target_build, "failed to fetch versioned constants: {formatted:?}");
                let _ = self
                    .result_tx
                    .send(NetworkResult::VersionedConstantsFetchFailed { build: target_build, msg: format!("{e}") });
            }
        }
    }
}

/// Save versioned constants to `constants_{build}.json` on disk.
#[instrument(skip(data))]
fn save_versioned_constants(build: u32, data: &serde_json::Value) {
    if let Some(storage_dir) = crate::storage_dir() {
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
    let storage_dir = crate::storage_dir()?;
    let path = storage_dir.join(filename);
    if path.exists() {
        let data = std::fs::read(&path).ok()?;
        serde_json::from_slice(&data).ok()
    } else {
        None
    }
}

// --- Download update task (stays here, already async with progress) ---

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
async fn download_update(
    tx: crate::ui_channel::ThrottledSender<DownloadProgress>,
    file: Url,
    proxy: Option<&crate::util::proxy::ProxyConfig>,
) -> Result<PathBuf, Report> {
    let client = crate::util::http::async_client(proxy, reqwest::redirect::Policy::default())
        .context("failed to build HTTP client for update download")?;
    let mut body = client
        .get(file)
        .send()
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

    let exe_dir = new_exe_path.parent().unwrap_or_else(|| Path::new("."));

    let mut zip = ZipArchive::new(cursor).context("failed to create ZipArchive reader")?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).context("failed to get zip inner file by index")?;
        if file.name().ends_with(".exe") {
            let mut out_file = std::fs::File::create(&new_exe_path)
                .context("failed to create update tmp file")
                .attach_with(|| format!("{new_exe_path:?}"))?;
            std::io::copy(&mut file, &mut out_file).context("failed to decompress update file to disk")?;
        } else if file.name().ends_with(".pdb") {
            let pdb_path = exe_dir.join("wows_toolkit.pdb");
            if let Ok(mut out_file) = std::fs::File::create(&pdb_path) {
                let _ = std::io::copy(&mut file, &mut out_file);
            }
        }
    }

    Ok(new_exe_path)
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn start_download_update_task(
    runtime: &Runtime,
    release: &Asset,
    egui_ctx: egui::Context,
    proxy: Option<crate::util::proxy::ProxyConfig>,
) -> BackgroundTask {
    let (tx, rx) = crate::task::completion_channel();

    // Throttled: progress reports per downloaded chunk.
    let (progress_tx, progress_rx) =
        crate::ui_channel::throttled_channel(egui_ctx, std::time::Duration::from_millis(100));
    let url = release.browser_download_url.clone();

    runtime.spawn(async move {
        let result =
            download_update(progress_tx, url, proxy.as_ref()).await.map(BackgroundTaskCompletion::UpdateDownloaded);

        if let Err(report) = &result {
            // ASCII formatting keeps the log file, which users paste into bug
            // reports, free of Unicode box-drawing characters.
            let formatted = report.format_with(&DefaultReportFormatter::ASCII);
            tracing::warn!("update download failed: {formatted:?}");
        }

        let _ = tx.send(result);
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::Updating { rx: progress_rx, last_progress: None } }
}

// --- Constants/PR loading tasks (deserialize JSON in background thread) ---

pub fn load_personal_rating_data(data: Vec<u8>) -> BackgroundTask {
    let (tx, rx) = crate::task::completion_channel();
    crate::util::thread::spawn_logged("load-personal-rating", move || {
        let result: Result<BackgroundTaskCompletion, Report> = serde_json::from_slice(&data)
            .map(BackgroundTaskCompletion::PersonalRatingDataLoaded)
            .map_err(|err| Report::from(ToolkitError::from(err)));

        tx.send(result).expect("tx closed");
    });
    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingPersonalRatingData }
}

// --- Twitch task ---

use crate::twitch;
use crate::twitch::Token;
use crate::twitch::TwitchState;
use crate::twitch::TwitchUpdate;
use jiff::Timestamp;
use parking_lot::RwLock;
use twitch_api::twitch_oauth2::AccessToken;
use twitch_api::twitch_oauth2::UserToken;

/// How long a persisted Twitch chat observation is retained before pruning.
const TWITCH_OBSERVATION_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;

/// Persists `chatters` as Twitch observations at `now`, then prunes
/// observations older than the retention window. Best-effort: a DB hiccup is
/// logged and never allowed to interrupt the poll loop.
async fn persist_twitch_observations(pool: &sqlx::SqlitePool, chatters: &[String], now: Timestamp) {
    let seen_at = now.as_second();
    let observations: Vec<(String, i64)> = chatters.iter().map(|login| (login.clone(), seen_at)).collect();
    if let Err(e) = crate::db::index::query::record_twitch_observations(pool, &observations).await {
        tracing::warn!("failed to persist twitch observations: {e}");
    }
    if let Err(e) =
        crate::db::index::query::prune_twitch_observations(pool, seen_at - TWITCH_OBSERVATION_RETENTION_SECS).await
    {
        tracing::warn!("failed to prune twitch observations: {e}");
    }
}

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
    db_pool: Option<sqlx::SqlitePool>,
    egui_ctx: egui::Context,
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
                                {
                                    let mut state = twitch_state.write();
                                    for chatter in &chatters {
                                        state.participants.entry(chatter.clone()).or_default().insert(now);
                                    }
                                }
                                egui_ctx.request_repaint();
                                if let Some(pool) = &db_pool {
                                    persist_twitch_observations(pool, &chatters, now).await;
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
                                            {
                                                let mut state = twitch_state.write();
                                                for chatter in &chatters {
                                                    state.participants.entry(chatter.clone()).or_default().insert(now);
                                                }
                                            }
                                            egui_ctx.request_repaint();
                                            if let Some(pool) = &db_pool {
                                                persist_twitch_observations(pool, &chatters, now).await;
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

#[cfg(test)]
mod tests {
    use super::host_matches_bypass_pattern;

    #[test]
    fn a_leading_wildcard_matches_a_suffix() {
        assert!(host_matches_bypass_pattern("*.corp.example", "internal.corp.example"));
        assert!(!host_matches_bypass_pattern("*.corp.example", "corp.example.evil.com"));
    }

    #[test]
    fn a_trailing_wildcard_matches_a_prefix() {
        assert!(host_matches_bypass_pattern("10.*", "10.0.0.5"));
        assert!(!host_matches_bypass_pattern("10.*", "192.10.0.5"));
    }

    #[test]
    fn a_bare_pattern_matches_itself_and_subdomains() {
        // Matches `reqwest::NoProxy`'s documented handling of the same
        // `ProxyConfig.bypass` entries elsewhere in this app.
        assert!(host_matches_bypass_pattern("github.com", "github.com"));
        assert!(host_matches_bypass_pattern("github.com", "api.github.com"));
        assert!(!host_matches_bypass_pattern("github.com", "notgithub.com"));
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(host_matches_bypass_pattern("Localhost", "LOCALHOST"));
    }

    #[test]
    fn an_ipv6_bracketed_host_matches_the_bare_bypass_entry() {
        // `parse_bypass`'s `<local>` expansion pushes bare "::1"; the host
        // this app's `Intercept` callback receives keeps IPv6 brackets
        // (`http::Uri::host()`), e.g. "[::1]".
        assert!(host_matches_bypass_pattern("::1", "[::1]"));
        assert!(host_matches_bypass_pattern("127.0.0.1", "127.0.0.1"));
    }
}
