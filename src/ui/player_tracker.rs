use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::icons;
use crate::task;
use crate::util;
use egui::Color32;
use egui::RichText;
use egui_extras::Column;
use egui_extras::TableBuilder;
use itertools::Itertools;
use jiff::Timestamp;
use jiff::ToSpan;
use jiff::fmt::friendly::Designator;
use jiff::fmt::friendly::SpanPrinter;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::ReplayMeta;

use crate::app::ToolkitTabViewer;
use crate::ui::replay_parser::Replay;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PlayerTracker {
    tracked_players_by_time: BTreeMap<Timestamp, Vec<i64>>,
    tracked_players: HashMap<i64, TrackedPlayer>,
    filter_time_period: TimePeriod,
    sort_order: SortedBy,
    player_filter: String,

    #[serde(skip)]
    live_game_players: Option<(Timestamp, Vec<String>)>,
}

impl PlayerTracker {
    pub fn update_from_live_arena_info(&mut self, meta: &ReplayMeta) {
        // Clear the data from the last game
        self.live_game_players = None;

        let timestamp = util::replay_timestamp(meta);
        let players = meta.vehicles.iter().map(|player| player.name.clone()).collect();

        self.live_game_players = Some((timestamp, players))
    }
    pub fn update_from_replay(&mut self, replay: &Replay) {
        if !matches!(replay.replay_file.meta.gameType.as_str(), "RandomBattle" | "RankedBattle") {
            // Only update from randoms / ranked
            return;
        }

        if let Some(report) = replay.battle_report.as_ref() {
            let tracked_players = &mut self.tracked_players;
            let tracked_players_by_ts = &mut self.tracked_players_by_time;

            let timestamp = util::replay_timestamp(&replay.replay_file.meta);

            let self_player = report.players().iter().find(|player| {
                if let Some(meta_player) = replay.replay_file.meta.vehicles.iter().find(|metadata_player| metadata_player.name == player.name()) {
                    meta_player.relation == 0
                } else {
                    false
                }
            });

            for player in report.players() {
                if let Some(self_player) = self_player {
                    // Ignore ourselves and people in our division
                    if Arc::ptr_eq(self_player, player) || (self_player.division_id() > 0 && player.division_id() == self_player.division_id()) {
                        continue;
                    }
                }

                let tracked_player = tracked_players.entry(player.db_id()).or_default();
                if tracked_player.arena_ids.contains(&report.arena_id()) {
                    continue;
                }

                let mut update_metadata = false;

                if let Some(last_seen) = tracked_player.timestamps.first() {
                    if *last_seen < timestamp {
                        update_metadata = true;
                    }
                }

                if update_metadata || tracked_player.timestamps.is_empty() {
                    if update_metadata
                        && !tracked_player.names.contains(&tracked_player.last_name)
                        && tracked_player.last_name != player.name()
                        && !tracked_player.last_name.is_empty()
                    {
                        // If we need to update the name, let's add the name to the alias list
                        tracked_player.names.insert(tracked_player.last_name.clone());
                    }

                    tracked_player.last_name = player.name().to_string();

                    tracked_player.clan = player.clan().to_string();
                }

                tracked_player.db_id = player.db_id();
                tracked_player.clan_id = player.clan_id();
                tracked_player.timestamps.insert(timestamp);
                tracked_player.arena_ids.insert(report.arena_id());

                tracked_players_by_ts.entry(timestamp).or_default().push(player.db_id());
            }
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TrackedPlayer {
    last_name: String,
    db_id: i64,
    names: HashSet<String>,
    clan_id: i64,
    clan: String,
    timestamps: BTreeSet<Timestamp>,
    arena_ids: BTreeSet<i64>,
    #[serde(default)]
    notes: String,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
enum TimePeriod {
    LastHour,
    LastSixHours,
    #[default]
    LastDay,
    LastWeek,
    LastMonth,
    AllTime,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn icon(&self) -> &'static str {
        match self {
            SortOrder::Asc => icons::SORT_ASCENDING,
            SortOrder::Desc => icons::SORT_DESCENDING,
        }
    }

    fn toggle(&mut self) {
        match self {
            SortOrder::Asc => *self = SortOrder::Desc,
            SortOrder::Desc => *self = SortOrder::Asc,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum SortedBy {
    Name(SortOrder),
    Clan(SortOrder),
    LastEncountered(SortOrder),
    TimesEncountered(SortOrder),
    TimesEncounteredInTimeRange(SortOrder),
}

impl SortedBy {
    fn transition_to(&mut self, new: SortedBy) {
        match (self, new) {
            (SortedBy::Name(sort_order), SortedBy::Name(_)) => sort_order.toggle(),
            (SortedBy::Clan(sort_order), SortedBy::Clan(_)) => {
                sort_order.toggle();
            }
            (SortedBy::LastEncountered(sort_order), SortedBy::LastEncountered(_)) => {
                sort_order.toggle();
            }
            (SortedBy::TimesEncountered(sort_order), SortedBy::TimesEncountered(_)) => {
                sort_order.toggle();
            }
            (SortedBy::TimesEncounteredInTimeRange(sort_order), SortedBy::TimesEncounteredInTimeRange(_)) => {
                sort_order.toggle();
            }
            (old, new) => {
                *old = new;
            }
        }
    }
}

impl Default for SortedBy {
    fn default() -> Self {
        SortedBy::TimesEncounteredInTimeRange(SortOrder::Desc)
    }
}

impl TimePeriod {
    fn description(&self) -> &'static str {
        match self {
            TimePeriod::LastHour => "Past Hour",
            TimePeriod::LastSixHours => "Past 6 Hour",
            TimePeriod::LastDay => "Past 24 Hours",
            TimePeriod::LastWeek => "Past Week",
            TimePeriod::LastMonth => "Past Month",
            TimePeriod::AllTime => "All Time",
        }
    }

    fn to_date(self) -> Option<Timestamp> {
        let now = Timestamp::now();
        match self {
            TimePeriod::LastHour => Some(now - 1.hour()),
            TimePeriod::LastSixHours => Some(now - 6.hours()),
            TimePeriod::LastDay => Some(now - 1.day()),
            TimePeriod::LastWeek => Some(now - 7.days()),
            TimePeriod::LastMonth => Some(now - 1.month()),
            TimePeriod::AllTime => None,
        }
    }
}

impl ToolkitTabViewer<'_> {
    pub fn build_player_tracker_tab(&mut self, ui: &mut egui::Ui) {
        let mut player_tracker_settings = self.tab_state.settings.player_tracker.write();
        let player_tracker_settings = &mut *player_tracker_settings;
        let filter_lower = player_tracker_settings.player_filter.to_ascii_lowercase();
        let now = Timestamp::now();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button("Clear Stats").clicked() {
                    player_tracker_settings.tracked_players.clear();
                    player_tracker_settings.tracked_players_by_time.clear();
                }

                let selected = &mut player_tracker_settings.filter_time_period;
                egui::ComboBox::from_id_salt("player_inspector_time_period_selection").selected_text(selected.description()).show_ui(ui, |ui| {
                    ui.selectable_value(selected, TimePeriod::LastHour, "Past Hour");
                    ui.selectable_value(selected, TimePeriod::LastSixHours, "Past 6 Hours");
                    ui.selectable_value(selected, TimePeriod::LastDay, "Past 24 Hours");
                    ui.selectable_value(selected, TimePeriod::LastWeek, "Past Week");
                    ui.selectable_value(selected, TimePeriod::LastMonth, "Past Month");
                    ui.selectable_value(selected, TimePeriod::AllTime, "All Time");
                });
                ui.label("Player Filter");
                ui.text_edit_singleline(&mut player_tracker_settings.player_filter);
                if let Some(replay_files) = self.tab_state.replay_files.as_ref() {
                    if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
                        if ui.button("Populate Data From Replays").clicked() {
                            crate::update_background_task!(
                                self.tab_state.background_tasks,
                                Some(task::start_populating_player_inspector(
                                    replay_files.keys().cloned().collect(),
                                    Arc::clone(wows_data),
                                    Arc::clone(&self.tab_state.settings.player_tracker)
                                ))
                            );
                        }
                    }
                }
            });

            ui.add_space(10.0);

            ui.separator();
            egui::SidePanel::left("current_match_side_panel").default_width(450.0).show_inside(ui, |ui| {
                ui.vertical(|ui| {
                    ui.heading("Players in Current Match");
                    egui::ScrollArea::both().id_salt("current_match_scroll_area").show(ui, |ui| {
                        let table = TableBuilder::new(ui)
                            .striped(true)
                            .resizable(true)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(Column::initial(115.0).clip(true))
                            .column(Column::initial(135.0).clip(true))
                            .column(Column::remainder())
                            .min_scrolled_height(0.0)
                            .id_salt("live_game_table");

                        let table = table.header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Player Name");
                            });
                            header.col(|ui| {
                                ui.strong("Possible Twitch Names");
                            });
                        });

                        if let Some((match_timestamp, live_players)) = player_tracker_settings.live_game_players.as_ref() {
                            table.body(|mut body| {
                                let twitch_state = self.tab_state.twitch_state.read();
                                for player_name in live_players {
                                    body.row(30.0, |mut row| {
                                        row.col(|ui| {
                                            ui.label(player_name);
                                        });
                                        if let Some(participant_info) = twitch_state.player_is_potential_stream_sniper(player_name, *match_timestamp) {
                                            row.col(|ui| {
                                                for (participant, timestamps) in participant_info {
                                                    ui.label(participant).on_hover_text(format!(
                                                        "Seen {} minutes after match start",
                                                        timestamps
                                                            .iter()
                                                            .map(|ts| {
                                                                let delta = *ts - *match_timestamp;
                                                                delta.get_minutes()
                                                            })
                                                            .join(", ")
                                                    ));
                                                }
                                            });
                                        } else {
                                            row.col(|_| {
                                                // nothing to show
                                            });
                                        }
                                    });
                                }
                            });
                        }
                    });
                });
            });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.heading("Historical Matches");
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
                        .column(Column::remainder())
                        .min_scrolled_height(0.0);

                    let sorted_by = player_tracker_settings.sort_order;
                    table
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                let raw_text = "Clan";
                                let text = if let SortedBy::Clan(sort_order) = sorted_by { format!("{} {}", raw_text, sort_order.icon()) } else { raw_text.to_string() };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings.sort_order.transition_to(SortedBy::Clan(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                let raw_text = "Player Name";
                                let text = if let SortedBy::Name(sort_order) = sorted_by { format!("{} {}", raw_text, sort_order.icon()) } else { raw_text.to_string() };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings.sort_order.transition_to(SortedBy::Name(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                ui.strong("WG ID");
                            });
                            header.col(|ui| {
                                let raw_text = "Total Encounters";
                                let text = if let SortedBy::TimesEncountered(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text.to_string()
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings.sort_order.transition_to(SortedBy::TimesEncountered(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                let raw_text = "Encounters in Time Range";
                                let text = if let SortedBy::TimesEncounteredInTimeRange(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text.to_string()
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings.sort_order.transition_to(SortedBy::TimesEncounteredInTimeRange(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                let raw_text = "Last Encountered";
                                let text = if let SortedBy::LastEncountered(sort_order) = sorted_by {
                                    format!("{} {}", raw_text, sort_order.icon())
                                } else {
                                    raw_text.to_string()
                                };

                                if ui.strong(text).clicked() {
                                    player_tracker_settings.sort_order.transition_to(SortedBy::LastEncountered(SortOrder::Asc));
                                }
                            });
                            header.col(|ui| {
                                ui.strong("Aliases");
                            });
                            header.col(|ui| {
                                ui.strong("Notes");
                            });
                        })
                        .body(|mut body| {
                            let tracked_players_by_ts = &player_tracker_settings.tracked_players_by_time;
                            // Filter by the date range
                            let player_range: BTreeSet<_> = if let Some(filter_range) = player_tracker_settings.filter_time_period.to_date() {
                                tracked_players_by_ts.iter().filter_map(|(ts, ids)| if *ts > filter_range { Some(ids) } else { None }).flatten().cloned().collect()
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
                                                || player.names.iter().any(|name| name.to_ascii_lowercase().contains(&filter_lower)))
                                    } else {
                                        player_range.contains(id)
                                    }
                                })
                                .sorted_by(|(_ida, playera), (_idb, playerb)| match sorted_by {
                                    SortedBy::Name(sort_order) => {
                                        let playera_name = &playera.last_name;
                                        let playerb_name = &playerb.last_name;

                                        if sort_order == SortOrder::Asc { playera_name.cmp(playerb_name) } else { playerb_name.cmp(playera_name) }
                                    }
                                    SortedBy::Clan(sort_order) => {
                                        let playera_clan = &playera.clan;
                                        let playerb_clan = &playerb.clan;

                                        if sort_order == SortOrder::Asc { playera_clan.cmp(playerb_clan) } else { playerb_clan.cmp(playera_clan) }
                                    }
                                    SortedBy::LastEncountered(sort_order) => {
                                        let playera_last = playera.timestamps.last().unwrap();
                                        let playerb_last = playerb.timestamps.last().unwrap();

                                        if sort_order == SortOrder::Asc { playera_last.cmp(playerb_last) } else { playerb_last.cmp(playera_last) }
                                    }
                                    SortedBy::TimesEncountered(sort_order) => {
                                        let playera_count = playera.timestamps.len();
                                        let playerb_count = playerb.timestamps.len();

                                        if sort_order == SortOrder::Asc { playera_count.cmp(&playerb_count) } else { playerb_count.cmp(&playera_count) }
                                    }
                                    SortedBy::TimesEncounteredInTimeRange(sort_order) => {
                                        let (playera_count, playerb_count) = if let Some(filter_range) = player_tracker_settings.filter_time_period.to_date() {
                                            let playera_count = playera.timestamps.iter().filter(|ts| **ts > filter_range).count();
                                            let playerb_count = playerb.timestamps.iter().filter(|ts| **ts > filter_range).count();

                                            (playera_count, playerb_count)
                                        } else {
                                            let playera_count = playera.timestamps.len();
                                            let playerb_count = playerb.timestamps.len();

                                            (playera_count, playerb_count)
                                        };

                                        if sort_order == SortOrder::Asc { playera_count.cmp(&playerb_count) } else { playerb_count.cmp(&playera_count) }
                                    }
                                });

                            for (player_id, player) in players {
                                body.row(30.0, |mut row| {
                                    let times_encountered = player.arena_ids.len();
                                    let times_encountered_in_range = if let Some(filter_range) = player_tracker_settings.filter_time_period.to_date() {
                                        player.timestamps.iter().filter(|ts| **ts > filter_range).count()
                                    } else {
                                        times_encountered
                                    };

                                    let encounters_color = match times_encountered_in_range {
                                        0..=1 => None,
                                        2..=3 => Some(Color32::YELLOW),
                                        4..=5 => Some(Color32::ORANGE),
                                        _ => Some(Color32::LIGHT_RED),
                                    };

                                    row.col(|ui| {
                                        ui.label(&player.clan);
                                    });
                                    row.col(|ui| {
                                        let text = RichText::new(&player.last_name);
                                        let text = if let Some(color) = encounters_color { text.color(color) } else { text };

                                        ui.label(text);
                                    });
                                    row.col(|ui| {
                                        ui.label(player_id.to_string());
                                    });
                                    row.col(|ui| {
                                        let text = RichText::new(times_encountered.to_string());
                                        let text = if let Some(color) = encounters_color { text.color(color) } else { text };
                                        ui.label(text);
                                    });
                                    row.col(|ui| {
                                        let text = RichText::new(times_encountered_in_range.to_string());
                                        let text = if let Some(color) = encounters_color { text.color(color) } else { text };
                                        ui.label(text);
                                    });
                                    row.col(|ui| {
                                        let timestamp = player.timestamps.last().unwrap();
                                        let delta = now - *timestamp;
                                        let printer = SpanPrinter::new().designator(Designator::HumanTime);

                                        let delta_text = printer.span_to_string(&delta);
                                        ui.label(delta_text).on_hover_text(timestamp.strftime("%Y-%m-%d %H:%M:%S").to_string());
                                    });
                                    row.col(|ui| {
                                        ui.label(player.names.iter().join(", "));
                                    });
                                    row.col(|ui| {
                                        ui.text_edit_singleline(&mut player.notes);
                                    });
                                });
                            }
                        });
                });
            });
        });
    }
}
