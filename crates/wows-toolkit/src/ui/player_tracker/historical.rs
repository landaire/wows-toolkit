use std::collections::HashSet;
use std::sync::Arc;

use egui::RichText;
use egui_extras::Column;
use egui_extras::TableBuilder;
use itertools::Itertools;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use rust_i18n::t;
use wows_replays::types::AccountId;

use crate::app::ToolkitTabViewer;
use crate::task;

use super::SortOrder;
use super::SortedBy;
use super::TimePeriod;
use super::encounter_severity_color;
use super::last_seen_text;

impl ToolkitTabViewer<'_> {
    pub(crate) fn build_historical_sub_tab(&mut self, ui: &mut egui::Ui) {
        // Collected during the table pass and applied after the table (the
        // player-tracker write lock is held while rendering rows, so we can't
        // touch other `self.tab_state` fields from inside a row closure).
        let mut find_matches_target: Option<AccountId> = None;

        // Scoped so the write guard is released before the deferred actions run.
        {
            let mut player_tracker_settings = self.tab_state.player_tracker.write();
            let player_tracker_settings = &mut *player_tracker_settings;
            let filter_lower = player_tracker_settings.player_filter.to_ascii_lowercase();
            let now = Timestamp::now();
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.button(t!("ui.player_tracker.clear_stats")).clicked() {
                        player_tracker_settings.tracked_players.clear();
                        player_tracker_settings.tracked_players_by_time.clear();
                    }

                    let selected = &mut player_tracker_settings.filter_time_period;
                    egui::ComboBox::from_id_salt("player_inspector_time_period_selection")
                        .selected_text(selected.description())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastHour,
                                t!("ui.player_tracker.period.past_hour"),
                            );
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastSixHours,
                                t!("ui.player_tracker.period.past_six_hours"),
                            );
                            ui.selectable_value(selected, TimePeriod::LastDay, t!("ui.player_tracker.period.past_day"));
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastWeek,
                                t!("ui.player_tracker.period.past_week"),
                            );
                            ui.selectable_value(
                                selected,
                                TimePeriod::LastMonth,
                                t!("ui.player_tracker.period.past_month"),
                            );
                            ui.selectable_value(selected, TimePeriod::AllTime, t!("ui.player_tracker.period.all_time"));
                        });
                    ui.label(t!("ui.player_tracker.player_filter"));
                    ui.text_edit_singleline(&mut player_tracker_settings.player_filter);

                    // Never a silent no-op: only enable the button when at least one of the
                    // two population paths (durable index, or on-demand replay re-parse) has
                    // its prerequisites available.
                    let index_path_available =
                        self.tab_state.db_pool.is_some() && self.tab_state.tokio_runtime.is_some();
                    let fallback_path_available =
                        self.tab_state.replay_files.is_some() && self.tab_state.wows_data_map.is_some();
                    let populate_enabled = index_path_available || fallback_path_available;

                    if ui
                        .add_enabled(populate_enabled, egui::Button::new(t!("ui.player_tracker.populate_from_replays")))
                        .clicked()
                    {
                        // Prefer the durable index: it's already parsed and avoids
                        // re-reading every replay from disk. Fall back to the
                        // background re-parse task when the index has nothing yet.
                        let populated_from_index =
                            match (self.tab_state.db_pool.as_ref(), self.tab_state.tokio_runtime.as_ref()) {
                                (Some(pool), Some(rt)) => player_tracker_settings.populate_from_index(pool, rt),
                                _ => false,
                            };

                        if !populated_from_index
                            && let Some(replay_files) = self.tab_state.replay_files.as_ref()
                            && let Some(wows_data_map) = self.tab_state.wows_data_map.as_ref()
                        {
                            crate::update_background_task!(
                                self.tab_state.background_tasks,
                                Some(task::start_populating_player_inspector(
                                    replay_files.keys().cloned().collect(),
                                    wows_data_map.clone(),
                                    Arc::clone(&self.tab_state.player_tracker)
                                ))
                            );
                        }
                    }
                });

                ui.add_space(10.0);

                ui.separator();

                egui::ScrollArea::horizontal().id_salt("player_tracker_central").show(ui, |ui| {
                    let table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::initial(60.0).clip(true))
                        .column(Column::initial(115.0).clip(true))
                        .column(Column::initial(65.0).clip(true))
                        .column(Column::initial(115.0).clip(true))
                        .column(Column::initial(165.0).clip(true))
                        .column(Column::initial(130.0).clip(true))
                        .column(Column::initial(200.0).clip(true))
                        .column(Column::initial(110.0).clip(true))
                        .column(Column::remainder())
                        .min_scrolled_height(0.0);

                    let sorted_by = player_tracker_settings.sort_order;
                    table
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                let raw_text: String = t!("ui.player_tracker.column.clan").into();
                                let text = if let SortedBy::Clan(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings.sort_order.transition_to(SortedBy::Clan(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                let raw_text: String = t!("ui.player_tracker.column.player_name").into();
                                let text = if let SortedBy::Name(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings.sort_order.transition_to(SortedBy::Name(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                ui.strong(t!("ui.player_tracker.column.wg_id"));
                            });
                            header.col(|ui| {
                                let raw_text: String = t!("ui.player_tracker.column.total_encounters").into();
                                let text = if let SortedBy::TimesEncountered(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings
                                        .sort_order
                                        .transition_to(SortedBy::TimesEncountered(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                let raw_text: String = t!("ui.player_tracker.column.encounters_in_range").into();
                                let text = if let SortedBy::TimesEncounteredInTimeRange(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings
                                        .sort_order
                                        .transition_to(SortedBy::TimesEncounteredInTimeRange(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                let raw_text: String = t!("ui.player_tracker.column.last_encountered").into();
                                let text = if let SortedBy::LastEncountered(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings
                                        .sort_order
                                        .transition_to(SortedBy::LastEncountered(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                ui.strong(t!("ui.player_tracker.column.aliases"));
                            });
                            header.col(|_ui| {});
                            header.col(|ui| {
                                ui.strong(t!("ui.player_tracker.column.notes"));
                            });
                        })
                        .body(|mut body| {
                            let tracked_players_by_ts = &player_tracker_settings.tracked_players_by_time;
                            // Filter by the date range
                            let player_range: HashSet<_> =
                                if let Some(filter_range) = player_tracker_settings.filter_time_period.to_date() {
                                    tracked_players_by_ts
                                        .iter()
                                        .filter_map(|(ts, ids)| if *ts > filter_range { Some(ids) } else { None })
                                        .flatten()
                                        .cloned()
                                        .collect()
                                } else {
                                    tracked_players_by_ts.iter().flat_map(|(_ts, ids)| ids).cloned().collect()
                                };

                            let tracked_players = &mut player_tracker_settings.tracked_players;
                            let players = tracked_players
                                .iter_mut()
                                .filter(|(id, player)| {
                                    if !player_tracker_settings.player_filter.is_empty() {
                                        player_range.contains(id)
                                            && (player.clan.to_ascii_lowercase().contains(&filter_lower)
                                                || player.last_name.to_ascii_lowercase().contains(&filter_lower)
                                                || player
                                                    .names
                                                    .iter()
                                                    .any(|name| name.to_ascii_lowercase().contains(&filter_lower)))
                                    } else {
                                        player_range.contains(id)
                                    }
                                })
                                .sorted_by(|(_ida, playera), (_idb, playerb)| match sorted_by {
                                    SortedBy::Name(sort_order) => {
                                        let playera_name = &playera.last_name;
                                        let playerb_name = &playerb.last_name;

                                        if sort_order == SortOrder::Asc {
                                            playera_name.cmp(playerb_name)
                                        } else {
                                            playerb_name.cmp(playera_name)
                                        }
                                    }
                                    SortedBy::Clan(sort_order) => {
                                        let playera_clan = &playera.clan;
                                        let playerb_clan = &playerb.clan;

                                        if sort_order == SortOrder::Asc {
                                            playera_clan.cmp(playerb_clan)
                                        } else {
                                            playerb_clan.cmp(playera_clan)
                                        }
                                    }
                                    SortedBy::LastEncountered(sort_order) => {
                                        let playera_last = playera.timestamps.last().unwrap();
                                        let playerb_last = playerb.timestamps.last().unwrap();

                                        if sort_order == SortOrder::Asc {
                                            playera_last.cmp(playerb_last)
                                        } else {
                                            playerb_last.cmp(playera_last)
                                        }
                                    }
                                    SortedBy::TimesEncountered(sort_order) => {
                                        let playera_count = playera.timestamps.len();
                                        let playerb_count = playerb.timestamps.len();

                                        if sort_order == SortOrder::Asc {
                                            playera_count.cmp(&playerb_count)
                                        } else {
                                            playerb_count.cmp(&playera_count)
                                        }
                                    }
                                    SortedBy::TimesEncounteredInTimeRange(sort_order) => {
                                        let (playera_count, playerb_count) = if let Some(filter_range) =
                                            player_tracker_settings.filter_time_period.to_date()
                                        {
                                            let playera_count =
                                                playera.timestamps.iter().filter(|ts| **ts > filter_range).count();
                                            let playerb_count =
                                                playerb.timestamps.iter().filter(|ts| **ts > filter_range).count();

                                            (playera_count, playerb_count)
                                        } else {
                                            let playera_count = playera.timestamps.len();
                                            let playerb_count = playerb.timestamps.len();

                                            (playera_count, playerb_count)
                                        };

                                        if sort_order == SortOrder::Asc {
                                            playera_count.cmp(&playerb_count)
                                        } else {
                                            playerb_count.cmp(&playera_count)
                                        }
                                    }
                                });

                            for (player_id, player) in players {
                                body.row(30.0, |mut row| {
                                    let times_encountered = player.arena_ids.len();
                                    let times_encountered_in_range = if let Some(filter_range) =
                                        player_tracker_settings.filter_time_period.to_date()
                                    {
                                        player.timestamps.iter().filter(|ts| **ts > filter_range).count()
                                    } else {
                                        times_encountered
                                    };

                                    row.col(|ui| {
                                        ui.label(&player.clan);
                                    });
                                    row.col(|ui| {
                                        let text = RichText::new(&player.last_name);
                                        let text = if let Some(color) =
                                            encounter_severity_color(ui, times_encountered_in_range)
                                        {
                                            text.color(color)
                                        } else {
                                            text
                                        };

                                        ui.label(text);
                                    });
                                    row.col(|ui| {
                                        ui.label(player_id.to_string());
                                    });
                                    row.col(|ui| {
                                        let text = RichText::new(times_encountered.to_string());
                                        let text = if let Some(color) =
                                            encounter_severity_color(ui, times_encountered_in_range)
                                        {
                                            text.color(color)
                                        } else {
                                            text
                                        };
                                        ui.label(text);
                                    });
                                    row.col(|ui| {
                                        let text = RichText::new(times_encountered_in_range.to_string());
                                        let text = if let Some(color) =
                                            encounter_severity_color(ui, times_encountered_in_range)
                                        {
                                            text.color(color)
                                        } else {
                                            text
                                        };
                                        ui.label(text);
                                    });
                                    row.col(|ui| {
                                        let label = ui.label(last_seen_text(player, now));
                                        if let Some(last) = player.timestamps.last() {
                                            let timestamp = last.to_zoned(TimeZone::system());
                                            label.on_hover_text(timestamp.strftime("%Y-%m-%d %H:%M:%S").to_string());
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(player.names.iter().join(", "));
                                    });
                                    row.col(|ui| {
                                        if ui.button(t!("ui.player_tracker.find_matches")).clicked() {
                                            find_matches_target = Some(*player_id);
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.text_edit_singleline(&mut player.notes);
                                    });
                                });
                            }
                        });
                });
            });
        }

        if let Some(id) = find_matches_target {
            self.queue_player_search(id);
        }
    }
}
