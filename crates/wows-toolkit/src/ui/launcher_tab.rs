//! The Launcher tab: discover, launch, and manage WoWs installs.

use crate::app::ToolkitTabViewer;
use crate::tab_state::CatalogCheckState;
use crate::tab_state::DownloadUiState;
use crate::update_background_task;

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

/// Render one install's download/install run status, if any. Returns the
/// download's `cancel` flag (the same `Arc` the background task checks) if
/// the user clicked "Cancel" this frame, so the caller can flip it after
/// the row loop (same borrow-collection pattern as
/// `to_launch`/`to_activate`/`to_recheck`).
fn show_download_status(
    ui: &mut egui::Ui,
    state: Option<&DownloadUiState>,
) -> Option<std::sync::Arc<std::sync::atomic::AtomicBool>> {
    let mut to_cancel = None;
    match state {
        Some(DownloadUiState::Running { phase, done_bytes, total_bytes, current, cancel }) => {
            ui.horizontal(|ui| {
                if *total_bytes > 0 {
                    let frac = (*done_bytes as f32 / *total_bytes as f32).clamp(0.0, 1.0);
                    ui.add(egui::ProgressBar::new(frac).show_percentage().desired_width(200.0));
                    ui.label(format!(
                        "{} / {}",
                        humansize::format_size(*done_bytes, humansize::BINARY),
                        humansize::format_size(*total_bytes, humansize::BINARY)
                    ));
                } else {
                    ui.spinner();
                }
                if ui.small_button("Cancel").clicked() {
                    to_cancel = Some(std::sync::Arc::clone(cancel));
                }
            });
            ui.weak(format!("{phase}: {current}"));
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
    to_cancel
}

impl ToolkitTabViewer<'_> {
    pub fn build_launcher_tab(&mut self, ui: &mut egui::Ui) {
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
        let mut to_cancel_download: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;

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
                    let is_running = matches!(download_state, Some(DownloadUiState::Running { .. }));
                    if let Some(cancel) = show_download_status(ui, download_state) {
                        to_cancel_download = Some(cancel);
                    }

                    let has_downloads = matches!(
                        self.tab_state.catalog_status.get(&install.path),
                        Some(CatalogCheckState::Ready(build)) if !build.downloads.is_empty()
                    );
                    if has_downloads && !is_running {
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

        if let Some((install_path, default_dir)) = to_download
            && let Some(dest_dir) = rfd::FileDialog::new().set_directory(&default_dir).pick_folder()
        {
            self.tab_state.start_download(install_path, dest_dir);
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

        // Online play. Slice 1 has no account system, so the session status is
        // NotConfigured, which the gate allows (the game shows its own login).
        // Slice 3 replaces NotConfigured with the active account's real status,
        // and an expired token then refuses here with AuthenticationRequired.
        let resolved = match wgc_launch::resolve(&install, wgc_launch::LaunchMode::Online, &launch_args) {
            Ok(r) => r,
            Err(e) => {
                self.tab_state.toasts.lock().error(format!("Cannot launch: {e}"));
                return;
            }
        };
        if let Err(e) = resolved.spawn(proton.as_ref(), wgc_launch::TokenStatus::NotConfigured) {
            self.tab_state.toasts.lock().error(format!("Launch failed: {e}"));
        }
    }
}
