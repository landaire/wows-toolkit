//! The Launcher tab: discover, launch, and manage WoWs installs.

use crate::app::ToolkitTabViewer;
use crate::update_background_task;

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
