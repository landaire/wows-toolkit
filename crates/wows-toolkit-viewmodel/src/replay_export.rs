//! The replay export: one match as a serializable document.
//!
//! Both front ends write this, and ShipBuilds.com reads it, so the shape is
//! fixed and lives in one place rather than being rebuilt per front end.

use std::collections::HashMap;

use escaper::decode_html;
use jiff::Timestamp;
use serde::Serialize;
use serde::Serializer;
use wows_replay_insights::battle_report::Damage;
use wows_replay_insights::battle_report::Hits;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::battle_report::NormalizedPlayer;
use wows_replay_insights::battle_report::PotentialDamage;
use wows_replay_insights::battle_report::SkillInfo;
use wows_replay_insights::battle_report::TranslatedBuild;
use wows_replays::Rc as ReplayRc;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wowsunpack::data::Version;
use wowsunpack::data::ship_config::ShipConfig;
use wowsunpack::game_params::types::Species;

#[derive(Clone, Serialize)]
pub struct Match {
    pub vehicles: Vec<Vehicle>,
    pub metadata: Metadata,
    pub game_chat: Vec<Message>,
}

impl Match {
    /// The match `normalized` describes.
    ///
    /// `players` is the battle report's own player list, which
    /// `normalized.players` is built positionally from; the two are zipped, so
    /// they must come from the same report. The raw player carries the
    /// identity and entity details (realm, clan colour, ship config, captain)
    /// that normalization does not keep.
    ///
    /// `is_debug_mode` keeps the fields an ordinary export strips: enemy
    /// builds, and test-ship results for anyone but the recording player.
    pub fn new(
        normalized: &NormalizedBattleReport,
        players: &[ReplayRc<Player>],
        game_chat: &[GameMessage],
        is_debug_mode: bool,
    ) -> Self {
        let metadata = Metadata {
            map: normalized.metadata.map.clone(),
            game_mode: normalized.metadata.game_mode.clone(),
            game_type: normalized.metadata.game_type.clone(),
            match_group: normalized.metadata.match_group.clone(),
            version: normalized.metadata.version,
            max_duration: normalized.metadata.max_duration,
            played_duration: normalized.metadata.played_duration,
            extra_duration: normalized.metadata.extra_duration,
            timestamp: normalized.metadata.timestamp,
            battle_result: normalized.metadata.battle_result,
        };

        // The two lists are the same roster in the same order, and a document
        // built from a mismatched pair would attribute one player's results
        // to another's account. Checked rather than assumed: this document is
        // read by a service that cannot tell.
        assert_eq!(
            normalized.players.len(),
            players.len(),
            "the normalized roster and the battle report's own must be the same players"
        );
        let vehicles: Vec<Vehicle> = normalized
            .players
            .iter()
            .zip(players.iter())
            .map(|(np, player)| {
                debug_assert_eq!(np.db_id, player.initial_state().db_id(), "roster order drifted");
                Vehicle::new(np, player)
            })
            .collect();

        let match_data = Match {
            vehicles,
            metadata,
            game_chat: game_chat
                .iter()
                .filter(|message| message.sender_relation.is_some())
                .filter_map(Message::from_game_message)
                .collect(),
        };

        if is_debug_mode { match_data } else { match_data.stripped() }
    }

    /// The same match with everything an ordinary export leaves out: enemy
    /// builds, and test-ship results for anyone but the recording player.
    ///
    /// Separate from [`Self::new`] so a caller can hold the full document and
    /// strip a copy at write time, which is what a runtime debug toggle
    /// needs.
    pub fn stripped(mut self) -> Self {
        for vehicle in &mut self.vehicles {
            if vehicle.is_enemy {
                vehicle.translated_build = None;
                vehicle.raw_config = None;
                vehicle.skill_meta_info = None;
            }

            if vehicle.is_test_ship && !vehicle.player.is_replay_perspective {
                vehicle.server_results = None;
                vehicle.observed_results = None;
            }
        }

        self
    }
}

#[derive(Clone, Serialize)]
pub struct Metadata {
    map: String,
    game_mode: String,
    game_type: String,
    match_group: String,
    version: Version,
    #[serde(default)]
    max_duration: u32,
    #[serde(default)]
    played_duration: Option<f32>,
    #[serde(default)]
    extra_duration: Option<f32>,
    timestamp: Timestamp,
    battle_result: Option<BattleResult>,
}

#[derive(Clone, Serialize)]
pub struct ExportPlayer {
    /// WG database ID
    db_id: AccountId,
    /// Which server this player is on
    realm: Option<String>,
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

/// Fallback clan colours for pre-clan-color replays with no `clanColor`
/// field. Exported data must not vary with the app's theme, so these are
/// fixed values rather than a resolved-at-theme call. These are the
/// historical values (`Color32::WHITE` / `LIGHT_GREEN` / `LIGHT_RED`,
/// predating the semantic theme layer entirely), deliberately frozen so
/// files already sent to ShipBuilds.com don't change meaning.
const EXPORT_FALLBACK_CLAN_COLOR_SELF: u32 = 0x00_FF_FF_FF;
const EXPORT_FALLBACK_CLAN_COLOR_ALLY: u32 = 0x00_90_EE_90;
const EXPORT_FALLBACK_CLAN_COLOR_ENEMY: u32 = 0x00_FF_80_80;

impl From<&Player> for ExportPlayer {
    fn from(value: &Player) -> Self {
        let state = value.initial_state();
        let clan = state.clan().to_string();
        // Clanless players never had a clan color, and older replays omit
        // clanColor for everyone; in that case fall back to the team color.
        let clan_color_rgb = if clan.is_empty() {
            0
        } else {
            match state.raw_with_names().get("clanColor").and_then(|c| c.as_i64()) {
                Some(clan_color) => (clan_color & 0xFFFFFF) as u32,
                None => {
                    tracing::warn!("player '{}' has no clanColor; using team color", state.username());
                    let relation = value.relation();
                    if relation.is_self() {
                        EXPORT_FALLBACK_CLAN_COLOR_SELF
                    } else if relation.is_enemy() {
                        EXPORT_FALLBACK_CLAN_COLOR_ENEMY
                    } else {
                        EXPORT_FALLBACK_CLAN_COLOR_ALLY
                    }
                }
            }
        };
        Self {
            db_id: state.db_id(),
            realm: state.realm().map(str::to_owned),
            name: state.username().to_string(),
            clan,
            clan_color_rgb,
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

#[derive(Clone, Serialize)]
pub struct DamageInteraction {
    damage_dealt: u64,
    damage_dealt_percentage: f64,
    /// % of the victim's total received damage that came from this player
    damage_dealt_inverse_percentage: f64,
    damage_received: u64,
    damage_received_percentage: f64,
    /// % of the attacker's total dealt damage that went to this player
    damage_received_inverse_percentage: f64,
}

impl From<&wows_replay_insights::battle_report::DamageInteraction> for DamageInteraction {
    fn from(value: &wows_replay_insights::battle_report::DamageInteraction) -> Self {
        DamageInteraction {
            damage_dealt: value.damage_dealt,
            damage_dealt_percentage: value.damage_dealt_percentage,
            damage_dealt_inverse_percentage: value.damage_dealt_inverse_percentage,
            damage_received: value.damage_received,
            damage_received_percentage: value.damage_received_percentage,
            damage_received_inverse_percentage: value.damage_received_inverse_percentage,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ExportAchievement {
    name: String,
    count: usize,
}

#[derive(Clone, Serialize)]
pub struct ExportRibbon {
    name: String,
    display_name: String,
    count: u64,
}

fn relation_to_string(relation: Relation) -> String {
    if relation.is_self() {
        "self".to_string()
    } else if relation.is_enemy() {
        "enemy".to_string()
    } else {
        "ally".to_string()
    }
}

#[derive(Clone, Serialize)]
pub struct FlattenedVehicle {
    player_name: String,
    player_clan: String,
    player_id: AccountId,
    player_realm: Option<String>,
    /// Ship index that can be mapped to a GameParam
    index: String,
    /// Ship name from EN localization
    ship_name: String,
    /// Ship nation (e.g. "usa", "pan asia", etc.)
    ship_nation: String,
    /// Ship class
    ship_class: Species,
    /// Ship tier
    ship_tier: Option<u32>,
    /// Whether this is a test ship
    is_test_ship: bool,
    /// Whether this is an enemy
    pub is_enemy: bool,
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
    relation: String,
    division_label: Option<String>,
    pub personal_rating: Option<f64>,
    pub personal_rating_category: Option<String>,
    achievement_count: usize,
    ribbon_count: usize,
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
            relation,
            division_label,
            achievements,
            ribbons,
            personal_rating,
            personal_rating_category,
        } = value;

        let (modules, abilities, captain_skills) = if let Some(translated_config) = translated_build {
            let modules =
                translated_config.modernization_slots.iter().flatten().filter_map(|m| m.name.clone()).collect();
            let abilities = translated_config.abilities.iter().filter_map(|ability| ability.name.clone()).collect();
            let captain_skills = translated_config.captain_skills.map(|rows| {
                rows.iter()
                    .flat_map(|row| row.skills.iter())
                    .filter(|skill| skill.learned)
                    .filter_map(|skill| skill.name.clone())
                    .collect()
            });
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
            relation,
            division_label,
            personal_rating,
            personal_rating_category,
            achievement_count: achievements.len(),
            ribbon_count: ribbons.len(),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Vehicle {
    pub player: ExportPlayer,
    /// Ship index that can be mapped to a GameParam
    pub index: String,
    /// Ship name from EN localization
    pub name: String,
    /// Ship nation (e.g. "usa", "pan asia", etc.)
    pub nation: String,
    /// Ship class
    pub class: Species,
    /// Ship tier. `None` for an entity carrying no vehicle ref, which a
    /// spectator row does.
    pub tier: Option<u32>,
    /// Whether this is a test ship
    pub is_test_ship: bool,
    /// Whether this is an enemy
    pub is_enemy: bool,
    pub raw_config: Option<ShipConfig>,
    pub translated_build: Option<TranslatedBuild>,
    /// Captain ID that can be mapped to a GameParam
    pub captain_id: String,
    /// Player's results as provided by the WG server at match end. May not be present
    /// if the player left the match early and quit the game before the match finished.
    ///
    /// Additionally, test ship data is omitted unless it's played by whoever is
    /// the main player in the replay.
    pub server_results: Option<ServerResults>,
    /// Observed results from the replay file. This is the minimum stats possible for the player,
    /// but some results such as actual damage may be higher than what was provided.
    pub observed_results: Option<ObservedResults>,
    pub skill_meta_info: Option<SkillInfo>,
    pub time_lived_secs: Option<u64>,
    /// Relation to the replay perspective player
    pub relation: String,
    /// Division label (e.g. "(A)", "(B)"), None if solo
    pub division_label: Option<String>,
    /// Achievements earned in this battle
    pub achievements: Vec<ExportAchievement>,
    /// Ribbons earned in this battle
    pub ribbons: Vec<ExportRibbon>,
    /// Personal Rating for this game
    pub personal_rating: Option<f64>,
    /// Personal Rating category name (e.g. "Unicum", "Great", etc.)
    pub personal_rating_category: Option<String>,
}

impl Vehicle {
    fn new(np: &NormalizedPlayer, player_data: &Player) -> Self {
        let vehicle_entity = player_data.vehicle_entity();
        let player = ExportPlayer::from(player_data);
        let server = np.server_results.as_ref();

        Self {
            player,
            index: player_data.vehicle().index().to_string(),
            name: np.ship_name.clone(),
            nation: player_data.vehicle().nation().to_string(),
            class: np.ship_class,
            tier: np.ship_tier,
            is_test_ship: np.is_test_ship,
            is_enemy: np.relation.is_enemy(),
            raw_config: vehicle_entity.map(|v| v.props().ship_config().clone()),
            translated_build: np.build.clone(),
            captain_id: vehicle_entity
                .and_then(|v| v.captain())
                .map(|captain| captain.index())
                .unwrap_or("PCW001")
                .to_string(),
            // The results object is exported only when it carried a damage
            // total; old-format results that omit the key are no more a
            // results row here than they are in the table.
            server_results: server.filter(|results| results.damage.is_some()).map(|results| ServerResults {
                xp: results.xp.unwrap_or_default(),
                raw_xp: results.raw_xp.unwrap_or_default(),
                damage: results.damage.unwrap_or_default(),
                damage_details: results.damage_details.clone(),
                hits_details: results.hits_details.clone(),
                spotting_damage: np.spotting_damage().unwrap_or_default(),
                potential_damage: np.potential_damage().unwrap_or_default(),
                potential_damage_details: results.potential_damage_details.clone(),
                received_damage: results.received_damage,
                received_damage_details: results.received_damage_details.clone(),
                fires_dealt: results.fires_dealt.unwrap_or_default(),
                floods_dealt: results.floods_dealt.unwrap_or_default(),
                citadels_dealt: results.citadels_dealt.unwrap_or_default(),
                crits_dealt: results.crits_dealt.unwrap_or_default(),
                distance_traveled: results.distance_traveled.unwrap_or_default(),
                kills: results.kills.unwrap_or_default(),
                // The interactions map is exported only when the results
                // carried one, which is the same gate the counts above use.
                damage_interactions: match results.fires_dealt {
                    Some(_) => results
                        .damage_interactions
                        .iter()
                        .map(|(id, interaction)| (*id, DamageInteraction::from(interaction)))
                        .collect(),
                    None => HashMap::new(),
                },
            }),
            observed_results: Some(ObservedResults {
                damage: np.observed_results.damage,
                kills: np.observed_results.kills,
            }),
            skill_meta_info: Some(np.skill_info.clone()),
            time_lived_secs: np.time_lived_secs,
            relation: relation_to_string(np.relation),
            division_label: np.division_label.clone(),
            achievements: np
                .achievements
                .iter()
                .map(|a| ExportAchievement { name: a.display_name.clone(), count: a.count })
                .collect(),
            ribbons: np
                .ribbons
                .iter()
                .map(|r| ExportRibbon { name: r.name.clone(), display_name: r.display_name.clone(), count: r.count })
                .collect(),
            personal_rating: np.personal_rating.as_ref().map(|pr| pr.pr),
            personal_rating_category: np.personal_rating.as_ref().map(|pr| pr.category.name().to_string()),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ObservedResults {
    damage: u64,
    kills: i64,
}

#[derive(Clone, Serialize)]
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

#[derive(Clone, Serialize)]
pub struct Message {
    sender_db_id: AccountId,
    channel: ChatChannel,
    message: String,
}

impl Message {
    /// One chat message, or `None` when its sender cannot be named.
    ///
    /// A message whose relation was resolved from the arena metadata rather
    /// than from a player object carries no account to attribute it to, and
    /// the document has no way to say "somebody"; such a message is left out
    /// rather than panicking the parse it was read by.
    fn from_game_message(value: &GameMessage) -> Option<Self> {
        let message =
            if let Ok(decoded) = decode_html(value.message.as_str()) { decoded } else { value.message.clone() };
        Some(Self {
            sender_db_id: value.player.as_ref()?.initial_state().db_id(),
            channel: value.channel.clone(),
            message,
        })
    }
}

/// What an exported battle's file is called, without its extension.
///
/// Ship, map, scenario, mode and the time it was played, joined so a directory
/// of exports sorts and reads without opening any of them. Dots, colons and
/// spaces become dashes, because a file name carrying them is awkward on one
/// platform or another.
///
/// Shared so the two apps name the same battle the same file
/// (`ui/replay_parser/mod.rs`'s `better_file_name`).
pub fn exported_file_stem(
    meta: &wows_replays::ReplayMeta,
    metadata: &wowsunpack::game_params::provider::GameMetadataProvider,
) -> String {
    use wowsunpack::data::ResourceLoader;

    // Relation zero is the recording player, which is whose ship the export is
    // named after. A recording with none is a spectator's.
    let ship = meta
        .vehicles
        .iter()
        .find(|vehicle| vehicle.relation == 0)
        .and_then(|vehicle| metadata.param_localization_id(vehicle.shipId.raw().into()))
        .and_then(|id| metadata.localized_name_from_id(&wowsunpack::data::TranslationKey::new(id)))
        .unwrap_or_else(|| rust_i18n::t!("ui.replay.spectator").into_owned());

    let parts = [
        ship,
        wowsunpack::game_params::translations::translate_map_name(&meta.mapName, metadata),
        wowsunpack::game_params::translations::translate_scenario(&meta.scenario, metadata),
        wowsunpack::game_params::translations::translate_game_mode(meta.gameType.as_deref().unwrap_or(""), metadata),
        meta.dateTime.clone(),
    ];
    parts.join("_").replace(['.', ':', ' '], "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vehicle carrying only what `stripped` looks at; every other field is
    /// at its empty value, which the strip never reads.
    fn vehicle(is_enemy: bool, is_test_ship: bool, is_replay_perspective: bool) -> Vehicle {
        Vehicle {
            player: ExportPlayer {
                db_id: AccountId(1),
                realm: None,
                name: String::new(),
                clan: String::new(),
                clan_color_rgb: 0,
                division_id: None,
                team_id: 0,
                is_replay_perspective,
            },
            index: String::new(),
            name: String::new(),
            nation: String::new(),
            class: Species::Destroyer,
            tier: Some(10),
            is_test_ship,
            is_enemy,
            raw_config: None,
            translated_build: Some(TranslatedBuild {
                modernization_slots: Vec::new(),
                signals: Vec::new(),
                loadout: Vec::new(),
                abilities: Vec::new(),
                captain_skills: None,
            }),
            captain_id: String::new(),
            server_results: None,
            observed_results: Some(ObservedResults { damage: 1, kills: 1 }),
            skill_meta_info: Some(SkillInfo { skill_points: 0, num_skills: 0, highest_tier: 0, num_tier_1_skills: 0 }),
            time_lived_secs: None,
            relation: String::new(),
            division_label: None,
            achievements: Vec::new(),
            ribbons: Vec::new(),
            personal_rating: None,
            personal_rating_category: None,
        }
    }

    fn document(vehicles: Vec<Vehicle>) -> Match {
        Match {
            vehicles,
            metadata: Metadata {
                map: String::new(),
                game_mode: String::new(),
                game_type: String::new(),
                match_group: String::new(),
                version: Version::default(),
                max_duration: 0,
                played_duration: None,
                extra_duration: None,
                timestamp: Timestamp::UNIX_EPOCH,
                battle_result: None,
            },
            game_chat: Vec::new(),
        }
    }

    #[test]
    fn an_ordinary_export_carries_no_enemy_build() {
        let stripped = document(vec![vehicle(true, false, false), vehicle(false, false, false)]).stripped();

        let enemy = stripped.vehicles.iter().find(|vehicle| vehicle.is_enemy).expect("the enemy is still listed");
        assert!(enemy.translated_build.is_none(), "an enemy build is stripped");
        assert!(enemy.skill_meta_info.is_none());

        let ally = stripped.vehicles.iter().find(|vehicle| !vehicle.is_enemy).expect("the ally is still listed");
        assert!(ally.translated_build.is_some(), "an ally keeps theirs");
    }

    /// The game hides test-ship results for everyone but the player flying
    /// one, and so does an ordinary export.
    #[test]
    fn a_test_ship_keeps_its_results_only_for_the_recording_player() {
        let stripped = document(vec![vehicle(false, true, false), vehicle(false, true, true)]).stripped();

        let other = &stripped.vehicles[0];
        assert!(other.observed_results.is_none(), "another player's test ship is stripped");

        let mine = &stripped.vehicles[1];
        assert!(mine.observed_results.is_some(), "my own test ship is not");
    }

    #[test]
    fn stripping_drops_no_vehicle() {
        let full = document(vec![vehicle(true, false, false), vehicle(false, true, true)]);
        assert_eq!(full.clone().stripped().vehicles.len(), full.vehicles.len());
    }
}
