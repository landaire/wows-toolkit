use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::OsStr,
    io::{self, Cursor, Write},
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
    sync::{mpsc, Arc, Mutex},
};

use bounded_vec_deque::BoundedVecDeque;
use byteorder::{LittleEndian, ReadBytesExt};
use egui::{epaint::util, text::LayoutJob, Color32, Grid, Image, ImageSource, Label, OpenUrl, RichText, Sense, Separator, TextFormat, Vec2};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use env_logger::fmt::Color;
use flate2::{
    write::{DeflateEncoder, ZlibEncoder},
    Compression,
};
use language_tags::LanguageTag;
use notify::{EventKind, RecursiveMode, Watcher};
use ouroboros::self_referencing;
use serde::{de::IntoDeserializer, Deserialize, Serialize};
use serde_json::json;
use tap::{Pipe, Tap};
use thousands::Separable;
use wows_replays::{
    analyzer::{
        battle_controller::{self, BattleController, BattleReport, ChatChannel, EventHandler, GameMessage, Player},
        AnalyzerBuilder, AnalyzerMut,
    },
    packet2::{Packet, PacketType, PacketTypeKind},
    parse_scripts,
    resource_loader::ResourceLoader,
    rpc::typedefs::ArgValue,
    ReplayFile, ReplayMeta,
};

use itertools::Itertools;
use wowsunpack::{idx::FileNode, pkg::PkgFileLoader};

use crate::{
    app::{ReplayParserTabState, ToolkitTabViewer},
    game_params::GameMetadataProvider,
    util::{build_ship_config_url, player_color_for_team_relation, separate_number},
};

const CHAT_VIEW_WIDTH: f32 = 200.0;

pub type SharedReplayParserTabState = Arc<Mutex<ReplayParserTabState>>;

pub struct Replay {
    replay_file: ReplayFile,

    resource_loader: Rc<GameMetadataProvider>,

    battle_report: Option<BattleReport>,
}

fn player_name_with_clan(player: &Player) -> Cow<'_, str> {
    if player.clan().is_empty() {
        Cow::Borrowed(player.name())
    } else {
        Cow::Owned(format!("[{}] {}", player.clan(), player.name()))
    }
}

impl Replay {
    pub fn parse(&mut self, file_tree: &FileNode, pkg_loader: Arc<PkgFileLoader>) {
        let version_parts: Vec<_> = self.replay_file.meta.clientVersionFromExe.split(",").collect();
        assert!(version_parts.len() == 4);

        // Parse packets
        let packet_data = &self.replay_file.packet_data;
        let mut controller = BattleController::new(&self.replay_file.meta, self.resource_loader.as_ref());
        let mut p = wows_replays::packet2::Parser::new(&self.resource_loader.entity_specs());

        match p.parse_packets_mut(packet_data, &mut controller) {
            Ok(()) => {
                controller.finish();
                self.battle_report = Some(controller.build_report());
            }
            Err(e) => panic!("{:?}", e),
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
            .column(Column::initial(100.0).clip(true))
            .pipe(|table| {
                if self.tab_state.settings.replay_settings.show_observed_damage {
                    table.column(Column::initial(100.0).clip(true))
                } else {
                    table
                }
            })
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
                header.col(|ui| {
                    ui.strong("Ship Class");
                });
                if self.tab_state.settings.replay_settings.show_observed_damage {
                    header.col(|ui| {
                        ui.strong("Observed Damage").on_hover_text(
                            "Observed damage reflects only damage you witnessed (i.e. victim was visible on your screen). This value may be lower than actual damage.",
                        );
                    });
                }
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
                    let ship = player.vehicle();
                    body.row(30.0, |mut ui| {
                        ui.col(|ui| {
                            let is_dark_mode = ui.visuals().dark_mode;
                            let name_color = player_color_for_team_relation(player.relation(), is_dark_mode);
                            ui.label(RichText::new(player_name_with_clan(&*player)).color(name_color));
                        });

                        if self.tab_state.settings.replay_settings.show_entity_id {
                            ui.col(|ui| {
                                ui.label(format!("{}", player.avatar_id()));
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
                                let (path, icon_data) = icons.get(&ship.species().expect("ship has no species")).expect("failed to get ship icon for species");

                                let color = match player.relation() {
                                    0 => Color32::GOLD,
                                    1 => Color32::LIGHT_GREEN,
                                    _ => Color32::LIGHT_RED,
                                };

                                let image = Image::new(ImageSource::Bytes {
                                    uri: path.clone().into(),
                                    // the icon size is <1k, this clone is fairly cheap
                                    bytes: icon_data.clone().into(),
                                })
                                .tint(color)
                                .fit_to_exact_size((20.0, 20.0).into())
                                .rotate(90.0_f32.to_radians() as f32, Vec2::splat(0.5));

                                ui.add(image).on_hover_text(species);
                            } else {
                                ui.label(species);
                            }
                        });

                        if self.tab_state.settings.replay_settings.show_observed_damage {
                            ui.col(|ui| {
                                ui.label(separate_number(entity.damage(), self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())));
                            });
                        }

                        let species = ship.species().expect("ship has no species?");
                        let (skill_points, num_skills) = entity
                            .commander_skills()
                            .map(|skills| {
                                let points = skills.iter().fold(0usize, |accum, skill| accum + skill.tier().get_for_species(species.clone()));

                                (points, skills.len())
                            })
                            .unwrap_or((0, 0));

                        ui.col(|ui| {
                            ui.label(format!("{}pts ({} skills)", skill_points, num_skills));
                        });
                        ui.col(|ui| {
                            if ui.small_button("Build").clicked() {
                                let metadata_provider = self.tab_state.world_of_warships_data.game_metadata.as_ref().unwrap();

                                let url = build_ship_config_url(entity, metadata_provider);

                                ui.ctx().open_url(OpenUrl::new_tab(url));
                            }
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

                    if ui.add(Label::new(job).sense(Sense::click())).on_hover_text(format!("Click to copy")).clicked() {
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
                ui.heading(player_name_with_clan(&*self_player));
                ui.label(report.match_group());
                ui.label(report.version().to_path());
                ui.label(report.game_mode());
                ui.label(report.map_name());
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
        let replay_dir = Path::new(self.tab_state.settings.wows_dir.as_str()).join("replays");
        if let Some(replay_files) = &mut self.tab_state.replay_files {
            if let Some(file) = self.tab_state.file_receiver.as_ref() {
                while let Ok(new_file) = file.try_recv() {
                    replay_files.insert(0, new_file);
                }
            }
        } else {
            let mut files = Vec::new();
            if replay_dir.exists() {
                for file in std::fs::read_dir(&replay_dir).expect("failed to read replay dir") {
                    if let Ok(file) = file {
                        if !file.file_type().expect("failed to get file type").is_file() {
                            continue;
                        }

                        let file_path = file.path();

                        if let Some("wowsreplay") = file_path.extension().map(|s| s.to_str().expect("failed to convert extension to str")) {
                            files.push(file_path)
                        }
                    }
                }
            }
            if !files.is_empty() {
                files.sort_by(|a, b| a.metadata().unwrap().created().unwrap().cmp(&b.metadata().unwrap().created().unwrap()));
                files.reverse();
                self.tab_state.replay_files = Some(files);
            }
        };

        if self.tab_state.replay_files.is_some() && self.tab_state.file_watcher.is_none() && replay_dir.exists() {
            let (tx, rx) = mpsc::channel();
            // Automatically select the best implementation for your platform.
            let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    eprintln!("{:?}", event);
                    if let EventKind::Create(_) = event.kind {
                        for path in event.paths {
                            if path.is_file() && path.extension().map(|ext| ext == "wowsreplay").unwrap_or(false) {
                                tx.send(path).expect("failed to send file creation event");
                            }
                        }
                    }
                }
                Err(e) => println!("watch error: {:?}", e),
                _ => {
                    // ignore other events
                }
            })
            .expect("failed to create fs watcher for replays dir");

            // Add a path to be watched. All files and directories at that path and
            // below will be monitored for changes.
            watcher.watch(replay_dir.as_ref(), RecursiveMode::NonRecursive).expect("failed to watch directory");

            self.tab_state.file_watcher = Some(watcher);
            self.tab_state.file_receiver = Some(rx);
        }

        ui.vertical(|ui| {
            egui::Grid::new("replay_files_grid").num_columns(1).striped(true).show(ui, |ui| {
                if let Some(files) = self.tab_state.replay_files.clone() {
                    for file in files {
                        if ui
                            .add(Label::new(file.file_name().expect("no filename?").to_str().expect("failed to convert path to string")).sense(Sense::click()))
                            .double_clicked()
                        {
                            self.parse_replay(file);
                        }
                        ui.end_row();
                    }
                }
            });
        });
    }

    fn parse_replay<P: AsRef<Path>>(&mut self, replay_path: P) {
        let path = replay_path.as_ref();

        {
            self.tab_state.replay_parser_tab.lock().unwrap().game_chat.clear();
        }
        let replay_file: ReplayFile = ReplayFile::from_file(path).unwrap();

        let mut replay = Replay {
            replay_file,
            resource_loader: self.tab_state.world_of_warships_data.game_metadata.clone().unwrap(),
            battle_report: None,
        };

        if let (Some(file_tree), Some(pkg_loader)) = (
            self.tab_state.world_of_warships_data.file_tree.as_ref(),
            self.tab_state.world_of_warships_data.pkg_loader.as_ref(),
        ) {
            replay.parse(file_tree, pkg_loader.clone());
        }

        self.tab_state.world_of_warships_data.current_replay = Some(replay);
    }

    /// Builds the replay parser tab
    pub fn build_replay_parser_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.tab_state.settings.current_replay_path.to_string_lossy().to_owned()).hint_text("Current Replay File"));

                if ui.button("Parse").clicked() {
                    self.parse_replay(self.tab_state.settings.current_replay_path.clone());
                }

                if ui.button("Browse...").clicked() {
                    if let Some(file) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file() {
                        //println!("{:#?}", ReplayFile::from_file(&file));

                        self.tab_state.settings.current_replay_path = file;
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
                    self.build_replay_view(replay_file, ui);
                }
            });
        });
    }
}
