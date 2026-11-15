//! The Launcher tab: saved accounts, Play, and WoWs install management.

use std::path::Path;
use std::path::PathBuf;

use rust_i18n::t;
use wgc_catalog::known_games::KnownGame;
use wows_toolkit_launcher::AccountKey;
use wows_toolkit_launcher::AccountStatus;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::CatalogSummary;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::GameState;
use wows_toolkit_launcher::Launcher;
use wows_toolkit_launcher::LauncherError;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::LoginTarget;
use wows_toolkit_launcher::RunnerState;
use wows_toolkit_launcher::SessionStorage;

use crate::app::ToolkitTabViewer;
use crate::update_background_task;
use crate::util::file_dialog::DialogKind;
use crate::util::file_dialog::FileDialogSlot;

/// Fixed width of the whole download-progress block, so the row keeps its
/// size as the byte count and the in-flight file name change width.
const PROGRESS_AREA_WIDTH: f32 = 380.0;
/// Fixed width of the "X / Y" byte label within the progress area.
const BYTE_LABEL_WIDTH: f32 = 150.0;

const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(230, 180, 40);
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(230, 80, 80);
const OK_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 200, 120);

/// A download row's button press, applied after the row loop so the loop's
/// borrows of the launcher have ended.
enum DownloadRowAction {
    Cancel(PathBuf),
    Pause(PathBuf),
    Resume(PathBuf),
}

/// An account row's button press, applied after rendering.
enum AccountAction {
    MakeDefault(AccountKey),
    SignIn(AccountKey),
    SignOut(AccountKey),
    Forget(AccountKey),
    Add,
    CancelForm,
}

/// Native dialogs opened from the launcher tab.
#[derive(Default)]
pub struct LauncherDialogs {
    folder: FileDialogSlot<FolderRequest>,
    proton_dir: FileDialogSlot,
}

/// What a chosen folder from [`LauncherDialogs::folder`] is used for.
enum FolderRequest {
    AddInstall,
    Download { install: PathBuf },
    Install(KnownGame),
}

/// What the user asked for this frame, applied after rendering.
enum Intent {
    Rescan,
    AddInstall(PathBuf),
    Play,
    UseForToolkit(PathBuf),
    Recheck(PathBuf),
    Download { install: PathBuf, default_dir: PathBuf },
    StartDownload { install: PathBuf, dest_dir: PathBuf },
    Install(KnownGame),
    InstallGame { game: KnownGame, dest_dir: PathBuf },
    Row(DownloadRowAction),
    RetryAvailableGames,
    DetectRunner,
}

fn show_catalog_status(ui: &mut egui::Ui, state: Option<&CatalogCheckState>) {
    match state {
        Some(CatalogCheckState::Checking) => {
            ui.spinner();
            ui.label("checking updates");
        }
        Some(CatalogCheckState::Ready(build)) => {
            let summary = CatalogSummary::of(build);
            if summary.updates.is_empty() {
                ui.label("Up to date");
            } else {
                ui.colored_label(WARN_COLOR, format!("Update available: {}", summary.updates.join(", ")));
            }
            if !summary.not_installed.is_empty() {
                ui.weak(format!("not installed: {}", summary.not_installed.join(", ")));
            }
        }
        Some(CatalogCheckState::Error(detail)) => {
            ui.weak(format!("update check failed: {detail}"));
        }
        Some(CatalogCheckState::NoUpdateHost) | None => {}
    }
}

/// One download's status in a fixed-width block; long file names truncate
/// rather than growing the row.
fn show_download_status(ui: &mut egui::Ui, key: &Path, state: Option<&DownloadState>) -> Option<DownloadRowAction> {
    let mut action = None;
    match state {
        Some(DownloadState::Running { phase, done_bytes, total_bytes, current }) => {
            ui.allocate_ui_with_layout(
                egui::vec2(PROGRESS_AREA_WIDTH, ui.spacing().interact_size.y * 2.0 + ui.spacing().item_spacing.y),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_width(PROGRESS_AREA_WIDTH);
                    ui.horizontal(|ui| {
                        if *total_bytes > 0 {
                            let frac = (*done_bytes as f32 / *total_bytes as f32).clamp(0.0, 1.0);
                            ui.add(egui::ProgressBar::new(frac).show_percentage().desired_width(200.0));
                            let bytes_text = format!(
                                "{} / {}",
                                humansize::format_size(*done_bytes, humansize::BINARY),
                                humansize::format_size(*total_bytes, humansize::BINARY)
                            );
                            ui.add_sized(
                                egui::vec2(BYTE_LABEL_WIDTH, ui.spacing().interact_size.y),
                                egui::Label::new(bytes_text).truncate(),
                            );
                        } else {
                            ui.spinner();
                        }
                        if ui.small_button("Pause").clicked() {
                            action = Some(DownloadRowAction::Pause(key.to_path_buf()));
                        }
                        if ui.small_button("Cancel").clicked() {
                            action = Some(DownloadRowAction::Cancel(key.to_path_buf()));
                        }
                    });
                    ui.add(egui::Label::new(format!("{phase}: {current}")).truncate());
                },
            );
        }
        Some(DownloadState::Paused) => {
            ui.horizontal(|ui| {
                ui.weak("Download paused; staged files are kept.");
                if ui.small_button("Resume").clicked() {
                    action = Some(DownloadRowAction::Resume(key.to_path_buf()));
                }
            });
        }
        Some(DownloadState::Done { skipped_deltas }) => {
            if *skipped_deltas > 0 {
                ui.colored_label(
                    WARN_COLOR,
                    format!(
                        "Installed ({skipped_deltas} delta part(s) skipped: not yet supported, downloaded but not applied)"
                    ),
                );
            } else {
                ui.colored_label(OK_COLOR, "Installed");
            }
        }
        Some(DownloadState::Failed(detail)) => {
            ui.colored_label(ERROR_COLOR, format!("Download failed: {detail}"));
        }
        Some(DownloadState::Cancelled) => {
            ui.weak("Download cancelled.");
        }
        None => {}
    }
    action
}

/// A known game's installable version, from a fresh catalog check. Returns
/// whether the user asked to retry a failed check.
fn show_available_game_status(ui: &mut egui::Ui, state: Option<&CatalogCheckState>) -> bool {
    match state {
        Some(CatalogCheckState::Checking) => {
            ui.spinner();
            ui.label("checking available version");
        }
        Some(CatalogCheckState::Ready(build)) => match wows_toolkit_launcher::fresh_install_version(build) {
            Some(v) => {
                ui.label(format!("version {v}"));
            }
            None => {
                ui.weak("client version not reported");
            }
        },
        Some(CatalogCheckState::Error(detail)) => {
            ui.weak(format!("unavailable: {detail}"));
            return ui.small_button("Retry").clicked();
        }
        Some(CatalogCheckState::NoUpdateHost) | None => {}
    }
    false
}

impl ToolkitTabViewer<'_> {
    fn toast_launcher_error(&self, result: Result<(), LauncherError>) {
        if let Err(e) = result {
            self.tab_state.toasts.lock().error(e.to_string());
        }
    }

    /// Applies the result of any launcher dialog that closed since the last frame.
    pub(crate) fn poll_launcher_dialogs(&mut self) {
        if let Some((request, dir)) = self.tab_state.launcher_dialogs.folder.poll() {
            self.apply_intent(match request {
                FolderRequest::AddInstall => Intent::AddInstall(dir),
                FolderRequest::Download { install } => Intent::StartDownload { install, dest_dir: dir },
                FolderRequest::Install(game) => Intent::InstallGame { game, dest_dir: dir },
            });
        }

        let Some(proton_dir) = self.tab_state.launcher_dialogs.proton_dir.poll_path() else {
            return;
        };
        self.tab_state.launcher_op(|launcher, settings, env| {
            settings.runner = wgc_launch::RunnerChoice::SteamProton { proton_dir };
            launcher.start_runner_probe(settings, env);
            true
        });
    }

    pub fn build_launcher_tab(&mut self, ui: &mut egui::Ui) {
        self.build_account_section(ui);
        ui.separator();

        let mut intents = Vec::new();

        ui.horizontal(|ui| {
            ui.heading("Launcher");
            if ui.button("Rescan").clicked() {
                intents.push(Intent::Rescan);
            }
            let folder_dialog = &mut self.tab_state.launcher_dialogs.folder;
            if ui.add_enabled(!folder_dialog.is_pending(), egui::Button::new("Add install...")).clicked() {
                folder_dialog.open(
                    ui.ctx(),
                    DialogKind::PickFolder,
                    rfd::AsyncFileDialog::new(),
                    FolderRequest::AddInstall,
                );
            }
            if self.tab_state.launcher.is_discovering() {
                ui.spinner();
            }
        });

        self.build_play_row(ui, &mut intents);

        ui.collapsing("Launch options", |ui| self.build_launch_options(ui));

        ui.separator();

        let launcher = &self.tab_state.launcher;
        for install in launcher.installs() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(install.source.label());
                    ui.monospace(install.path.display().to_string());
                    if let Some(v) = &install.version {
                        ui.label(format!("{}.{}.{}", v.major, v.minor, v.patch));
                    } else if install.source.is_read_only_content() {
                        ui.label("managed by Steam");
                    }
                });
                if !install.source.is_read_only_content() {
                    ui.horizontal(|ui| {
                        show_catalog_status(ui, launcher.catalog_status(&install.path));
                        if ui.small_button("Recheck").clicked() {
                            intents.push(Intent::Recheck(install.path.clone()));
                        }
                    });

                    let download_state = launcher.download_state(&install.path);
                    if let Some(action) = show_download_status(ui, &install.path, download_state) {
                        intents.push(Intent::Row(action));
                    }

                    let is_active = download_state.is_some_and(DownloadState::is_active);
                    if launcher.has_downloads(&install.path) && !is_active {
                        ui.horizontal(|ui| {
                            if ui.button("Download / Update...").clicked() {
                                // An install with no detected version has no content
                                // on disk yet (e.g. a manually added empty directory),
                                // so the picker starts at the platform default.
                                let default_dir = if install.version.is_some() {
                                    install.path.clone()
                                } else {
                                    wgc_download::default_install_dir()
                                };
                                intents.push(Intent::Download { install: install.path.clone(), default_dir });
                            }
                            ui.weak(
                                "A fresh install is tens of GB; delta updates are not supported yet, so a changed part re-downloads in full.",
                            );
                        });
                    }
                }
                if self.tab_state.launcher.needs_runner_setup() {
                    ui.horizontal(|ui| {
                        ui.colored_label(ui.visuals().warn_fg_color, "Needs Proton to launch on Linux.");
                        if ui.button("Look again").clicked() {
                            intents.push(Intent::DetectRunner);
                        }
                    });
                }
                if ui.button("Use for toolkit").clicked() {
                    intents.push(Intent::UseForToolkit(install.path.clone()));
                }
            });
        }

        ui.separator();
        self.build_install_game_section(ui, &mut intents);

        for intent in intents {
            self.apply_intent(intent);
        }
    }

    fn build_launch_options(&mut self, ui: &mut egui::Ui) {
        let mut launcher = self.tab_state.persisted.read().settings.launcher.clone();
        let mut changed = false;
        let mut detect = false;

        ui.label("Extra launch args (one per line):");
        let mut args_text = launcher.extra_args_text();
        if ui.text_edit_multiline(&mut args_text).changed() {
            launcher.set_extra_args_text(&args_text);
            changed = true;
        }

        ui.label("Environment variables (KEY=VALUE, one per line):");
        let mut env_text = launcher.env_vars_text();
        if ui.text_edit_multiline(&mut env_text).changed() {
            launcher.set_env_vars_text(&env_text);
            changed = true;
        }

        ui.separator();
        ui.strong("Proton (Linux)");
        ui.weak(
            "The game runs through Steam's Proton Experimental or newest Proton when Steam is installed, else umu-run.",
        );
        match self.tab_state.launcher.runner() {
            RunnerState::Unknown => {
                ui.weak("Looking for Proton.");
            }
            RunnerState::Found(runner) => {
                ui.label(match runner.runner() {
                    wgc_launch::Runner::Native => "The game runs natively.".to_owned(),
                    wgc_launch::Runner::SteamProton { proton, .. } => format!("Steam's {}", proton.name()),
                    wgc_launch::Runner::Umu { proton: Some(proton), .. } => format!("umu-run with {}", proton.name()),
                    wgc_launch::Runner::Umu { proton: None, .. } => "umu-run with UMU-Proton".to_owned(),
                });
            }
            RunnerState::Missing(err) => {
                ui.colored_label(ui.visuals().warn_fg_color, err.to_string());
            }
        }
        ui.horizontal(|ui| {
            let proton_dialog = &mut self.tab_state.launcher_dialogs.proton_dir;
            if ui
                .add_enabled(!proton_dialog.is_pending(), egui::Button::new("Choose Proton folder...").small())
                .clicked()
            {
                proton_dialog.open(ui.ctx(), DialogKind::PickFolder, rfd::AsyncFileDialog::new(), ());
            }
            if ui
                .add_enabled(
                    launcher.runner != wgc_launch::RunnerChoice::Auto,
                    egui::Button::new("Find automatically").small(),
                )
                .clicked()
            {
                launcher.runner = wgc_launch::RunnerChoice::Auto;
                changed = true;
                detect = true;
            }
            if ui.small_button("Look again").clicked() {
                detect = true;
            }
        });

        if changed {
            self.tab_state.persisted.write().settings.launcher = launcher;
            self.tab_state.request_save();
        }
        // After the save, so the probe looks for the runner now chosen.
        if detect {
            self.apply_intent(Intent::DetectRunner);
        }
    }

    /// Lets a user download and install one of the built-in known games from
    /// scratch. WGUS serves one current build per product and no history, so
    /// there is a choice of product but not of version.
    fn build_install_game_section(&mut self, ui: &mut egui::Ui, intents: &mut Vec<Intent>) {
        // Only runs while the tab is visible; once every game has a result or
        // a check in flight this is false, so the check fires once.
        if !self.tab_state.launcher.available_games_checked() {
            self.tab_state.launcher_op(|launcher, _, env| {
                launcher.start_available_games_check(env);
                false
            });
        }

        let launcher = &self.tab_state.launcher;
        ui.collapsing("Install a game", |ui| {
            ui.weak("A fresh install is tens of GB.");
            for known in wgc_catalog::known_games::known_games() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(known.display_name);
                        if show_available_game_status(ui, launcher.available_game(known)) {
                            intents.push(Intent::RetryAvailableGames);
                        }
                    });
                    if let Some((key, state)) = launcher.installing_download(known)
                        && let Some(action) = show_download_status(ui, key, Some(state))
                    {
                        intents.push(Intent::Row(action));
                    }
                    ui.add_enabled_ui(launcher.can_install(known), |ui| {
                        if ui.button("Install...").clicked() {
                            intents.push(Intent::Install(*known));
                        }
                    });
                });
            }
        });
    }

    fn apply_intent(&mut self, intent: Intent) {
        match intent {
            Intent::Rescan => self.tab_state.launcher_op(|launcher, settings, env| {
                launcher.start_install_discovery(settings, env);
                false
            }),
            Intent::AddInstall(dir) => self.tab_state.launcher_op(|launcher, settings, env| {
                settings.manual_installs.push(dir);
                launcher.start_install_discovery(settings, env);
                true
            }),
            Intent::Play => self.play_default_account(),
            Intent::UseForToolkit(path) => {
                self.tab_state.persisted.write().settings.game.wows_dir = path.display().to_string();
                self.tab_state.revalidate_wows_dir();
                if !self.tab_state.wows_dir_invalid {
                    self.tab_state.prevent_changing_wows_dir();
                    update_background_task!(self.tab_state.background_tasks, Some(self.tab_state.load_game_data(path)));
                }
            }
            Intent::Recheck(path) => self.tab_state.launcher_op(|launcher, _, env| {
                launcher.recheck_catalog(&path, env);
                false
            }),
            Intent::Download { install, default_dir } => {
                let dialog = rfd::AsyncFileDialog::new().set_directory(&default_dir);
                let request = FolderRequest::Download { install };
                self.tab_state.launcher_dialogs.folder.open(
                    &self.tab_state.egui_ctx,
                    DialogKind::PickFolder,
                    dialog,
                    request,
                );
            }
            Intent::StartDownload { install, dest_dir } => {
                let mut result = Ok(());
                self.tab_state.launcher_op(|launcher, _, env| {
                    result = launcher.start_download(install, dest_dir, env);
                    false
                });
                self.toast_launcher_error(result);
            }
            Intent::Install(game) => {
                let dialog = rfd::AsyncFileDialog::new().set_directory(wgc_download::default_install_dir());
                let request = FolderRequest::Install(game);
                self.tab_state.launcher_dialogs.folder.open(
                    &self.tab_state.egui_ctx,
                    DialogKind::PickFolder,
                    dialog,
                    request,
                );
            }
            Intent::InstallGame { game, dest_dir } => {
                let mut result = Ok(());
                self.tab_state.launcher_op(|launcher, _, env| {
                    result = launcher.install_known_game(&game, dest_dir, env);
                    false
                });
                self.toast_launcher_error(result);
            }
            Intent::Row(DownloadRowAction::Cancel(key)) => self.tab_state.launcher.cancel_download(&key),
            Intent::Row(DownloadRowAction::Pause(key)) => self.tab_state.launcher.pause_download(&key),
            Intent::Row(DownloadRowAction::Resume(key)) => {
                let mut result = Ok(());
                self.tab_state.launcher_op(|launcher, _, env| {
                    result = launcher.resume_download(&key, env);
                    false
                });
                self.toast_launcher_error(result);
            }
            Intent::RetryAvailableGames => self.tab_state.launcher_op(|launcher, _, env| {
                launcher.start_available_games_check(env);
                false
            }),
            Intent::DetectRunner => self.tab_state.launcher_op(|launcher, settings, env| {
                launcher.start_runner_probe(settings, env);
                false
            }),
        }
    }

    /// Play for the default account, the Safe Mode toggle, and the running game.
    fn build_play_row(&mut self, ui: &mut egui::Ui, intents: &mut Vec<Intent>) {
        let default_account = self.tab_state.persisted.read().settings.launcher.default_account().cloned();
        let launcher = &self.tab_state.launcher;
        ui.horizontal(|ui| {
            match &default_account {
                Some(account) => {
                    let ready = matches!(launcher.account_status(account.key), AccountStatus::Ready(_));
                    let enabled = ready && launcher.game_state().is_idle();
                    let button = egui::Button::new(format!("Play as {}", account.nickname.as_str()));
                    if ui.add_enabled(enabled, button).clicked() {
                        intents.push(Intent::Play);
                    }
                }
                None => {
                    ui.add_enabled(false, egui::Button::new("Play"));
                    ui.weak("Sign in to an account to play.");
                }
            }
            ui.checkbox(&mut self.tab_state.launch_safe_mode, "Safe mode (disable mods)");
        });
        match launcher.game_state() {
            GameState::Idle => {}
            GameState::Starting { install, .. } => {
                ui.weak(format!("Game starting from {}", install.display()));
            }
            GameState::Running { install, .. } => {
                ui.weak(format!("Game running from {}", install.display()));
            }
        }
    }

    /// The saved accounts with their session status, and the sign-in form.
    fn build_account_section(&mut self, ui: &mut egui::Ui) {
        let mut actions = Vec::new();
        let mut sign_in = false;
        let mut submit_code = false;
        let mut dismiss_captcha = false;

        let settings = self.tab_state.persisted.read().settings.launcher.clone();
        let launcher = &self.tab_state.launcher;
        let form = &mut self.tab_state.login_form;
        if form.show && launcher.login_finished().is_some() {
            form.show = false;
        }
        ui.group(|ui| {
            ui.heading("Accounts");
            if settings.accounts.is_empty() {
                ui.weak("No saved accounts.");
            }
            for account in &settings.accounts {
                let key = account.key;
                ui.horizontal(|ui| {
                    let is_default = settings.default_account == Some(key);
                    if ui.radio(is_default, account.nickname.as_str()).on_hover_text("Play uses this account").clicked()
                        && !is_default
                    {
                        actions.push(AccountAction::MakeDefault(key));
                    }
                    ui.label(wows_toolkit_launcher::realm_label(key.realm));
                    let status = launcher.account_status(key);
                    show_account_status(ui, launcher, key, status);
                    match status {
                        AccountStatus::Ready(_) => {
                            if ui.small_button("Sign out").clicked() {
                                actions.push(AccountAction::SignOut(key));
                            }
                        }
                        AccountStatus::Expired | AccountStatus::SignedOut | AccountStatus::StoreUnavailable => {
                            if ui.small_button("Sign in").clicked() {
                                actions.push(AccountAction::SignIn(key));
                            }
                        }
                        AccountStatus::Restoring => {}
                    }
                    if ui.small_button("Remove").clicked() {
                        actions.push(AccountAction::Forget(key));
                    }
                });
            }
            ui.separator();

            if launcher.two_factor_pending() {
                ui.label("Two-factor code required.");
                let mut code_entered = false;
                ui.horizontal(|ui| {
                    let field = ui.add(egui::TextEdit::singleline(&mut form.two_factor_code).hint_text("code"));
                    code_entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.checkbox(&mut form.use_backup_code, "Use backup code");
                });
                if let Some(err) = launcher.login_error() {
                    ui.colored_label(ERROR_COLOR, err);
                }
                let can_submit = !launcher.login_in_progress() && !form.two_factor_code.trim().is_empty();
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(can_submit, |ui| {
                        if ui.button("Submit code").clicked() {
                            submit_code = true;
                        }
                    });
                    if can_submit && code_entered {
                        submit_code = true;
                    }
                    if launcher.login_in_progress() {
                        ui.spinner();
                    }
                });
            } else if launcher.captcha_blocked() {
                ui.colored_label(
                    WARN_COLOR,
                    "This login needs a CAPTCHA, which this launcher can't show yet. Sign in once via the official Wargaming Game Center or wargaming.net in a browser, then try again.",
                );
                if ui.button("Try again").clicked() {
                    dismiss_captcha = true;
                }
            } else if !form.show {
                if ui.button("Add account").clicked() {
                    actions.push(AccountAction::Add);
                }
            } else {
                let target = launcher.login_target();
                match target {
                    LoginTarget::NewAccount => {
                        ui.strong("Add account");
                    }
                    LoginTarget::Reauthenticate(key) => {
                        let name = settings.account(key).map_or("", |account| account.nickname.as_str());
                        ui.strong(format!("Sign in again: {name}"));
                    }
                }
                let mut credentials_entered = false;
                ui.horizontal(|ui| {
                    // A reauthentication signs in to the account's own realm.
                    ui.add_enabled_ui(target == LoginTarget::NewAccount, |ui| {
                        egui::ComboBox::from_id_salt("launcher_account_realm")
                            .selected_text(wows_toolkit_launcher::realm_label(form.realm))
                            .show_ui(ui, |ui| {
                                for realm in wows_toolkit_launcher::REALMS {
                                    ui.selectable_value(
                                        &mut form.realm,
                                        realm,
                                        wows_toolkit_launcher::realm_label(realm),
                                    );
                                }
                            });
                    });
                    ui.label(t!("ui.launcher.account.email"));
                    let field = ui.text_edit_singleline(&mut form.username);
                    credentials_entered |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                });
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    let field = ui.add(egui::TextEdit::singleline(&mut form.password).password(true));
                    credentials_entered |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                });
                if let Some(err) = launcher.login_error() {
                    ui.colored_label(ERROR_COLOR, err);
                }
                let can_sign_in = !launcher.login_in_progress()
                    && wows_toolkit_launcher::is_plausible_email(&form.username)
                    && !form.password.is_empty();
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(can_sign_in, |ui| {
                        if ui.button("Sign in").clicked() {
                            sign_in = true;
                        }
                    });
                    if can_sign_in && credentials_entered {
                        sign_in = true;
                    }
                    if ui.button("Cancel").clicked() {
                        actions.push(AccountAction::CancelForm);
                    }
                    if launcher.login_in_progress() {
                        ui.spinner();
                    }
                });
            }
        });

        if sign_in {
            // Taken out of the non-zeroizing form field the moment it is used.
            let password = wgc_auth::SecretPassword::new(std::mem::take(&mut self.tab_state.login_form.password));
            let realm = self.tab_state.login_form.realm;
            let username = self.tab_state.login_form.username.trim().to_string();
            self.tab_state.launcher_op(|launcher, settings, env| {
                launcher.start_login(launcher.login_target(), realm, username, password, settings, env)
            });
        }
        if submit_code {
            let form = &mut self.tab_state.login_form;
            let code = std::mem::take(&mut form.two_factor_code);
            let code = if form.use_backup_code {
                wgc_auth::TwoFactorCode::Backup(code)
            } else {
                wgc_auth::TwoFactorCode::Otp(code)
            };
            self.tab_state.launcher_op(|launcher, settings, env| launcher.submit_two_factor(code, settings, env));
        }
        if dismiss_captcha {
            self.tab_state.launcher.dismiss_captcha();
            self.tab_state.login_form.show = true;
        }
        for action in actions {
            self.apply_account_action(action, &settings);
        }
    }

    fn apply_account_action(&mut self, action: AccountAction, settings: &LauncherSettings) {
        match action {
            AccountAction::MakeDefault(key) => {
                if self.tab_state.persisted.write().settings.launcher.set_default_account(key) {
                    self.tab_state.request_save();
                }
            }
            AccountAction::SignIn(key) => {
                self.tab_state.launcher.reset_login(LoginTarget::Reauthenticate(key));
                let form = &mut self.tab_state.login_form;
                form.realm = key.realm;
                form.show = true;
            }
            AccountAction::SignOut(key) => self.tab_state.launcher_op(|launcher, settings, env| {
                launcher.sign_out(key, settings, env);
                false
            }),
            AccountAction::Forget(key) => {
                self.tab_state.launcher_op(|launcher, settings, env| launcher.forget(key, settings, env));
            }
            AccountAction::Add => {
                self.tab_state.launcher.reset_login(LoginTarget::NewAccount);
                let form = &mut self.tab_state.login_form;
                if let Some(realm) = settings.sign_in_realm {
                    form.realm = realm;
                }
                form.show = true;
            }
            AccountAction::CancelForm => {
                self.tab_state.launcher.reset_login(LoginTarget::NewAccount);
                let form = &mut self.tab_state.login_form;
                form.password.clear();
                form.show = false;
            }
        }
    }

    fn play_default_account(&mut self) {
        let (default_account, wows_dir) = {
            let persisted = self.tab_state.persisted.read();
            (persisted.settings.launcher.default_account, persisted.settings.game.wows_dir.clone())
        };
        let Some(key) = default_account else {
            return;
        };
        let safe_mode = self.tab_state.launch_safe_mode;

        let mut played = Ok(());
        self.tab_state.launcher_op(|launcher, settings, env| {
            // An empty `wows_dir` means the app has no game directory set.
            let wows_dir = (!wows_dir.is_empty()).then(|| Path::new(&wows_dir));
            played = launcher.play(key, wows_dir, safe_mode, settings, env, |command| {
                crate::hardening::prepare_child(command, crate::gpu::PIN_VARS);
            });
            false
        });
        if let Err(e) = played {
            self.tab_state.toasts.lock().error(format!("Launch failed: {e}"));
        }
    }
}

/// One account's session status, with its expiry and storage when signed in.
fn show_account_status(ui: &mut egui::Ui, launcher: &Launcher, key: AccountKey, status: AccountStatus) {
    match status {
        AccountStatus::Restoring => {
            ui.spinner();
            ui.weak("restoring session");
        }
        AccountStatus::Ready(storage) => {
            let expiry = launcher.session(key).map(|session| &session.expiry);
            match expiry {
                Some(wgc_auth::TokenExpiry::KnownAt(expires_at)) => {
                    match expires_at.duration_since(std::time::SystemTime::now()) {
                        Ok(remaining) => {
                            ui.label(format!(
                                "Signed in (expires in {})",
                                wows_toolkit_launcher::format_duration(remaining)
                            ));
                        }
                        Err(_) => {
                            ui.colored_label(ERROR_COLOR, "Session expired - sign in again");
                        }
                    }
                }
                Some(wgc_auth::TokenExpiry::Unknown) | None => {
                    ui.label("Signed in");
                }
            }
            if storage == SessionStorage::MemoryOnly {
                ui.weak("not saved to keyring; you'll sign in again next launch");
            }
        }
        AccountStatus::Expired => {
            ui.colored_label(ERROR_COLOR, "Session expired - sign in again");
        }
        AccountStatus::SignedOut => {
            ui.weak("Signed out");
        }
        AccountStatus::StoreUnavailable => {
            ui.colored_label(WARN_COLOR, "Keyring unavailable");
        }
    }
}
