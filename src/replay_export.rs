use std::sync::Arc;

use chrono::DateTime;
use chrono::Local;
use escaper::decode_html;
use serde::Serialize;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::ShipConfig;
use wowsunpack::data::Version;

use crate::ui::replay_parser::Damage;
use crate::ui::replay_parser::PotentialDamage;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SkillInfo;
use crate::ui::replay_parser::VehicleReport;

#[derive(Serialize)]
pub struct Match {
    vehicles: Vec<Vehicle>,
    metadata: Metadata,
    game_chat: Vec<Message>,
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

        let vehicles: Vec<Vehicle> = ui_report.vehicle_reports().iter().map(Vehicle::from).collect();

        let mut match_data =
            Match { vehicles, metadata, game_chat: battle_report.game_chat().iter().filter(|message| message.sender_relation.is_some()).map(Message::from).collect() };

        if is_debug_mode {
            return match_data;
        }

        for vehicle in &mut match_data.vehicles {
            // Remove enemy build information
            if vehicle.is_test_ship {
                vehicle.config = None;
                vehicle.skill_info = None;
            }

            // Remove stats the game doesn't show for test ships
            if vehicle.is_test_ship && !vehicle.player.is_replay_perspective {
                vehicle.server_results = None;
                vehicle.observed_results = None;
            }
        }

        match_data
    }

    pub fn vehicles(&self) -> &[Vehicle] {
        &self.vehicles
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
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
    timestamp: DateTime<Local>,
    battle_result: Option<BattleResult>,
}

#[derive(Serialize)]
pub struct Player {
    /// WG database ID
    db_id: i64,
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
        let clan_color = value.raw_props_with_name().get("clanColor").expect("no clan color?");
        let clan_color = clan_color.as_i64().expect("clan color is not an i64") & 0xFFFFFF;
        Self {
            db_id: value.db_id(),
            realm: value.realm().to_string(),
            name: value.name().to_string(),
            clan: value.clan().to_string(),
            clan_color_rgb: clan_color as u32,
            division_id: if value.division_id() > 0 { Some(value.division_id()) } else { None },
            team_id: value.team_id(),
            is_replay_perspective: value.relation() == 0,
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
    /// Whether this is a test ship
    is_test_ship: bool,
    /// Ship config which includes modules, upgrades, signals, etc.
    config: Option<ShipConfig>,
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
    skill_info: Option<SkillInfo>,
    time_lived_secs: Option<u64>,
}

impl From<&VehicleReport> for Vehicle {
    fn from(value: &VehicleReport) -> Self {
        let vehicle_entity = value.vehicle();
        let player_entity = vehicle_entity.player().expect("vehicle has no player?");
        let player = Player::from(Arc::as_ref(player_entity));
        Self {
            player,
            index: player_entity.vehicle().index().to_string(),
            name: value.ship_name().to_string(),
            nation: player_entity.vehicle().nation().to_string(),
            is_test_ship: value.is_test_ship(),
            config: Some(vehicle_entity.props().ship_config().clone()),
            captain_id: vehicle_entity.captain().map(|captain| captain.index()).unwrap_or("PCW001").to_string(),
            server_results: if value.actual_damage_report().is_some() {
                Some(ServerResults {
                    xp: value.base_xp().unwrap_or_default(),
                    raw_xp: value.raw_xp().unwrap_or_default(),
                    damage: value.actual_damage().unwrap_or_default(),
                    damage_details: value.actual_damage_report().cloned().expect("no actual damage report"),
                    spotting_damage: value.spotting_damage().unwrap_or_default(),
                    potential_damage: value.potential_damage().unwrap_or_default(),
                    potential_damage_details: value.potential_damage_report().cloned().expect("no potential damage report"),
                    received_damage: value.received_damage().unwrap_or_default(),
                    received_damage_details: value.received_damage_report().cloned().expect("no received damage report"),
                    fires_dealt: value.fires().unwrap_or_default(),
                    floods_dealt: value.floods().unwrap_or_default(),
                    citadels_dealt: value.citadels().unwrap_or_default(),
                    crits_dealt: value.crits().unwrap_or_default(),
                    distance_traveled: value.distance_traveled().unwrap_or_default(),
                    kills: value.kills().unwrap_or_default(),
                })
            } else {
                None
            },
            observed_results: Some(ObservedResults { damage: value.observed_damage(), kills: value.observed_kills() }),
            skill_info: Some(value.skill_info().clone()),
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
    sender_db_id: i64,
    channel: ChatChannel,
    message: String,
}

impl From<&GameMessage> for Message {
    fn from(value: &GameMessage) -> Self {
        let message = if let Ok(decoded) = decode_html(value.message.as_str()) { decoded } else { value.message.clone() };
        Self { sender_db_id: value.player.as_ref().expect("no player for message").db_id(), channel: value.channel, message }
    }
}
