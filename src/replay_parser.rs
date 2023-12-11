use std::{
    borrow::Cow,
    collections::HashMap,
    io::{self, Cursor},
    path::Path,
    sync::{Arc, Mutex},
};

use bounded_vec_deque::BoundedVecDeque;
use byteorder::{LittleEndian, ReadBytesExt};
use egui::{text::LayoutJob, Color32, Label, Sense, Separator, TextFormat};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use serde::{Deserialize, Serialize};
use wows_replays::{
    analyzer::{Analyzer, AnalyzerBuilder},
    packet2::{Packet, PacketType, PacketTypeKind},
    parse_scripts,
    rpc::typedefs::ArgValue,
    ReplayFile, ReplayMeta,
};

use itertools::Itertools;

use crate::app::{GameMessage, ReplayParserTabState, ToolkitTabViewer};

const CHAT_VIEW_WIDTH: f32 = 200.0;

pub type SharedReplayParserTabState = Arc<Mutex<ReplayParserTabState>>;

#[derive(Debug)]
pub struct ShipConfig {
    abilities: Vec<u32>,
    hull: u32,
    modernization: Vec<u32>,
    units: Vec<u32>,
    signals: Vec<u32>,
}

#[derive(Debug)]
pub struct Skills {
    aircraft_carrier: Vec<u8>,
    battleship: Vec<u8>,
    cruiser: Vec<u8>,
    destroyer: Vec<u8>,
    auxiliary: Vec<u8>,
    submarine: Vec<u8>,
}

#[derive(Debug)]
pub struct ShipLoadout {
    config: ShipConfig,
    skills: Skills,
}

struct ReplayAnalyzer {
    game_meta: ReplayMeta,
    replay_tab_state: SharedReplayParserTabState,
    last_packets: BoundedVecDeque<Vec<u8>>,
}

impl ReplayAnalyzer {
    pub fn new(game_meta: ReplayMeta, replay_tab_state: SharedReplayParserTabState) -> Self {
        Self {
            game_meta,
            replay_tab_state,
            last_packets: BoundedVecDeque::new(5),
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum ChatChannel {
    Division,
    Global,
    Team,
}

fn parse_ship_config(blob: &[u8]) -> io::Result<ShipConfig> {
    let mut reader = Cursor::new(blob);
    let _unk = reader.read_u32::<LittleEndian>()?;

    let ship_params_id = reader.read_u32::<LittleEndian>()?;

    let _unk2 = reader.read_u32::<LittleEndian>()?;

    let unit_count = reader.read_u32::<LittleEndian>()?;
    let mut units = Vec::with_capacity(unit_count as usize);
    for _ in 0..unit_count {
        units.push(reader.read_u32::<LittleEndian>()?);
    }

    let modernization_count = reader.read_u32::<LittleEndian>()?;
    let mut modernization = Vec::with_capacity(modernization_count as usize);
    for _ in 0..modernization_count {
        modernization.push(reader.read_u32::<LittleEndian>()?);
    }

    let signal_count = reader.read_u32::<LittleEndian>()?;
    let mut signals = Vec::with_capacity(signal_count as usize);
    for _ in 0..signal_count {
        signals.push(reader.read_u32::<LittleEndian>()?);
    }

    let _supply_state = reader.read_u32::<LittleEndian>()?;

    let camo_info_count = reader.read_u32::<LittleEndian>()?;
    println!("camo info count: {camo_info_count}");
    for _ in 0..camo_info_count {
        let _camo_info = reader.read_u32::<LittleEndian>()?;
        let _camo_scheme = reader.read_u32::<LittleEndian>()?;
    }

    let abilities_count = reader.read_u32::<LittleEndian>()?;
    let mut abilities = Vec::with_capacity(abilities_count as usize);
    for _ in 0..abilities_count {
        abilities.push(reader.read_u32::<LittleEndian>()?)
    }

    Ok(ShipConfig {
        abilities,
        hull: units[0],
        modernization,
        units,
        signals,
    })
}

impl Analyzer for ReplayAnalyzer {
    fn process(&mut self, packet: &wows_replays::packet2::Packet<'_, '_>) {
        println!(
            "packet: {}, type: 0x{:x}, len: {}",
            packet.payload.kind(),
            packet.packet_type,
            packet.packet_size,
        );
        if !matches!(packet.payload.kind(), PacketTypeKind::Unknown) {
            println!("{:#?}", packet.payload);
        }
        self.last_packets.push_back(packet.raw.to_vec());

        if let PacketType::BattleResults(results) = &packet.payload {
            std::fs::write("battle_results.json", results);
        }
        if let PacketType::EntityCreate(packet) = &packet.payload {
            println!("\t {:#?}", packet);
            if packet.entity_type != "Vehicle" {
                return;
            }

            {
                self.replay_tab_state
                    .lock()
                    .unwrap()
                    .vehicle_id_to_entity_id
                    .insert(packet.vehicle_id, packet.entity_id);
            }
            let config = if let Some(ArgValue::Blob(ship_config)) = packet.props.get("shipConfig") {
                let config =
                    parse_ship_config(ship_config.as_slice()).expect("failed to parse ship config");
                println!("{:#?}", config);

                config
            } else {
                panic!("ship config is not a blob")
            };

            let skills = if let Some(ArgValue::FixedDict(crew_modifiers)) =
                packet.props.get("crewModifiersCompactParams")
            {
                if let Some(ArgValue::Array(learned_skills)) = crew_modifiers.get("learnedSkills") {
                    let skills_from_idx = |idx: usize| -> Vec<u8> {
                        learned_skills[idx]
                            .array_ref()
                            .unwrap()
                            .iter()
                            .map(|idx| *(*idx).uint_8_ref().unwrap())
                            .collect()
                    };

                    Skills {
                        aircraft_carrier: skills_from_idx(0),
                        battleship: skills_from_idx(1),
                        cruiser: skills_from_idx(2),
                        destroyer: skills_from_idx(3),
                        auxiliary: skills_from_idx(4),
                        submarine: skills_from_idx(5),
                    }
                } else {
                    panic!("learnedSkills is not an array");
                }
            } else {
                panic!("crew modifiers is not a dictionary");
            };

            let loadout = ShipLoadout { config, skills };
            println!("{:#?}", loadout);
            self.replay_tab_state
                .lock()
                .unwrap()
                .ship_configs
                .insert(packet.entity_id, loadout);
        }

        if let PacketType::BasePlayerCreate(packet) = &packet.payload {
            println!("\t {:?}", packet)
        }
        if let PacketType::CellPlayerCreate(packet) = &packet.payload {
            println!("\t {:?}", packet)
        }
        if let PacketType::PropertyUpdate(packet) = &packet.payload {
            println!("\t {:?}", packet)
        }
        if let PacketType::EntityProperty(packet) = &packet.payload {
            println!("\t {:?}", packet);
        }

        if let PacketType::EntityMethod(packet) = &packet.payload {
            println!("\t {}", packet.method);
            if packet.method == "onBattleEnd" {
                println!("{:?}", packet);
            }
            if packet.method == "onChatMessage" {
                let sender = packet.args[0].clone().int_32().unwrap();
                let mut sender_team = None;
                let channel = std::str::from_utf8(packet.args[1].string_ref().unwrap()).unwrap();
                let message = std::str::from_utf8(packet.args[2].string_ref().unwrap()).unwrap();

                let channel = match channel {
                    "battle_common" => ChatChannel::Global,
                    "battle_team" => ChatChannel::Team,
                    other => panic!("unknown channel {channel}"),
                };

                let mut sender_name = "Unknown".to_owned();
                for player in &self.game_meta.vehicles {
                    if player.id == (sender as i64) {
                        sender_name = player.name.clone();
                        sender_team = Some(player.relation);
                    }
                }

                println!(
                    "chat message from sender {sender_name} in channel {channel:?}: {message}"
                );

                self.replay_tab_state
                    .lock()
                    .unwrap()
                    .game_chat
                    .push(GameMessage {
                        sender_relation: sender_team.unwrap(),
                        sender_name,
                        channel,
                        message: message.to_string(),
                    });
            }
        }
        if let PacketTypeKind::Invalid = packet.payload.kind() {
            println!("{:#?}", packet.payload);
        }
    }

    fn finish(&self) {
        for (i, data) in self.last_packets.iter().enumerate() {
            std::fs::write(format!("packet_{i}.bin"), data);
        }
    }
}

impl ToolkitTabViewer<'_> {
    /// Builds the replay parser tab
    pub fn build_replay_parser_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(
                        &mut self
                            .tab_state
                            .settings
                            .current_replay_path
                            .to_string_lossy()
                            .to_owned(),
                    )
                    .hint_text("Current Replay File"),
                );

                if ui.button("reparse").clicked() {
                    {
                        self.tab_state
                            .replay_parser_tab
                            .lock()
                            .unwrap()
                            .game_chat
                            .clear();
                    }
                    let replay_file: ReplayFile =
                        ReplayFile::from_file(&self.tab_state.settings.current_replay_path)
                            .unwrap();
                    println!("{:#?}", replay_file.meta);

                    if let (Some(file_tree), Some(pkg_loader)) =
                        (&self.tab_state.file_tree, &self.tab_state.pkg_loader)
                    {
                        let data_file_loader =
                            wows_replays::version::DataFileWithCallback::new(|path| {
                                println!("requesting file: {path}");

                                let path = Path::new(path);

                                let mut file_data = Vec::new();
                                file_tree
                                    .read_file_at_path(path, pkg_loader, &mut file_data)
                                    .expect("failed to read file");

                                Ok(Cow::Owned(file_data))
                            });

                        let analyzer = Box::new(ReplayAnalyzer::new(
                            replay_file.meta.clone(),
                            Arc::clone(&self.tab_state.replay_parser_tab),
                        ));
                        let specs = parse_scripts(&data_file_loader).unwrap();

                        let version_parts: Vec<_> =
                            replay_file.meta.clientVersionFromExe.split(",").collect();
                        assert!(version_parts.len() == 4);

                        let processor = analyzer;

                        // Parse packets
                        let mut p = wows_replays::packet2::Parser::new(&specs);
                        let mut analyzer_set =
                            wows_replays::analyzer::AnalyzerAdapter::new(vec![processor]);
                        match p.parse_packets::<wows_replays::analyzer::AnalyzerAdapter>(
                            &replay_file.packet_data,
                            &mut analyzer_set,
                        ) {
                            Ok(()) => {
                                analyzer_set.finish();
                            }
                            Err(e) => panic!("{:?}", e),
                        }
                    }

                    self.tab_state.settings.current_replay = Some(replay_file);
                }

                if ui.button("parse").clicked() {
                    if let Some(file) = rfd::FileDialog::new()
                        .add_filter("WoWs Replays", &["wowsreplay"])
                        .pick_file()
                    {
                        //println!("{:#?}", ReplayFile::from_file(&file));

                        self.tab_state.settings.current_replay_path = file;
                    }
                }
            });

            if let Some(replay_file) = &self.tab_state.settings.current_replay {
                let meta = &replay_file.meta;
                std::fs::write("meta.txt", format!("{:#?}", meta).as_bytes());
                ui.horizontal(|ui| {
                    ui.heading(meta.playerName.as_str());
                    ui.label(meta.matchGroup.as_str());
                    ui.label(meta.clientVersionFromExe.as_str());
                    if let Some(translations) = self.tab_state.translations.as_ref() {
                        let id = format!("IDS_{}", meta.scenario.to_uppercase());
                        ui.label(translations.gettext(id.as_str()));

                        let id = format!("IDS_{}", meta.mapName.to_uppercase());
                        ui.label(translations.gettext(id.as_str()));
                    } else {
                        ui.label(meta.scenario.as_str());
                        ui.label(meta.mapDisplayName.as_str());
                    }
                });

                StripBuilder::new(ui)
                    .size(Size::remainder())
                    .size(Size::exact(CHAT_VIEW_WIDTH))
                    .horizontal(|mut strip| {
                        strip.cell(|ui| {
                            let table = TableBuilder::new(ui)
                                .striped(true)
                                .resizable(true)
                                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                .column(Column::auto())
                                .column(Column::initial(100.0).range(40.0..=300.0))
                                .column(Column::initial(100.0).at_least(40.0).clip(true))
                                .column(Column::initial(100.0).at_least(40.0).clip(true))
                                .column(Column::initial(100.0).at_least(40.0).clip(true))
                                .column(Column::remainder())
                                .min_scrolled_height(0.0);

                            table
                                .header(20.0, |mut header| {
                                    header.col(|ui| {
                                        ui.strong("Player Name");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Relation");
                                    });
                                    header.col(|ui| {
                                        ui.strong("ID");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Ship Name");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Ship Class");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Allocated Skills");
                                    });
                                })
                                .body(|mut body| {
                                    let mut sorted_players = meta.vehicles.clone();
                                    sorted_players.sort_by(|a, b| a.relation.cmp(&b.relation));
                                    for player in &sorted_players {
                                        body.row(30.0, |mut ui| {
                                            ui.col(|ui| {
                                                ui.label(player.name.clone());
                                            });
                                            ui.col(|ui| {
                                                ui.label(match player.relation {
                                                    0 => "Self".to_string(),
                                                    1 => "Friendly".to_string(),
                                                    other => format!("Enemy Team ({other})"),
                                                });
                                            });
                                            ui.col(|ui| {
                                                ui.label(format!("{}", player.id));
                                            });
                                            ui.col(|ui| {
                                                if let (Some(game_params), Some(translations)) = (
                                                    &self.tab_state.game_params,
                                                    &self.tab_state.translations,
                                                ) {
                                                    let translation_id = game_params
                                                        .ship_id_to_localization_id(
                                                            player.shipId as i64,
                                                        )
                                                        .unwrap();
                                                    ui.label(translations.gettext(translation_id));
                                                } else {
                                                    ui.label(format!("{}", player.shipId));
                                                }
                                            });
                                            ui.col(|ui| {
                                                let mut got_ship_info = true;
                                                if let (Some(game_params), Some(translations)) = (
                                                    &self.tab_state.game_params,
                                                    &self.tab_state.translations,
                                                ) {
                                                    if let Some(type_info) = game_params
                                                        .ship_type_info(player.shipId as i64)
                                                    {
                                                        if let Some(serde_pickle::Value::String(
                                                            species,
                                                        )) = type_info.get(
                                                            &serde_pickle::HashableValue::String(
                                                                "species".to_string(),
                                                            ),
                                                        ) {
                                                            let translation_id = format!(
                                                                "IDS_{}",
                                                                species.to_uppercase()
                                                            );
                                                            ui.label(
                                                                translations.gettext(
                                                                    translation_id.as_str(),
                                                                ),
                                                            );
                                                        } else {
                                                            println!("failed to get species");
                                                            got_ship_info = false;
                                                        }
                                                    } else {
                                                        println!("failed to get ship info");
                                                        got_ship_info = false;
                                                    }
                                                }
                                                if !got_ship_info {
                                                    ui.label("unk");
                                                }
                                            });

                                            ui.col(|ui| {
                                                ui.label("foo");
                                                // let replay_tab = self
                                                //     .tab_state
                                                //     .replay_parser_tab
                                                //     .lock()
                                                //     .unwrap();
                                                // let entity_id = replay_tab
                                                //     .vehicle_id_to_entity_id
                                                //     .get(&(player.id as u32))
                                                //     .expect("failed to get entity ID");

                                                // let ship_loadouts =
                                                //     replay_tab.ship_configs.get(entity_id).unwrap();
                                                // let skills = [
                                                //     &ship_loadouts.skills.aircraft_carrier,
                                                //     &ship_loadouts.skills.auxiliary,
                                                //     &ship_loadouts.skills.battleship,
                                                //     &ship_loadouts.skills.cruiser,
                                                //     &ship_loadouts.skills.destroyer,
                                                //     &ship_loadouts.skills.submarine,
                                                // ];
                                                // for skillset in skills {
                                                //     if !skillset.is_empty() {
                                                //         ui.label(format!("{}", skillset.len()));
                                                //         break;
                                                //     }
                                                // }
                                            });
                                        });
                                    }
                                });
                        });
                        strip.cell(|ui| {
                            let tab_state = self.tab_state.replay_parser_tab.lock().unwrap();
                            egui::ScrollArea::vertical()
                                .id_source("game_chat_scroll_area")
                                .show(ui, |ui| {
                                    egui::Grid::new("filtered_files_grid")
                                        .max_col_width(CHAT_VIEW_WIDTH)
                                        .num_columns(1)
                                        .striped(true)
                                        .show(ui, |ui| {
                                            for message in &tab_state.game_chat {
                                                let GameMessage {
                                                    sender_relation,
                                                    sender_name,
                                                    channel,
                                                    message,
                                                } = message;

                                                let text = format!(
                                                    "{sender_name} ({channel:?}): {message}"
                                                );

                                                let is_dark_mode = ui.visuals().dark_mode;
                                                let name_color = match *sender_relation {
                                                    0 => Color32::GOLD,
                                                    1 => {
                                                        if is_dark_mode {
                                                            Color32::LIGHT_GREEN
                                                        } else {
                                                            Color32::DARK_GREEN
                                                        }
                                                    }
                                                    _ => {
                                                        if is_dark_mode {
                                                            Color32::LIGHT_RED
                                                        } else {
                                                            Color32::DARK_RED
                                                        }
                                                    }
                                                };

                                                let mut job = LayoutJob::default();
                                                job.append(
                                                    &format!("{sender_name}: "),
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

                                                if ui
                                                    .add(Label::new(job).sense(Sense::click()))
                                                    .on_hover_text(format!("Click to copy"))
                                                    .clicked()
                                                {
                                                    ui.output_mut(|output| {
                                                        output.copied_text = text
                                                    });
                                                }
                                                ui.end_row();
                                            }
                                        });
                                });
                        });
                    });
            }
        });
    }
}
