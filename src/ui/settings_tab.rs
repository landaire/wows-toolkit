use std::path::Path;
use std::path::PathBuf;

use egui::Color32;
use egui::OpenUrl;
use egui::Slider;

use crate::app::ToolkitTabViewer;
use crate::icons;
use crate::task::DataExportSettings;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::task::ReplayExportFormat;
use crate::twitch::Token;
use crate::update_background_task;

const DEFAULT_ZOOM_FACTOR: f32 = 1.15;

impl ToolkitTabViewer<'_> {
    pub fn build_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label("Application Settings");
            ui.group(|ui| {
                ui.checkbox(&mut self.tab_state.settings.check_for_updates, "Check for Updates on Startup");
                if ui
                    .checkbox(
                        &mut self.tab_state.settings.send_replay_data,
                        "Send Builds from Ranked and Random Battles Replays to ShipBuilds.com",
                    )
                    .changed()
                {
                    self.tab_state.send_replay_consent_changed();
                }
                ui.horizontal(|ui| {
                    let mut zoom = ui.ctx().zoom_factor();
                    if ui
                        .add(Slider::new(&mut zoom, 0.5..=2.0).text("Zoom Factor (Ctrl + and Ctrl - also changes this)"))
                        .changed()
                    {
                        ui.ctx().set_zoom_factor(zoom);
                    }
                    if ui.button("Reset").clicked() {
                        ui.ctx().set_zoom_factor(DEFAULT_ZOOM_FACTOR);
                    }
                });
            });
            ui.label("World of Warships Settings");
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(self.tab_state.can_change_wows_dir, egui::Button::new("Choose..."))
                                .clicked()
                            {
                                let folder = rfd::FileDialog::new().pick_folder();
                                if let Some(folder) = folder {
                                    self.tab_state.prevent_changing_wows_dir();
                                    update_background_task!(
                                        self.tab_state.background_tasks,
                                        Some(self.tab_state.load_game_data(folder))
                                    );
                                }
                            }

                            let show_text_error = {
                                let path = Path::new(&self.tab_state.settings.wows_dir);
                                !(path.exists() && path.join("bin").exists())
                            };

                            let response = ui.add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(&mut self.tab_state.settings.wows_dir)
                                    .interactive(self.tab_state.can_change_wows_dir)
                                    .hint_text("World of Warships Directory")
                                    .text_color_opt(show_text_error.then_some(Color32::LIGHT_RED)),
                            );

                            // If someone pastes a path in, let's do some basic validation to see if this
                            // can be a WoWs path. If so, reload game data.
                            if response.changed() {
                                let path = Path::new(&self.tab_state.settings.wows_dir).to_owned();
                                if path.exists() && path.join("bin").exists() {
                                    self.tab_state.prevent_changing_wows_dir();
                                    update_background_task!(
                                        self.tab_state.background_tasks,
                                        Some(self.tab_state.load_game_data(path))
                                    );
                                }
                            }
                        });
                    });
                })
            });
            ui.label("Replay Settings");
            ui.group(|ui| {
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_game_chat, "Show Game Chat");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_raw_xp, "Show Raw XP Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_entity_id, "Show Entity ID Column");
                ui.checkbox(
                    &mut self.tab_state.settings.replay_settings.show_observed_damage,
                    "Show Observed Damage Column",
                );
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_fires, "Show Fires Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_floods, "Show Floods Column");
                ui.checkbox(&mut self.tab_state.settings.replay_settings.show_citadels, "Show Citadels Column");
                ui.checkbox(
                    &mut self.tab_state.settings.replay_settings.show_crits,
                    "Show Critical Module Hits Column",
                );
                ui.horizontal(|ui| {
                    let mut alert_data_export_change = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Choose...").clicked() {
                            let folder = rfd::FileDialog::new().pick_folder();
                            if let Some(folder) = folder {
                                self.tab_state.settings.replay_settings.auto_export_path =
                                    folder.to_string_lossy().to_string();
                                alert_data_export_change = true;
                            }
                        }

                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if ui
                                .checkbox(
                                    &mut self.tab_state.settings.replay_settings.auto_export_data,
                                    "Auto-Export Data",
                                )
                                .changed()
                            {
                                alert_data_export_change = true;
                            }

                            let selected_format = &mut self.tab_state.settings.replay_settings.auto_export_format;
                            let previously_selected_format = *selected_format;
                            egui::ComboBox::from_id_salt("auto_export_format_combobox")
                                .selected_text(selected_format.as_str())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(selected_format, ReplayExportFormat::Json, "JSON");
                                    ui.selectable_value(selected_format, ReplayExportFormat::Csv, "CSV");
                                    ui.selectable_value(selected_format, ReplayExportFormat::Cbor, "CBOR");
                                });
                            if previously_selected_format != *selected_format {
                                alert_data_export_change = true;
                            }
                            let path = Path::new(&self.tab_state.settings.replay_settings.auto_export_path);
                            let path_is_valid = path.exists() && path.is_dir();
                            let response = ui.add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(&mut self.tab_state.settings.replay_settings.auto_export_path)
                                    .hint_text("Data auto-export directory")
                                    .text_color_opt((!path_is_valid).then_some(Color32::LIGHT_RED)),
                            );

                            if response.lost_focus() {
                                let path = Path::new(&self.tab_state.settings.replay_settings.auto_export_path);
                                if path.exists() && path.is_dir() {
                                    alert_data_export_change = true;
                                }
                            }
                        });
                    });

                    if alert_data_export_change {
                        let _ = self.tab_state.background_parser_tx.as_ref().map(|tx| {
                            tx.send(ReplayBackgroundParserThreadMessage::DataAutoExportSettingChange(DataExportSettings {
                                should_auto_export: self.tab_state.settings.replay_settings.auto_export_data,
                                export_path: PathBuf::from(
                                    self.tab_state.settings.replay_settings.auto_export_path.clone(),
                                ),
                                export_format: self.tab_state.settings.replay_settings.auto_export_format,
                            }))
                        });
                    }
                });
            });
            ui.label("Twitch Settings");
            ui.group(|ui| {
                if ui
                    .button(format!("{} Get Login Token", icons::BROWSER))
                    .on_hover_text(
                        "We use Chatterino's login page as it provides a token with the \
                        necessary permissions (basically a moderator token with chat permissions), \
                        and it removes the need for the WoWs Toolkit developer to host their own login page website which would have the same result.",
                    )
                    .clicked()
                {
                    ui.ctx().open_url(OpenUrl::new_tab("https://chatterino.com/client_login"));
                }

                let text = if self.tab_state.twitch_state.read().token_is_valid() {
                    format!(
                        "{} Paste Token (Current Token is Valid {})",
                        icons::CLIPBOARD_TEXT,
                        icons::CHECK_CIRCLE
                    )
                } else {
                    format!(
                        "{} Paste Token (No Current Token / Invalid Token {})",
                        icons::CLIPBOARD_TEXT,
                        icons::X_CIRCLE
                    )
                };
                if ui.button(text).clicked()
                    && let Ok(mut clipboard) = arboard::Clipboard::new()
                        && let Ok(contents) = clipboard.get_text()
                    {
                        let token: Result<Token, _> = contents.parse();
                        if let Ok(token) = token
                            && let Some(tx) = self.tab_state.twitch_update_sender.as_ref()
                        {
                            self.tab_state.settings.twitch_token = Some(token.clone());
                            let _ = tx.blocking_send(crate::twitch::TwitchUpdate::Token(token));
                        }
                    }
                ui.label("Monitored Channel (Default to Self)");
                let response = ui.text_edit_singleline(&mut self.tab_state.settings.twitch_monitored_channel);
                if response.lost_focus()
                    && let Some(tx) = self.tab_state.twitch_update_sender.as_ref()
                {
                    let _ =
                        tx.blocking_send(crate::twitch::TwitchUpdate::User(self.tab_state.settings.twitch_monitored_channel.clone()));
                }
            });
        });
    }
}
