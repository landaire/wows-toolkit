use std::{
    borrow::Cow,
    collections::HashMap,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use crate::{
    app::TimedMessage,
    icons, update_background_task,
    util::build_tomato_gg_url,
    wows_data::{load_replay, parse_replay, ShipIcon, WorldOfWarshipsData},
};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use egui::{mutex::Mutex, text::LayoutJob, Color32, FontId, Image, ImageSource, Label, OpenUrl, RichText, Sense, Separator, TextFormat, Vec2};
use egui_extras::{Column, TableBuilder, TableRow};

use escaper::decode_html;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tap::Pipe;
use tracing::debug;

use wows_replays::{
    analyzer::{
        battle_controller::{BattleController, BattleReport, ChatChannel, GameMessage, Player, VehicleEntity},
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

fn ship_class_icon_from_species(species: Species, wows_data: &WorldOfWarshipsData) -> Option<Arc<ShipIcon>> {
    wows_data.ship_icons.get(&species).cloned()
}

struct SkillInfo {
    skill_points: usize,
    num_skills: usize,
    highest_tier: usize,
    num_tier_1_skills: usize,
    hover_text: Option<&'static str>,
    label_text: RichText,
}

struct Damage {
    ap: Option<u64>,
    sap: Option<u64>,
    he: Option<u64>,
    he_secondaries: Option<u64>,
    sap_secondaries: Option<u64>,
    torps: Option<u64>,
    deep_water_torps: Option<u64>,
    fire: Option<u64>,
    flooding: Option<u64>,
}

struct PotentialDamage {
    artillery: u64,
    torpedoes: u64,
    planes: u64,
}

pub struct VehicleReport {
    vehicle: Arc<VehicleEntity>,
    color: Color32,
    name_text: RichText,
    clan_text: Option<RichText>,
    ship_species_text: String,
    icon: Option<Arc<ShipIcon>>,
    division_label: Option<String>,
    base_xp: Option<i64>,
    base_xp_text: Option<RichText>,
    raw_xp: Option<i64>,
    raw_xp_text: Option<String>,
    observed_damage: u64,
    observed_damage_text: String,
    actual_damage: Option<u64>,
    actual_damage_report: Option<Damage>,
    actual_damage_text: Option<RichText>,
    /// RichText to support monospace font
    actual_damage_hover_text: Option<RichText>,
    ship_name: String,
    spotting_damage: Option<u64>,
    spotting_damage_text: Option<String>,
    potential_damage: Option<u64>,
    potential_damage_text: Option<String>,
    potential_damage_hover_text: Option<String>,
    potential_damage_report: Option<PotentialDamage>,
    time_lived_secs: Option<u64>,
    time_lived_text: Option<String>,
    skill_info: SkillInfo,
}

use std::cmp::Reverse;

enum SortKey {
    String(String),
    i64(Option<i64>),
    u64(Option<u64>),
    Species(Species),
}

impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SortKey::String(a), SortKey::String(b)) => a == b,
            (SortKey::i64(a), SortKey::i64(b)) => a == b,
            (SortKey::u64(a), SortKey::u64(b)) => a == b,
            (SortKey::Species(a), SortKey::Species(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for SortKey {}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (SortKey::String(a), SortKey::String(b)) => a.cmp(b),
            (SortKey::i64(a), SortKey::i64(b)) => a.cmp(b),
            (SortKey::u64(a), SortKey::u64(b)) => a.cmp(b),
            (SortKey::Species(a), SortKey::Species(b)) => a.cmp(b),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

pub struct UiReport {
    match_timestamp: DateTime<Local>,
    self_player: Option<Arc<VehicleEntity>>,
    vehicle_reports: Vec<VehicleReport>,
    sorted: bool,
}

impl UiReport {
    fn new(replay_file: &ReplayFile, report: &BattleReport, wows_data: &WorldOfWarshipsData, metadata_provider: &GameMetadataProvider) -> Self {
        let match_timestamp = NaiveDateTime::parse_from_str(&replay_file.meta.dateTime, "%d.%m.%Y %H:%M:%S").expect("parsing replay date failed");
        let match_timestamp = Local.from_local_datetime(&match_timestamp).single().expect("failed to convert to local time");

        let players = report.player_entities().to_vec();

        let mut divisions: HashMap<u32, char> = Default::default();
        let mut remaining_div_identifiers: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().rev().collect();

        let self_player = players
            .iter()
            .find(|vehicle| vehicle.player().map(|player| player.relation() == 0).unwrap_or(false))
            .cloned();
        let locale = "en-US";

        let player_reports = players.iter().filter_map(|vehicle| {
            let player = vehicle.player()?;
            let mut player_color = player_color_for_team_relation(player.relation());

            if let Some(self_player) = self_player.as_ref().and_then(|vehicle| vehicle.player().cloned()) {
                if self_player.db_id() != player.db_id() && self_player.division_id() > 0 && player.division_id() == self_player.division_id() {
                    player_color = Color32::GOLD;
                }
            }

            let vehicle_param = player.vehicle();

            let ship_species_text: String = vehicle_param
                .species()
                .and_then(|species| {
                    let species: &'static str = species.into();
                    let id = format!("IDS_{}", species.to_uppercase());
                    metadata_provider.localized_name_from_id(&id)
                })
                .unwrap_or_else(|| "unk".to_string());

            let icon = ship_class_icon_from_species(vehicle_param.species().expect("ship has no species"), wows_data);

            let name_color = if player.is_abuser() {
                Color32::from_rgb(0xFF, 0xC0, 0xCB) // pink
            } else {
                player_color
            };

            // Assign division
            let div = player.division_id();
            let division_char = if div > 0 {
                Some(divisions.entry(div).or_insert_with(|| remaining_div_identifiers.pop().unwrap_or('?')).clone())
            } else {
                None
            };

            let div_text = if let Some(div) = division_char { Some(format!("({})", div)) } else { None };

            let clan_text = if !player.clan().is_empty() {
                Some(RichText::new(format!("[{}]", player.clan())).color(clan_color_for_player(player).unwrap()))
            } else {
                None
            };
            let name_text = RichText::new(player.name()).color(name_color);

            let (base_xp, base_xp_text) = if let Some(base_xp) = vehicle.results_info().and_then(|info| {
                info.as_array()
                    .and_then(|info_array| info_array[XP_INDEX].as_number().and_then(|number| number.as_i64()))
            }) {
                let label_text = separate_number(base_xp, Some(locale));
                (Some(base_xp), Some(RichText::new(label_text).color(player_color)))
            } else {
                (None, None)
            };

            let (raw_xp, raw_xp_text) = if let Some(raw_xp) = vehicle.results_info().and_then(|info| {
                info.as_array()
                    .and_then(|info_array| info_array[XP_INDEX - 1].as_number().and_then(|number| number.as_i64()))
            }) {
                let label_text = separate_number(raw_xp, Some(locale));
                (Some(raw_xp), Some(label_text))
            } else {
                (None, None)
            };

            let ship_name = metadata_provider
                .localized_name_from_param(vehicle_param)
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("{}", vehicle_param.id()));

            let observed_damage = vehicle.damage().ceil() as u64;
            let observed_damage_text = separate_number(observed_damage, Some(locale));

            // Actual damage
            let (damage, damage_text, damage_hover_text, damage_report) = vehicle
                .results_info()
                .and_then(|info| info.as_array())
                .and_then(|info_array| {
                    info_array[DAMAGE_INDEX].as_number().and_then(|number| number.as_u64()).map(|damage_number| {
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
                            (DAMAGE_FLOODS, "Flood"),
                        ];

                        // Grab each damage index and format by <DAMAGE_TYPE>: <DAMAGE_NUM> as a collection of strings
                        let breakdowns: Vec<String> = damage_stats
                            .iter()
                            .filter_map(|(idx, description)| {
                                info_array[*idx].as_number().and_then(|number| number.as_u64()).map(|num| {
                                    let num = separate_number(num, Some(locale));
                                    format!("{:<16}: {}", description, num)
                                })
                            })
                            .collect();

                        let damage_report_text = separate_number(damage_number, Some(locale));
                        let damage_report_text = RichText::new(damage_report_text).color(player_color);
                        let damage_report_hover_text = RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                        (
                            Some(damage_number),
                            Some(damage_report_text),
                            Some(damage_report_hover_text),
                            Some(Damage {
                                ap: info_array[DAMAGE_AP].as_number().and_then(|number| number.as_u64()),
                                sap: info_array[DAMAGE_SAP].as_number().and_then(|number| number.as_u64()),
                                he: info_array[DAMAGE_HE].as_number().and_then(|number| number.as_u64()),
                                he_secondaries: info_array[DAMAGE_HE_SECONDARIES].as_number().and_then(|number| number.as_u64()),
                                sap_secondaries: info_array[DAMAGE_SAP_SECONDARIES].as_number().and_then(|number| number.as_u64()),
                                torps: info_array[DAMAGE_NORMAL_TORPS].as_number().and_then(|number| number.as_u64()),
                                deep_water_torps: info_array[DAMAGE_DEEP_WATER_TORPS].as_number().and_then(|number| number.as_u64()),
                                fire: info_array[DAMAGE_FIRE].as_number().and_then(|number| number.as_u64()),
                                flooding: info_array[DAMAGE_FLOODS].as_number().and_then(|number| number.as_u64()),
                            }),
                        )
                    })
                })
                .unwrap_or_default();

            // Spotting damage
            const SPOTTING_DAMAGE_INDEX: usize = 398;
            let (spotting_damage, spotting_damage_text) = if let Some(damage_number) = vehicle.results_info().and_then(|info| {
                info.as_array()
                    .and_then(|info_array| info_array[SPOTTING_DAMAGE_INDEX].as_number().and_then(|number| number.as_u64()))
            }) {
                (Some(damage_number), Some(separate_number(damage_number, Some(locale))))
            } else {
                (None, None)
            };

            // Potential damage
            const ARTILLERY_POTENTIAL_DAMAGE: usize = 402;
            const _TORPEDO_POTENTIAL_DAMAGE: usize = 403; // may not be accurate?
            const AIRSTRIKE_POTENTIAL_DAMAGE: usize = 404;

            let (potential_damage, potential_damage_text, potential_damage_hover_text, potential_damage_report) = if let Some(damage_numbers) = vehicle
                .results_info()
                .and_then(|info| info.as_array().map(|info_array| &info_array[ARTILLERY_POTENTIAL_DAMAGE..=AIRSTRIKE_POTENTIAL_DAMAGE]))
            {
                let total_pot = damage_numbers
                    .iter()
                    .map(|num| num.as_f64())
                    .fold(0, |accum, num| accum + num.map(|f| f as u64).unwrap_or_default());

                let potential_damage_text = separate_number(total_pot, Some(locale));

                let potential_damage_report = PotentialDamage {
                    artillery: damage_numbers[0].as_f64().unwrap_or_default() as u64,
                    torpedoes: damage_numbers[0].as_f64().unwrap_or_default() as u64,
                    planes: damage_numbers[0].as_f64().unwrap_or_default() as u64,
                };

                let hover_string = format!(
                    "Artillery: {}\nTorpedo: {}\nPlanes: {}",
                    separate_number(potential_damage_report.artillery, Some(locale)),
                    separate_number(potential_damage_report.torpedoes, Some(locale)),
                    separate_number(potential_damage_report.planes, Some(locale)),
                );

                (Some(total_pot), Some(potential_damage_text), Some(hover_string), Some(potential_damage_report))
            } else {
                (None, None, None, None)
            };

            let (time_lived, time_lived_text) = vehicle
                .death_info()
                .map(|death_info| {
                    let secs = death_info.time_lived().as_secs();
                    (Some(secs), Some(format!("{}:{:02}", secs / 60, secs % 60)))
                })
                .unwrap_or_default();

            let species = vehicle_param.species().expect("ship has no species?");
            let (skill_points, num_skills, highest_tier, num_tier_1_skills) = vehicle
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

            let (label, hover_text) = util::colorize_captain_points(skill_points, num_skills, highest_tier, num_tier_1_skills);

            let skill_info = SkillInfo {
                skill_points,
                num_skills,
                highest_tier,
                num_tier_1_skills,
                hover_text: hover_text,
                label_text: label,
            };

            Some(VehicleReport {
                vehicle: Arc::clone(&vehicle),
                color: player_color,
                name_text,
                clan_text,
                icon,
                division_label: div_text,
                base_xp,
                base_xp_text,
                raw_xp,
                raw_xp_text,
                observed_damage,
                observed_damage_text,
                actual_damage: damage,
                actual_damage_report: damage_report,
                actual_damage_text: damage_text,
                actual_damage_hover_text: damage_hover_text,
                ship_name,
                spotting_damage,
                spotting_damage_text,
                potential_damage,
                potential_damage_hover_text,
                potential_damage_report,
                time_lived_secs: time_lived,
                time_lived_text,
                skill_info,
                potential_damage_text,
                ship_species_text,
            })
        });

        Self {
            match_timestamp,
            vehicle_reports: player_reports.collect(),
            self_player,
            sorted: false,
        }
    }

    fn sort_players(&mut self, sort_order: SortOrder) {
        let self_player_team_id = self
            .self_player
            .as_ref()
            .expect("no self player?")
            .player()
            .as_ref()
            .expect("no self player player")
            .team_id();

        let sort_key = |report: &VehicleReport, column: &SortColumn| {
            let player = report.vehicle.player().expect("no player?");
            let team_id = player.team_id() != self_player_team_id;
            let db_id = player.db_id();

            let key = match column {
                SortColumn::Name => SortKey::String(player.name().to_string()),
                SortColumn::BaseXp => SortKey::i64(report.base_xp),
                SortColumn::RawXp => SortKey::i64(report.raw_xp),
                SortColumn::ShipName => SortKey::String(report.ship_name.clone()),
                SortColumn::ShipClass => SortKey::Species(player.vehicle().species().expect("no species for vehicle?")),
                SortColumn::ObservedDamage => SortKey::u64(Some(report.observed_damage)),
                SortColumn::ActualDamage => SortKey::u64(report.actual_damage),
                SortColumn::SpottingDamage => SortKey::u64(report.spotting_damage),
                SortColumn::PotentialDamage => SortKey::u64(report.potential_damage),
                SortColumn::TimeLived => SortKey::u64(report.time_lived_secs),
            };

            (team_id, key, db_id)
        };

        match sort_order {
            SortOrder::Desc(column) => {
                self.vehicle_reports.sort_unstable_by_key(|report| {
                    let key = sort_key(report, &column);
                    (key.0, Reverse(key.1), key.2)
                });
            }
            SortOrder::Asc(column) => {
                self.vehicle_reports.sort_unstable_by_key(|report| sort_key(report, &column));
            }
        }

        self.sorted = true;
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortOrder {
    Asc(SortColumn),
    Desc(SortColumn),
}

impl Default for SortOrder {
    fn default() -> Self {
        SortOrder::Asc(SortColumn::ShipClass)
    }
}

impl SortOrder {
    fn icon(&self) -> &'static str {
        match self {
            SortOrder::Asc(_) => icons::SORT_ASCENDING,
            SortOrder::Desc(_) => icons::SORT_DESCENDING,
        }
    }

    fn toggle(&mut self) {
        match self {
            // By default everything should be Descending. Descending transitions to ascending. Ascending transitions back to default state.
            SortOrder::Asc(_) => *self = Default::default(),
            SortOrder::Desc(column) => *self = SortOrder::Asc(*column),
        }
    }

    fn update_column(&mut self, new_column: SortColumn) {
        match self {
            SortOrder::Asc(sort_column) | SortOrder::Desc(sort_column) if *sort_column == new_column => {
                self.toggle();
            }
            _ => *self = SortOrder::Desc(new_column),
        }
    }

    fn column(&self) -> SortColumn {
        match self {
            SortOrder::Asc(sort_column) | SortOrder::Desc(sort_column) => *sort_column,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortColumn {
    Name,
    BaseXp,
    RawXp,
    ShipName,
    ShipClass,
    ObservedDamage,
    ActualDamage,
    SpottingDamage,
    PotentialDamage,
    TimeLived,
}

pub struct Replay {
    pub replay_file: ReplayFile,

    pub resource_loader: Arc<GameMetadataProvider>,

    pub battle_report: Option<BattleReport>,
    pub ui_report: Option<UiReport>,
}

fn clan_color_for_player(player: &Player) -> Option<Color32> {
    if player.clan().is_empty() {
        None
    } else {
        let clan_color = player.raw_props_with_name().get("clanColor").expect("no clan color?");
        let clan_color = clan_color.as_i64().expect("clan color is not an i64");
        Some(Color32::from_rgb(
            ((clan_color & 0xFF0000) >> 16) as u8,
            ((clan_color & 0xFF00) >> 8) as u8,
            (clan_color & 0xFF) as u8,
        ))
    }
}

impl Replay {
    pub fn new(replay_file: ReplayFile, resource_loader: Arc<GameMetadataProvider>) -> Self {
        Replay {
            replay_file,
            resource_loader,
            battle_report: None,
            ui_report: None,
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
    pub fn build_ui_report(&mut self, wows_data: &WorldOfWarshipsData, metadata_provider: &GameMetadataProvider) {
        if let Some(battle_report) = &self.battle_report {
            self.ui_report = Some(UiReport::new(&self.replay_file, battle_report, wows_data, metadata_provider))
        }
    }
}

fn column_name_with_sort_order(text: &'static str, has_info: bool, sort_order: SortOrder, column: SortColumn) -> Cow<'static, str> {
    if sort_order.column() == column {
        let text_with_icon = if has_info {
            format!("{} {} {}", text, icons::INFO, sort_order.icon())
        } else {
            format!("{} {}", text, sort_order.icon())
        };
        Cow::Owned(text_with_icon)
    } else if has_info {
        Cow::Owned(format!("{} {}", text, icons::INFO))
    } else {
        Cow::Borrowed(text)
    }
}

impl ToolkitTabViewer<'_> {
    fn metadata_provider(&self) -> Option<Arc<GameMetadataProvider>> {
        self.tab_state
            .world_of_warships_data
            .as_ref()
            .and_then(|wows_data| wows_data.read().game_metadata.clone())
    }

    fn replays_dir(&self) -> Option<PathBuf> {
        self.tab_state.world_of_warships_data.as_ref().map(|wows_data| wows_data.read().replays_dir.clone())
    }

    fn build_replay_player_list(&self, ui_report: &mut UiReport, report: &BattleReport, ui: &mut egui::Ui) {
        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::auto().clip(true))
            .column(Column::initial(75.0).clip(true))
            .column(Column::initial(85.0).clip(true))
            .pipe(|table| {
                if self.tab_state.settings.replay_settings.show_entity_id {
                    table.column(Column::initial(120.0).clip(true))
                } else {
                    table
                }
            })
            .column(Column::initial(120.0).clip(true))
            .pipe(|table| {
                if self.tab_state.settings.replay_settings.show_observed_damage {
                    table.column(Column::initial(135.0).clip(true))
                } else {
                    table
                }
            })
            .column(Column::initial(130.0).clip(true))
            .column(Column::initial(135.0).clip(true))
            .column(Column::initial(135.0).clip(true))
            // Time lived
            .column(Column::initial(110.0).clip(true))
            .column(Column::initial(120.0).clip(true))
            .column(Column::remainder())
            .min_scrolled_height(0.0);

        let mut replay_sort = self.tab_state.replay_sort.lock().expect("could not lock replay sort");
        if !ui_report.sorted {
            ui_report.sort_players(*replay_sort);
        }
        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Player Name", false, *replay_sort, SortColumn::Name)).clicked() {
                        replay_sort.update_column(SortColumn::Name);
                        ui_report.sort_players(*replay_sort);
                    };
                });
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Base XP", false, *replay_sort, SortColumn::BaseXp)).clicked() {
                        replay_sort.update_column(SortColumn::BaseXp);
                        ui_report.sort_players(*replay_sort);
                    }
                });
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Raw XP",true, *replay_sort, SortColumn::RawXp)).on_hover_text("Raw XP before win modifiers are applied.").clicked() {
                        replay_sort.update_column(SortColumn::RawXp);
                        ui_report.sort_players(*replay_sort);
                    }
                });
                if self.tab_state.settings.replay_settings.show_entity_id {
                    header.col(|ui| {
                        ui.strong("ID");
                    });
                }
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Ship Name", false, *replay_sort, SortColumn::ShipName)).clicked() {
                        replay_sort.update_column(SortColumn::ShipName);
                        ui_report.sort_players(*replay_sort);
                    }
                });
                if self.tab_state.settings.replay_settings.show_observed_damage {
                    header.col(|ui| {
                        if ui.strong(column_name_with_sort_order("Observed Damage", true, *replay_sort, SortColumn::ObservedDamage)).on_hover_text(
                            "Observed damage reflects only damage you witnessed (i.e. victim was visible on your screen). This value may be lower than actual damage.",
                        ).clicked() {
                            replay_sort.update_column(SortColumn::ObservedDamage);
                            ui_report.sort_players(*replay_sort);
                        }
                    });
                }
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Actual Damage", true, *replay_sort, SortColumn::ActualDamage)).on_hover_text(
                        "Actual damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
                    ).clicked() {
                            replay_sort.update_column(SortColumn::ActualDamage);
                            ui_report.sort_players(*replay_sort);
                    }
                });
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Spotting Damage", true, *replay_sort, SortColumn::SpottingDamage)).on_hover_text(
                        "Spotting damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
                    ).clicked() {
                            replay_sort.update_column(SortColumn::SpottingDamage);
                            ui_report.sort_players(*replay_sort);
                    }
                });
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Potential Damage", true, *replay_sort, SortColumn::PotentialDamage)).on_hover_text(
                        "Potential damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
                    ).clicked() {
                        replay_sort.update_column(SortColumn::PotentialDamage);
                        ui_report.sort_players(*replay_sort);
                    }
                });
                header.col(|ui| {
                    if ui.strong(column_name_with_sort_order("Time Lived", false, *replay_sort, SortColumn::TimeLived)).clicked() {
                        replay_sort.update_column(SortColumn::TimeLived);
                        ui_report.sort_players(*replay_sort);
                    }
                });
                header.col(|ui| {
                    ui.strong("Allocated Skills");
                });
                header.col(|ui| {
                    ui.strong("Actions");
                });
            })
            .body(|mut body| {
                for player_report in &ui_report.vehicle_reports {

                    body.row(30.0, |ui| {
                        self.build_player_row(ui_report, player_report, ui);
                    });
                }
            });
    }

    fn build_player_row(&self, ui_report: &UiReport, player_report: &VehicleReport, mut ui: TableRow<'_, '_>) {
        let twitch_state = self.tab_state.twitch_state.read();
        let match_timestamp = ui_report.match_timestamp;

        ui.col(|ui| {
            // Add ship icon
            if let Some(icon) = player_report.icon.as_ref() {
                let image = Image::new(ImageSource::Bytes {
                    uri: icon.path.clone().into(),
                    // the icon size is <1k, this clone is fairly cheap
                    bytes: icon.data.clone().into(),
                })
                .tint(player_report.color)
                .fit_to_exact_size((20.0, 20.0).into())
                .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));

                ui.add(image).on_hover_text(&player_report.ship_species_text);
            } else {
                ui.label(&player_report.ship_species_text);
            }

            // Add division ID
            if let Some(div) = player_report.division_label.as_ref() {
                ui.label(div);
            }

            // Add player clan
            if let Some(clan_text) = player_report.clan_text.clone() {
                ui.label(clan_text);
            }

            // Add player name
            ui.label(player_report.name_text.clone());

            // Add icons for player properties
            if let Some(player) = player_report.vehicle.player() {
                // Hidden profile icon
                if player.is_hidden() {
                    ui.label(icons::EYE_SLASH).on_hover_text("Player has a hidden profile");
                }

                // Stream sniper icon
                if let Some(timestamps) = twitch_state.player_is_potential_stream_sniper(player.name(), match_timestamp) {
                    let hover_text = timestamps
                        .iter()
                        .map(|(name, timestamps)| {
                            format!(
                                "Possible stream name: {}\nSeen: {} minutes after match start",
                                name,
                                timestamps
                                    .iter()
                                    .map(|ts| {
                                        let delta = ts.signed_duration_since(match_timestamp);
                                        delta.num_minutes()
                                    })
                                    .join(", ")
                            )
                        })
                        .join("\n\n");
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
            }
        });

        // Base XP
        ui.col(|ui| {
            if let Some(base_xp_text) = player_report.base_xp_text.clone() {
                ui.label(base_xp_text);
            } else {
                ui.label("-");
            }
        });

        // Raw XP
        ui.col(|ui| {
            if let Some(raw_xp_text) = player_report.raw_xp_text.clone() {
                ui.label(raw_xp_text);
            } else {
                ui.label("-");
            }
        });

        // Entity ID (debugging)
        if self.tab_state.settings.replay_settings.show_entity_id {
            ui.col(|ui| {
                ui.label(format!("{}", player_report.vehicle.id()));
            });
        }

        // Ship name
        ui.col(|ui| {
            ui.label(&player_report.ship_name);
        });

        // Observed damage
        if self.tab_state.settings.replay_settings.show_observed_damage {
            ui.col(|ui| {
                ui.label(&player_report.observed_damage_text);
            });
        }

        // Actual damage
        ui.col(|ui| {
            if let Some(damage_text) = player_report.actual_damage_text.clone() {
                let response = ui.label(damage_text);
                if let Some(hover_text) = player_report.actual_damage_hover_text.clone() {
                    response.on_hover_text(hover_text);
                }
            } else {
                ui.label("-");
            }
        });

        // Spotting damage
        ui.col(|ui| {
            if let Some(spotting_damage) = player_report.spotting_damage_text.as_ref() {
                ui.label(spotting_damage);
            } else {
                ui.label("-");
            }
        });

        // Potential damage
        ui.col(|ui| {
            if let Some(damage_text) = player_report.potential_damage_text.clone() {
                let response = ui.label(damage_text);
                if let Some(hover_text) = player_report.potential_damage_hover_text.as_ref() {
                    response.on_hover_text(hover_text);
                }
            } else {
                ui.label("-");
            }
        });

        // Time lived
        ui.col(|ui| {
            if let Some(time_lived) = player_report.time_lived_text.as_ref() {
                ui.label(time_lived);
            } else {
                ui.label("-");
            }
        });

        ui.col(|ui| {
            let response = ui.label(player_report.skill_info.label_text.clone());
            if let Some(hover_text) = player_report.skill_info.hover_text {
                response.on_hover_text(hover_text);
            }
        });
        ui.col(|ui| {
            ui.menu_button(icons::DOTS_THREE, |ui| {
                if ui.small_button(format!("{} Open Build in Browser", icons::SHARE)).clicked() {
                    let metadata_provider = self.metadata_provider().unwrap();

                    let url = build_ship_config_url(&player_report.vehicle, &metadata_provider);

                    ui.ctx().open_url(OpenUrl::new_tab(url));
                    ui.close_menu();
                }

                if ui.small_button(format!("{} Copy Build Link", icons::COPY)).clicked() {
                    let metadata_provider = self.metadata_provider().unwrap();

                    let url = build_ship_config_url(&player_report.vehicle, &metadata_provider);
                    ui.output_mut(|output| output.copied_text = url);
                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Build link copied", icons::CHECK_CIRCLE)));

                    ui.close_menu();
                }

                if ui.small_button(format!("{} Copy Short Build Link", icons::COPY)).clicked() {
                    let metadata_provider = self.metadata_provider().unwrap();

                    let url = build_short_ship_config_url(&player_report.vehicle, &metadata_provider);
                    ui.output_mut(|output| output.copied_text = url);
                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Build link copied", icons::CHECK_CIRCLE)));

                    ui.close_menu();
                }

                ui.separator();

                if ui.small_button(format!("{} Open Tomato.gg Page", icons::SHARE)).clicked() {
                    if let Some(url) = build_tomato_gg_url(&player_report.vehicle) {
                        ui.ctx().open_url(OpenUrl::new_tab(url));
                    }

                    ui.close_menu();
                }

                if ui.small_button(format!("{} Open WoWs Numbers Page", icons::SHARE)).clicked() {
                    if let Some(url) = build_wows_numbers_url(&player_report.vehicle) {
                        ui.ctx().open_url(OpenUrl::new_tab(url));
                    }

                    ui.close_menu();
                }

                ui.separator();

                if let Some(player) = player_report.vehicle.player() {
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
                }
            });
        });
    }

    fn build_replay_chat(&self, battle_report: &BattleReport, ui: &mut egui::Ui) {
        for message in battle_report.game_chat() {
            let GameMessage {
                sender_relation,
                sender_name,
                channel,
                message,
                entity_id,
                player,
            } = message;

            let translated_text = if sender_relation.is_none() {
                self.metadata_provider().and_then(|provider| {
                    let name = provider.localized_name_from_id(message).map(|name| Cow::Owned(name));

                    name
                })
            } else {
                None
            };

            let message = if let Ok(decoded) = decode_html(message.as_str()) {
                Cow::Owned(decoded)
            } else {
                Cow::Borrowed(message)
            };

            let text = match player {
                Some(player) if !player.clan().is_empty() => {
                    format!("[{}] {sender_name} ({channel:?}): {}", player.clan(), translated_text.as_ref().unwrap_or(&message))
                }
                _ => {
                    format!("{sender_name} ({channel:?}): {}", translated_text.as_ref().unwrap_or(&message))
                }
            };

            let name_color = if let Some(relation) = sender_relation {
                player_color_for_team_relation(*relation)
            } else {
                Color32::GRAY
            };

            let mut job = LayoutJob::default();
            if let Some(player) = player {
                if !player.clan().is_empty() {
                    job.append(
                        &format!("[{}] ", player.clan()),
                        0.0,
                        TextFormat {
                            color: clan_color_for_player(&*player).unwrap(),
                            ..Default::default()
                        },
                    );
                }
            }
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
                ChatChannel::Global => Color32::WHITE,
                ChatChannel::Team => Color32::LIGHT_GREEN,
            };

            job.append(
                translated_text.as_ref().unwrap_or(&message),
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

    fn build_replay_view(&self, replay_file: &mut Replay, ui: &mut egui::Ui) {
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
                                        entity_id,
                                        player,
                                    } = message;

                                    match player {
                                        Some(player) if !player.clan().is_empty() => {
                                            let _ = writeln!(file, "[{}] {} ({:?}): {}", player.clan(), sender_name, channel, message);
                                        }
                                        _ => {
                                            let _ = writeln!(file, "{} ({:?}): {}", sender_name, channel, message);
                                        }
                                    }
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
                                entity_id,
                                player,
                            } = message;
                            match player {
                                Some(player) if !player.clan().is_empty() => {
                                    let _ = writeln!(buf, "[{}] {} ({:?}): {}", player.clan(), sender_name, channel, message);
                                }
                                _ => {
                                    let _ = writeln!(buf, "{} ({:?}): {}", sender_name, channel, message);
                                }
                            }
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
                    if let Some(ui_report) = replay_file.ui_report.as_mut() {
                        self.build_replay_player_list(ui_report, report, ui);
                    }
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
                                update_background_task!(self.tab_state.background_task, load_replay(Arc::clone(wows_data), replay.clone()));
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
                                parse_replay(Arc::clone(wows_data), self.tab_state.settings.current_replay_path.clone())
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
                    let mut replay_file = replay_file.write();
                    self.build_replay_view(&mut replay_file, ui);
                } else {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        ui.heading("Double click or load a replay to view data");
                    });
                }
            });
        });
    }
}
