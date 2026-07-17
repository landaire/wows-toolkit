//! Fabricated sample model so the Replay Inspector tab renders a populated
//! table before the real parse pipeline is wired. Replaced by real parsing in
//! a later milestone; nothing here reflects a real replay.

use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
use wowsunpack::game_params::types::Species;

use super::columns::ReplayColumn;
use super::model::PlayerRow;
use super::model::ReplayReportModel;

/// A `PlayerRow` with every field at its absent/zero default, mirroring the
/// test-only `base_row` fixture so the sample rows below can override just the
/// handful of fields that vary.
fn base_row(db_id: i64, team_id: i64, relation: Relation, is_self: bool) -> PlayerRow {
    PlayerRow {
        db_id: AccountId(db_id),
        team_id: TeamId::from(team_id),
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

/// A filled-in row with the stat scalars and formatted strings the collapsed
/// table shows, so cells render with realistic values and colors.
#[allow(clippy::too_many_arguments)]
fn stat_row(
    db_id: i64,
    team_id: i64,
    relation: Relation,
    is_self: bool,
    name: &str,
    clan: Option<&str>,
    ship: &str,
    species: Species,
    damage: u64,
    kills: i64,
    pr: f64,
    skill_points: usize,
) -> PlayerRow {
    let sep = |n: u64| separate(n);
    PlayerRow {
        display_name: name.to_string(),
        clan_tag: clan.map(|c| format!("[{c}]")),
        ship_name: ship.to_string(),
        ship_class: species,
        base_xp: Some(1500),
        base_xp_text: Some(sep(1500)),
        raw_xp: Some(1200),
        raw_xp_text: Some(sep(1200)),
        observed_damage: damage,
        observed_damage_text: sep(damage),
        observed_kills: kills,
        actual_damage: Some(damage),
        actual_damage_text: Some(sep(damage)),
        hits: Some(120),
        hits_text: Some("120".to_string()),
        spotting_damage: Some(damage / 4),
        spotting_damage_text: Some(sep(damage / 4)),
        potential_damage: Some(damage * 3),
        potential_damage_text: Some(sep(damage * 3)),
        received_damage: Some(damage / 2),
        received_damage_text: Some(sep(damage / 2)),
        distance_traveled: Some(42.7),
        kills: Some(kills),
        heal_count: Some(2),
        skill_points,
        num_skills: skill_points / 2,
        skill_label_text: format!("{skill_points}pts ({} skills)", skill_points / 2),
        personal_rating: Some(PersonalRatingResult::new(pr)),
        ..base_row(db_id, team_id, relation, is_self)
    }
}

fn separate(n: u64) -> String {
    super::model::separate_number(n)
}

/// Eight fabricated players across two teams with varied stats, so the table
/// shows self/ally/enemy coloring, PR tiers, and sort behavior.
pub fn sample_model() -> ReplayReportModel {
    let rows = vec![
        stat_row(
            1,
            0,
            Relation::new(0),
            true,
            "You",
            Some("WTK"),
            "Shimakaze",
            Species::Destroyer,
            74_500,
            3,
            1820.0,
            21,
        ),
        stat_row(
            2,
            0,
            Relation::new(1),
            false,
            "Ally_Alpha",
            Some("WTK"),
            "Des Moines",
            Species::Cruiser,
            112_300,
            2,
            2410.0,
            19,
        ),
        stat_row(
            3,
            0,
            Relation::new(1),
            false,
            "Ally_Bravo",
            None,
            "Montana",
            Species::Battleship,
            96_800,
            1,
            1340.0,
            14,
        ),
        stat_row(
            4,
            0,
            Relation::new(1),
            false,
            "Ally_Charlie",
            Some("REL"),
            "Midway",
            Species::AirCarrier,
            61_200,
            0,
            980.0,
            10,
        ),
        stat_row(
            5,
            1,
            Relation::new(2),
            false,
            "Enemy_Delta",
            Some("RED"),
            "Yamato",
            Species::Battleship,
            130_400,
            4,
            2680.0,
            21,
        ),
        stat_row(6, 1, Relation::new(2), false, "Enemy_Echo", None, "Gearing", Species::Destroyer, 48_900, 1, 640.0, 7),
        stat_row(
            7,
            1,
            Relation::new(2),
            false,
            "Enemy_Foxtrot",
            Some("RED"),
            "Zao",
            Species::Cruiser,
            88_100,
            2,
            1560.0,
            17,
        ),
        stat_row(
            8,
            1,
            Relation::new(2),
            false,
            "Enemy_Golf",
            None,
            "Hakuryu",
            Species::AirCarrier,
            70_300,
            3,
            1210.0,
            13,
        ),
    ];

    ReplayReportModel { self_team: TeamId::from(0i64), rows, battle_result: None, columns: ReplayColumn::ALL.to_vec() }
}
