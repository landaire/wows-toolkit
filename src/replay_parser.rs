use std::{
    borrow::Cow,
    collections::HashMap,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use crate::{app::TimedMessage, icons, update_background_task, util::build_tomato_gg_url, wows_data::ShipIcon};
use chrono::{Local, NaiveDateTime, TimeZone};
use egui::{mutex::Mutex, text::LayoutJob, Color32, FontId, Image, ImageSource, Label, OpenUrl, RichText, Sense, Separator, TextFormat, Vec2};
use egui_extras::{Column, TableBuilder};

use parking_lot::RwLock;
use tap::Pipe;
use tracing::debug;

use wows_replays::{
    analyzer::{
        battle_controller::{BattleController, BattleReport, ChatChannel, GameMessage, Player},
        AnalyzerMut,
    },
    ReplayFile,
};

use itertools::Itertools;
use wowsunpack::{
    data::ResourceLoader,
    game_params::{provider::GameMetadataProvider, types::Species},
};

use crate::{
    app::{ReplayParserTabState, ToolkitTabViewer},
    error::ToolkitError,
    plaintext_viewer::{self, FileType},
    util::{self, build_ship_config_url, build_short_ship_config_url, build_wows_numbers_url, player_color_for_team_relation, separate_number},
};

const CHAT_VIEW_WIDTH: f32 = 500.0;
const XP_INDEX: usize = 389;
const DAMAGE_INDEX: usize = 412;

const DAMAGE_AP: usize = 147;
const DAMAGE_SAP: usize = 148;
const DAMAGE_HE: usize = 149;
const DAMAGE_SAP_SECONDARIES: usize = 151;
const DAMAGE_HE_SECONDARIES: usize = 152;
const DAMAGE_NORMAL_TORPS: usize = 153;
const DAMAGE_DEEP_WATER_TORPS: usize = 154;
const DAMAGE_FIRE: usize = 166;
const DAMAGE_FLOODS: usize = 167;

pub type SharedReplayParserTabState = Arc<Mutex<ReplayParserTabState>>;

pub struct Replay {
    pub replay_file: ReplayFile,

    pub resource_loader: Arc<GameMetadataProvider>,

    pub battle_report: Option<BattleReport>,

    pub divisions: HashMap<u32, char>,

    pub remaining_div_identifiers: String,
}

fn clan_color_for_player(player: &Player) -> Option<Color32> {
    if player.clan().is_empty() {
        None
    } else {

        let clan_color = player.raw_props_with_name().get("clanColor").expect("no clan color?");
        let clan_color = clan_color.as_i64().expect("clan color is not an i64");
        Some(Color32::from_rgb(((clan_color & 0xFF0000) >> 16) as u8, ((clan_color & 0xFF00)  >> 8) as u8, (clan_color & 0xFF) as u8))
    }
}

impl Replay {
    pub fn new(replay_file: ReplayFile, resource_loader: Arc<GameMetadataProvider>) -> Self {
        Replay {
            replay_file,
            resource_loader,
            battle_report: None,
            divisions: HashMap::new(),
            remaining_div_identifiers: "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().rev().collect(),
        }
    }
    pub fn parse(&self, expected_build: &str) -> Result<BattleReport, ToolkitError> {
        let version_parts: Vec<_> = self.replay_file.meta.clientVersionFromExe.split(',').collect();
        assert!(version_parts.len() == 4);
        if version_parts[3] != expected_build {
            return Err(ToolkitError::ReplayVersionMismatch {
                game_version: expected_build.to_string(),
                replay_version: version_parts[3].to_string(),
            });
        }

        // Parse packets
        let packet_data = &self.replay_file.packet_data;
        let mut controller = BattleController::new(&self.replay_file.meta, self.resource_loader.as_ref());
        let mut p = wows_replays::packet2::Parser::new(self.resource_loader.entity_specs());

        let report = match p.parse_packets_mut(packet_data, &mut controller) {
            Ok(()) => {
                controller.finish();
                controller.build_report()
            }
            Err(e) => {
                debug!("{:?}", e);
                controller.finish();
                controller.build_report()
            }
        };

        Ok(report)
    }
    pub fn assign_divs(&mut self) {
        if let Some(report) = self.battle_report.as_ref() {
            for vehicle in report.player_entities() {
                if let Some(player) = vehicle.player() {
                    let div = player.division_id();
                    if div > 0 {
                        self.divisions.entry(div).or_insert_with(|| self.remaining_div_identifiers.pop().unwrap_or('?'));
                    }
                }
            }
        }
    }
}

impl ToolkitTabViewer<'_> {
    fn ship_class_icon_from_species(&self, species: Species) -> Option<Arc<ShipIcon>> {
        self.tab_state
            .world_of_warships_data
            .as_ref()
            .and_then(|wows_data| wows_data.read().ship_icons.get(&species).cloned())
    }

    fn metadata_provider(&self) -> Option<Arc<GameMetadataProvider>> {
        self.tab_state
            .world_of_warships_data
            .as_ref()
            .and_then(|wows_data| wows_data.read().game_metadata.clone())
    }

    fn replays_dir(&self) -> Option<PathBuf> {
        self.tab_state.world_of_warships_data.as_ref().map(|wows_data| wows_data.read().replays_dir.clone())
    }

    fn build_replay_player_list(&self, replay_file: &Replay, report: &BattleReport, ui: &mut egui::Ui) {
        let match_timestamp = NaiveDateTime::parse_from_str(&replay_file.replay_file.meta.dateTime, "%d.%m.%Y %H:%M:%S").expect("parsing replay date failed");
        let match_timestamp = Local.from_local_datetime(&match_timestamp).single().expect("failed to convert to local time");
        let twitch_state = self.tab_state.twitch_state.read();

        let is_dark_mode = ui.visuals().dark_mode;
        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::auto().clip(true))
            .column(Column::initial(55.0).clip(true))
            .column(Column::initial(65.0).clip(true))
            .pipe(|table| {
                if self.tab_state.settings.replay_settings.show_entity_id {
                    table.column(Column::initial(100.0).clip(true))
                } else {
                    table
                }
            })
            .column(Column::initial(100.0).clip(true))
            .pipe(|table| {
                if self.tab_state.settings.replay_settings.show_observed_damage {
                    table.column(Column::initial(115.0).clip(true))
                } else {
                    table
                }
            })
            .column(Column::initial(110.0).clip(true))
            .column(Column::initial(115.0).clip(true))
            .column(Column::initial(115.0).clip(true))
            // Time lived
            .column(Column::initial(90.0).clip(true))
            .column(Column::initial(100.0).clip(true))
            .column(Column::remainder())
            .min_scrolled_height(0.0);

        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Player Name");
                });
                header.col(|ui| {
                    ui.strong("Base XP");
                });
                header.col(|ui| {
                    ui.strong(format!("Raw XP {}", icons::INFO)).on_hover_text("Raw XP before win modifiers are applied.");
                });
                if self.tab_state.settings.replay_settings.show_entity_id {
                    header.col(|ui| {
                        ui.strong("ID");
                    });
                }
                header.col(|ui| {
                    ui.strong("Ship Name");
                });
                if self.tab_state.settings.replay_settings.show_observed_damage {
                    header.col(|ui| {
                        ui.strong(format!("Observed Damage {}", icons::INFO)).on_hover_text(
                            "Observed damage reflects only damage you witnessed (i.e. victim was visible on your screen). This value may be lower than actual damage.",
                        );
                    });
                }
                header.col(|ui| {
                    ui.strong(format!("Actual Damage {}", icons::INFO)).on_hover_text(
                        "Actual damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
                    );
                });
                header.col(|ui| {
                    ui.strong(format!("Spotting Damage {}", icons::INFO)).on_hover_text(
                        "Spotting damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
                    );
                });
                header.col(|ui| {
                    ui.strong(format!("Potential Damage {}", icons::INFO)).on_hover_text(
                        "Potential damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
                    );
                });
                header.col(|ui| {
                    ui.strong("Time Lived");
                });
                header.col(|ui| {
                    ui.strong("Allocated Skills");
                });
                header.col(|ui| {
                    ui.strong("Actions");
                });
            })
            .body(|mut body| {
                let mut sorted_players = report.player_entities().to_vec();
                sorted_players.sort_unstable_by_key(|item| {
                    let player = item.player().unwrap();
                    (player.relation(), player.vehicle().species(), player.entity_id())
                });
                for entity in &sorted_players {
                    let player = entity.player().unwrap();
                    let mut player_color = player_color_for_team_relation(player.relation(), is_dark_mode);

                    if let Some(self_player) = sorted_players[0].player() {
                        if self_player.division_id() > 0 && player.division_id() == self_player.division_id() {
                            player_color = Color32::GOLD;
                        }
                    }
                    let ship = player.vehicle();

                    body.row(30.0, |mut ui| {
                        ui.col(|ui| {
                            let species: String = ship
                                .species()
                                .and_then(|species| {
                                    let species: &'static str = species.into();
                                    let id = format!("IDS_{}", species.to_uppercase());
                                    self.metadata_provider().and_then(|metadata| metadata.localized_name_from_id(&id))
                                })
                                .unwrap_or_else(|| "unk".to_string());
                            if let Some(icon) = self.ship_class_icon_from_species(ship.species().expect("ship has no species")) {
                                let mut color = match player.relation() {
                                    0 => Color32::GOLD,
                                    1 => Color32::LIGHT_GREEN,
                                    _ => Color32::LIGHT_RED,
                                };

                                if let Some(self_player) = sorted_players[0].player() {
                                    if player.team_id() == self_player.team_id() {
                                        color = Color32::GOLD;
                                    }
                                }

                                let image = Image::new(ImageSource::Bytes {
                                    uri: icon.path.clone().into(),
                                    // the icon size is <1k, this clone is fairly cheap
                                    bytes: icon.data.clone().into(),
                                })
                                .tint(color)
                                .fit_to_exact_size((20.0, 20.0).into())
                                .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));

                                ui.add(image).on_hover_text(species);
                            } else {
                                ui.label(species);
                            }

                            let name_color = if player.is_abuser() {
                                Color32::from_rgb(0xFF, 0xC0, 0xCB) // pink
                            } else {
                                player_color
                            };
                            if let Some(div) = replay_file.divisions.get(&player.division_id()).cloned() {
                                ui.label(format!("({})", div));
                            }
                            if !player.clan().is_empty() {
                                ui.label(RichText::new(format!("[{}]", player.clan())).color(clan_color_for_player(player).unwrap()));
                            }
                            ui.label(RichText::new(player.name()).color(name_color));
                            if player.is_hidden() {
                                ui.label(icons::EYE_SLASH).on_hover_text("Player has a hidden profile");
                            }

                            if let Some(timestamps) = twitch_state.player_is_potential_stream_sniper(player.name(), match_timestamp) {
                                let hover_text = timestamps.iter().map(|(name, timestamps)| {
                                    format!("Possible stream name: {}\nSeen: {} minutes after match start", name, timestamps.iter().map(|ts| {
                                        let delta = ts.signed_duration_since(match_timestamp);
                                        delta.num_minutes()
                                    }).join(", "))
                                }).join("\n\n");
                                ui.label(icons::TWITCH_LOGO).on_hover_text(hover_text);
                            }

                            let disconnect_hover_text = if player.did_disconnect() {
                                Some("Player disconnected from the match")
                            } else {
                                None
                            };

                            if let Some(disconnect_text) = disconnect_hover_text {
                                ui.label(icons::PLUGS).on_hover_text(disconnect_text);
                            }
                        });
                        ui.col(|ui| {
                            if let Some(base_xmp) = entity.results_info().and_then(|info| info.as_array().and_then(|info_array| info_array[XP_INDEX].as_number().and_then(|number| number.as_i64()))) {
                                let label_text =  separate_number(base_xmp, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref()));
                                ui.label(RichText::new(label_text).color(player_color));
                            } else {
                                ui.label("-");
                            }
                        });
                        ui.col(|ui| {
                            if let Some(raw_xp) = entity.results_info().and_then(|info| info.as_array().and_then(|info_array| info_array[XP_INDEX - 1].as_number().and_then(|number| number.as_i64()))) {
                                let raw_xp =  separate_number(raw_xp, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref()));
                                ui.label(raw_xp);
                            } else {
                                ui.label("-");
                            }
                        });

                        if self.tab_state.settings.replay_settings.show_entity_id {
                            ui.col(|ui| {
                                ui.label(format!("{}", entity.id()));
                            });
                        }

                        ui.col(|ui| {
                            let ship_name = self
                                .metadata_provider()
                                .and_then(|metadata| metadata.localized_name_from_param(ship).map(ToString::to_string))
                                .unwrap_or_else(|| format!("{}", ship.id()));
                            ui.label(ship_name);
                        });

                        if self.tab_state.settings.replay_settings.show_observed_damage {
                            ui.col(|ui| {
                                ui.label(separate_number(entity.damage(), self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())));
                            });
                        }

                        // Actual damage
                        ui.col(|ui| {
                            let got_valid_damages = if let Some(info_array) = entity.results_info().and_then(|info| info.as_array()) {

                                if let Some(damage_number) =  info_array[DAMAGE_INDEX].as_number().and_then(|number| number.as_i64()) {
                                    // Grab other damage numbers
                                    let damage_stats = [
                                        (DAMAGE_AP, "AP"),
                                        (DAMAGE_SAP, "SAP"),
                                        (DAMAGE_HE, "HE"),
                                        (DAMAGE_HE_SECONDARIES, "HE Sec"),
                                        (DAMAGE_SAP_SECONDARIES, "SAP Sec"),
                                        (DAMAGE_NORMAL_TORPS, "Torps"),
                                        (DAMAGE_DEEP_WATER_TORPS, "Deep Water Torps"),
                                        (DAMAGE_FIRE, "Fire"),
                                        (DAMAGE_FLOODS, "Flood")
                                    ];

                                    let breakdowns: Vec<String> = damage_stats.iter().filter_map(|(idx, description)| {
                                        info_array[*idx].as_number().and_then(|number| number.as_i64()).map(|num| {
                                            let num =  separate_number(num, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref()));
                                            format!("{:<16}: {}", description, num)
                                        })
                                    }).collect();

                                    let label_text = separate_number(damage_number, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref()));
                                    let label_text = RichText::new(label_text).color(player_color);
                                    // if let Some(damage_breakdowns) = breakdowns {
                                        ui.label(label_text).on_hover_text(RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0)));

                                    // } else {
                                    //     ui.label(label_text);
                                    // }
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if !got_valid_damages {
                                ui.label("-");
                            }
                        });
                        // Spotting damage
                        ui.col(|ui| {
                            const DAMAGE_INDEX: usize = 398;
                            if let Some(damage_number) = entity.results_info().and_then(|info| info.as_array().and_then(|info_array| info_array[DAMAGE_INDEX].as_number().and_then(|number| number.as_i64()))) {
                                ui.label(separate_number(damage_number, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())));
                            } else {
                                ui.label("-");
                            }
                        });
                        // Potential damage
                        ui.col(|ui| {
                            const ARTILLERY_POTENTIAL_DAMAGE: usize = 402;
                            const _TORPEDO_POTENTIAL_DAMAGE: usize = 403; // may not be accurate?
                            const AIRSTRIKE_POTENTIAL_DAMAGE: usize = 404;

                            if let Some(damage_numbers) = entity.results_info().and_then(|info| info.as_array().map(|info_array| &info_array[ARTILLERY_POTENTIAL_DAMAGE..=AIRSTRIKE_POTENTIAL_DAMAGE])) {
                                let total_pot = damage_numbers.iter().map(|num| num.as_f64()).fold(0, |accum, num| accum + num.map(|f| f as u64).unwrap_or_default());
                                let hover_string = format!("Artillery: {}\nTorpedo: {}\nPlanes: {}",
                                    separate_number(damage_numbers[0].as_f64().unwrap_or_default() as u64, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())),
                                    separate_number(damage_numbers[1].as_f64().unwrap_or_default() as u64, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())),
                                    separate_number(damage_numbers[2].as_f64().unwrap_or_default() as u64, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())),
                                );
                                
                                ui.label(separate_number(total_pot, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref()))).on_hover_text(hover_string);
                            } else {
                                ui.label("-");
                            }
                        });

                        ui.col(|ui| {
                            if let Some(death_info) = entity.death_info() {
                                let secs = death_info.time_lived().as_secs();
                                ui.label(format!("{}:{:02}", secs / 60, secs % 60));
                            } else {
                                ui.label("-");
                            }
                        });

                        let species = ship.species().expect("ship has no species?");
                        let (skill_points, num_skills, highest_tier, num_tier_1_skills) = entity
                            .commander_skills()
                            .map(|skills| {
                                let points = skills.iter().fold(0usize, |accum, skill| accum + skill.tier().get_for_species(species.clone()));
                                let highest_tier = skills.iter().map(|skill| skill.tier().get_for_species(species.clone())).max();
                                let num_tier_1_skills = skills.iter().fold(0, |mut accum, skill| {
                                    if skill.tier().get_for_species(species.clone()) == 1 {
                                        accum += 1;
                                    }
                                    accum
                                });

                                (points, skills.len(), highest_tier.unwrap_or(0), num_tier_1_skills)
                            })
                            .unwrap_or((0, 0, 0, 0));
                        ui.col(|ui| {
                            let (label, hover_text) = util::colorize_captain_points(skill_points, num_skills, highest_tier, num_tier_1_skills);
                            ui.label(label).pipe(|label| {
                                if let Some(hover_text) = hover_text {
                                    label.on_hover_text(hover_text)
                                } else {
                                    label
                                }
                            });
                        });
                        ui.col(|ui| {
                            ui.menu_button(icons::DOTS_THREE, |ui| {
                                if ui.small_button(format!("{} Open Build in Browser", icons::SHARE)).clicked() {
                                    let metadata_provider = self.metadata_provider().unwrap();

                                    let url = build_ship_config_url(entity, &metadata_provider);

                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Copy Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.metadata_provider().unwrap();

                                    let url = build_ship_config_url(entity, &metadata_provider);
                                    ui.output_mut(|output| output.copied_text = url);
                                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Build link copied", icons::CHECK_CIRCLE)));

                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Copy Short Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.metadata_provider().unwrap();

                                    let url = build_short_ship_config_url(entity, &metadata_provider);
                                    ui.output_mut(|output| output.copied_text = url);
                                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Build link copied", icons::CHECK_CIRCLE)));

                                    ui.close_menu();
                                }

                                ui.separator();

                                if ui.small_button(format!("{} Open Tomato.gg Page", icons::SHARE)).clicked() {
                                    if let Some(url) = build_tomato_gg_url(entity) {
                                        ui.ctx().open_url(OpenUrl::new_tab(url));
                                    }

                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Open WoWs Numbers Page", icons::SHARE)).clicked() {
                                    if let Some(url) = build_wows_numbers_url(entity) {
                                        ui.ctx().open_url(OpenUrl::new_tab(url));
                                    }

                                    ui.close_menu();
                                }

                                ui.separator();

                                if ui.small_button(format!("{} View Raw Player Metadata", icons::BUG)).clicked() {
                                    let pretty_meta = serde_json::to_string_pretty(player).expect("failed to serialize player");
                                    let viewer = plaintext_viewer::PlaintextFileViewer {
                                        title: Arc::new("metadata.json".to_owned()),
                                        file_info: Arc::new(egui::mutex::Mutex::new(FileType::PlainTextFile {
                                            ext: ".json".to_owned(),
                                            contents: pretty_meta,
                                        })),
                                        open: Arc::new(AtomicBool::new(true)),
                                    };

                                    self.tab_state.file_viewer.lock().push(viewer);

                                    ui.close_menu();
                                }

                            });
                        });
                    });
                }
            });
    }

    fn build_replay_chat(&self, battle_report: &BattleReport, ui: &mut egui::Ui) {
        for message in battle_report.game_chat() {
            let GameMessage {
                sender_relation,
                sender_name,
                channel,
                message,
            } = message;

            let translated_text = if sender_relation.is_none() {
                self.metadata_provider().and_then(|provider| {
                    let name = provider.localized_name_from_id(message);

                    name
                })
            } else {
                None
            };

            let text = format!("{sender_name} ({channel:?}): {}", translated_text.as_ref().unwrap_or(message));

            let is_dark_mode = ui.visuals().dark_mode;
            let name_color = if let Some(relation) = sender_relation {
                player_color_for_team_relation(*relation, is_dark_mode)
            } else {
                Color32::GRAY
            };

            let mut job = LayoutJob::default();
            job.append(
                &format!("{sender_name}:\n"),
                0.0,
                TextFormat {
                    color: name_color,
                    ..Default::default()
                },
            );

            let text_color = match channel {
                ChatChannel::Division => Color32::GOLD,
                ChatChannel::Global => {
                    if is_dark_mode {
                        Color32::WHITE
                    } else {
                        Color32::BLACK
                    }
                }
                ChatChannel::Team => {
                    if is_dark_mode {
                        Color32::LIGHT_GREEN
                    } else {
                        Color32::DARK_GREEN
                    }
                }
            };

            job.append(
                translated_text.as_ref().unwrap_or(message),
                0.0,
                TextFormat {
                    color: text_color,
                    ..Default::default()
                },
            );

            if ui.add(Label::new(job).sense(Sense::click())).on_hover_text("Click to copy").clicked() {
                ui.output_mut(|output| output.copied_text = text);
                *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Message copied", icons::CHECK_CIRCLE)));
            }
            ui.add(Separator::default());
            ui.end_row();
        }
    }

    fn build_replay_view(&self, replay_file: &Replay, ui: &mut egui::Ui) {
        if let Some(report) = replay_file.battle_report.as_ref() {
            let self_entity = report.self_entity();
            let self_player = self_entity.player().unwrap();
            ui.horizontal(|ui| {
                if !self_player.clan().is_empty() {
                    ui.label(format!("[{}]", self_player.clan()));
                }
                ui.label(self_player.name());
                ui.label(report.game_type());
                ui.label(report.version().to_path());
                ui.label(report.game_mode());
                ui.label(report.map_name());
                if report.battle_results().is_some() {
                    let mut team_damage = 0;
                    let mut red_team_damage = 0;
                    for vehicle in report.player_entities() {
                        if let Some(player) = vehicle.player() {
                            if let Some(player_damage) = vehicle
                                .results_info()
                                .expect("no player info")
                                .as_array()
                                .and_then(|values| values[DAMAGE_INDEX].as_i64())
                            {
                                if player.relation() > 1 {
                                    red_team_damage += player_damage;
                                } else {
                                    team_damage += player_damage;
                                }
                            }
                        }
                    }

                    let mut job = LayoutJob::default();
                    job.append("Damage Dealt: ", 0.0, Default::default());
                    job.append(
                        &separate_number(team_damage, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())),
                        0.0,
                        TextFormat {
                            color: Color32::LIGHT_GREEN,
                            ..Default::default()
                        },
                    );
                    job.append(" : ", 0.0, Default::default());
                    job.append(
                        &separate_number(red_team_damage, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())),
                        0.0,
                        TextFormat {
                            color: Color32::LIGHT_RED,
                            ..Default::default()
                        },
                    );

                    job.append(
                        &format!(
                            " ({})",
                            separate_number(team_damage + red_team_damage, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref()))
                        ),
                        0.0,
                        Default::default(),
                    );

                    ui.label(job);
                }
                ui.menu_button("Export Chat", |ui| {
                    if ui.small_button(format!("{} Save To File", icons::FLOPPY_DISK)).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("{} {} {} - Game Chat.txt", report.game_type(), report.game_mode(), report.map_name()))
                            .save_file()
                        {
                            if let Ok(mut file) = std::fs::File::create(path) {
                                for message in report.game_chat() {
                                    let GameMessage {
                                        sender_relation: _,
                                        sender_name,
                                        channel,
                                        message,
                                    } = message;
                                    let _ = writeln!(file, "{} ({:?}): {}", sender_name, channel, message);
                                }
                            }
                        }

                        ui.close_menu();
                    }

                    if ui.small_button(format!("{} Copy", icons::COPY)).clicked() {
                        let mut buf = BufWriter::new(Vec::new());
                        for message in report.game_chat() {
                            let GameMessage {
                                sender_relation: _,
                                sender_name,
                                channel,
                                message,
                            } = message;
                            let _ = writeln!(&mut buf, "{} ({:?}): {}", sender_name, channel, message);
                        }

                        let game_chat = String::from_utf8(buf.into_inner().expect("failed to get buf inner")).expect("failed to convert game chat buffer to string");

                        ui.output_mut(|output| output.copied_text = game_chat);

                        ui.close_menu();
                    }
                });
                if ui.button("Raw Metadata").clicked() {
                    let parsed_meta: serde_json::Value = serde_json::from_str(&replay_file.replay_file.raw_meta).expect("failed to parse replay metadata");
                    let pretty_meta = serde_json::to_string_pretty(&parsed_meta).expect("failed to serialize replay metadata");
                    let viewer = plaintext_viewer::PlaintextFileViewer {
                        title: Arc::new("metadata.json".to_owned()),
                        file_info: Arc::new(egui::mutex::Mutex::new(FileType::PlainTextFile {
                            ext: ".json".to_owned(),
                            contents: pretty_meta,
                        })),
                        open: Arc::new(AtomicBool::new(true)),
                    };

                    self.tab_state.file_viewer.lock().push(viewer);
                }
                let results_button = egui::Button::new("Results Raw JSON");
                if ui
                    .add_enabled(report.battle_results().is_some(), results_button)
                    .on_hover_text("This is the disgustingly terribly-formatted raw battle results which is serialized by WG, not by this tool.")
                    .clicked()
                {
                    if let Some(results_json) = report.battle_results() {
                        let parsed_results: serde_json::Value = serde_json::from_str(results_json).expect("failed to parse replay metadata");
                        let pretty_meta = serde_json::to_string_pretty(&parsed_results).expect("failed to serialize replay metadata");
                        let viewer = plaintext_viewer::PlaintextFileViewer {
                            title: Arc::new("results.json".to_owned()),
                            file_info: Arc::new(egui::mutex::Mutex::new(FileType::PlainTextFile {
                                ext: ".json".to_owned(),
                                contents: pretty_meta,
                            })),
                            open: Arc::new(AtomicBool::new(true)),
                        };

                        self.tab_state.file_viewer.lock().push(viewer);
                    }
                }
            });

            if self.tab_state.settings.replay_settings.show_game_chat {
                egui::SidePanel::left("replay_view_chat")
                    .default_width(CHAT_VIEW_WIDTH)
                    .max_width(CHAT_VIEW_WIDTH)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::both().id_salt("replay_chat_scroll_area").show(ui, |ui| {
                            self.build_replay_chat(report, ui);
                        });
                    });
            }

            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::horizontal().id_salt("replay_player_list_scroll_area").show(ui, |ui| {
                    self.build_replay_player_list(replay_file, report, ui);
                });
            });
        }
    }

    fn build_file_listing(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            egui::Grid::new("replay_files_grid").num_columns(1).striped(true).show(ui, |ui| {
                if let Some(mut files) = self
                    .tab_state
                    .replay_files
                    .as_ref()
                    .map(|files| files.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>())
                {
                    // Sort by filename -- WoWs puts the date first in a sortable format
                    files.sort_by(|a, b| b.0.cmp(&a.0));
                    let metadata_provider = self.metadata_provider().unwrap();
                    for (path, replay) in files {
                        let label = {
                            let file = replay.read();
                            let meta = &file.replay_file.meta;
                            let player_vehicle = meta.vehicles.iter().find(|vehicle| vehicle.relation == 0);
                            let vehicle_name = player_vehicle
                                .and_then(|vehicle| metadata_provider.param_localization_id(vehicle.shipId as u32))
                                .and_then(|id| metadata_provider.localized_name_from_id(id))
                                .unwrap_or_else(|| "Spectator".to_string());
                            let map_id = format!("IDS_{}", meta.mapName.to_uppercase());
                            let map_name = metadata_provider.localized_name_from_id(&map_id).unwrap_or_else(|| meta.mapName.clone());

                            let mode = metadata_provider
                                .localized_name_from_id(&format!("IDS_{}", meta.gameType.to_ascii_uppercase()))
                                .expect("failed to get game type translation");

                            let scenario = metadata_provider
                                .localized_name_from_id(&format!("IDS_SCENARIO_{}", meta.scenario.to_ascii_uppercase()))
                                .expect("failed to get scenario translation");

                            let time = meta.dateTime.as_str();

                            [vehicle_name.as_str(), map_name.as_str(), scenario.as_str(), mode.as_str(), time].iter().join(" - ")
                        };

                        let mut label_text = egui::RichText::new(label.as_str());
                        if let Some(current_replay) = self.tab_state.current_replay.as_ref() {
                            if Arc::ptr_eq(current_replay, &replay) {
                                label_text = label_text.background_color(Color32::DARK_GRAY).color(Color32::WHITE);
                            }
                        }

                        let label = ui.add(Label::new(label_text).selectable(false).sense(Sense::click())).on_hover_text(label.as_str());
                        label.context_menu(|ui| {
                            if ui.button("Copy Path").clicked() {
                                ui.output_mut(|output| output.copied_text = path.to_string_lossy().into_owned());
                                ui.close_menu();
                            }
                            if ui.button("Show in File Explorer").clicked() {
                                util::open_file_explorer(&path);
                                ui.close_menu();
                            }
                        });

                        if label.double_clicked() {
                            if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
                                update_background_task!(self.tab_state.background_task, wows_data.read().load_replay(replay.clone()));
                            }
                        }
                        ui.end_row();
                    }
                }
            });
        });
    }

    pub fn clear_chat(&mut self, _replay: Arc<RwLock<Replay>>) {
        self.tab_state.replay_parser_tab.lock().game_chat.clear();
    }

    /// Builds the replay parser tab
    pub fn build_replay_parser_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button(format!("{} Manually Open Replay File...", icons::FOLDER_OPEN)).clicked() {
                    if let Some(file) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file() {
                        self.tab_state.settings.current_replay_path = file;

                        if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
                            update_background_task!(
                                self.tab_state.background_task,
                                wows_data.read().parse_replay(self.tab_state.settings.current_replay_path.clone())
                            );
                        }
                    }
                }

                // Only show the live game button if the replays dir exists
                if let Some(_replays_dir) = self.replays_dir() {
                    ui.checkbox(&mut self.tab_state.auto_load_latest_replay, "Autoload Latest Replay");
                }
            });

            egui::SidePanel::left("replay_listing_panel").show_inside(ui, |ui| {
                egui::ScrollArea::both().id_source("replay_chat_scroll_area").show(ui, |ui| {
                    self.build_file_listing(ui);
                });
            });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                if let Some(replay_file) = self.tab_state.current_replay.as_ref() {
                    let replay_file = replay_file.read();
                    self.build_replay_view(&replay_file, ui);
                } else {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        ui.heading("Double click or load a replay to view data");
                    });
                }
            });
        });
    }
}
