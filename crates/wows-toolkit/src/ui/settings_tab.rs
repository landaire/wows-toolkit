use std::path::Path;
use std::path::PathBuf;

use egui::Color32;
use egui::OpenUrl;
use egui::RichText;
use egui::Slider;
use rust_i18n::t;

use crate::app::ToolkitTabViewer;
use crate::data::settings::AppPreferences;
use crate::icons;
use crate::task::DataExportSettings;
use crate::task::ReplayBackgroundParserThreadMessage;
use crate::task::ReplayExportFormat;
use crate::twitch::Token;
use crate::update_background_task;

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
                let mut check_for_updates = self.tab_state.persisted.read().settings.app.check_for_updates;
                if ui.checkbox(&mut check_for_updates, t!("ui.settings.app.check_for_updates")).changed() {
                    self.tab_state.persisted.write().settings.app.check_for_updates = check_for_updates;
                }
                let mut enable_logging = self.tab_state.persisted.read().settings.app.enable_logging;
                if ui
                    .checkbox(&mut enable_logging, t!("ui.settings.app.enable_logging"))
                    .on_hover_text(t!("ui.settings.app.enable_logging_tooltip"))
                    .changed()
                {
                    self.tab_state.persisted.write().settings.app.enable_logging = enable_logging;
                }
                let mut send_replay_data = self.tab_state.persisted.read().settings.integrations.send_replay_data;
                if ui.checkbox(&mut send_replay_data, t!("ui.settings.app.send_replay_data")).changed() {
                    self.tab_state.persisted.write().settings.integrations.send_replay_data = send_replay_data;
                    self.tab_state.send_replay_consent_changed();
                }
                ui.horizontal(|ui| {
                    let mut zoom = ui.ctx().zoom_factor();
                    if ui.add(Slider::new(&mut zoom, 0.5..=2.0).text(t!("ui.settings.app.zoom_factor"))).changed() {
                        ui.ctx().set_zoom_factor(zoom);
                        self.tab_state.persisted.write().settings.app.zoom_factor = zoom;
                    }
                    if ui.button(t!("ui.buttons.reset")).clicked() {
                        let default_zoom = AppPreferences::default().zoom_factor;
                        ui.ctx().set_zoom_factor(default_zoom);
                        self.tab_state.persisted.write().settings.app.zoom_factor = default_zoom;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t!("ui.settings.app.language"));
                    let current_locale =
                        self.tab_state.persisted.read().settings.app.locale.clone().unwrap_or_else(|| "en".into());
                    let current_name = wt_translations::language_name(&current_locale).unwrap_or("English");
                    egui::ComboBox::from_id_salt("language_selector").selected_text(current_name).show_ui(ui, |ui| {
                        for lang in wt_translations::SUPPORTED_LANGUAGES {
                            if ui.selectable_label(current_locale == lang.code, lang.native_name).clicked() {
                                self.tab_state.persisted.write().settings.app.locale = Some(lang.code.to_string());
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

                        let show_text_error = self.tab_state.wows_dir_invalid;

                        let mut wows_dir = self.tab_state.persisted.read().settings.game.wows_dir.clone();
                        let response = ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::singleline(&mut wows_dir)
                                .interactive(self.tab_state.can_change_wows_dir)
                                .hint_text(t!("ui.settings.wows.directory_hint"))
                                .text_color_opt(show_text_error.then_some(Color32::LIGHT_RED)),
                        );

                        // If someone pastes or types a path, revalidate and reload if valid.
                        if response.changed() {
                            self.tab_state.persisted.write().settings.game.wows_dir = wows_dir.clone();
                            self.tab_state.revalidate_wows_dir();
                            if !self.tab_state.wows_dir_invalid && !wows_dir.is_empty() {
                                let path = Path::new(&wows_dir).to_owned();
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
                    let mut p = self.tab_state.persisted.write();
                    let rs = &mut p.settings.replay;

                    ui.checkbox(&mut rs.show_raw_xp, t!("ui.settings.replay.show_raw_xp"));
                    ui.checkbox(&mut rs.show_entity_id, t!("ui.settings.replay.show_entity_id"));
                    ui.end_row();

                    ui.checkbox(&mut rs.show_observed_damage, t!("ui.settings.replay.show_observed_damage"));
                    ui.checkbox(&mut rs.show_fires, t!("ui.settings.replay.show_fires"));
                    ui.end_row();

                    ui.checkbox(&mut rs.show_floods, t!("ui.settings.replay.show_floods"));
                    ui.checkbox(&mut rs.show_citadels, t!("ui.settings.replay.show_citadels"));
                    ui.end_row();

                    ui.checkbox(&mut rs.show_crits, t!("ui.settings.replay.show_crits"));
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
                                self.tab_state.persisted.write().settings.replay.auto_export_path =
                                    folder.to_string_lossy().to_string();
                                alert_data_export_change = true;
                            }
                        }

                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let mut p = self.tab_state.persisted.write();
                            let rs = &mut p.settings.replay;

                            if ui
                                .checkbox(&mut rs.auto_export_data, t!("ui.settings.replay.auto_export_data"))
                                .changed()
                            {
                                alert_data_export_change = true;
                            }

                            let previously_selected_format = rs.auto_export_format;
                            egui::ComboBox::from_id_salt("auto_export_format_combobox")
                                .selected_text(rs.auto_export_format.as_str())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut rs.auto_export_format,
                                        ReplayExportFormat::Json,
                                        t!("ui.settings.replay.format_json"),
                                    );
                                    ui.selectable_value(
                                        &mut rs.auto_export_format,
                                        ReplayExportFormat::Csv,
                                        t!("ui.settings.replay.format_csv"),
                                    );
                                    ui.selectable_value(
                                        &mut rs.auto_export_format,
                                        ReplayExportFormat::Cbor,
                                        t!("ui.settings.replay.format_cbor"),
                                    );
                                });
                            if previously_selected_format != rs.auto_export_format {
                                alert_data_export_change = true;
                            }
                            let path = Path::new(&rs.auto_export_path);
                            let path_is_valid = path.exists() && path.is_dir();
                            let response = ui.add_sized(
                                ui.available_size(),
                                egui::TextEdit::singleline(&mut rs.auto_export_path)
                                    .hint_text(t!("ui.settings.replay.export_path_hint"))
                                    .text_color_opt((!path_is_valid).then_some(Color32::LIGHT_RED)),
                            );

                            if response.lost_focus() {
                                let path = Path::new(&rs.auto_export_path);
                                if path.exists() && path.is_dir() {
                                    alert_data_export_change = true;
                                }
                            }
                        });
                    });

                    if alert_data_export_change {
                        let p = self.tab_state.persisted.read();
                        let rs = &p.settings.replay;
                        let _ = self.tab_state.background_parser_tx.as_ref().map(|tx| {
                            tx.send(ReplayBackgroundParserThreadMessage::DataAutoExportSettingChange(
                                DataExportSettings {
                                    should_auto_export: rs.auto_export_data,
                                    export_path: PathBuf::from(rs.auto_export_path.clone()),
                                    export_format: rs.auto_export_format,
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
                {
                    let mut p = self.tab_state.persisted.write();
                    ui.text_edit_singleline(&mut p.settings.collab.display_name);
                    ui.checkbox(
                        &mut p.settings.collab.suppress_p2p_ip_warning,
                        t!("ui.settings.session.suppress_ip_warning"),
                    )
                    .on_hover_text(t!("ui.settings.session.ip_warning_tooltip"));
                    ui.checkbox(
                        &mut p.settings.collab.disable_auto_open_session_windows,
                        t!("ui.settings.session.disable_auto_open"),
                    )
                    .on_hover_text(t!("ui.settings.session.auto_open_tooltip"));
                }
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

                let text = if self.tab_state.persisted.read().settings.integrations.twitch_token.is_none() {
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
                        self.tab_state.persisted.write().settings.integrations.twitch_token = Some(token.clone());
                        let _ = tx.blocking_send(crate::twitch::TwitchUpdate::Token(token));
                    }
                }
                ui.label(t!("ui.settings.twitch.monitored_channel"));
                let mut monitored_channel =
                    self.tab_state.persisted.read().settings.integrations.twitch_monitored_channel.clone();
                let response = ui.text_edit_singleline(&mut monitored_channel);
                if response.changed() {
                    self.tab_state.persisted.write().settings.integrations.twitch_monitored_channel =
                        monitored_channel.clone();
                }
                if response.lost_focus()
                    && let Some(tx) = self.tab_state.twitch_update_sender.as_ref()
                {
                    let _ = tx.blocking_send(crate::twitch::TwitchUpdate::User(monitored_channel));
                }
            });
        });
    }
}
