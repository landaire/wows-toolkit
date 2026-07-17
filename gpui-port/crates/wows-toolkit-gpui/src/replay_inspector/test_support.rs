//! Shared test-only fixtures for `model`, `columns`, and `sort`. Every
//! `PlayerRow`/`NormalizedPlayer`/`NormalizedBattleReport` here is fabricated
//! by hand (no parsed replay), matching the brief's "fabricated PlayerRow"
//! testing strategy.

use std::collections::HashMap;

use wows_replay_insights::battle_report::MatchMetadata;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::battle_report::NormalizedPlayer;
use wows_replay_insights::battle_report::ObservedResults;
use wows_replay_insights::battle_report::SkillInfo;
use wows_replays::ReplayMeta;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::Species;

use super::model::PlayerRow;

/// A minimal `PlayerRow` with every field at its "absent/zero" default.
/// Callers override the handful of fields relevant to what they're testing
/// via struct-update syntax: `PlayerRow { actual_damage: Some(1), ..base_row(1, Relation::new(2), false) }`.
pub(crate) fn base_row(db_id: i64, relation: Relation, is_self: bool) -> PlayerRow {
    PlayerRow {
        db_id: AccountId(db_id),
        team_id: TeamId::from(0i64),
        relation,
        is_self,
        is_bot: false,
        is_abuser: false,
        is_test_ship: false,
        manual_stat_hide_toggle: false,
        display_name: format!("Player{db_id}"),
        clan_tag: None,
        clan_color_rgb: 0,
        division_label: None,
        is_self_division_mate: false,
        ship_name: "Test Ship".to_string(),
        ship_species_text: "Destroyer".to_string(),
        ship_class: Species::Destroyer,
        ship_id: None,
        has_vehicle_entity: true,
        base_xp: None,
        base_xp_text: None,
        raw_xp: None,
        raw_xp_text: None,
        observed_damage: 0,
        observed_damage_text: "0".to_string(),
        observed_kills: 0,
        actual_damage: None,
        actual_damage_report: None,
        actual_damage_text: None,
        actual_damage_hover_text: None,
        hits: None,
        hits_report: None,
        hits_text: None,
        hits_hover_text: None,
        spotting_damage: None,
        spotting_damage_text: None,
        spotting_damage_hover_text: None,
        potential_damage: None,
        potential_damage_text: None,
        potential_damage_hover_text: None,
        potential_damage_report: None,
        received_damage: None,
        received_damage_text: None,
        received_damage_hover_text: None,
        received_damage_report: None,
        damage_interactions: None,
        fires: None,
        floods: None,
        citadels: None,
        crits: None,
        time_lived_secs: None,
        time_lived_text: None,
        distance_traveled: None,
        kills: None,
        heal_count: None,
        skill_points: 0,
        num_skills: 0,
        highest_tier: 0,
        num_tier_1_skills: 0,
        skill_label_text: "0pts (0 skills)".to_string(),
        skill_hover_text: None,
        skill_warning: false,
        has_dazzle: false,
        has_ifa: false,
        translated_build: None,
        achievements: Vec::new(),
        ribbons: Vec::new(),
        consumables: Vec::new(),
        personal_rating: None,
    }
}

/// A minimal `NormalizedPlayer` with every field at its "absent/zero"
/// default, for `ReplayReportModel::build` tests.
pub(crate) fn base_normalized_player(db_id: i64, relation: Relation, is_self: bool) -> NormalizedPlayer {
    NormalizedPlayer {
        db_id: AccountId(db_id),
        name: format!("Player{db_id}"),
        display_name: format!("Player{db_id}"),
        clan: String::new(),
        clan_color_rgb: 0,
        realm: None,
        division_id: None,
        division_label: None,
        team_id: if is_self { 0 } else { 1 },
        relation,
        is_self,
        is_bot: false,
        is_abuser: false,
        ship_index: "TEST01".to_string(),
        ship_name: "Test Ship".to_string(),
        ship_nation: "USA".to_string(),
        ship_class: Species::Destroyer,
        ship_tier: Some(10),
        is_test_ship: false,
        server_results: None,
        controller_spotting_damage: None,
        controller_potential_damage: None,
        observed_results: ObservedResults { damage: 0, kills: 0 },
        skill_info: SkillInfo { skill_points: 0, num_skills: 0, highest_tier: 0, num_tier_1_skills: 0 },
        build: None,
        achievements: Vec::new(),
        ribbons: Vec::new(),
        consumables: Vec::new(),
        heal_count: None,
        personal_rating: None,
        time_lived_secs: None,
    }
}

/// A fabricated two-player `NormalizedBattleReport`: a self player with
/// server results (nonzero damage, an interaction breakdown) and an enemy
/// player with none, matching the brief's request for a fixture that shows
/// "a breakdown Option populated only when server results exist".
pub(crate) fn fixture_normalized_battle_report() -> NormalizedBattleReport {
    use std::collections::BTreeMap;

    use wows_replay_insights::battle_report::Damage;
    use wows_replay_insights::battle_report::Hits;
    use wows_replay_insights::battle_report::PotentialDamage;
    use wows_replay_insights::battle_report::ServerResults;

    let self_player = NormalizedPlayer {
        server_results: Some(ServerResults {
            xp: Some(1500),
            raw_xp: Some(1200),
            damage: Some(50_000),
            damage_details: Damage { ap: Some(50_000), ..Default::default() },
            damage_by_type: BTreeMap::from([("damage_main_ap".to_string(), 50_000)]),
            hits_details: Hits { ap: Some(12), ..Default::default() },
            hits: Some(12),
            hits_by_type: BTreeMap::from([("hits_main_ap".to_string(), 12)]),
            spotting_damage: Some(8_000),
            potential_damage: 20_000,
            potential_damage_details: PotentialDamage { artillery: 20_000, torpedoes: 0, planes: 0 },
            received_damage: 4_000,
            received_damage_details: Damage { he: Some(4_000), ..Default::default() },
            received_damage_by_type: BTreeMap::from([("damage_main_he".to_string(), 4_000)]),
            fires_dealt: Some(1),
            floods_dealt: Some(0),
            citadels_dealt: Some(1),
            crits_dealt: Some(0),
            distance_traveled: Some(12.5),
            kills: Some(1),
            damage_interactions: HashMap::new(),
        }),
        ..base_normalized_player(1, Relation::new(0), true)
    };

    let enemy_player = base_normalized_player(2, Relation::new(2), false);

    NormalizedBattleReport {
        metadata: MatchMetadata {
            map: "Test Map".to_string(),
            game_mode: "Random Battle".to_string(),
            game_type: "RandomBattle".to_string(),
            match_group: "pvp".to_string(),
            version: Version::default(),
            max_duration: 1200,
            played_duration: Some(600.0),
            extra_duration: None,
            timestamp: jiff::Timestamp::UNIX_EPOCH,
            battle_result: None,
        },
        players: vec![self_player, enemy_player],
    }
}

/// A minimal but valid `ReplayMeta`, for the ignored provider-backed test.
pub(crate) fn fixture_replay_meta() -> ReplayMeta {
    serde_json::from_value(serde_json::json!({
        "matchGroup": "pvp",
        "gameMode": 1,
        "gameType": "RandomBattle",
        "clientVersionFromExe": "0, 0, 0, 0",
        "mapDisplayName": "Test Map",
        "mapId": 1,
        "clientVersionFromXml": "0.0.0",
        "duration": 1200,
        "gameLogic": "RandomBattle",
        "name": "test",
        "scenario": "test",
        "playerID": 1,
        "vehicles": [],
        "playersPerTeam": 1,
        "dateTime": "01.01.2026 00:00:00",
        "mapName": "spaces/test",
        "playerName": "Self",
        "scenarioConfigId": 1,
        "teamsCount": 2,
        "logic": "RandomBattle",
        "playerVehicle": "TestShip",
    }))
    .expect("fixture replay meta should deserialize")
}
