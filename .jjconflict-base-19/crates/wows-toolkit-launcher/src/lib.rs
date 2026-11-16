//! Launcher state and background work shared by the egui app and the GPUI
//! port: install discovery, catalog update checks, downloads, WGNI sign-in,
//! and launching.
//!
//! [`Launcher`] holds what the Launcher tab shows. Its operations spawn work
//! on a tokio runtime and report back as [`LauncherEvent`]s; a front end
//! drains [`LauncherEvents`] and hands each one to [`Launcher::apply`].
//! Settings stay with the front end, which passes them in and saves them when
//! an operation reports a change.

mod display;
mod download;
pub mod proton;
mod settings;
#[cfg(feature = "test-support")]
pub mod test_support;

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use thiserror::Error;
use tracing::warn;
use wgc_catalog::known_games::KnownGame;
use wgc_catalog::update::LatestBuild;
use wgc_install::GameInstall;

pub use display::CatalogSummary;
pub use display::REALMS;
pub use display::format_duration;
pub use display::fresh_install_version;
pub use display::realm_label;
pub use download::DownloadPhase;
pub use settings::LauncherSettings;
pub use settings::PersistedAccount;
pub use settings::SETTINGS_KEY;

/// What a front end supplies for background work.
#[derive(Clone)]
pub struct LauncherEnv {
    pub runtime: tokio::runtime::Handle,
    /// The resolved proxy URL, if the app uses one.
    pub proxy_url: Option<String>,
    /// Called after every event is sent, so a front end that polls can redraw.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

/// Result of a catalog update check, for an install or a known game.
#[derive(Debug, Clone)]
pub enum CatalogCheckState {
    Checking,
    Ready(LatestBuild),
    /// The check failed (network, parse, or protocol). Carries the error's
    /// `Display` text; the crate's own error stays structured, this is only
    /// what the UI shows.
    Error(String),
    /// The install has no `game_info.xml`, so there is no WGUS base URL to
    /// check against (e.g. a manually added install WGC never touched).
    NoUpdateHost,
}

/// Status of a download/install run.
#[derive(Debug, Clone)]
pub enum DownloadState {
    Running {
        phase: DownloadPhase,
        done_bytes: u64,
        total_bytes: u64,
        current: String,
    },
    /// Every torrent's FULL packages were downloaded and extracted.
    /// `skipped_deltas` counts DELTA packages downloaded but left
    /// un-extracted (not yet supported; see `wgc_download`).
    Done {
        skipped_deltas: usize,
    },
    /// Carries the error's `Display` text, as [`CatalogCheckState::Error`].
    Failed(String),
    Cancelled,
    /// Sha1-verified staged files remain on disk; [`Launcher::resume_download`]
    /// starts a fresh run that skips them.
    Paused,
}

impl DownloadState {
    /// Running or paused: the row offers Pause/Cancel or Resume instead of a
    /// new download.
    pub fn is_active(&self) -> bool {
        matches!(self, DownloadState::Running { .. } | DownloadState::Paused)
    }
}

/// How to re-dispatch a paused download. The task that ran it has exited, so
/// resuming starts a fresh run.
#[derive(Debug, Clone)]
enum ResumeSource {
    KnownGame { game_id: &'static str, dest_dir: PathBuf },
    Update { install_path: PathBuf, dest_dir: PathBuf },
}

/// Why an operation could not start.
#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("no update check result for this install yet")]
    NoCatalogResult,
    #[error("no version check result for this game yet")]
    NoVersionCheck,
    #[error("nothing to download: the catalog reports no parts to fetch")]
    NothingToDownload,
    #[error("catalog response has no web seed base to download from")]
    NoWebSeed,
    #[error("catalog response has no torrent URLs to download from")]
    NoTorrentUrls,
    #[error("cannot build download client: {0}")]
    HttpClient(#[from] reqwest::Error),
    #[error("nothing to resume for this download")]
    NothingToResume,
}

/// One result of background work, for [`Launcher::apply`].
pub struct LauncherEvent(Event);

enum Event {
    Installs(Vec<GameInstall>),
    Catalog(PathBuf, CatalogCheckState),
    AvailableGame(&'static str, CatalogCheckState),
    Download(PathBuf, DownloadState),
    Login(LoginMsg),
    SessionRestored { restore: RestoreId, session: Option<wgc_auth::StoredSession> },
}

/// Which keyring restore an event answers. Bumped by every restore, sign-in
/// and sign-out, so a restore that a later change has overtaken is ignored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RestoreId(u64);

enum LoginMsg {
    /// `persisted`: whether the session reached the OS keyring.
    Success {
        session: Box<wgc_auth::StoredSession>,
        persisted: bool,
    },
    TwoFactorRequired {
        twofactor_token: String,
    },
    /// The code was rejected but the challenge is still open: keep it and the
    /// pending password so the user retries the code, not the whole login.
    TwoFactorRetry(String),
    CaptchaRequired,
    Failed(String),
}

/// The receiving end of a [`Launcher`]'s events.
pub struct LauncherEvents(tokio::sync::mpsc::UnboundedReceiver<LauncherEvent>);

impl LauncherEvents {
    /// The next event already delivered, for a front end that polls per frame.
    pub fn try_next(&mut self) -> Option<LauncherEvent> {
        self.0.try_recv().ok()
    }

    /// Waits for the next event. Needs no tokio runtime on the awaiting side.
    pub async fn next(&mut self) -> Option<LauncherEvent> {
        self.0.recv().await
    }
}

/// A login waiting on a two-factor code. WGNI resubmits full credentials
/// alongside the code, so the password is held, zeroizing, until the
/// attempt ends.
struct PendingLogin {
    realm: wgc_auth::AccountRealm,
    username: String,
    password: wgc_auth::SecretPassword,
    twofactor_token: Option<String>,
}

#[derive(Default)]
struct LoginState {
    in_progress: bool,
    error: Option<String>,
    captcha_blocked: bool,
    pending: Option<PendingLogin>,
}

/// What the Launcher tab shows, and the operations that change it.
pub struct Launcher {
    installs: Vec<GameInstall>,
    discovering: bool,
    catalog: HashMap<PathBuf, CatalogCheckState>,
    /// Keyed by the install path for an update, by the destination for a
    /// fresh install.
    downloads: HashMap<PathBuf, DownloadState>,
    controls: HashMap<PathBuf, wgc_download::DownloadControl>,
    resume: HashMap<PathBuf, ResumeSource>,
    available: HashMap<&'static str, CatalogCheckState>,
    /// Known game id to the destination of a fresh install in flight or just
    /// finished, so its row finds its download and a finished one is told
    /// apart from an update.
    installing: HashMap<&'static str, PathBuf>,
    session: Option<wgc_auth::StoredSession>,
    session_persisted: bool,
    /// The keyring's session for the persisted account, when it arrived while a
    /// sign-in was under way.
    deferred_session: Option<wgc_auth::StoredSession>,
    /// The newest keyring restore; see [`RestoreId`].
    restore: RestoreId,
    login: LoginState,
    sender: tokio::sync::mpsc::UnboundedSender<LauncherEvent>,
}

impl Launcher {
    pub fn new() -> (Launcher, LauncherEvents) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let launcher = Launcher {
            installs: Vec::new(),
            discovering: false,
            catalog: HashMap::new(),
            downloads: HashMap::new(),
            controls: HashMap::new(),
            resume: HashMap::new(),
            available: HashMap::new(),
            installing: HashMap::new(),
            session: None,
            session_persisted: false,
            deferred_session: None,
            restore: RestoreId::default(),
            login: LoginState::default(),
            sender,
        };
        (launcher, LauncherEvents(receiver))
    }

    pub fn installs(&self) -> &[GameInstall] {
        &self.installs
    }

    pub fn is_discovering(&self) -> bool {
        self.discovering
    }

    pub fn catalog_status(&self, install: &Path) -> Option<&CatalogCheckState> {
        self.catalog.get(install)
    }

    /// Whether the install's catalog check found something to download.
    pub fn has_downloads(&self, install: &Path) -> bool {
        matches!(self.catalog.get(install), Some(CatalogCheckState::Ready(build)) if !build.downloads.is_empty())
    }

    pub fn download_state(&self, key: &Path) -> Option<&DownloadState> {
        self.downloads.get(key)
    }

    pub fn available_game(&self, game: &KnownGame) -> Option<&CatalogCheckState> {
        self.available.get(game.game_id)
    }

    /// Whether a fresh install of `game` can start.
    pub fn can_install(&self, game: &KnownGame) -> bool {
        let ready = matches!(self.available.get(game.game_id), Some(CatalogCheckState::Ready(build)) if !build.downloads.is_empty());
        let active = self.installing_download(game).is_some_and(|(_, state)| state.is_active());
        ready && !active
    }

    /// The download key and state of a fresh install of `game`, if one was
    /// started.
    pub fn installing_download(&self, game: &KnownGame) -> Option<(&Path, &DownloadState)> {
        let dest = self.installing.get(game.game_id)?;
        self.downloads.get(dest).map(|state| (dest.as_path(), state))
    }

    pub fn session(&self) -> Option<&wgc_auth::StoredSession> {
        self.session.as_ref()
    }

    /// False when the OS had no usable secret store and the session lives
    /// only in memory.
    pub fn session_persisted(&self) -> bool {
        self.session_persisted
    }

    pub fn login_in_progress(&self) -> bool {
        self.login.in_progress
    }

    pub fn login_error(&self) -> Option<&str> {
        self.login.error.as_deref()
    }

    /// Whether a login is waiting on a two-factor code.
    pub fn two_factor_pending(&self) -> bool {
        self.login.pending.as_ref().is_some_and(|p| p.twofactor_token.is_some())
    }

    pub fn captcha_blocked(&self) -> bool {
        self.login.captcha_blocked
    }

    fn sender(&self, env: &LauncherEnv) -> EventSender {
        EventSender { sender: self.sender.clone(), wake: Arc::clone(&env.wake) }
    }

    /// Apply one event. Returns whether `settings` changed and need saving.
    #[must_use]
    pub fn apply(&mut self, event: LauncherEvent, settings: &mut LauncherSettings, env: &LauncherEnv) -> bool {
        match event.0 {
            Event::Installs(installs) => {
                self.installs = installs;
                self.discovering = false;
                // Only a delivered discovery round starts checks; the guard in
                // spawn_catalog_check skips installs already checked.
                let installs = self.installs.clone();
                for install in &installs {
                    self.spawn_catalog_check(install, false, env);
                }
                false
            }
            Event::Catalog(path, state) => {
                self.catalog.insert(path, state);
                false
            }
            Event::AvailableGame(game_id, state) => {
                self.available.insert(game_id, state);
                false
            }
            Event::Download(key, state) => {
                let finished = !state.is_active();
                let fresh_install_done =
                    matches!(state, DownloadState::Done { .. }) && self.installing.values().any(|dest| dest == &key);
                if finished {
                    self.controls.remove(&key);
                }
                self.downloads.insert(key.clone(), state);
                if !fresh_install_done {
                    return false;
                }
                let changed = !settings.manual_installs.contains(&key);
                if changed {
                    settings.manual_installs.push(key);
                }
                self.start_install_discovery(settings, env);
                changed
            }
            Event::Login(msg) => self.apply_login(msg, settings),
            Event::SessionRestored { restore, session } => {
                // A sign-in that succeeded, or a sign-out, since the restore
                // started has the last word.
                if restore != self.restore || self.session.is_some() {
                    return false;
                }
                // A sign-in under way decides first; the stored session waits
                // in case it fails.
                if self.login.in_progress || self.login.pending.is_some() {
                    self.deferred_session = session;
                } else {
                    self.session_persisted = session.is_some();
                    self.session = session;
                }
                false
            }
        }
    }

    fn adopt_deferred_session(&mut self) {
        if self.session.is_none()
            && let Some(session) = self.deferred_session.take()
        {
            self.session = Some(session);
            self.session_persisted = true;
        }
    }

    /// Leave the CAPTCHA notice for the sign-in form, once the user has signed
    /// in elsewhere and wants to try again.
    pub fn dismiss_captcha(&mut self) {
        self.login.captcha_blocked = false;
    }

    fn apply_login(&mut self, msg: LoginMsg, settings: &mut LauncherSettings) -> bool {
        self.login.in_progress = false;
        match msg {
            LoginMsg::Success { session, persisted } => {
                settings.active_account = Some(PersistedAccount {
                    realm: session.realm,
                    account_id: session.account_id,
                    nickname: session.nickname.clone(),
                });
                self.session = Some(*session);
                self.session_persisted = persisted;
                self.login = LoginState::default();
                self.deferred_session = None;
                self.restore.0 += 1;
                true
            }
            LoginMsg::TwoFactorRequired { twofactor_token } => {
                if let Some(pending) = self.login.pending.as_mut() {
                    pending.twofactor_token = Some(twofactor_token);
                }
                false
            }
            LoginMsg::TwoFactorRetry(error) => {
                self.login.error = Some(error);
                false
            }
            LoginMsg::CaptchaRequired => {
                self.login.captcha_blocked = true;
                self.login.pending = None;
                self.adopt_deferred_session();
                false
            }
            LoginMsg::Failed(error) => {
                self.login.error = Some(error);
                self.login.pending = None;
                self.adopt_deferred_session();
                false
            }
        }
    }

    /// Load the session of the persisted account from the OS keyring, off the
    /// calling thread: the keyring may block on its service.
    pub fn start_session_restore(&mut self, settings: &LauncherSettings, env: &LauncherEnv) {
        let Some(account) = settings.active_account.clone() else {
            return;
        };
        self.restore.0 += 1;
        let restore = self.restore;
        let sender = self.sender(env);
        env.runtime.spawn_blocking(move || {
            let session = match wgc_auth::SessionStore::new().load(account.realm, account.account_id) {
                Ok(session) => Some(session),
                Err(wgc_auth::AuthError::NoStoredSession) => None,
                // Covers both an absent store and a locked one; the keyring
                // error taxonomy does not tell them apart.
                Err(wgc_auth::AuthError::NoSecretStore) => {
                    tracing::debug!("no accessible OS secret store; starting signed out");
                    None
                }
                Err(e) => {
                    warn!("could not load stored session from keyring: {e}");
                    None
                }
            };
            sender.send(Event::SessionRestored { restore, session });
        });
    }

    /// Scan for game installs. A no-op while a scan is running.
    pub fn start_install_discovery(&mut self, settings: &LauncherSettings, env: &LauncherEnv) {
        if self.discovering {
            return;
        }
        self.discovering = true;
        let data_dir_override = settings.wgc_data_dir_override.clone();
        let manual = settings.manual_installs.clone();
        let sender = self.sender(env);
        env.runtime.spawn_blocking(move || {
            let installs = wgc_install::discover_all(data_dir_override.as_deref(), &manual);
            sender.send(Event::Installs(installs));
        });
    }

    /// Force a fresh catalog check for one install.
    pub fn recheck_catalog(&mut self, install: &Path, env: &LauncherEnv) {
        let Some(install) = self.installs.iter().find(|i| i.path == install).cloned() else {
            return;
        };
        self.spawn_catalog_check(&install, true, env);
    }

    /// Skips Steam installs (never patched by this app) and, unless `force`,
    /// installs whose state is already `Checking` or `Ready`.
    fn spawn_catalog_check(&mut self, install: &GameInstall, force: bool, env: &LauncherEnv) {
        if install.source.is_read_only_content() {
            return;
        }
        if !force
            && matches!(
                self.catalog.get(&install.path),
                Some(CatalogCheckState::Checking | CatalogCheckState::Ready(_))
            )
        {
            return;
        }
        self.catalog.insert(install.path.clone(), CatalogCheckState::Checking);
        let install_path = install.path.clone();
        let proxy_url = env.proxy_url.clone();
        let sender = self.sender(env);
        env.runtime.spawn(async move {
            let state =
                download::check_install_catalog(&install_path.join("game_info.xml"), proxy_url.as_deref()).await;
            sender.send(Event::Catalog(install_path, state));
        });
    }

    /// Check what each built-in known game would install. Per game a no-op
    /// while a check is running or after one succeeded.
    ///
    /// Every known game is an EU-hosted WGUS descriptor (see
    /// `wgc_catalog::known_games`), shown regardless of the signed-in realm.
    pub fn start_available_games_check(&mut self, env: &LauncherEnv) {
        for known in wgc_catalog::known_games::known_games() {
            if matches!(
                self.available.get(known.game_id),
                Some(CatalogCheckState::Checking | CatalogCheckState::Ready(_))
            ) {
                continue;
            }
            let game_info = match known.to_fresh_game_info() {
                Ok(info) => info,
                Err(e) => {
                    self.available.insert(known.game_id, CatalogCheckState::Error(e.to_string()));
                    continue;
                }
            };
            self.available.insert(known.game_id, CatalogCheckState::Checking);
            let game_id = known.game_id;
            let proxy_url = env.proxy_url.clone();
            let sender = self.sender(env);
            env.runtime.spawn(async move {
                let state = match wgc_catalog::CatalogClient::with_proxy(proxy_url.as_deref()) {
                    Ok(client) => match client.check_updates(&game_info).await {
                        Ok(build) => CatalogCheckState::Ready(build),
                        Err(e) => CatalogCheckState::Error(e.to_string()),
                    },
                    Err(e) => CatalogCheckState::Error(e.to_string()),
                };
                sender.send(Event::AvailableGame(game_id, state));
            });
        }
    }

    /// Whether every known game has a check result or one running.
    pub fn available_games_checked(&self) -> bool {
        wgc_catalog::known_games::known_games().iter().all(|known| self.available.contains_key(known.game_id))
    }

    /// Download and install `install`'s newest catalog build into `dest_dir`.
    pub fn start_download(
        &mut self,
        install: PathBuf,
        dest_dir: PathBuf,
        env: &LauncherEnv,
    ) -> Result<(), LauncherError> {
        let Some(CatalogCheckState::Ready(latest)) = self.catalog.get(&install) else {
            return Err(LauncherError::NoCatalogResult);
        };
        let latest = latest.clone();
        let source = ResumeSource::Update { install_path: install.clone(), dest_dir: dest_dir.clone() };
        self.start_download_build(&latest, install, dest_dir, source, env)
    }

    /// Install `game` fresh into `dest_dir`.
    pub fn install_known_game(
        &mut self,
        game: &KnownGame,
        dest_dir: PathBuf,
        env: &LauncherEnv,
    ) -> Result<(), LauncherError> {
        self.install_game_id(game.game_id, dest_dir, env)
    }

    fn install_game_id(
        &mut self,
        game_id: &'static str,
        dest_dir: PathBuf,
        env: &LauncherEnv,
    ) -> Result<(), LauncherError> {
        let Some(CatalogCheckState::Ready(latest)) = self.available.get(game_id) else {
            return Err(LauncherError::NoVersionCheck);
        };
        let latest = latest.clone();
        let source = ResumeSource::KnownGame { game_id, dest_dir: dest_dir.clone() };
        self.start_download_build(&latest, dest_dir.clone(), dest_dir.clone(), source, env)?;
        self.installing.insert(game_id, dest_dir);
        Ok(())
    }

    /// Start a fresh run of the paused download under `key`.
    pub fn resume_download(&mut self, key: &Path, env: &LauncherEnv) -> Result<(), LauncherError> {
        let Some(source) = self.resume.get(key).cloned() else {
            return Err(LauncherError::NothingToResume);
        };
        match source {
            ResumeSource::KnownGame { game_id, dest_dir } => self.install_game_id(game_id, dest_dir, env),
            ResumeSource::Update { install_path, dest_dir } => self.start_download(install_path, dest_dir, env),
        }
    }

    /// Ask the run under `key` to pause; it reports `Paused` once it stops.
    pub fn pause_download(&self, key: &Path) {
        if let Some(control) = self.controls.get(key) {
            control.pause.store(true, Ordering::SeqCst);
        }
    }

    /// Ask the run under `key` to stop; it reports `Cancelled` once it stops.
    pub fn cancel_download(&self, key: &Path) {
        if let Some(control) = self.controls.get(key) {
            control.cancel.store(true, Ordering::SeqCst);
        }
    }

    fn start_download_build(
        &mut self,
        latest: &LatestBuild,
        key: PathBuf,
        dest_dir: PathBuf,
        source: ResumeSource,
        env: &LauncherEnv,
    ) -> Result<(), LauncherError> {
        let plan = download::DownloadPlan::of(latest, dest_dir)?;
        let client = download::http_client(env.proxy_url.as_deref())?;
        let control = wgc_download::DownloadControl {
            cancel: Arc::new(AtomicBool::new(false)),
            pause: Arc::new(AtomicBool::new(false)),
        };
        self.resume.insert(key.clone(), source);
        self.controls.insert(key.clone(), control.clone());
        self.downloads.insert(
            key.clone(),
            DownloadState::Running {
                phase: DownloadPhase::Starting,
                done_bytes: 0,
                total_bytes: 0,
                current: String::new(),
            },
        );
        let sender = self.sender(env);
        // `spawn_blocking` + `Handle::block_on`, not `spawn`: the download chain
        // nests async fns taking borrowed parameters and fans a progress closure
        // out through `buffer_unordered`, which trips rustc's higher-ranked
        // `Send` inference under `spawn`. `block_on` has no `Send` bound.
        env.runtime.spawn_blocking(move || {
            let handle = tokio::runtime::Handle::current();
            let progress_sender = sender.clone();
            let progress_key = key.clone();
            let final_state = handle.block_on(download::run(&client, &plan, &control, |update| {
                progress_sender.send(Event::Download(progress_key.clone(), update));
            }));
            sender.send(Event::Download(key, final_state));
        });
        Ok(())
    }

    /// Start a sign-in. Returns whether `settings` changed (a tracking id was
    /// minted) and need saving.
    #[must_use]
    pub fn start_login(
        &mut self,
        realm: wgc_auth::AccountRealm,
        username: String,
        password: wgc_auth::SecretPassword,
        settings: &mut LauncherSettings,
        env: &LauncherEnv,
    ) -> bool {
        let (tracking_id, changed) = settings.ensure_tracking_id();
        self.login = LoginState {
            in_progress: true,
            pending: Some(PendingLogin {
                realm,
                username: username.clone(),
                password: wgc_auth::SecretPassword::new(password.expose_secret().to_string()),
                twofactor_token: None,
            }),
            ..LoginState::default()
        };
        let sender = self.sender(env);
        env.runtime.spawn(async move {
            let client = wgc_auth::AuthClient::new(realm, tracking_id);
            let result = client.login(wgc_auth::AccountCredentials { username, password }).await;
            sender.send(Event::Login(login_result(result).await));
        });
        changed
    }

    /// Resubmit the pending login with a two-factor code. A no-op without a
    /// pending challenge. Returns whether `settings` changed and need saving.
    #[must_use]
    pub fn submit_two_factor(
        &mut self,
        code: wgc_auth::TwoFactorCode,
        settings: &mut LauncherSettings,
        env: &LauncherEnv,
    ) -> bool {
        let Some(pending) = self.login.pending.as_ref() else {
            return false;
        };
        let Some(twofactor_token) = pending.twofactor_token.clone() else {
            return false;
        };
        let realm = pending.realm;
        let credentials = wgc_auth::AccountCredentials {
            username: pending.username.clone(),
            password: wgc_auth::SecretPassword::new(pending.password.expose_secret().to_string()),
        };
        let (tracking_id, changed) = settings.ensure_tracking_id();
        self.login.in_progress = true;
        self.login.error = None;
        let sender = self.sender(env);
        env.runtime.spawn(async move {
            let client = wgc_auth::AuthClient::new(realm, tracking_id);
            let result = client.login_with_two_factor(credentials, twofactor_token, code).await;
            sender.send(Event::Login(login_result(result).await));
        });
        changed
    }

    /// Clear the session locally, then revoke it server-side (best effort) and
    /// delete its keyring entry. Returns whether `settings` changed.
    #[must_use]
    pub fn sign_out(&mut self, settings: &mut LauncherSettings, env: &LauncherEnv) -> bool {
        let Some(session) = self.session.take() else {
            return false;
        };
        self.session_persisted = false;
        self.deferred_session = None;
        self.restore.0 += 1;
        settings.active_account = None;
        let (tracking_id, _) = settings.ensure_tracking_id();
        let realm = session.realm;
        let account_id = session.account_id;
        env.runtime.spawn(async move {
            let client = wgc_auth::AuthClient::new(realm, tracking_id);
            if let Err(e) = client.logout(&session.tokens).await {
                warn!("server-side session revoke failed (signing out locally anyway): {e}");
            }
            drop(session);
            let deleted =
                tokio::task::spawn_blocking(move || wgc_auth::SessionStore::new().delete(realm, account_id)).await;
            if let Ok(Err(e)) = deleted {
                warn!("could not delete the stored session from the keyring: {e}");
            }
        });
        true
    }

    /// Launch `install` for online play. With no signed-in account the gate
    /// allows the launch (the game shows its own login); with one, an expired
    /// session is refused here instead of failing inside the game. `prepare`
    /// decorates the command before it spawns.
    pub fn launch(
        &self,
        install: &GameInstall,
        settings: &LauncherSettings,
        safe_mode: bool,
        prepare: impl FnOnce(&mut std::process::Command),
    ) -> Result<(), wgc_launch::LaunchError> {
        let args = settings.launch_args(safe_mode);
        let resolved = wgc_launch::resolve(install, wgc_launch::LaunchMode::Online, &args)?;
        let status = wgc_auth::launch_token_status(self.session.as_ref(), std::time::SystemTime::now());
        resolved.spawn(settings.proton_config().as_ref(), status, prepare)?;
        Ok(())
    }
}

/// Open `replay` in the game installed at `wows_dir`. Replay playback is
/// offline and never gated. `prepare` decorates the command before it spawns.
pub fn open_replay_in_game(
    wows_dir: &Path,
    replay: PathBuf,
    prepare: impl FnOnce(&mut std::process::Command),
) -> Result<(), wgc_launch::LaunchError> {
    let install = GameInstall::manual(wows_dir.to_path_buf());
    let resolved = wgc_launch::resolve(
        &install,
        wgc_launch::LaunchMode::Replay { path: replay },
        &wgc_launch::LaunchArgs::default(),
    )?;
    let mut command = resolved.native_command();
    prepare(&mut command);
    command.spawn()?;
    Ok(())
}

#[derive(Clone)]
struct EventSender {
    sender: tokio::sync::mpsc::UnboundedSender<LauncherEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl EventSender {
    fn send(&self, event: Event) {
        // A closed channel means the front end dropped its Launcher; there is
        // no one left to tell.
        let _ = self.sender.send(LauncherEvent(event));
        (self.wake)();
    }
}

/// Turns a login result into the message for it. On success the session is
/// first saved to the OS keyring (sync, so on a blocking-pool thread).
async fn login_result(result: rootcause::Result<wgc_auth::StoredSession, wgc_auth::AuthError>) -> LoginMsg {
    let report = match result {
        Ok(session) => {
            let saved = tokio::task::spawn_blocking(move || {
                let persisted = wgc_auth::SessionStore::new().save(&session);
                (session, persisted)
            })
            .await;
            return match saved {
                Ok((session, Ok(()))) => LoginMsg::Success { session: Box::new(session), persisted: true },
                Ok((session, Err(wgc_auth::AuthError::NoSecretStore))) => {
                    LoginMsg::Success { session: Box::new(session), persisted: false }
                }
                Ok((session, Err(e))) => {
                    warn!("could not save session to keyring: {e}");
                    LoginMsg::Success { session: Box::new(session), persisted: false }
                }
                Err(join_err) => {
                    warn!("keyring save task did not complete: {join_err}");
                    LoginMsg::Failed("signed in, but could not save the session; sign in again next launch".to_string())
                }
            };
        }
        Err(report) => report,
    };
    match report.current_context() {
        wgc_auth::AuthError::TwoFactorRequired { twofactor_token } => {
            LoginMsg::TwoFactorRequired { twofactor_token: twofactor_token.clone() }
        }
        wgc_auth::AuthError::CaptchaRequired => LoginMsg::CaptchaRequired,
        // A wrong code keeps the challenge open; a lockout or ban does not.
        error @ wgc_auth::AuthError::TwoFactorInvalidCode => LoginMsg::TwoFactorRetry(error.to_string()),
        // The full report, so attached diagnostic context reaches the user;
        // none of these error paths carry a password or token.
        _ => LoginMsg::Failed(report.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(nickname: &str) -> wgc_auth::StoredSession {
        wgc_auth::StoredSession {
            realm: wgc_auth::AccountRealm::Eu,
            account_id: wgc_auth::AccountId::new(1),
            nickname: wgc_auth::Nickname::new(nickname),
            tokens: wgc_auth::TokenPair {
                access_token: wgc_auth::AccessToken::new("token".into()),
                exchange_code: wgc_auth::ExchangeCode::from_raw("code".into()),
            },
            expiry: wgc_auth::TokenExpiry::Unknown,
        }
    }

    fn env(runtime: &tokio::runtime::Runtime) -> LauncherEnv {
        LauncherEnv { runtime: runtime.handle().clone(), proxy_url: None, wake: Arc::new(|| {}) }
    }

    fn signing_in(launcher: &mut Launcher) {
        launcher.login.in_progress = true;
        launcher.login.pending = Some(PendingLogin {
            realm: wgc_auth::AccountRealm::Eu,
            username: "user".into(),
            password: wgc_auth::SecretPassword::new("password".into()),
            twofactor_token: None,
        });
    }

    fn nickname(launcher: &Launcher) -> Option<&str> {
        launcher.session().map(|s| s.nickname.as_str())
    }

    /// The event a keyring restore started now would deliver.
    fn restored(launcher: &Launcher, nickname: &str) -> LauncherEvent {
        LauncherEvent(Event::SessionRestored { restore: launcher.restore, session: Some(session(nickname)) })
    }

    #[test]
    fn a_restored_session_waiting_on_a_failed_sign_in_is_adopted() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let mut settings = LauncherSettings::default();
        let (mut launcher, _events) = Launcher::new();
        signing_in(&mut launcher);

        let _ = launcher.apply(restored(&launcher, "stored"), &mut settings, &env);
        assert_eq!(nickname(&launcher), None, "the sign-in under way decides first");

        let _ = launcher.apply(LauncherEvent(Event::Login(LoginMsg::Failed("nope".into()))), &mut settings, &env);
        assert_eq!(nickname(&launcher), Some("stored"));
        assert!(launcher.session_persisted());
    }

    #[test]
    fn a_successful_sign_in_discards_the_restored_session() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let mut settings = LauncherSettings::default();
        let (mut launcher, _events) = Launcher::new();
        signing_in(&mut launcher);

        let _ = launcher.apply(restored(&launcher, "stored"), &mut settings, &env);
        let success = LoginMsg::Success { session: Box::new(session("fresh")), persisted: false };
        assert!(launcher.apply(LauncherEvent(Event::Login(success)), &mut settings, &env));
        assert_eq!(nickname(&launcher), Some("fresh"));
        assert_eq!(settings.active_account.as_ref().map(|a| a.nickname.as_str()), Some("fresh"));

        // The discarded session must not come back once the user signs out.
        assert!(launcher.sign_out(&mut settings, &env));
        let _ = launcher.apply(LauncherEvent(Event::Login(LoginMsg::Failed("later".into()))), &mut settings, &env);
        assert_eq!(nickname(&launcher), None);
    }

    #[test]
    fn a_restore_overtaken_by_sign_in_and_sign_out_is_ignored() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let mut settings = LauncherSettings::default();
        let (mut launcher, _events) = Launcher::new();
        let in_flight = restored(&launcher, "stored");
        signing_in(&mut launcher);

        let success = LoginMsg::Success { session: Box::new(session("fresh")), persisted: false };
        let _ = launcher.apply(LauncherEvent(Event::Login(success)), &mut settings, &env);
        assert!(launcher.sign_out(&mut settings, &env));

        let _ = launcher.apply(in_flight, &mut settings, &env);
        assert_eq!(nickname(&launcher), None, "the sign-out has the last word");
    }
}
