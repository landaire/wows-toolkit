//! The launcher's state and the settings it runs on, as one handle the tab's views share.

use std::path::Path;
use std::sync::Arc;

use wgc_install::GameInstall;
use wows_toolkit_launcher::Launcher;
use wows_toolkit_launcher::LauncherEnv;
use wows_toolkit_launcher::LauncherEvent;
use wows_toolkit_launcher::LauncherEvents;
use wows_toolkit_launcher::LauncherSettings;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;

use crate::services::Services;

/// What the tab shows.
pub struct LauncherState {
    pub launcher: Launcher,
    /// `None` until the settings row has been read, which needs the database.
    pub settings: Option<LauncherSettings>,
}

/// The tab's launcher, shared by its views. A `Copy` handle.
#[derive(Clone, Copy)]
pub struct LauncherModel {
    state: RwSignal<LauncherState, LocalStorage>,
    /// The resolved proxy the launcher's own requests go through. `None` is a direct connection.
    proxy: StoredValue<Option<String>>,
    services: Services,
}

impl LauncherModel {
    /// A launcher with nothing found yet, and the events its background work reports through.
    pub fn new(services: Services) -> (Self, LauncherEvents) {
        let (launcher, events) = Launcher::new();
        let model = Self {
            state: RwSignal::new_local(LauncherState { launcher, settings: None }),
            proxy: StoredValue::new(None),
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

    /// Adopts the app's proxy setting. Empty is no manual proxy, which leaves the environment's.
    pub fn set_proxy(self, proxy_url: &str) {
        self.proxy.set_value(wows_toolkit_viewmodel::proxy::resolve_proxy(Some(proxy_url)).map(|proxy| proxy.url));
    }

    /// Takes the stored settings, then restores the signed-in session and looks for installs.
    pub fn adopt(self, settings: LauncherSettings) {
        self.state.update(|state| state.settings = Some(settings));
        self.op(|launcher, settings, env| {
            launcher.start_session_restore(settings, env);
            launcher.start_install_discovery(settings, env);
            false
        });
    }

    /// Takes settings without starting any work, as a test does before it sets the state.
    #[cfg(test)]
    pub fn adopt_quietly(self, settings: LauncherSettings) {
        self.state.update(|state| state.settings = Some(settings));
    }

    /// Runs `operation` against the launcher and the settings, and saves the settings when it
    /// reports a change. `None`, and nothing run, before the settings are read or without a
    /// runtime to run background work on.
    pub fn run<R>(
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

    /// [`Self::run`] for an operation that returns only whether the settings changed.
    pub fn op(self, operation: impl FnOnce(&mut Launcher, &mut LauncherSettings, &LauncherEnv) -> bool) {
        self.run(|launcher, settings, env| ((), operation(launcher, settings, env)));
    }

    /// Hands one result of background work to the launcher.
    pub fn apply(self, event: LauncherEvent) {
        self.op(|launcher, settings, env| launcher.apply(event, settings, env));
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

    /// The install at `path`, as the last discovery found it.
    pub fn install(self, path: &Path) -> Option<GameInstall> {
        self.state
            .with_untracked(|state| state.launcher.installs().iter().find(|install| install.path == path).cloned())
    }

    fn env(self) -> Option<LauncherEnv> {
        let Some(runtime) = self.services.runtime() else {
            tracing::warn!("launcher: tokio runtime is not available");
            return None;
        };

        Some(LauncherEnv {
            runtime: runtime.handle().clone(),
            proxy_url: self.proxy.get_value(),
            // The tab's event task awaits the channel, and its wake-up is the channel's own.
            wake: Arc::new(|| {}),
        })
    }

    fn save(self, settings: &LauncherSettings) {
        self.services.settings.save(wows_toolkit_launcher::SETTINGS_KEY, settings);
    }
}
