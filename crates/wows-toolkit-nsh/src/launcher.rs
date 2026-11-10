//! The Launcher tab's service: install discovery, catalog checks, downloads,
//! WGNI sign-in, Proton setup and launching, over
//! `wows_toolkit_launcher::Launcher`. Background work runs on the app's tokio
//! runtime and reports as `LauncherEvents`, which [`LauncherSession::run`]
//! applies on the engine thread and hands to the view as [`LauncherState`].

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use rust_i18n::t;
use wgc_install::GameInstall;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::CatalogSummary;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::Launcher;
use wows_toolkit_launcher::LauncherEnv;
use wows_toolkit_launcher::LauncherError;
use wows_toolkit_launcher::LauncherEvent;
use wows_toolkit_launcher::LauncherEvents;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::proton::Severity;
use wows_toolkit_viewmodel::settings::keys;

use crate::runtime;
use crate::settings_store;

/// What the Launcher tab shows.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct LauncherState {
    /// Whether the launcher's settings have been read. Nothing else is shown
    /// before.
    pub loaded: bool,
    /// Whether an install scan is running.
    pub discovering: bool,
    pub installs: Vec<InstallRow>,
    pub account: AccountState,
    pub options: LaunchOptions,
    /// The games that can be installed fresh, in the catalog's order.
    pub games: Vec<GameRow>,
}

/// One install.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct InstallRow {
    /// The install's path, which names it in the row's actions.
    pub key: String,
    /// `WGC`, `Steam` or `Manual`.
    pub source: String,
    pub path: String,
    pub version: Option<String>,
    /// Steam installs are Steam's to update; no checks or downloads.
    pub managed_by_steam: bool,
    pub catalog: Option<StatusLine>,
    pub download: Option<DownloadRow>,
    /// Whether the catalog has something to download and nothing is running.
    pub can_download: bool,
    /// Whether launching needs Proton set up first.
    pub needs_proton: bool,
}

/// One game the catalog can install fresh.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct GameRow {
    /// Its place in the catalog's list, which names it in the row's actions.
    pub index: u32,
    pub name: String,
    pub version: Option<StatusLine>,
    /// Whether its check failed and may be retried.
    pub retry: bool,
    pub download: Option<DownloadRow>,
    pub can_install: bool,
}

/// A line of status: what it says and how it is colored.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StatusLine {
    pub text: String,
    /// `normal`, `dim`, `success`, `warning` or `danger`.
    pub tone: String,
    /// Whether work is under way, for a spinner.
    pub busy: bool,
    /// A second, quieter line.
    pub detail: Option<String>,
}

/// A download's progress, or how it ended.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct DownloadRow {
    /// Names the download in pause, cancel and resume.
    pub key: String,
    /// `running`, `paused` or `ended`.
    pub phase: String,
    /// Done over total, once the total is known.
    pub fraction: Option<f64>,
    /// `done / total` in bytes, once the total is known.
    pub sizes: Option<String>,
    /// What it is doing, or how it ended.
    pub status: StatusLine,
}

/// The account section.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct AccountState {
    /// `signedIn`, `twoFactor`, `captcha` or `signedOut`.
    pub kind: String,
    pub nickname: Option<String>,
    /// `Region: EU`, signed in.
    pub region: Option<String>,
    /// When the session expires, signed in.
    pub expiry: Option<StatusLine>,
    /// Signed in, but the session is not in the OS keyring.
    pub not_persisted: bool,
    /// Why the last sign-in or code failed.
    pub error: Option<String>,
    pub in_progress: bool,
}

/// What every launch is given.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct LaunchOptions {
    /// Extra args, one per line.
    pub extra_args: String,
    /// `KEY=VALUE`, one per line.
    pub env_vars: String,
    pub proton_binary: Option<String>,
    pub proton_prefix: Option<String>,
}

/// The result of a Proton auto-detect.
#[derive(Debug, Clone, PartialEq, JsObject)]
#[non_exhaustive]
pub struct ProtonOutcome {
    pub state: LauncherState,
    pub message: String,
    /// `success`, `warning` or `error`.
    pub level: String,
}

/// The Launcher tab's state and the work behind it.
#[class]
pub struct LauncherSession {
    launcher: Launcher,
    /// Taken by [`LauncherSession::run`], which drains them.
    events: Option<LauncherEvents>,
    /// `None` until the settings row has been read.
    settings: Option<LauncherSettings>,
    /// The resolved proxy the launcher's requests use.
    proxy_url: Option<String>,
}

#[class]
impl LauncherSession {
    /// A launcher with nothing read yet; `run` reads its settings.
    pub fn new() -> Self {
        let (launcher, events) = Launcher::new();
        Self { launcher, events: Some(events), settings: None, proxy_url: None }
    }

    /// The realms the sign-in form offers, by label, in order.
    pub fn realms() -> Vec<String> {
        wows_toolkit_launcher::REALMS.iter().map(|realm| wows_toolkit_launcher::realm_label(*realm).to_owned()).collect()
    }

    /// Reads the settings, restores the saved session and scans for
    /// installs, then calls `listener` after every change background work
    /// makes. Runs while the app does.
    pub async fn run(this: Instance<Self>, #[args(state)] listener: Callback<LauncherState>) -> Result<(), String> {
        let Some(mut events) = this.with_mut(|session| session.events.take()) else {
            return Err("the launcher is already running".to_owned());
        };

        let proxy = match settings_store::load::<Option<String>>(keys::PROXY_URL).await {
            Ok(proxy) => proxy.flatten(),
            Err(error) => {
                log::warn!("launcher: {error}");
                None
            }
        };
        // No row is a launcher never configured, for which the defaults are
        // right; an unreadable row starts from the defaults too, as the egui
        // and GPUI apps do.
        let settings = match settings_store::load::<LauncherSettings>(wows_toolkit_launcher::SETTINGS_KEY).await {
            Ok(settings) => settings.unwrap_or_default(),
            Err(error) => {
                log::warn!("launcher: {error}");
                LauncherSettings::default()
            }
        };
        let state = this.with_mut(|session| {
            session.set_proxy(proxy.unwrap_or_default());
            session.adopt_settings(settings);
            session.state()
        });
        listener.post(state);

        while let Some(event) = events.next().await {
            let state = this.with_mut(|session| {
                session.apply(event);
                session.state()
            });
            listener.post(state);
        }

        Ok(())
    }

    /// The tab now.
    pub fn state(&self) -> LauncherState {
        let installs = self.launcher.installs();
        let Some(settings) = &self.settings else {
            return LauncherState {
                loaded: false,
                discovering: self.launcher.is_discovering(),
                installs: Vec::new(),
                account: self.account(),
                options: LaunchOptions {
                    extra_args: String::new(),
                    env_vars: String::new(),
                    proton_binary: None,
                    proton_prefix: None,
                },
                games: Vec::new(),
            };
        };

        LauncherState {
            loaded: true,
            discovering: self.launcher.is_discovering(),
            installs: installs.iter().map(|install| self.install_row(install, settings)).collect(),
            account: self.account(),
            options: LaunchOptions {
                extra_args: settings.extra_args_text(),
                env_vars: settings.env_vars_text(),
                proton_binary: settings.proton_binary.as_deref().map(display_path),
                proton_prefix: settings.proton_prefix.as_deref().map(display_path),
            },
            games: self.game_rows(),
        }
    }

    /// The proxy the launcher's own requests go through, from the app's
    /// setting. Empty is no manual proxy.
    pub fn set_proxy(&mut self, proxy_url: String) {
        self.proxy_url = wows_toolkit_viewmodel::proxy::resolve_proxy(Some(&proxy_url)).map(|proxy| proxy.url);
    }

    /// Scans for installs again.
    pub fn rescan(&mut self) -> LauncherState {
        self.op(|launcher, settings, env| {
            launcher.start_install_discovery(settings, env);
            false
        });
        self.state()
    }

    /// Checks the catalog for the install at `key` again.
    pub fn recheck(&mut self, key: String) -> LauncherState {
        if let Some(path) = self.install_path(&key) {
            self.op(|launcher, _, env| {
                launcher.recheck_catalog(&path, env);
                false
            });
        }
        self.state()
    }

    /// Checks what each known game would install, for those not checked yet
    /// or whose check failed.
    pub fn check_games(&mut self) -> LauncherState {
        self.op(|launcher, _, env| {
            launcher.start_available_games_check(env);
            false
        });
        self.state()
    }

    /// Asks the download under `key` to pause.
    pub fn pause(&mut self, key: String) -> LauncherState {
        if let Some(key) = self.download_key(&key) {
            self.launcher.pause_download(&key);
        }
        self.state()
    }

    /// Asks the download under `key` to stop.
    pub fn cancel(&mut self, key: String) -> LauncherState {
        if let Some(key) = self.download_key(&key) {
            self.launcher.cancel_download(&key);
        }
        self.state()
    }

    /// Resumes the paused download under `key`.
    pub fn resume(&mut self, key: String) -> Result<LauncherState, String> {
        let key = self.download_key(&key).ok_or_else(|| LauncherError::NothingToResume.to_string())?;
        self.try_op(|launcher, env| launcher.resume_download(&key, env))?;

        Ok(self.state())
    }

    /// Replaces the extra launch args from one-per-line text.
    pub fn set_extra_args(&mut self, text: String) -> LauncherState {
        self.edit_settings(|settings| settings.set_extra_args_text(&text));
        self.state()
    }

    /// Replaces the launch environment from `KEY=VALUE`-per-line text.
    pub fn set_env_vars(&mut self, text: String) -> LauncherState {
        self.edit_settings(|settings| settings.set_env_vars_text(&text));
        self.state()
    }

    /// Fills in the Proton binary and prefix from the local Steam install.
    pub fn autodetect_proton(&mut self) -> ProtonOutcome {
        let storage = wows_toolkit_config::storage_dir();
        let result = match self.settings.as_mut() {
            Some(settings) => wows_toolkit_launcher::proton::autodetect(settings, storage.as_deref()),
            None => {
                return ProtonOutcome {
                    state: self.state(),
                    message: t!("ui.launcher.loading").into_owned(),
                    level: "warning".to_owned(),
                };
            }
        };
        if result.changed() {
            self.save();
        }
        let level = match result.severity() {
            Severity::Success => "success",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };

        ProtonOutcome { state: self.state(), message: result.to_string(), level: level.to_owned() }
    }

    /// Picks the Proton binary.
    pub async fn browse_proton_binary(this: Instance<Self>) -> Result<LauncherState, String> {
        let dialog = OpenDialog::files().title(t!("ui.launcher.proton.binary").into_owned());
        if let Some(path) = pick_one(dialog).await? {
            this.with_mut(|session| session.edit_settings(|settings| settings.proton_binary = Some(path)));
        }

        Ok(this.with(LauncherSession::state))
    }

    /// Picks the Proton prefix.
    pub async fn browse_proton_prefix(this: Instance<Self>) -> Result<LauncherState, String> {
        let dialog = OpenDialog::folders().title(t!("ui.launcher.proton.prefix").into_owned());
        if let Some(path) = pick_one(dialog).await? {
            this.with_mut(|session| session.edit_settings(|settings| settings.proton_prefix = Some(path)));
        }

        Ok(this.with(LauncherSession::state))
    }

    /// Picks an install directory to add, and scans again.
    pub async fn add_install(this: Instance<Self>) -> Result<LauncherState, String> {
        let dialog = OpenDialog::folders().title(t!("ui.launcher.add_install_title").into_owned());
        if let Some(dir) = pick_one(dialog).await? {
            this.with_mut(|session| {
                session.op(|launcher, settings, env| {
                    settings.manual_installs.push(dir);
                    launcher.start_install_discovery(settings, env);
                    true
                });
            });
        }

        Ok(this.with(LauncherSession::state))
    }

    /// Picks where to put the newest build of the install at `key`, and
    /// downloads it there.
    pub async fn download(this: Instance<Self>, key: String) -> Result<LauncherState, String> {
        let Some(install) = this.with(|session| session.install(&key).cloned()) else {
            return Ok(this.with(LauncherSession::state));
        };
        // An install with no detected version has no content on disk yet, so
        // the picker starts at the platform default.
        let start = if install.version.is_some() { install.path.clone() } else { wgc_download::default_install_dir() };
        let dialog = OpenDialog::folders().title(t!("ui.launcher.download_title").into_owned()).directory(start);
        let Some(dest) = pick_one(dialog).await? else {
            return Ok(this.with(LauncherSession::state));
        };
        this.with_mut(|session| session.try_op(|launcher, env| launcher.start_download(install.path, dest, env)))?;

        Ok(this.with(LauncherSession::state))
    }

    /// Picks where to install the known game at `index`, and installs it.
    pub async fn install_game(this: Instance<Self>, index: u32) -> Result<LauncherState, String> {
        let Some(game) = known_game(index) else {
            return Err(format!("no known game at {index}"));
        };
        let dialog = OpenDialog::folders()
            .title(t!("ui.launcher.install_title").into_owned())
            .directory(wgc_download::default_install_dir());
        let Some(dest) = pick_one(dialog).await? else {
            return Ok(this.with(LauncherSession::state));
        };
        this.with_mut(|session| session.try_op(|launcher, env| launcher.install_known_game(game, dest, env)))?;

        Ok(this.with(LauncherSession::state))
    }

    /// Starts the install at `key` for online play.
    pub fn launch(&self, key: String, safe_mode: bool) -> Result<(), String> {
        let (Some(settings), Some(install)) = (&self.settings, self.install(&key)) else {
            return Err(t!("ui.launcher.launch_failed", error = key).into_owned());
        };
        self.launcher
            .launch(install, settings, safe_mode, |command| {
                wows_toolkit_hardening::prepare_child(command, &[]);
            })
            .map_err(|error| t!("ui.launcher.launch_failed", error = error.to_string()).into_owned())
    }

    /// Signs in to the realm at `realm` of [`LauncherSession::realms`].
    pub fn sign_in(&mut self, realm: u32, username: String, password: String) -> Result<LauncherState, String> {
        let Some(realm) = usize::try_from(realm).ok().and_then(|index| wows_toolkit_launcher::REALMS.get(index)) else {
            return Err(format!("no realm at {realm}"));
        };
        let realm = *realm;
        let username = username.trim().to_owned();
        let password = wgc_auth::SecretPassword::new(password);
        self.op(|launcher, settings, env| launcher.start_login(realm, username, password, settings, env));

        Ok(self.state())
    }

    /// Answers the two-factor challenge with `code`, a backup code when
    /// `backup` is set.
    pub fn submit_code(&mut self, code: String, backup: bool) -> LauncherState {
        let code = code.trim().to_owned();
        let code = if backup { wgc_auth::TwoFactorCode::Backup(code) } else { wgc_auth::TwoFactorCode::Otp(code) };
        self.op(|launcher, settings, env| launcher.submit_two_factor(code, settings, env));
        self.state()
    }

    /// Signs out and forgets the stored session.
    pub fn sign_out(&mut self) -> LauncherState {
        self.op(|launcher, settings, env| launcher.sign_out(settings, env));
        self.state()
    }

    /// Leaves the CAPTCHA notice for the sign-in form.
    pub fn dismiss_captcha(&mut self) -> LauncherState {
        self.launcher.dismiss_captcha();
        self.state()
    }

    fn adopt_settings(&mut self, settings: LauncherSettings) {
        self.settings = Some(settings);
        self.op(|launcher, settings, env| {
            launcher.start_session_restore(settings, env);
            launcher.start_install_discovery(settings, env);
            false
        });
    }

    fn apply(&mut self, event: LauncherEvent) {
        self.op(|launcher, settings, env| launcher.apply(event, settings, env));
    }

    fn env(&self) -> Option<LauncherEnv> {
        let runtime = match runtime::shared() {
            Ok(runtime) => runtime,
            Err(error) => {
                log::warn!("launcher: {error}");
                return None;
            }
        };

        Some(LauncherEnv {
            runtime: runtime.handle().clone(),
            proxy_url: self.proxy_url.clone(),
            // Events are awaited by `run`, which needs no other wake-up.
            wake: Arc::new(|| {}),
        })
    }

    /// Runs a launcher operation against the settings, saving them when it
    /// reports a change. Nothing before the settings are read.
    fn op(&mut self, op: impl FnOnce(&mut Launcher, &mut LauncherSettings, &LauncherEnv) -> bool) {
        let Some(env) = self.env() else {
            return;
        };
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        if op(&mut self.launcher, settings, &env) {
            self.save();
        }
    }

    /// Runs an operation that can refuse to start, with its reason.
    fn try_op(&mut self, op: impl FnOnce(&mut Launcher, &LauncherEnv) -> Result<(), LauncherError>) -> Result<(), String> {
        let env = self.env().ok_or_else(|| t!("ui.collab.no_runtime").into_owned())?;

        op(&mut self.launcher, &env).map_err(|error| error.to_string())
    }

    fn edit_settings(&mut self, edit: impl FnOnce(&mut LauncherSettings)) {
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        edit(settings);
        self.save();
    }

    fn save(&self) {
        if let Some(settings) = &self.settings {
            settings_store::save(wows_toolkit_launcher::SETTINGS_KEY, settings);
        }
    }

    fn install(&self, key: &str) -> Option<&GameInstall> {
        self.launcher.installs().iter().find(|install| display_path(&install.path) == key)
    }

    fn install_path(&self, key: &str) -> Option<PathBuf> {
        self.install(key).map(|install| install.path.clone())
    }

    /// The download a row's `key` names: an install's update, or a known
    /// game's fresh install.
    fn download_key(&self, key: &str) -> Option<PathBuf> {
        let updates = self.launcher.installs().iter().map(|install| install.path.as_path());
        let fresh = wgc_catalog::known_games::known_games()
            .iter()
            .filter_map(|game| self.launcher.installing_download(game).map(|(path, _)| path));

        updates.chain(fresh).find(|path| display_path(path) == key).map(Path::to_path_buf)
    }

    fn account(&self) -> AccountState {
        let launcher = &self.launcher;
        let error = launcher.login_error().map(str::to_owned);
        let in_progress = launcher.login_in_progress();
        let signed_out = |kind: &str| AccountState {
            kind: kind.to_owned(),
            nickname: None,
            region: None,
            expiry: None,
            not_persisted: false,
            error: error.clone(),
            in_progress,
        };

        if let Some(session) = launcher.session() {
            let expiry = match session.expiry {
                wgc_auth::TokenExpiry::KnownAt(at) => match at.duration_since(std::time::SystemTime::now()) {
                    Ok(remaining) => line(
                        t!(
                            "ui.launcher.account.expires_in",
                            duration = wows_toolkit_launcher::format_duration(remaining)
                        ),
                        Tone::Dim,
                    ),
                    Err(_) => line(t!("ui.launcher.account.expired"), Tone::Danger),
                },
                wgc_auth::TokenExpiry::Unknown => line(t!("ui.launcher.account.expiry_unknown"), Tone::Dim),
            };
            return AccountState {
                kind: "signedIn".to_owned(),
                nickname: Some(session.nickname.as_str().to_owned()),
                region: Some(
                    t!("ui.launcher.account.region", region = wows_toolkit_launcher::realm_label(session.realm))
                        .into_owned(),
                ),
                expiry: Some(expiry),
                not_persisted: !launcher.session_persisted(),
                error: None,
                in_progress,
            };
        }
        if launcher.two_factor_pending() {
            return signed_out("twoFactor");
        }
        if launcher.captcha_blocked() {
            return signed_out("captcha");
        }

        signed_out("signedOut")
    }

    fn install_row(&self, install: &GameInstall, settings: &LauncherSettings) -> InstallRow {
        let launcher = &self.launcher;
        let managed_by_steam = install.source.is_read_only_content();
        let version = match &install.version {
            Some(version) => Some(format!("{}.{}.{}", version.major, version.minor, version.patch)),
            None if managed_by_steam => Some(t!("ui.launcher.managed_by_steam").into_owned()),
            None => None,
        };
        let state = launcher.download_state(&install.path);
        let (catalog, download, can_download) = if managed_by_steam {
            (None, None, false)
        } else {
            let can_download =
                launcher.has_downloads(&install.path) && !state.is_some_and(DownloadState::is_active);
            (
                catalog_line(launcher.catalog_status(&install.path)),
                state.map(|state| download_row(&install.path, state)),
                can_download,
            )
        };

        InstallRow {
            key: display_path(&install.path),
            source: install.source.label().to_owned(),
            path: display_path(&install.path),
            version,
            managed_by_steam,
            catalog,
            download,
            can_download,
            needs_proton: wows_toolkit_launcher::proton::needs_setup(install, settings),
        }
    }

    fn game_rows(&self) -> Vec<GameRow> {
        let launcher = &self.launcher;
        let games = wgc_catalog::known_games::known_games();

        games
            .iter()
            .enumerate()
            .map(|(index, game)| {
                let checked = launcher.available_game(game);
                let version = match checked {
                    Some(CatalogCheckState::Checking) => {
                        Some(StatusLine { busy: true, ..line(t!("ui.launcher.checking_version"), Tone::Dim) })
                    }
                    Some(CatalogCheckState::Ready(build)) => Some(match wows_toolkit_launcher::fresh_install_version(build) {
                        Some(version) => line(t!("ui.launcher.version", version = version), Tone::Normal),
                        None => line(t!("ui.launcher.version_unreported"), Tone::Dim),
                    }),
                    Some(CatalogCheckState::Error(detail)) => {
                        Some(line(t!("ui.launcher.unavailable", error = detail), Tone::Dim))
                    }
                    Some(CatalogCheckState::NoUpdateHost) | None => None,
                };

                GameRow {
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                    name: game.display_name.to_owned(),
                    version,
                    retry: matches!(checked, Some(CatalogCheckState::Error(_))),
                    download: launcher.installing_download(game).map(|(key, state)| download_row(key, state)),
                    can_install: launcher.can_install(game),
                }
            })
            .collect()
    }
}

impl Default for LauncherSession {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum Tone {
    Normal,
    Dim,
    Success,
    Warning,
    Danger,
}

impl Tone {
    fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Dim => "dim",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
        }
    }
}

fn line(text: impl Into<String>, tone: Tone) -> StatusLine {
    StatusLine { text: text.into(), tone: tone.name().to_owned(), busy: false, detail: None }
}

fn catalog_line(state: Option<&CatalogCheckState>) -> Option<StatusLine> {
    match state? {
        CatalogCheckState::Checking => {
            Some(StatusLine { busy: true, ..line(t!("ui.launcher.checking_updates"), Tone::Dim) })
        }
        CatalogCheckState::Ready(build) => {
            let summary = CatalogSummary::of(build);
            let headline = if summary.updates.is_empty() {
                line(t!("ui.launcher.up_to_date"), Tone::Normal)
            } else {
                line(t!("ui.launcher.update_available", parts = summary.updates.join(", ")), Tone::Warning)
            };
            let detail = (!summary.not_installed.is_empty())
                .then(|| t!("ui.launcher.not_installed", parts = summary.not_installed.join(", ")).into_owned());

            Some(StatusLine { detail, ..headline })
        }
        CatalogCheckState::Error(detail) => Some(line(t!("ui.launcher.check_failed", error = detail), Tone::Dim)),
        CatalogCheckState::NoUpdateHost => None,
    }
}

fn download_row(key: &Path, state: &DownloadState) -> DownloadRow {
    let key = display_path(key);
    let ended = |status: StatusLine| DownloadRow {
        key: key.clone(),
        phase: "ended".to_owned(),
        fraction: None,
        sizes: None,
        status,
    };

    match state {
        DownloadState::Running { phase, done_bytes, total_bytes, current } => {
            let known = *total_bytes > 0;
            DownloadRow {
                key,
                phase: "running".to_owned(),
                fraction: known.then(|| (*done_bytes as f64 / *total_bytes as f64).clamp(0.0, 1.0)),
                sizes: known.then(|| {
                    format!(
                        "{} / {}",
                        humansize::format_size(*done_bytes, humansize::BINARY),
                        humansize::format_size(*total_bytes, humansize::BINARY)
                    )
                }),
                status: StatusLine { busy: !known, ..line(format!("{phase}: {current}"), Tone::Normal) },
            }
        }
        DownloadState::Paused => DownloadRow {
            key,
            phase: "paused".to_owned(),
            fraction: None,
            sizes: None,
            status: line(t!("ui.launcher.paused"), Tone::Dim),
        },
        DownloadState::Done { skipped_deltas: 0 } => ended(line(t!("ui.launcher.installed"), Tone::Success)),
        DownloadState::Done { skipped_deltas } => {
            ended(line(t!("ui.launcher.installed_skipped", count = skipped_deltas), Tone::Warning))
        }
        DownloadState::Failed(detail) => ended(line(t!("ui.launcher.download_failed", error = detail), Tone::Danger)),
        DownloadState::Cancelled => ended(line(t!("ui.launcher.cancelled"), Tone::Dim)),
    }
}

fn known_game(index: u32) -> Option<&'static wgc_catalog::known_games::KnownGame> {
    wgc_catalog::known_games::known_games().get(usize::try_from(index).ok()?)
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

/// The one path a dialog answered, `None` when cancelled.
async fn pick_one(dialog: OpenDialog) -> Result<Option<PathBuf>, String> {
    let picked = pick_paths(dialog).await?;

    Ok(picked.and_then(|paths| paths.into_iter().next()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (LauncherSession, tokio::runtime::Runtime) {
        // Never driven: what an operation spawns stays queued, so nothing
        // reaches the network and the events below decide the state.
        let runtime = tokio::runtime::Builder::new_current_thread().build().expect("a runtime");
        let (launcher, _events) = Launcher::new();
        let session = LauncherSession {
            launcher,
            events: None,
            settings: Some(LauncherSettings::default()),
            proxy_url: None,
        };

        (session, runtime)
    }

    fn apply(session: &mut LauncherSession, runtime: &tokio::runtime::Runtime, event: LauncherEvent) {
        let env = LauncherEnv { runtime: runtime.handle().clone(), proxy_url: None, wake: Arc::new(|| {}) };
        let Some(settings) = session.settings.as_mut() else { panic!("the settings are read") };
        let _ = session.launcher.apply(event, settings, &env);
    }

    fn start_login(session: &mut LauncherSession, runtime: &tokio::runtime::Runtime) {
        let env = LauncherEnv { runtime: runtime.handle().clone(), proxy_url: None, wake: Arc::new(|| {}) };
        let Some(settings) = session.settings.as_mut() else { panic!("the settings are read") };
        let _ = session.launcher.start_login(
            wgc_auth::AccountRealm::Eu,
            "user".into(),
            wgc_auth::SecretPassword::new("password".into()),
            settings,
            &env,
        );
    }

    #[test]
    fn nothing_is_shown_before_the_settings_are_read() {
        let (launcher, _events) = Launcher::new();
        let session = LauncherSession { launcher, events: None, settings: None, proxy_url: None };

        let state = session.state();

        assert!(!state.loaded);
        assert!(state.installs.is_empty());
    }

    #[test]
    fn a_manual_install_offers_its_path_and_no_steam_note() {
        let (mut session, runtime) = session();
        let path = PathBuf::from("/games/wows");
        apply(&mut session, &runtime, LauncherEvent::installs(vec![GameInstall::manual(path.clone())]));

        let state = session.state();
        let [row] = &state.installs[..] else { panic!("one install") };

        assert_eq!(row.source, "Manual");
        assert_eq!(row.key, "/games/wows");
        assert!(!row.managed_by_steam);
        assert!(row.version.is_none());
        assert_eq!(row.needs_proton, cfg!(target_os = "linux"));
    }

    #[test]
    fn a_failed_check_is_reported_dim() {
        let (mut session, runtime) = session();
        let path = PathBuf::from("/games/wows");
        apply(&mut session, &runtime, LauncherEvent::installs(vec![GameInstall::manual(path.clone())]));
        apply(&mut session, &runtime, LauncherEvent::catalog(path, CatalogCheckState::Error("offline".into())));

        let state = session.state();
        let catalog = state.installs[0].catalog.as_ref().expect("a check result");

        assert_eq!(catalog.tone, "dim");
        assert!(catalog.text.contains("offline"), "{}", catalog.text);
        assert!(!state.installs[0].can_download);
    }

    #[test]
    fn a_running_download_reports_its_progress() {
        let (mut session, runtime) = session();
        let path = PathBuf::from("/games/wows");
        apply(&mut session, &runtime, LauncherEvent::installs(vec![GameInstall::manual(path.clone())]));
        apply(
            &mut session,
            &runtime,
            LauncherEvent::download(
                path,
                DownloadState::Running {
                    phase: wows_toolkit_launcher::DownloadPhase::Downloading,
                    done_bytes: 1024,
                    total_bytes: 4096,
                    current: "client.pkg".into(),
                },
            ),
        );

        let state = session.state();
        let download = state.installs[0].download.as_ref().expect("a download");

        assert_eq!(download.phase, "running");
        assert_eq!(download.fraction, Some(0.25));
        assert_eq!(download.sizes.as_deref(), Some("1 KiB / 4 KiB"));
        assert_eq!(download.status.text, "Downloading: client.pkg");
        assert_eq!(session.download_key(&download.key), Some(PathBuf::from("/games/wows")));
    }

    #[test]
    fn a_paused_and_a_finished_download_say_so() {
        let (mut session, runtime) = session();
        let path = PathBuf::from("/games/wows");
        apply(&mut session, &runtime, LauncherEvent::installs(vec![GameInstall::manual(path.clone())]));

        apply(&mut session, &runtime, LauncherEvent::download(path.clone(), DownloadState::Paused));
        assert_eq!(session.state().installs[0].download.as_ref().map(|row| row.phase.as_str()), Some("paused"));

        apply(&mut session, &runtime, LauncherEvent::download(path.clone(), DownloadState::Done { skipped_deltas: 2 }));
        let state = session.state();
        let download = state.installs[0].download.as_ref().expect("a download");
        assert_eq!(download.phase, "ended");
        assert_eq!(download.status.tone, "warning");

        apply(&mut session, &runtime, LauncherEvent::download(path, DownloadState::Failed("disk full".into())));
        let state = session.state();
        assert_eq!(state.installs[0].download.as_ref().map(|row| row.status.tone.as_str()), Some("danger"));
    }

    #[test]
    fn a_sign_in_waits_on_its_code_and_says_why_a_code_failed() {
        let (mut session, runtime) = session();
        start_login(&mut session, &runtime);
        assert!(session.state().account.in_progress);

        apply(&mut session, &runtime, LauncherEvent::two_factor_required("token".into()));
        assert_eq!(session.state().account.kind, "twoFactor");

        apply(&mut session, &runtime, LauncherEvent::two_factor_retry("wrong code".into()));
        let account = session.state().account;
        assert_eq!(account.kind, "twoFactor");
        assert_eq!(account.error.as_deref(), Some("wrong code"));
    }

    #[test]
    fn a_captcha_holds_the_form_until_dismissed() {
        let (mut session, runtime) = session();
        start_login(&mut session, &runtime);
        apply(&mut session, &runtime, LauncherEvent::captcha_required());
        assert_eq!(session.state().account.kind, "captcha");

        let state = session.dismiss_captcha();
        assert_eq!(state.account.kind, "signedOut");
    }

    #[test]
    fn a_signed_in_account_shows_its_name_region_and_keyring_note() {
        let (mut session, runtime) = session();
        start_login(&mut session, &runtime);
        let stored = wgc_auth::StoredSession {
            realm: wgc_auth::AccountRealm::Na,
            account_id: wgc_auth::AccountId::new(1),
            nickname: wgc_auth::Nickname::new("Captain"),
            tokens: wgc_auth::TokenPair {
                access_token: wgc_auth::AccessToken::new("token".into()),
                exchange_code: wgc_auth::ExchangeCode::from_raw("code".into()),
            },
            expiry: wgc_auth::TokenExpiry::Unknown,
        };
        apply(&mut session, &runtime, LauncherEvent::signed_in(stored, false));

        let account = session.state().account;
        assert_eq!(account.kind, "signedIn");
        assert_eq!(account.nickname.as_deref(), Some("Captain"));
        assert_eq!(account.region.as_deref(), Some("Region: NA"));
        assert!(account.not_persisted);
        assert_eq!(account.expiry.map(|expiry| expiry.tone), Some("dim".to_owned()));
    }

    #[test]
    fn every_known_game_is_listed_with_its_check() {
        let (mut session, runtime) = session();
        let games = wgc_catalog::known_games::known_games();
        let Some(first) = games.first() else { panic!("the catalog knows a game") };
        apply(&mut session, &runtime, LauncherEvent::available_game(first, CatalogCheckState::Error("offline".into())));

        let state = session.state();

        assert_eq!(state.games.len(), games.len());
        assert!(state.games[0].retry);
        assert!(!state.games[0].can_install);
    }
}
