use std::{
    borrow::Cow,
    path::Path,
    sync::{atomic::AtomicBool, mpsc, Arc},
};

use crate::icons;
use egui::{
    mutex::{Mutex, RwLock},
    text::LayoutJob,
    Color32, Image, ImageSource, Label, OpenUrl, RichText, Sense, TextFormat, Vec2,
};
use egui_extras::{Column, TableBuilder};

use tap::Pipe;

use wows_replays::{
    analyzer::{
        battle_controller::{BattleController, BattleReport, ChatChannel, GameMessage, Player},
        AnalyzerMut,
    },
    resource_loader::ResourceLoader,
    ReplayFile,
};

use itertools::Itertools;

use crate::{
    app::{ReplayParserTabState, ToolkitTabViewer},
    error::ToolkitError,
    game_params::GameMetadataProvider,
    plaintext_viewer::{self, FileType},
    task::{BackgroundTask, BackgroundTaskCompletion, BackgroundTaskKind},
    util::{self, build_ship_config_url, build_short_ship_config_url, build_wows_numbers_url, player_color_for_team_relation, separate_number},
};

const CHAT_VIEW_WIDTH: f32 = 200.0;

pub type SharedReplayParserTabState = Arc<Mutex<ReplayParserTabState>>;

pub struct Replay {
    pub replay_file: ReplayFile,

    pub resource_loader: Arc<GameMetadataProvider>,

    pub battle_report: Option<BattleReport>,
}

fn player_name_with_clan(player: &Player) -> Cow<'_, str> {
    if player.clan().is_empty() {
        Cow::Borrowed(player.name())
    } else {
        Cow::Owned(format!("[{}] {}", player.clan(), player.name()))
    }
}

impl Replay {
    pub fn new(replay_file: ReplayFile, resource_loader: Arc<GameMetadataProvider>) -> Self {
        Replay {
            replay_file,
            resource_loader,
            battle_report: None,
        }
    }
    pub fn parse(&self, expected_build: &str) -> Result<BattleReport, ToolkitError> {
        let version_parts: Vec<_> = self.replay_file.meta.clientVersionFromExe.split(',').collect();
        assert!(version_parts.len() == 4);
        if version_parts[3] != expected_build {
            return Err(ToolkitError::ReplayVersionMismatch {
                expected: expected_build.to_string(),
                actual: version_parts[3].to_string(),
            });
        }

        // Parse packets
        let packet_data = &self.replay_file.packet_data;
        let mut controller = BattleController::new(&self.replay_file.meta, self.resource_loader.as_ref());
        let mut p = wows_replays::packet2::Parser::new(self.resource_loader.entity_specs());

        match p.parse_packets_mut(packet_data, &mut controller) {
            Ok(()) => {
                controller.finish();
                Ok(controller.build_report())
            }
            Err(e) => {
                eprintln!("{:?}", e);
                controller.finish();
                Ok(controller.build_report())
            }
        }
    }
}

impl ToolkitTabViewer<'_> {
    fn build_replay_player_list(&self, report: &BattleReport, ui: &mut egui::Ui) {
        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::auto().clip(true))
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
                    table.column(Column::initial(100.0).clip(true))
                } else {
                    table
                }
            })
            .column(Column::initial(100.0).clip(true))
            .column(Column::initial(100.0).clip(true))
            .column(Column::initial(100.0).clip(true))
            .column(Column::initial(100.0).clip(true))
            .column(Column::remainder())
            .min_scrolled_height(0.0);

        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Player Name");
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
                        ui.strong("Observed Damage").on_hover_text(
                            "Observed damage reflects only damage you witnessed (i.e. victim was visible on your screen). This value may be lower than actual damage.",
                        );
                    });
                }
                header.col(|ui| {
                    ui.strong("Time Lived");
                });
                header.col(|ui| {
                    ui.strong("Max HP");
                });
                header.col(|ui| {
                    ui.strong("Allocated Skills");
                });
                header.col(|ui| {
                    ui.strong("Modules");
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
                    let ship = player.vehicle();

                    body.row(30.0, |mut ui| {
                        ui.col(|ui| {
                            let species: String = ship
                                .species()
                                .and_then(|species| {
                                    let species: &'static str = species.into();
                                    let id = format!("IDS_{}", species.to_uppercase());
                                    self.tab_state.world_of_warships_data.game_metadata.as_ref().unwrap().localized_name_from_id(&id)
                                })
                                .unwrap_or_else(|| "unk".to_string());
                            if let Some(icons) = self.tab_state.world_of_warships_data.ship_icons.as_ref() {
                                let icon = icons.get(&ship.species().expect("ship has no species")).expect("failed to get ship icon for species");

                                let color = match player.relation() {
                                    0 => Color32::GOLD,
                                    1 => Color32::LIGHT_GREEN,
                                    _ => Color32::LIGHT_RED,
                                };

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

                            let is_dark_mode = ui.visuals().dark_mode;
                            let name_color = player_color_for_team_relation(player.relation(), is_dark_mode);
                            ui.label(RichText::new(player_name_with_clan(player)).color(name_color));
                            if player.is_hidden() {
                                ui.label(icons::EYE_SLASH).on_hover_text("Player has a hidden profile");
                            }
                            if player.is_abuser() {
                                ui.label(icons::RADIOACTIVE).on_hover_text("Player is marked as an \"abuser\"");
                            }
                        });

                        if self.tab_state.settings.replay_settings.show_entity_id {
                            ui.col(|ui| {
                                ui.label(format!("{}", entity.id()));
                            });
                        }

                        ui.col(|ui| {
                            let ship_name = self
                                .tab_state
                                .world_of_warships_data
                                .game_metadata
                                .as_ref()
                                .unwrap()
                                .localized_name_from_param(ship)
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| format!("{}", ship.id()));
                            ui.label(ship_name);
                        });

                        if self.tab_state.settings.replay_settings.show_observed_damage {
                            ui.col(|ui| {
                                ui.label(separate_number(entity.damage(), self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())));
                            });
                        }

                        ui.col(|ui| {
                            if let Some(death_info) = entity.death_info() {
                                let secs = death_info.time_lived().as_secs();
                                ui.label(format!("{}:{:02}", secs / 60, secs % 60));
                            } else {
                                ui.label("-");
                            }
                        });

                        ui.col(|ui| {
                            ui.label(separate_number(player.max_health(), self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())));
                        });

                        let species = ship.species().expect("ship has no species?");
                        let (skill_points, num_skills, highest_tier) = entity
                            .commander_skills()
                            .map(|skills| {
                                let points = skills.iter().fold(0usize, |accum, skill| accum + skill.tier().get_for_species(species.clone()));
                                let highest_tier = skills.iter().map(|skill| skill.tier().get_for_species(species.clone())).max();

                                (points, skills.len(), highest_tier.unwrap_or(0))
                            })
                            .unwrap_or((0, 0, 0));
                        ui.col(|ui| {
                            ui.label(util::colorize_captain_points(skill_points, num_skills, highest_tier));
                        });
                        ui.col(|ui| {
                            ui.label(entity.props().ship_config().modernization().len().to_string());
                        });
                        ui.col(|ui| {
                            ui.menu_button(icons::DOTS_THREE, |ui| {
                                if ui.small_button(format!("{} Open Build in Browser", icons::SHARE)).clicked() {
                                    let metadata_provider = self.tab_state.world_of_warships_data.game_metadata.as_ref().unwrap();

                                    let url = build_ship_config_url(entity, metadata_provider);

                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Copy Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.tab_state.world_of_warships_data.game_metadata.as_ref().unwrap();

                                    let url = build_ship_config_url(entity, metadata_provider);
                                    ui.output_mut(|output| output.copied_text = url);

                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Copy Short Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.tab_state.world_of_warships_data.game_metadata.as_ref().unwrap();

                                    let url = build_short_ship_config_url(entity, metadata_provider);
                                    ui.output_mut(|output| output.copied_text = url);

                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Open WoWs Numbers Page", icons::SHARE)).clicked() {
                                    if let Some(url) = build_wows_numbers_url(entity) {
                                        ui.ctx().open_url(OpenUrl::new_tab(url));
                                    }

                                    ui.close_menu();
                                }
                            });
                        });
                    });
                }
            });
    }

    fn build_replay_chat(&self, battle_report: &BattleReport, ui: &mut egui::Ui) {
        egui::Grid::new("filtered_files_grid")
            .max_col_width(CHAT_VIEW_WIDTH)
            .num_columns(1)
            .striped(true)
            .show(ui, |ui| {
                for message in battle_report.game_chat() {
                    let GameMessage {
                        sender_relation,
                        sender_name,
                        channel,
                        message,
                    } = message;

                    let text = format!("{sender_name} ({channel:?}): {message}");

                    let is_dark_mode = ui.visuals().dark_mode;
                    let name_color = player_color_for_team_relation(*sender_relation, is_dark_mode);

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
                        message,
                        0.0,
                        TextFormat {
                            color: text_color,
                            ..Default::default()
                        },
                    );

                    if ui.add(Label::new(job).sense(Sense::click())).on_hover_text("Click to copy").clicked() {
                        ui.output_mut(|output| output.copied_text = text);
                    }
                    ui.end_row();
                }
            });
    }

    fn build_replay_view(&self, replay_file: &Replay, ui: &mut egui::Ui) {
        if let Some(report) = replay_file.battle_report.as_ref() {
            let self_entity = report.self_entity();
            let self_player = self_entity.player().unwrap();
            ui.horizontal(|ui| {
                ui.label(player_name_with_clan(self_player));
                ui.label(report.game_type());
                ui.label(report.version().to_path());
                ui.label(report.game_mode());
                ui.label(report.map_name());
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
            });

            if self.tab_state.settings.replay_settings.show_game_chat {
                egui::SidePanel::left("replay_view_chat").default_width(CHAT_VIEW_WIDTH).show_inside(ui, |ui| {
                    egui::ScrollArea::both().id_source("replay_chat_scroll_area").show(ui, |ui| {
                        self.build_replay_chat(report, ui);
                    });
                });
            }

            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::horizontal().id_source("replay_player_list_scroll_area").show(ui, |ui| {
                    self.build_replay_player_list(report, ui);
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
                    let resource_provider = self.tab_state.world_of_warships_data.game_metadata.clone().unwrap();
                    for (path, replay) in files {
                        let label = {
                            let file = replay.read();
                            let meta = &file.replay_file.meta;
                            let player_vehicle = meta.vehicles.iter().find(|vehicle| vehicle.relation == 0);
                            let vehicle_name = player_vehicle
                                .and_then(|vehicle| resource_provider.param_localization_id(vehicle.shipId as u32))
                                .and_then(|id| resource_provider.localized_name_from_id(id))
                                .unwrap_or_else(|| "Spectator".to_string());
                            let map_id = format!("IDS_{}", meta.mapName.to_uppercase());
                            let map_name = resource_provider.localized_name_from_id(&map_id).unwrap_or_else(|| meta.mapName.clone());

                            let mode = resource_provider
                                .localized_name_from_id(&format!("IDS_{}", meta.gameType.to_ascii_uppercase()))
                                .expect("failed to get game type translation");

                            let scenario = resource_provider
                                .localized_name_from_id(&format!("IDS_SCENARIO_{}", meta.scenario.to_ascii_uppercase()))
                                .expect("failed to get scenario translation");

                            let time = meta.dateTime.as_str();

                            [vehicle_name.as_str(), map_name.as_str(), scenario.as_str(), mode.as_str(), time].iter().join(" - ")
                        };

                        if ui
                            .add(Label::new(label.as_str()).sense(Sense::click()))
                            .on_hover_text(label.as_str())
                            .context_menu(|ui| {
                                if ui.button("Copy Path").clicked() {
                                    ui.output_mut(|output| output.copied_text = path.to_string_lossy().into_owned());
                                    ui.close_menu();
                                }
                                if ui.button("Show in File Explorer").clicked() {
                                    util::open_file_explorer(&path);
                                    ui.close_menu();
                                }
                            })
                            .double_clicked()
                        {
                            self.load_replay(replay.clone());
                        }
                        ui.end_row();
                    }
                }
            });
        });
    }

    fn parse_live_replay(&mut self) {
        if let Some(replays_dir) = self.tab_state.replays_dir.as_ref() {
            let meta = replays_dir.join("tempArenaInfo.json");
            let replay = replays_dir.join("temp.wowsreplay");

            let meta_data = std::fs::read(meta);
            let replay_data = std::fs::read(replay);

            if meta_data.is_err() || replay_data.is_err() {
                return;
            }

            let replay_file: ReplayFile = ReplayFile::from_decrypted_parts(meta_data.unwrap(), replay_data.unwrap()).unwrap();
            let game_metadata = self.tab_state.world_of_warships_data.game_metadata.clone().unwrap();
            let replay = Replay::new(replay_file, game_metadata);

            self.load_replay(Arc::new(RwLock::new(replay)));
        }
    }

    fn parse_replay<P: AsRef<Path>>(&mut self, replay_path: P) {
        if self.tab_state.world_of_warships_data.is_ready() {
            let path = replay_path.as_ref();

            let replay_file: ReplayFile = ReplayFile::from_file(path).unwrap();
            let game_metadata = self.tab_state.world_of_warships_data.game_metadata.clone().unwrap();
            let replay = Replay::new(replay_file, game_metadata);

            self.load_replay(Arc::new(RwLock::new(replay)));
        }
    }

    fn load_replay(&mut self, replay: Arc<RwLock<Replay>>) {
        let game_version = self.tab_state.world_of_warships_data.game_version.unwrap();
        {
            // Reset the chat state
            self.tab_state.replay_parser_tab.lock().game_chat.clear();
        }

        let (tx, rx) = mpsc::channel();

        let _join_handle = std::thread::spawn(move || {
            let res = { replay.read().parse(game_version.to_string().as_str()) };
            let res = res.map(move |report| {
                replay.write().battle_report = Some(report);
                BackgroundTaskCompletion::ReplayLoaded { replay }
            });

            let _ = tx.send(res);
        });

        self.tab_state.background_task = Some(BackgroundTask {
            receiver: rx,
            kind: BackgroundTaskKind::LoadingReplay,
        });
    }

    /// Builds the replay parser tab
    pub fn build_replay_parser_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.tab_state.settings.current_replay_path.to_string_lossy().into_owned()).hint_text("Current Replay File"));

                if ui.button("Parse").clicked() {
                    self.parse_replay(self.tab_state.settings.current_replay_path.clone());
                }

                if ui.button(format!("{} Browse...", icons::FOLDER_OPEN)).clicked() {
                    if let Some(file) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file() {
                        //println!("{:#?}", ReplayFile::from_file(&file));

                        self.tab_state.settings.current_replay_path = file;
                    }
                }

                if let Some(_replays_dir) = self.tab_state.replays_dir.as_ref() {
                    if ui.button(format!("{} Load Live Game", icons::DETECTIVE)).clicked() {
                        self.parse_live_replay();
                    }
                }
            });

            egui::SidePanel::left("replay_listing_panel").show_inside(ui, |ui| {
                egui::ScrollArea::both().id_source("replay_chat_scroll_area").show(ui, |ui| {
                    self.build_file_listing(ui);
                });
            });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                if let Some(replay_file) = self.tab_state.world_of_warships_data.current_replay.as_ref() {
                    let replay_file = replay_file.read();
                    self.build_replay_view(&replay_file, ui);
                }
            });
        });
    }
}
