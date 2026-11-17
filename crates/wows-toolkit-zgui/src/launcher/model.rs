//! The launcher's state and the settings it runs on, as one handle the shell and its surfaces
//! share.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use wows_toolkit_launcher::AccountKey;
use wows_toolkit_launcher::Launcher;
use wows_toolkit_launcher::LauncherEnv;
use wows_toolkit_launcher::LauncherEvent;
use wows_toolkit_launcher::LauncherEvents;
use wows_toolkit_launcher::LauncherSettings;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;

use crate::services::Services;

/// What the launcher surfaces show.
pub struct LauncherState {
    pub launcher: Launcher,
    /// `None` until the settings row has been read, which needs the database.
    pub settings: Option<LauncherSettings>,
}

impl LauncherState {
    /// Whether the settings are read and the user opted in.
    pub fn is_enabled(&self) -> bool {
        self.settings.as_ref().is_some_and(LauncherSettings::is_enabled)
    }
}

/// Which account a Play press names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayOrigin {
    /// The Play control and its popover, which play whichever account is the default.
    Default,
    /// An account the user picked by name, as the palette's "Play as" does.
    Chosen,
}

/// A Play press that waits for its account's saved session to be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingPlay {
    pub account: AccountKey,
    pub safe_mode: bool,
    pub origin: PlayOrigin,
}

impl PendingPlay {
    /// Whether the press still stands: the launcher is on and its account is kept, and still the
    /// default when it was pressed as the default.
    pub fn holds(&self, state: &LauncherState) -> bool {
        let Some(settings) = state.settings.as_ref().filter(|_| state.is_enabled()) else { return false };
        settings.accounts.iter().any(|account| account.key == self.account)
            && match self.origin {
                PlayOrigin::Default => settings.default_account == Some(self.account),
                PlayOrigin::Chosen => true,
            }
    }
}

/// The shell's launcher. A `Copy` handle.
#[derive(Clone, Copy)]
pub struct LauncherModel {
    state: RwSignal<LauncherState, LocalStorage>,
    /// The resolved proxy the launcher's own requests go through. `None` is a direct connection.
    proxy: StoredValue<Option<String>>,
    /// Whether discovery and the catalog checks have been started.
    working: StoredValue<bool>,
    pending_play: RwSignal<Option<PendingPlay>, LocalStorage>,
    services: Services,
}

impl LauncherModel {
    /// A launcher with nothing found yet, and the events its background work reports through.
    pub fn new(services: Services) -> (Self, LauncherEvents) {
        let (launcher, events) = Launcher::new();
        let model = Self {
            state: RwSignal::new_local(LauncherState { launcher, settings: None }),
            proxy: StoredValue::new(None),
            working: StoredValue::new(false),
            pending_play: RwSignal::new_local(None),
            services,
        };

        (model, events)
    }

    /// Reads the state, subscribing to every change.
    pub fn with<R>(self, read: impl FnOnce(&LauncherState) -> R) -> R {
        self.state.with(read)
    }

    /// [`Self::with`] without subscribing.
    pub fn with_untracked<R>(self, read: impl FnOnce(&LauncherState) -> R) -> R {
        self.state.with_untracked(read)
    }

    /// Whether the user opted in, subscribing to changes.
    pub fn is_enabled(self) -> bool {
        self.with(LauncherState::is_enabled)
    }

    /// The Play press waiting on a session read, subscribing to changes.
    pub fn pending_play(self) -> Option<PendingPlay> {
        self.pending_play.get()
    }

    /// Keeps a Play press until its session is read; the Play control then plays it.
    pub fn defer_play(self, pending: PendingPlay) {
        self.pending_play.set(Some(pending));
    }

    pub fn clear_pending_play(self) {
        self.pending_play.set(None);
    }

    /// The app's game directory, which an account's [`InstallChoice::Toolkit`] plays. Subscribes
    /// to the app's settings.
    ///
    /// [`InstallChoice::Toolkit`]: wows_toolkit_launcher::InstallChoice::Toolkit
    pub fn wows_dir(self) -> Option<PathBuf> {
        self.services
            .settings
            .read(|settings| settings.wows_dir.clone())
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
    }

    /// [`Self::wows_dir`] without subscribing.
    pub fn wows_dir_untracked(self) -> Option<PathBuf> {
        self.services
            .settings
            .read_untracked(|settings| settings.wows_dir.clone())
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
    }

    /// Adopts the app's proxy setting. Empty is no manual proxy, which leaves the environment's.
    pub fn set_proxy(self, proxy_url: &str) {
        self.proxy.set_value(wows_toolkit_viewmodel::proxy::resolve_proxy(Some(proxy_url)).map(|proxy| proxy.url));
    }

    /// Takes the stored settings, and starts the launcher's work when the user opted in.
    pub fn adopt(self, settings: LauncherSettings) {
        self.state.update(|state| state.settings = Some(settings));
        self.start_work();
    }

    /// Takes settings without starting any work, as a test does before it sets the state.
    #[cfg(test)]
    pub fn adopt_quietly(self, settings: LauncherSettings) {
        self.working.set_value(true);
        self.state.update(|state| state.settings = Some(settings));
    }

    /// Looks for installs and the runner and follows a game session an earlier run left behind,
    /// once, while the launcher is enabled. Sessions wait for [`Self::ensure_sessions`], since
    /// reading them may prompt to unlock the system keyring.
    fn start_work(self) {
        if self.working.get_value() || !self.with_untracked(LauncherState::is_enabled) {
            return;
        }
        self.working.set_value(true);

        self.op(|launcher, settings, env| {
            launcher.start_session_adoption(env);
            launcher.start_install_discovery(settings, env);
            launcher.start_runner_probe(settings, env);
            launcher.start_available_games_check(env);
            false
        });
    }

    /// Reads the saved sessions not read yet, once something needs one. Nothing happens while
    /// the launcher is off.
    pub fn ensure_sessions(self) {
        if !self.with_untracked(LauncherState::is_enabled) {
            return;
        }

        self.op(|launcher, settings, env| launcher.ensure_sessions(settings, env));
    }

    /// Turns the launcher on, after the user accepted the notice, and starts its work.
    pub fn enable(self) {
        self.edit_settings(LauncherSettings::enable);
        self.start_work();
    }

    /// Turns the launcher off. Work already running finishes and its results are kept; nothing
    /// new starts until it is turned on again, which starts the work afresh.
    pub fn disable(self) {
        self.clear_pending_play();
        self.working.set_value(false);
        self.edit_settings(LauncherSettings::disable);
    }

    /// Runs `operation` against the launcher and the settings, and saves the settings when it
    /// reports a change. `None`, and nothing run, while the launcher is off, before the settings
    /// are read or without a runtime to run background work on.
    pub fn run<R>(
        self,
        operation: impl FnOnce(&mut Launcher, &mut LauncherSettings, &LauncherEnv) -> (R, bool),
    ) -> Option<R> {
        if !self.with_untracked(LauncherState::is_enabled) {
            return None;
        }

        self.run_ungated(operation)
    }

    /// [`Self::run`] for an operation that returns only whether the settings changed.
    pub fn op(self, operation: impl FnOnce(&mut Launcher, &mut LauncherSettings, &LauncherEnv) -> bool) {
        self.run(|launcher, settings, env| ((), operation(launcher, settings, env)));
    }

    /// Hands one result of background work to the launcher, which keeps it while the launcher
    /// is off too.
    pub fn apply(self, event: LauncherEvent) {
        self.run_ungated(|launcher, settings, env| ((), launcher.apply(event, settings, env)));
    }

    /// [`Self::run`] whether or not the launcher is on.
    fn run_ungated<R>(
        self,
        operation: impl FnOnce(&mut Launcher, &mut LauncherSettings, &LauncherEnv) -> (R, bool),
    ) -> Option<R> {
        let env = self.env()?;
        let mut outcome = None;
        let mut changed = None;
        self.state.update(|state| {
            let Some(settings) = state.settings.as_mut() else { return };
            let (result, settings_changed) = operation(&mut state.launcher, settings, &env);
            outcome = Some(result);
            if settings_changed {
                changed = Some(settings.clone());
            }
        });

        if let Some(settings) = changed {
            self.save(&settings);
        }

        outcome
    }

    /// Changes the settings and saves them. Nothing happens before they are read.
    pub fn edit_settings(self, edit: impl FnOnce(&mut LauncherSettings)) {
        let mut edited = None;
        self.state.update(|state| {
            let Some(settings) = state.settings.as_mut() else { return };
            edit(settings);
            edited = Some(settings.clone());
        });

        if let Some(settings) = edited {
            self.save(&settings);
        }
    }

    /// Changes the launcher with no settings or background work involved.
    pub fn update_launcher(self, change: impl FnOnce(&mut Launcher)) {
        self.state.update(|state| change(&mut state.launcher));
    }

    /// Asks the download under `key` to pause.
    pub fn pause_download(self, key: &Path) {
        self.state.with_untracked(|state| state.launcher.pause_download(key));
    }

    /// Asks the download under `key` to stop.
    pub fn cancel_download(self, key: &Path) {
        self.state.with_untracked(|state| state.launcher.cancel_download(key));
    }

    fn env(self) -> Option<LauncherEnv> {
        let Some(runtime) = self.services.runtime() else {
            tracing::warn!("launcher: tokio runtime is not available");
            return None;
        };

        let storage_dir = wows_toolkit_config::storage_dir();
        Some(LauncherEnv {
            runtime: runtime.handle().clone(),
            proxy_url: self.proxy.get_value(),
            // The shell's event task awaits the channel, and its wake-up is the channel's own.
            wake: Arc::new(|| {}),
            runtime_dirs: storage_dir.as_deref().map(wgc_launch::RuntimeDirs::new),
            #[cfg(not(test))]
            credentials: wgc_auth::CredentialStore::new(storage_dir.as_deref()),
            #[cfg(test)]
            credentials: wgc_auth::CredentialStore::with_backends(crate::launcher::testing::store(), None),
        })
    }

    /// Changes which runner plays the game, and looks for the one now chosen.
    pub fn set_runner(self, choice: wgc_launch::RunnerChoice) {
        self.run(|launcher, settings, env| {
            let changed = settings.runner != choice;
            settings.runner = choice;
            launcher.start_runner_probe(settings, env);
            ((), changed)
        });
    }

    /// Looks for the chosen runner again, after the user installed or removed one.
    pub fn probe_runner(self) {
        self.op(|launcher, settings, env| {
            launcher.start_runner_probe(settings, env);
            false
        });
    }

    /// Why the last launch failed, once.
    pub fn take_launch_failure(self) -> Option<wows_toolkit_launcher::LaunchFailure> {
        if self.with_untracked(|state| state.launcher.launch_failure().is_none()) {
            return None;
        }

        let mut failure = None;
        self.update_launcher(|launcher| failure = launcher.take_launch_failure());

        failure
    }

    fn save(self, settings: &LauncherSettings) {
        self.services.settings.save(wows_toolkit_launcher::SETTINGS_KEY, settings);
    }
}
