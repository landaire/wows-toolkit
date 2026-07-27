use egui_extras::Column;
use egui_extras::TableBuilder;
use itertools::Itertools;
use rust_i18n::t;

use crate::app::ToolkitTabViewer;

impl ToolkitTabViewer<'_> {
    pub(crate) fn build_current_match_sub_tab(&mut self, ui: &mut egui::Ui) {
        let player_tracker = self.tab_state.player_tracker.read();

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
                    ui.strong(t!("ui.player_tracker.column.player_name"));
                });
                header.col(|ui| {
                    ui.strong(t!("ui.player_tracker.column.twitch_names"));
                });
            });

            if let Some((match_timestamp, live_players)) = player_tracker.live_game_players.as_ref() {
                table.body(|mut body| {
                    let twitch_state = self.tab_state.twitch_state.read();
                    for player_name in live_players {
                        body.row(30.0, |mut row| {
                            row.col(|ui| {
                                ui.label(player_name);
                            });
                            if let Some(participant_info) =
                                twitch_state.player_is_potential_stream_sniper(player_name, *match_timestamp)
                            {
                                row.col(|ui| {
                                    for (participant, timestamps) in participant_info {
                                        let minutes_str = timestamps
                                            .iter()
                                            .map(|ts| {
                                                let delta = *ts - *match_timestamp;
                                                delta.total(jiff::Unit::Minute).unwrap_or(0.0) as i64
                                            })
                                            .join(", ");
                                        ui.label(participant)
                                            .on_hover_text(t!("ui.player_tracker.seen_minutes", minutes = minutes_str));
                                    }
                                });
                            } else {
                                row.col(|_| {});
                            }
                        });
                    }
                });
            }
        });
    }
}
