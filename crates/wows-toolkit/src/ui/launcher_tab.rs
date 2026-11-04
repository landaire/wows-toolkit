//! The Launcher tab: discover, launch, and manage WoWs installs.

use std::path::Path;
use std::path::PathBuf;

use wgc_catalog::known_games::KnownGame;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::CatalogSummary;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::Launcher;
use wows_toolkit_launcher::LauncherError;
use wows_toolkit_launcher::proton::ProtonSetup;
use wows_toolkit_launcher::proton::Severity;

use crate::app::ToolkitTabViewer;
use crate::update_background_task;

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

/// What the user asked for this frame, applied after rendering.
enum Intent {
    Rescan,
    AddInstall(PathBuf),
    Launch(usize),
    UseForToolkit(PathBuf),
    Recheck(PathBuf),
    Download { install: PathBuf, default_dir: PathBuf },
    Install(KnownGame),
    Row(DownloadRowAction),
    RetryAvailableGames,
    SetUpProton,
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
    fn toast_proton_setup(&self, result: ProtonSetup) {
        let mut toasts = self.tab_state.toasts.lock();
        match result.severity() {
            Severity::Success => toasts.success(result.to_string()),
            Severity::Warning => toasts.warning(result.to_string()),
            Severity::Error => toasts.error(result.to_string()),
        };
    }

    fn toast_launcher_error(&self, result: Result<(), LauncherError>) {
        if let Err(e) = result {
            self.tab_state.toasts.lock().error(e.to_string());
        }
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
            if ui.button("Add install...").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                intents.push(Intent::AddInstall(dir));
            }
            if self.tab_state.launcher.is_discovering() {
                ui.spinner();
            }
        });

        ui.checkbox(&mut self.tab_state.launch_safe_mode, "Safe mode (disable mods)");

        ui.collapsing("Launch options", |ui| self.build_launch_options(ui));

        ui.separator();

        let launcher = &self.tab_state.launcher;
        let settings = self.tab_state.persisted.read().settings.launcher.clone();
        for (i, install) in launcher.installs().iter().enumerate() {
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
                if wows_toolkit_launcher::proton::needs_setup(install, &settings) {
                    ui.horizontal(|ui| {
                        ui.colored_label(ui.visuals().warn_fg_color, "Needs Proton to launch on Linux.");
                        if ui.button("Set up Proton").clicked() {
                            intents.push(Intent::SetUpProton);
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Launch").clicked() {
                        intents.push(Intent::Launch(i));
                    }
                    if ui.button("Use for toolkit").clicked() {
                        intents.push(Intent::UseForToolkit(install.path.clone()));
                    }
                });
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
            "A non-Steam install launches through this Proton binary and prefix. Steam installs launch through Steam and ignore these. Auto-detect fills both in; the prefix is a toolkit-managed one Proton sets up on first launch unless you already have WoWs on Steam.",
        );

        ui.horizontal(|ui| {
            ui.label("Binary:");
            match &launcher.proton_binary {
                Some(path) => {
                    ui.add(egui::Label::new(path.display().to_string()).truncate());
                }
                None => {
                    ui.weak("not set");
                }
            }
            if ui.small_button("Browse...").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                launcher.proton_binary = Some(path);
                changed = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("Prefix:");
            match &launcher.proton_prefix {
                Some(path) => {
                    ui.add(egui::Label::new(path.display().to_string()).truncate());
                }
                None => {
                    ui.weak("not set");
                }
            }
            if ui.small_button("Browse...").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                launcher.proton_prefix = Some(path);
                changed = true;
            }
        });
        if ui.button("Auto-detect").clicked() {
            let result = wows_toolkit_launcher::proton::autodetect(&mut launcher, crate::storage_dir().as_deref());
            changed |= result.changed();
            self.toast_proton_setup(result);
        }

        if changed {
            self.tab_state.persisted.write().settings.launcher = launcher;
            self.tab_state.request_save();
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
            Intent::Launch(index) => self.launch_install(index),
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
                let Some(dest_dir) = rfd::FileDialog::new().set_directory(&default_dir).pick_folder() else {
                    return;
                };
                let mut result = Ok(());
                self.tab_state.launcher_op(|launcher, _, env| {
                    result = launcher.start_download(install, dest_dir, env);
                    false
                });
                self.toast_launcher_error(result);
            }
            Intent::Install(game) => {
                let Some(dest_dir) =
                    rfd::FileDialog::new().set_directory(wgc_download::default_install_dir()).pick_folder()
                else {
                    return;
                };
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
            Intent::SetUpProton => {
                let mut settings = self.tab_state.persisted.read().settings.launcher.clone();
                let result = wows_toolkit_launcher::proton::autodetect(&mut settings, crate::storage_dir().as_deref());
                if result.changed() {
                    self.tab_state.persisted.write().settings.launcher = settings;
                    self.tab_state.request_save();
                }
                self.toast_proton_setup(result);
            }
        }
    }

    /// Sign-in form, or the signed-in account's summary.
    fn build_account_section(&mut self, ui: &mut egui::Ui) {
        let mut sign_in = false;
        let mut submit_code = false;
        let mut sign_out = false;
        let mut dismiss_captcha = false;

        let launcher: &Launcher = &self.tab_state.launcher;
        let form = &mut self.tab_state.login_form;
        ui.group(|ui| {
            ui.heading("Account");
            if let Some(session) = launcher.session() {
                ui.horizontal(|ui| {
                    ui.strong(session.nickname.as_str());
                    ui.label(format!("Region: {}", wows_toolkit_launcher::realm_label(session.realm)));
                });
                match session.expiry {
                    wgc_auth::TokenExpiry::KnownAt(expires_at) => {
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
                    wgc_auth::TokenExpiry::Unknown => {
                        ui.weak("Signed in (expiry unknown; verified on launch)");
                    }
                }
                if !launcher.session_persisted() {
                    ui.weak("Not saved to keyring; you'll sign in again next launch.");
                }
                if ui.button("Sign out").clicked() {
                    sign_out = true;
                }
            } else if launcher.two_factor_pending() {
                ui.label("Two-factor code required.");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut form.two_factor_code).hint_text("code"));
                    ui.checkbox(&mut form.use_backup_code, "Use backup code");
                });
                if let Some(err) = launcher.login_error() {
                    ui.colored_label(ERROR_COLOR, err);
                }
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!launcher.login_in_progress(), |ui| {
                        if ui.button("Submit code").clicked() {
                            submit_code = true;
                        }
                    });
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
                ui.horizontal(|ui| {
                    ui.weak("Not signed in.");
                    if ui.button("Sign in").clicked() {
                        form.show = true;
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("launcher_account_realm")
                        .selected_text(wows_toolkit_launcher::realm_label(form.realm))
                        .show_ui(ui, |ui| {
                            for realm in wows_toolkit_launcher::REALMS {
                                ui.selectable_value(&mut form.realm, realm, wows_toolkit_launcher::realm_label(realm));
                            }
                        });
                    ui.label("Username:");
                    ui.text_edit_singleline(&mut form.username);
                });
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut form.password).password(true));
                });
                if let Some(err) = launcher.login_error() {
                    ui.colored_label(ERROR_COLOR, err);
                }
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!launcher.login_in_progress(), |ui| {
                        if ui.button("Sign in").clicked() {
                            sign_in = true;
                        }
                    });
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
            let username = self.tab_state.login_form.username.clone();
            self.tab_state
                .launcher_op(|launcher, settings, env| launcher.start_login(realm, username, password, settings, env));
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
        if sign_out {
            self.tab_state.launcher_op(|launcher, settings, env| launcher.sign_out(settings, env));
        }
        if dismiss_captcha {
            self.tab_state.launcher.dismiss_captcha();
            self.tab_state.login_form.show = true;
        }
    }

    fn launch_install(&mut self, index: usize) {
        let Some(install) = self.tab_state.launcher.installs().get(index).cloned() else {
            return;
        };
        let settings = self.tab_state.persisted.read().settings.launcher.clone();
        let launched =
            self.tab_state.launcher.launch(&install, &settings, self.tab_state.launch_safe_mode, |command| {
                crate::hardening::prepare_child(command, crate::gpu::PIN_VARS);
            });
        if let Err(e) = launched {
            self.tab_state.toasts.lock().error(format!("Launch failed: {e}"));
        }
    }
}
