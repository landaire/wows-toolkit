use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool, mpsc::Sender},
};

use crate::{
    app::{ReplaySettings, TimedMessage},
    icons,
    task::{BackgroundTask, BackgroundTaskKind},
    update_background_task,
    util::build_tomato_gg_url,
    wows_data::{ShipIcon, WorldOfWarshipsData, load_replay, parse_replay},
};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use egui::{
    Color32, ComboBox, Context, FontId, Id, Image, ImageSource, Label, Margin, OpenUrl, PopupCloseBehavior, RichText, Sense, Separator, TextFormat, Vec2, text::LayoutJob,
};

use egui_extras::TableRow;
use escaper::decode_html;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tracing::debug;

use wows_replays::{
    ReplayFile,
    analyzer::{
        AnalyzerMut,
        battle_controller::{BattleController, BattleReport, ChatChannel, GameMessage, Player, VehicleEntity},
    },
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

pub type SharedReplayParserTabState = Arc<Mutex<ReplayParserTabState>>;

const DAMAGE_MAIN_AP: &str = "damage_main_ap";
const DAMAGE_MAIN_CS: &str = "damage_main_cs";
const DAMAGE_MAIN_HE: &str = "damage_main_he";
const DAMAGE_ATBA_AP: &str = "damage_atba_ap";
const DAMAGE_ATBA_CS: &str = "damage_atba_cs";
const DAMAGE_ATBA_HE: &str = "damage_atba_he";
const DAMAGE_TPD_NORMAL: &str = "damage_tpd_normal";
const DAMAGE_TPD_DEEP: &str = "damage_tpd_deep";
const DAMAGE_TPD_ALTER: &str = "damage_tpd_alter";
const DAMAGE_TPD_PHOTON: &str = "damage_tpd_photon";
const DAMAGE_BOMB: &str = "damage_bomb";
const DAMAGE_BOMB_AVIA: &str = "damage_bomb_avia";
const DAMAGE_BOMB_ALT: &str = "damage_bomb_alt";
const DAMAGE_BOMB_AIRSUPPORT: &str = "damage_bomb_airsupport";
const DAMAGE_DBOMB_AIRSUPPORT: &str = "damage_dbomb_airsupport";
const DAMAGE_TBOMB: &str = "damage_tbomb";
const DAMAGE_TBOMB_ALT: &str = "damage_tbomb_alt";
const DAMAGE_TBOMB_AIRSUPPORT: &str = "damage_tbomb_airsupport";
const DAMAGE_FIRE: &str = "damage_fire";
const DAMAGE_RAM: &str = "damage_ram";
const DAMAGE_FLOOD: &str = "damage_flood";
const DAMAGE_DBOMB_DIRECT: &str = "damage_dbomb_direct";
const DAMAGE_DBOMB_SPLASH: &str = "damage_dbomb_splash";
const DAMAGE_SEA_MINE: &str = "damage_sea_mine";
const DAMAGE_ROCKET: &str = "damage_rocket";
const DAMAGE_ROCKET_AIRSUPPORT: &str = "damage_rocket_airsupport";
const DAMAGE_SKIP: &str = "damage_skip";
const DAMAGE_SKIP_ALT: &str = "damage_skip_alt";
const DAMAGE_SKIP_AIRSUPPORT: &str = "damage_skip_airsupport";
const DAMAGE_WAVE: &str = "damage_wave";
const DAMAGE_CHARGE_LASER: &str = "damage_charge_laser";
const DAMAGE_PULSE_LASER: &str = "damage_pulse_laser";
const DAMAGE_AXIS_LASER: &str = "damage_axis_laser";
const DAMAGE_PHASER_LASER: &str = "damage_phaser_laser";

static DAMAGE_DESCRIPTIONS: [(&str, &str); 34] = [
    (DAMAGE_MAIN_AP, "AP"),
    (DAMAGE_MAIN_CS, "SAP"),
    (DAMAGE_MAIN_HE, "HE"),
    (DAMAGE_ATBA_AP, "AP Sec"),
    (DAMAGE_ATBA_CS, "SAP Sec"),
    (DAMAGE_ATBA_HE, "HE Sec"),
    (DAMAGE_TPD_NORMAL, "Torps"),
    (DAMAGE_TPD_DEEP, "Deep Water Torps"),
    (DAMAGE_TPD_ALTER, "Alt Torps"),
    (DAMAGE_TPD_PHOTON, "Photon Torps"),
    (DAMAGE_BOMB, "HE Bomb"),
    (DAMAGE_BOMB_AVIA, "Bomb"),
    (DAMAGE_BOMB_ALT, "Alt Bomb"),
    (DAMAGE_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (DAMAGE_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (DAMAGE_TBOMB, "Torpedo Bomber"),
    (DAMAGE_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (DAMAGE_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (DAMAGE_FIRE, "Fire"),
    (DAMAGE_RAM, "Ram"),
    (DAMAGE_FLOOD, "Flood"),
    (DAMAGE_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (DAMAGE_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (DAMAGE_SEA_MINE, "Sea Mine"),
    (DAMAGE_ROCKET, "Rocket"),
    (DAMAGE_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (DAMAGE_SKIP, "Skip Bomb"),
    (DAMAGE_SKIP_ALT, "Alt Skip Bomb"),
    (DAMAGE_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (DAMAGE_WAVE, "Wave"),
    (DAMAGE_CHARGE_LASER, "Charge Laser"),
    (DAMAGE_PULSE_LASER, "Pulse Laser"),
    (DAMAGE_AXIS_LASER, "Axis Laser"),
    (DAMAGE_PHASER_LASER, "Phaser Laser"),
];

static POTENTIAL_DAMAGE_DESCRIPTIONS: [(&str, &str); 4] = [
    ("agro_art", "Artillery"),
    ("agro_tpd", "Torpedo"),
    ("agro_air", "Planes"),
    ("agro_dbomb", "Depth Charge"),
];

fn ship_class_icon_from_species(species: Species, wows_data: &WorldOfWarshipsData) -> Option<Arc<ShipIcon>> {
    wows_data.ship_icons.get(&species).cloned()
}

struct SkillInfo {
    skill_points: usize,
    num_skills: usize,
    highest_tier: usize,
    num_tier_1_skills: usize,
    hover_text: Option<String>,
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
    potential_damage_hover_text: Option<RichText>,
    potential_damage_report: Option<PotentialDamage>,
    time_lived_secs: Option<u64>,
    time_lived_text: Option<String>,
    skill_info: SkillInfo,
    received_damage: Option<u64>,
    received_damage_text: Option<RichText>,
    received_damage_hover_text: Option<RichText>,
    fires: Option<u64>,
    floods: Option<u64>,
    citadels: Option<u64>,
    crits: Option<u64>,
    distance_traveled: Option<f64>,
    is_test_ship: bool,
    is_enemy: bool,
}

impl VehicleReport {
    fn remove_nda_info(&mut self) {
        self.observed_damage = 0;
        self.observed_damage_text = "NDA".to_string();
        self.actual_damage = Some(0);
        self.actual_damage_text = Some("NDA".into());
        self.actual_damage_hover_text = None;
        self.potential_damage = Some(0);
        self.potential_damage_text = Some("NDA".into());
        self.potential_damage_hover_text = None;
        self.received_damage = Some(0);
        self.received_damage_text = Some("NDA".into());
        self.received_damage_hover_text = None;
        self.fires = Some(0);
        self.floods = Some(0);
        self.citadels = Some(0);
        self.crits = Some(0);
    }
}

use std::cmp::Reverse;

#[allow(non_camel_case_types)]
enum SortKey {
    String(String),
    i64(Option<i64>),
    u64(Option<u64>),
    f64(Option<f64>),
    Species(Species),
}

impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SortKey::String(a), SortKey::String(b)) => a == b,
            (SortKey::i64(a), SortKey::i64(b)) => a == b,
            (SortKey::u64(a), SortKey::u64(b)) => a == b,
            (SortKey::f64(a), SortKey::f64(b)) => a == b,
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
            (SortKey::f64(a), SortKey::f64(b)) => a.partial_cmp(b).expect("could not compare f64  keys?"),
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
    is_row_expanded: BTreeMap<u64, bool>,
    constants: Arc<RwLock<serde_json::Value>>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    replay_sort: Arc<Mutex<SortOrder>>,
    columns: Vec<ReplayColumn>,
    row_heights: BTreeMap<u64, f32>,
    background_task_sender: Sender<BackgroundTask>,
    selected_row: Option<(u64, bool)>,
    debug_mode: bool,
}

impl UiReport {
    fn new(
        replay_file: &ReplayFile,
        report: &BattleReport,
        constants: Arc<RwLock<serde_json::Value>>,
        wows_data: Arc<RwLock<WorldOfWarshipsData>>,
        replay_sort: Arc<Mutex<SortOrder>>,
        background_task_sender: Sender<BackgroundTask>,
        is_debug_mode: bool,
    ) -> Self {
        let wows_data_inner = wows_data.read();
        let metadata_provider = wows_data_inner.game_metadata.as_ref().expect("no game metadata?");
        let constants_inner = constants.read();

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
            let is_enemy = player.relation() > 1;
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

            let icon = ship_class_icon_from_species(vehicle_param.species().expect("ship has no species"), &wows_data_inner);

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
                let index = constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/exp")?.as_u64()? as usize;
                info.as_array()
                    .and_then(|info_array| info_array[index].as_number().and_then(|number| number.as_i64()))
            }) {
                let label_text = separate_number(base_xp, Some(locale));
                (Some(base_xp), Some(RichText::new(label_text).color(player_color)))
            } else {
                (None, None)
            };

            let (raw_xp, raw_xp_text) = if let Some(raw_xp) = vehicle.results_info().and_then(|info| {
                let index = constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/raw_exp")?.as_u64()? as usize;
                info.as_array()
                    .and_then(|info_array| info_array[index].as_number().and_then(|number| number.as_i64()))
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

            let results_info = vehicle.results_info().and_then(|info| info.as_array());

            // Actual damage done to other players
            let (damage, damage_text, damage_hover_text, damage_report) = results_info
                .and_then(|info_array| {
                    let total_damage_index = constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/damage")?.as_u64()? as usize;

                    info_array[total_damage_index].as_number().and_then(|number| number.as_u64()).map(|damage_number| {
                        // First pass over damage numbers: grab the longest description so that we can later format it
                        let longest_width = DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let idx = constants_inner.pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{}", key).as_str())?.as_u64()? as usize;
                                info_array[idx]
                                    .as_number()
                                    .and_then(|number| number.as_u64())
                                    .and_then(|num| if num > 0 { Some(description.len()) } else { None })
                            })
                            .max()
                            .unwrap_or_default()
                            + 1;

                        // Grab each damage index and format by <DAMAGE_TYPE>: <DAMAGE_NUM> as a collection of strings
                        let (all_damage, breakdowns): (Vec<(String, u64)>, Vec<String>) = DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let idx = constants_inner.pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{}", key).as_str())?.as_u64()? as usize;
                                info_array[idx].as_number().and_then(|number| number.as_u64()).and_then(|num| {
                                    if num > 0 {
                                        let num_str = separate_number(num, Some(locale));
                                        Some(((key.to_string(), num), format!("{:<longest_width$}: {}", description, num_str)))
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect();

                        let all_damage: HashMap<String, u64> = HashMap::from_iter(all_damage);

                        let damage_report_text = separate_number(damage_number, Some(locale));
                        let damage_report_text = RichText::new(damage_report_text).color(player_color);
                        let damage_report_hover_text = RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                        (
                            Some(damage_number),
                            Some(damage_report_text),
                            Some(damage_report_hover_text),
                            Some(Damage {
                                ap: all_damage.get(DAMAGE_MAIN_AP).copied(),
                                sap: all_damage.get(DAMAGE_MAIN_CS).copied(),
                                he: all_damage.get(DAMAGE_MAIN_HE).copied(),
                                he_secondaries: all_damage.get(DAMAGE_ATBA_HE).copied(),
                                sap_secondaries: all_damage.get(DAMAGE_ATBA_CS).copied(),
                                torps: all_damage.get(DAMAGE_TPD_NORMAL).copied(),
                                deep_water_torps: all_damage.get(DAMAGE_TPD_DEEP).copied(),
                                fire: all_damage.get(DAMAGE_FIRE).copied(),
                                flooding: all_damage.get(DAMAGE_FLOOD).copied(),
                            }),
                        )
                    })
                })
                .unwrap_or_default();

            // Received damage
            let (received_damage, received_damage_text, received_damage_hover_text) = results_info
                .map(|info_array| {
                    // First pass over damage numbers: grab the longest description so that we can later format it
                    let longest_width = DAMAGE_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let idx = constants_inner
                                .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/received_{}", key).as_str())?
                                .as_u64()? as usize;
                            info_array[idx]
                                .as_number()
                                .and_then(|number| number.as_u64())
                                .and_then(|num| if num > 0 { Some(description.len()) } else { None })
                        })
                        .max()
                        .unwrap_or_default()
                        + 1;

                    // Grab each damage index and format by <DAMAGE_TYPE>: <DAMAGE_NUM> as a collection of strings
                    let breakdowns: Vec<(u64, String)> = DAMAGE_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let idx = constants_inner
                                .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/received_{}", key).as_str())?
                                .as_u64()? as usize;
                            info_array[idx].as_number().and_then(|number| number.as_u64()).and_then(|num| {
                                if num > 0 {
                                    let num_str = separate_number(num, Some(locale));
                                    Some((num, format!("{:<longest_width$}: {}", description, num_str)))
                                } else {
                                    None
                                }
                            })
                        })
                        .collect();

                    let total_received = breakdowns.iter().fold(0, |total, (dmg, _)| total + *dmg);

                    let received_damage_report_text = separate_number(total_received, Some(locale));
                    let received_damage_report_text = RichText::new(received_damage_report_text).color(player_color);
                    let received_damage_report_hover_text = RichText::new(breakdowns.iter().map(|(_num, desc)| desc).join("\n")).font(FontId::monospace(12.0));

                    (Some(total_received), Some(received_damage_report_text), Some(received_damage_report_hover_text))
                })
                .unwrap_or_default();

            // Spotting damage
            let (spotting_damage, spotting_damage_text) = if let Some(damage_number) = results_info.and_then(|info_array| {
                let idx = constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/scouting_damage")?.as_u64()? as usize;
                info_array[idx].as_number().and_then(|number| number.as_u64())
            }) {
                (Some(damage_number), Some(separate_number(damage_number, Some(locale))))
            } else {
                (None, None)
            };

            let (potential_damage, potential_damage_text, potential_damage_hover_text, potential_damage_report) = results_info
                .and_then(|info_array| {
                    // First pass over damage numbers: grab the longest description so that we can later format it
                    let longest_width = POTENTIAL_DAMAGE_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let idx = constants_inner.pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{}", key).as_str())?.as_u64()? as usize;
                            info_array[idx]
                                .as_number()
                                .and_then(|number| number.as_u64().or_else(|| number.as_f64().map(|f| f as u64)))
                                .and_then(|num| if num > 0 { Some(description.len()) } else { None })
                        })
                        .max()
                        .unwrap_or_default()
                        + 1;

                    // Grab each damage index and format by <DAMAGE_TYPE>: <DAMAGE_NUM> as a collection of strings
                    let (all_agro, breakdowns): (Vec<(String, u64)>, Vec<String>) = POTENTIAL_DAMAGE_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let idx = constants_inner.pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{}", key).as_str())?.as_u64()? as usize;
                            info_array[idx]
                                .as_number()
                                .and_then(|number| number.as_u64().or_else(|| number.as_f64().map(|f| f as u64)))
                                .and_then(|num| {
                                    if num > 0 {
                                        let num_str = separate_number(num, Some(locale));
                                        Some(((key.to_string(), num), format!("{:<longest_width$}: {}", description, num_str)))
                                    } else {
                                        None
                                    }
                                })
                        })
                        .unzip();
                    let all_agro: HashMap<String, u64> = HashMap::from_iter(all_agro);

                    let total_agro = all_agro.values().sum();
                    let damage_report_text = separate_number(total_agro, Some(locale));
                    let damage_report_hover_text = RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                    Some((
                        Some(total_agro),
                        Some(damage_report_text),
                        Some(damage_report_hover_text),
                        Some(PotentialDamage {
                            artillery: all_agro.get("agro_art").copied().unwrap_or_default(),
                            torpedoes: all_agro.get("agro_tpd").copied().unwrap_or_default(),
                            planes: all_agro.get("agro_air").copied().unwrap_or_default(),
                        }),
                    ))
                })
                .unwrap_or_default();

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

            let (label, hover_text) = util::colorize_captain_points(skill_points, num_skills, highest_tier, num_tier_1_skills, vehicle.commander_skills());

            let skill_info = SkillInfo {
                skill_points,
                num_skills,
                highest_tier,
                num_tier_1_skills,
                hover_text,
                label_text: label,
            };

            let (fires, floods, cits, crits) = constants_inner
                .pointer("/CLIENT_PUBLIC_RESULTS_INDICES/interactions")
                .and_then(|interactions_idx| {
                    let mut fires = 0;
                    let mut floods = 0;
                    let mut cits = 0;
                    let mut crits = 0;

                    let interactions_idx = interactions_idx.as_u64()? as usize;
                    let dict = results_info?[interactions_idx].as_object()?;
                    for (victim, victim_interactions) in dict {
                        let victim_id: i64 = victim.parse().expect("failed to convert victim ID to name");
                        let victim_vehicle = players
                            .iter()
                            .find(|vehicle| if let Some(player) = vehicle.player() { player.db_id() == victim_id } else { false });

                        let victim_interactions = victim_interactions.as_array()?;

                        // if let Some(victim_vehicle) = victim_vehicle {
                        //     if vehicle.player().unwrap().name() != "Paulo_Rogerio1" {
                        //         continue;
                        //     }
                        //     println!(
                        //         "Damage done from {} to {}:",
                        //         vehicle.player().expect("no player").name(),
                        //         victim_vehicle.player().unwrap().name(),
                        //     );

                        //     DAMAGE_DESCRIPTIONS.iter().for_each(|(key, description)| {
                        //         if let Some(idx) = constants
                        //             .pointer("/CLIENT_VEH_INTERACTION_DETAILS")
                        //             .and_then(|arr| arr.as_array())
                        //             .and_then(|arr| arr.iter().position(|name| name.as_str().map(|name| name == *key).unwrap_or_default()))
                        //         {
                        //             if let Some(num) = victim_interactions[idx as usize].as_number().and_then(|number| number.as_u64()) {
                        //                 if num > 0 {
                        //                     let num_str = separate_number(num, Some(locale));
                        //                     println!("{}: {}", description, num_str)
                        //                 }
                        //             }
                        //         }

                        //         let hits_key = key.replace("damage", "hits");
                        //         if let Some(idx) = constants
                        //             .pointer("/CLIENT_VEH_INTERACTION_DETAILS")
                        //             .and_then(|arr| arr.as_array())
                        //             .and_then(|arr| arr.iter().position(|name| name.as_str().map(|name| name == &hits_key).unwrap_or_default()))
                        //         {
                        //             if let Some(num) = victim_interactions[idx as usize].as_number().and_then(|number| number.as_u64()) {
                        //                 if num > 0 {
                        //                     let num_str = separate_number(num, Some(locale));
                        //                     println!("Hits {}: {}", description, num_str)
                        //                 }
                        //             }
                        //         }
                        //     });
                        // }

                        fires += constants_inner
                            .pointer("/CLIENT_VEH_INTERACTION_DETAILS")?
                            .as_array()
                            .and_then(|names| names.iter().position(|name| name.as_str().map(|name| name == "fires").unwrap_or_default()))
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();

                        floods += constants_inner
                            .pointer("/CLIENT_VEH_INTERACTION_DETAILS")?
                            .as_array()
                            .and_then(|names| names.iter().position(|name| name.as_str().map(|name| name == "floods").unwrap_or_default()))
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();

                        cits += constants_inner
                            .pointer("/CLIENT_VEH_INTERACTION_DETAILS")?
                            .as_array()
                            .and_then(|names| names.iter().position(|name| name.as_str().map(|name| name == "citadels").unwrap_or_default()))
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();

                        crits += constants_inner
                            .pointer("/CLIENT_VEH_INTERACTION_DETAILS")?
                            .as_array()
                            .and_then(|names| names.iter().position(|name| name.as_str().map(|name| name == "crits").unwrap_or_default()))
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();
                    }

                    Some((Some(fires), Some(floods), Some(cits), Some(crits)))
                })
                .unwrap_or_default();

            let distance_traveled = constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/distance").and_then(|distance_idx| {
                let distance_idx = distance_idx.as_u64()? as usize;
                results_info?[distance_idx].as_f64()
            });

            let is_test_ship = vehicle_param
                .data()
                .vehicle_ref()
                .map(|vehicle| vehicle.group().starts_with("demo"))
                .unwrap_or_default();

            let report = VehicleReport {
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
                received_damage,
                received_damage_text,
                received_damage_hover_text,
                fires,
                floods,
                citadels: cits,
                crits,
                distance_traveled,
                is_test_ship,
                is_enemy,
            };

            Some(report)
        });

        let vehicle_reports = player_reports.collect();

        drop(constants_inner);
        drop(wows_data_inner);

        Self {
            match_timestamp,
            vehicle_reports,
            self_player,
            replay_sort,
            wows_data,
            constants,
            is_row_expanded: Default::default(),
            sorted: false,
            columns: vec![
                ReplayColumn::Actions,
                ReplayColumn::Name,
                ReplayColumn::ShipName,
                ReplayColumn::BaseXp,
                ReplayColumn::RawXp,
                ReplayColumn::ObservedDamage,
                ReplayColumn::ActualDamage,
                ReplayColumn::ReceivedDamage,
                ReplayColumn::PotentialDamage,
                ReplayColumn::SpottingDamage,
                ReplayColumn::TimeLived,
                ReplayColumn::Fires,
                ReplayColumn::Floods,
                ReplayColumn::Citadels,
                ReplayColumn::Crits,
                ReplayColumn::DistanceTraveled,
                ReplayColumn::Skills,
            ],
            row_heights: Default::default(),
            background_task_sender,
            selected_row: None,
            debug_mode: is_debug_mode,
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
                SortColumn::ObservedDamage => SortKey::u64(Some(if report.is_test_ship && !self.debug_mode { 0 } else { report.observed_damage })),
                SortColumn::ActualDamage => SortKey::u64(if report.is_test_ship && !self.debug_mode { None } else { report.actual_damage }),
                SortColumn::SpottingDamage => SortKey::u64(report.spotting_damage),
                SortColumn::PotentialDamage => SortKey::u64(if report.is_test_ship && !self.debug_mode { None } else { report.potential_damage }),
                SortColumn::TimeLived => SortKey::u64(report.time_lived_secs),
                SortColumn::Fires => SortKey::u64(if report.is_test_ship && !self.debug_mode { None } else { report.fires }),
                SortColumn::Floods => SortKey::u64(if report.is_test_ship && !self.debug_mode { None } else { report.floods }),
                SortColumn::Citadels => SortKey::u64(if report.is_test_ship && !self.debug_mode { None } else { report.citadels }),
                SortColumn::Crits => SortKey::u64(if report.is_test_ship && !self.debug_mode { None } else { report.crits }),
                SortColumn::ReceivedDamage => SortKey::u64(if report.is_test_ship && !self.debug_mode { None } else { report.received_damage }),
                SortColumn::DistanceTraveled => SortKey::f64(report.distance_traveled),
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

    fn update_visible_columns(&mut self, settings: &ReplaySettings) {
        let optional_columns = [
            (ReplayColumn::RawXp, settings.show_raw_xp),
            (ReplayColumn::ObservedDamage, settings.show_observed_damage),
            (ReplayColumn::Fires, settings.show_fires),
            (ReplayColumn::Floods, settings.show_floods),
            (ReplayColumn::Citadels, settings.show_citadels),
            (ReplayColumn::Crits, settings.show_crits),
            (ReplayColumn::ReceivedDamage, settings.show_received_damage),
            (ReplayColumn::DistanceTraveled, settings.show_distance_traveled),
        ];

        let mut optional_columns: HashMap<ReplayColumn, bool> = optional_columns.iter().copied().collect();

        let mut remove_columns = Vec::with_capacity(optional_columns.len());
        // For each column in our existing set, check to see if it's been disabled.
        // If so,
        for (i, column) in self.columns.iter().enumerate() {
            if optional_columns.contains_key(column) {
                if let Some(false) = optional_columns.remove(column) {
                    remove_columns.push(i);
                }
            }
        }

        // Remove columns in reverse order so that we don't invalidate indices
        for i in remove_columns.into_iter().rev() {
            self.columns.remove(i);
        }

        // The optional_columns set above is the remaining columns which are enabled,
        // but not in the existing set, or disabled and not in the existing set. Add the former.
        for (column, enabled) in optional_columns {
            if enabled {
                self.columns.push(column);
            }
        }

        // Finally, sort the remaining columns by their order in the enum.
        self.columns.sort_unstable_by_key(|column| *column as u8);
    }

    fn cell_content_ui(&mut self, row_nr: u64, col_nr: usize, ui: &mut egui::Ui) {
        let is_expanded = self.is_row_expanded.get(&row_nr).copied().unwrap_or_default();
        let expandedness = ui.ctx().animate_bool(Id::new(row_nr), is_expanded);

        let Some(report) = self.vehicle_reports.get(row_nr as usize) else {
            return;
        };

        let column = *self.columns.get(col_nr).expect("somehow ended up with zero columns?");
        let mut change_expand = false;

        let inner_response = ui.vertical(|ui| {
            ui.horizontal(|ui| {
                // The first column always has the expand/collapse button
                if col_nr == 1 {
                    let (_, response) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::click());
                    egui::collapsing_header::paint_default_icon(ui, expandedness, &response);
                    if response.clicked() {
                        change_expand = true;
                    }
                }

                match column {
                    ReplayColumn::Name => {
                        // Add ship icon
                        if let Some(icon) = report.icon.as_ref() {
                            let image = Image::new(ImageSource::Bytes {
                                uri: icon.path.clone().into(),
                                // the icon size is <1k, this clone is fairly cheap
                                bytes: icon.data.clone().into(),
                            })
                            .tint(report.color)
                            .fit_to_exact_size((20.0, 20.0).into())
                            .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));

                            ui.add(image).on_hover_text(&report.ship_species_text);
                        } else {
                            ui.label(&report.ship_species_text);
                        }

                        // Add division ID
                        if let Some(div) = report.division_label.as_ref() {
                            ui.label(div);
                        }

                        // Add player clan
                        if let Some(clan_text) = report.clan_text.clone() {
                            ui.label(clan_text);
                        }

                        // Add player name
                        ui.label(report.name_text.clone());

                        // Add icons for player properties
                        if let Some(player) = report.vehicle.player() {
                            // Hidden profile icon
                            if player.is_hidden() {
                                ui.label(icons::EYE_SLASH).on_hover_text("Player has a hidden profile");
                            }

                            // // Stream sniper icon
                            // if let Some(timestamps) = twitch_state.player_is_potential_stream_sniper(player.name(), match_timestamp) {
                            //     let hover_text = timestamps
                            //         .iter()
                            //         .map(|(name, timestamps)| {
                            //             format!(
                            //                 "Possible stream name: {}\nSeen: {} minutes after match start",
                            //                 name,
                            //                 timestamps
                            //                     .iter()
                            //                     .map(|ts| {
                            //                         let delta = ts.signed_duration_since(match_timestamp);
                            //                         delta.num_minutes()
                            //                     })
                            //                     .join(", ")
                            //             )
                            //         })
                            //         .join("\n\n");
                            //     ui.label(icons::TWITCH_LOGO).on_hover_text(hover_text);
                            // }

                            let disconnect_hover_text = if player.did_disconnect() {
                                Some("Player disconnected from the match")
                            } else {
                                None
                            };
                            if let Some(disconnect_text) = disconnect_hover_text {
                                ui.label(icons::PLUGS).on_hover_text(disconnect_text);
                            }
                        }
                    }
                    ReplayColumn::BaseXp => {
                        if let Some(base_xp_text) = report.base_xp_text.clone() {
                            ui.label(base_xp_text);
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::RawXp => {
                        if let Some(raw_xp_text) = report.raw_xp_text.clone() {
                            ui.label(raw_xp_text);
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::ShipName => {
                        ui.label(&report.ship_name);
                    }
                    ReplayColumn::ObservedDamage => {
                        if report.is_test_ship && !self.debug_mode {
                            ui.label("NDA");
                        } else {
                            ui.label(&report.observed_damage_text);
                        }
                    }
                    ReplayColumn::ActualDamage => {
                        if let Some(damage_text) = report.actual_damage_text.clone() {
                            if report.is_test_ship && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(damage_text);
                                if let Some(hover_text) = report.actual_damage_hover_text.clone() {
                                    response.on_hover_text(hover_text);
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::ReceivedDamage => {
                        if let Some(received_damage_text) = report.received_damage_text.clone() {
                            if report.is_test_ship && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(received_damage_text);
                                if let Some(hover_text) = report.received_damage_hover_text.clone() {
                                    response.on_hover_text(hover_text);
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::PotentialDamage => {
                        if let Some(damage_text) = report.potential_damage_text.clone() {
                            if report.is_test_ship && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(damage_text);
                                if let Some(hover_text) = report.potential_damage_hover_text.as_ref() {
                                    response.on_hover_text(hover_text.clone());
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::SpottingDamage => {
                        if let Some(spotting_damage_text) = report.spotting_damage_text.clone() {
                            ui.label(spotting_damage_text);
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::TimeLived => {
                        if let Some(time_lived_text) = report.time_lived_text.clone() {
                            ui.label(time_lived_text);
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Fires => {
                        if let Some(fires) = report.fires {
                            if report.is_test_ship && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(fires.to_string());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Floods => {
                        if let Some(floods) = report.floods {
                            if report.is_test_ship && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(floods.to_string());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Citadels => {
                        if let Some(citadels) = report.citadels {
                            if report.is_test_ship && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(citadels.to_string());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Crits => {
                        if let Some(crits) = report.crits {
                            if report.is_test_ship && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(crits.to_string());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::DistanceTraveled => {
                        if let Some(distance) = report.distance_traveled {
                            ui.label(format!("{:.2}km", distance));
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Skills => {
                        if report.is_enemy && !self.debug_mode {
                            ui.label("-");
                        } else {
                            let response = ui.label(report.skill_info.label_text.clone());
                            if let Some(hover_text) = &report.skill_info.hover_text {
                                response.on_hover_text(hover_text);
                            }
                        }
                    }
                    ReplayColumn::Actions => {
                        ui.menu_button(icons::DOTS_THREE, |ui| {
                            if !report.is_enemy || self.debug_mode {
                                if ui.small_button(format!("{} Open Build in Browser", icons::SHARE)).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    let url = build_ship_config_url(&report.vehicle, &metadata_provider);

                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Copy Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    let url = build_ship_config_url(&report.vehicle, &metadata_provider);
                                    ui.ctx().copy_text(url);

                                    let _ = self.background_task_sender.send(BackgroundTask {
                                        receiver: None,
                                        kind: BackgroundTaskKind::UpdateTimedMessage(TimedMessage::new(format!("{} Build link copied", icons::CHECK_CIRCLE))),
                                    });

                                    ui.close_menu();
                                }

                                if ui.small_button(format!("{} Copy Short Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    let url = build_short_ship_config_url(&report.vehicle, &metadata_provider);
                                    ui.ctx().copy_text(url);
                                    let _ = self.background_task_sender.send(BackgroundTask {
                                        receiver: None,
                                        kind: BackgroundTaskKind::UpdateTimedMessage(TimedMessage::new(format!("{} Build link copied", icons::CHECK_CIRCLE))),
                                    });

                                    ui.close_menu();
                                }

                                ui.separator();
                            }

                            if ui.small_button(format!("{} Open Tomato.gg Page", icons::SHARE)).clicked() {
                                if let Some(url) = build_tomato_gg_url(&report.vehicle) {
                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                }

                                ui.close_menu();
                            }

                            if ui.small_button(format!("{} Open WoWs Numbers Page", icons::SHARE)).clicked() {
                                if let Some(url) = build_wows_numbers_url(&report.vehicle) {
                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                }

                                ui.close_menu();
                            }

                            if self.debug_mode {
                                ui.separator();

                                if let Some(player) = report.vehicle.player() {
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

                                        self.background_task_sender
                                            .send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::OpenFileViewer(viewer),
                                            })
                                            .unwrap();

                                        ui.close_menu();
                                    }
                                }
                            }
                        });
                    }
                }
            });

            // // Entity ID (debugging)
            // if self.tab_state.settings.replay_settings.show_entity_id {
            //     ui.col(|ui| {
            //         ui.label(format!("{}", player_report.vehicle.id()));
            //     });
            // }

            // Expanded content goes here
            if 0.0 < expandedness {
                match column {
                    ReplayColumn::ActualDamage => {
                        if report.is_test_ship && !self.debug_mode {
                            ui.label("NDA");
                        } else if let Some(damage_extended_info) = report.actual_damage_hover_text.clone() {
                            ui.label(damage_extended_info);
                        }
                    }
                    ReplayColumn::PotentialDamage => {
                        if report.is_test_ship && !self.debug_mode {
                            ui.label("NDA");
                        } else if let Some(damage_extended_info) = report.potential_damage_hover_text.clone() {
                            ui.label(damage_extended_info);
                        }
                    }
                    ReplayColumn::ReceivedDamage => {
                        if report.is_test_ship && !self.debug_mode {
                            ui.label("NDA");
                        } else if let Some(damage_extended_info) = report.received_damage_hover_text.clone() {
                            ui.label(damage_extended_info);
                        }
                    }
                    ReplayColumn::Skills => {
                        if !report.is_enemy || self.debug_mode {
                            if let Some(hover_text) = &report.skill_info.hover_text {
                                ui.label(hover_text);
                            }
                        }
                    }
                    _ => {
                        // Do nothing
                    }
                }
            }
        });

        match ui.input(|i| {
            let double_clicked = i.pointer.button_double_clicked(egui::PointerButton::Primary) && ui.max_rect().contains(i.pointer.interact_pos().unwrap_or_default());
            let single_clicked =
                i.pointer.button_clicked(egui::PointerButton::Primary) && i.modifiers.ctrl && ui.max_rect().contains(i.pointer.interact_pos().unwrap_or_default());

            (double_clicked, single_clicked)
        }) {
            (true, _) => {
                // A double-click shouldn't enable row selection
                if let Some((_row, false)) = self.selected_row {
                    self.selected_row = None;
                }

                change_expand = true;
            }
            (false, true) => {
                if self.selected_row.take().filter(|prev| prev.0 == row_nr).is_none() {
                    self.selected_row = Some((row_nr, true));
                    ui.ctx().request_repaint();
                }
            }
            _ => {
                // both false
            }
        }

        if change_expand {
            // Toggle.
            // Note: we use a map instead of a set so that we can animate opening and closing of each column.
            self.is_row_expanded.insert(row_nr, !is_expanded);
            self.row_heights.remove(&row_nr);
        }

        let cell_height = inner_response.response.rect.height();
        let previous_height = self.row_heights.entry(row_nr).or_insert(cell_height);

        if *previous_height < cell_height {
            *previous_height = cell_height;
        }
    }

    fn metadata_provider(&self) -> Arc<GameMetadataProvider> {
        self.wows_data.read().game_metadata.as_ref().expect("no metadata provider?").clone()
    }
}

impl egui_table::TableDelegate for UiReport {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell_inf: &egui_table::HeaderCellInfo) {
        let egui_table::HeaderCellInfo { group_index, .. } = cell_inf;

        let margin = 4;

        egui::Frame::new().inner_margin(Margin::symmetric(margin, 0)).show(ui, |ui| {
            let column = *self.columns.get(*group_index).expect("somehow ended up with zero columns?");
            match column {
                ReplayColumn::Actions => {
                    ui.label("Actions");
                }
                ReplayColumn::Name => {
                    if ui
                        .strong(column_name_with_sort_order("Player Name", false, *self.replay_sort.lock(), SortColumn::Name))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Name);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::BaseXp => {
                    if ui
                        .strong(column_name_with_sort_order("Base XP", false, *self.replay_sort.lock(), SortColumn::BaseXp))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::BaseXp);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::RawXp => {
                    if ui
                        .strong(column_name_with_sort_order("Raw XP", false, *self.replay_sort.lock(), SortColumn::RawXp))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::RawXp);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ShipName => {
                    if ui
                        .strong(column_name_with_sort_order("Ship Name", false, *self.replay_sort.lock(), SortColumn::ShipName))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ShipName);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ObservedDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Observed Damage",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ObservedDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ObservedDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ActualDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Actual Damage",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ActualDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ActualDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::SpottingDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Spotting Damage",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::SpottingDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::SpottingDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::PotentialDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Potential Damage",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::PotentialDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::PotentialDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::TimeLived => {
                    ui.strong("Time Lived");
                }
                ReplayColumn::Fires => {
                    if ui
                        .strong(column_name_with_sort_order("Fires", false, *self.replay_sort.lock(), SortColumn::Fires))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Fires);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Floods => {
                    if ui
                        .strong(column_name_with_sort_order("Floods", false, *self.replay_sort.lock(), SortColumn::Floods))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Floods);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Citadels => {
                    if ui
                        .strong(column_name_with_sort_order("Citadels", false, *self.replay_sort.lock(), SortColumn::Citadels))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Citadels);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Crits => {
                    if ui
                        .strong(column_name_with_sort_order("Crits", false, *self.replay_sort.lock(), SortColumn::Crits))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Crits);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ReceivedDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Received Damage",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ReceivedDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ReceivedDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::DistanceTraveled => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Distance Traveled",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::DistanceTraveled,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::DistanceTraveled);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Skills => {
                    ui.strong("Skills");
                }
            }
        });
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell_info: &egui_table::CellInfo) {
        let egui_table::CellInfo { row_nr, col_nr, .. } = *cell_info;

        if self.selected_row.filter(|row| row.0 == row_nr && row.1).is_some() {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().selection.bg_fill);
        } else if row_nr % 2 == 1 {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }

        egui::Frame::new().inner_margin(Margin::symmetric(4, 4)).show(ui, |ui| {
            self.cell_content_ui(row_nr, col_nr, ui);
        });
    }

    fn row_top_offset(&self, ctx: &Context, _table_id: Id, row_nr: u64) -> f32 {
        let offset = self
            .is_row_expanded
            .range(0..row_nr)
            .map(|(expanded_row_nr, expanded)| {
                let how_expanded = ctx.animate_bool(Id::new(expanded_row_nr), *expanded);
                how_expanded * self.row_heights.get(expanded_row_nr).copied().unwrap()
            })
            .sum::<f32>()
            + row_nr as f32 * ROW_HEIGHT;

        offset
    }
}

const ROW_HEIGHT: f32 = 28.0;

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

    fn update_column(&mut self, new_column: SortColumn) -> SortOrder {
        match self {
            SortOrder::Asc(sort_column) | SortOrder::Desc(sort_column) if *sort_column == new_column => {
                self.toggle();
            }
            _ => *self = SortOrder::Desc(new_column),
        }

        self.clone()
    }

    fn column(&self) -> SortColumn {
        match self {
            SortOrder::Asc(sort_column) | SortOrder::Desc(sort_column) => *sort_column,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// All columns
pub enum ReplayColumn {
    Actions,
    Name,
    ShipName,
    Skills,
    BaseXp,
    RawXp,
    ObservedDamage,
    ActualDamage,
    ReceivedDamage,
    SpottingDamage,
    PotentialDamage,
    Fires,
    Floods,
    Citadels,
    Crits,
    DistanceTraveled,
    TimeLived,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
/// Columns which are sortable
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
    Fires,
    Floods,
    Citadels,
    Crits,
    ReceivedDamage,
    DistanceTraveled,
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
    pub fn build_ui_report(
        &mut self,
        game_constants: Arc<RwLock<serde_json::Value>>,
        wows_data: Arc<RwLock<WorldOfWarshipsData>>,
        replay_sort: Arc<Mutex<SortOrder>>,
        background_task_sender: Sender<BackgroundTask>,
        is_debug_mode: bool,
    ) {
        if let Some(battle_report) = &self.battle_report {
            self.ui_report = Some(UiReport::new(
                &self.replay_file,
                battle_report,
                game_constants,
                wows_data,
                replay_sort,
                background_task_sender,
                is_debug_mode,
            ))
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
        if !ui_report.sorted {
            let replay_sort = self.tab_state.replay_sort.lock();
            ui_report.sort_players(*replay_sort);
        }

        ui_report.update_visible_columns(&self.tab_state.settings.replay_settings);

        let mut columns = vec![egui_table::Column::new(100.0).range(10.0..=500.0).resizable(true); ui_report.columns.len()];
        let action_label_layout = ui
            .painter()
            .layout_no_wrap("Actions".to_string(), egui::FontId::default(), ui.style().visuals.text_color());
        let action_label_width = action_label_layout.rect.width() + 4.0;
        columns[ReplayColumn::Actions as usize] = egui_table::Column::new(action_label_width).resizable(false);

        let table = egui_table::Table::new()
            .id_salt("replay_player_list")
            .num_rows(ui_report.vehicle_reports.len() as u64)
            .columns(columns)
            .num_sticky_cols(3)
            .headers([egui_table::HeaderRow {
                height: 14.0f32,
                groups: Default::default(),
            }])
            .auto_size_mode(egui_table::AutoSizeMode::Never);
        table.show(ui, ui_report);
        // let table = TableBuilder::new(ui)
        //     .striped(true)
        //     .resizable(true)
        //     .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        //     .column(Column::auto().clip(true))
        //     .column(Column::initial(75.0).clip(true))
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_raw_xp {
        //             table.column(Column::initial(85.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_entity_id {
        //             table.column(Column::initial(120.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     .column(Column::initial(120.0).clip(true))
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_observed_damage {
        //             table.column(Column::initial(135.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     // Actual damage
        //     .column(Column::initial(130.0).clip(true))
        //     // Received damage
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_received_damage {
        //             table.column(Column::initial(130.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     // Potential damage
        //     .column(Column::initial(135.0).clip(true))
        //     // Spotting damage
        //     .column(Column::initial(135.0).clip(true))
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_distance_traveled {
        //             // Distance Traveled
        //             table.column(Column::initial(80.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_fires {
        //             // Fires
        //             table.column(Column::initial(80.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_floods {
        //             // Floods
        //             table.column(Column::initial(80.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_citadels {
        //             // Citadels
        //             table.column(Column::initial(80.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     .pipe(|table| {
        //         if self.tab_state.settings.replay_settings.show_crits {
        //             // Crits
        //             table.column(Column::initial(80.0).clip(true))
        //         } else {
        //             table
        //         }
        //     })
        //     // Time lived
        //     .column(Column::initial(110.0).clip(true))
        //     // Allocated skills
        //     .column(Column::initial(120.0).clip(true))
        //     // Actions
        //     .column(Column::remainder())
        //     .min_scrolled_height(0.0);

        // table
        //     .header(20.0, |mut header| {
        //         header.col(|ui| {
        //             if ui.strong(column_name_with_sort_order("Player Name", false, *replay_sort, SortColumn::Name)).clicked() {
        //                 replay_sort.update_column(SortColumn::Name);
        //                 ui_report.sort_players(*replay_sort);
        //             };
        //         });
        //         header.col(|ui| {
        //             if ui.strong(column_name_with_sort_order("Base XP", false, *replay_sort, SortColumn::BaseXp)).clicked() {
        //                 replay_sort.update_column(SortColumn::BaseXp);
        //                 ui_report.sort_players(*replay_sort);
        //             }
        //         });
        //         if self.tab_state.settings.replay_settings.show_raw_xp {
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Raw XP",true, *replay_sort, SortColumn::RawXp)).on_hover_text("Raw XP before win modifiers are applied.").clicked() {
        //                     replay_sort.update_column(SortColumn::RawXp);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });
        //         }
        //         if self.tab_state.settings.replay_settings.show_entity_id {
        //             header.col(|ui| {
        //                 ui.strong("ID");
        //             });
        //         }
        //         header.col(|ui| {
        //             if ui.strong(column_name_with_sort_order("Ship Name", false, *replay_sort, SortColumn::ShipName)).clicked() {
        //                 replay_sort.update_column(SortColumn::ShipName);
        //                 ui_report.sort_players(*replay_sort);
        //             }
        //         });
        //         if self.tab_state.settings.replay_settings.show_observed_damage {
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Observed Damage", true, *replay_sort, SortColumn::ObservedDamage)).on_hover_text(
        //                     "Observed damage reflects only damage you witnessed (i.e. victim was visible on your screen). This value may be lower than actual damage.",
        //                 ).clicked() {
        //                     replay_sort.update_column(SortColumn::ObservedDamage);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });
        //         }
        //         header.col(|ui| {
        //             if ui.strong(column_name_with_sort_order("Actual Damage", true, *replay_sort, SortColumn::ActualDamage)).on_hover_text(
        //                 "Actual damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
        //             ).clicked() {
        //                     replay_sort.update_column(SortColumn::ActualDamage);
        //                     ui_report.sort_players(*replay_sort);
        //             }
        //         });
        //         if self.tab_state.settings.replay_settings.show_received_damage{
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Recv Damage", true, *replay_sort, SortColumn::ReceivedDamage)).on_hover_text(
        //                     "Total damage received. This column may break between patches because the data format is absolute junk and undocumented.",
        //                 ).clicked() {
        //                     replay_sort.update_column(SortColumn::ReceivedDamage);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });
        //         }
        //         header.col(|ui| {
        //             if ui.strong(column_name_with_sort_order("Potential Damage", true, *replay_sort, SortColumn::PotentialDamage)).on_hover_text(
        //                 "Potential damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
        //             ).clicked() {
        //                 replay_sort.update_column(SortColumn::PotentialDamage);
        //                 ui_report.sort_players(*replay_sort);
        //             }
        //         });
        //         header.col(|ui| {
        //             if ui.strong(column_name_with_sort_order("Spotting Damage", true, *replay_sort, SortColumn::SpottingDamage)).on_hover_text(
        //                 "Spotting damage seen from battle results. May not be present in the replay file if you left the game before it ended. This column may break between patches because the data format is absolute junk and undocumented.",
        //             ).clicked() {
        //                     replay_sort.update_column(SortColumn::SpottingDamage);
        //                     ui_report.sort_players(*replay_sort);
        //             }
        //         });
        //         if self.tab_state.settings.replay_settings.show_distance_traveled {
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Dist. Traveled", true, *replay_sort, SortColumn::DistanceTraveled)).on_hover_text(
        //                     "This column may break between patches because the data format is absolute junk and undocumented.",
        //                 ).clicked() {
        //                     replay_sort.update_column(SortColumn::DistanceTraveled);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });
        //         }
        //         if self.tab_state.settings.replay_settings.show_fires {
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Fires", true, *replay_sort, SortColumn::Fires)).on_hover_text(
        //                     "This column may break between patches because the data format is absolute junk and undocumented.",
        //                 ).clicked() {
        //                     replay_sort.update_column(SortColumn::Fires);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });
        //         }
        //         if self.tab_state.settings.replay_settings.show_floods {
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Floods", true, *replay_sort, SortColumn::Floods)).on_hover_text(
        //                     "This column may break between patches because the data format is absolute junk and undocumented.",
        //                 ).clicked() {
        //                     replay_sort.update_column(SortColumn::Floods);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });

        //         }
        //         if self.tab_state.settings.replay_settings.show_citadels{
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Cits", true, *replay_sort, SortColumn::Citadels)).on_hover_text(
        //                     "This column may break between patches because the data format is absolute junk and undocumented.",
        //                 ).clicked() {
        //                     replay_sort.update_column(SortColumn::Citadels);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });
        //         }
        //         if self.tab_state.settings.replay_settings.show_crits {
        //             header.col(|ui| {
        //                 if ui.strong(column_name_with_sort_order("Crits", true, *replay_sort, SortColumn::Crits )).on_hover_text(
        //                     "Critical Module Hits. This column may break between patches because the data format is absolute junk and undocumented.",
        //                 ).clicked() {
        //                     replay_sort.update_column(SortColumn::Crits);
        //                     ui_report.sort_players(*replay_sort);
        //                 }
        //             });
        //         }
        //         header.col(|ui| {
        //             if ui.strong(column_name_with_sort_order("Time Lived", false, *replay_sort, SortColumn::TimeLived)).clicked() {
        //                 replay_sort.update_column(SortColumn::TimeLived);
        //                 ui_report.sort_players(*replay_sort);
        //             }
        //         });
        //         header.col(|ui| {
        //             ui.strong("Allocated Skills");
        //         });
        //         header.col(|ui| {
        //             ui.strong("Actions");
        //         });
        //     })
        //     .body(|mut body| {
        //         for player_report in &ui_report.vehicle_reports {

        //             body.row(30.0, |ui| {
        //                 self.build_player_row(ui_report, player_report, ui);
        //             });
        //         }
        //     });
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
        if self.tab_state.settings.replay_settings.show_raw_xp {
            ui.col(|ui| {
                if let Some(raw_xp_text) = player_report.raw_xp_text.clone() {
                    ui.label(raw_xp_text);
                } else {
                    ui.label("-");
                }
            });
        }

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

        // Total Damage Received
        if self.tab_state.settings.replay_settings.show_received_damage {
            ui.col(|ui| {
                if let Some(received_damage_text) = player_report.received_damage_text.clone() {
                    let response = ui.label(received_damage_text);
                    if let Some(hover_text) = player_report.received_damage_hover_text.clone() {
                        response.on_hover_text(hover_text);
                    }
                } else {
                    ui.label("-");
                }
            });
        }

        // Potential damage
        ui.col(|ui| {
            if let Some(damage_text) = player_report.potential_damage_text.clone() {
                let response = ui.label(damage_text);
                if let Some(hover_text) = player_report.potential_damage_hover_text.as_ref() {
                    response.on_hover_text(hover_text.clone());
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

        // Fires
        if self.tab_state.settings.replay_settings.show_distance_traveled {
            ui.col(|ui| {
                if let Some(distance) = player_report.distance_traveled {
                    ui.label(format!("{:.2}km", distance));
                } else {
                    ui.label("-");
                }
            });
        }

        // Fires
        if self.tab_state.settings.replay_settings.show_fires {
            ui.col(|ui| {
                if let Some(fires) = player_report.fires {
                    ui.label(fires.to_string());
                } else {
                    ui.label("-");
                }
            });
        }

        // Floods
        if self.tab_state.settings.replay_settings.show_floods {
            ui.col(|ui| {
                if let Some(floods) = player_report.floods {
                    ui.label(floods.to_string());
                } else {
                    ui.label("-");
                }
            });
        }

        // Cits
        if self.tab_state.settings.replay_settings.show_citadels {
            ui.col(|ui| {
                if let Some(citadels) = player_report.citadels {
                    ui.label(citadels.to_string());
                } else {
                    ui.label("-");
                }
            });
        }

        // Crits
        if self.tab_state.settings.replay_settings.show_crits {
            ui.col(|ui| {
                if let Some(crits) = player_report.crits {
                    ui.label(crits.to_string());
                } else {
                    ui.label("-");
                }
            });
        }

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
            if let Some(hover_text) = &player_report.skill_info.hover_text {
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
                    ui.ctx().copy_text(url);
                    *self.tab_state.timed_message.write() = Some(TimedMessage::new(format!("{} Build link copied", icons::CHECK_CIRCLE)));

                    ui.close_menu();
                }

                if ui.small_button(format!("{} Copy Short Build Link", icons::COPY)).clicked() {
                    let metadata_provider = self.metadata_provider().unwrap();

                    let url = build_short_ship_config_url(&player_report.vehicle, &metadata_provider);
                    ui.ctx().copy_text(url);
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
                ui.ctx().copy_text(text);
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
                if let Some(ui_report) = &replay_file.ui_report {
                    let mut team_damage = 0;
                    let mut red_team_damage = 0;

                    for vehicle_report in &ui_report.vehicle_reports {
                        if vehicle_report.is_enemy {
                            red_team_damage += vehicle_report.actual_damage.unwrap_or(0);
                        } else {
                            team_damage += vehicle_report.actual_damage.unwrap_or(0);
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

                        ui.ctx().copy_text(game_chat);

                        ui.close_menu();
                    }
                });
                if self.tab_state.settings.debug_mode && ui.button("Raw Metadata").clicked() {
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
                if self.tab_state.settings.debug_mode
                    && ui
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
                        ui_report.debug_mode = self.tab_state.settings.debug_mode;
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
                                ui.ctx().copy_text(path.to_string_lossy().into_owned());
                                ui.close_menu();
                            }
                            if ui.button("Show in File Explorer").clicked() {
                                util::open_file_explorer(&path);
                                ui.close_menu();
                            }
                        });

                        if label.double_clicked() {
                            if let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref() {
                                update_background_task!(
                                    self.tab_state.background_tasks,
                                    load_replay(
                                        Arc::clone(&self.tab_state.game_constants),
                                        Arc::clone(wows_data),
                                        replay.clone(),
                                        Arc::clone(&self.tab_state.replay_sort),
                                        self.tab_state.background_task_sender.clone(),
                                        self.tab_state.settings.debug_mode
                                    )
                                );
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
                                self.tab_state.background_tasks,
                                parse_replay(
                                    Arc::clone(&self.tab_state.game_constants),
                                    Arc::clone(wows_data),
                                    self.tab_state.settings.current_replay_path.clone(),
                                    Arc::clone(&self.tab_state.replay_sort),
                                    self.tab_state.background_task_sender.clone(),
                                    self.tab_state.settings.debug_mode
                                )
                            );
                        }
                    }
                }

                ui.checkbox(&mut self.tab_state.auto_load_latest_replay, "Autoload Latest Replay");
                ComboBox::from_id_salt("column_filters")
                    .selected_text("Column Filters")
                    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                    .show_ui(ui, |ui| {
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_game_chat, "Game Chat");
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_raw_xp, "Raw XP");
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_entity_id, "Entity ID");
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_observed_damage, "Observed Damage");
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_fires, "Fires");
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_floods, "Floods");
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_citadels, "Citadels");
                        ui.checkbox(&mut self.tab_state.settings.replay_settings.show_crits, "Critical Module Hits");
                    });
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
