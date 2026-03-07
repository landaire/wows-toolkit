use std::path::Path;
use std::path::PathBuf;

use egui::Color32;
use egui::OpenUrl;
use egui::RichText;
use egui::Slider;
use rust_i18n::t;

use crate::app::ToolkitTabViewer;
use crate::icons;
use crate::task::DataExportSettings;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::task::ReplayExportFormat;
use crate::twitch::Token;
use crate::update_background_task;

const DEFAULT_ZOOM_FACTOR: f32 = 1.15;

/// Render a styled section header with an icon, title, and dimmed description.
fn section_header(ui: &mut egui::Ui, icon: &str, title: &str, description: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).size(16.0).strong());
        ui.label(RichText::new(title).size(14.0).strong());
    });
    ui.label(RichText::new(description).size(11.0).weak());
    ui.add_space(2.0);
}

impl ToolkitTabViewer<'_> {
    pub fn build_settings_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width());

            // ── Application Settings ──────────────────────────────────
            section_header(ui, icons::GEAR_FINE, &t!("ui.settings.app.heading"), &t!("ui.settings.app.description"));
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.checkbox(&mut self.tab_state.settings.check_for_updates, t!("ui.settings.app.check_for_updates"));
                ui.checkbox(&mut self.tab_state.settings.enable_logging, t!("ui.settings.app.enable_logging"))
                    .on_hover_text(t!("ui.settings.app.enable_logging_tooltip"));
                if ui
                    .checkbox(&mut self.tab_state.settings.send_replay_data, t!("ui.settings.app.send_replay_data"))
                    .changed()
                {
                    self.tab_state.send_replay_consent_changed();
                }
                ui.horizontal(|ui| {
                    let mut zoom = ui.ctx().zoom_factor();
                    if ui.add(Slider::new(&mut zoom, 0.5..=2.0).text(t!("ui.settings.app.zoom_factor"))).changed() {
                        ui.ctx().set_zoom_factor(zoom);
                    }
                    if ui.button(t!("ui.buttons.reset")).clicked() {
                        ui.ctx().set_zoom_factor(DEFAULT_ZOOM_FACTOR);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t!("ui.settings.app.language"));
                    let current_locale = self.tab_state.settings.locale.clone().unwrap_or_else(|| "en".into());
                    let current_name = wt_translations::language_name(&current_locale).unwrap_or("English");
                    egui::ComboBox::from_id_salt("language_selector").selected_text(current_name).show_ui(ui, |ui| {
                        for lang in wt_translations::SUPPORTED_LANGUAGES {
                            if ui.selectable_label(current_locale == lang.code, lang.native_name).clicked() {
                                self.tab_state.settings.locale = Some(lang.code.to_string());
                                rust_i18n::set_locale(lang.code);

                                // Swap the gettext catalog so WoWs translations (ship names,
                                // achievements, etc.) resolve in the new locale without a
                                // full game data reload.
                                if let Some(data_map) = &self.tab_state.wows_data_map {
                                    data_map.reload_translations(lang.code);
                                }
                            }
                        }
                    });
                });
            });

            ui.add_space(12.0);

            // ── World of Warships Settings ────────────────────────────
            section_header(
                ui,
                icons::FOLDER_OPEN,
                &t!("ui.settings.wows.heading"),
                &t!("ui.settings.wows.description"),
            );
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(self.tab_state.can_change_wows_dir, egui::Button::new(t!("ui.buttons.choose")))
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
                            if self.tab_state.settings.wows_dir.is_empty() {
                                false
                            } else if !path.exists() {
                                true
                            } else {
                                let has_exe = path.join("WorldOfWarships.exe").exists();
                                let has_bin = path.join("bin").exists();
                                let has_replays = path.join("replays").exists();
                                !has_exe && !has_bin && !has_replays
                            }
                        };

                        let response = ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(&mut self.tab_state.settings.wows_dir)
                                .interactive(self.tab_state.can_change_wows_dir)
                                .hint_text(t!("ui.settings.wows.directory_hint"))
                                .text_color_opt(show_text_error.then_some(Color32::LIGHT_RED)),
                        );

                        // If someone pastes a path in, let's do some basic validation to see if this
                        // can be a WoWs path. If so, reload game data.
                        if response.changed() {
                            let path = Path::new(&self.tab_state.settings.wows_dir).to_owned();
                            let has_exe = path.join("WorldOfWarships.exe").exists();
                            let has_bin = path.join("bin").exists();
                            if path.exists() && (has_bin || has_exe) {
                                self.tab_state.prevent_changing_wows_dir();
                                update_background_task!(
                                    self.tab_state.background_tasks,
                                    Some(self.tab_state.load_game_data(path))
                                );
                            }
                        }
                    });
                });
            });

            ui.add_space(12.0);

            // ── Replay Settings ───────────────────────────────────────
            section_header(ui, icons::TABLE, &t!("ui.settings.replay.heading"), &t!("ui.settings.replay.description"));
            ui.group(|ui| {
                ui.set_width(ui.available_width());

                // 2-column grid for the column visibility checkboxes
                egui::Grid::new("replay_columns_grid").num_columns(2).spacing([40.0, 4.0]).show(ui, |ui| {
                    ui.checkbox(
                        &mut self.tab_state.settings.replay_settings.show_raw_xp,
                        t!("ui.settings.replay.show_raw_xp"),
                    );
                    ui.checkbox(
                        &mut self.tab_state.settings.replay_settings.show_entity_id,
                        t!("ui.settings.replay.show_entity_id"),
                    );
                    ui.end_row();

                    ui.checkbox(
                        &mut self.tab_state.settings.replay_settings.show_observed_damage,
                        t!("ui.settings.replay.show_observed_damage"),
                    );
                    ui.checkbox(
                        &mut self.tab_state.settings.replay_settings.show_fires,
                        t!("ui.settings.replay.show_fires"),
                    );
                    ui.end_row();

                    ui.checkbox(
                        &mut self.tab_state.settings.replay_settings.show_floods,
                        t!("ui.settings.replay.show_floods"),
                    );
                    ui.checkbox(
                        &mut self.tab_state.settings.replay_settings.show_citadels,
                        t!("ui.settings.replay.show_citadels"),
                    );
                    ui.end_row();

                    ui.checkbox(
                        &mut self.tab_state.settings.replay_settings.show_crits,
                        t!("ui.settings.replay.show_crits"),
                    );
                    ui.end_row();
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    let mut alert_data_export_change = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t!("ui.buttons.choose")).clicked() {
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
                                    t!("ui.settings.replay.auto_export_data"),
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
                                    ui.selectable_value(
                                        selected_format,
                                        ReplayExportFormat::Json,
                                        t!("ui.settings.replay.format_json"),
                                    );
                                    ui.selectable_value(
                                        selected_format,
                                        ReplayExportFormat::Csv,
                                        t!("ui.settings.replay.format_csv"),
                                    );
                                    ui.selectable_value(
                                        selected_format,
                                        ReplayExportFormat::Cbor,
                                        t!("ui.settings.replay.format_cbor"),
                                    );
                                });
                            if previously_selected_format != *selected_format {
                                alert_data_export_change = true;
                            }
                            let path = Path::new(&self.tab_state.settings.replay_settings.auto_export_path);
                            let path_is_valid = path.exists() && path.is_dir();
                            let response = ui.add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(
                                    &mut self.tab_state.settings.replay_settings.auto_export_path,
                                )
                                .hint_text(t!("ui.settings.replay.export_path_hint"))
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
                            tx.send(ReplayBackgroundParserThreadMessage::DataAutoExportSettingChange(
                                DataExportSettings {
                                    should_auto_export: self.tab_state.settings.replay_settings.auto_export_data,
                                    export_path: PathBuf::from(
                                        self.tab_state.settings.replay_settings.auto_export_path.clone(),
                                    ),
                                    export_format: self.tab_state.settings.replay_settings.auto_export_format,
                                },
                            ))
                        });
                    }
                });
            });

            ui.add_space(12.0);

            // ── Session Settings ──────────────────────────────────────
            section_header(
                ui,
                icons::USERS,
                &t!("ui.settings.session.heading"),
                &t!("ui.settings.session.description"),
            );
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.label(t!("ui.settings.session.display_name"));
                ui.text_edit_singleline(&mut self.tab_state.settings.collab_display_name);
                ui.checkbox(
                    &mut self.tab_state.settings.suppress_p2p_ip_warning,
                    t!("ui.settings.session.suppress_ip_warning"),
                )
                .on_hover_text(t!("ui.settings.session.ip_warning_tooltip"));
                ui.checkbox(
                    &mut self.tab_state.settings.disable_auto_open_session_windows,
                    t!("ui.settings.session.disable_auto_open"),
                )
                .on_hover_text(t!("ui.settings.session.auto_open_tooltip"));
            });

            ui.add_space(12.0);

            // ── Twitch Settings ───────────────────────────────────────
            section_header(
                ui,
                icons::BROADCAST,
                &t!("ui.settings.twitch.heading"),
                &t!("ui.settings.twitch.description"),
            );
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                if ui
                    .button(wt_translations::icon_t(icons::BROWSER, &t!("ui.settings.twitch.get_token")))
                    .on_hover_text(t!("ui.settings.twitch.get_token_tooltip"))
                    .clicked()
                {
                    ui.ctx().open_url(OpenUrl::new_tab("https://chatterino.com/client_login"));
                }

                let text = if self.tab_state.settings.twitch_token.is_none() {
                    format!(
                        "{} {} {}",
                        icons::CLIPBOARD_TEXT,
                        t!("ui.settings.twitch.paste_token_no_token"),
                        icons::WARNING
                    )
                } else if self.tab_state.twitch_state.read().token_is_valid() {
                    format!(
                        "{} {} {}",
                        icons::CLIPBOARD_TEXT,
                        t!("ui.settings.twitch.paste_token_valid"),
                        icons::CHECK_CIRCLE
                    )
                } else {
                    format!(
                        "{} {} {}",
                        icons::CLIPBOARD_TEXT,
                        t!("ui.settings.twitch.paste_token_invalid"),
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
                ui.label(t!("ui.settings.twitch.monitored_channel"));
                let response = ui.text_edit_singleline(&mut self.tab_state.settings.twitch_monitored_channel);
                if response.lost_focus()
                    && let Some(tx) = self.tab_state.twitch_update_sender.as_ref()
                {
                    let _ = tx.blocking_send(crate::twitch::TwitchUpdate::User(
                        self.tab_state.settings.twitch_monitored_channel.clone(),
                    ));
                }
            });
        });
    }
}
