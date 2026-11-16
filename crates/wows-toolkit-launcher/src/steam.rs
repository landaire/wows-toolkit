//! Steam installs: Play hands them to `steam -applaunch` by default, or, once the user opts in
//! per install, runs them through the toolkit's WGC sign-in with the WGC layer laid over them.

use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use std::time::Instant;

use wgc_install::GameInstall;
use wgc_install::InstallSource;
use wgc_install::steam::AppManifest;
use wgc_install::steam::AppStateFlags;
use wgc_install::steam::WOWS_STEAM_APP_ID;

use crate::accounts::AccountSession;
use crate::error::LaunchDeltaError;
use crate::error::LaunchFailure;
use crate::error::PlayError;
use crate::error::WgcLayerError;
use crate::game::GameBuild;
use crate::game::GameEvent;
use crate::game::GameExit;
use crate::game::GameState;
use crate::game::LaunchId;
use crate::launcher::Event;
use crate::launcher::EventSender;
use crate::launcher::Launcher;
use crate::launcher::LauncherEnv;
use crate::settings::LauncherSettings;
use crate::wgc_layer::LayerOffer;
use crate::wgc_layer::LayerProbe;
use crate::wgc_layer::LayerStore;
use crate::wgc_layer::WgcLayer;

/// How Play starts an install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayRoute {
    /// Through the runner, signed in as an account.
    Toolkit,
    /// `steam -applaunch`; no account is involved.
    Steam,
    /// A Steam install through the runner, signed in as an account, with the WGC layer.
    WgcLogin,
}

/// Where the WGC layer for one game build stands.
#[derive(Clone, Debug)]
pub enum WgcLayerState {
    Probing,
    Ready(Arc<WgcLayer>),
    /// WG offers the files for the build; none are downloaded yet.
    Fetchable(Arc<LayerOffer>),
    Fetching {
        done_bytes: u64,
        total_bytes: u64,
    },
    Unavailable(Arc<WgcLayerError>),
}

/// Whether the per-install Wargaming login can be turned on.
#[derive(Clone, Debug)]
pub enum WgcLoginAvailability {
    Available,
    /// No saved toolkit sign-in is signed in or still being restored.
    NeedsSignIn,
    /// The layer for the install's build is being looked up.
    Checking,
    /// WG offers no layer for the install's build, or it could not be fetched.
    Unavailable(Arc<WgcLayerError>),
}

/// The layer a launch lays over a Steam install, and what Play checked the install against.
pub(crate) struct LaunchDelta {
    layer: Arc<WgcLayer>,
    /// The Steam library holding the install; its appmanifest says whether Steam updates it.
    library: PathBuf,
    /// The game build the layer was picked for.
    build: GameBuild,
}

impl LaunchDelta {
    pub(crate) fn delta_dir(&self) -> PathBuf {
        self.layer.delta_dir()
    }

    /// Checks again that Steam leaves the install at the build the layer was picked for, then
    /// writes what the layer needs for the install. Call under the session lock, just before the
    /// layer is selected: Steam may have started an update since Play.
    pub(crate) fn prepare_for(&self, install: &Path) -> Result<(), LaunchFailure> {
        let manifest = settled_manifest(&self.library, install)?;
        let found = install_build(install)?;
        if found != self.build {
            return Err(
                LaunchDeltaError::BuildChanged { path: install.to_path_buf(), expected: self.build, found }.into()
            );
        }
        if !self.layer.covers(found) {
            return Err(LaunchDeltaError::WgcLayerNotReady { build: found }.into());
        }

        self.layer.prepare_for(install, manifest.language.as_deref())?;

        Ok(())
    }
}

impl Launcher {
    /// Whether Steam installs offer the Wargaming login here. The layer needs an overlay, which
    /// the runner only builds on Linux.
    pub fn wgc_login_supported(&self) -> bool {
        cfg!(target_os = "linux")
    }

    /// The discovered install at `path` when Steam manages it.
    pub fn steam_install(&self, path: &Path) -> Option<&GameInstall> {
        self.installs().iter().find(|install| install.path == path && install.source.is_read_only_content())
    }

    /// How Play starts the install at `path`.
    pub fn play_route(&self, path: &Path, settings: &LauncherSettings) -> PlayRoute {
        if self.steam_install(path).is_none() {
            return PlayRoute::Toolkit;
        }
        if self.wgc_login_supported() && settings.uses_wgc_login(path) {
            return PlayRoute::WgcLogin;
        }

        PlayRoute::Steam
    }

    /// The layer state for the game build of the Steam install at `path`.
    pub fn wgc_layer_state(&self, path: &Path) -> Option<&WgcLayerState> {
        self.wgc_layers.get(self.steam_builds.get(path)?)
    }

    /// Whether the Wargaming login can be turned on for the Steam install at `path`.
    pub fn wgc_login_availability(&self, path: &Path, settings: &LauncherSettings) -> WgcLoginAvailability {
        // A session not read from the store yet, or being restored, may still turn out signed in.
        let usable = settings.accounts.iter().any(|account| {
            matches!(
                self.account_session(account.key),
                AccountSession::SignedIn { .. } | AccountSession::NotLoaded | AccountSession::Restoring
            )
        });
        if !usable {
            return WgcLoginAvailability::NeedsSignIn;
        }

        match self.wgc_layer_state(path) {
            None | Some(WgcLayerState::Probing) => WgcLoginAvailability::Checking,
            Some(WgcLayerState::Ready(_) | WgcLayerState::Fetchable(_) | WgcLayerState::Fetching { .. }) => {
                WgcLoginAvailability::Available
            }
            Some(WgcLayerState::Unavailable(err)) => WgcLoginAvailability::Unavailable(Arc::clone(err)),
        }
    }

    /// Turns the Wargaming login on or off for the Steam install at `path`. Turning it on
    /// downloads the layer if it is not cached. Returns whether `settings` changed.
    pub fn set_wgc_login(&mut self, path: &Path, on: bool, settings: &mut LauncherSettings, env: &LauncherEnv) -> bool {
        if !settings.set_wgc_login(path, on) {
            return false;
        }

        if on {
            self.pending_delta_clears.remove(path);
            if let Some(build) = self.steam_builds.get(path).copied() {
                self.fetch_wgc_layer_if_wanted(build, settings, env);
            }
        } else if self.game.install() == Some(path) {
            // The running game's view keeps the layer; it is cleared once that game ends.
            self.pending_delta_clears.insert(path.to_path_buf());
        } else {
            self.clear_wgc_delta(path.to_path_buf(), env);
        }

        true
    }

    /// Clears the layer of `install` if turning the login off left it selected while the game
    /// ran from it. Call once the game from `install` has ended.
    pub(crate) fn finish_pending_delta_clear(&mut self, install: &Path, env: &LauncherEnv) {
        if self.pending_delta_clears.remove(install) {
            self.clear_wgc_delta(install.to_path_buf(), env);
        }
    }

    /// Stops laying the WGC layer over `install`. The toolkit reads the install's files from the
    /// view of its next launch.
    fn clear_wgc_delta(&self, install: PathBuf, env: &LauncherEnv) {
        let Some(dirs) = env.runtime_dirs.clone() else {
            return;
        };

        env.runtime.spawn_blocking(move || {
            if let Err(err) = wgc_launch::InstallView::select_delta(&install, &dirs, None) {
                tracing::warn!("could not clear the WGC layer of {}: {err}", install.display());
            }
        });
    }

    /// Looks up the layer for every discovered Steam install's build. Runs after discovery.
    pub(crate) fn probe_wgc_layers(&mut self, settings: &LauncherSettings, env: &LauncherEnv) {
        if !self.wgc_login_supported() {
            return;
        }

        let steam_installs: Vec<PathBuf> = self
            .installs()
            .iter()
            .filter(|install| install.source.is_read_only_content())
            .map(|install| install.path.clone())
            .collect();
        for path in steam_installs {
            let build = match crate::wgc_layer::newest_build(&path) {
                Ok(Some(build)) => build,
                Ok(None) => continue,
                Err(err) => {
                    tracing::warn!("could not read the game builds of {}: {err}", path.display());
                    continue;
                }
            };
            self.probe_wgc_layer(path, build, settings, env);
        }
    }

    /// Records `build` as the game build of the Steam install at `path` and looks up its layer,
    /// unless that is known or under way. Fetches it when the login is on.
    fn probe_wgc_layer(&mut self, path: PathBuf, build: GameBuild, settings: &LauncherSettings, env: &LauncherEnv) {
        self.steam_builds.insert(path, build);
        if self.wgc_layers.contains_key(&build) {
            self.fetch_wgc_layer_if_wanted(build, settings, env);
            return;
        }

        let Some(dirs) = env.runtime_dirs.clone() else {
            return;
        };
        self.wgc_layers.insert(build, WgcLayerState::Probing);
        let proxy_url = env.proxy_url.clone();
        let sender = self.sender(env);
        env.runtime.spawn(async move {
            let state = match LayerStore::new(&dirs).probe(build, proxy_url.as_deref()).await {
                Ok(LayerProbe::Cached(layer)) => WgcLayerState::Ready(Arc::new(layer)),
                Ok(LayerProbe::Fetchable(offer)) => WgcLayerState::Fetchable(Arc::new(offer)),
                Err(err) => WgcLayerState::Unavailable(Arc::new(err)),
            };
            sender.send(Event::WgcLayer(build, state));
        });
    }

    pub(crate) fn apply_wgc_layer(
        &mut self,
        build: GameBuild,
        state: WgcLayerState,
        settings: &LauncherSettings,
        env: &LauncherEnv,
    ) {
        self.wgc_layers.insert(build, state);
        self.fetch_wgc_layer_if_wanted(build, settings, env);
    }

    /// Downloads the layer for `build` when an install of that build has the login on and WG
    /// offers the files.
    fn fetch_wgc_layer_if_wanted(&mut self, build: GameBuild, settings: &LauncherSettings, env: &LauncherEnv) {
        let Some(WgcLayerState::Fetchable(offer)) = self.wgc_layers.get(&build) else {
            return;
        };
        let wanted =
            self.steam_builds.iter().any(|(path, path_build)| *path_build == build && settings.uses_wgc_login(path));
        let Some(dirs) = env.runtime_dirs.clone() else {
            return;
        };
        if !wanted || !settings.is_enabled() {
            return;
        }

        let offer = Arc::clone(offer);
        let total_bytes = offer.size();
        self.wgc_layers.insert(build, WgcLayerState::Fetching { done_bytes: 0, total_bytes });
        let proxy_url = env.proxy_url.clone();
        let sender = self.sender(env);
        let runtime = env.runtime.clone();
        // The webseed download's stream is not `Send`; it runs on a blocking-pool thread.
        env.runtime.spawn_blocking(move || {
            let control = wgc_download::DownloadControl {
                cancel: Arc::new(AtomicBool::new(false)),
                pause: Arc::new(AtomicBool::new(false)),
            };
            let progress = |done_bytes: u64, total_bytes: u64| {
                sender.send(Event::WgcLayer(build, WgcLayerState::Fetching { done_bytes, total_bytes }));
            };
            let fetched =
                runtime.block_on(LayerStore::new(&dirs).fetch(&offer, proxy_url.as_deref(), &control, progress));
            let state = match fetched {
                Ok(layer) => WgcLayerState::Ready(Arc::new(layer)),
                Err(err) => WgcLayerState::Unavailable(Arc::new(err)),
            };
            sender.send(Event::WgcLayer(build, state));
        });
    }

    /// The layer a Play of the Steam install at `path` lays over it, once Steam has finished
    /// updating it and the layer for its build is ready. Looks up the layer when Steam changed
    /// the install's build since the last lookup.
    pub(crate) fn launch_delta(
        &mut self,
        path: &Path,
        settings: &LauncherSettings,
        env: &LauncherEnv,
    ) -> Result<LaunchDelta, LaunchDeltaError> {
        let Some(GameInstall { source: InstallSource::Steam { library, .. }, .. }) = self.steam_install(path) else {
            return Err(LaunchDeltaError::NotSteamInstall { path: path.to_path_buf() });
        };
        let library = library.clone();

        settled_manifest(&library, path)?;
        let build = install_build(path)?;
        self.probe_wgc_layer(path.to_path_buf(), build, settings, env);
        let Some(WgcLayerState::Ready(layer)) = self.wgc_layers.get(&build) else {
            return Err(LaunchDeltaError::WgcLayerNotReady { build });
        };
        if !layer.covers(build) {
            return Err(LaunchDeltaError::WgcLayerNotReady { build });
        }

        Ok(LaunchDelta { layer: Arc::clone(layer), library, build })
    }

    /// Hands the Steam install at `path` to `steam -applaunch` with the user's extra args, then
    /// follows the game Steam starts from it. No account is involved.
    pub fn play_through_steam(
        &mut self,
        path: &Path,
        safe_mode: bool,
        settings: &LauncherSettings,
        env: &LauncherEnv,
        prepare: impl FnOnce(&mut std::process::Command) + Send + 'static,
    ) -> Result<(), PlayError> {
        if !self.game.is_idle() {
            return Err(PlayError::GameRunning);
        }
        let Some(GameInstall { source: InstallSource::Steam { library, .. }, .. }) = self.steam_install(path) else {
            return Err(PlayError::NotSteamInstall { path: path.to_path_buf() });
        };
        let library = library.clone();
        let steam = wgc_launch::HostSearch::from_env().steam_client().ok_or(PlayError::SteamNotFound)?;
        if !self.gate.try_activate() {
            return Err(PlayError::SavingGameSettings);
        }

        let launch = self.next_launch();
        self.game = GameState::Starting { account: None, install: path.to_path_buf() };
        let argv = steam_argv(&steam, &settings.launch_args(safe_mode).extra);
        let install = path.to_path_buf();
        let sender = self.sender(env);
        env.runtime.spawn_blocking(move || run_through_steam(launch, argv, install, library, prepare, &sender));

        Ok(())
    }
}

/// The install's appmanifest, unless it shows Steam may still change the install's files.
fn settled_manifest(library: &Path, install: &Path) -> Result<AppManifest, LaunchDeltaError> {
    // Overlayfs leaves a lower layer that changes while mounted undefined.
    let manifest = AppManifest::read(library, WOWS_STEAM_APP_ID)?;
    if let Some(update) = manifest.pending_update() {
        return Err(LaunchDeltaError::SteamUpdatePending { path: install.to_path_buf(), update });
    }

    Ok(manifest)
}

fn install_build(install: &Path) -> Result<GameBuild, LaunchDeltaError> {
    crate::wgc_layer::newest_build(install)
        .map_err(|source| LaunchDeltaError::ReadBuilds { path: install.to_path_buf(), source })?
        .ok_or_else(|| LaunchDeltaError::NoGameBuild { path: install.to_path_buf() })
}

/// `steam -applaunch 552990 <extra args>`.
fn steam_argv(steam: &Path, extra: &[String]) -> Vec<OsString> {
    let mut argv = vec![steam.as_os_str().to_owned(), "-applaunch".into(), WOWS_STEAM_APP_ID.0.to_string().into()];
    argv.extend(extra.iter().map(OsString::from));

    argv
}

/// Starts Steam and reports the game it starts from `install` until that game exits.
fn run_through_steam(
    launch: LaunchId,
    argv: Vec<OsString>,
    install: PathBuf,
    library: PathBuf,
    prepare: impl FnOnce(&mut std::process::Command),
    sender: &EventSender,
) {
    // How long Steam may take to start the game while it runs no update for it, such as while
    // it waits on its own sign-in.
    const START_TIMEOUT: Duration = Duration::from_secs(300);
    const POLL: Duration = Duration::from_secs(1);

    let (program, args) = argv.split_first().expect("the Steam argv has a program");
    let mut command = std::process::Command::new(program);
    command.args(args).stdin(std::process::Stdio::null());
    prepare(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            sender.send(Event::Game(GameEvent::Failed { launch, failure: LaunchFailure::SteamLaunch(err) }));
            return;
        }
    };
    // `steam` stays running as the client when it had to start it.
    std::thread::spawn(move || child.wait());

    let canonical = std::fs::canonicalize(&install).unwrap_or_else(|_| install.clone());
    let game = wgc_launch::ExternalGame::new(&canonical);
    let mut idle_since = Instant::now();
    loop {
        match game.is_running() {
            Ok(true) => break,
            Ok(false) => {}
            Err(err) => {
                tracing::warn!("could not look for the game Steam started: {err}");
                sender.send(Event::Game(GameEvent::Exited { launch, exit: GameExit { install, status: None } }));
                return;
            }
        }

        // An update Steam runs before it starts the game takes as long as its download, so the
        // wait restarts while the appmanifest shows Steam updating or running the app.
        if steam_is_busy(&library) {
            idle_since = Instant::now();
        }
        if idle_since.elapsed() >= START_TIMEOUT {
            tracing::warn!("Steam started no game from {} within {START_TIMEOUT:?}", install.display());
            sender.send(Event::Game(GameEvent::Exited { launch, exit: GameExit { install, status: None } }));
            return;
        }
        std::thread::sleep(POLL);
    }

    sender.send(Event::Game(GameEvent::ClientStarted { launch }));
    if let Err(err) = game.wait_exit() {
        tracing::warn!("could not follow the game Steam started: {err}");
    }
    sender.send(Event::Game(GameEvent::Exited { launch, exit: GameExit { install, status: None } }));
}

/// Whether the appmanifest in `library` shows Steam updating or running the game. An unreadable
/// one shows neither.
fn steam_is_busy(library: &Path) -> bool {
    match AppManifest::read(library, WOWS_STEAM_APP_ID) {
        Ok(manifest) => {
            manifest.pending_update().is_some() || manifest.state_flags.contains(AppStateFlags::APP_RUNNING)
        }
        Err(err) => {
            tracing::debug!("could not read the Steam appmanifest: {err}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::test_env;
    use crate::settings::AccountKey;

    /// A Steam library holding a WoWs install.
    struct SteamFixture {
        library: PathBuf,
        install: PathBuf,
    }

    impl SteamFixture {
        /// An install of `build` under `root` whose appmanifest has `state_flags`.
        fn new(root: &Path, build: u32, state_flags: u32) -> Self {
            let library = root.join("library");
            let install = library.join("steamapps/common/World of Warships");
            std::fs::create_dir_all(install.join("bin").join(build.to_string())).unwrap();
            let manifest =
                format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"552990\"\n\t\"StateFlags\"\t\t\"{state_flags}\"\n}}\n");
            std::fs::write(wgc_install::steam::app_manifest_path(&library, WOWS_STEAM_APP_ID), manifest).unwrap();

            Self { library, install }
        }

        fn game_install(&self) -> GameInstall {
            GameInstall {
                path: self.install.clone(),
                version: None,
                build: None,
                source: InstallSource::Steam { app_id: WOWS_STEAM_APP_ID, library: self.library.clone() },
                product_id: None,
                auto_update: false,
            }
        }

        /// A delta checked against `build` with a layer covering it.
        fn delta(&self, root: &Path, build: u32) -> LaunchDelta {
            let build = GameBuild::new(build);
            let layer = Arc::new(WgcLayer::for_test(root.join("layer"), build));
            LaunchDelta { layer, library: self.library.clone(), build }
        }
    }

    #[test]
    fn steam_launches_wows_with_the_extra_args() {
        let argv = steam_argv(Path::new("/usr/bin/steam"), &["-safe".to_owned(), "--no-intro".to_owned()]);

        insta::assert_debug_snapshot!(argv);
    }

    #[test]
    fn a_launch_refuses_a_build_steam_changed_after_play() {
        let tmp = tempfile::tempdir().unwrap();
        let fixture = SteamFixture::new(tmp.path(), 13533378, AppStateFlags::FULLY_INSTALLED.bits());

        let err = fixture.delta(tmp.path(), 13357625).prepare_for(&fixture.install).unwrap_err();

        assert!(matches!(
            err,
            LaunchFailure::LaunchDelta(LaunchDeltaError::BuildChanged { expected, found, .. })
                if expected == GameBuild::new(13357625) && found == GameBuild::new(13533378)
        ));
    }

    #[test]
    fn a_launch_refuses_an_install_steam_started_updating_after_play() {
        const UPDATE_STARTED: u32 = 1024;
        let tmp = tempfile::tempdir().unwrap();
        let fixture = SteamFixture::new(tmp.path(), 13533378, AppStateFlags::FULLY_INSTALLED.bits() | UPDATE_STARTED);

        let err = fixture.delta(tmp.path(), 13533378).prepare_for(&fixture.install).unwrap_err();

        assert!(matches!(err, LaunchFailure::LaunchDelta(LaunchDeltaError::SteamUpdatePending { .. })));
    }

    #[test]
    fn turning_the_login_off_while_the_game_runs_clears_the_layer_once_it_exits() {
        let tmp = tempfile::tempdir().unwrap();
        let fixture = SteamFixture::new(tmp.path(), 13533378, AppStateFlags::FULLY_INSTALLED.bits());
        let dirs = wgc_launch::RuntimeDirs::new(&tmp.path().join("app"));
        let record = dirs.install_state(&fixture.install.canonicalize().unwrap()).delta_record();
        wgc_launch::InstallView::select_delta(&fixture.install, &dirs, Some(&tmp.path().join("layer"))).unwrap();
        let mut settings = LauncherSettings::default();
        settings.enable();
        settings.set_wgc_login(&fixture.install, true);
        let (mut launcher, _events) = Launcher::new();
        launcher.installs = vec![fixture.game_install()];
        let launch = launcher.next_launch();
        launcher.game = GameState::Running { account: None, install: fixture.install.clone() };

        // Dropping a runtime waits for its blocking tasks, which clear the layer.
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = LauncherEnv { runtime_dirs: Some(dirs.clone()), ..test_env(&runtime) };
        assert!(launcher.set_wgc_login(&fixture.install, false, &mut settings, &env));
        drop(env);
        drop(runtime);
        assert!(record.exists());

        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = LauncherEnv { runtime_dirs: Some(dirs), ..test_env(&runtime) };
        let exit = GameExit { install: fixture.install.clone(), status: None };
        launcher.apply_game(GameEvent::Exited { launch, exit }, &env);
        drop(env);
        drop(runtime);
        assert!(!record.exists());
    }

    #[test]
    fn the_wgc_login_needs_a_session_that_is_or_may_be_signed_in() {
        let key = AccountKey { realm: wgc_auth::AccountRealm::Eu, account_id: wgc_auth::AccountId::new(697_769_785) };
        let mut settings = LauncherSettings::default();
        let _ = settings.upsert_account(key, wgc_auth::Nickname::new("saved"));
        let (mut launcher, _events) = Launcher::new();
        let install = Path::new("/games/World_of_Warships");

        for (session, usable) in [
            (AccountSession::NotLoaded, true),
            (AccountSession::Restoring, true),
            (AccountSession::Expired, false),
            (AccountSession::SignedOut, false),
            (AccountSession::KeyringLocked, false),
        ] {
            launcher.sessions.insert(key, session.clone());
            let availability = launcher.wgc_login_availability(install, &settings);

            assert_eq!(!matches!(availability, WgcLoginAvailability::NeedsSignIn), usable, "{session:?}");
        }
    }
}
