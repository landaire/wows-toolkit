use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::BufWriter;
use std::io::Write;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use egui::Layout;
use egui::ScrollArea;
use rootcause::Report;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ParamData;

use crate::app::PerformanceInfo;
use crate::app::ReplayGrouping;
use crate::app::ReplaySettings;
use crate::app::TimedMessage;
use crate::icons;
use crate::replay_export::FlattenedVehicle;
use crate::replay_export::Match;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskKind;
use crate::task::ReplayExportFormat;
use crate::update_background_task;
use crate::wows_data::ShipIcon;
use crate::wows_data::WorldOfWarshipsData;
use crate::wows_data::load_replay;
use crate::wows_data::parse_replay;
use egui::Color32;
use egui::ComboBox;
use egui::Context;
use egui::FontId;
use egui::Id;
use egui::Image;
use egui::ImageSource;
use egui::Label;
use egui::Margin;
use egui::OpenUrl;
use egui::PopupCloseBehavior;
use egui::RichText;
use egui::Sense;
use egui::Separator;
use egui::Style;
use egui::TextFormat;
use egui::Tooltip;
use egui::UiKind;
use egui::Vec2;
use egui::text::LayoutJob;

use escaper::decode_html;
use jiff::Timestamp;
use parking_lot::Mutex;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;

use tracing::error;
use wows_replays::ReplayFile;
use wows_replays::VehicleInfoMeta;
use wows_replays::analyzer::AnalyzerMut;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::BattleReport;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleEntity;

use itertools::Itertools;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

use crate::app::ReplayParserTabState;
use crate::app::ToolkitTabViewer;
use crate::error::ToolkitError;
use crate::plaintext_viewer;
use crate::plaintext_viewer::FileType;
use crate::util;
use crate::util::build_ship_config_url;
use crate::util::build_short_ship_config_url;
use crate::util::build_wows_numbers_url;
use crate::util::player_color_for_team_relation;
use crate::util::separate_number;

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
// const DAMAGE_BOMB_AIRSUPPORT: &str = "damage_bomb_airsupport";
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

const HITS_MAIN_AP: &str = "hits_main_ap";
const HITS_MAIN_CS: &str = "hits_main_cs";
const HITS_MAIN_HE: &str = "hits_main_he";
const HITS_ATBA_AP: &str = "hits_atba_ap";
const HITS_ATBA_CS: &str = "hits_atba_cs";
const HITS_ATBA_HE: &str = "hits_atba_he";
const HITS_TPD_NORMAL: &str = "hits_tpd";
const HITS_BOMB: &str = "hits_bomb";
const HITS_BOMB_AVIA: &str = "hits_bomb_avia";
const HITS_BOMB_ALT: &str = "hits_bomb_alt";
const HITS_BOMB_AIRSUPPORT: &str = "hits_bomb_airsupport";
const HITS_DBOMB_AIRSUPPORT: &str = "hits_dbomb_airsupport";
const HITS_TBOMB: &str = "hits_tbomb";
const HITS_TBOMB_ALT: &str = "hits_tbomb_alt";
const HITS_TBOMB_AIRSUPPORT: &str = "hits_tbomb_airsupport";
const HITS_RAM: &str = "hits_ram";
const HITS_DBOMB_DIRECT: &str = "hits_dbomb_direct";
const HITS_DBOMB_SPLASH: &str = "hits_dbomb_splash";
const HITS_SEA_MINE: &str = "hits_sea_mine";
const HITS_ROCKET: &str = "hits_rocket";
const HITS_ROCKET_AIRSUPPORT: &str = "hits_rocket_airsupport";
const HITS_SKIP: &str = "hits_skip";
const HITS_SKIP_ALT: &str = "hits_skip_alt";
const HITS_SKIP_AIRSUPPORT: &str = "hits_skip_airsupport";
const HITS_WAVE: &str = "hits_wave";
const HITS_CHARGE_LASER: &str = "hits_charge_laser";
const HITS_PULSE_LASER: &str = "hits_pulse_laser";
const HITS_AXIS_LASER: &str = "hits_axis_laser";
const HITS_PHASER_LASER: &str = "hits_phaser_laser";

static DAMAGE_DESCRIPTIONS: [(&str, &str); 33] = [
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
    // (DAMAGE_BOMB_AIRSUPPORT, "Air Support Bomb"),
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

static HITS_DESCRIPTIONS: [(&str, &str); 29] = [
    (HITS_MAIN_AP, "AP"),
    (HITS_MAIN_CS, "SAP"),
    (HITS_MAIN_HE, "HE"),
    (HITS_ATBA_AP, "AP Sec"),
    (HITS_ATBA_CS, "SAP Sec"),
    (HITS_ATBA_HE, "HE Sec"),
    (HITS_TPD_NORMAL, "Torps"),
    (HITS_BOMB, "HE Bomb"),
    (HITS_BOMB_AVIA, "Bomb"),
    (HITS_BOMB_ALT, "Alt Bomb"),
    (HITS_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (HITS_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (HITS_TBOMB, "Torpedo Bomber"),
    (HITS_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (HITS_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (HITS_RAM, "Ram"),
    (HITS_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (HITS_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (HITS_SEA_MINE, "Sea Mine"),
    (HITS_ROCKET, "Rocket"),
    (HITS_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (HITS_SKIP, "Skip Bomb"),
    (HITS_SKIP_ALT, "Alt Skip Bomb"),
    (HITS_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (HITS_WAVE, "Wave"),
    (HITS_CHARGE_LASER, "Charge Laser"),
    (HITS_PULSE_LASER, "Pulse Laser"),
    (HITS_AXIS_LASER, "Axis Laser"),
    (HITS_PHASER_LASER, "Phaser Laser"),
];

static POTENTIAL_DAMAGE_DESCRIPTIONS: [(&str, &str); 4] =
    [("agro_art", "Artillery"), ("agro_tpd", "Torpedo"), ("agro_air", "Planes"), ("agro_dbomb", "Depth Charge")];

fn ship_class_icon_from_species(species: Species, wows_data: &WorldOfWarshipsData) -> Option<Arc<ShipIcon>> {
    wows_data.ship_icons.get(&species).cloned()
}

#[derive(Clone, Serialize)]
pub struct SkillInfo {
    pub skill_points: usize,
    pub num_skills: usize,
    pub highest_tier: usize,
    pub num_tier_1_skills: usize,
    #[serde(skip)]
    hover_text: Option<String>,
    #[serde(skip)]
    label_text: RichText,
}

#[derive(Clone, Serialize)]
pub struct Damage {
    pub ap: Option<u64>,
    pub sap: Option<u64>,
    pub he: Option<u64>,
    pub he_secondaries: Option<u64>,
    pub sap_secondaries: Option<u64>,
    pub torps: Option<u64>,
    pub deep_water_torps: Option<u64>,
    pub fire: Option<u64>,
    pub flooding: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct Hits {
    pub ap: Option<u64>,
    pub sap: Option<u64>,
    pub he: Option<u64>,
    pub he_secondaries: Option<u64>,
    pub sap_secondaries: Option<u64>,
    pub torps: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct PotentialDamage {
    pub artillery: u64,
    pub torpedoes: u64,
    pub planes: u64,
}

#[derive(Clone, Serialize)]
pub struct TranslatedAbility {
    pub name: Option<String>,
    pub game_params_name: String,
}

#[derive(Clone, Serialize)]
pub struct TranslatedModule {
    pub name: Option<String>,
    pub description: Option<String>,
    pub game_params_name: String,
}

#[derive(Clone, Serialize)]
pub struct TranslatedBuild {
    pub modules: Vec<TranslatedModule>,
    pub abilities: Vec<TranslatedAbility>,
    pub captain_skills: Option<Vec<TranslatedCrewSkill>>,
}

impl TranslatedBuild {
    pub fn new(player: &Player, metadata_provider: &GameMetadataProvider) -> Option<Self> {
        let vehicle_entity = player.vehicle_entity()?;
        let config = vehicle_entity.props().ship_config();
        let species = player.vehicle().species()?;
        let result = Self {
            modules: config
                .modernization()
                .iter()
                .filter_map(|id| {
                    let game_params_name =
                        <GameMetadataProvider as GameParamProvider>::game_param_by_id(metadata_provider, *id)?
                            .name()
                            .to_string();
                    let translation_id = format!("IDS_TITLE_{}", game_params_name.to_uppercase());
                    let name = metadata_provider.localized_name_from_id(&translation_id);

                    let translation_id = format!("IDS_DESC_{}", game_params_name.to_uppercase());
                    let description = metadata_provider
                        .localized_name_from_id(&translation_id)
                        .and_then(|desc| if desc.is_empty() || desc == " " { None } else { Some(desc) });

                    Some(TranslatedModule { name, description, game_params_name })
                })
                .collect(),
            abilities: config
                .abilities()
                .iter()
                .filter_map(|id| {
                    let game_params_name =
                        <GameMetadataProvider as GameParamProvider>::game_param_by_id(metadata_provider, *id)?
                            .name()
                            .to_string();

                    let translation_id = format!("IDS_DOCK_CONSUME_TITLE_{}", game_params_name.to_uppercase());
                    let name = metadata_provider.localized_name_from_id(&translation_id);

                    Some(TranslatedAbility { name, game_params_name })
                })
                .collect(),
            captain_skills: vehicle_entity.commander_skills(species.clone()).map(|skills| {
                let mut skills: Vec<TranslatedCrewSkill> = skills
                    .iter()
                    .filter_map(|skill| Some(TranslatedCrewSkill::new(skill, species.clone(), metadata_provider)))
                    .collect();

                skills.sort_by_key(|skill| skill.tier);

                skills
            }),
        };

        Some(result)
    }
}

#[derive(Clone, Serialize)]
pub struct TranslatedCrewSkill {
    pub tier: usize,
    pub name: Option<String>,
    pub description: Option<String>,
    pub internal_name: String,
}

impl TranslatedCrewSkill {
    fn new(skill: &CrewSkill, species: Species, metadata_provider: &GameMetadataProvider) -> Self {
        Self {
            tier: skill.tier().get_for_species(species),
            name: skill.translated_name(metadata_provider),
            description: skill.translated_description(metadata_provider),
            internal_name: skill.internal_name().to_string(),
        }
    }
}

#[derive(Debug, Default)]
pub struct DamageInteraction {
    damage_dealt: u64,
    damage_dealt_text: String,
    damage_dealt_percentage: f64,
    damage_dealt_percentage_text: String,
    damage_received: u64,
    damage_received_text: String,
    damage_received_percentage: f64,
    damage_received_percentage_text: String,
}

impl DamageInteraction {
    pub fn damage_dealt(&self) -> u64 {
        self.damage_dealt
    }

    pub fn damage_dealt_percentage(&self) -> f64 {
        self.damage_dealt_percentage
    }

    pub fn damage_received(&self) -> u64 {
        self.damage_received
    }

    pub fn damage_received_percentage(&self) -> f64 {
        self.damage_received_percentage
    }
}

#[derive(Clone)]
struct Achievement {
    game_param: Arc<Param>,
    display_name: String,
    description: String,
    count: usize,
}

pub struct PlayerReport {
    player: Arc<Player>,
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
    hits: Option<u64>,
    hits_report: Option<Hits>,
    hits_text: Option<RichText>,
    /// RichText to support monospace font
    hits_hover_text: Option<RichText>,
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
    received_damage_report: Option<Damage>,
    damage_interactions: Option<HashMap<i64, DamageInteraction>>,
    fires: Option<u64>,
    floods: Option<u64>,
    citadels: Option<u64>,
    crits: Option<u64>,
    distance_traveled: Option<f64>,
    is_test_ship: bool,
    is_enemy: bool,
    is_self: bool,
    manual_stat_hide_toggle: bool,
    // TODO: Maybe in the future refactor this to be a HashMap<Rc<Player>, DeathInfo> ?
    kills: Option<i64>,
    observed_kills: i64,
    translated_build: Option<TranslatedBuild>,
    achievements: Vec<Achievement>,
    personal_rating: Option<crate::personal_rating::PersonalRatingResult>,
}

#[allow(dead_code)]
impl PlayerReport {
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

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn vehicle(&self) -> Option<&VehicleEntity> {
        self.player.vehicle_entity()
    }

    pub fn color(&self) -> Color32 {
        self.color
    }

    pub fn name_text(&self) -> &RichText {
        &self.name_text
    }

    pub fn clan_text(&self) -> Option<&RichText> {
        self.clan_text.as_ref()
    }

    pub fn ship_species_text(&self) -> &str {
        &self.ship_species_text
    }

    pub fn icon(&self) -> Option<Arc<ShipIcon>> {
        self.icon.clone()
    }

    pub fn division_label(&self) -> Option<&String> {
        self.division_label.as_ref()
    }

    pub fn base_xp(&self) -> Option<i64> {
        self.base_xp
    }

    pub fn base_xp_text(&self) -> Option<&RichText> {
        self.base_xp_text.as_ref()
    }

    pub fn raw_xp(&self) -> Option<i64> {
        self.raw_xp
    }

    pub fn raw_xp_text(&self) -> Option<&String> {
        self.raw_xp_text.as_ref()
    }

    pub fn observed_damage(&self) -> u64 {
        self.observed_damage
    }

    pub fn observed_damage_text(&self) -> &str {
        &self.observed_damage_text
    }

    pub fn actual_damage(&self) -> Option<u64> {
        self.actual_damage
    }

    pub fn actual_damage_report(&self) -> Option<&Damage> {
        self.actual_damage_report.as_ref()
    }

    pub fn actual_damage_text(&self) -> Option<&RichText> {
        self.actual_damage_text.as_ref()
    }

    pub fn actual_damage_hover_text(&self) -> Option<&RichText> {
        self.actual_damage_hover_text.as_ref()
    }

    pub fn ship_name(&self) -> &str {
        &self.ship_name
    }

    pub fn spotting_damage(&self) -> Option<u64> {
        self.spotting_damage
    }

    pub fn spotting_damage_text(&self) -> Option<&String> {
        self.spotting_damage_text.as_ref()
    }

    pub fn potential_damage(&self) -> Option<u64> {
        self.potential_damage
    }

    pub fn potential_damage_text(&self) -> Option<&String> {
        self.potential_damage_text.as_ref()
    }

    pub fn potential_damage_hover_text(&self) -> Option<&RichText> {
        self.potential_damage_hover_text.as_ref()
    }

    pub fn potential_damage_report(&self) -> Option<&PotentialDamage> {
        self.potential_damage_report.as_ref()
    }

    pub fn time_lived_secs(&self) -> Option<u64> {
        self.time_lived_secs
    }

    pub fn time_lived_text(&self) -> Option<&String> {
        self.time_lived_text.as_ref()
    }

    pub fn skill_info(&self) -> &SkillInfo {
        &self.skill_info
    }

    pub fn received_damage(&self) -> Option<u64> {
        self.received_damage
    }

    pub fn received_damage_text(&self) -> Option<&RichText> {
        self.received_damage_text.as_ref()
    }

    pub fn received_damage_hover_text(&self) -> Option<&RichText> {
        self.received_damage_hover_text.as_ref()
    }

    pub fn received_damage_report(&self) -> Option<&Damage> {
        self.received_damage_report.as_ref()
    }

    pub fn fires(&self) -> Option<u64> {
        self.fires
    }

    pub fn floods(&self) -> Option<u64> {
        self.floods
    }

    pub fn citadels(&self) -> Option<u64> {
        self.citadels
    }

    pub fn crits(&self) -> Option<u64> {
        self.crits
    }

    pub fn distance_traveled(&self) -> Option<f64> {
        self.distance_traveled
    }

    pub fn is_test_ship(&self) -> bool {
        self.is_test_ship
    }

    pub fn is_enemy(&self) -> bool {
        self.is_enemy
    }

    pub fn observed_kills(&self) -> i64 {
        self.observed_kills
    }

    pub fn kills(&self) -> Option<i64> {
        self.kills
    }

    pub fn translated_build(&self) -> Option<&TranslatedBuild> {
        self.translated_build.as_ref()
    }

    pub fn should_hide_stats(&self) -> bool {
        self.manual_stat_hide_toggle || (!self.is_self && self.is_test_ship)
    }

    pub fn is_self(&self) -> bool {
        self.is_self
    }

    pub fn hits_report(&self) -> Option<&Hits> {
        self.hits_report.as_ref()
    }

    pub fn damage_interactions(&self) -> Option<&HashMap<i64, DamageInteraction>> {
        self.damage_interactions.as_ref()
    }

    pub fn personal_rating(&self) -> Option<&crate::personal_rating::PersonalRatingResult> {
        self.personal_rating.as_ref()
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
    match_timestamp: Timestamp,
    self_player: Option<Arc<Player>>,
    player_reports: Vec<PlayerReport>,
    sorted: bool,
    is_row_expanded: BTreeMap<u64, bool>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    replay_sort: Arc<Mutex<SortOrder>>,
    columns: Vec<ReplayColumn>,
    row_heights: BTreeMap<u64, f32>,
    background_task_sender: Option<Sender<BackgroundTask>>,
    selected_row: Option<(u64, bool)>,
    debug_mode: bool,
    battle_result: Option<BattleResult>,
}

impl UiReport {
    pub fn new(
        replay_file: &ReplayFile,
        report: &BattleReport,
        constants: Arc<RwLock<serde_json::Value>>,
        wows_data: Arc<RwLock<WorldOfWarshipsData>>,
        replay_sort: Arc<Mutex<SortOrder>>,
        background_task_sender: Option<Sender<BackgroundTask>>,
        is_debug_mode: bool,
    ) -> Self {
        let wows_data_inner = wows_data.read();
        let metadata_provider = wows_data_inner.game_metadata.as_ref().expect("no game metadata?");
        let constants_inner = constants.read();

        let match_timestamp = util::replay_timestamp(&replay_file.meta);

        let players = report.players().to_vec();

        let mut divisions: HashMap<u32, char> = Default::default();
        let mut remaining_div_identifiers: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().rev().collect();

        let self_player = players.iter().find(|player| player.relation() == 0).cloned();

        let battle_result = constants_inner.pointer("/COMMON_RESULTS").and_then(|common_results_names| {
            let winner_team_id_idx = common_results_names.as_array().and_then(|names| {
                names.iter().position(|name| name.as_str().map(|name| name == "winner_team_id").unwrap_or_default())
            })?;
            let battle_results: serde_json::Value = serde_json::from_str(report.battle_results()?).ok()?;

            let self_team_id = self_player.as_ref().map(|player| player.team_id())?;

            let common_list = battle_results.pointer("/commonList")?.as_array()?;
            let winning_team_id = common_list.get(winner_team_id_idx)?.as_i64()?;

            if winning_team_id == self_team_id as i64 {
                Some(BattleResult::Win(self_team_id as i8))
            } else if winning_team_id >= 0 {
                Some(BattleResult::Loss(winning_team_id as i8))
            } else {
                Some(BattleResult::Draw)
            }
        });

        let locale = "en-US";

        let player_reports = players.iter().filter_map(|player| {
            // Get the VehicleEntity for this player
            let vehicle = player.vehicle_entity()?;
            let is_enemy = player.relation() > 1;
            let mut player_color = player_color_for_team_relation(player.relation());

            if let Some(self_player) = self_player.as_ref()
                && self_player.db_id() != player.db_id()
                && self_player.division_id() > 0
                && player.division_id() == self_player.division_id()
            {
                player_color = Color32::GOLD;
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

            let icon =
                ship_class_icon_from_species(vehicle_param.species().expect("ship has no species"), &wows_data_inner);

            let name_color = if player.is_abuser() {
                Color32::from_rgb(0xFF, 0xC0, 0xCB) // pink
            } else {
                player_color
            };

            // Assign division
            let div = player.division_id();
            let division_char = if div > 0 {
                Some(*divisions.entry(div).or_insert_with(|| remaining_div_identifiers.pop().unwrap_or('?')))
            } else {
                None
            };

            let div_text = division_char.map(|div| format!("({div})"));

            let clan_text = if !player.clan().is_empty() {
                Some(RichText::new(format!("[{}]", player.clan())).color(clan_color_for_player(player).unwrap()))
            } else {
                None
            };
            let name_text = RichText::new(player.name()).color(name_color);

            let (base_xp, base_xp_text) = if let Some(base_xp) = vehicle.results_info().and_then(|info| {
                let index = constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/exp")?.as_u64()? as usize;
                info.as_array().and_then(|info_array| info_array[index].as_number().and_then(|number| number.as_i64()))
            }) {
                let label_text = separate_number(base_xp, Some(locale));
                (Some(base_xp), Some(RichText::new(label_text).color(player_color)))
            } else {
                (None, None)
            };

            let (raw_xp, raw_xp_text) = if let Some(raw_xp) = vehicle.results_info().and_then(|info| {
                let index = constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/raw_exp")?.as_u64()? as usize;
                info.as_array().and_then(|info_array| info_array[index].as_number().and_then(|number| number.as_i64()))
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
                    let total_damage_index =
                        constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/damage")?.as_u64()? as usize;

                    info_array[total_damage_index].as_number().and_then(|number| number.as_u64()).map(|damage_number| {
                        // First pass over damage numbers: grab the longest description so that we can later format it
                        let longest_width = DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let idx = constants_inner
                                    .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{key}").as_str())?
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
                        let (all_damage, breakdowns): (Vec<(String, u64)>, Vec<String>) = DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let idx = constants_inner
                                    .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{key}").as_str())?
                                    .as_u64()? as usize;
                                info_array[idx].as_number().and_then(|number| number.as_u64()).and_then(|num| {
                                    if num > 0 {
                                        let num_str = separate_number(num, Some(locale));
                                        Some((
                                            (key.to_string(), num),
                                            format!("{description:<longest_width$}: {num_str}"),
                                        ))
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect();

                        let all_damage: HashMap<String, u64> = HashMap::from_iter(all_damage);

                        let damage_report_text = separate_number(damage_number, Some(locale));
                        let damage_report_text = RichText::new(damage_report_text).color(player_color);
                        let damage_report_hover_text =
                            RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

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

            // Armament hit information
            let (hits, hits_text, hits_hover_text, hits_report) = results_info
                .map(|info_array| {
                    // First pass over damage numbers: grab the longest description so that we can later format it
                    let longest_width = HITS_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let idx = constants_inner
                                .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{key}").as_str())?
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
                    let (all_hits, breakdowns): (Vec<(String, u64)>, Vec<String>) = HITS_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let idx = constants_inner
                                .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{key}").as_str())?
                                .as_u64()? as usize;
                            info_array[idx].as_number().and_then(|number| number.as_u64()).and_then(|num| {
                                if num > 0 {
                                    let num_str = separate_number(num, Some(locale));
                                    Some(((key.to_string(), num), format!("{description:<longest_width$}: {num_str}")))
                                } else {
                                    None
                                }
                            })
                        })
                        .collect();

                    let all_hits: HashMap<String, u64> = HashMap::from_iter(all_hits);

                    let main_hits = all_hits.get(HITS_MAIN_HE).copied().unwrap_or(0)
                        + all_hits.get(HITS_MAIN_CS).copied().unwrap_or(0)
                        + all_hits.get(HITS_MAIN_AP).copied().unwrap_or(0);

                    let plane_hits = all_hits.get(HITS_ROCKET).copied().unwrap_or(0)
                        + all_hits.get(HITS_ROCKET_AIRSUPPORT).copied().unwrap_or(0)
                        + all_hits.get(HITS_SKIP).copied().unwrap_or(0)
                        + all_hits.get(HITS_SKIP_ALT).copied().unwrap_or(0)
                        + all_hits.get(HITS_SKIP_AIRSUPPORT).copied().unwrap_or(0);

                    let relevant_hits_number =
                        if vehicle_param.species().map(|species| species == Species::AirCarrier).unwrap_or(false) {
                            plane_hits
                        } else {
                            main_hits
                        };

                    let main_hits_text = separate_number(relevant_hits_number, Some(locale));

                    let main_hits_text = RichText::new(main_hits_text).color(player_color);
                    let hits_hover_text = RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                    (
                        Some(relevant_hits_number),
                        Some(main_hits_text),
                        Some(hits_hover_text),
                        Some(Hits {
                            ap: all_hits.get(HITS_MAIN_AP).copied(),
                            sap: all_hits.get(HITS_MAIN_CS).copied(),
                            he: all_hits.get(HITS_MAIN_HE).copied(),
                            he_secondaries: all_hits.get(HITS_ATBA_HE).copied(),
                            sap_secondaries: all_hits.get(HITS_ATBA_CS).copied(),
                            torps: all_hits.get(HITS_TPD_NORMAL).copied(),
                        }),
                    )
                })
                .unwrap_or_default();

            // Received damage
            let (received_damage, received_damage_text, received_damage_hover_text, received_damage_report) =
                results_info
                    .map(|info_array| {
                        // First pass over damage numbers: grab the longest description so that we can later format it
                        let longest_width = DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let idx = constants_inner
                                    .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/received_{key}").as_str())?
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
                        let (all_damage, breakdowns): (Vec<(String, u64)>, Vec<String>) = DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let idx = constants_inner
                                    .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/received_{key}").as_str())?
                                    .as_u64()? as usize;
                                info_array[idx].as_number().and_then(|number| number.as_u64()).and_then(|num| {
                                    if num > 0 {
                                        let num_str = separate_number(num, Some(locale));
                                        Some((
                                            (key.to_string(), num),
                                            format!("{description:<longest_width$}: {num_str}"),
                                        ))
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect();

                        let all_damage: HashMap<String, u64> = HashMap::from_iter(all_damage);

                        let total_received = all_damage.values().fold(0, |total, dmg| total + *dmg);

                        let received_damage_report_text = separate_number(total_received, Some(locale));
                        let received_damage_report_text =
                            RichText::new(received_damage_report_text).color(player_color);
                        let received_damage_report_hover_text =
                            RichText::new(breakdowns.iter().join("\n")).font(FontId::monospace(12.0));

                        (
                            Some(total_received),
                            Some(received_damage_report_text),
                            Some(received_damage_report_hover_text),
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
                    .unwrap_or_default();

            // Spotting damage
            let (spotting_damage, spotting_damage_text) = if let Some(damage_number) =
                results_info.and_then(|info_array| {
                    let idx =
                        constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/scouting_damage")?.as_u64()? as usize;
                    info_array[idx].as_number().and_then(|number| number.as_u64())
                }) {
                (Some(damage_number), Some(separate_number(damage_number, Some(locale))))
            } else {
                (None, None)
            };

            let (potential_damage, potential_damage_text, potential_damage_hover_text, potential_damage_report) =
                results_info
                    .map(|info_array| {
                        // First pass over damage numbers: grab the longest description so that we can later format it
                        let longest_width = POTENTIAL_DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let idx = constants_inner
                                    .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{key}").as_str())?
                                    .as_u64()? as usize;
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
                                let idx = constants_inner
                                    .pointer(format!("/CLIENT_PUBLIC_RESULTS_INDICES/{key}").as_str())?
                                    .as_u64()? as usize;
                                info_array[idx]
                                    .as_number()
                                    .and_then(|number| number.as_u64().or_else(|| number.as_f64().map(|f| f as u64)))
                                    .and_then(|num| {
                                        if num > 0 {
                                            let num_str = separate_number(num, Some(locale));
                                            Some((
                                                (key.to_string(), num),
                                                format!("{description:<longest_width$}: {num_str}"),
                                            ))
                                        } else {
                                            None
                                        }
                                    })
                            })
                            .unzip();
                        let all_agro: HashMap<String, u64> = HashMap::from_iter(all_agro);

                        let total_agro = all_agro.values().sum();
                        let damage_report_text = separate_number(total_agro, Some(locale));
                        let damage_report_hover_text =
                            RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                        (
                            Some(total_agro),
                            Some(damage_report_text),
                            Some(damage_report_hover_text),
                            Some(PotentialDamage {
                                artillery: all_agro.get("agro_art").copied().unwrap_or_default(),
                                torpedoes: all_agro.get("agro_tpd").copied().unwrap_or_default(),
                                planes: all_agro.get("agro_air").copied().unwrap_or_default(),
                            }),
                        )
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
                .commander_skills(species.clone())
                .map(|skills| {
                    let points = skills
                        .iter()
                        .fold(0usize, |accum, skill| accum + skill.tier().get_for_species(species.clone()));
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

            let (label, hover_text) = util::colorize_captain_points(
                skill_points,
                num_skills,
                highest_tier,
                num_tier_1_skills,
                vehicle.commander_skills(species.clone()),
            );

            let skill_info =
                SkillInfo { skill_points, num_skills, highest_tier, num_tier_1_skills, hover_text, label_text: label };

            let (damage_interactions, fires, floods, cits, crits) = constants_inner
                .pointer("/CLIENT_PUBLIC_RESULTS_INDICES/interactions")
                .and_then(|interactions_idx| {
                    let mut damage_interactions = HashMap::new();
                    let mut fires = 0;
                    let mut floods = 0;
                    let mut cits = 0;
                    let mut crits = 0;

                    let interactions_idx = interactions_idx.as_u64()? as usize;
                    let dict = results_info?[interactions_idx].as_object()?;
                    for (victim, victim_interactions) in dict {
                        // Not sure if this can ever fail, but we can report wrong info to a "nobody" player
                        // or something
                        let victim_id: i64 = victim.parse().unwrap_or_default();
                        let vehicle_interaction_details_idx =
                            constants_inner.pointer("/CLIENT_VEH_INTERACTION_DETAILS")?.as_array();

                        fires += vehicle_interaction_details_idx
                            .and_then(|names| {
                                names
                                    .iter()
                                    .position(|name| name.as_str().map(|name| name == "fires").unwrap_or_default())
                            })
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();

                        floods += vehicle_interaction_details_idx
                            .and_then(|names| {
                                names
                                    .iter()
                                    .position(|name| name.as_str().map(|name| name == "floods").unwrap_or_default())
                            })
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();

                        cits += vehicle_interaction_details_idx
                            .and_then(|names| {
                                names
                                    .iter()
                                    .position(|name| name.as_str().map(|name| name == "citadels").unwrap_or_default())
                            })
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();

                        crits += vehicle_interaction_details_idx
                            .and_then(|names| {
                                names
                                    .iter()
                                    .position(|name| name.as_str().map(|name| name == "crits").unwrap_or_default())
                            })
                            .and_then(|idx| victim_interactions[idx].as_u64())
                            .unwrap_or_default();

                        // Add up all of the damage dealt
                        let mut damage_interaction = DamageInteraction::default();

                        // Grab each damage index and format by <DAMAGE_TYPE>: <DAMAGE_NUM> as a collection of strings
                        let all_damage: u64 = DAMAGE_DESCRIPTIONS.iter().fold(0, |accum, (key, _description)| {
                            let damage = vehicle_interaction_details_idx
                                .and_then(|names| {
                                    names
                                        .iter()
                                        .position(|name| name.as_str().map(|name| name == *key).unwrap_or_default())
                                })
                                .and_then(|idx| victim_interactions[idx].as_u64())
                                .unwrap_or_default();

                            damage + accum
                        });

                        damage_interaction.damage_dealt = all_damage;
                        if damage_interaction.damage_dealt > 0 {
                            damage_interaction.damage_dealt_text =
                                separate_number(damage_interaction.damage_dealt, Some(locale));

                            if let Some(total_damage) = damage {
                                damage_interaction.damage_dealt_percentage =
                                    (all_damage as f64 / total_damage as f64) * 100.0;
                                damage_interaction.damage_dealt_percentage_text =
                                    format!("{:.0}%", damage_interaction.damage_dealt_percentage);
                            }
                        }

                        damage_interactions.insert(victim_id, damage_interaction);
                    }

                    Some((Some(damage_interactions), Some(fires), Some(floods), Some(cits), Some(crits)))
                })
                .unwrap_or_default();

            let distance_traveled =
                constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/distance").and_then(|distance_idx| {
                    let distance_idx = distance_idx.as_u64()? as usize;
                    results_info?[distance_idx].as_f64()
                });

            let kills =
                constants_inner.pointer("/CLIENT_PUBLIC_RESULTS_INDICES/ships_killed").and_then(|distance_idx| {
                    let distance_idx = distance_idx.as_i64()? as usize;
                    results_info?[distance_idx].as_i64()
                });
            let observed_kills = vehicle.frags().len() as i64;

            let is_test_ship = vehicle_param
                .data()
                .vehicle_ref()
                .map(|vehicle| vehicle.group().starts_with("demo"))
                .unwrap_or_default();

            let achievements = constants_inner
                .pointer("/CLIENT_PUBLIC_RESULTS_INDICES/achievements")
                .and_then(|achievements_idx| {
                    let achievements_idx = achievements_idx.as_u64()? as usize;
                    let achievements_array = results_info?[achievements_idx].as_array()?;

                    let achievements = achievements_array
                        .iter()
                        .filter_map(|achievement_info| {
                            let achievement_info = achievement_info.as_array()?;
                            let achievement_id = achievement_info[0].as_u64()?;
                            let achievement_count = achievement_info[1].as_u64()?;

                            // Look this achievement up from game params
                            let game_param = <GameMetadataProvider as GameParamProvider>::game_param_by_id(
                                metadata_provider,
                                achievement_id as u32,
                            )?;

                            let ParamData::Achievement(achievement_data) = game_param.data() else {
                                return None;
                            };

                            let Some(achievement_name) = metadata_provider
                                .localized_name_from_id(&format!("IDS_ACHIEVEMENT_{}", achievement_data.ui_name()))
                            else {
                                return None;
                            };

                            let Some(achievement_description) = metadata_provider.localized_name_from_id(&format!(
                                "IDS_ACHIEVEMENT_DESCRIPTION_{}",
                                achievement_data.ui_name()
                            )) else {
                                return None;
                            };

                            Some(Achievement {
                                game_param,
                                display_name: achievement_name,
                                description: achievement_description,
                                count: achievement_count as usize,
                            })
                        })
                        .collect::<Vec<_>>();

                    Some(achievements)
                })
                .unwrap_or_default();

            let report = PlayerReport {
                player: Arc::clone(player),
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
                is_self: player.relation() == 0,
                manual_stat_hide_toggle: false,
                received_damage_report,
                kills,
                observed_kills,
                translated_build: TranslatedBuild::new(player, metadata_provider),
                hits,
                hits_report,
                hits_text,
                hits_hover_text,
                damage_interactions,
                achievements,
                personal_rating: None,
            };

            Some(report)
        });

        let mut player_reports: Vec<PlayerReport> = player_reports.collect();
        let mut all_received_damages = HashMap::new();

        // For each player report, we need to update the damage interactions so they
        // have the correct received damage
        for report in &player_reports {
            let mut received_damages = HashMap::new();
            let Some(damage_interactions) = report.damage_interactions.as_ref() else {
                continue;
            };

            let this_player = report.player();

            for player_id in damage_interactions.keys() {
                let Some(other_player) = player_reports.iter().find(|report| report.player().db_id() == *player_id)
                else {
                    continue;
                };

                if let Some(interactions) = other_player.damage_interactions.as_ref() {
                    let Some(interaction_with_this_player) = interactions.get(&this_player.db_id()) else {
                        continue;
                    };

                    received_damages.insert(
                        *player_id,
                        (
                            interaction_with_this_player.damage_dealt,
                            interaction_with_this_player.damage_dealt_text.clone(),
                        ),
                    );
                }
            }

            all_received_damages.insert(this_player.db_id(), received_damages);
        }

        for report in &mut player_reports {
            let total_received_damage = report.received_damage().unwrap_or_default();
            let this_player = report.player();

            let Some(this_player_received_damages) = all_received_damages.remove(&this_player.db_id()) else {
                continue;
            };

            let Some(interaction_report) = report.damage_interactions.as_mut() else {
                continue;
            };

            for (interacted_player_id, (received_damage, received_damage_text)) in this_player_received_damages {
                let interacted_player = interaction_report.entry(interacted_player_id).or_default();
                interacted_player.damage_received = received_damage;
                interacted_player.damage_received_text = received_damage_text;

                // This should never happen I think?
                if total_received_damage > 0 {
                    interacted_player.damage_received_percentage =
                        (received_damage as f64 / total_received_damage as f64) * 100.0;
                    interacted_player.damage_received_percentage_text =
                        format!("{:.0}%", interacted_player.damage_received_percentage);
                }
            }
        }

        drop(constants_inner);
        drop(wows_data_inner);

        Self {
            match_timestamp,
            player_reports,
            self_player,
            replay_sort,
            wows_data,
            battle_result,
            is_row_expanded: Default::default(),
            sorted: false,
            columns: vec![
                ReplayColumn::Actions,
                ReplayColumn::Name,
                ReplayColumn::ShipName,
                ReplayColumn::PersonalRating,
                ReplayColumn::BaseXp,
                ReplayColumn::RawXp,
                ReplayColumn::Kills,
                ReplayColumn::ObservedDamage,
                ReplayColumn::ActualDamage,
                ReplayColumn::Hits,
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
        let self_player_team_id = self.self_player.as_ref().expect("no self player?").team_id();

        let sort_key = |report: &PlayerReport, column: &SortColumn| {
            let player = report.player();
            let team_id = player.team_id() != self_player_team_id;
            let db_id = player.db_id();

            let key = match column {
                SortColumn::Name => SortKey::String(player.name().to_string()),
                SortColumn::BaseXp => SortKey::i64(report.base_xp),
                SortColumn::RawXp => SortKey::i64(report.raw_xp),
                SortColumn::ShipName => SortKey::String(report.ship_name.clone()),
                SortColumn::ShipClass => SortKey::Species(player.vehicle().species().expect("no species for vehicle?")),
                SortColumn::ObservedDamage => SortKey::u64(Some(if report.should_hide_stats() && !self.debug_mode {
                    0
                } else {
                    report.observed_damage
                })),
                SortColumn::ActualDamage => SortKey::u64(if report.should_hide_stats() && !self.debug_mode {
                    None
                } else {
                    report.actual_damage
                }),
                SortColumn::SpottingDamage => SortKey::u64(report.spotting_damage),
                SortColumn::PotentialDamage => SortKey::u64(if report.should_hide_stats() && !self.debug_mode {
                    None
                } else {
                    report.potential_damage
                }),
                SortColumn::TimeLived => SortKey::u64(report.time_lived_secs),
                SortColumn::Fires => {
                    SortKey::u64(if report.should_hide_stats() && !self.debug_mode { None } else { report.fires })
                }
                SortColumn::Floods => {
                    SortKey::u64(if report.should_hide_stats() && !self.debug_mode { None } else { report.floods })
                }
                SortColumn::Citadels => {
                    SortKey::u64(if report.should_hide_stats() && !self.debug_mode { None } else { report.citadels })
                }
                SortColumn::Crits => {
                    SortKey::u64(if report.should_hide_stats() && !self.debug_mode { None } else { report.crits })
                }
                SortColumn::ReceivedDamage => SortKey::u64(if report.should_hide_stats() && !self.debug_mode {
                    None
                } else {
                    report.received_damage
                }),
                SortColumn::DistanceTraveled => SortKey::f64(report.distance_traveled),
                SortColumn::Kills => SortKey::i64(report.kills.or(Some(report.observed_kills))),
                SortColumn::Hits => {
                    SortKey::u64(if report.should_hide_stats() && !self.debug_mode { None } else { report.hits })
                }
                SortColumn::PersonalRating => SortKey::f64(report.personal_rating.as_ref().map(|pr| pr.pr)),
            };

            (team_id, key, db_id)
        };

        match sort_order {
            SortOrder::Desc(column) => {
                self.player_reports.sort_unstable_by_key(|report| {
                    let key = sort_key(report, &column);
                    (key.0, Reverse(key.1), key.2)
                });
            }
            SortOrder::Asc(column) => {
                self.player_reports.sort_unstable_by_key(|report| sort_key(report, &column));
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
            if optional_columns.contains_key(column)
                && let Some(false) = optional_columns.remove(column)
            {
                remove_columns.push(i);
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

    fn received_damage_details(&self, report: &PlayerReport, ui: &mut egui::Ui) {
        let style = Style::default();

        ui.vertical(|ui| {
            if let Some(received_hover_text) = report.received_damage_hover_text() {
                ui.label(received_hover_text.clone());

                if report.damage_interactions.is_some() {
                    ui.separator();
                }
            }

            if let Some(interactions) = report.damage_interactions.as_ref() {
                // TODO: this sucks, it allocates for each sort
                for interaction in
                    interactions.iter().sorted_by(|a, b| Ord::cmp(&b.1.damage_received, &a.1.damage_received))
                {
                    if interaction.1.damage_received == 0 {
                        continue;
                    }

                    let Some(interaction_player) =
                        self.player_reports().iter().find(|report| report.player().db_id() == *interaction.0)
                    else {
                        // TODO: Handle bots?
                        continue;
                    };

                    // Build hover text with clan tag and player name
                    let mut hover_layout = LayoutJob::default();
                    if let Some(clan_text) = interaction_player.clan_text() {
                        clan_text.clone().append_to(
                            &mut hover_layout,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        );
                        hover_layout.append(" ", 0.0, Default::default());
                    }
                    interaction_player.name_text.clone().append_to(
                        &mut hover_layout,
                        &style,
                        egui::FontSelection::Default,
                        egui::Align::Center,
                    );

                    ui.label(format!(
                        "{}: {} ({})",
                        interaction_player.ship_name(),
                        interaction.1.damage_received_text,
                        &interaction.1.damage_received_percentage_text
                    ))
                    .on_hover_text(hover_layout);
                }
            };
        });
    }

    fn dealt_damage_details(&self, report: &PlayerReport, ui: &mut egui::Ui) {
        let style = Style::default();

        ui.vertical(|ui| {
            if let Some(received_hover_text) = report.actual_damage_hover_text() {
                ui.label(received_hover_text.clone());

                if report.damage_interactions.is_some() {
                    ui.separator();
                }
            }

            if let Some(interactions) = report.damage_interactions.as_ref() {
                // TODO: this sucks, it allocates for each sort
                for interaction in interactions.iter().sorted_by(|a, b| Ord::cmp(&b.1.damage_dealt, &a.1.damage_dealt))
                {
                    if interaction.1.damage_dealt == 0 {
                        continue;
                    }

                    let Some(interaction_player) =
                        self.player_reports().iter().find(|report| report.player().db_id() == *interaction.0)
                    else {
                        // In co-op, you may not have an interaction
                        continue;
                    };

                    // Build hover text with clan tag and player name
                    let mut hover_layout = LayoutJob::default();
                    if let Some(clan_text) = interaction_player.clan_text() {
                        clan_text.clone().append_to(
                            &mut hover_layout,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        );
                        hover_layout.append(" ", 0.0, Default::default());
                    }
                    interaction_player.name_text.clone().append_to(
                        &mut hover_layout,
                        &style,
                        egui::FontSelection::Default,
                        egui::Align::Center,
                    );

                    ui.label(format!(
                        "{}: {} ({})",
                        interaction_player.ship_name(),
                        interaction.1.damage_dealt_text,
                        &interaction.1.damage_dealt_percentage_text
                    ))
                    .on_hover_text(hover_layout);
                }
            };
        });
    }

    fn cell_content_ui(&mut self, row_nr: u64, col_nr: usize, ui: &mut egui::Ui) {
        let is_expanded = self.is_row_expanded.get(&row_nr).copied().unwrap_or_default();
        let expandedness = ui.ctx().animate_bool(Id::new(row_nr), is_expanded);

        let Some(report) = self.player_reports.get(row_nr as usize) else {
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
                        {
                            let player = report.player();
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

                            let disconnect_hover_text =
                                if player.did_disconnect() { Some("Player disconnected from the match") } else { None };
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
                    ReplayColumn::Kills => {
                        if let Some(kills) = report.kills {
                            ui.label(kills.to_string());
                        } else {
                            ui.label(report.observed_kills.to_string());
                        }
                    }
                    ReplayColumn::ObservedDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else {
                            ui.label(&report.observed_damage_text);
                        }
                    }
                    ReplayColumn::ActualDamage => {
                        if let Some(damage_text) = report.actual_damage_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(damage_text);
                                if report.actual_damage_hover_text().is_some() || report.damage_interactions.is_some() {
                                    let tooltip = Tooltip::for_enabled(&response);
                                    tooltip.show(|ui| {
                                        self.dealt_damage_details(report, ui);
                                    });
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::ReceivedDamage => {
                        if let Some(received_damage_text) = report.received_damage_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(received_damage_text);
                                if report.received_damage_hover_text().is_some() || report.damage_interactions.is_some()
                                {
                                    let tooltip = Tooltip::for_enabled(&response);
                                    tooltip.show(|ui| {
                                        self.received_damage_details(report, ui);
                                    });
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::PotentialDamage => {
                        if let Some(damage_text) = report.potential_damage_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
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
                            if report.should_hide_stats() && !self.debug_mode {
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
                            if report.should_hide_stats() && !self.debug_mode {
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
                            if report.should_hide_stats() && !self.debug_mode {
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
                            if report.should_hide_stats() && !self.debug_mode {
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
                            ui.label(format!("{distance:.2}km"));
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
                    ReplayColumn::PersonalRating => {
                        if let Some(pr) = report.personal_rating.as_ref() {
                            ui.label(RichText::new(format!("{:.0}", pr.pr)).color(pr.category.color()))
                                .on_hover_text(pr.category.name());
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Actions => {
                        ui.menu_button(icons::DOTS_THREE, |ui| {
                            if !report.is_enemy || self.debug_mode {
                                if ui.small_button(format!("{} Open Build in Browser", icons::SHARE)).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_ship_config_url(report.player(), &metadata_provider) {
                                        ui.ctx().open_url(OpenUrl::new_tab(url));
                                    }
                                    ui.close_kind(UiKind::Menu);
                                }

                                if ui.small_button(format!("{} Copy Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_ship_config_url(report.player(), &metadata_provider) {
                                        ui.ctx().copy_text(url);

                                        let _ = self.background_task_sender.as_ref().map(|sender| {
                                            sender.send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::UpdateTimedMessage(TimedMessage::new(
                                                    format!("{} Build link copied", icons::CHECK_CIRCLE),
                                                )),
                                            })
                                        });
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }

                                if ui.small_button(format!("{} Copy Short Build Link", icons::COPY)).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_short_ship_config_url(report.player(), &metadata_provider)
                                    {
                                        ui.ctx().copy_text(url);
                                        let _ = self.background_task_sender.as_ref().map(|sender| {
                                            sender.send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::UpdateTimedMessage(TimedMessage::new(
                                                    format!("{} Build link copied", icons::CHECK_CIRCLE),
                                                )),
                                            })
                                        });
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }

                                ui.separator();
                            }

                            if ui.small_button(format!("{} Open WoWs Numbers Page", icons::SHARE)).clicked() {
                                if let Some(url) = build_wows_numbers_url(report.player()) {
                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                }

                                ui.close_kind(UiKind::Menu);
                            }

                            if self.debug_mode {
                                ui.separator();

                                if let Some(player) = Some(report.player())
                                    && ui.small_button(format!("{} View Raw Player Metadata", icons::BUG)).clicked()
                                {
                                    let pretty_meta =
                                        serde_json::to_string_pretty(player).expect("failed to serialize player");
                                    let viewer = plaintext_viewer::PlaintextFileViewer {
                                        title: Arc::new("metadata.json".to_owned()),
                                        file_info: Arc::new(egui::mutex::Mutex::new(FileType::PlainTextFile {
                                            ext: ".json".to_owned(),
                                            contents: pretty_meta,
                                        })),
                                        open: Arc::new(AtomicBool::new(true)),
                                    };

                                    if let Some(sender) = self.background_task_sender.as_ref() {
                                        sender
                                            .send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::OpenFileViewer(viewer),
                                            })
                                            .expect("failed to send file viewer task")
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }
                            }
                        });
                    }
                    ReplayColumn::Hits => {
                        if let Some(hits_text) = report.hits_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(hits_text);
                                if let Some(hover_text) = report.hits_hover_text.clone() {
                                    response.on_hover_text(hover_text);
                                }
                            }
                        } else {
                            ui.label("-");
                        }
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
                    ReplayColumn::Name => {
                        ui.vertical(|ui| {
                            if !report.achievements.is_empty() {
                                ui.strong("Achievements");
                            }

                            for achievement in &report.achievements {
                                let response = if achievement.count > 1 {
                                    ui.label(format!("{} ({}x)", &achievement.display_name, achievement.count))
                                } else {
                                    ui.label(&achievement.display_name)
                                };
                                response.on_hover_text(&achievement.description);
                            }
                        });
                    }
                    ReplayColumn::ActualDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else if report.actual_damage_hover_text().is_some() || report.damage_interactions.is_some() {
                            self.dealt_damage_details(report, ui);
                        }
                    }
                    ReplayColumn::PotentialDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else if let Some(damage_extended_info) = report.potential_damage_hover_text.clone() {
                            ui.label(damage_extended_info);
                        }
                    }
                    ReplayColumn::ReceivedDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else if report.received_damage_hover_text.is_some() || report.damage_interactions.is_some() {
                            self.received_damage_details(report, ui);
                        }
                    }
                    ReplayColumn::Skills => {
                        if !report.is_enemy || self.debug_mode {
                            ui.vertical(|ui| {
                                if let Some(hover_text) = &report.skill_info.hover_text {
                                    ui.label(hover_text);
                                }
                                if let Some(build_info) = &report.translated_build {
                                    ui.separator();

                                    if build_info.modules.is_empty() {
                                        ui.label("No Modules");
                                    } else {
                                        ui.label("Modules:");
                                        for module in &build_info.modules {
                                            if let Some(name) = &module.name {
                                                let label = ui.label(name);
                                                if let Some(hover_text) = module.description.as_ref() {
                                                    label.on_hover_text(hover_text);
                                                }
                                            }
                                        }
                                    }

                                    ui.separator();

                                    if build_info.abilities.is_empty() {
                                        ui.label("No Abilities");
                                    } else {
                                        ui.label("Abilities:");
                                        for ability in &build_info.abilities {
                                            if let Some(name) = &ability.name {
                                                ui.label(name);
                                            }
                                        }
                                    }

                                    ui.separator();

                                    if let Some(captain_skills) = build_info.captain_skills.as_ref() {
                                        ui.label("Captain Skills:");
                                        if captain_skills.is_empty() {
                                            ui.label("No Captain Skills");
                                        } else {
                                            for skill in captain_skills {
                                                if let Some(name) = &skill.name {
                                                    let label = ui.label(format!("({}) {}", skill.tier, name));
                                                    if let Some(hover_text) = skill.description.as_ref() {
                                                        label.on_hover_text(hover_text);
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        ui.label("No Captain Skills");
                                    }
                                }
                            });
                        }
                    }
                    ReplayColumn::Hits => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else if let Some(hits_extended_info) = report.hits_hover_text.clone() {
                            ui.label(hits_extended_info);
                        }
                    }
                    _ => {
                        // Do nothing
                    }
                }
            }
        });

        match ui.input(|i| {
            let double_clicked = i.pointer.button_double_clicked(egui::PointerButton::Primary)
                && ui.max_rect().contains(i.pointer.interact_pos().unwrap_or_default());
            let single_clicked = i.pointer.button_clicked(egui::PointerButton::Primary)
                && i.modifiers.ctrl
                && ui.max_rect().contains(i.pointer.interact_pos().unwrap_or_default());

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

    pub fn match_timestamp(&self) -> Timestamp {
        self.match_timestamp
    }

    pub fn player_reports(&self) -> &[PlayerReport] {
        &self.player_reports
    }

    pub fn battle_result(&self) -> Option<BattleResult> {
        self.battle_result
    }

    /// Populate Personal Rating for all players using the provided PR data
    pub fn populate_personal_ratings(&mut self, pr_data: &crate::personal_rating::PersonalRatingData) {
        for report in &mut self.player_reports {
            if report.personal_rating.is_some() {
                continue;
            }

            let Some(player) = Some(report.player()) else {
                continue;
            };

            let ship_id = player.vehicle().id();
            let battle_result = self.battle_result;

            // We need actual damage, kills, and win/loss for a single battle
            let Some(actual_damage) = report.actual_damage else {
                continue;
            };

            let is_win = matches!(battle_result, Some(BattleResult::Win(_)));
            let frags = report.kills.unwrap_or(0);

            let stats = crate::personal_rating::ShipBattleStats {
                ship_id: ship_id as u64,
                battles: 1,
                damage: actual_damage,
                wins: if is_win { 1 } else { 0 },
                frags,
            };

            report.personal_rating = pr_data.calculate_pr(&[stats]);
        }
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
                        .strong(column_name_with_sort_order(
                            "Player Name",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Name,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Name);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::BaseXp => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Base XP",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::BaseXp,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::BaseXp);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::RawXp => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Raw XP",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::RawXp,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::RawXp);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ShipName => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Ship Name",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ShipName,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ShipName);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Hits => {
                    if ui
                        .strong(column_name_with_sort_order("Hits", false, *self.replay_sort.lock(), SortColumn::Hits))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Hits);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Kills => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Kills",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Kills,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Kills);

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
                        .strong(column_name_with_sort_order(
                            "Fires",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Fires,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Fires);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Floods => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Floods",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Floods,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Floods);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Citadels => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Citadels",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Citadels,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Citadels);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Crits => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Crits",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Crits,
                        ))
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
                ReplayColumn::PersonalRating => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "PR",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::PersonalRating,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::PersonalRating);

                        self.sort_players(new_sort);
                    };
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
        self.is_row_expanded
            .range(0..row_nr)
            .map(|(expanded_row_nr, expanded)| {
                let how_expanded = ctx.animate_bool(Id::new(expanded_row_nr), *expanded);
                how_expanded * self.row_heights.get(expanded_row_nr).copied().unwrap()
            })
            .sum::<f32>()
            + row_nr as f32 * ROW_HEIGHT
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

        *self
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
    PersonalRating,
    BaseXp,
    RawXp,
    Kills,
    ObservedDamage,
    ActualDamage,
    ReceivedDamage,
    SpottingDamage,
    PotentialDamage,
    Hits,
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
    Kills,
    ObservedDamage,
    ActualDamage,
    SpottingDamage,
    PotentialDamage,
    Hits,
    TimeLived,
    Fires,
    Floods,
    Citadels,
    Crits,
    ReceivedDamage,
    DistanceTraveled,
    PersonalRating,
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
        Replay { replay_file, resource_loader, battle_report: None, ui_report: None }
    }

    pub fn player_vehicle(&self) -> Option<&VehicleInfoMeta> {
        let meta = &self.replay_file.meta;
        meta.vehicles.iter().find(|vehicle| vehicle.relation == 0)
    }

    pub fn vehicle_name(&self, metadata_provider: &GameMetadataProvider) -> String {
        self.player_vehicle()
            .and_then(|vehicle| metadata_provider.param_localization_id(vehicle.shipId as u32))
            .and_then(|id| metadata_provider.localized_name_from_id(id))
            .unwrap_or_else(|| "Spectator".to_string())
    }

    #[allow(dead_code)]
    pub fn player_name(&self) -> Option<&str> {
        self.player_vehicle().map(|vehicle| vehicle.name.as_str())
    }

    pub fn map_name(&self, metadata_provider: &GameMetadataProvider) -> String {
        let meta = &self.replay_file.meta;
        let map_id = format!("IDS_{}", meta.mapName.to_uppercase());
        metadata_provider.localized_name_from_id(&map_id).unwrap_or_else(|| meta.mapName.clone())
    }

    pub fn game_mode(&self, metadata_provider: &GameMetadataProvider) -> String {
        let meta = &self.replay_file.meta;
        let mode_id = format!("IDS_{}", meta.gameType.to_uppercase());
        metadata_provider.localized_name_from_id(&mode_id).unwrap_or_else(|| meta.gameType.clone())
    }

    pub fn scenario(&self, metadata_provider: &GameMetadataProvider) -> String {
        let meta = &self.replay_file.meta;
        let scenario_id = format!("IDS_SCENARIO_{}", meta.scenario.to_uppercase());
        metadata_provider.localized_name_from_id(&scenario_id).unwrap_or_else(|| meta.scenario.clone())
    }

    pub fn game_time(&self) -> &str {
        &self.replay_file.meta.dateTime
    }

    /// Get the battle result, preferring battle_report if available, otherwise cached result.
    pub fn battle_report(&self) -> Option<&BattleReport> {
        self.battle_report.as_ref()
    }

    pub fn label(&self, metadata_provider: &GameMetadataProvider) -> String {
        [
            self.vehicle_name(metadata_provider).as_str(),
            self.map_name(metadata_provider).as_str(),
            self.scenario(metadata_provider).as_str(),
            self.game_mode(metadata_provider).as_str(),
            self.game_time(),
        ]
        .iter()
        .join("\n")
    }

    pub fn better_file_name(&self, metadata_provider: &GameMetadataProvider) -> String {
        [
            self.vehicle_name(metadata_provider).as_str(),
            self.map_name(metadata_provider).as_str(),
            self.scenario(metadata_provider).as_str(),
            self.game_mode(metadata_provider).as_str(),
            self.game_time(),
        ]
        .iter()
        .join("_")
        .replace(['.', ':', ' '], "-")
    }

    pub fn parse(&self, expected_build: &str) -> Result<BattleReport, Report> {
        let version_parts: Vec<_> = self.replay_file.meta.clientVersionFromExe.split(',').collect();
        assert!(version_parts.len() == 4);
        if version_parts[3] != expected_build {
            return Err(ToolkitError::ReplayVersionMismatch {
                game_version: expected_build.to_string(),
                replay_version: version_parts[3].to_string(),
            }
            .into());
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
        background_task_sender: Option<Sender<BackgroundTask>>,
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

    pub fn battle_result(&self) -> Option<BattleResult> {
        self.battle_report()
            .and_then(|report| report.battle_result().cloned())
            .or_else(|| self.ui_report.as_ref().and_then(|report| report.battle_result()))
    }

    /// Convert this replay's player stats to ShipBattleStats for PR calculation
    pub fn to_battle_stats(&self) -> Option<crate::personal_rating::ShipBattleStats> {
        let vehicle = self.player_vehicle()?;
        let battle_result = self.battle_result()?;
        let ui_report = self.ui_report.as_ref()?;
        let self_report = ui_report.player_reports().iter().find(|report| report.is_self())?;

        let is_win = matches!(battle_result, BattleResult::Win(_));

        Some(crate::personal_rating::ShipBattleStats {
            ship_id: vehicle.shipId as u64,
            battles: 1,
            damage: self_report.actual_damage().unwrap_or_default(),
            wins: if is_win { 1 } else { 0 },
            frags: self_report.kills().unwrap_or_default(),
        })
    }
}

fn column_name_with_sort_order(
    text: &'static str,
    has_info: bool,
    sort_order: SortOrder,
    column: SortColumn,
) -> Cow<'static, str> {
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
        self.tab_state.world_of_warships_data.as_ref().and_then(|wows_data| wows_data.read().game_metadata.clone())
    }

    fn build_replay_player_list(&self, ui_report: &mut UiReport, ui: &mut egui::Ui) {
        // Populate PR data if available (must happen before sorting so PR sort works)
        {
            let pr_data = self.tab_state.personal_rating_data.read();
            if pr_data.is_loaded() {
                ui_report.populate_personal_ratings(&pr_data);
            }
        }

        if !ui_report.sorted {
            let replay_sort = self.tab_state.replay_sort.lock();
            ui_report.sort_players(*replay_sort);
        }

        ui_report.update_visible_columns(&self.tab_state.settings.replay_settings);

        let mut columns =
            vec![egui_table::Column::new(100.0).range(10.0..=500.0).resizable(true); ui_report.columns.len()];
        let action_label_layout = ui.painter().layout_no_wrap(
            "Actions".to_string(),
            egui::FontId::default(),
            ui.style().visuals.text_color(),
        );
        let action_label_width = action_label_layout.rect.width() + 4.0;
        columns[ReplayColumn::Actions as usize] = egui_table::Column::new(action_label_width).resizable(false);

        let table = egui_table::Table::new()
            .id_salt("replay_player_list")
            .num_rows(ui_report.player_reports.len() as u64)
            .columns(columns)
            .num_sticky_cols(3)
            .headers([egui_table::HeaderRow { height: 14.0f32, groups: Default::default() }])
            .auto_size_mode(egui_table::AutoSizeMode::Never);
        table.show(ui, ui_report);
    }

    fn build_replay_chat(&self, battle_report: &BattleReport, ui: &mut egui::Ui) {
        for message in battle_report.game_chat() {
            let GameMessage { sender_relation, sender_name, channel, message, entity_id: _, player } = message;

            let translated_text = if sender_relation.is_none() {
                self.metadata_provider().and_then(|provider| provider.localized_name_from_id(message).map(Cow::Owned))
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
                    format!(
                        "[{}] {sender_name} ({channel:?}): {}",
                        player.clan(),
                        translated_text.as_ref().unwrap_or(&message)
                    )
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
            if let Some(player) = player
                && !player.clan().is_empty()
            {
                job.append(
                    &format!("[{}] ", player.clan()),
                    0.0,
                    TextFormat { color: clan_color_for_player(player).unwrap(), ..Default::default() },
                );
            }
            job.append(&format!("{sender_name}:\n"), 0.0, TextFormat { color: name_color, ..Default::default() });

            let text_color = match channel {
                ChatChannel::Division => Color32::GOLD,
                ChatChannel::Global => Color32::WHITE,
                ChatChannel::Team => Color32::LIGHT_GREEN,
            };

            job.append(
                translated_text.as_ref().unwrap_or(&message),
                0.0,
                TextFormat { color: text_color, ..Default::default() },
            );

            if ui.add(Label::new(job).sense(Sense::click())).on_hover_text("Click to copy").clicked() {
                ui.ctx().copy_text(text);
                *self.tab_state.timed_message.write() =
                    Some(TimedMessage::new(format!("{} Message copied", icons::CHECK_CIRCLE)));
            }
            ui.add(Separator::default());
            ui.end_row();
        }
    }

    fn build_replay_view(&self, replay_file: &mut Replay, ui: &mut egui::Ui, metadata_provider: &GameMetadataProvider) {
        // little hack because of borrowing issues
        let mut hide_my_stats = false;
        let mut hide_my_stats_changed = false;
        if let Some(report) = replay_file.battle_report.as_ref() {
            let self_player = report.self_player();
            ui.horizontal(|ui| {
                if !self_player.clan().is_empty() {
                    ui.label(format!("[{}]", self_player.clan()));
                }
                ui.label(self_player.name());
                ui.label(report.game_type());
                ui.label(report.version().to_path());
                ui.label(report.game_mode());
                ui.label(report.map_name());
                if let Some(battle_result) = replay_file.battle_result() {
                    let text = match battle_result {
                        BattleResult::Win(_) => RichText::new(format!("{} Victory", icons::TROPHY)).color(Color32::LIGHT_GREEN),
                        BattleResult::Loss(_) => RichText::new(format!("{} Defeat", icons::SMILEY_SAD)).color(Color32::LIGHT_RED),
                        BattleResult::Draw => RichText::new(format!("{} Draw", icons::NOTCHES)).color(Color32::LIGHT_YELLOW),
                    };

                    ui.label(text);
                }

                // Show single-game PR
                if let Some(battle_stats) = replay_file.to_battle_stats() {
                    let pr_data = self.tab_state.personal_rating_data.read();
                    if let Some(pr_result) = pr_data.calculate_pr(&[battle_stats]) {
                        ui.label(RichText::new(format!("PR: {:.0} ({})", pr_result.pr, pr_result.category.name())).color(pr_result.category.color()));
                    }
                }

                let mut self_report = None;
                if let Some(ui_report) = replay_file.ui_report.as_ref() {
                    let mut team_damage = 0;
                    let mut red_team_damage = 0;

                    for vehicle_report in &ui_report.player_reports {
                        if vehicle_report.is_enemy {
                            red_team_damage += vehicle_report.actual_damage.unwrap_or(0);
                        } else {
                            team_damage += vehicle_report.actual_damage.unwrap_or(0);
                        }

                        if vehicle_report.is_self {
                            self_report = Some(vehicle_report);
                            hide_my_stats = vehicle_report.manual_stat_hide_toggle;
                        }
                    }

                    let mut job = LayoutJob::default();
                    job.append("Damage Dealt: ", 0.0, Default::default());
                    job.append(
                        &separate_number(team_damage, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())),
                        0.0,
                        TextFormat { color: Color32::LIGHT_GREEN, ..Default::default() },
                    );
                    job.append(" : ", 0.0, Default::default());
                    job.append(
                        &separate_number(red_team_damage, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref())),
                        0.0,
                        TextFormat { color: Color32::LIGHT_RED, ..Default::default() },
                    );

                    job.append(
                        &format!(" ({})", separate_number(team_damage + red_team_damage, self.tab_state.settings.locale.as_ref().map(|s| s.as_ref()))),
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
                            && let Ok(mut file) = std::fs::File::create(path)
                        {
                            for message in report.game_chat() {
                                let GameMessage { sender_relation: _, sender_name, channel, message, entity_id: _, player } = message;

                                match player {
                                    Some(player) if !player.clan().is_empty() => {
                                        let _ = writeln!(file, "[{}] {} ({:?}): {}", player.clan(), sender_name, channel, message);
                                    }
                                    _ => {
                                        let _ = writeln!(file, "{sender_name} ({channel:?}): {message}");
                                    }
                                }
                            }
                        }

                        ui.close_kind(UiKind::Menu);
                    }

                    if ui.small_button(format!("{} Copy", icons::COPY)).clicked() {
                        let mut buf = BufWriter::new(Vec::new());
                        for message in report.game_chat() {
                            let GameMessage { sender_relation: _, sender_name, channel, message, entity_id: _, player } = message;
                            match player {
                                Some(player) if !player.clan().is_empty() => {
                                    let _ = writeln!(buf, "[{}] {} ({:?}): {}", player.clan(), sender_name, channel, message);
                                }
                                _ => {
                                    let _ = writeln!(buf, "{sender_name} ({channel:?}): {message}");
                                }
                            }
                        }

                        let game_chat = String::from_utf8(buf.into_inner().expect("failed to get buf inner")).expect("failed to convert game chat buffer to string");

                        ui.ctx().copy_text(game_chat);

                        ui.close_kind(UiKind::Menu);
                    }
                });
                ui.menu_button("Export Results", |ui| {
                    let format = if ui.button("JSON").clicked() {
                        Some(ReplayExportFormat::Json)
                    } else if ui.button("CBOR").clicked() {
                        Some(ReplayExportFormat::Cbor)
                    } else if ui.button("CSV").clicked() {
                        Some(ReplayExportFormat::Csv)
                    } else {
                        None
                    };
                    if let Some(format) = format
                        && let Some(path) =
                            rfd::FileDialog::new().set_file_name(format!("{}.{}", replay_file.better_file_name(metadata_provider), format.extension())).save_file()
                        && let Ok(mut file) = std::fs::File::create(path)
                    {
                        let transformed_results = Match::new(replay_file, self.tab_state.settings.debug_mode);
                        let result = match format {
                            ReplayExportFormat::Json => serde_json::to_writer(&mut file, &transformed_results).map_err(|e| Box::new(e) as Box<dyn std::error::Error>),
                            ReplayExportFormat::Cbor => serde_cbor::to_writer(&mut file, &transformed_results).map_err(|e| Box::new(e) as Box<dyn std::error::Error>),
                            ReplayExportFormat::Csv => {
                                let mut writer = csv::WriterBuilder::new().has_headers(true).from_writer(file);
                                let mut result = Ok(());
                                for vehicle in transformed_results.vehicles {
                                    result = writer.serialize(FlattenedVehicle::from(vehicle));
                                    if result.is_err() {
                                        break;
                                    }
                                }

                                let _ = writer.flush();

                                result.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
                            }
                        };
                        if let Err(e) = result {
                            error!("Failed to write results to file: {}", e);
                        }
                    }
                });
                if self.tab_state.settings.debug_mode && ui.button("Raw Metadata").clicked() {
                    let parsed_meta: serde_json::Value = serde_json::from_str(&replay_file.replay_file.raw_meta).expect("failed to parse replay metadata");
                    let pretty_meta = serde_json::to_string_pretty(&parsed_meta).expect("failed to serialize replay metadata");
                    let viewer = plaintext_viewer::PlaintextFileViewer {
                        title: Arc::new("metadata.json".to_owned()),
                        file_info: Arc::new(egui::mutex::Mutex::new(FileType::PlainTextFile { ext: ".json".to_owned(), contents: pretty_meta })),
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
                    && let Some(results_json) = report.battle_results()
                {
                    let parsed_results: serde_json::Value = serde_json::from_str(results_json).expect("failed to parse replay metadata");
                    let pretty_meta = serde_json::to_string_pretty(&parsed_results).expect("failed to serialize replay metadata");
                    let viewer = plaintext_viewer::PlaintextFileViewer {
                        title: Arc::new("results.json".to_owned()),
                        file_info: Arc::new(egui::mutex::Mutex::new(FileType::PlainTextFile { ext: ".json".to_owned(), contents: pretty_meta })),
                        open: Arc::new(AtomicBool::new(true)),
                    };

                    self.tab_state.file_viewer.lock().push(viewer);
                }

                if let Some(self_report) = self_report
                    && self_report.is_test_ship()
                    && ui.checkbox(&mut hide_my_stats, "Hide My Test Ship Stats").changed()
                {
                    hide_my_stats_changed = true;
                }
            });

            // Synchronize the hide_my_stats value
            if hide_my_stats_changed
                && let Some(ui_report) = replay_file.ui_report.as_mut()
                && let Some(self_report) = ui_report.player_reports.iter_mut().find(|report| report.is_self)
            {
                self_report.manual_stat_hide_toggle = hide_my_stats;
            }

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
                        self.build_replay_player_list(ui_report, ui);
                    }
                });
            });
        }
    }

    fn build_file_listing(&mut self, ui: &mut egui::Ui) {
        let grouping = self.tab_state.settings.replay_settings.grouping;

        match grouping {
            ReplayGrouping::None => self.build_file_listing_ungrouped(ui),
            ReplayGrouping::Date => self.build_file_listing_grouped_by_date(ui),
            ReplayGrouping::Ship => self.build_file_listing_grouped_by_ship(ui),
        }
    }

    fn build_file_listing_ungrouped(&mut self, ui: &mut egui::Ui) {
        let mut replay_to_load: Option<Arc<RwLock<Replay>>> = None;

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
                        let replay_guard = replay.read();
                        let label = replay_guard.label(&metadata_provider);
                        let battle_result = replay_guard.battle_result();
                        drop(replay_guard);

                        let is_selected = self
                            .tab_state
                            .current_replay
                            .as_ref()
                            .map(|current| Arc::ptr_eq(current, &replay))
                            .unwrap_or(false);

                        // Apply color based on battle result (white for selected to be readable on dark background)
                        let label_text = if is_selected {
                            egui::RichText::new(label.as_str())
                                .color(Color32::WHITE)
                                .background_color(Color32::DARK_GRAY)
                        } else {
                            match battle_result {
                                Some(BattleResult::Win(_)) => {
                                    egui::RichText::new(label.as_str()).color(Color32::LIGHT_GREEN)
                                }
                                Some(BattleResult::Loss(_)) => {
                                    egui::RichText::new(label.as_str()).color(Color32::LIGHT_RED)
                                }
                                Some(BattleResult::Draw) => {
                                    egui::RichText::new(label.as_str()).color(Color32::LIGHT_YELLOW)
                                }
                                None => egui::RichText::new(label.as_str()),
                            }
                        };

                        let label_response = ui
                            .add(Label::new(label_text).selectable(false).sense(Sense::click()))
                            .on_hover_text(label.as_str());
                        label_response.context_menu(|ui| {
                            if ui.button("Copy Path").clicked() {
                                ui.ctx().copy_text(path.to_string_lossy().into_owned());
                                ui.close_kind(UiKind::Menu);
                            }
                            if ui.button("Show in File Explorer").clicked() {
                                util::open_file_explorer(&path);
                                ui.close_kind(UiKind::Menu);
                            }
                        });

                        if label_response.double_clicked() {
                            replay_to_load = Some(replay.clone());
                        }
                        ui.end_row();
                    }
                }
            });
        });

        // Load replay outside of the closure to avoid borrow issues
        if let Some(replay) = replay_to_load
            && let Some(wows_data) = self.tab_state.world_of_warships_data.as_ref()
        {
            update_background_task!(
                self.tab_state.background_tasks,
                load_replay(
                    Arc::clone(&self.tab_state.game_constants),
                    Arc::clone(wows_data),
                    replay,
                    Arc::clone(&self.tab_state.replay_sort),
                    self.tab_state.background_task_sender.clone(),
                    self.tab_state.settings.debug_mode
                )
            );
        }
    }

    fn build_file_listing_grouped_by_date(&mut self, ui: &mut egui::Ui) {
        if let Some(mut files) = self
            .tab_state
            .replay_files
            .as_ref()
            .map(|files| files.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>())
        {
            // Sort by filename (date) descending
            files.sort_by(|a, b| b.0.cmp(&a.0));

            let metadata_provider = self.metadata_provider().unwrap();

            // Group by date (extract date part from game_time which is "DD.MM.YYYY HH:MM:SS")
            let mut groups: Vec<(String, Vec<(std::path::PathBuf, Arc<RwLock<Replay>>)>)> = Vec::new();
            for (path, replay) in files {
                let game_time = replay.read().game_time().to_string();
                // Extract just the date part (DD.MM.YYYY)
                let date = game_time.split(' ').next().unwrap_or(&game_time).to_string();

                if let Some((last_date, last_group)) = groups.last_mut()
                    && *last_date == date
                {
                    last_group.push((path, replay));
                    continue;
                }
                groups.push((date, vec![(path, replay)]));
            }

            // Build maps from Id to replay and path for activation and context menu handling
            let mut id_to_replay: HashMap<egui::Id, Arc<RwLock<Replay>>> = HashMap::new();
            let mut id_to_path: HashMap<egui::Id, std::path::PathBuf> = HashMap::new();

            // Pre-populate the maps before building the tree
            for (_date, replays) in &groups {
                for (path, replay) in replays {
                    let id = egui::Id::new(path);
                    id_to_replay.insert(id, replay.clone());
                    id_to_path.insert(id, path.clone());
                }
            }

            let tree = egui_ltreeview::TreeView::new(ui.make_persistent_id("replay_date_tree"));
            let (_response, actions) = tree.show(ui, |builder| {
                for (date, replays) in &groups {
                    // Calculate win/loss stats for this group
                    let mut wins = 0;
                    let mut losses = 0;
                    for (_, replay) in replays {
                        match replay.read().battle_result() {
                            Some(BattleResult::Win(_)) => wins += 1,
                            Some(BattleResult::Loss(_)) => losses += 1,
                            _ => {}
                        }
                    }
                    let total_with_result = wins + losses;
                    let win_rate = if total_with_result > 0 {
                        format!(" - {}W/{}L ({:.0}%)", wins, losses, (wins as f64 / total_with_result as f64) * 100.0)
                    } else {
                        String::new()
                    };

                    let is_open = builder
                        .dir(egui::Id::new(("date_group", date)), format!("{} ({}){}", date, replays.len(), win_rate));
                    if is_open {
                        for (path, _replay) in replays {
                            let id = egui::Id::new(path);
                            let path_clone = path.clone();

                            let replay_guard = id_to_replay.get(&id).unwrap().read();
                            let ship_name = replay_guard.vehicle_name(&metadata_provider);
                            let map_name = replay_guard.map_name(&metadata_provider);
                            let game_time = replay_guard.game_time().to_string();
                            let time_part = game_time.split(' ').nth(1).unwrap_or(&game_time);
                            let battle_result = replay_guard.battle_result();
                            drop(replay_guard);

                            let label = format!("{} - {} ({})", ship_name, map_name, time_part);
                            let label_text = match battle_result {
                                Some(BattleResult::Win(_)) => RichText::new(label).color(Color32::LIGHT_GREEN),
                                Some(BattleResult::Loss(_)) => RichText::new(label).color(Color32::LIGHT_RED),
                                Some(BattleResult::Draw) => RichText::new(label).color(Color32::LIGHT_YELLOW),
                                None => RichText::new(label),
                            };

                            let node =
                                egui_ltreeview::NodeBuilder::leaf(id).label(label_text).context_menu(move |ui| {
                                    if ui.button("Copy Path").clicked() {
                                        ui.ctx().copy_text(path_clone.to_string_lossy().into_owned());
                                        ui.close_kind(UiKind::Menu);
                                    }
                                    if ui.button("Show in File Explorer").clicked() {
                                        util::open_file_explorer(&path_clone);
                                        ui.close_kind(UiKind::Menu);
                                    }
                                });
                            builder.node(node);
                        }
                    }
                    builder.close_dir();
                }
            });

            // Handle activation (double-click/enter) from tree
            for action in actions {
                if let egui_ltreeview::Action::Activate(activate) = action {
                    for id in activate.selected {
                        if let Some(replay) = id_to_replay.get(&id) {
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
                            break;
                        }
                    }
                }
            }
        }
    }

    fn build_file_listing_grouped_by_ship(&mut self, ui: &mut egui::Ui) {
        if let Some(mut files) = self
            .tab_state
            .replay_files
            .as_ref()
            .map(|files| files.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>())
        {
            // Sort by filename (date) descending first
            files.sort_by(|a, b| b.0.cmp(&a.0));

            let metadata_provider = self.metadata_provider().unwrap();

            // Group by ship name
            let mut ship_groups: HashMap<String, Vec<(std::path::PathBuf, Arc<RwLock<Replay>>)>> = HashMap::new();
            let mut ship_most_recent: HashMap<String, std::path::PathBuf> = HashMap::new();

            for (path, replay) in files {
                let ship_name = replay.read().vehicle_name(&metadata_provider);

                ship_groups.entry(ship_name.clone()).or_default().push((path.clone(), replay));

                // Track most recent replay path for each ship (first one since sorted desc)
                ship_most_recent.entry(ship_name).or_insert(path);
            }

            // Sort groups by most recently played (using the path which contains the date)
            let mut groups: Vec<(String, Vec<(std::path::PathBuf, Arc<RwLock<Replay>>)>)> =
                ship_groups.into_iter().collect();
            groups.sort_by(|a, b| {
                let a_recent = ship_most_recent.get(&a.0).unwrap();
                let b_recent = ship_most_recent.get(&b.0).unwrap();
                b_recent.cmp(a_recent)
            });

            // Build maps from Id to replay and path for activation and context menu handling
            let mut id_to_replay: HashMap<egui::Id, Arc<RwLock<Replay>>> = HashMap::new();
            let mut id_to_path: HashMap<egui::Id, std::path::PathBuf> = HashMap::new();

            // Pre-populate the maps before building the tree
            for (_ship_name, replays) in &groups {
                for (path, replay) in replays {
                    let id = egui::Id::new(path);
                    id_to_replay.insert(id, replay.clone());
                    id_to_path.insert(id, path.clone());
                }
            }

            let tree = egui_ltreeview::TreeView::new(ui.make_persistent_id("replay_ship_tree"));
            let (_response, actions) = tree.show(ui, |builder| {
                for (ship_name, replays) in &groups {
                    // Calculate win/loss stats for this ship
                    let mut wins = 0;
                    let mut losses = 0;
                    for (_, replay) in replays {
                        match replay.read().battle_result() {
                            Some(BattleResult::Win(_)) => wins += 1,
                            Some(BattleResult::Loss(_)) => losses += 1,
                            _ => {}
                        }
                    }
                    let total_with_result = wins + losses;
                    let win_rate = if total_with_result > 0 {
                        format!(" - {}W/{}L ({:.0}%)", wins, losses, (wins as f64 / total_with_result as f64) * 100.0)
                    } else {
                        String::new()
                    };

                    let is_open = builder.dir(
                        egui::Id::new(("ship_group", ship_name)),
                        format!("{} ({}){}", ship_name, replays.len(), win_rate),
                    );
                    if is_open {
                        for (path, _replay) in replays {
                            let id = egui::Id::new(path);
                            let path_clone = path.clone();

                            let replay_guard = id_to_replay.get(&id).unwrap().read();
                            let map_name = replay_guard.map_name(&metadata_provider);
                            let game_time = replay_guard.game_time().to_string();
                            let battle_result = replay_guard.battle_result();
                            drop(replay_guard);

                            let label = format!("{} - {}", map_name, game_time);
                            let label_text = match battle_result {
                                Some(BattleResult::Win(_)) => RichText::new(label).color(Color32::LIGHT_GREEN),
                                Some(BattleResult::Loss(_)) => RichText::new(label).color(Color32::LIGHT_RED),
                                Some(BattleResult::Draw) => RichText::new(label).color(Color32::LIGHT_YELLOW),
                                None => RichText::new(label),
                            };

                            let node =
                                egui_ltreeview::NodeBuilder::leaf(id).label(label_text).context_menu(move |ui| {
                                    if ui.button("Copy Path").clicked() {
                                        ui.ctx().copy_text(path_clone.to_string_lossy().into_owned());
                                        ui.close_kind(UiKind::Menu);
                                    }
                                    if ui.button("Show in File Explorer").clicked() {
                                        util::open_file_explorer(&path_clone);
                                        ui.close_kind(UiKind::Menu);
                                    }
                                });
                            builder.node(node);
                        }
                    }
                    builder.close_dir();
                }
            });

            // Handle activation (double-click/enter) from tree
            for action in actions {
                if let egui_ltreeview::Action::Activate(activate) = action {
                    for id in activate.selected {
                        if let Some(replay) = id_to_replay.get(&id) {
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
                            break;
                        }
                    }
                }
            }
        }
    }

    fn build_replay_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(format!("{} Manually Open Replay File...", icons::FOLDER_OPEN)).clicked()
                && let Some(file) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file()
            {
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

            ui.checkbox(&mut self.tab_state.auto_load_latest_replay, "Autoload Latest Replay");

            ComboBox::from_id_salt("replay_grouping")
                .selected_text(format!("Group: {}", self.tab_state.settings.replay_settings.grouping.label()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.tab_state.settings.replay_settings.grouping,
                        ReplayGrouping::Date,
                        "Date",
                    );
                    ui.selectable_value(
                        &mut self.tab_state.settings.replay_settings.grouping,
                        ReplayGrouping::Ship,
                        "Ship",
                    );
                    ui.selectable_value(
                        &mut self.tab_state.settings.replay_settings.grouping,
                        ReplayGrouping::None,
                        "None",
                    );
                });

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

            if ui.button(format!("{} Show Session Stats", icons::CHART_BAR)).clicked() {
                self.tab_state.show_session_stats = true;
            }
        });
    }

    /// Builds the replay parser tab
    pub fn build_replay_parser_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            self.build_replay_header(ui);

            egui::SidePanel::left("replay_listing_panel").show_inside(ui, |ui| {
                egui::ScrollArea::both().id_salt("replay_chat_scroll_area").show(ui, |ui| {
                    self.build_file_listing(ui);
                });
            });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                if let Some(replay_file) = self.tab_state.current_replay.as_ref() {
                    let mut replay_file = replay_file.write();
                    self.build_replay_view(
                        &mut replay_file,
                        ui,
                        self.metadata_provider().expect("no metadata provider?").as_ref(),
                    );
                } else {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        ui.heading("Double click or load a replay to view data");
                    });
                }
            });
        });

        self.show_session_stats_window(ui);
    }

    pub fn show_session_stats_window(&mut self, ui: &mut egui::Ui) {
        if !self.tab_state.show_session_stats {
            return;
        }

        let Some(metadata_provider) = self.metadata_provider() else {
            return;
        };

        let ctx = ui.ctx();

        egui::Window::new("Session Stats").open(&mut self.tab_state.show_session_stats).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Overall Stats");
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(format!("{} Clear", icons::ERASER)).clicked() {
                        self.tab_state.session_stats.clear();
                    }
                });
            });

            let wins = self.tab_state.session_stats.games_won();
            let losses = self.tab_state.session_stats.games_lost();
            let win_rate = self.tab_state.session_stats.win_rate().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.strong("Win Rate:");
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{wins}W/{losses}L ({win_rate:.02}%)"));
                });
            });

            // Session PR
            if let Some(pr_result) =
                self.tab_state.session_stats.calculate_pr(&self.tab_state.personal_rating_data.read())
            {
                ui.horizontal(|ui| {
                    ui.strong("Personal Rating:");
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{:.0} ({})", pr_result.pr, pr_result.category.name()))
                                .color(pr_result.category.color()),
                        );
                    });
                });
            }

            let total_frags = self.tab_state.session_stats.total_frags();
            ui.horizontal(|ui| {
                ui.strong("Total Frags:");
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{total_frags}"));
                });
            });

            if let Some((ship_name, max_frags)) = self.tab_state.session_stats.max_frags(&metadata_provider) {
                ui.strong("Max Frags:");
                ui.horizontal(|ui| {
                    ui.label(ship_name);
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{max_frags}"));
                    });
                });
            }

            if let Some((ship_name, max_damage)) = self.tab_state.session_stats.max_damage(&metadata_provider) {
                ui.strong("Max Damage:");
                ui.horizontal(|ui| {
                    ui.label(ship_name);
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(separate_number(max_damage, self.tab_state.settings.locale.as_deref()));
                    });
                });
            }

            let mut all_achievements: Vec<Achievement> = Vec::new();
            for replay in &self.tab_state.session_stats.session_replays {
                let replay = replay.read();
                let Some(self_report) = replay
                    .ui_report
                    .as_ref()
                    .and_then(|report| report.player_reports().iter().find(|report| report.is_self()))
                else {
                    continue;
                };

                for achievement in self_report.achievements.as_slice() {
                    match all_achievements.iter_mut().find(|existing_achievement| {
                        existing_achievement.game_param.id() == achievement.game_param.id()
                    }) {
                        Some(existing_achievement) => {
                            existing_achievement.count += achievement.count;
                        }
                        None => all_achievements.push(achievement.clone()),
                    }
                }
            }

            all_achievements
                .sort_by(|a, b| (Reverse(a.count), &b.display_name).cmp(&(Reverse(b.count), &b.display_name)));

            if !all_achievements.is_empty() {
                ui.strong("Achievements");

                for achievement in all_achievements {
                    ui.horizontal(|ui| {
                        let response = ui.label(achievement.display_name);
                        response.on_hover_text(&achievement.description);

                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("{}", achievement.count));
                        });
                    });
                }
            }

            ui.separator();

            ui.heading("Ship Stats");

            ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                let mut battle_results: Vec<(String, PerformanceInfo)> =
                    self.tab_state.session_stats.ship_stats(&metadata_provider).drain().collect();
                battle_results.sort_by(|a, b| {
                    (Reverse(a.1.wins()), a.1.losses(), &a.0).cmp(&(Reverse(b.1.wins()), b.1.losses(), &b.0))
                });
                for (ship_name, perf_info) in battle_results {
                    if perf_info.win_rate().is_none() {
                        continue;
                    }

                    let locale = self.tab_state.settings.locale.as_deref();
                    let pr_data = self.tab_state.personal_rating_data.read();
                    let ship_pr = perf_info.calculate_pr(&pr_data);
                    drop(pr_data);

                    let header = if let Some(ref pr) = ship_pr {
                        format!(
                            "{ship_name} {}W/{}L ({:.0}%) - PR: {:.0}",
                            perf_info.wins(),
                            perf_info.losses(),
                            perf_info.win_rate().unwrap(),
                            pr.pr
                        )
                    } else {
                        format!(
                            "{ship_name} {}W/{}L ({:.0}%)",
                            perf_info.wins(),
                            perf_info.losses(),
                            perf_info.win_rate().unwrap()
                        )
                    };

                    ui.collapsing(header, |ui| {
                        // Show PR at the top of the expanded section
                        if let Some(ref pr) = ship_pr {
                            ui.horizontal(|ui| {
                                ui.label("Personal Rating:");
                                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format!("{:.0} ({})", pr.pr, pr.category.name()))
                                            .color(pr.category.color()),
                                    );
                                });
                            });
                        }

                        ui.horizontal(|ui| {
                            ui.label("Avg Damage:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.avg_damage().unwrap_or_default() as u64, locale));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max Damage:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.max_damage(), locale));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Avg Spotting Damage:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(
                                    perf_info.avg_spotting_damage().unwrap_or_default() as u64,
                                    locale,
                                ));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max Spotting Damage:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.max_spotting_damage(), locale));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Avg Frags:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(format!("{:.2}", perf_info.avg_frags().unwrap_or_default()));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Total Frags:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.total_frags(), locale));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max Frags:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.max_frags(), locale));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Avg Raw XP:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.avg_xp().unwrap_or_default() as i64, locale));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max Raw XP:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.max_xp(), locale));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Avg Base XP:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(
                                    perf_info.avg_win_adjusted_xp().unwrap_or_default() as i64,
                                    locale,
                                ));
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Max Base XP:");
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(separate_number(perf_info.max_win_adjusted_xp(), locale));
                            });
                        });
                    });
                }
            });
        });
    }
}
