//! The launcher's state and background work: install discovery, catalog update checks and
//! downloads. Accounts and the game session extend [`Launcher`] in their own modules.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use wgc_catalog::known_games::KnownGame;
use wgc_catalog::update::LatestBuild;
use wgc_install::GameInstall;

use crate::accounts::AccountSession;
use crate::accounts::KeyringState;
use crate::accounts::LoginMsg;
use crate::accounts::LoginState;
use crate::accounts::LoginTarget;
use crate::accounts::RestoreId;
use crate::accounts::RestoreOutcome;
use crate::download;
use crate::download::DownloadPhase;
use crate::error::LaunchFailure;
use crate::error::LauncherError;
use crate::game::GameEvent;
use crate::game::GameExit;
use crate::game::GameGate;
use crate::game::GameState;
use crate::game::LaunchId;
use crate::runner::RunnerState;
use crate::settings::AccountKey;
use crate::settings::LauncherSettings;

/// What a front end supplies for background work.
#[derive(Clone)]
pub struct LauncherEnv {
    pub runtime: tokio::runtime::Handle,
    /// The resolved proxy URL, if the app uses one.
    pub proxy_url: Option<String>,
    /// Called after every event is sent, so a front end that polls can redraw.
    pub wake: Arc<dyn Fn() + Send + Sync>,
    /// Where the game runner keeps its prefixes and overlays, under the app's data directory.
    /// `None` when the app has none, which leaves the game unlaunchable.
    pub runtime_dirs: Option<wgc_launch::RuntimeDirs>,
    /// Where sessions and game tokens are kept: the OS secret store, or an unencrypted file in
    /// the app's data directory when there is none.
    pub credentials: wgc_auth::CredentialStore,
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

/// One result of background work, for [`Launcher::apply`].
pub struct LauncherEvent(pub(crate) Event);

pub(crate) enum Event {
    Installs(Vec<GameInstall>),
    Catalog(PathBuf, CatalogCheckState),
    AvailableGame(&'static str, CatalogCheckState),
    Download(PathBuf, DownloadState),
    Login(LoginMsg),
    SessionRestored {
        key: AccountKey,
        restore: RestoreId,
        outcome: RestoreOutcome,
    },
    Game(GameEvent),
    KeyringProbed(KeyringState),
    RunnerProbed {
        choice: wgc_launch::RunnerChoice,
        result: Result<wgc_launch::ResolvedRunner, wgc_launch::RunnerError>,
    },
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

/// What the launcher shows, and the operations that change it.
pub struct Launcher {
    installs: Vec<GameInstall>,
    discovering: bool,
    pub(crate) catalog: HashMap<PathBuf, CatalogCheckState>,
    /// Keyed by the install path for an update, by the destination for a fresh install.
    downloads: HashMap<PathBuf, DownloadState>,
    controls: HashMap<PathBuf, wgc_download::DownloadControl>,
    resume: HashMap<PathBuf, ResumeSource>,
    available: HashMap<&'static str, CatalogCheckState>,
    /// Known game id to the destination of a fresh install in flight or just finished, so its
    /// row finds its download and a finished one is told apart from an update.
    installing: HashMap<&'static str, PathBuf>,

    pub(crate) sessions: HashMap<AccountKey, AccountSession>,
    /// The newest keyring restore per account; see [`RestoreId`].
    pub(crate) restores: HashMap<AccountKey, RestoreId>,
    pub(crate) next_restore: u64,
    pub(crate) login: LoginState,
    pub(crate) keyring: KeyringState,

    pub(crate) game: GameState,
    pub(crate) gate: GameGate,
    pub(crate) launch: LaunchId,
    pub(crate) last_exit: Option<GameExit>,
    pub(crate) launch_failure: Option<LaunchFailure>,
    pub(crate) runner: RunnerState,

    sender: tokio::sync::mpsc::UnboundedSender<LauncherEvent>,

    /// Sign-in answers a front end's test scripted, which stand in for WGNI.
    #[cfg(feature = "test-support")]
    pub(crate) script: crate::test_support::SignInScript,
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
            sessions: HashMap::new(),
            restores: HashMap::new(),
            next_restore: 0,
            login: LoginState::new(LoginTarget::NewAccount),
            keyring: KeyringState::Unknown,
            game: GameState::Idle,
            gate: GameGate::new(),
            launch: LaunchId::default(),
            last_exit: None,
            launch_failure: None,
            runner: RunnerState::Unknown,
            sender,
            #[cfg(feature = "test-support")]
            script: crate::test_support::SignInScript::default(),
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

    /// The bytes an update of the install would fetch, when its catalog check found parts to
    /// download.
    pub fn update_size(&self, install: &Path) -> Option<u64> {
        let Some(CatalogCheckState::Ready(build)) = self.catalog.get(install) else {
            return None;
        };
        if build.downloads.is_empty() {
            return None;
        }

        Some(build.downloads.iter().flat_map(|part| &part.files).map(|file| file.size).sum())
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

    pub(crate) fn sender(&self, env: &LauncherEnv) -> EventSender {
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
                self.carry_all_replay_settings(env);
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
                self.carry_replay_settings_after(&key, &state, env);
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
            Event::SessionRestored { key, restore, outcome } => self.apply_restore(key, restore, outcome, settings),
            Event::Game(event) => {
                self.apply_game(event);
                false
            }
            Event::KeyringProbed(state) => {
                self.apply_keyring_probe(state);
                false
            }
            Event::RunnerProbed { choice, result } => {
                self.apply_runner_probe(choice, result, settings);
                false
            }
        }
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
}

#[derive(Clone)]
pub(crate) struct EventSender {
    sender: tokio::sync::mpsc::UnboundedSender<LauncherEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl EventSender {
    pub(crate) fn send(&self, event: Event) {
        // A closed channel means the front end dropped its Launcher; there is
        // no one left to tell.
        let _ = self.sender.send(LauncherEvent(event));
        (self.wake)();
    }
}
