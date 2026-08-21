//! The Launcher tab: discover, launch, and manage WoWs installs.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::app::ToolkitTabViewer;
use crate::tab_state::CatalogCheckState;
use crate::tab_state::DownloadUiState;
use crate::update_background_task;

/// Fixed width of the whole download-progress block (progress bar + byte
/// label + phase/file line), so the row's size stays constant frame to
/// frame instead of jumping as the byte count and in-flight file name
/// change width. See `show_download_status`.
const PROGRESS_AREA_WIDTH: f32 = 380.0;
/// Fixed width of the "X / Y" byte label within the progress area.
const BYTE_LABEL_WIDTH: f32 = 150.0;

/// One row's download-status action the user triggered this frame (if
/// any), collected by `show_download_status` and applied by the caller
/// after the row loop -- same borrow-collection pattern as
/// `to_launch`/`to_activate`/`to_recheck`.
enum DownloadRowAction {
    None,
    Cancel(Arc<AtomicBool>),
    Pause(Arc<AtomicBool>),
    /// Resume a paused download tracked under this `dest_key` in
    /// `download_state`/`download_resume`.
    Resume(PathBuf),
}

/// Display label for a WGNI account realm. Ru is flagged experimental
/// everywhere it appears -- see `wgc_auth::AccountRealm::Ru`'s own doc.
fn realm_label(realm: wgc_auth::AccountRealm) -> &'static str {
    match realm {
        wgc_auth::AccountRealm::Eu => "EU",
        wgc_auth::AccountRealm::Na => "NA",
        wgc_auth::AccountRealm::Asia => "Asia",
        wgc_auth::AccountRealm::Ru => "RU (experimental)",
    }
}

/// Coarse human-readable duration ("2h 15m", "3d 4h"), for the token-expiry
/// display. No external crate: the app has no other duration-formatting
/// need, so a few lines here beat a new dependency.
fn format_duration(d: std::time::Duration) -> String {
    let total_secs = d.as_secs();
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "less than a minute".to_string()
    }
}

/// Outcome of a Proton auto-detect attempt, so the settings panel and the
/// per-install "Set up Proton" prompt report it identically.
enum ProtonSetup {
    /// Binary set, and (because it was unset) a prefix was chosen. `detail`
    /// names which prefix.
    Ready { detail: &'static str },
    /// Binary set; an existing user-chosen prefix was left alone.
    BinaryOnly,
    /// A Proton binary was found but no app data dir exists for a managed prefix.
    NoManagedPrefixDir,
    /// Steam is installed but carries no Proton under its libraries.
    NoProton,
    /// No Steam installation was found.
    NoSteam,
}

impl ProtonSetup {
    /// Whether `launcher` was mutated (so the caller knows to persist).
    fn changed(&self) -> bool {
        matches!(self, ProtonSetup::Ready { .. } | ProtonSetup::BinaryOnly)
    }
}

/// Fill in `launcher.proton_binary` and, when unset, `launcher.proton_prefix`
/// from the local Steam/Proton install. A non-Steam (WGC-style) install has no
/// Steam prefix to reuse, so it falls back to a toolkit-managed prefix Proton
/// initializes on first launch.
fn autodetect_proton(launcher: &mut crate::data::settings::LauncherSettings) -> ProtonSetup {
    let Some(steam_root) = wgc_launch::proton::find_steam_root() else {
        return ProtonSetup::NoSteam;
    };
    // With no prefix chosen yet there is no `config_info` to read, so
    // `find_proton_in_library` falls back to the newest Proton in the library.
    let compat_data = launcher.proton_prefix.clone().unwrap_or_else(|| steam_root.clone());
    let Some(binary) = wgc_launch::proton::find_proton_in_library(&steam_root, &compat_data) else {
        return ProtonSetup::NoProton;
    };
    launcher.proton_binary = Some(binary);
    if launcher.proton_prefix.is_some() {
        return ProtonSetup::BinaryOnly;
    }
    if let Some(existing) = wgc_launch::proton::steam_wows_prefix() {
        launcher.proton_prefix = Some(existing);
        return ProtonSetup::Ready { detail: "Reusing your Steam WoWs prefix." };
    }
    match crate::storage_dir() {
        Some(base) => {
            launcher.proton_prefix = Some(wgc_launch::proton::managed_prefix(&base));
            ProtonSetup::Ready { detail: "Using a toolkit-managed prefix (created on first launch)." }
        }
        None => ProtonSetup::NoManagedPrefixDir,
    }
}

/// Render one install's catalog-check status (checking/up to date/update
/// available/error), skipping Steam installs entirely -- they are never
/// checked, and the row already says "managed by Steam" above.
fn show_catalog_status(ui: &mut egui::Ui, state: Option<&CatalogCheckState>) {
    match state {
        Some(CatalogCheckState::Checking) => {
            ui.spinner();
            ui.label("checking updates");
        }
        Some(CatalogCheckState::Ready(build)) => {
            let mut changed = Vec::new();
            let mut not_installed = Vec::new();
            for (part, status) in &build.parts {
                match status {
                    wgc_catalog::update::PartStatus::UpdateAvailable { from, to } => {
                        changed.push(format!("{part} {from} -> {to}"))
                    }
                    wgc_catalog::update::PartStatus::NotInstalled { to } => {
                        not_installed.push(format!("{part} ({to})"))
                    }
                    wgc_catalog::update::PartStatus::UpToDate => {}
                }
            }
            if changed.is_empty() {
                ui.label("Up to date");
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 180, 40),
                    format!("Update available: {}", changed.join(", ")),
                );
            }
            if !not_installed.is_empty() {
                ui.weak(format!("not installed: {}", not_installed.join(", ")));
            }
        }
        Some(CatalogCheckState::Error(detail)) => {
            ui.weak(format!("update check failed: {detail}"));
        }
        Some(CatalogCheckState::NoUpdateHost) | None => {}
    }
}

/// Render one install's download/install run status, if any, in a
/// fixed-width block so the row's size stays constant frame to frame as
/// byte counts and the in-flight file name change (long names truncate
/// rather than growing the row). Returns whatever action the user
/// triggered this frame, so the caller can apply it after the row loop
/// (same borrow-collection pattern as `to_launch`/`to_activate`/`to_recheck`).
fn show_download_status(
    ui: &mut egui::Ui,
    dest_key: Option<&std::path::Path>,
    state: Option<&DownloadUiState>,
) -> DownloadRowAction {
    let mut action = DownloadRowAction::None;
    match state {
        Some(DownloadUiState::Running { phase, done_bytes, total_bytes, current, cancel, pause }) => {
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
                            action = DownloadRowAction::Pause(Arc::clone(pause));
                        }
                        if ui.small_button("Cancel").clicked() {
                            action = DownloadRowAction::Cancel(Arc::clone(cancel));
                        }
                    });
                    ui.add(egui::Label::new(format!("{phase}: {current}")).truncate());
                },
            );
        }
        Some(DownloadUiState::Paused) => {
            ui.horizontal(|ui| {
                ui.weak("Download paused; staged files are kept.");
                // `dest_key` is only absent for a row that never started a
                // download at all, which can't be in `Paused`; the guard is
                // defensive, not expected to trip.
                if let Some(dest_key) = dest_key
                    && ui.small_button("Resume").clicked()
                {
                    action = DownloadRowAction::Resume(dest_key.to_path_buf());
                }
            });
        }
        Some(DownloadUiState::Done { skipped_deltas }) => {
            if *skipped_deltas > 0 {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 180, 40),
                    format!("Installed ({skipped_deltas} delta part(s) skipped -- not yet supported, downloaded but not applied)"),
                );
            } else {
                ui.colored_label(egui::Color32::from_rgb(120, 200, 120), "Installed");
            }
        }
        Some(DownloadUiState::Failed(detail)) => {
            ui.colored_label(egui::Color32::from_rgb(230, 80, 80), format!("Download failed: {detail}"));
        }
        Some(DownloadUiState::Cancelled) => {
            ui.weak("Download cancelled.");
        }
        None => {}
    }
    action
}

/// Render one known game's installable version, derived from a fresh
/// catalog check (every part reports `NotInstalled` -- there is nothing
/// installed yet to diff against). The client part's version is the
/// headline: WGUS serves one current build per product, so this is "the"
/// version a fresh install of this game gets, not a choice among several.
fn show_available_game_status(ui: &mut egui::Ui, state: Option<&CatalogCheckState>) {
    match state {
        Some(CatalogCheckState::Checking) => {
            ui.spinner();
            ui.label("checking available version");
        }
        Some(CatalogCheckState::Ready(build)) => {
            let client_version = build.parts.iter().find_map(|(part, status)| {
                if *part != wgc_catalog::models::PartName::Client {
                    return None;
                }
                match status {
                    wgc_catalog::update::PartStatus::NotInstalled { to } => Some(to.to_string()),
                    wgc_catalog::update::PartStatus::UpToDate
                    | wgc_catalog::update::PartStatus::UpdateAvailable { .. } => None,
                }
            });
            match client_version {
                Some(v) => ui.label(format!("version {v}")),
                None => ui.weak("client version not reported"),
            };
        }
        Some(CatalogCheckState::Error(detail)) => {
            ui.weak(format!("unavailable: {detail}"));
        }
        Some(CatalogCheckState::NoUpdateHost) | None => {}
    }
}

impl ToolkitTabViewer<'_> {
    /// Toast the outcome of a Proton auto-detect attempt.
    fn toast_proton_setup(&self, result: &ProtonSetup) {
        let mut toasts = self.tab_state.toasts.lock();
        match result {
            ProtonSetup::Ready { detail } => {
                toasts.success(format!("Proton detected. {detail}"));
            }
            ProtonSetup::BinaryOnly => {
                toasts.success("Proton binary detected.");
            }
            ProtonSetup::NoManagedPrefixDir => {
                toasts.warning(
                    "Found a Proton binary, but could not determine a data directory for a managed prefix -- set the prefix manually.",
                );
            }
            ProtonSetup::NoProton => {
                toasts.warning(
                    "Found a Steam installation, but no Proton install under its library -- install Proton via Steam, then try again.",
                );
            }
            ProtonSetup::NoSteam => {
                toasts.error("Could not find a Steam installation. Proton needs Steam installed.");
            }
        }
    }

    /// Run Proton auto-detect against the persisted launcher settings and save.
    /// Used by the per-install "Set up Proton" prompt (Linux only; other
    /// platforms launch non-Steam installs natively).
    #[cfg(target_os = "linux")]
    fn apply_proton_autodetect(&mut self) {
        let mut launcher = self.tab_state.persisted.read().settings.launcher.clone();
        let result = autodetect_proton(&mut launcher);
        if result.changed() {
            self.tab_state.persisted.write().settings.launcher = launcher;
            self.tab_state.request_save();
        }
        self.toast_proton_setup(&result);
    }

    pub fn build_launcher_tab(&mut self, ui: &mut egui::Ui) {
        self.build_account_section(ui);
        ui.separator();

        ui.horizontal(|ui| {
            ui.heading("Launcher");
            if ui.button("Rescan").clicked() {
                self.tab_state.start_install_discovery();
            }
            if ui.button("Add install...").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                self.tab_state.persisted.write().settings.launcher.manual_installs.push(dir);
                self.tab_state.start_install_discovery();
            }
            if self.tab_state.discovering_installs {
                ui.spinner();
            }
        });

        ui.checkbox(&mut self.tab_state.launch_safe_mode, "Safe mode (disable mods)");

        ui.collapsing("Launch options", |ui| {
            let mut launcher = self.tab_state.persisted.read().settings.launcher.clone();
            let mut changed = false;

            ui.label("Extra launch args (one per line):");
            let mut args_text = launcher.extra_args.join("\n");
            if ui.text_edit_multiline(&mut args_text).changed() {
                launcher.extra_args = args_text.lines().map(str::to_string).filter(|s| !s.is_empty()).collect();
                changed = true;
            }

            ui.label("Environment variables (KEY=VALUE, one per line):");
            let mut env_text = launcher.env_vars.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
            if ui.text_edit_multiline(&mut env_text).changed() {
                launcher.env_vars = env_text
                    .lines()
                    .filter_map(|line| line.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
                    .filter(|(k, _)| !k.is_empty())
                    .collect();
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
                let result = autodetect_proton(&mut launcher);
                changed |= result.changed();
                self.toast_proton_setup(&result);
            }

            if changed {
                self.tab_state.persisted.write().settings.launcher = launcher;
                self.tab_state.request_save();
            }
        });

        ui.separator();

        // Collect launch/select intents first so we do not borrow-conflict with
        // the install list while iterating it.
        let mut to_launch: Option<usize> = None;
        let mut to_activate: Option<std::path::PathBuf> = None;
        let mut to_recheck: Option<std::path::PathBuf> = None;
        // (install_path, folder-picker default dir).
        let mut to_download: Option<(std::path::PathBuf, std::path::PathBuf)> = None;
        let mut to_cancel_download: Option<Arc<AtomicBool>> = None;
        let mut to_pause_download: Option<Arc<AtomicBool>> = None;
        let mut to_resume_download: Option<std::path::PathBuf> = None;
        // Linux non-Steam installs launch through Proton; if it is not
        // configured yet, offer one-click setup right at the install.
        #[cfg(target_os = "linux")]
        let mut to_setup_proton = false;
        #[cfg(target_os = "linux")]
        let proton_configured = {
            let guard = self.tab_state.persisted.read();
            let l = &guard.settings.launcher;
            l.proton_binary.is_some() && l.proton_prefix.is_some()
        };

        for (i, install) in self.tab_state.discovered_installs.iter().enumerate() {
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
                        show_catalog_status(ui, self.tab_state.catalog_status.get(&install.path));
                        if ui.small_button("Recheck").clicked() {
                            to_recheck = Some(install.path.clone());
                        }
                    });

                    let download_state = self.tab_state.download_state.get(&install.path);
                    let is_active =
                        matches!(download_state, Some(DownloadUiState::Running { .. } | DownloadUiState::Paused));
                    match show_download_status(ui, Some(&install.path), download_state) {
                        DownloadRowAction::Cancel(cancel) => to_cancel_download = Some(cancel),
                        DownloadRowAction::Pause(pause) => to_pause_download = Some(pause),
                        DownloadRowAction::Resume(dest_key) => to_resume_download = Some(dest_key),
                        DownloadRowAction::None => {}
                    }

                    let has_downloads = matches!(
                        self.tab_state.catalog_status.get(&install.path),
                        Some(CatalogCheckState::Ready(build)) if !build.downloads.is_empty()
                    );
                    if has_downloads && !is_active {
                        ui.horizontal(|ui| {
                            if ui.button("Download / Update...").clicked() {
                                // A discovered install with no detected version has
                                // no real content on disk yet (e.g. a manually added
                                // empty target directory), so default the folder
                                // picker to the platform-standard suggestion instead
                                // of that empty directory.
                                let default_dir = if install.version.is_some() {
                                    install.path.clone()
                                } else {
                                    wgc_download::default_install_dir()
                                };
                                to_download = Some((install.path.clone(), default_dir));
                            }
                            ui.weak(
                                "A fresh install is tens of GB; delta updates are not supported yet, so a changed part re-downloads in full.",
                            );
                        });
                    }
                }
                #[cfg(target_os = "linux")]
                if !install.source.is_read_only_content() && !proton_configured {
                    ui.horizontal(|ui| {
                        let warn = ui.visuals().warn_fg_color;
                        ui.colored_label(warn, "Needs Proton to launch on Linux.");
                        if ui.button("Set up Proton").clicked() {
                            to_setup_proton = true;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Launch").clicked() {
                        to_launch = Some(i);
                    }
                    if ui.button("Use for toolkit").clicked() {
                        to_activate = Some(install.path.clone());
                    }
                });
            });
        }

        #[cfg(target_os = "linux")]
        if to_setup_proton {
            self.apply_proton_autodetect();
        }

        if let Some(path) = to_recheck {
            self.tab_state.recheck_catalog(&path);
        }

        if let Some(path) = to_activate {
            self.tab_state.persisted.write().settings.game.wows_dir = path.display().to_string();
            self.tab_state.revalidate_wows_dir();
            if !self.tab_state.wows_dir_invalid {
                self.tab_state.prevent_changing_wows_dir();
                update_background_task!(self.tab_state.background_tasks, Some(self.tab_state.load_game_data(path)));
            }
        }

        if let Some(i) = to_launch {
            self.launch_install(i);
        }

        if let Some(cancel) = to_cancel_download {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(pause) = to_pause_download {
            pause.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some(dest_key) = to_resume_download {
            self.tab_state.resume_download(&dest_key);
        }

        if let Some((install_path, default_dir)) = to_download
            && let Some(dest_dir) = rfd::FileDialog::new().set_directory(&default_dir).pick_folder()
        {
            self.tab_state.start_download(install_path, dest_dir);
        }

        ui.separator();
        self.build_install_game_section(ui);
    }

    /// "Install a game" section: lets a user with zero (or more) discovered
    /// installs download and install one of the built-in known games
    /// (`wgc_catalog::known_games`) from scratch. Each entry is the current
    /// build WGUS serves for that product (live WoWs vs Public Test) -- WGUS
    /// has no history of older builds to offer, so there is no version
    /// dropdown here, only a choice of product.
    fn build_install_game_section(&mut self, ui: &mut egui::Ui) {
        // Guarded by "no results yet" rather than a one-shot flag: once every
        // known game has an entry (Checking, Ready, or Error) this is
        // permanently false, so the check fires exactly once per first
        // render of this section (this fn only runs while the Launcher tab
        // is visible), same effect `start_catalog_check`'s own per-install
        // guard achieves for discovered installs.
        if self.tab_state.available_games.is_empty() {
            self.tab_state.start_available_games_check();
        }

        ui.collapsing("Install a game", |ui| {
            ui.weak("A fresh install is tens of GB.");

            // Collect the install intent (and any row action) first so we
            // do not borrow-conflict with `available_games`/`download_state`
            // while iterating -- same pattern as `to_download`/`to_launch`
            // above.
            let mut to_install: Option<(String, std::path::PathBuf)> = None;
            let mut to_cancel: Option<Arc<AtomicBool>> = None;
            let mut to_pause: Option<Arc<AtomicBool>> = None;
            let mut to_resume: Option<std::path::PathBuf> = None;

            for known in wgc_catalog::known_games::known_games() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(known.display_name);
                        show_available_game_status(ui, self.tab_state.available_games.get(known.game_id));
                    });

                    let dest = self.tab_state.installing_games.get(known.game_id).cloned();
                    let download_state = dest.as_ref().and_then(|d| self.tab_state.download_state.get(d));
                    let is_active =
                        matches!(download_state, Some(DownloadUiState::Running { .. } | DownloadUiState::Paused));
                    match show_download_status(ui, dest.as_deref(), download_state) {
                        DownloadRowAction::Cancel(cancel) => to_cancel = Some(cancel),
                        DownloadRowAction::Pause(pause) => to_pause = Some(pause),
                        DownloadRowAction::Resume(dest_key) => to_resume = Some(dest_key),
                        DownloadRowAction::None => {}
                    }

                    let is_ready = matches!(
                        self.tab_state.available_games.get(known.game_id),
                        Some(CatalogCheckState::Ready(build)) if !build.downloads.is_empty()
                    );
                    ui.add_enabled_ui(is_ready && !is_active, |ui| {
                        if ui.button("Install...").clicked()
                            && let Some(dest_dir) =
                                rfd::FileDialog::new().set_directory(wgc_download::default_install_dir()).pick_folder()
                        {
                            to_install = Some((known.game_id.to_string(), dest_dir));
                        }
                    });
                });
            }

            if let Some(cancel) = to_cancel {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(pause) = to_pause {
                pause.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(dest_key) = to_resume {
                self.tab_state.resume_download(&dest_key);
            }
            if let Some((game_id, dest_dir)) = to_install {
                self.tab_state.install_known_game(game_id, dest_dir);
            }
        });
    }

    /// Sign-in / signed-in summary. Renders from `tab_state`, then applies
    /// the one action the user triggered this frame (if any) after the UI
    /// closure ends -- `start_login`/`start_two_factor`/`sign_out` all need
    /// `&mut self.tab_state`, which the closure's borrows of it must not
    /// still be alive for, so intents are collected first and applied last
    /// (the same shape `build_launcher_tab` uses for install actions).
    fn build_account_section(&mut self, ui: &mut egui::Ui) {
        let mut to_sign_in = false;
        let mut to_submit_code = None;
        let mut to_sign_out = false;

        ui.group(|ui| {
            ui.heading("Account");
            if let Some(session) = self.tab_state.active_session.as_ref() {
                ui.horizontal(|ui| {
                    ui.strong(session.nickname.as_str());
                    ui.label(format!("Region: {}", realm_label(session.realm)));
                });
                match session.expiry {
                    wgc_auth::TokenExpiry::KnownAt(expires_at) => match expires_at.duration_since(std::time::SystemTime::now()) {
                        Ok(remaining) => {
                            ui.label(format!("Signed in (expires in {})", format_duration(remaining)));
                        }
                        Err(_) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 80, 80), "Session expired - sign in again");
                        }
                    },
                    wgc_auth::TokenExpiry::Unknown => {
                        ui.weak("Signed in (expiry unknown; verified on launch)");
                    }
                }
                if !self.tab_state.session_persisted {
                    ui.weak("Not saved to keyring; you'll sign in again next launch.");
                }
                if ui.button("Sign out").clicked() {
                    to_sign_out = true;
                }
            } else if let Some(prompt) = self.tab_state.login_form.two_factor.clone() {
                ui.label("Two-factor code required.");
                let mut code = prompt.code;
                let mut use_backup = prompt.use_backup;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut code).hint_text("code"));
                    ui.checkbox(&mut use_backup, "Use backup code");
                });
                if let Some(err) = &self.tab_state.login_form.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 80, 80), err);
                }
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.tab_state.login_form.in_progress, |ui| {
                        if ui.button("Submit code").clicked() {
                            to_submit_code = Some(if use_backup {
                                wgc_auth::TwoFactorCode::Backup(code.clone())
                            } else {
                                wgc_auth::TwoFactorCode::Otp(code.clone())
                            });
                        }
                    });
                    if self.tab_state.login_form.in_progress {
                        ui.spinner();
                    }
                });
                if let Some(p) = self.tab_state.login_form.two_factor.as_mut() {
                    p.code = code;
                    p.use_backup = use_backup;
                }
            } else if self.tab_state.login_form.captcha_blocked {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 180, 40),
                    "This login needs a CAPTCHA, which this launcher can't show yet. Sign in once via the official Wargaming Game Center or wargaming.net in a browser, then try again.",
                );
            } else if !self.tab_state.login_form.show {
                ui.horizontal(|ui| {
                    ui.weak("Not signed in.");
                    if ui.button("Sign in").clicked() {
                        self.tab_state.login_form.show = true;
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("launcher_account_realm")
                        .selected_text(realm_label(self.tab_state.login_form.realm))
                        .show_ui(ui, |ui| {
                            for realm in [
                                wgc_auth::AccountRealm::Eu,
                                wgc_auth::AccountRealm::Na,
                                wgc_auth::AccountRealm::Asia,
                                wgc_auth::AccountRealm::Ru,
                            ] {
                                if ui.selectable_label(self.tab_state.login_form.realm == realm, realm_label(realm)).clicked() {
                                    self.tab_state.login_form.realm = realm;
                                }
                            }
                        });
                    ui.label("Username:");
                    ui.text_edit_singleline(&mut self.tab_state.login_form.username);
                });
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.tab_state.login_form.password).password(true));
                });
                if let Some(err) = &self.tab_state.login_form.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 80, 80), err);
                }
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.tab_state.login_form.in_progress, |ui| {
                        if ui.button("Sign in").clicked() {
                            to_sign_in = true;
                        }
                    });
                    if self.tab_state.login_form.in_progress {
                        ui.spinner();
                    }
                });
            }
        });

        if to_sign_in {
            self.tab_state.start_login();
        }
        if let Some(code) = to_submit_code {
            self.tab_state.start_two_factor(code);
        }
        if to_sign_out {
            self.tab_state.sign_out();
        }
    }

    fn launch_install(&mut self, index: usize) {
        let Some(install) = self.tab_state.discovered_installs.get(index).cloned() else {
            return;
        };
        let launcher = self.tab_state.persisted.read().settings.launcher.clone();
        let proton = match (launcher.proton_binary, launcher.proton_prefix) {
            (Some(binary), Some(prefix)) => {
                Some(wgc_launch::proton::ProtonConfig { proton_binary: binary, compat_prefix: prefix })
            }
            _ => None,
        };

        // Compose user args + env, appending safe-mode args when toggled.
        let mut extra = launcher.extra_args.clone();
        if self.tab_state.launch_safe_mode {
            extra.extend(launcher.safe_mode_args.clone());
        }
        let launch_args = wgc_launch::LaunchArgs { extra, env: launcher.env_vars.clone() };

        // Online play. With no signed-in account the gate reports
        // NotConfigured and allows the launch (the game shows its own
        // login); with a signed-in account the gate reflects its real
        // status, so an expired session refuses here with
        // AuthenticationRequired instead of failing inside the game.
        let resolved = match wgc_launch::resolve(&install, wgc_launch::LaunchMode::Online, &launch_args) {
            Ok(r) => r,
            Err(e) => {
                self.tab_state.toasts.lock().error(format!("Cannot launch: {e}"));
                return;
            }
        };
        let token_status =
            wgc_auth::launch_token_status(self.tab_state.active_session.as_ref(), std::time::SystemTime::now());
        if let Err(e) = resolved.spawn(proton.as_ref(), token_status) {
            self.tab_state.toasts.lock().error(format!("Launch failed: {e}"));
        }
    }
}
