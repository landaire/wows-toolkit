//! The game session the launcher started: Play for an account, or a replay, and its lifetime.

use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;

use wgc_install::GameInstall;

use crate::accounts::AccountSession;
use crate::error::PlayError;
use crate::error::ReplayLaunchError;
use crate::launcher::Event;
use crate::launcher::Launcher;
use crate::launcher::LauncherEnv;
use crate::settings::AccountKey;
use crate::settings::LauncherSettings;

/// A game build number: the `bin/<build>` directory of an install, and the build a replay was
/// recorded with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GameBuild(u32);

impl GameBuild {
    pub fn new(build: u32) -> GameBuild {
        GameBuild(build)
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for GameBuild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The game the launcher started, if any. One client runs at a time: the runner holds a global
/// mutex while the game runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameState {
    Idle,
    /// Spawned; the client has not signed in yet. `account` is `None` for a replay.
    Starting {
        account: Option<AccountKey>,
        install: PathBuf,
    },
    /// Signed in and in port, or playing a replay.
    Running {
        account: Option<AccountKey>,
        install: PathBuf,
    },
}

impl GameState {
    pub fn is_idle(&self) -> bool {
        matches!(self, GameState::Idle)
    }

    /// The install the game runs from, unless idle.
    pub fn install(&self) -> Option<&Path> {
        match self {
            GameState::Idle => None,
            GameState::Starting { install, .. } | GameState::Running { install, .. } => Some(install),
        }
    }

    pub fn account(&self) -> Option<AccountKey> {
        match self {
            GameState::Idle => None,
            GameState::Starting { account, .. } | GameState::Running { account, .. } => *account,
        }
    }
}

/// Whether the launcher's game is stopped, shared with work off the UI thread. Starting the game
/// takes it for writing; a game settings save holds it for reading, so Play cannot start the game
/// in the middle of a save.
#[derive(Debug, Clone)]
pub struct GameGate(Arc<RwLock<GameActivityState>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameActivityState {
    Stopped,
    Active,
}

impl GameGate {
    pub(crate) fn new() -> GameGate {
        GameGate(Arc::new(RwLock::new(GameActivityState::Stopped)))
    }

    /// Marks the game active, unless a save holds the gate. Returns whether it did.
    fn try_activate(&self) -> bool {
        let Ok(mut state) = self.0.try_write() else {
            return false;
        };
        *state = GameActivityState::Active;

        true
    }

    fn stop(&self) {
        // A poisoned lock still holds the state; the panic that poisoned it is reported where it
        // happened.
        let mut state = self.0.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        *state = GameActivityState::Stopped;
    }
}

/// Keeps the game from starting while it lives.
#[derive(Debug)]
pub struct GameHold<'gate>(
    #[allow(dead_code, reason = "held for its lock, never read")] RwLockReadGuard<'gate, GameActivityState>,
);

impl wows_game_settings::GameActivity for GameGate {
    type Hold<'state> = GameHold<'state>;

    fn hold_stopped(&self) -> Option<Self::Hold<'_>> {
        let state = self.0.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        (*state == GameActivityState::Stopped).then_some(GameHold(state))
    }
}

/// How the last game session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameExit {
    pub install: PathBuf,
    /// `None` when the process could not be waited on.
    pub status: Option<std::process::ExitStatus>,
}

/// Names one launch, so an exit reported for an older session is ignored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LaunchId(u64);

pub(crate) enum GameEvent {
    Exited { launch: LaunchId, exit: GameExit },
}

impl Launcher {
    pub fn game_state(&self) -> &GameState {
        &self.game
    }

    /// The gate a game settings save checks and holds.
    pub fn game_gate(&self) -> GameGate {
        self.gate.clone()
    }

    /// How the last game session ended, until the next one starts.
    pub fn last_exit(&self) -> Option<&GameExit> {
        self.last_exit.as_ref()
    }

    /// The install `key` plays, given the app's game directory.
    pub fn play_target(
        &self,
        key: AccountKey,
        settings: &LauncherSettings,
        wows_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        settings.account(key)?.install.resolve(wows_dir)
    }

    /// Starts the game for `key` from the install it plays, signed in as that account.
    /// `prepare` decorates the command before it spawns.
    pub fn play(
        &mut self,
        key: AccountKey,
        wows_dir: Option<&Path>,
        safe_mode: bool,
        settings: &LauncherSettings,
        env: &LauncherEnv,
        prepare: impl FnOnce(&mut std::process::Command),
    ) -> Result<(), PlayError> {
        if !self.game.is_idle() {
            return Err(PlayError::GameRunning);
        }
        let Some(account) = settings.account(key) else {
            return Err(PlayError::UnknownAccount { key });
        };
        let AccountSession::SignedIn { session, .. } = self.account_session(key) else {
            return Err(PlayError::NotSignedIn { key, status: self.account_status(key) });
        };
        let Some(path) = account.install.resolve(wows_dir) else {
            return Err(PlayError::NoInstall { key });
        };
        if self.download_state(&path).is_some_and(|state| state.is_active()) {
            return Err(PlayError::InstallBusy { path });
        }

        let install = self.install_at(&path);
        let args = settings.launch_args(safe_mode);
        let resolved = wgc_launch::resolve(&install, wgc_launch::LaunchMode::Online, &args)?;
        let status = wgc_auth::launch_token_status(Some(session), std::time::SystemTime::now());
        if !self.gate.try_activate() {
            return Err(PlayError::SavingGameSettings);
        }
        let child = match resolved.spawn(settings.proton_config().as_ref(), status, prepare) {
            Ok(child) => child,
            Err(err) => {
                self.gate.stop();
                return Err(err.into());
            }
        };

        let launch = self.next_launch();
        self.game = GameState::Starting { account: Some(key), install: path.clone() };
        self.watch_exit(launch, path, child, env);

        Ok(())
    }

    /// Opens `replay` in the game, from an install that has `build` when it is known, else from
    /// the app's game directory. Replay playback is offline and needs no account.
    pub fn open_replay(
        &mut self,
        replay: PathBuf,
        build: Option<GameBuild>,
        wows_dir: Option<&Path>,
        settings: &LauncherSettings,
        env: &LauncherEnv,
        prepare: impl FnOnce(&mut std::process::Command),
    ) -> Result<(), ReplayLaunchError> {
        if !self.game.is_idle() {
            return Err(ReplayLaunchError::GameRunning);
        }
        let path = match build {
            Some(build) => {
                self.replay_install(build, settings, wows_dir).ok_or(ReplayLaunchError::NoInstallForBuild { build })?
            }
            None => wows_dir.map(Path::to_path_buf).ok_or(ReplayLaunchError::NoInstall)?,
        };

        let install = self.install_at(&path);
        let mode = wgc_launch::LaunchMode::Replay { path: replay };
        let resolved = wgc_launch::resolve(&install, mode, &settings.launch_args(false))?;
        if !self.gate.try_activate() {
            return Err(ReplayLaunchError::SavingGameSettings);
        }
        let spawned =
            resolved.spawn(settings.proton_config().as_ref(), wgc_launch::TokenStatus::NotConfigured, prepare);
        let child = match spawned {
            Ok(child) => child,
            Err(err) => {
                self.gate.stop();
                return Err(err.into());
            }
        };

        let launch = self.next_launch();
        self.game = GameState::Running { account: None, install: path.clone() };
        self.watch_exit(launch, path, child, env);

        Ok(())
    }

    /// The first install with `build` among the app's game directory, the accounts' installs and
    /// the discovered ones, in that order.
    pub fn replay_install(
        &self,
        build: GameBuild,
        settings: &LauncherSettings,
        wows_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        let accounts = settings.accounts.iter().filter_map(|account| account.install.resolve(wows_dir));
        let discovered = self.installs().iter().map(|install| install.path.clone());
        wows_dir
            .map(Path::to_path_buf)
            .into_iter()
            .chain(accounts)
            .chain(discovered)
            .find(|path| has_build(path, build))
    }

    pub(crate) fn apply_game(&mut self, event: GameEvent) {
        match event {
            GameEvent::Exited { launch, exit } => {
                if launch != self.launch {
                    return;
                }
                self.game = GameState::Idle;
                self.gate.stop();
                let install = exit.install.clone();
                self.last_exit = Some(exit);
                // The game may have updated itself; its catalog state is stale.
                self.catalog.remove(&install);
            }
        }
    }

    fn next_launch(&mut self) -> LaunchId {
        self.launch = LaunchId(self.launch.0 + 1);
        self.last_exit = None;

        self.launch
    }

    fn install_at(&self, path: &Path) -> GameInstall {
        self.installs()
            .iter()
            .find(|install| install.path == path)
            .cloned()
            .unwrap_or_else(|| GameInstall::manual(path.to_path_buf()))
    }

    fn watch_exit(&self, launch: LaunchId, install: PathBuf, mut child: std::process::Child, env: &LauncherEnv) {
        let sender = self.sender(env);
        env.runtime.spawn_blocking(move || {
            let status = match child.wait() {
                Ok(status) => Some(status),
                Err(e) => {
                    tracing::warn!("could not wait for the game process: {e}");
                    None
                }
            };
            sender.send(Event::Game(GameEvent::Exited { launch, exit: GameExit { install, status } }));
        });
    }
}

/// Whether the install at `path` keeps `bin/<build>`.
fn has_build(path: &Path, build: GameBuild) -> bool {
    path.join("bin").join(build.to_string()).is_dir()
}
