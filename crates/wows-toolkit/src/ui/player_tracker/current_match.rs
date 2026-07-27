use std::collections::HashMap;

use egui::Color32;
use egui::Image;
use egui::ImageSource;
use egui::RichText;
use egui::Vec2;
use egui_extras::Column;
use egui_extras::TableBuilder;
use jiff::Timestamp;
use rust_i18n::t;
use wows_replays::types::AccountId;

use crate::app::ToolkitTabViewer;
use crate::data::wows_data::WorldOfWarshipsData;
use crate::icons;
use crate::twitch::TwitchState;
use crate::ui::replay_parser::ship_class_icon_from_species;
use crate::ui::theme::semantic::SemanticExt;

use super::TrackedPlayer;
use super::encounter_severity_color;
use super::last_seen_text;
use super::live::LiveRosterRow;

impl ToolkitTabViewer<'_> {
    pub(crate) fn build_current_match_sub_tab(&mut self, ui: &mut egui::Ui) {
        // Collected during the table pass and applied after, because the
        // player-tracker lock is held while rendering rows.
        let mut actions = TeamActions::default();

        {
            let mut player_tracker = self.tab_state.player_tracker.write();
            let filter_range = player_tracker.filter_time_period.to_date();
            let now = Timestamp::now();

            let build = player_tracker.live_match.as_ref().and_then(|live| live.build);
            let wows_data = build.zip(self.tab_state.wows_data_map.as_ref()).and_then(|(build, map)| map.get(build));
            let wows_data_guard = wows_data.as_ref().map(|data| data.read());
            // SharedWoWsData is Arc<RwLock<Box<WorldOfWarshipsData>>>, so the
            // guard needs three derefs to reach the data itself.
            let wows_data_ref: Option<&WorldOfWarshipsData> = wows_data_guard.as_ref().map(|guard| &***guard);

            // Scoped so the roster borrow ends before `tracked_players` is read.
            let (started_at, friendly, enemy) = {
                let Some(roster) = player_tracker.roster(wows_data_ref) else {
                    ui.label(RichText::new(t!("ui.player_tracker.no_live_match")).weak());
                    return;
                };
                (roster.started_at, roster.friendly.clone(), roster.enemy.clone())
            };

            let twitch_state = self.tab_state.twitch_state.read();
            let ctx = TeamContext {
                tracked: &player_tracker.tracked_players,
                wows_data: wows_data_ref,
                twitch_state: &twitch_state,
                started_at,
                now,
                filter_range,
            };

            ui.columns(2, |columns| {
                let win = columns[0].sem().win;
                render_team(
                    &mut columns[0],
                    t!("ui.player_tracker.your_team", count = friendly.len()).to_string(),
                    win,
                    &friendly,
                    "current_match_friendly",
                    &ctx,
                    &mut actions,
                );

                let loss = columns[1].sem().loss;
                render_team(
                    &mut columns[1],
                    t!("ui.player_tracker.enemy_team", count = enemy.len()).to_string(),
                    loss,
                    &enemy,
                    "current_match_enemy",
                    &ctx,
                    &mut actions,
                );
            });
        }

        if let Some(login) = actions.copy_login {
            self.tab_state.toasts.lock().success(t!("ui.twitch.copied", name = &login).to_string());
            ui.ctx().copy_text(login);
        }
        if let Some(id) = actions.find_matches_target {
            self.queue_player_search(id);
        }
    }
}

/// Everything both team tables read. Bundled because the two calls differ only
/// in their rows, heading and id salt.
struct TeamContext<'a> {
    tracked: &'a HashMap<AccountId, TrackedPlayer>,
    wows_data: Option<&'a WorldOfWarshipsData>,
    twitch_state: &'a TwitchState,
    started_at: Timestamp,
    now: Timestamp,
    filter_range: Option<Timestamp>,
}

/// What a row click asks the caller to do once the player-tracker lock is
/// released.
#[derive(Default)]
struct TeamActions {
    find_matches_target: Option<AccountId>,
    copy_login: Option<String>,
}

/// One team's table.
fn render_team(
    ui: &mut egui::Ui,
    heading: String,
    heading_color: Color32,
    rows: &[LiveRosterRow],
    id_salt: &'static str,
    ctx: &TeamContext<'_>,
    actions: &mut TeamActions,
) {
    ui.label(RichText::new(heading).heading().color(heading_color));

    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(150.0).clip(true))
        .column(Column::remainder().clip(true))
        .column(Column::initial(60.0).clip(true))
        .column(Column::initial(30.0))
        .min_scrolled_height(0.0)
        .id_salt(id_salt)
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.strong(t!("ui.player_tracker.column.ship"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.player_tracker.column.player_name"));
            });
            header.col(|ui| {
                ui.strong(t!("ui.player_tracker.column.encounters"));
            });
            header.col(|_ui| {});
        })
        .body(|mut body| {
            for row_data in rows {
                body.row(30.0, |mut row| {
                    row.col(|ui| {
                        let icon = row_data
                            .species
                            .zip(ctx.wows_data)
                            .and_then(|(species, data)| ship_class_icon_from_species(species, data));
                        if let Some(icon) = icon {
                            let image = Image::new(ImageSource::Bytes {
                                uri: icon.path.clone().into(),
                                bytes: icon.data.clone().into(),
                            })
                            .tint(row_data.tint.color(ui.visuals()))
                            .fit_to_exact_size((20.0, 20.0).into())
                            .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));
                            let response = ui.add(image);
                            if let Some(species_text) = row_data.species_text.as_ref() {
                                response.on_hover_text(species_text);
                            }
                        }
                        if let Some(ship_name) = row_data.ship_name.as_ref() {
                            ui.label(ship_name);
                        } else if let Some(species_text) = row_data.species_text.as_ref() {
                            ui.label(species_text);
                        }
                    });
                    row.col(|ui| {
                        ui.label(RichText::new(&row_data.name).color(row_data.tint.color(ui.visuals())));

                        if let Some(candidates) =
                            ctx.twitch_state.player_is_potential_stream_sniper(&row_data.name, ctx.started_at)
                            && let Some(login) = crate::ui::widgets::twitch_chip(ui, &candidates, ctx.started_at)
                        {
                            actions.copy_login = Some(login);
                        }

                        if let Some(player) = row_data.tracked.and_then(|id| ctx.tracked.get(&id))
                            && !player.notes.is_empty()
                        {
                            ui.label(icons::NOTE_PENCIL).on_hover_text(&player.notes);
                        }
                    });
                    row.col(|ui| {
                        let Some(player) = row_data.tracked.and_then(|id| ctx.tracked.get(&id)) else {
                            ui.label(RichText::new("-").weak())
                                .on_hover_text(t!("ui.player_tracker.never_encountered"));
                            return;
                        };

                        let total = player.arena_ids.len();
                        let in_range = match ctx.filter_range {
                            Some(range) => player.timestamps.iter().filter(|ts| **ts > range).count(),
                            None => total,
                        };
                        let last = last_seen_text(player, ctx.now);

                        let mut text = RichText::new(format!("x{total}"));
                        if let Some(color) = encounter_severity_color(ui, in_range) {
                            text = text.color(color);
                        }
                        ui.label(text).on_hover_text(t!(
                            "ui.player_tracker.encounters_hover",
                            total = total,
                            range = in_range,
                            last = last
                        ));
                    });
                    row.col(|ui| {
                        if let Some(id) = row_data.tracked
                            && ui
                                .button(icons::MAGNIFYING_GLASS)
                                .on_hover_text(t!("ui.player_tracker.find_matches"))
                                .clicked()
                        {
                            actions.find_matches_target = Some(id);
                        }
                    });
                });
            }
        });
}
