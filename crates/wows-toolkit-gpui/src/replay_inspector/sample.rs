//! Fabricated sample model so the Replay Inspector tab renders a populated
//! table before the real parse pipeline is wired. Replaced by real parsing in
//! a later milestone; nothing here reflects a real replay.

use std::collections::HashMap;

use wows_replay_insights::battle_report::AchievementResult;
use wows_replay_insights::battle_report::ConsumableResult;
use wows_replay_insights::battle_report::DamageInteraction;
use wows_replay_insights::battle_report::RibbonResult;
use wows_replay_insights::battle_report::TranslatedBuild;
use wows_replay_insights::battle_report::TranslatedModule;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::types::AccountId;
use wows_replays::types::GameClock;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
use wowsunpack::game_params::skill_grid_data::SkillGridRow;
use wowsunpack::game_params::skill_grid_data::SkillGridSkill;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::CrewSkillType;
use wowsunpack::game_params::types::SkillPointCost;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::ChargeCount;

use super::columns::ReplayColumn;
use super::model::ChatMessage;
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
        ship_config_url: None,
        short_ship_config_url: None,
        wows_numbers_url: None,
        raw_metadata_json: None,
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
    // A plausible clan-league color, so the sample table demonstrates the
    // Name cell's clan-tag coloring (`ColorRole::Fixed`) even though the
    // fixture has no real `clanColor` to read.
    const SAMPLE_CLAN_COLOR: u32 = 0x4fc3f7;
    PlayerRow {
        display_name: name.to_string(),
        clan_tag: clan.map(|c| format!("[{c}]")),
        clan_color_rgb: if clan.is_some() { SAMPLE_CLAN_COLOR } else { 0 },
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

/// Sample achievements for the self row's expanded Name-column content, so
/// `cargo run` shows the "icon (or fallback text) + name (Nx)" list without
/// needing a real parsed replay.
fn sample_achievements() -> Vec<AchievementResult> {
    vec![
        AchievementResult {
            name: "dfc".to_string(),
            display_name: "Dreadnought".to_string(),
            description: "Deal damage exceeding your ship's hit points in a single battle.".to_string(),
            icon_key: "dfc".to_string(),
            count: 1,
        },
        AchievementResult {
            name: "kraken".to_string(),
            display_name: "Kraken Unleashed".to_string(),
            description: "Destroy 5 or more enemy ships in one battle.".to_string(),
            icon_key: "kraken".to_string(),
            count: 2,
        },
    ]
}

/// Sample ribbons, including a MAIN_CALIBER/BULGE pair so the sample table
/// demonstrates the one-off reorder in `expanded::reorder_bulge_after_main_caliber`.
fn sample_ribbons() -> Vec<RibbonResult> {
    vec![
        RibbonResult {
            name: "RIBBON_MAIN_CALIBER".to_string(),
            display_name: "Main Caliber Hit".to_string(),
            description: "Main battery hit.".to_string(),
            icon_key: "main_caliber".to_string(),
            is_subribbon: false,
            count: 24,
        },
        RibbonResult {
            name: "RIBBON_BULGE".to_string(),
            display_name: "Torpedo Protection Hit".to_string(),
            description: "Hit a torpedo protection bulge.".to_string(),
            icon_key: "bulge".to_string(),
            is_subribbon: false,
            count: 3,
        },
        RibbonResult {
            name: "RIBBON_DESTROYED".to_string(),
            display_name: "Destroyed".to_string(),
            description: "Destroyed an enemy ship.".to_string(),
            icon_key: "destroyed".to_string(),
            is_subribbon: false,
            count: 3,
        },
    ]
}

/// Sample consumables: one unlimited (Damage Control Party), one finite
/// (Repair Party), so the sample table demonstrates both branches of
/// `expanded::consumable_row`'s `ChargeCount` match.
fn sample_consumables() -> Vec<ConsumableResult> {
    vec![
        ConsumableResult {
            display_name: "Damage Control Party".to_string(),
            description: "Instantly extinguishes fires and stops flooding.".to_string(),
            icon_key: "damage_control".to_string(),
            charges_used: 1,
            total_charges: ChargeCount::Unlimited,
        },
        ConsumableResult {
            display_name: "Repair Party".to_string(),
            description: "Restores a portion of the ship's hit points over time.".to_string(),
            icon_key: "repair_party".to_string(),
            charges_used: 1,
            total_charges: ChargeCount::Finite(3),
        },
    ]
}

fn sample_module(name: &str, description: &str, game_params_name: &str) -> TranslatedModule {
    TranslatedModule {
        name: Some(name.to_string()),
        description: Some(description.to_string()),
        game_params_name: game_params_name.to_string(),
    }
}

fn sample_skill(name: &str, internal_name: &str, cost: u8, learned: bool) -> SkillGridSkill {
    SkillGridSkill {
        internal_name: CrewSkillName::from(internal_name),
        skill_type: CrewSkillType::new(0),
        name: Some(name.to_string()),
        description: Some(format!("{name} skill description.")),
        point_cost: Some(SkillPointCost::new(cost)),
        learned,
    }
}

/// A sample captain build: two modernization slots filled and one empty, one
/// signal, a two-module loadout, and a two-tier captain-skill grid with a
/// learned/unlearned skill in each tier, so the sample table demonstrates
/// every branch of `expanded::render_build_section`.
fn sample_translated_build() -> TranslatedBuild {
    TranslatedBuild {
        modernization_slots: vec![
            Some(sample_module("Main Battery Mod 1", "Improves main battery reload.", "PCM001_MainGun_Mod_I")),
            Some(sample_module(
                "Damage Control System Mod 1",
                "Reduces Damage Control Party cooldown.",
                "PCM042_DamageControl_Mod_I",
            )),
            None,
        ],
        signals: vec![sample_module(
            "Zulu Hotel",
            "Increases the chance of your crew earning extra experience points.",
            "PCEF56_XP",
        )],
        loadout: vec![
            sample_module("Hull (B)", "Standard hull upgrade.", "AB1_Hull"),
            sample_module("Engine (B)", "Standard engine upgrade.", "AB2_Engine"),
        ],
        abilities: Vec::new(),
        captain_skills: Some(vec![
            SkillGridRow {
                point_cost: Some(SkillPointCost::new(1)),
                skills: vec![
                    sample_skill("Priority Target", "PriorityTarget", 1, true),
                    sample_skill("Incoming Fire Alert", "IncomingFireAlert", 1, false),
                ],
            },
            SkillGridRow {
                point_cost: Some(SkillPointCost::new(2)),
                skills: vec![
                    sample_skill("Concealment Expert", "ConcealmentExpert", 2, true),
                    sample_skill("Demolition Expert", "DemolitionExpert", 2, false),
                ],
            },
        ]),
    }
}

/// Damage dealt to / received from two enemy rows (`Enemy_Delta`/`Enemy_Echo`,
/// `db_id` 5 and 6 below), so the sample table demonstrates
/// `expanded::render_damage_section`'s per-victim breakdown.
fn sample_damage_interactions() -> HashMap<AccountId, DamageInteraction> {
    HashMap::from([
        (
            AccountId(5),
            DamageInteraction {
                damage_dealt: 42_000,
                damage_dealt_percentage: 56.4,
                damage_received: 12_000,
                damage_received_percentage: 38.1,
                ..Default::default()
            },
        ),
        (
            AccountId(6),
            DamageInteraction {
                damage_dealt: 18_000,
                damage_dealt_percentage: 24.2,
                damage_received: 6_500,
                damage_received_percentage: 20.6,
                ..Default::default()
            },
        ),
    ])
}

/// Eight fabricated players across two teams with varied stats, so the table
/// shows self/ally/enemy coloring, PR tiers, and sort behavior.
pub fn sample_model() -> ReplayReportModel {
    let rows = vec![
        // Division label on the self row, plus achievements/ribbons/damage
        // events/consumables/a captain build/damage interactions, so the
        // sample table demonstrates the full expanded-row content
        // (`expanded.rs`) without needing a real parsed replay.
        PlayerRow {
            division_label: Some("(A)".to_string()),
            achievements: sample_achievements(),
            ribbons: sample_ribbons(),
            consumables: sample_consumables(),
            translated_build: Some(sample_translated_build()),
            fires: Some(2),
            floods: Some(0),
            citadels: Some(1),
            crits: Some(3),
            damage_interactions: Some(sample_damage_interactions()),
            ..stat_row(
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
            )
        },
        // Incoming Fire Alert skill marker demo (Skills cell siren icon).
        PlayerRow {
            has_ifa: true,
            ..stat_row(
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
            )
        },
        // Tower-defense skill-warning demo: the Skills cell forces the "bad"
        // tier color and a warning icon even though this row's raw point
        // count would otherwise land in the "caution" tier.
        PlayerRow {
            division_label: Some("(A)".to_string()),
            is_self_division_mate: true,
            skill_warning: true,
            num_tier_1_skills: 6,
            ..stat_row(
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
            )
        },
        // Dazzle skill marker demo (Skills cell star icon).
        PlayerRow {
            has_dazzle: true,
            ..stat_row(
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
            )
        },
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
        // A test/demo-ship enemy row, so the sample table demonstrates the
        // NDA gating on the damage/hits columns (`should_hide_stats`).
        PlayerRow {
            is_test_ship: true,
            ..stat_row(
                6,
                1,
                Relation::new(2),
                false,
                "Enemy_Echo",
                None,
                "Gearing",
                Species::Destroyer,
                48_900,
                1,
                640.0,
                7,
            )
        },
        // An abuser row, so the sample table demonstrates the Name cell's
        // pink override.
        PlayerRow {
            is_abuser: true,
            ..stat_row(
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
            )
        },
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

    ReplayReportModel {
        self_team: TeamId::from(0i64),
        rows,
        battle_result: None,
        columns: ReplayColumn::ALL.to_vec(),
        map: "Fault Line".to_string(),
        chat: sample_chat_messages(),
    }
}

/// A short chat log covering every rendered variation: a division/team/global
/// message from clanned allies and enemies, a clanless message, and a bot
/// message with no team relation (the gray, untranslated fallback path since
/// the sample has no real `GameMetadataProvider` to translate through).
fn sample_chat_messages() -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            clock: GameClock(12.0),
            sender_relation: Some(Relation::new(0)),
            sender_name: "You".to_string(),
            channel: ChatChannel::Division,
            message: "focus the Yamato".to_string(),
            clan_tag: Some("WTK".to_string()),
            clan_color_rgb: Some(0x3399ff),
        },
        ChatMessage {
            clock: GameClock(45.0),
            sender_relation: Some(Relation::new(1)),
            sender_name: "Ally_Bravo".to_string(),
            channel: ChatChannel::Team,
            message: "cap is contested".to_string(),
            clan_tag: None,
            clan_color_rgb: None,
        },
        ChatMessage {
            clock: GameClock(90.0),
            sender_relation: Some(Relation::new(2)),
            sender_name: "Enemy_Delta".to_string(),
            channel: ChatChannel::Global,
            message: "gg well played".to_string(),
            clan_tag: Some("RED".to_string()),
            clan_color_rgb: Some(0xff3333),
        },
        ChatMessage {
            clock: GameClock(150.0),
            sender_relation: None,
            sender_name: "Bot_Reinforcement".to_string(),
            channel: ChatChannel::System,
            message: "IDS_BATTLE_MESSAGE_REINFORCEMENT".to_string(),
            clan_tag: None,
            clan_color_rgb: None,
        },
    ]
}
