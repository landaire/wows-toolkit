use std::collections::HashMap;

use escaper::decode_html;
use jiff::Timestamp;
use serde::Serialize;
use serde::Serializer;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::ShipConfig;
use wows_replays::types::AccountId;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::Species;

use crate::ui::replay_parser::Damage;
use crate::ui::replay_parser::Hits;
use crate::ui::replay_parser::PlayerReport;
use crate::ui::replay_parser::PotentialDamage;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SkillInfo;
use crate::ui::replay_parser::TranslatedBuild;

#[derive(Serialize)]
pub struct Match {
    pub vehicles: Vec<Vehicle>,
    pub metadata: Metadata,
    pub game_chat: Vec<Message>,
}

impl Match {
    pub fn new(replay: &Replay, is_debug_mode: bool) -> Self {
        let battle_report = replay.battle_report.as_ref().expect("no battle report for replay?");
        let ui_report = replay.ui_report.as_ref().expect("no UI report for replay?");
        let metadata = Metadata {
            map: battle_report.map_name().to_string(),
            game_mode: battle_report.game_mode().to_string(),
            game_type: battle_report.game_type().to_string(),
            match_group: battle_report.match_group().to_string(),
            version: battle_report.version(),
            duration: replay.replay_file.meta.duration,
            timestamp: ui_report.match_timestamp(),
            battle_result: battle_report.battle_result().cloned(),
        };

        let vehicles: Vec<Vehicle> = ui_report.player_reports().iter().map(Vehicle::new).collect();

        let mut match_data = Match {
            vehicles,
            metadata,
            game_chat: battle_report
                .game_chat()
                .iter()
                .filter(|message| message.sender_relation.is_some())
                .map(Message::from)
                .collect(),
        };

        if is_debug_mode {
            return match_data;
        }

        for vehicle in &mut match_data.vehicles {
            // Remove enemy build information
            if vehicle.is_enemy {
                vehicle.translated_build = None;
                vehicle.raw_config = None;
                vehicle.skill_meta_info = None;
            }

            // Remove stats the game doesn't show for test ships
            if vehicle.is_test_ship && !vehicle.player.is_replay_perspective {
                vehicle.server_results = None;
                vehicle.observed_results = None;
            }
        }

        match_data
    }
}

#[derive(Serialize)]
pub struct Metadata {
    map: String,
    game_mode: String,
    game_type: String,
    match_group: String,
    version: Version,
    duration: u32,
    timestamp: Timestamp,
    battle_result: Option<BattleResult>,
}

#[derive(Serialize)]
pub struct Player {
    /// WG database ID
    db_id: AccountId,
    /// Which server this player is on
    realm: String,
    /// Player name
    name: String,
    clan: String,
    /// Clan color corresponding to the clan's league (hurricane, typhoon, etc.)
    clan_color_rgb: u32,
    /// ID that can be used to find who is in the same division. This is `None` if the player is not in a division.
    division_id: Option<u32>,
    /// Team assignment. This is `None` if the player is a spectator.
    team_id: u32,
    /// Whether or not this is who the replay is from
    is_replay_perspective: bool,
}

impl From<&wows_replays::analyzer::battle_controller::Player> for Player {
    fn from(value: &wows_replays::analyzer::battle_controller::Player) -> Self {
        let state = value.initial_state();
        let clan_color = state.raw_with_names().get("clanColor").expect("no clan color?");
        let clan_color = clan_color.as_i64().expect("clan color is not an i64") & 0xFFFFFF;
        Self {
            db_id: state.db_id(),
            realm: state.realm().to_string(),
            name: state.username().to_string(),
            clan: state.clan().to_string(),
            clan_color_rgb: clan_color as u32,
            division_id: if state.division_id() > 0 { Some(state.division_id() as u32) } else { None },
            team_id: state.team_id() as u32,
            is_replay_perspective: value.relation().is_self(),
        }
    }
}

fn serialize_option_vec<S>(opt_vec: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match opt_vec {
        Some(vec) => {
            let joined = vec.join(",");
            serializer.serialize_str(&joined)
        }
        None => serializer.serialize_none(),
    }
}

#[derive(Serialize)]
pub struct DamageInteraction {
    damage_dealt: u64,
    damage_dealt_percentage: f64,
    damage_received: u64,
    damage_received_percentage: f64,
}

impl From<&crate::ui::replay_parser::DamageInteraction> for DamageInteraction {
    fn from(value: &crate::ui::replay_parser::DamageInteraction) -> Self {
        DamageInteraction {
            damage_dealt: value.damage_dealt(),
            damage_dealt_percentage: value.damage_dealt_percentage(),
            damage_received: value.damage_received(),
            damage_received_percentage: value.damage_received_percentage(),
        }
    }
}

#[derive(Serialize)]
pub struct FlattenedVehicle {
    player_name: String,
    player_clan: String,
    player_id: AccountId,
    player_realm: String,
    /// Ship index that can be mapped to a GameParam
    index: String,
    /// Ship name from EN localization
    ship_name: String,
    /// Ship nation (e.g. "usa", "pan asia", etc.)
    ship_nation: String,
    /// Ship class
    ship_class: Species,
    /// Ship tier
    ship_tier: u32,
    /// Whether this is a test ship
    is_test_ship: bool,
    /// Whether this is an enemy
    is_enemy: bool,
    #[serde(serialize_with = "serialize_option_vec")]
    modules: Option<Vec<String>>,
    #[serde(serialize_with = "serialize_option_vec")]
    abilities: Option<Vec<String>>,
    /// Captain ID that can be mapped to a GameParam
    captain_id: Option<String>,
    #[serde(serialize_with = "serialize_option_vec")]
    captain_skills: Option<Vec<String>>,
    xp: Option<i64>,
    raw_xp: Option<i64>,
    damage: Option<u64>,
    ap: Option<u64>,
    sap: Option<u64>,
    he: Option<u64>,
    he_secondaries: Option<u64>,
    sap_secondaries: Option<u64>,
    torps: Option<u64>,
    deep_water_torps: Option<u64>,
    fire: Option<u64>,
    flooding: Option<u64>,
    hits_ap: Option<u64>,
    hits_sap: Option<u64>,
    hits_he: Option<u64>,
    hits_he_secondaries: Option<u64>,
    hits_sap_secondaries: Option<u64>,
    hits_ap_secondaries_manual: Option<u64>,
    hits_he_secondaries_manual: Option<u64>,
    hits_sap_secondaries_manual: Option<u64>,
    hits_torps: Option<u64>,
    spotting_damage: Option<u64>,
    potential_damage: Option<u64>,
    potential_damage_artillery: Option<u64>,
    potential_damage_torpedoes: Option<u64>,
    potential_damage_planes: Option<u64>,
    received_damage: Option<u64>,
    received_damage_ap: Option<u64>,
    received_damage_sap: Option<u64>,
    received_damage_he: Option<u64>,
    received_damage_he_secondaries: Option<u64>,
    received_damage_sap_secondaries: Option<u64>,
    received_damage_torps: Option<u64>,
    received_damage_deep_water_torps: Option<u64>,
    received_damage_fire: Option<u64>,
    received_damage_flooding: Option<u64>,
    fires_dealt: Option<u64>,
    floods_dealt: Option<u64>,
    citadels_dealt: Option<u64>,
    crits_dealt: Option<u64>,
    distance_traveled: Option<f64>,
    kills: Option<i64>,
    observed_damage: u64,
    observed_kills: i64,
    skill_points_allocated: Option<usize>,
    num_skills: Option<usize>,
    highest_tier_skill: Option<usize>,
    num_tier_1_skills: Option<usize>,
    time_lived_secs: Option<u64>,
}

impl From<Vehicle> for FlattenedVehicle {
    fn from(value: Vehicle) -> Self {
        let Vehicle {
            player,
            index,
            name,
            nation,
            class,
            tier,
            is_test_ship,
            is_enemy,
            raw_config: _,
            translated_build,
            captain_id,
            server_results,
            observed_results,
            skill_meta_info,
            time_lived_secs,
        } = value;

        let (modules, abilities, captain_skills) = if let Some(translated_config) = translated_build {
            let modules = translated_config.modules.iter().filter_map(|module| module.name.clone()).collect();
            let abilities = translated_config.abilities.iter().filter_map(|ability| ability.name.clone()).collect();
            let captain_skills = translated_config
                .captain_skills
                .map(|skills| skills.iter().filter_map(|skill| skill.name.clone()).collect());
            (Some(modules), Some(abilities), captain_skills)
        } else {
            (None, None, None)
        };
        Self {
            player_name: player.name,
            player_clan: player.clan,
            player_id: player.db_id,
            player_realm: player.realm,
            index,
            ship_name: name,
            ship_nation: nation,
            ship_class: class,
            ship_tier: tier,
            is_test_ship,
            is_enemy,
            modules,
            abilities,
            captain_id: Some(captain_id),
            captain_skills,
            xp: server_results.as_ref().map(|results| results.xp),
            raw_xp: server_results.as_ref().map(|results| results.raw_xp),
            damage: server_results.as_ref().map(|results| results.damage),
            ap: server_results.as_ref().and_then(|results| results.damage_details.ap),
            sap: server_results.as_ref().and_then(|results| results.damage_details.sap),
            he: server_results.as_ref().and_then(|results| results.damage_details.he),
            he_secondaries: server_results.as_ref().and_then(|results| results.damage_details.he_secondaries),
            sap_secondaries: server_results.as_ref().and_then(|results| results.damage_details.sap_secondaries),
            torps: server_results.as_ref().and_then(|results| results.damage_details.torps),
            deep_water_torps: server_results.as_ref().and_then(|results| results.damage_details.deep_water_torps),
            fire: server_results.as_ref().and_then(|results| results.damage_details.fire),
            flooding: server_results.as_ref().and_then(|results| results.damage_details.flooding),
            spotting_damage: server_results.as_ref().map(|results| results.spotting_damage),
            potential_damage: server_results.as_ref().map(|results| results.potential_damage),
            potential_damage_artillery: server_results
                .as_ref()
                .map(|results| results.potential_damage_details.artillery),
            potential_damage_torpedoes: server_results
                .as_ref()
                .map(|results| results.potential_damage_details.torpedoes),
            potential_damage_planes: server_results.as_ref().map(|results| results.potential_damage_details.planes),
            received_damage: server_results.as_ref().map(|results| results.received_damage),
            received_damage_ap: server_results.as_ref().and_then(|results| results.received_damage_details.ap),
            received_damage_sap: server_results.as_ref().and_then(|results| results.received_damage_details.sap),
            received_damage_he: server_results.as_ref().and_then(|results| results.received_damage_details.he),
            received_damage_he_secondaries: server_results
                .as_ref()
                .and_then(|results| results.received_damage_details.he_secondaries),
            received_damage_sap_secondaries: server_results
                .as_ref()
                .and_then(|results| results.received_damage_details.sap_secondaries),
            received_damage_torps: server_results.as_ref().and_then(|results| results.received_damage_details.torps),
            received_damage_deep_water_torps: server_results
                .as_ref()
                .and_then(|results| results.received_damage_details.deep_water_torps),
            received_damage_fire: server_results.as_ref().and_then(|results| results.received_damage_details.fire),
            received_damage_flooding: server_results
                .as_ref()
                .and_then(|results| results.received_damage_details.flooding),
            fires_dealt: server_results.as_ref().map(|results| results.fires_dealt),
            floods_dealt: server_results.as_ref().map(|results| results.floods_dealt),
            citadels_dealt: server_results.as_ref().map(|results| results.citadels_dealt),
            crits_dealt: server_results.as_ref().map(|results| results.crits_dealt),
            distance_traveled: server_results.as_ref().map(|results| results.distance_traveled),
            kills: server_results.as_ref().map(|results| results.kills),
            observed_damage: observed_results.as_ref().map(|results| results.damage).unwrap_or_default(),
            observed_kills: observed_results.as_ref().map(|results| results.kills).unwrap_or_default(),
            skill_points_allocated: skill_meta_info.as_ref().map(|info| info.skill_points),
            num_skills: skill_meta_info.as_ref().map(|info| info.num_skills),
            highest_tier_skill: skill_meta_info.as_ref().map(|info| info.highest_tier),
            num_tier_1_skills: skill_meta_info.as_ref().map(|info| info.num_tier_1_skills),
            time_lived_secs,
            hits_ap: server_results.as_ref().and_then(|results| results.hits_details.ap),
            hits_sap: server_results.as_ref().and_then(|results| results.hits_details.sap),
            hits_he: server_results.as_ref().and_then(|results| results.hits_details.he),
            hits_he_secondaries: server_results.as_ref().and_then(|results| results.hits_details.he_secondaries),
            hits_sap_secondaries: server_results.as_ref().and_then(|results| results.hits_details.sap_secondaries),
            hits_ap_secondaries_manual: server_results
                .as_ref()
                .and_then(|results| results.hits_details.ap_secondaries_manual),
            hits_he_secondaries_manual: server_results
                .as_ref()
                .and_then(|results| results.hits_details.he_secondaries_manual),
            hits_sap_secondaries_manual: server_results
                .as_ref()
                .and_then(|results| results.hits_details.sap_secondaries_manual),
            hits_torps: server_results.as_ref().and_then(|results| results.hits_details.torps),
        }
    }
}

#[derive(Serialize)]
pub struct Vehicle {
    player: Player,
    /// Ship index that can be mapped to a GameParam
    index: String,
    /// Ship name from EN localization
    name: String,
    /// Ship nation (e.g. "usa", "pan asia", etc.)
    nation: String,
    /// Ship class
    class: Species,
    /// Ship tier
    tier: u32,
    /// Whether this is a test ship
    is_test_ship: bool,
    /// Whether this is an enemy
    is_enemy: bool,
    raw_config: Option<ShipConfig>,
    translated_build: Option<TranslatedBuild>,
    /// Captain ID that can be mapped to a GameParam
    captain_id: String,
    /// Player's results as provided by the WG server at match end. May not be present
    /// if the player left the match early and quit the game before the match finished.
    ///
    /// Additionally, test ship data is omitted unless it's played by whoever is
    /// the main player in the replay.
    server_results: Option<ServerResults>,
    /// Observed results from the replay file. This is the minimum stats possible for the player,
    /// but some results such as actual damage may be higher than what was provided.
    observed_results: Option<ObservedResults>,
    skill_meta_info: Option<SkillInfo>,
    time_lived_secs: Option<u64>,
}

impl Vehicle {
    fn new(value: &PlayerReport) -> Self {
        let player_data = value.player();
        let vehicle_entity = player_data.vehicle_entity();
        let player = Player::from(player_data);
        Self {
            player,
            index: player_data.vehicle().index().to_string(),
            name: value.ship_name().to_string(),
            nation: player_data.vehicle().nation().to_string(),
            class: player_data.vehicle().species().expect("no species"),
            tier: player_data.vehicle().data().vehicle_ref().expect("no vehicle ref").level(),
            is_test_ship: value.is_test_ship(),
            is_enemy: value.relation().is_enemy(),
            raw_config: vehicle_entity.map(|v| v.props().ship_config().clone()),
            translated_build: value.translated_build().cloned(),
            captain_id: vehicle_entity
                .and_then(|v| v.captain())
                .map(|captain| captain.index())
                .unwrap_or("PCW001")
                .to_string(),
            server_results: if value.actual_damage_report().is_some() {
                Some(ServerResults {
                    xp: value.base_xp().unwrap_or_default(),
                    raw_xp: value.raw_xp().unwrap_or_default(),
                    damage: value.actual_damage().unwrap_or_default(),
                    damage_details: value.actual_damage_report().cloned().expect("no actual damage report"),
                    hits_details: value.hits_report().cloned().expect("no hit report"),
                    spotting_damage: value.spotting_damage().unwrap_or_default(),
                    potential_damage: value.potential_damage().unwrap_or_default(),
                    potential_damage_details: value
                        .potential_damage_report()
                        .cloned()
                        .expect("no potential damage report"),
                    received_damage: value.received_damage().unwrap_or_default(),
                    received_damage_details: value
                        .received_damage_report()
                        .cloned()
                        .expect("no received damage report"),
                    fires_dealt: value.fires().unwrap_or_default(),
                    floods_dealt: value.floods().unwrap_or_default(),
                    citadels_dealt: value.citadels().unwrap_or_default(),
                    crits_dealt: value.crits().unwrap_or_default(),
                    distance_traveled: value.distance_traveled().unwrap_or_default(),
                    kills: value.kills().unwrap_or_default(),
                    damage_interactions: value
                        .damage_interactions()
                        .map(|interactions| {
                            HashMap::from_iter(interactions.iter().map(|(key, value)| (*key, value.into())))
                        })
                        .unwrap_or_default(),
                })
            } else {
                None
            },
            observed_results: Some(ObservedResults { damage: value.observed_damage(), kills: value.observed_kills() }),
            skill_meta_info: Some(value.skill_info().clone()),
            time_lived_secs: value.time_lived_secs(),
        }
    }
}

#[derive(Serialize)]
pub struct ObservedResults {
    damage: u64,
    kills: i64,
}

#[derive(Serialize)]
pub struct ServerResults {
    xp: i64,
    raw_xp: i64,
    damage: u64,
    damage_details: Damage,
    damage_interactions: HashMap<AccountId, DamageInteraction>,
    hits_details: Hits,
    spotting_damage: u64,
    potential_damage: u64,
    potential_damage_details: PotentialDamage,
    received_damage: u64,
    received_damage_details: Damage,
    fires_dealt: u64,
    floods_dealt: u64,
    citadels_dealt: u64,
    crits_dealt: u64,
    distance_traveled: f64,
    kills: i64,
}

#[derive(Serialize)]
pub struct Message {
    sender_db_id: AccountId,
    channel: ChatChannel,
    message: String,
}

impl From<&GameMessage> for Message {
    fn from(value: &GameMessage) -> Self {
        let message =
            if let Ok(decoded) = decode_html(value.message.as_str()) { decoded } else { value.message.clone() };
        Self {
            sender_db_id: value.player.as_ref().expect("no player for message").initial_state().db_id(),
            channel: value.channel,
            message,
        }
    }
}
