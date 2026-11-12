//! Queries over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::LazyLock;

use serde_json::json;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wows_query::GameData;
use wows_query::QueryError;
use wows_query::aircraft::ShipAircraftArgs;
use wows_query::armor_assessment::ArmorAssessmentArgs;
use wows_query::armor_assessment::ArmorAssessmentOutput;
use wows_query::armor_trace::AmmoTrace;
use wows_query::armor_trace::ArmorTraceArgs;
use wows_query::armor_trace::RayOutcome;
use wows_query::armor_trace::TraceWeapon;
use wows_query::battle_impact::BattleImpactArgs;
use wows_query::citadel::CitadelProfileArgs;
use wows_query::citadel::Turtleback;
use wows_query::commanders::CommanderArgs;
use wows_query::commanders::CommanderSearchArgs;
use wows_query::comparison::CruiserType;
use wows_query::comparison::ShipComparisonArgs;
use wows_query::consumables::ShipConsumablesArgs;
use wows_query::firing_arcs::BatteryArcs;
use wows_query::firing_arcs::FiringArcsArgs;
use wows_query::firing_arcs::ShipFiringArcs;
use wows_query::firing_arcs::Weapon;
use wows_query::game_modes::GameModeBuffsArgs;
use wows_query::healing::HealingSourcesArgs;
use wows_query::hit_locations::ShipHitLocationsArgs;
use wows_query::penetration::Armament;
use wows_query::penetration::DefeatMethod;
use wows_query::penetration::PenetrationSearchArgs;
use wows_query::shells::Ammo;
use wows_query::shells::Battery;
use wows_query::ships::ShipArgs;
use wows_query::ships::ShipClass;
use wows_query::ships::ShipOverviewArgs;
use wows_query::ships::Tier;
use wows_query::skills::CaptainSkillArgs;
use wows_query::skills::SkillTreeArgs;
use wows_query::stat_search::RankBuild;
use wows_query::stat_search::RankLimit;
use wows_query::stat_search::RankOrder;
use wows_query::stat_search::StatNamesArgs;
use wows_query::stat_search::StatRank;
use wows_query::stat_search::StatSearchArgs;
use wows_query::stats::ShipStatsArgs;
use wows_query::tech_tree::Cost;
use wows_query::tech_tree::CostPerPoint;
use wows_query::tech_tree::LineOrder;
use wows_query::tech_tree::ResearchPoints;
use wows_query::tech_tree::ResetCostRank;
use wows_query::tech_tree::TechTreeLinesArgs;
use wows_query::turtleback::TurtlebackSearchArgs;
use wows_query::upgrades::UniqueUpgradesArgs;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::game_params::types::Credits;
use wowsunpack::game_params::types::Experience;
use wowsunpack::game_params::types::GameParamProvider;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
        .expect("dump root has a readable builds.toml");
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

fn game() -> &'static GameData {
    &GAME
}

fn ship(name: &str) -> ShipArgs {
    ShipArgs { ship: name.to_owned() }
}

fn overview(name: &str) -> ShipOverviewArgs {
    ShipOverviewArgs { ship: name.to_owned(), modules: None }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_bow_is_27_mm_above_water() {
    let armor = game().ship_armor(&ship("Des Moines")).expect("Des Moines exists");

    let bow = armor.zones.iter().find(|zone| zone.zone == "Bow").expect("Des Moines has a bow");
    insta::assert_yaml_snapshot!(bow);

    let search = game()
        .penetration_search(&PenetrationSearchArgs {
            target_ship: Some("Des Moines".to_owned()),
            zone: Some("bow".to_owned()),
            ammo: Some(Ammo::Ap),
            armament: Some(vec![Armament::MainBattery]),
            ..Default::default()
        })
        .expect("zone resolves");
    assert_eq!(search.thickness_mm, Millimeters::new(27.0));
    insta::assert_yaml_snapshot!(search);
}

/// SAP needs penetration for Des Moines' 27 mm bow; the 406 mm and larger
/// SAP shells that also overmatch it cannot ricochet off it.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_bow_sap_search_marks_overmatch() {
    let search = game()
        .penetration_search(&PenetrationSearchArgs {
            target_ship: Some("Des Moines".to_owned()),
            zone: Some("bow".to_owned()),
            ammo: Some(Ammo::Sap),
            armament: Some(vec![Armament::MainBattery]),
            ..Default::default()
        })
        .expect("zone resolves");
    assert!(search.groups[0].ships.iter().any(|ship| ship.ends_with(", overmatches")), "{:?}", search.groups[0].ships);
    insta::assert_yaml_snapshot!(search);
}

/// A named shooter gets a verdict per group: GK's 420 mm AP overmatches Des
/// Moines' 27 mm bow, Des Moines' own 203 mm AP does not.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn penetration_search_gives_a_shooter_verdict() {
    let verdicts: Vec<_> = ["GK", "Des Moines"]
        .into_iter()
        .map(|shooter| {
            game()
                .penetration_search(&PenetrationSearchArgs {
                    target_ship: Some("Des Moines".to_owned()),
                    zone: Some("bow".to_owned()),
                    shooter: Some(shooter.to_owned()),
                    armament: Some(vec![Armament::MainBattery]),
                    ..Default::default()
                })
                .expect("ships resolve")
                .groups
                .into_iter()
                .map(|group| group.verdict)
                .collect::<Vec<_>>()
        })
        .collect();
    insta::assert_yaml_snapshot!(verdicts);
}

/// Yamato's superstructure side and roof, and each turret's barbettes, share a
/// game name; their part tells them apart and selects each one.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn yamato_plates_sharing_a_game_name_are_told_apart() {
    let armor = game().ship_armor(&ship("Yamato")).expect("Yamato exists");
    let zones: Vec<_> = armor.zones.iter().filter(|zone| matches!(zone.zone, "Superstructure" | "Turret")).collect();
    insta::assert_yaml_snapshot!(zones);

    let roof = armor
        .zones
        .iter()
        .flat_map(|zone| zone.plates.iter())
        .find(|plate| plate.material.name() == "SS_Top")
        .expect("Yamato has a superstructure roof");
    let search = game()
        .penetration_search(&PenetrationSearchArgs {
            target_ship: Some("Yamato".to_owned()),
            zone: Some(roof.plate.clone()),
            ammo: Some(Ammo::Ap),
            ..Default::default()
        })
        .expect("the roof's name selects it");
    assert_eq!(search.plates, std::slice::from_ref(&roof.plate));
    assert_eq!(Some(&search.thickness_mm), roof.thickness_mm.last());
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn penetration_search_without_ammo_reports_every_method() {
    let search = game()
        .penetration_search(&PenetrationSearchArgs {
            thickness_mm: Some(Millimeters::new(57.0)),
            armament: Some(vec![Armament::MainBattery]),
            ..Default::default()
        })
        .expect("thickness is valid");

    let groups: Vec<_> = search
        .groups
        .iter()
        .map(|group| json!({ "method": group.method, "qualifying": group.qualifying, "considered": group.considered }))
        .collect();
    insta::assert_yaml_snapshot!(groups);
}

/// Tier 10 carriers against Des Moines' 27 mm bow: AP bombs and rockets wider
/// than 386.1 mm overmatch it.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_bow_aircraft_ap_overmatch() {
    let search = game()
        .penetration_search(&PenetrationSearchArgs {
            target_ship: Some("Des Moines".to_owned()),
            zone: Some("bow".to_owned()),
            ammo: Some(Ammo::Ap),
            armament: Some(vec![Armament::Aircraft, Armament::Airstrike]),
            class: Some(ShipClass::AircraftCarrier),
            min_tier: Some(Tier(10)),
            max_tier: Some(Tier(10)),
            ..Default::default()
        })
        .expect("zone resolves");
    insta::assert_yaml_snapshot!(search);
}

/// Hybrid battleships' squadrons count as aircraft and Dutch air strikes as
/// airstrike. Inertia Fuse for HE Shells leaves bomb penetration as it is.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tier_10_battleship_bombs_ignore_ifhe() {
    let search = game()
        .penetration_search(&PenetrationSearchArgs {
            thickness_mm: Some(Millimeters::new(50.0)),
            ammo: Some(Ammo::He),
            ifhe: Some(true),
            armament: Some(vec![Armament::Aircraft, Armament::Airstrike]),
            class: Some(ShipClass::Battleship),
            min_tier: Some(Tier(10)),
            max_tier: Some(Tier(10)),
            ..Default::default()
        })
        .expect("thickness is valid");
    insta::assert_yaml_snapshot!(search.groups);
}

/// Without armament, every armament some ship fires gets its own groups.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn penetration_search_without_armament_reports_each_armament() {
    let search = game()
        .call_tool(
            "penetration_search",
            json!({ "target_ship": "Des Moines", "zone": "bow", "class": "aircraft_carrier", "min_tier": 10, "max_tier": 10 }),
        )
        .expect("zone resolves");
    let groups: Vec<_> = search["groups"]
        .as_array()
        .expect("groups is a list")
        .iter()
        .map(|group| {
            json!({
                "armament": group["armament"],
                "method": group["method"],
                "considered": group["considered"]["total"],
                "qualifying": group["qualifying"]["total"],
                "share_pct": group["share_pct"],
            })
        })
        .collect();
    insta::assert_yaml_snapshot!(groups);
}

/// AP and SAP secondaries against Des Moines' 27 mm bow.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_bow_secondary_ap_and_sap() {
    let search = game()
        .penetration_search(&PenetrationSearchArgs {
            target_ship: Some("Des Moines".to_owned()),
            zone: Some("bow".to_owned()),
            armament: Some(vec![Armament::SecondaryBattery]),
            ..Default::default()
        })
        .expect("zone resolves");
    let groups: Vec<_> = search.groups.iter().filter(|group| group.method != DefeatMethod::HePenetration).collect();
    insta::assert_yaml_snapshot!(groups);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn schlieffen_ifhe_penetration_is_floored_per_battery() {
    let shells = game().ship_shells(&ship("Schlieffen")).expect("Schlieffen exists");

    let pen = |battery: Battery, caliber: Millimeters| {
        let report = shells
            .shells
            .iter()
            .find(|report| {
                report.shell.battery == battery && report.shell.ammo == Ammo::He && report.shell.caliber_mm == caliber
            })
            .expect("HE shell present");
        (report.shell.penetration_mm.map(Millimeters::value), report.shell.ifhe_penetration_mm.map(Millimeters::value))
    };
    assert_eq!(pen(Battery::Main, Millimeters::new(420.0)), (Some(105.0), Some(131.0)));
    assert_eq!(pen(Battery::Secondary, Millimeters::new(150.0)), (Some(38.0), Some(47.0)));
    assert_eq!(pen(Battery::Secondary, Millimeters::new(105.0)), (Some(26.0), Some(32.0)));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ap_overmatch_reaches_the_thickest_whole_mm_plate() {
    let overmatch = |name: &str, caliber: Millimeters| {
        let shells = game().ship_shells(&ship(name)).expect("ship exists");
        shells
            .shells
            .iter()
            .map(|report| &report.shell)
            .find(|shell| shell.battery == Battery::Main && shell.ammo == Ammo::Ap && shell.caliber_mm == caliber)
            .and_then(|shell| shell.overmatches_up_to_mm)
    };
    assert_eq!(overmatch("Tashkent", Millimeters::new(130.0)), Some(Millimeters::new(9.0)));
    assert_eq!(overmatch("Dunkerque", Millimeters::new(330.0)), Some(Millimeters::new(23.0)));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fpe_names_fire_prevention_expert() {
    let skill = game()
        .captain_skill(&CaptainSkillArgs { skill: "FPE".to_owned(), ..Default::default() })
        .expect("FPE resolves");
    let names: Vec<&str> = skill.skills.iter().map(|skill| skill.internal_name.as_str()).collect();
    assert_eq!(names, ["DefenceFireProbability"]);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_card_explains_skills_and_upgrades() {
    let stats = game()
        .ship_stats(&ShipStatsArgs {
            ship: "Schlieffen".to_owned(),
            skills: Some(vec!["IFHE".to_owned(), "Concealment Expert".to_owned()]),
            upgrades: Some(vec!["Main Battery Modification 2".to_owned()]),
            ..Default::default()
        })
        .expect("loadout resolves");

    insta::assert_yaml_snapshot!(stats);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_overview_reports_cost_and_research() {
    let overviews: Vec<_> = ["Des Moines", "Annapolis", "Salem"]
        .into_iter()
        .map(|name| game().ship_overview(&overview(name)).expect("ship exists"))
        .collect();

    insta::assert_yaml_snapshot!(overviews);
}

/// Tier 11 ships cost no experience: GameParams lists `costXP` 0 and the
/// tier 10 ship to research in `requiredShip`.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_overview_reports_purchase_and_research_costs() {
    let costs: Vec<_> = ["Zao", "Kurama", "Montana", "Maine", "Rhode Island", "Sibir"]
        .into_iter()
        .map(|name| {
            let overview = game().ship_overview(&overview(name)).expect("ship exists");
            json!({
                "ship": overview.ship.name,
                "purchase_credits": overview.purchase_credits,
                "research_xp": overview.research_xp,
                "acquisition_note": overview.acquisition_note,
                "researched_from": overview.researched_from,
                "unlocks": overview.unlocks,
                "requires_ship": overview.requires_ship,
            })
        })
        .collect();

    insta::assert_yaml_snapshot!(costs);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_cheapest_by_research_xp() {
    let lines = game()
        .tech_tree_lines(&TechTreeLinesArgs {
            sort_by: Some(LineOrder::ResearchXp),
            limit: Some(5),
            ..Default::default()
        })
        .expect("default tiers are valid");

    insta::assert_yaml_snapshot!(lines);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_usa_battleships_by_credits() {
    let lines = game()
        .tech_tree_lines(&TechTreeLinesArgs {
            nation: Some("USA".to_owned()),
            class: Some(ShipClass::Battleship),
            sort_by: Some(LineOrder::PurchaseCredits),
            ..Default::default()
        })
        .expect("default tiers are valid");

    insta::assert_yaml_snapshot!(lines);
}

/// Both Japanese destroyer lines branch at tier 4 Isokaze, which the carrier
/// line shares.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_japan_destroyers_share_ships() {
    let lines = game()
        .tech_tree_lines(&TechTreeLinesArgs {
            nation: Some("Japan".to_owned()),
            class: Some(ShipClass::Destroyer),
            sort_by: Some(LineOrder::ResearchXp),
            min_tier: Some(Tier(4)),
            ..Default::default()
        })
        .expect("tiers 4 to 10 are valid");

    insta::assert_yaml_snapshot!(lines);
}

/// The reset rank of a line, without its ships.
#[derive(serde::Serialize)]
struct ResetRank {
    tier_10_ship: String,
    class: ShipClass,
    research_points: ResearchPoints,
    credits_per_point: CostPerPoint<Credits>,
    xp_per_point: CostPerPoint<Experience>,
    regrind: Cost,
    #[serde(skip_serializing_if = "Option::is_none")]
    reset_cost: Option<ResetCostRank>,
}

fn reset_ranks(sort_by: LineOrder) -> Vec<ResetRank> {
    let lines = game()
        .tech_tree_lines(&TechTreeLinesArgs { sort_by: Some(sort_by), limit: Some(10), ..Default::default() })
        .expect("default tiers are valid");
    assert_eq!(lines.sort_by, sort_by);

    lines
        .lines
        .into_iter()
        .map(|line| {
            let reset = line.reset.expect("lines sorted by reset carry it");
            ResetRank {
                tier_10_ship: line.tier_10_ship,
                class: line.class,
                research_points: reset.research_points,
                credits_per_point: reset.credits_per_point,
                xp_per_point: reset.xp_per_point,
                regrind: reset.regrind,
                reset_cost: reset.reset_cost,
            }
        })
        .collect()
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_cheapest_to_reset_by_credits_per_point() {
    insta::assert_yaml_snapshot!(reset_ranks(LineOrder::CreditsPerPoint));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_cheapest_to_reset_by_xp_per_point() {
    insta::assert_yaml_snapshot!(reset_ranks(LineOrder::XpPerPoint));
}

/// reset_cost weighs credits and XP per point alike: each line's mean rank
/// by the two, with the formula stated beside the lines.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_cheapest_to_reset_by_reset_cost() {
    let lines = game()
        .tech_tree_lines(&TechTreeLinesArgs { sort_by: Some(LineOrder::ResetCost), ..Default::default() })
        .expect("default tiers are valid");
    assert!(lines.ranking.is_some());
    assert!(lines.lines.iter().all(|line| line.reset.as_ref().is_some_and(|reset| reset.reset_cost.is_some())));
    let summary = lines.summary.as_ref().expect("reset_cost carries a summary");
    assert_eq!(summary.lines.len(), lines.lines.len());
    assert!(summary.lines[0].starts_with(&format!("1. {} (", lines.lines[0].tier_10_ship)));

    insta::assert_yaml_snapshot!(json!({
        "ranking": lines.ranking,
        "summary": lines.summary,
        "lines": reset_ranks(LineOrder::ResetCost),
    }));
}

/// Carrier lines pay fewer Research Points than gunship lines. The Harugumo
/// line branches from the Shimakaze line.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_reset_value_of_named_lines() {
    let lines = game()
        .tech_tree_lines(&TechTreeLinesArgs { include_reset: true, limit: Some(usize::MAX), ..Default::default() })
        .expect("default tiers are valid");
    assert_eq!(lines.sort_by, LineOrder::Nation);

    for end in ["Harugumo", "Shimakaze", "Essex", "Midway"] {
        let named: Vec<_> = lines.lines.iter().filter(|line| line.tier_10_ship == end).collect();
        assert!(!named.is_empty(), "{end} ends a line");
        insta::assert_yaml_snapshot!(format!("tech_tree_lines_reset_{end}"), named);
    }
}

/// Path entries carry their tier and class: a carrier line runs through
/// cruisers and destroyers before its first carrier. The tool leaves the
/// reset out unless it is asked for or sorts the lines.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_carrier_paths_name_tier_and_class() {
    let output = game()
        .call_tool("tech_tree_lines", json!({ "class": "aircraft_carrier", "nation": "USA" }))
        .expect("carrier lines list");
    let lines = output["lines"].as_array().expect("lines is an array");
    assert!(lines.iter().all(|line| line.get("reset").is_none()), "{output:#}");

    let paths: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| {
            json!({
                "tier_10_ship": line["tier_10_ship"],
                "class_starts_at": line["class_starts_at"],
                "path": line["path"],
            })
        })
        .collect();
    insta::assert_yaml_snapshot!(paths);

    let by_reset = game()
        .call_tool("tech_tree_lines", json!({ "class": "aircraft_carrier", "sort_by": "credits_per_point" }))
        .expect("carrier lines list");
    assert!(by_reset["lines"].as_array().is_some_and(|lines| lines.iter().all(|line| line.get("reset").is_some())));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_reject_tier_11() {
    let error = game()
        .tech_tree_lines(&TechTreeLinesArgs { max_tier: Some(Tier(11)), ..Default::default() })
        .expect_err("tier 11 is outside every line");

    assert!(matches!(error, QueryError::InvalidArguments { .. }));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_overview_lists_armament_and_what_is_absent() {
    for name in ["Ragnar", "Gearing", "Yamato", "Midway", "Balao", "Ise"] {
        let overview = game().ship_overview(&overview(name)).expect("ship exists");
        insta::assert_yaml_snapshot!(format!("ship_overview_armament_{name}"), overview.armament);
    }
}

/// Ship-mounted depth charges and the depth charge air strike are reported
/// apart: Zao has no launchers but has the air strike.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_overview_states_each_anti_submarine_weapon() {
    let anti_submarine: Vec<serde_json::Value> = ["Zao", "Des Moines", "Vermont", "Gearing", "Archerfish"]
        .into_iter()
        .map(|name| {
            let overview = game().ship_overview(&overview(name)).expect("ship exists");
            json!({ "ship": overview.ship.name, "anti_submarine": overview.armament.anti_submarine })
        })
        .collect();
    let zao = &anti_submarine[0]["anti_submarine"];
    assert_eq!(zao["depth_charge_launchers"], "none");
    assert!(zao["depth_charge_air_strike"].is_object(), "{zao:#}");

    insta::assert_yaml_snapshot!(anti_submarine);
}

/// Gun Fire Control System Modification 2 is named in the best-fit main
/// battery range, and Spotting Aircraft is an alternative on top of it.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_overview_reports_firing_range_builds() {
    for name in ["Victoria", "Des Moines"] {
        let overview = game().ship_overview(&overview(name)).expect("ship exists");
        let snapshot = format!("ship_overview_firing_range_{}", name.replace(' ', "_"));
        insta::assert_yaml_snapshot!(snapshot, overview.firing_range);
    }
}

/// Ships and the value of each listed shell beside its class standard, from
/// an `ap_shell_traits` call.
fn shell_trait_rows(output: &serde_json::Value) -> Vec<String> {
    output["ships"]
        .as_array()
        .expect("ships is a list")
        .iter()
        .flat_map(|ship| {
            ship["shells"].as_array().into_iter().flatten().map(move |shell| {
                format!(
                    "{}: {}, {}",
                    ship["ship"].as_str().unwrap_or_default(),
                    shell["name"].as_str().unwrap_or_default(),
                    shell["versus_standard"].as_str().unwrap_or_default()
                )
            })
        })
        .collect()
}

/// Destroyers without any anti-submarine weapon are listed apart from the
/// ranked ships the limit leaves out, with the tiers that meet submarines.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_lists_ships_lacking_the_ranked_stat() {
    let output = game()
        .call_tool(
            "stat_search",
            json!({ "rank": { "stat": "asw.best_damage_per_attack", "order": "asc", "limit": 3 }, "class": "destroyer" }),
        )
        .expect("the stat ranks");
    let ranking = &output["ranking"];
    assert!(ranking["omitted"].as_u64().is_some_and(|omitted| omitted > 0), "{ranking:#}");
    assert!(ranking["lacking"]["total"].as_u64().is_some_and(|total| total > 0), "{ranking:#}");

    insta::assert_yaml_snapshot!(json!({
        "ranked": ranking["ranked"],
        "omitted": ranking["omitted"],
        "lacking": ranking["lacking"],
        "note": ranking["note"],
    }));

    // Every class, low tiers listed by name and higher tiers by class: where
    // anti-submarine weapons start.
    let ranking = |min_tier: u32, max_tier: u32, limit: usize| {
        let output = game()
            .call_tool(
                "stat_search",
                json!({
                    "rank": { "stat": "asw.best_damage_per_attack", "order": "desc", "limit": limit },
                    "min_tier": min_tier,
                    "max_tier": max_tier,
                }),
            )
            .expect("the stat ranks");
        output["ranking"].clone()
    };
    let low_tiers = ranking(1, 6, 1);
    let armed_low_tiers = ranking(1, 4, 25);
    let high_tiers = ranking(5, 11, 1);
    let armed: Vec<&serde_json::Value> =
        armed_low_tiers["ships"].as_array().expect("ships").iter().map(|ship| &ship["ship"]).collect();
    insta::assert_yaml_snapshot!(
        "lacking_by_tier_and_class",
        json!({
            "tiers_1_to_6": {
                "by_tier": low_tiers["lacking"]["by_tier"],
                "by_class": low_tiers["lacking"]["by_class"],
                "note": low_tiers["note"],
            },
            "armed_tiers_1_to_4": { "ranked": armed_low_tiers["ranked"], "ships": armed },
            "tiers_5_to_11": {
                "by_tier": high_tiers["lacking"]["by_tier"],
                "by_class": high_tiers["lacking"]["by_class"],
                "ships": high_tiers["lacking"]["ships"],
            },
        })
    );
}

/// Tier 10 ships ranked by anti-submarine weapons, both ends, one call per
/// stat; rows name the weapon.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_anti_submarine_weapons() {
    let ranking = |stat: &str, order: &str| {
        let output = game()
            .call_tool(
                "stat_search",
                json!({ "rank": { "stat": stat, "order": order, "limit": 5 }, "min_tier": 10, "max_tier": 10 }),
            )
            .expect("the stat ranks");
        let rows: Vec<String> = output["ranking"]["ships"]
            .as_array()
            .expect("ranked ships")
            .iter()
            .map(|ship| {
                let row = ship["row"].as_str().map(|row| format!(" ({row})")).unwrap_or_default();
                format!(
                    "{} {}{row}",
                    ship["ship"].as_str().unwrap_or_default(),
                    ship["value"].as_str().unwrap_or_default()
                )
            })
            .collect();
        json!({ "stat": stat, "order": order, "ranked": output["ranking"]["ranked"], "ships": rows })
    };
    let rankings: Vec<serde_json::Value> = [
        ("asw.best_damage_per_attack", "asc"),
        ("asw.best_damage_per_attack", "desc"),
        ("asw.depth_charge.splash_radius", "asc"),
        ("asw.depth_charge.splash_radius", "desc"),
        ("asw.airstrike.splash_radius", "asc"),
        ("asw.airstrike.splash_radius", "desc"),
    ]
    .into_iter()
    .map(|(stat, order)| ranking(stat, order))
    .collect();

    insta::assert_yaml_snapshot!(rankings);
}

/// Two named ships of one class get a factor-by-factor split and a verdict:
/// Yamato leads Amagi on more than twice as many factors; Des Moines and
/// Zao split evenly.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn comparison_head_to_head_gives_a_verdict() {
    let pairs: Vec<serde_json::Value> = [["Yamato", "Amagi"], ["Des Moines", "Zao"]]
        .into_iter()
        .map(|pair| {
            let output =
                game().call_tool("ship_comparison", json!({ "ships": pair })).expect("both ships are of one class");
            json!({ "group": output["group"], "head_to_head": output["head_to_head"] })
        })
        .collect();
    let verdict = |index: usize| pairs[index]["head_to_head"][0]["verdict"].as_str().unwrap_or_default().to_owned();
    assert!(verdict(0).starts_with("On paper Yamato is better"), "{}", verdict(0));
    assert!(verdict(1).starts_with("Mixed:"), "{}", verdict(1));

    insta::assert_yaml_snapshot!(pairs);
}

/// Des Moines' 203 mm AP ricochets from 60 deg instead of the cruiser
/// standard 45; Minotaur's 152 mm AP has a 0.005 s fuse against 0.033 s.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ap_shell_traits_find_improved_angles_and_short_fuses() {
    let improved = game()
        .call_tool(
            "ap_shell_traits",
            json!({ "trait": "improved_ricochet", "class": "cruiser", "min_tier": 8, "max_tier": 10 }),
        )
        .expect("cruisers list");
    let improved_rows = shell_trait_rows(&improved);
    assert!(improved_rows.iter().any(|row| row.starts_with("T10 Des Moines ")), "{improved_rows:#?}");

    let short = game()
        .call_tool("ap_shell_traits", json!({ "trait": "short_fuse", "min_tier": 8, "max_tier": 10 }))
        .expect("ships list");
    let short_rows = shell_trait_rows(&short);
    assert!(short_rows.iter().any(|row| row.starts_with("T10 Minotaur ")), "{short_rows:#?}");
    // A 0.025 s cruiser fuse is slightly short, not short.
    assert!(short_rows.iter().all(|row| !row.contains("fuse 0.025 s")), "{short_rows:#?}");

    let slightly_short = game()
        .call_tool("ap_shell_traits", json!({ "trait": "slightly_short_fuse", "min_tier": 8, "max_tier": 10 }))
        .expect("ships list");

    insta::assert_yaml_snapshot!(json!({
        "standards": improved["standards"],
        "fuse_threshold_caliber_ratio": improved["fuse_threshold_caliber_ratio"],
        "improved_ricochet": {
            "considered": improved["considered"],
            "qualifying": improved["qualifying"],
            "ships": improved_rows,
        },
        "short_fuse": {
            "considered": short["considered"],
            "qualifying": short["qualifying"],
            "omitted": short["omitted"],
            "ships": short_rows,
        },
        "slightly_short_fuse": {
            "qualifying": slightly_short["qualifying"],
            "omitted": slightly_short["omitted"],
        },
    }));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn penetration_search_states_shares_per_class() {
    // Through call_tool, which prints each f32 share at its own precision.
    let search = game()
        .call_tool(
            "penetration_search",
            json!({ "target_ship": "Des Moines", "zone": "bow", "ammo": "AP", "armament": ["main_battery"], "min_tier": 8, "max_tier": 11 }),
        )
        .expect("zone resolves");
    let shares: Vec<_> = search["groups"]
        .as_array()
        .expect("groups is a list")
        .iter()
        .map(|group| {
            json!({
                "considered": group["considered"],
                "qualifying": group["qualifying"],
                "share_pct": group["share_pct"],
                "share_pct_by_class": group["share_pct_by_class"],
            })
        })
        .collect();

    insta::assert_yaml_snapshot!(shares);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn other_modules_select_another_armament() {
    let selected: Vec<_> = ["Gearing", "Cleveland"]
        .into_iter()
        .map(|name| {
            let stock = game().ship_overview(&overview(name)).expect("ship exists");
            let module = stock.armament.other_modules.first().expect("the ship has another module").name.clone();
            let args = ShipOverviewArgs { ship: name.to_owned(), modules: Some(vec![module.clone()]) };
            match game().ship_overview(&args) {
                Ok(chosen) => json!({ "ship": name, "module": module, "armament": chosen.armament }),
                Err(error) => json!({ "ship": name, "module": module, "error": error.to_string() }),
            }
        })
        .collect();

    insta::assert_yaml_snapshot!(selected);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn super_names_resolve_to_the_tier_11_successor() {
    let resolved: Vec<_> = ["Super Colombo", "super-montana", "SUPER DM", "Super Stalingrad", "Super Yamato"]
        .into_iter()
        .map(|query| match game().ship_summary(query) {
            Ok(resolved) => {
                json!({ "query": query, "ship": resolved.ship.name, "interpretations": resolved.interpretations })
            }
            Err(error) => json!({ "query": query, "error": error.to_string() }),
        })
        .collect();
    let names = |arguments: serde_json::Value| -> serde_json::Value {
        let found = game().call_tool("find_ships", arguments).expect("find_ships answers");
        let ships: Vec<_> = found["ships"]
            .as_array()
            .expect("ships is a list")
            .iter()
            .take(3)
            .map(|ship| json!({ "name": ship["name"], "name_match": ship["name_match"] }))
            .collect();
        json!({ "interpretations": found["interpretations"], "total": found["total"], "first": ships })
    };

    insta::assert_yaml_snapshot!(json!({
        "resolved": resolved,
        "tier_11_cruisers": names(json!({ "min_tier": "supership", "class": "CA" })),
        "find_super_colombo": names(json!({ "name": "Super Colombo" })),
        "find_super_colombo_up_to_tier_10": names(json!({ "name": "Super Colombo", "max_tier": 10 })),
    }));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn upgrades_and_consumables_of_des_moines() {
    let upgrades = game().ship_upgrades(&ship("Des Moines")).expect("Des Moines exists");
    insta::assert_yaml_snapshot!(upgrades);

    let consumables = game()
        .ship_consumables(&ShipConsumablesArgs {
            ship: "Des Moines".to_owned(),
            skills: Some(vec!["SI".to_owned()]),
            ..Default::default()
        })
        .expect("loadout resolves");
    insta::assert_yaml_snapshot!(consumables);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn per_class_skill_names_resolve_by_class() {
    let submarine = game()
        .captain_skill(&CaptainSkillArgs {
            skill: "Adrenaline Rush".to_owned(),
            class: Some(ShipClass::Submarine),
            ..Default::default()
        })
        .expect("submarines have Adrenaline Rush");
    let names: Vec<&str> = submarine.skills.iter().map(|skill| skill.internal_name.as_str()).collect();
    assert_eq!(names, ["ArmamentReloadSubmarine"]);

    let ifhe = game()
        .captain_skill(&CaptainSkillArgs { skill: "ifhe".to_owned(), ..Default::default() })
        .expect("IFHE exists");
    insta::assert_yaml_snapshot!(ifhe);
}

/// Player abbreviations of captain skills on a battleship, as shiptool_link
/// resolves them: IFA is Incoming Fire Alert, never Inertia Fuse for HE Shells.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn skill_abbreviations_resolve_to_their_skills() {
    let abbreviations = [
        "IFA", "IFHE", "CE", "SE", "AR", "BFT", "AFT", "FP", "FPE", "PM", "PT", "DE", "CQC", "MSBA", "MFCSA", "SI",
        "BoS", "RL", "RPF", "LS", "GtG", "SHAP", "EM", "PA",
    ];
    let resolved: Vec<String> = abbreviations
        .into_iter()
        .map(|abbreviation| {
            let resolution = game().captain_skill(&CaptainSkillArgs {
                skill: abbreviation.to_owned(),
                ship: Some("Vermont".to_owned()),
                ..Default::default()
            });
            match resolution {
                Ok(output) => {
                    let names: Vec<&str> = output.skills.iter().map(|skill| skill.name.as_str()).collect();
                    format!("{abbreviation} -> {}", names.join(", "))
                }
                Err(error) => format!("{abbreviation} -> error: {error}"),
            }
        })
        .collect();
    assert!(resolved.contains(&"IFA -> Incoming Fire Alert".to_owned()), "{resolved:#?}");

    insta::assert_yaml_snapshot!(resolved);
}

/// Adrenaline Rush reduces reload by its coefficient per 1% of health lost.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn adrenaline_rush_reloads_faster_per_percent_of_health_lost() {
    let skill = game()
        .captain_skill(&CaptainSkillArgs { skill: "Adrenaline Rush".to_owned(), ..Default::default() })
        .expect("Adrenaline Rush exists");
    insta::assert_yaml_snapshot!(skill);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn colbert_with_adrenaline_rush_at_low_and_half_health() {
    let at = |hp_percent: f32| {
        game()
            .ship_stats(&ShipStatsArgs {
                ship: "Colbert".to_owned(),
                skills: Some(vec!["Adrenaline Rush".to_owned()]),
                hp_percent: Some(Percent::new(hp_percent)),
                ..Default::default()
            })
            .expect("loadout resolves")
    };

    insta::assert_yaml_snapshot!("colbert_adrenaline_rush_at_1_percent_hp", at(1.0));
    insta::assert_yaml_snapshot!("colbert_adrenaline_rush_at_50_percent_hp", at(50.0));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn conflicting_upgrades_are_rejected() {
    let error = game()
        .ship_stats(&ShipStatsArgs {
            ship: "Des Moines".to_owned(),
            upgrades: Some(vec!["Main Battery Modification 2".to_owned(), "Aiming Systems Modification 1".to_owned()]),
            ..Default::default()
        })
        .expect_err("both use slot 3");

    insta::assert_snapshot!(error.to_string());
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn registry_calls_report_each_failure_kind() {
    let armor = game().call_tool("ship_armor", json!({ "ship": "Des Moines" })).expect("Des Moines exists");
    assert_eq!(armor["ship"]["name"], "Des Moines");

    let failures = [
        game().call_tool("ship_armour", json!({ "ship": "Des Moines" })),
        game().call_tool("ship_armor", json!({ "name": "Des Moines" })),
        game().call_tool("ship_armor", json!({ "ship": "Des Moines", "zone": "Bow" })),
        game().call_tool("ship_armor", json!({ "ship": "Nonexistent Ship" })),
    ];
    let messages: Vec<String> =
        failures.into_iter().map(|failure| failure.expect_err("call fails").to_string()).collect();
    insta::assert_yaml_snapshot!(messages);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn misspelled_names_resolve_or_suggest() {
    let resolved = |query: &str| game().ship_summary(query).map(|resolved| resolved.ship.name);
    assert_eq!(resolved("Sekryu").expect("one typo"), "Sekiryu");
    assert_eq!(resolved("Des Moins").expect("one typo"), "Des Moines");
    assert_eq!(resolved("Kurfurts").expect("one transposition"), "G. Kurf\u{fc}rst");

    assert_eq!(resolved("Kitakazee").expect("whole name beats a word of a longer name"), "Kitakaze");

    assert_eq!(resolved("Smolensker").expect("an original beats its replica Smolensk B"), "Smolensk");
    let skill = game()
        .captain_skill(&CaptainSkillArgs { skill: "Concealment Expertt".to_owned(), ..Default::default() })
        .map(|output| output.skills.into_iter().map(|skill| skill.name).collect::<Vec<_>>());
    let upgrade = game()
        .upgrade(&wows_query::upgrades::UpgradeArgs { upgrade: "Main Batery Modification 2".to_owned(), ship: None })
        .map(|output| output.upgrades.into_iter().map(|upgrade| upgrade.name).collect::<Vec<_>>());
    insta::assert_yaml_snapshot!((skill.expect("one typo"), upgrade.expect("one typo")));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tier_prefixes_must_match_the_ship() {
    let resolved: Vec<String> = [
        "T10 Fuyutsuki",
        "t8 Edinburgh",
        "Tier 9 ZH-1",
        "tier10 DM",
        "T11 Super Montana",
        "Tier X Yamato",
        "T9 Fuyutsuki",
        "T12 Yamato",
    ]
    .into_iter()
    .map(|query| match game().ship_summary(query) {
        Ok(resolved) => format!("{query} -> T{} {}", resolved.ship.tier.0, resolved.ship.name),
        Err(error) => format!("{query} -> {error}"),
    })
    .collect();

    insta::assert_yaml_snapshot!(resolved);

    let found = |name: &str| -> Vec<String> {
        let found =
            game().find_ships(&wows_query::ships::FindShipsArgs { name: Some(name.to_owned()), ..Default::default() });
        found.ships.into_iter().map(|found| format!("T{} {}", found.ship.tier.0, found.ship.name)).collect()
    };
    assert_eq!(found("T10 Fuyutsuki"), ["T10 Fuyutsuki"]);
    assert!(found("T9 Fuyutsuki").is_empty());
}

/// Names several ships share read as the ship players own; qualifiers, tiers
/// and indexes pick the others, which the interpretation lists.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn shared_names_rank_the_owned_ship_first() {
    let resolved: Vec<serde_json::Value> = [
        "Schlieffen",
        "Schlieffen co-op",
        "PGSB920",
        "Essex",
        "Essex (old)",
        "old Essex",
        "T9 Essex",
        "Zao",
        "[Zao]",
        "clan Zao",
        "Vrijheid",
        "test Vrijheid",
        "Kitakami",
        "old Kitakami",
        "BA Montana",
        "Satsuma +",
        "Test Sub 1",
        "test sub",
        "old Montana",
        "T8 old Baltimore",
    ]
    .into_iter()
    .map(|query| match game().ship_summary(query) {
        Ok(resolved) => json!({
            "query": query,
            "ship": format!("T{} {} ({}, {:?})", resolved.ship.tier.0, resolved.ship.name, resolved.ship.index, resolved.ship.category),
            "interpretations": resolved.interpretations,
        }),
        Err(error) => json!({ "query": query, "error": error.to_string() }),
    })
    .collect();

    insta::assert_yaml_snapshot!(resolved);
}

/// find_ships lists the named ship, then the ships sharing its name that the
/// filters admit.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn find_ships_lists_variants_after_the_named_ship() {
    let found = |name: &str, include_event_variants: bool| -> Vec<String> {
        let found = game().find_ships(&wows_query::ships::FindShipsArgs {
            name: Some(name.to_owned()),
            include_event_variants: Some(include_event_variants),
            ..Default::default()
        });
        found
            .ships
            .into_iter()
            .map(|found| {
                format!(
                    "{name} {include_event_variants}: T{} {} ({}, {:?}, {:?})",
                    found.ship.tier.0, found.ship.name, found.ship.index, found.ship.category, found.name_match
                )
            })
            .collect()
    };
    let listed: Vec<String> =
        [("Essex", false), ("Essex", true), ("old Essex", false), ("Zao", true), ("[Zao]", false)]
            .into_iter()
            .flat_map(|(name, include)| found(name, include))
            .collect();

    insta::assert_yaml_snapshot!(listed);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn full_names_resolve_to_abbreviated_ships() {
    let resolved: Vec<(&str, String)> = [
        "Grosser Kurfurst",
        "Gro\u{df}er Kurf\u{fc}rst",
        "Friedrich der Grosse",
        "Dmitri Donskoi",
        "Dmitry Donskoy",
        "Dmitri Pozharsky",
        "Dmitry Pozharsky",
        "Attilio Regolo",
        "Francesco Ferruccio",
        "Forrest Sherman",
        "Prinz Eitel Friedrich",
        "Friedrich der Grose",
    ]
    .into_iter()
    .map(|query| {
        let name = game().ship_summary(query).map(|resolved| resolved.ship.name);
        (query, name.unwrap_or_else(|error| error.to_string()))
    })
    .collect();
    insta::assert_yaml_snapshot!(resolved);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn sekiryu_is_indexed() {
    let found =
        game().find_ships(&wows_query::ships::FindShipsArgs { name: Some("Seki".to_owned()), ..Default::default() });
    let names: Vec<(&str, &str, u32)> = found
        .ships
        .iter()
        .map(|found| (found.ship.name.as_str(), found.ship.index.as_str(), found.ship.tier.0))
        .collect();
    insta::assert_yaml_snapshot!(names);
}

/// A named ship the filters leave out is not reported as read.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn find_ships_names_no_ship_the_filters_leave_out() {
    let found = game().find_ships(&wows_query::ships::FindShipsArgs {
        name: Some("DM".to_owned()),
        nation: Some("Russia".to_owned()),
        ..Default::default()
    });

    assert!(found.interpretations.is_empty(), "{:?}", found.interpretations);
    assert!(found.ships.iter().all(|found| found.name_match == Some(wows_query::ships::NameMatch::Partial)));
    assert!(found.ships.iter().any(|found| found.ship.name == "Dm. Donskoi"));
}

/// "DM" is an alias of Des Moines although "Dm. Donskoi" contains it.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn find_ships_lists_the_named_ship_first() {
    let found: Vec<_> = ["DM", "GK"]
        .into_iter()
        .map(|name| {
            let found = game()
                .find_ships(&wows_query::ships::FindShipsArgs { name: Some(name.to_owned()), ..Default::default() });
            let ships: Vec<_> = found
                .ships
                .iter()
                .map(|found| json!({ "ship": found.ship.name, "name_match": found.name_match }))
                .collect();
            json!({ "name": name, "interpretations": found.interpretations, "ships": ships })
        })
        .collect();
    insta::assert_yaml_snapshot!(found);
}

/// The penetration builds of a ship's HE shells, by battery and caliber.
fn he_penetration(name: &str) -> Vec<serde_json::Value> {
    let shells = game().ship_shells(&ship(name)).expect("ship exists");
    shells
        .shells
        .iter()
        .filter(|report| report.shell.ammo == Ammo::He)
        .map(|report| json!({ "battery": report.shell.battery, "caliber_mm": report.shell.caliber_mm, "penetration": report.penetration }))
        .collect()
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn conqueror_main_battery_he_penetration_leaves_out_ifhe() {
    insta::assert_yaml_snapshot!(he_penetration("Conqueror"));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_he_penetration_lists_ifhe() {
    insta::assert_yaml_snapshot!(he_penetration("DM"));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn he_penetration_notes_name_battery_and_plating() {
    for name in ["Hannover", "Schlieffen"] {
        insta::assert_yaml_snapshot!(format!("he_penetration_notes_{name}"), he_penetration(name));
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn smaland_concealment_with_and_without_concealment_expert() {
    let concealment = game().ship_concealment(&ship("Smaland")).expect("Smaland exists");
    insta::assert_yaml_snapshot!(concealment);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn moskva_radar_on_its_best_build() {
    let consumables = game()
        .ship_consumables(&ShipConsumablesArgs {
            ship: "Moskva".to_owned(),
            consumable: Some("radar".to_owned()),
            ..Default::default()
        })
        .expect("Moskva has radar");
    insta::assert_yaml_snapshot!(consumables);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_weapon_stats_on_their_best_builds() {
    let stats = game()
        .ship_stats(&ShipStatsArgs { ship: "Des Moines".to_owned(), ..Default::default() })
        .expect("Des Moines exists");
    insta::assert_yaml_snapshot!(stats.build_stats);
}

/// Improved Rangefinders, C. Colombo's unique upgrade in slot 6, narrows
/// dispersion on top of Aiming Systems Modification 1 in slot 3; a player
/// without it has nothing in slot 6 that does.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn colombo_dispersion_with_and_without_its_unique_upgrade() {
    let stats = game()
        .ship_stats(&ShipStatsArgs { ship: "C. Colombo".to_owned(), ..Default::default() })
        .expect("C. Colombo exists");
    let dispersion = stats.build_stats.and_then(|builds| builds.main_battery_dispersion).expect("C. Colombo has guns");

    let best = &dispersion.builds.best;
    assert!(best.assumed.iter().any(|item| item == "Improved Rangefinders (unique upgrade)"), "{best:?}");
    assert!(
        dispersion
            .builds
            .alternatives
            .iter()
            .any(|alternative| alternative.label == "without unique upgrade Improved Rangefinders"
                && alternative.value > best.value),
        "{dispersion:?}"
    );
    insta::assert_yaml_snapshot!(dispersion);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn unique_upgrades_and_the_ships_they_fit() {
    let unique = game().unique_upgrades(&UniqueUpgradesArgs::default());
    assert!(unique.upgrades.iter().any(|upgrade| upgrade.param == "PCM106_Special_Mod_I_Colombo"));
    insta::assert_yaml_snapshot!(unique);

    let colombo = game().ship_upgrades(&ship("C. Colombo")).expect("C. Colombo exists");
    insta::assert_yaml_snapshot!(colombo.unique_upgrades);
}

/// Quoted numbers traced to the stat and build that give them: Shimakaze's
/// HP with Survivability Expert, Des Moines' range with Gun Fire Control
/// System Modification 2 and its best AP damage per minute, Yamato's stock
/// rudder shift rounded to a decimal, and a number no build gives.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn explain_stat_value_finds_the_build_behind_a_number() {
    let explain =
        |arguments: serde_json::Value| game().call_tool("explain_stat_value", arguments).expect("the tool runs");

    insta::assert_yaml_snapshot!(
        "explain_stat_value_shimakaze_hp",
        explain(json!({ "ship": "Shimakaze", "value": 21400, "unit": "hp" }))
    );
    insta::assert_yaml_snapshot!(
        "explain_stat_value_des_moines_range",
        explain(json!({ "ship": "Des Moines", "value": 18.36 }))
    );
    insta::assert_yaml_snapshot!(
        "explain_stat_value_yamato_rudder",
        explain(json!({ "ship": "Yamato", "value": 22.1, "stat": "rudder" }))
    );
    insta::assert_yaml_snapshot!(
        "explain_stat_value_des_moines_dpm",
        explain(json!({ "ship": "Des Moines", "value": 585744 }))
    );
    insta::assert_yaml_snapshot!(
        "explain_stat_value_no_match",
        explain(json!({ "ship": "Des Moines", "value": 77777, "unit": "hp" }))
    );
}

/// Each ship's unique upgrade as ship_upgrades reports it, by the names
/// players use.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn unique_upgrades_are_found_for_their_ships() {
    let reports: Vec<serde_json::Value> =
        ["Minotaur", "G. Kurfürst", "GK", "Des Moines", "Kléber", "Kleber", "Harugumo"]
            .into_iter()
            .map(|name| {
                let upgrades = game().call_tool("ship_upgrades", json!({ "ship": name })).expect("ship exists");
                json!({
                    "name": name,
                    "ship": upgrades["ship"]["name"],
                    "index": upgrades["ship"]["index"],
                    "unique_upgrades": upgrades["unique_upgrades"],
                })
            })
            .collect();
    insta::assert_yaml_snapshot!(reports);

    // Nations as players write them reach the GameParams spelling.
    for nation in ["UK", "United Kingdom", "British", "United_Kingdom"] {
        let unique = game().unique_upgrades(&UniqueUpgradesArgs {
            class: Some(ShipClass::Cruiser),
            min_tier: Some(Tier(10)),
            max_tier: Some(Tier(10)),
            nation: Some(nation.to_owned()),
        });
        assert!(
            unique.upgrades.iter().any(|upgrade| upgrade.param == "PCM051_Special_Mod_I_Minotaur"),
            "{nation}: {unique:?}"
        );
    }
}

/// Every tier 10 ship of each scored class by battle impact, with its
/// component scores.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn battle_impact_tier_10_rankings() {
    for class in [ShipClass::Cruiser, ShipClass::Battleship, ShipClass::Destroyer] {
        let output = game()
            .battle_impact(&BattleImpactArgs {
                class: Some(class),
                tier: Some(Tier(10)),
                limit: Some(usize::MAX),
                ..Default::default()
            })
            .expect("tier 10 ships score");
        assert_eq!(output.ships.len(), output.ranked);
        assert!(output.lowest.is_empty());
        if class == ShipClass::Battleship {
            let position = |prefix: &str| {
                output
                    .ships
                    .iter()
                    .position(|ship| ship.ship.starts_with(prefix))
                    .unwrap_or_else(|| panic!("{prefix} is ranked"))
            };
            let libertad = position("T10 Libertad ");
            let colombo = position("T10 C. Colombo ");
            let schlieffen = position("T10 Schlieffen ");
            let kurfurst = position("T10 G. Kur");
            let bottom_five = output.ranked - 5;

            assert!(libertad < colombo);
            assert!(colombo < 10);
            assert!(schlieffen >= bottom_five, "Schlieffen position {} of {}", schlieffen + 1, output.ranked);
            assert!(kurfurst >= bottom_five, "G. Kurfurst position {} of {}", kurfurst + 1, output.ranked);
        }
        let lines: Vec<String> = output
            .ships
            .iter()
            .map(|ship| {
                let components = serde_json::to_value(&ship.components).expect("components serialize");
                let scores: Vec<String> = components
                    .as_object()
                    .expect("components is a map")
                    .iter()
                    .map(|(name, component)| {
                        format!("{name} {:.1}", component["score"].as_f64().expect("scores are numbers"))
                    })
                    .collect();
                format!("{}. {} {}: {}", ship.rank, ship.ship, ship.score.value(), scores.join(", "))
            })
            .collect();
        insta::assert_yaml_snapshot!(
            format!("battle_impact_tier_10_{}", output.group.trim_start_matches("tier 10 ")),
            json!({ "weights": output.weights, "ships": lines })
        );
    }
}

/// The battle_impact tool answer for one named ship outside the top.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn battle_impact_lists_a_named_ship() {
    let args = json!({ "class": "battleship", "tier": 10, "ships": ["Conqueror"], "limit": 3 });
    let output = game().call_tool("battle_impact", args).expect("tier 10 battleships score");
    let ships = output["ships"].as_array().expect("ships listed");
    let named = |ship: &serde_json::Value| ship["ship"].as_str().is_some_and(|name| name.contains("Conqueror"));
    assert!(ships.iter().any(named), "{output:#}");
    insta::assert_yaml_snapshot!(output);

    let carriers = game().call_tool("battle_impact", json!({ "class": "carrier", "tier": 10 }));
    assert!(carriers.is_err());
}

/// Incomparable, C. Colombo with its unique upgrade and Togasa, in testing,
/// ranked among every tier 10 battleship.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tier_10_battleship_comparison() {
    let comparison = game()
        .ship_comparison(&ShipComparisonArgs {
            class: Some(ShipClass::Battleship),
            tier: Some(Tier(10)),
            ships: Some(vec!["Incomparable".to_owned(), "C. Colombo".to_owned(), "T\u{14d}gasa".to_owned()]),
            ..Default::default()
        })
        .expect("tier 10 battleships compare");
    assert_eq!(comparison.ships.len(), 3);
    let colombo = &comparison.ships[1];
    assert!(colombo.unique_upgrade.as_deref().is_some_and(|upgrade| upgrade.contains("dispersion")), "{colombo:?}");
    insta::assert_yaml_snapshot!(comparison);
}

/// Chateaurenault's torpedo loses half its damage past 8 km, which only the
/// per-torpedo factors show; destroyers get no armor factors.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn chateaurenault_against_tier_10_destroyers() {
    let comparison = game()
        .ship_comparison(&ShipComparisonArgs {
            class: Some(ShipClass::Destroyer),
            tier: Some(Tier(10)),
            ships: Some(vec!["Chateaurenault".to_owned(), "Shimakaze".to_owned(), "Smaland".to_owned()]),
            ..Default::default()
        })
        .expect("tier 10 destroyers compare");
    let chateaurenault = &comparison.ships[0];
    let torpedo = chateaurenault.torpedoes.first().expect("Chateaurenault carries torpedoes");
    assert!(torpedo.damage_by_distance.as_deref().is_some_and(|falloff| falloff.contains("50% from")), "{torpedo:?}");
    assert!(comparison.ships.iter().all(|ship| ship.matchups.is_empty()));
    assert!(comparison.factors.iter().all(|factor| !factor.starts_with("bb_ap") && !factor.contains("overmatch")));
    insta::assert_yaml_snapshot!(comparison);
}

/// Defence's torpedo tubes are fixed facing the bow and its torpedoes turn up
/// to 65 deg after launch, so its launchers bear on targets off the bow.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn defence_torpedoes_turn_after_launch() {
    let stats = game().call_tool("ship_stats", json!({ "ship": "Defence" })).expect("Defence exists");
    let torpedoes = &stats["build_stats"]["torpedoes_per_minute"];
    let best = torpedoes["rate"]["builds"]["best"]["value"]["torpedoes_per_minute"]
        .as_f64()
        .expect("Defence launches torpedoes");
    assert!(best > 0.0, "{torpedoes:#}");

    let damage: Vec<serde_json::Value> = stats["build_stats"]["torpedo_damage"]
        .as_array()
        .expect("torpedo damage listed")
        .iter()
        .map(|ammo| json!({ "ammo": ammo["ammo"], "rates": ammo["rates"]["builds"] }))
        .collect();
    insta::assert_yaml_snapshot!(json!({ "torpedoes_per_minute": torpedoes, "torpedo_damage": damage }));
}

/// Des Moines' plating ranks among tier 10 heavy cruisers as well as among
/// every tier 10 cruiser, and a majority of battleships overmatching it reads
/// as a majority.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_overmatch_ranks_among_heavy_cruisers() {
    let comparison = game()
        .ship_comparison(&ShipComparisonArgs {
            class: Some(ShipClass::Cruiser),
            tier: Some(Tier(10)),
            ships: Some(vec!["Des Moines".to_owned()]),
            ..Default::default()
        })
        .expect("tier 10 cruisers compare");
    let des_moines = &comparison.ships[0];
    assert_eq!(des_moines.cruiser_type, Some(CruiserType::Heavy));

    let text = serde_json::to_value(&des_moines.values).expect("values serialize");
    let ends: Vec<serde_json::Value> = ["bow_overmatch", "stern_overmatch", "bb_ap_citadel"]
        .into_iter()
        .filter_map(|factor| {
            let standing = des_moines.standings.iter().find(|standing| standing.factor == factor)?;
            let within = standing.type_rank.expect("cruiser groups rank within the type");
            Some(json!({
                "factor": factor,
                "text": text[factor],
                "heavy_cruisers_better_pct": within.share_better().map(|share| format!("{:.1}", share.value())),
            }))
        })
        .collect();
    assert!(ends.iter().all(|end| end["text"].as_str().is_some_and(|shown| shown.contains("heavy cruisers rank"))));

    insta::assert_yaml_snapshot!(
        json!({ "ends": ends, "signals": des_moines.signals, "outliers": des_moines.outliers })
    );
}

/// Only values whose best-fit build takes a unique upgrade say so. Colbert
/// and Irresistible have none; Harugumo has one.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn comparison_marks_unique_upgrades_only_where_taken() {
    let entry = |class: &str, ship: &str| {
        let output = game()
            .call_tool("ship_comparison", json!({ "class": class, "tier": 10, "ships": [ship] }))
            .expect("group compares");
        (output["ships"][0].clone(), output["leaders"].clone())
    };
    let marked_leaders = |leaders: &serde_json::Value| -> Vec<String> {
        leaders
            .as_object()
            .expect("leaders is a map")
            .values()
            .flat_map(|leader| ["best", "worst"].map(|end| leader[end].clone()))
            .flat_map(|labels| labels.as_array().cloned().unwrap_or_default())
            .filter_map(|label| label.as_str().map(str::to_owned))
            .filter(|label| label.contains("with unique upgrade"))
            .collect()
    };
    for (class, ship) in [("cruiser", "Colbert"), ("battleship", "Irresistible")] {
        let (entry, leaders) = entry(class, ship);
        assert!(!entry.to_string().contains("unique"), "{entry:#}");
        let own = format!("{ship} (");
        assert!(marked_leaders(&leaders).iter().all(|label| !label.starts_with(&own)), "{leaders:#}");
    }

    let (harugumo, leaders) = entry("destroyer", "Harugumo");
    let marked_values: Vec<String> = harugumo["values"]
        .as_object()
        .expect("values is a map")
        .iter()
        .filter(|(_, text)| text.as_str().is_some_and(|text| text.contains("with unique upgrade")))
        .map(|(key, text)| format!("{key}: {}", text.as_str().unwrap_or_default()))
        .collect();
    let marked_leaders = marked_leaders(&leaders);
    insta::assert_yaml_snapshot!(json!({
        "unique_upgrade": harugumo["unique_upgrade"],
        "marked_values": marked_values,
        "marked_leaders": marked_leaders,
    }));
}

/// The ship_comparison tool output fits a small model's context: a group of
/// one class and tier, with one ship named and with none.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn comparison_output_stays_bounded() {
    let mut sizes = Vec::new();
    for (class, named) in [
        ("cruiser", Some("Sevastopol")),
        ("cruiser", None),
        ("battleship", Some("Yamato")),
        ("battleship", None),
        ("destroyer", Some("Shimakaze")),
        ("destroyer", None),
    ] {
        let mut args = json!({ "class": class, "min_tier": 10, "max_tier": 10 });
        if let Some(ship) = named {
            args["ships"] = json!([ship]);
        }
        let output = game().call_tool("ship_comparison", args).expect("group compares");
        let chars = serde_json::to_string(&output).expect("output serializes").len();
        let limit = if named.is_some() { 26_000 } else { 30_000 };
        assert!(chars < limit, "tier 10 {class} named {named:?}: {chars} chars, over {limit}");
        sizes.push(format!("tier 10 {class} named {named:?}: {chars} chars, about {} tokens", chars * 2 / 7));
        if class == "destroyer" && named.is_none() {
            assert!(output.get("ships").is_none(), "{output:#}");
            insta::assert_yaml_snapshot!("tier_10_destroyer_comparison_summary", output);
        }
    }
    println!("{sizes:#?}");

    // The library keeps every ship of a group compared without names.
    let full = game()
        .ship_comparison(&ShipComparisonArgs {
            class: Some(ShipClass::Cruiser),
            tier: Some(Tier(10)),
            ..Default::default()
        })
        .expect("tier 10 cruisers compare");
    assert_eq!(full.ships.len(), full.ranked);
}

/// The numbers behind each "value (rank r/n, pNN)" text, with the GameParams
/// index of each ship, for callers that rank ships themselves.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn comparison_standings_match_their_text() {
    let comparison = game()
        .ship_comparison(&ShipComparisonArgs {
            class: Some(ShipClass::Destroyer),
            tier: Some(Tier(10)),
            ..Default::default()
        })
        .expect("tier 10 destroyers compare");

    let mut tied = 0;
    for ship in &comparison.ships {
        assert!(!ship.index.is_empty(), "{} has no index", ship.ship);
        let text = serde_json::to_value(&ship.values).expect("values serialize");
        for standing in &ship.standings {
            let shown = text[standing.factor].as_str().expect("each standing has its text");
            let rank = standing.rank;
            assert_eq!(rank.better + rank.worse + rank.tied, rank.others(), "{} {standing:?}", ship.ship);
            if let Some(beaten) = rank.beaten() {
                let expected = format!("(rank {}/{}, p{})", rank.position, rank.count, beaten.value().round());
                // A unique upgrade marker may follow the ranks.
                let ranks = shown.split(" (with unique upgrade").next().unwrap_or(shown);
                assert!(ranks.ends_with(&expected), "{}: {shown} against {expected}", ship.ship);
            }
            tied += rank.tied;
        }
    }
    assert!(tied > 0, "some factor has ties");

    let chateaurenault = comparison
        .ships
        .iter()
        .find(|ship| ship.ship.contains("teaurenault"))
        .expect("Chateaurenault is a tier 10 destroyer");
    let beaten = |standing: &wows_query::comparison::FactorStanding| {
        let rank = standing.rank;
        let share = |share: Option<Percent>| share.map_or("none".to_owned(), |share| format!("{:.2}%", share.value()));
        format!(
            "{} {:.2} {} ({:?} is better): rank {}/{}, better {}, worse {}, tied {}, outlier {:?}, beaten {}, ties half {}",
            standing.factor,
            standing.value,
            standing.unit,
            standing.better,
            rank.position,
            rank.count,
            rank.better,
            rank.worse,
            rank.tied,
            rank.outlier,
            share(rank.beaten()),
            share(rank.beaten_ties_half())
        )
    };
    insta::assert_yaml_snapshot!(json!({
        "ship": chateaurenault.ship,
        "index": chateaurenault.index,
        "standings": chateaurenault.standings.iter().map(beaten).collect::<Vec<_>>(),
        "torpedoes": chateaurenault
            .torpedoes
            .iter()
            .map(|torpedo| json!({ "torpedo": torpedo.torpedo, "standings": torpedo.standings.iter().map(beaten).collect::<Vec<_>>() }))
            .collect::<Vec<_>>(),
    }));
}

/// Des Moines, Hawaii and Zao against the shooter groups of tier 10.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_matchups_against_hawaii_and_zao() {
    let comparison = game()
        .ship_comparison(&ShipComparisonArgs {
            ships: Some(vec!["Des Moines".to_owned(), "Hawaii".to_owned(), "Za\u{14d}".to_owned()]),
            ..Default::default()
        })
        .expect("tier 10 cruisers compare");
    assert!(comparison.ships.iter().all(|ship| !ship.matchups.is_empty()), "{comparison:?}");
    insta::assert_yaml_snapshot!(comparison);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn destroyer_armor_assessment_states_citadel_metrics_do_not_apply() {
    let assessment = game()
        .armor_assessment(&ArmorAssessmentArgs { ship: "Gearing".to_owned(), hull: None })
        .expect("Gearing has an armor model");
    assert!(assessment.citadel_metrics.is_some());
    assert!(assessment.verdict.starts_with("Citadel metrics do not apply"), "{}", assessment.verdict);
    assert!(!assessment.verdict.contains("no citadel plates"), "{}", assessment.verdict);
}

fn rates(name: &str) -> wows_query::rates::Rates {
    game()
        .ship_stats(&ShipStatsArgs { ship: name.to_owned(), ..Default::default() })
        .expect("ship exists")
        .build_stats
        .expect("a stock call reports build stats")
        .rates
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn weapon_rates_on_their_best_builds() {
    for name in ["Yamato", "Shimakaze", "Smaland", "Orzel", "Barfleur", "La Pampa", "Yodo", "Zao", "Kitakami"] {
        insta::assert_yaml_snapshot!(format!("weapon_rates_{}", name.replace(' ', "_")), rates(name));
    }
}

/// Barfleur's one loader serves both launchers; Orzel's bow and stern groups
/// reload two launchers each; Shimakaze reloads every launcher at once.
/// Every launcher counts: the ceiling of the one-target figure.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn torpedoes_per_minute_follow_the_loaders() {
    let base = |name: &str| {
        let rate = rates(name).torpedoes_per_minute.expect("the ship has torpedoes");
        let base = rate.rate.builds.base;
        // Submarine rates have no bearing and count every launcher already.
        (rate.tubes.0, base.all_launchers_ceiling.unwrap_or(base.torpedoes_per_minute).value())
    };

    assert_eq!(base("Barfleur"), (10, 5.26));
    assert_eq!(base("Orzel"), (12, 7.74));
    assert_eq!(base("Shimakaze"), (15, 6.0));
}

/// Consumable Specialist shortens every consumable's cooldown; the booster
/// rates name only the change to the Torpedo Reload Booster's own. With
/// the reload upgrades the shorter cooldown fits one salvo fewer per cycle.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn booster_rates_name_only_the_booster_cooldown() {
    let stats = game()
        .ship_stats(&ShipStatsArgs {
            ship: "La Pampa".to_owned(),
            skills: Some(vec!["Consumable Specialist".to_owned(), "Fill the Tubes".to_owned()]),
            upgrades: Some(vec!["Torpedo Tubes Modification 2".to_owned()]),
            ..Default::default()
        })
        .expect("La Pampa exists");
    let booster: Vec<_> =
        stats.changes.iter().filter(|change| change.stat.contains("Torpedo Reload Booster cycle")).collect();

    assert!(!booster.is_empty(), "{:?}", stats.changes);
    insta::assert_yaml_snapshot!(booster);
}

/// The 10 ships with the highest torpedo damage per minute against one
/// target on the launcher reload, over a Torpedo Reload Booster cycle and
/// over a reload buff cycle, and with every launcher firing at once.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn torpedo_damage_per_minute_ranks_ships() {
    let rank = |stat: &str| -> Vec<String> {
        let arguments = json!({ "rank": { "stat": stat, "order": "desc", "limit": 10 }, "exclude_test_ships": true });
        let search = game().call_tool("stat_search", arguments).expect("rank resolves");
        search["ranking"]["ships"]
            .as_array()
            .expect("ships is a list")
            .iter()
            .map(|ship| format!("{} {} {}", ship["rank"], ship["ship"].as_str().unwrap_or_default(), ship["value"]))
            .collect()
    };

    insta::assert_yaml_snapshot!(json!({
        "one_target": rank("torpedoes.torpedoes.damage_per_minute"),
        "one_target_with_reload_booster": rank("torpedoes.torpedoes.damage_per_minute_with_reload_booster"),
        "one_target_with_reload_buff": rank("torpedoes.torpedoes.damage_per_minute_with_reload_buff"),
        "all_launchers_ceiling": rank("torpedoes.torpedoes.damage_per_minute_all_launchers"),
    }));
}

fn fire_rates(name: &str) -> wows_query::fires::FireRates {
    game()
        .ship_stats(&ShipStatsArgs { ship: name.to_owned(), ..Default::default() })
        .expect("ship exists")
        .build_stats
        .and_then(|builds| builds.fires_per_minute)
        .expect("the ship can start fires")
}

/// Libertad's secondaries and Prins van Oranje's secondaries and air strike
/// start more fires than their main batteries; Thunderer and Conqueror carry
/// some of the highest main battery shell fire chances.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fire_starts_per_minute_cover_every_armament() {
    for name in ["Libertad", "Prins van Oranje", "Thunderer", "Conqueror"] {
        insta::assert_yaml_snapshot!(format!("fire_rates_{}", name.replace(' ', "_")), fire_rates(name));
    }
}

/// Secondary fire starts and damage against one target by bearing: only the
/// mounts whose firing sectors cover the bearing count.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn secondaries_bear_on_one_target_by_bearing() {
    #[derive(serde::Serialize)]
    struct SecondaryDamage<'a> {
        ammo: &'a str,
        base: &'a wows_query::rates::DamageRate,
        best: &'a wows_query::builds::BuildValue<wows_query::rates::DamageRate>,
    }

    #[derive(serde::Serialize)]
    struct Bearings<'a> {
        fires_per_minute: &'a wows_query::builds::Builds<wows_query::fires::FireTotal>,
        damage_per_minute: Vec<SecondaryDamage<'a>>,
    }

    for name in ["Sibir", "Libertad", "Patrie", "Ohio", "Yamato"] {
        let rates = rates(name);
        let fires = fire_rates(name).secondaries.expect("the ship has secondaries that start fires");
        let damage_per_minute = rates
            .secondary_damage
            .iter()
            .map(|ammo| SecondaryDamage {
                ammo: &ammo.ammo,
                base: &ammo.rates.builds.base,
                best: &ammo.rates.builds.best,
            })
            .collect();
        insta::assert_yaml_snapshot!(
            format!("secondaries_by_bearing_{name}"),
            Bearings { fires_per_minute: &fires.builds, damage_per_minute }
        );
    }
}

/// The 15 ships that start the most fires against one target, each with
/// what every armament adds at its best bearing, against the same ranking
/// with every gun mount firing at once, the 15 with the highest main battery
/// shell fire chance, and the 15 cruisers that start the most fires.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fire_starts_rank_ships_by_every_armament() {
    let rank = |stat: &str, class: Option<&str>| -> Vec<serde_json::Value> {
        let mut arguments =
            json!({ "rank": { "stat": stat, "order": "desc", "limit": 15 }, "exclude_test_ships": true });
        if let Some(class) = class {
            arguments["class"] = json!(class);
        }
        let search = game().call_tool("stat_search", arguments).expect("rank resolves");
        search["ranking"]["ships"].as_array().expect("ships is a list").clone()
    };
    let line = |ship: &serde_json::Value| {
        format!("{} {} {}", ship["rank"], ship["ship"].as_str().unwrap_or_default(), ship["value"])
    };

    let by_fires: Vec<serde_json::Value> = rank("fires_per_minute.total", None)
        .iter()
        .map(|ship| json!({ "line": line(ship), "fires_by_armament": ship["fires_by_armament"] }))
        .collect();
    let every_mount: Vec<String> = rank("fires_per_minute.total_all_mounts", None).iter().map(line).collect();
    let cruisers: Vec<String> = rank("fires_per_minute.total", Some("cruiser")).iter().map(line).collect();
    let by_shell: Vec<String> = rank("artillery.shells.burn_chance", None).iter().map(line).collect();

    assert!(by_fires.iter().all(|ship| ship["fires_by_armament"].is_object()), "{by_fires:?}");
    insta::assert_yaml_snapshot!(json!({
        "by_fire_starts_one_target": by_fires,
        "by_fire_starts_every_mount_ceiling": every_mount,
        "cruisers_by_fire_starts": cruisers,
        "by_shell_fire_chance": by_shell,
    }));
}

/// Torpedo Crew Training shares its trigger type with Fearless Brawler; its
/// alternative must carry its own torpedo sentence.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn triggered_alternatives_carry_their_own_skill_condition() {
    let stats =
        game().ship_stats(&ShipStatsArgs { ship: "Orzel".to_owned(), ..Default::default() }).expect("Orzel exists");
    let torpedo_reload = stats.build_stats.and_then(|builds| builds.torpedo_reload).expect("Orzel has torpedoes");
    let triggered: Vec<_> = torpedo_reload
        .builds
        .alternatives
        .iter()
        .filter(|alternative| alternative.label.ends_with(" active"))
        .collect();
    assert!(!triggered.is_empty(), "{torpedo_reload:?}");

    for alternative in triggered {
        let skill = alternative.label.trim_start_matches("with ").trim_end_matches(" active");
        let details = game()
            .captain_skill(&CaptainSkillArgs {
                skill: skill.to_owned(),
                ship: Some("Orzel".to_owned()),
                ..Default::default()
            })
            .expect("the alternative names a skill");
        assert_eq!(alternative.note, details.skills[0].trigger, "{}", alternative.label);
        let note = alternative.note.as_deref().expect("a triggered alternative states its condition");
        assert!(note.contains("torpedo"), "{}: {note}", alternative.label);
    }
}

/// A trigger sentence that names armament names the armament its skill's
/// triggered effects change.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn skill_trigger_sentences_match_their_effects() {
    const ARMAMENT: [&str; 3] = ["main battery", "torpedo", "secondary"];

    let mut checked = 0;
    for class in ShipClass::ALL {
        for row in game().skill_tree(&SkillTreeArgs { class }).rows {
            for name in row.skills {
                let output = game()
                    .captain_skill(&CaptainSkillArgs { skill: name.clone(), class: Some(class), ..Default::default() })
                    .expect("a skill tree skill resolves");
                for details in output.skills {
                    let Some(trigger) = details.trigger.as_deref().map(str::to_lowercase) else {
                        continue;
                    };
                    let effects = details
                        .classes
                        .iter()
                        .flat_map(|group| &group.triggered_effects)
                        .map(|line| line.to_lowercase())
                        .collect::<Vec<_>>();
                    let named = |text: &str, armament: &str| text.contains(armament);
                    if effects.is_empty() || !ARMAMENT.iter().any(|armament| named(&trigger, armament)) {
                        continue;
                    }

                    for armament in ARMAMENT {
                        if effects.iter().any(|line| named(line, armament)) {
                            assert!(
                                named(&trigger, armament),
                                "{name}: effects {effects:?} change {armament}, trigger {trigger:?} does not name it"
                            );
                        }
                    }
                    checked += 1;
                }
            }
        }
    }

    assert!(checked > 5, "only {checked} triggered skills checked");
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn community_ship_names_resolve() {
    let resolved: Vec<_> = [
        ("GK", "G. Kurf\u{fc}rst"),
        ("grober", "G. Kurf\u{fc}rst"),
        ("Grober Kurfurst", "G. Kurf\u{fc}rst"),
        ("Groer Kurfurst", "G. Kurf\u{fc}rst"),
        ("mega grober", "Hannover"),
        ("brik", "Vermont"),
        ("brick", "Vermont"),
    ]
    .into_iter()
    .map(|(name, expected)| {
        let ship = game().ship_summary(name).expect("name resolves");
        assert_eq!(ship.ship.name, expected);

        (name, ship.interpretations)
    })
    .collect();

    insta::assert_yaml_snapshot!(resolved);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn bracketed_variant_resolves_by_its_exact_name() {
    let copy = game().ship_summary("[Conqueror]").expect("the clan variant is in the client data");
    let released = game().ship_overview(&overview("Conqueror")).expect("Conqueror exists");
    let variants: Vec<&str> = released.variants.iter().map(|variant| variant.name.as_str()).collect();
    insta::assert_yaml_snapshot!(json!({ "copy": copy, "variants_of_conqueror": variants }));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn former_skill_name_in_part_is_historical() {
    let skill = game()
        .captain_skill(&CaptainSkillArgs { skill: "Basic Firing".to_owned(), ..Default::default() })
        .expect("a former name resolves");
    insta::assert_yaml_snapshot!(skill.interpretations);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ambiguous_names_list_candidates() {
    let error = game()
        .call_tool("captain_skill", json!({ "skill": "Consumable" }))
        .expect_err("several skills start with Consumable");
    insta::assert_yaml_snapshot!(error.details());
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn test_ships_resolve_with_status() {
    let overview = game().ship_overview(&overview("Neptunus")).expect("Neptunus is in the client data");
    insta::assert_yaml_snapshot!(overview);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn mentions_name_ships_skills_and_consumables() {
    let mentions: Vec<_> = [
        "what is Neptunus?",
        "is DM better than Moskva",
        "should I take SE or dm the streamer",
        "is radar on Grosser Kurfurst a thing",
        "is the Radar Mod worth a slot",
        "can you recommend a good pizza place near the station",
        "my boy scout fighter friend asked about AI on a PC",
    ]
    .into_iter()
    .map(|text| json!({ "text": text, "mentions": game().mentions(text) }))
    .collect();
    insta::assert_yaml_snapshot!(mentions);
}

fn firing_arcs(ship: &str, target: Option<&str>, skills: &[&str]) -> ShipFiringArcs {
    game()
        .ship_firing_arcs(&FiringArcsArgs {
            ship: ship.to_owned(),
            target_ship: target.map(str::to_owned),
            skills: (!skills.is_empty()).then(|| skills.iter().map(|skill| (*skill).to_owned()).collect()),
            ..Default::default()
        })
        .expect("firing arcs resolve")
}

fn battery(arcs: &ShipFiringArcs, weapon: Weapon) -> &BatteryArcs {
    arcs.batteries.iter().find(|battery| battery.weapon == weapon).expect("battery present")
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_bears_nine_barrels_on_the_broadside_and_fewer_ahead() {
    let arcs = firing_arcs("Des Moines", None, &[]);
    let main = battery(&arcs, Weapon::MainBattery);

    assert_eq!(main.barrels, 9);
    assert_eq!(main.coverage.starboard.barrels, 9);
    assert_eq!(main.coverage.port.barrels, 9);
    assert!(main.coverage.bow.fewest_barrels < 9);
    insta::assert_yaml_snapshot!(arcs);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn colbert_main_battery_coverage() {
    let arcs = firing_arcs("Colbert", None, &[]);
    let main = battery(&arcs, Weapon::MainBattery);

    insta::assert_yaml_snapshot!((&main.coverage, &main.all_barrels_bear));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn submarine_torpedoes_are_left_out() {
    let arcs = firing_arcs("Balao", None, &[]);

    assert!(arcs.batteries.iter().all(|battery| battery.weapon != Weapon::Torpedoes));
    assert!(arcs.notes.iter().any(|note| note.starts_with("Submarine torpedo tubes")));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn iowa_rear_turret_never_bears_within_30_degrees_of_the_bow() {
    let arcs = firing_arcs("Iowa", None, &[]);
    let main = battery(&arcs, Weapon::MainBattery);

    assert_eq!(main.barrels, 9);
    assert_eq!((main.coverage.bow.barrels, main.coverage.bow.fewest_barrels), (6, 6));
    assert_eq!(main.coverage.starboard.barrels, 9);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn modules_differing_by_punctuation_are_selected_by_exact_name() {
    for name in ["533 mm Mk IX", "533 mm Mk IX*"] {
        let arcs = game()
            .ship_firing_arcs(&FiringArcsArgs {
                ship: "Fiji".to_owned(),
                modules: Some(vec![name.to_owned()]),
                ..Default::default()
            })
            .expect("exact module name resolves");
        let chosen: Vec<&str> = arcs.modules.iter().map(|module| module.name.as_str()).collect();
        assert_eq!(chosen, [name]);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn jinan_fires_per_minute_against_colbert() {
    let stock = firing_arcs("Jinan", Some("Colbert"), &[]);
    let demolition = firing_arcs("Jinan", Some("Colbert"), &["Demolition Expert"]);

    let he = |arcs: &ShipFiringArcs| {
        let main = battery(arcs, Weapon::MainBattery);
        main.ammo.iter().find(|ammo| ammo.ammo == "HE").expect("HE").clone()
    };
    insta::assert_yaml_snapshot!((&stock.target, he(&stock), &demolition.loadout_changes, he(&demolition)));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn firing_arcs_name_the_hull_an_upgraded_module_needs() {
    let arcs_with = |ship: &str, modules: &[&str]| {
        game().ship_firing_arcs(&FiringArcsArgs {
            ship: ship.to_owned(),
            modules: Some(modules.iter().map(|module| (*module).to_owned()).collect()),
            ..Default::default()
        })
    };
    let guns = "203 mm/55 Mk.14 on a triple mount/twin mount (upgraded)";

    let alone = arcs_with("Pensacola (old)", &[guns]).expect_err("the upgraded guns need Hull B");
    assert!(matches!(&alone, QueryError::ModuleNeedsHull { hulls, .. } if hulls == &["Hull B"]), "{alone:?}");
    let torpedoes = arcs_with("Ognevoi (OLD)", &["53-39 mod. 1"]).expect_err("the torpedoes need Hull B");
    insta::assert_yaml_snapshot!([alone.to_string(), torpedoes.to_string()]);

    let with_hull = arcs_with("Pensacola (old)", &["Hull B", guns]).expect("Hull B takes the upgraded guns");
    assert!(!battery(&with_hull, Weapon::MainBattery).ammo.is_empty());
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn firing_arcs_accept_point_blank_range() {
    let arcs_at = |range: Km| {
        game().ship_firing_arcs(&FiringArcsArgs {
            ship: "Des Moines".to_owned(),
            range_km: Some(range),
            ..Default::default()
        })
    };

    let point_blank = arcs_at(Km::new(0.0)).expect("0 km is point blank");
    assert_eq!(point_blank.dispersion.expect("Des Moines has a main battery").range_km, Km::new(0.0));
    assert!(matches!(arcs_at(Km::new(-1.0)), Err(QueryError::InvalidArguments { .. })));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_reports_stock_ships_and_the_fewest_requirements() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({
                "conditions": [{ "stat": "artillery.reload_time", "op": "<", "value": 5 }],
                "min_tier": 10,
                "max_tier": 10,
                "class": "cruiser",
                "nation": "USA",
            }),
        )
        .expect("conditions resolve");

    let ships = search["ships"].as_array().expect("ships is a list");
    let des_moines =
        ships.iter().find(|hit| hit["ship"] == "T10 Des Moines (cruiser, USA)").expect("Des Moines qualifies");
    assert_eq!(des_moines["stock"], false);
    assert!(ships.iter().any(|hit| hit["stock"] == true), "some ship qualifies stock");
    insta::assert_yaml_snapshot!(search);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_conditions_leave_conditional_skills_off() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({
                "conditions": [
                    { "stat": "armor.citadel.max", "op": ">=", "value": 400 },
                    { "stat": "mobility.speed", "op": ">=", "value": 33 },
                ],
                "min_tier": 10,
                "max_tier": 10,
                "nation": "USA",
            }),
        )
        .expect("conditions resolve");

    let requires = search["ships"].to_string();
    assert!(!requires.contains("Brisk"), "Brisk counts only with its trigger met: {requires}");
    insta::assert_yaml_snapshot!(search);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_rejects_unreachable_conditions() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({ "conditions": [{ "stat": "durability.health", "op": ">", "value": 200000 }] }),
        )
        .expect("conditions resolve");

    assert_eq!(search["qualifying"]["total"], 0);
    assert!(search["considered"]["total"].as_u64().expect("count") > 500);
    assert!(search.get("undetermined").is_none());

    let errors: Vec<String> = [
        json!({ "conditions": [{ "stat": "artilery.reload_tim", "op": "<", "value": 5 }] }),
        json!({ "conditions": [{ "stat": "speed of light", "op": "<", "value": 5 }] }),
        json!({ "conditions": [{ "stat": "torpedoes.torpedoes.is_damage_increasing", "op": "=", "value": 1 }] }),
        json!({ "conditions": [] }),
    ]
    .into_iter()
    .map(|arguments| match game().call_tool("stat_search", arguments) {
        Ok(found) => format!("ok: {}", found["conditions"]),
        Err(error) => error.to_string(),
    })
    .collect();
    insta::assert_yaml_snapshot!(errors);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_names_qualifiers_that_match_no_row() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({
                "conditions": [{ "stat": "artillery.shell.damage", "op": ">", "value": 1000, "qualifier": "HEAT" }],
                "min_tier": 10,
                "class": "cruiser",
            }),
        )
        .expect("conditions resolve");

    assert_eq!(search["qualifying"]["total"], 0);
    insta::assert_yaml_snapshot!(search["unmatched_qualifiers"]);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_shells_report_ricochet_angles() {
    let shells = game().ship_shells(&ship("Des Moines")).expect("Des Moines exists");
    insta::assert_yaml_snapshot!(shells);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn projectiles_show_english_names() {
    let shells = |ship: &str, label: &str| -> Vec<serde_json::Value> {
        let found = game().call_tool("ship_shells", json!({ "ship": ship })).expect("ship exists");
        found["shells"]
            .as_array()
            .expect("shells is a list")
            .iter()
            .filter(|shell| shell["label"] == label)
            .map(|shell| json!({ "name": shell["name"], "param": shell["param"] }))
            .collect()
    };
    let overview = game().call_tool("ship_overview", json!({ "ship": "Shimakaze" })).expect("Shimakaze exists");

    let names = json!({
        "Conqueror main battery 457 mm HE": shells("Conqueror", "main battery 457 mm HE"),
        "Des Moines main battery 203 mm AP": shells("Des Moines", "main battery 203 mm AP"),
        "Shimakaze torpedoes": overview["armament"]["torpedoes"]["torpedoes"],
    });
    insta::assert_yaml_snapshot!(names);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_finds_ap_ricocheting_later_than_standard() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({
                "conditions": [{ "stat": "artillery.ap.ricochet_start", "op": ">", "value": 45 }],
                "exclude_test_ships": true,
            }),
        )
        .expect("conditions resolve");

    let ships = search["ships"].as_array().expect("ships is a list");
    assert!(ships.iter().any(|hit| hit["ship"] == "T10 Des Moines (cruiser, USA)"), "{ships:#?}");
    insta::assert_yaml_snapshot!(search);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_destroyers_by_surface_detectability() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({ "rank": { "stat": "visibility.sea_detection", "order": "asc" }, "class": "destroyer" }),
        )
        .expect("rank resolves");

    assert_eq!(search["ranking"]["ships"].as_array().expect("ships is a list").len(), 10);
    assert!(search.get("qualifying").is_none());
    insta::assert_yaml_snapshot!(search);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_tier_10_battleships_by_main_battery_range() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({
                "rank": { "stat": "artillery.range", "order": "desc" },
                "class": "battleship",
                "min_tier": 10,
                "max_tier": 10,
            }),
        )
        .expect("rank resolves");

    insta::assert_yaml_snapshot!(search);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_only_ships_meeting_the_conditions() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({
                "conditions": [{ "stat": "artillery.reload_time", "op": "<", "value": 6 }],
                "rank": { "stat": "visibility.sea_detection", "order": "asc", "limit": 5 },
                "class": "cruiser",
                "min_tier": 10,
                "max_tier": 10,
            }),
        )
        .expect("conditions and rank resolve");

    let qualifying = search["qualifying"]["total"].as_u64().expect("count");
    assert_eq!(search["ranking"]["ranked"].as_u64().expect("count") + undetermined(&search), qualifying);
    assert!(search.get("ships").is_none());
    insta::assert_yaml_snapshot!(search);

    let errors: Vec<String> = [
        json!({ "rank": { "stat": "visibility.sea_detection", "order": "asc", "limit": 26 } }),
        json!({ "rank": { "stat": "visibility.sea_detection", "order": "lowest" } }),
        json!({ "class": "destroyer" }),
    ]
    .into_iter()
    .map(|arguments| match game().call_tool("stat_search", arguments) {
        Ok(found) => format!("ok: {}", found["ranking"]["stat"]),
        Err(error) => error.to_string(),
    })
    .collect();
    insta::assert_yaml_snapshot!(errors);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_derived_and_ricochet_stats() {
    let ranked: Vec<String> = ["artillery.barrels", "artillery.ap.ricochet_start"]
        .into_iter()
        .flat_map(|stat| {
            let search = game()
                .call_tool(
                    "stat_search",
                    json!({
                        "rank": { "stat": stat, "order": "desc", "limit": 3 },
                        "class": "battleship",
                        "min_tier": 10,
                        "max_tier": 10,
                    }),
                )
                .expect("rank resolves");
            let ships = search["ranking"]["ships"].as_array().expect("ships is a list").clone();
            ships.into_iter().map(move |ship| format!("{stat}: {} {} {}", ship["rank"], ship["ship"], ship["value"]))
        })
        .collect();

    insta::assert_yaml_snapshot!(ranked);
}

/// Worst concealment among cruisers, highest detection first: as players
/// build their ships (the build that lowers detection most) and on base
/// values. Each list is ordered by the value it shows.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_worst_concealment_on_a_chosen_build() {
    let ranking = |build: &str| -> serde_json::Value {
        let search = game()
            .call_tool(
                "stat_search",
                json!({
                    "rank": { "stat": "visibility.sea_detection", "order": "desc", "limit": 5, "build": build },
                    "class": "cruiser",
                }),
            )
            .expect("rank resolves");
        let ships: Vec<String> = search["ranking"]["ships"]
            .as_array()
            .expect("ships is a list")
            .iter()
            .map(|ship| {
                format!(
                    "{} {}: {} (base {}, build {})",
                    ship["rank"],
                    ship["ship"].as_str().unwrap_or_default(),
                    ship["value"].as_str().unwrap_or_default(),
                    ship["base"].as_str().unwrap_or_default(),
                    ship["build"]
                )
            })
            .collect();

        json!({ "stat": search["ranking"]["stat"], "rule": search["ranking"]["rule"], "ships": ships })
    };

    insta::assert_yaml_snapshot!(json!({ "lowest": ranking("lowest"), "base": ranking("base") }));
}

/// Worst concealment among battleships with no build named: each ship on the
/// build that lowers detection, as players build them, highest first.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_worst_concealment_battleships_on_the_usual_build() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({ "rank": { "stat": "visibility.sea_detection", "order": "desc", "limit": 5 }, "class": "battleship" }),
        )
        .expect("rank resolves");
    let ships: Vec<String> = search["ranking"]["ships"]
        .as_array()
        .expect("ships is a list")
        .iter()
        .map(|ship| {
            format!(
                "{} {}: {} (base {}, build {})",
                ship["rank"],
                ship["ship"].as_str().unwrap_or_default(),
                ship["value"].as_str().unwrap_or_default(),
                ship["base"].as_str().unwrap_or_default(),
                ship["build"]
            )
        })
        .collect();
    assert!(ships.iter().all(|ship| ship.contains("Concealment Expert")), "{ships:#?}");

    insta::assert_yaml_snapshot!(
        json!({ "stat": search["ranking"]["stat"], "rule": search["ranking"]["rule"], "ships": ships })
    );
}

/// One condition on the capacity of time-based consumables lists every ship
/// with one, released and in testing, naming the consumable and its capacity.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_lists_time_based_consumables() {
    let search = game()
        .call_tool("stat_search", json!({ "conditions": [{ "stat": "consumable.capacity", "op": ">=", "value": 0 }] }))
        .expect("capacity resolves");

    let ships: Vec<String> = search["ships"]
        .as_array()
        .expect("ships is a list")
        .iter()
        .map(|ship| {
            format!("{}: {}", ship["ship"].as_str().unwrap_or_default(), ship["values"][0].as_str().unwrap_or_default())
        })
        .collect();
    for name in ["20 de Julio", "Sibir"] {
        assert!(ships.iter().any(|ship| ship.contains(name)), "{name} missing from {ships:#?}");
    }

    let ranking = game()
        .call_tool("stat_search", json!({ "rank": { "stat": "consumable.capacity", "order": "desc", "limit": 5 } }))
        .expect("capacity ranks");
    let ranked: Vec<String> = ranking["ranking"]["ships"]
        .as_array()
        .expect("ships is a list")
        .iter()
        .map(|ship| {
            format!(
                "{} {}: {} ({}), base {}",
                ship["rank"],
                ship["ship"].as_str().unwrap_or_default(),
                ship["value"].as_str().unwrap_or_default(),
                ship["row"].as_str().unwrap_or_default(),
                ship["base"].as_str().unwrap_or_default()
            )
        })
        .collect();

    insta::assert_yaml_snapshot!(json!({
        "conditions": search["conditions"],
        "considered": search["considered"],
        "qualifying": search["qualifying"],
        "omitted": search["omitted"],
        "ships": ships,
        "ranking": ranked,
    }));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_search_ranks_main_battery_penetration_by_row() {
    let ranked: Vec<serde_json::Value> = [None, Some("HE"), Some("SAP")]
        .into_iter()
        .map(|qualifier| {
            let mut rank = json!({ "stat": "artillery.shells.penetration", "order": "desc", "limit": 5 });
            if let Some(qualifier) = qualifier {
                rank["qualifier"] = json!(qualifier);
            }
            let search = game()
                .call_tool("stat_search", json!({ "rank": rank, "min_tier": 10, "max_tier": 10 }))
                .expect("rank resolves");
            let ships: Vec<String> = search["ranking"]["ships"]
                .as_array()
                .expect("ships is a list")
                .iter()
                .map(|ship| format!("{} {} {}: {}", ship["rank"], ship["ship"], ship["value"], ship["row"]))
                .collect();
            json!({ "stat": search["ranking"]["stat"], "ships": ships })
        })
        .collect();
    insta::assert_yaml_snapshot!(ranked);

    let error = game()
        .call_tool("stat_search", json!({ "rank": { "stat": "artillery.he.penetration", "order": "desc" } }))
        .expect_err("ammo type is a qualifier, not part of the stat name");
    insta::assert_snapshot!(error.to_string());
}

fn undetermined(search: &serde_json::Value) -> u64 {
    search.get("undetermined").and_then(|ships| ships.as_array()).map_or(0, |ships| ships.len() as u64)
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn captain_skill_outside_the_ship_tree_names_skill_ship_and_classes() {
    let error = game()
        .call_tool("captain_skill", json!({ "skill": "SE", "ship": "GK" }))
        .expect_err("Survivability Expert is not in the battleship tree");

    insta::assert_snapshot!(error.to_string());
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tech_tree_lines_by_tier_10_ship() {
    let lines: Vec<String> = ["Essex", "Harugumo", "DM"]
        .into_iter()
        .map(|ship| {
            let lines = game()
                .tech_tree_lines(&TechTreeLinesArgs { tier_10_ship: Some(ship.to_owned()), ..Default::default() })
                .expect("the ship ends a line");
            assert_eq!(lines.total, 1);
            lines.lines[0].path.iter().map(|ship| ship.name.as_str()).collect::<Vec<_>>().join(" > ")
        })
        .collect();
    insta::assert_yaml_snapshot!(lines);

    let error = game()
        .tech_tree_lines(&TechTreeLinesArgs { tier_10_ship: Some("Missouri".to_owned()), ..Default::default() })
        .expect_err("Missouri is a premium ship");
    insta::assert_snapshot!(error.to_string());
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_aircraft_lists_squadrons_air_strikes_and_plate_verdicts() {
    for (name, thickness) in
        [("Midway", None), ("Ise", Some(32.0)), ("Gouden Leeuw", Some(25.0)), ("Sekiryu", Some(32.0))]
    {
        let aircraft =
            game().call_tool("ship_aircraft", json!({ "ship": name, "thickness_mm": thickness })).expect("ship exists");
        insta::assert_yaml_snapshot!(format!("ship_aircraft_{}", name.replace(' ', "_")), aircraft);
    }
}

/// AP bomb and rocket penetration at bulletSpeed against a community
/// calculation of the same formula and, where reported, the port value.
/// Known differences are port values the formula does not reproduce.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ap_bomb_and_rocket_penetration_matches_reference_values() {
    struct Reference {
        projectile: &'static str,
        calculated_mm: f32,
        port_mm: Option<f32>,
        known_difference: Option<&'static str>,
    }
    const fn calculated(projectile: &'static str, calculated_mm: f32) -> Reference {
        Reference { projectile, calculated_mm, port_mm: None, known_difference: None }
    }
    const fn port(projectile: &'static str, calculated_mm: f32, port_mm: f32) -> Reference {
        Reference { projectile, calculated_mm, port_mm: Some(port_mm), known_difference: None }
    }
    const fn known(projectile: &'static str, calculated_mm: f32, port_mm: f32, why: &'static str) -> Reference {
        Reference { projectile, calculated_mm, port_mm: Some(port_mm), known_difference: Some(why) }
    }
    const SHIPS: &[&str] = &[
        "Richthofen",
        "Parseval",
        "Weser",
        "Rhein",
        "Hakuryu",
        "Shokaku",
        "Ryujo",
        "Hosho",
        "Enterprise",
        "Graf Zeppelin",
        "Hornet",
        "Malta",
        "Bearn",
        "Colossus",
        "Aquila",
    ];
    const REFERENCES: &[Reference] = &[
        port("PGPR004_M_RICHTHOFEN_STOCK", 277.0, 277.0),
        port("PGPR005_M_RICHTHOFEN_TOP", 277.0, 277.0),
        port("PGPB008_M_RICHTHOFEN_TOP", 331.0, 331.0),
        calculated("PGPR001_PARSEVAL_STOCK", 184.0),
        calculated("PGPR002_PARSEVAL_TOP", 184.0),
        calculated("PGPB004_PARSEVAL_TOP", 256.0),
        calculated("PGPR006_WESER_STOCK", 141.0),
        calculated("PGPR007_WESER_TOP", 141.0),
        calculated("PGPB009_WESER_STOCK", 227.0),
        calculated("PGPB010_WESER_TOP", 227.0),
        calculated("PGPR003_RHEIN_STOCK", 141.0),
        known("PGPB005_RHEIN_STOCK", 169.0, 149.0, "in-game tests show 160 to 177"),
        calculated("PJPB110_Hakuryu_stock", 351.0),
        calculated("PJPB210_Hakuryu_top", 351.0),
        calculated("PJPB208_Shokaku_top", 283.0),
        calculated("PJPB206_Ryujo_top", 228.0),
        calculated("PJPB104_Hosho_stock", 174.0),
        calculated("PAPB508_Enterprise_stock", 244.0),
        known("PGPB508_Graf_Zeppelin_stock", 344.0, 334.0, "port value predates a change to the bomb"),
        calculated("PAPB548_HORNET_STOCK", 328.0),
        calculated("PBPB112_MALTA_STOCK", 109.0),
        known("PFPB001_BEARN_STOCK", 215.0, 219.0, "unexplained"),
        known("PBPR308_COLOSSUS_STOCK", 200.5, 122.0, "unexplained"),
        calculated("PIPR001_AQUILA_STOCK", 129.0),
    ];
    // References are whole mm except where the source gave a fraction.
    let agrees = |penetration: Millimeters, reference_mm: f32| (penetration.value() - reference_mm).abs() <= 0.5;

    let mut table = String::new();
    let mut referenced = BTreeSet::new();
    for name in SHIPS {
        let aircraft = game()
            .ship_aircraft(&ShipAircraftArgs { ship: (*name).to_owned(), thickness_mm: None })
            .expect("ship exists");
        let mut seen = BTreeSet::new();
        for squadron in &aircraft.squadrons {
            let Some(ordnance) = &squadron.ordnance else {
                continue;
            };
            let Some(ap) = &ordnance.ap else {
                continue;
            };
            if !seen.insert(ordnance.param.clone()) {
                continue;
            }

            let penetration = ap.penetration_mm.expect("AP ordnance has penetration inputs");
            let comparison = match REFERENCES.iter().find(|reference| reference.projectile == ordnance.param) {
                Some(reference) => {
                    referenced.insert(reference.projectile);
                    assert!(
                        agrees(penetration, reference.calculated_mm),
                        "{} computes {} mm, calculated {} mm",
                        reference.projectile,
                        penetration.value(),
                        reference.calculated_mm
                    );
                    let mut text = format!("calculated {} match", reference.calculated_mm);
                    if let Some(port_mm) = reference.port_mm {
                        let agrees_with_port = agrees(penetration, port_mm);
                        assert_eq!(
                            agrees_with_port,
                            reference.known_difference.is_none(),
                            "{} computes {} mm, port {port_mm} mm",
                            reference.projectile,
                            penetration.value()
                        );
                        let verdict = if agrees_with_port { "match" } else { "differs" };
                        text.push_str(&format!(", port {port_mm} {verdict}"));
                    }
                    if let Some(why) = reference.known_difference {
                        text.push_str(&format!(", known difference: {why}"));
                    }
                    text
                }
                None => "no reference".to_owned(),
            };
            table.push_str(&format!(
                "{} | {} | {} | {} | krupp {} mass {} kg caliber {} mm speed {} m/s | {} mm | {comparison}\n",
                aircraft.ship.name,
                squadron.aircraft,
                ordnance.kind.label(),
                ordnance.param,
                ap.krupp.expect("krupp"),
                ap.mass_kg.expect("mass").value(),
                ap.caliber_mm.expect("caliber").value(),
                ap.speed_mps.expect("speed").value(),
                penetration.value(),
            ));
        }
    }
    assert_eq!(referenced.len(), REFERENCES.len(), "every reference names a projectile of the listed ships");
    insta::assert_snapshot!(table);
}

/// Raw AP penetration at the impact velocity of the trajectory solution.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ap_shell_raw_penetration_by_range() {
    let mut table = String::new();
    for name in ["Yamato", "Des Moines", "Shimakaze", "Worcester", "Montana"] {
        let shells = game().ship_shells(&ship(name)).expect("ship exists");
        for report in shells.shells.iter().filter(|report| report.shell.ammo == Ammo::Ap) {
            let projectile = game()
                .provider()
                .game_param_by_name(&report.shell.param)
                .and_then(|param| param.projectile().cloned())
                .expect("shell projectile exists");
            let params = projectile
                .to_shell_info(report.shell.param.clone())
                .and_then(|info| ShellParams::from_shell_info(&info))
                .expect("AP shell has ballistic data");

            let mut cells = Vec::new();
            for km in [0.0, 5.0, 10.0, 15.0, 20.0] {
                let Some(impact) = solve_for_range(&params, Km::new(km).to_meters()) else {
                    continue;
                };
                cells.push(format!(
                    "{km} km {:.0} m/s {:.1} mm",
                    impact.impact_velocity.value(),
                    impact.raw_penetration.value()
                ));
            }
            table.push_str(&format!("{} | {} | {}\n", shells.ship.name, report.label, cells.join(" | ")));
        }
    }
    insta::assert_snapshot!(table);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn sekiryu_ap_bombs_citadel_yamato_from_the_deck() {
    let trace = game()
        .armor_trace(&ArmorTraceArgs {
            shooter: "Sekiryu".to_owned(),
            target_ship: "Yamato".to_owned(),
            weapon: Some(TraceWeapon::Bombs),
            ammo: Some(Ammo::Ap),
            ..Default::default()
        })
        .expect("Sekiryu carries AP dive bombers");

    let [ap] = trace.traces.as_slice() else {
        panic!("one trace for the ammo asked for");
    };
    assert!(ap.citadel.detonating_inside > 0);
    insta::assert_yaml_snapshot!(trace);
}

/// Aki's dive bombers carry HE bombs only.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn aki_bombs_are_traced_with_the_ordnance_it_carries() {
    let trace = game()
        .armor_trace(&ArmorTraceArgs {
            shooter: "Aki".to_owned(),
            target_ship: "Vermont".to_owned(),
            weapon: Some(TraceWeapon::Bombs),
            ..Default::default()
        })
        .expect("Aki carries bombs");
    assert!(trace.traces.iter().all(|trace| trace.weapon.ammo == Ammo::He));
    insta::assert_yaml_snapshot!(trace);

    let ap = game()
        .armor_trace(&ArmorTraceArgs {
            shooter: "Aki".to_owned(),
            target_ship: "Vermont".to_owned(),
            weapon: Some(TraceWeapon::Bombs),
            ammo: Some(Ammo::Ap),
            ..Default::default()
        })
        .expect_err("Aki carries no AP bombs");
    assert!(matches!(ap, QueryError::AmmoNotCarried { .. }), "{ap:?}");
    insta::assert_snapshot!("aki_ap_bombs_error", ap.to_string());
}

/// AP is the load whenever it penetrates most of a zone it strikes, gets
/// through the turrets or reaches the citadel often, and the choice names
/// the nearest compared range where it flips. Against a flat Borodino at
/// 12 km Utrecht's 128 mm AP pens the ends and casemate, so AP; a Des Moines
/// turret duel at 8 km is AP too; Des Moines against Yamato is HE.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ammo_choice_follows_where_ap_penetrates() {
    struct Matchup {
        shooter: &'static str,
        target: &'static str,
        range_km: f32,
    }
    let matchups = [
        Matchup { shooter: "Utrecht", target: "Borodino", range_km: 12.0 },
        Matchup { shooter: "Des Moines", target: "Borodino", range_km: 12.0 },
        Matchup { shooter: "Zao", target: "Borodino", range_km: 12.0 },
        Matchup { shooter: "Des Moines", target: "Des Moines", range_km: 8.0 },
        Matchup { shooter: "Des Moines", target: "Yamato", range_km: 12.0 },
    ];
    let choices: Vec<_> = matchups
        .iter()
        .map(|matchup| {
            let trace = game()
                .armor_trace(&ArmorTraceArgs {
                    shooter: matchup.shooter.to_owned(),
                    target_ship: matchup.target.to_owned(),
                    range_km: Some(Km::new(matchup.range_km)),
                    target_angle_deg: Some(Degrees::new(0.0)),
                    ..Default::default()
                })
                .expect("ships resolve");
            format!(
                "{} vs {} at {} km: {}",
                matchup.shooter,
                matchup.target,
                matchup.range_km,
                trace.ammo_choice.unwrap_or_default()
            )
        })
        .collect();
    insta::assert_yaml_snapshot!(choices);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_ap_citadels_a_broadside_hindenburg_at_10_km() {
    let trace = game()
        .armor_trace(&ArmorTraceArgs {
            shooter: "Des Moines".to_owned(),
            target_ship: "Hindenburg".to_owned(),
            ammo: Some(Ammo::Ap),
            range_km: Some(Km::new(10.0)),
            ..Default::default()
        })
        .expect("Des Moines fires AP");

    assert!(trace.traces[0].citadel.detonating_inside > 0);
    insta::assert_yaml_snapshot!(trace);
}

/// The 300 mm belt and the sloped deck behind it keep 203 mm AP out at 10 km.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn friedrich_der_grosse_turtleback_stops_des_moines_ap_at_10_km() {
    let trace = game()
        .armor_trace(&ArmorTraceArgs {
            shooter: "Des Moines".to_owned(),
            target_ship: "Friedrich der Grosse".to_owned(),
            ammo: Some(Ammo::Ap),
            range_km: Some(Km::new(10.0)),
            ..Default::default()
        })
        .expect("Des Moines fires AP");

    let citadel = &trace.traces[0].citadel;
    assert!(citadel.rays_over_citadel > 0);
    assert_eq!(citadel.detonating_inside, 0);
    insta::assert_yaml_snapshot!(trace);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn guns_out_of_range_and_bombless_ships_are_refused() {
    let out_of_range = game().armor_trace(&ArmorTraceArgs {
        shooter: "Des Moines".to_owned(),
        target_ship: "Yamato".to_owned(),
        range_km: Some(Km::new(40.0)),
        ..Default::default()
    });
    assert!(matches!(out_of_range, Err(QueryError::OutOfRange { .. })));

    let no_bombs = game().armor_trace(&ArmorTraceArgs {
        shooter: "Des Moines".to_owned(),
        target_ship: "Yamato".to_owned(),
        weapon: Some(TraceWeapon::Bombs),
        ..Default::default()
    });
    assert!(matches!(no_bombs, Err(QueryError::WeaponNotFound { .. })));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn citadel_profiles_of_a_battleship_a_turtleback_and_cruisers() {
    let profile = |ship: &str| {
        game()
            .citadel_profile(&CitadelProfileArgs {
                ship: ship.to_owned(),
                shooter: Some("Des Moines".to_owned()),
                range_km: Some(Km::new(12.0)),
                ..Default::default()
            })
            .expect("the ship has an armor model")
    };

    let yamato = profile("Yamato");
    assert!(yamato.shape.turtleback.is_none());
    insta::assert_yaml_snapshot!("citadel_profile_yamato", yamato);

    let friedrich = profile("Friedrich der Grosse");
    assert!(friedrich.shape.turtleback.is_some());
    insta::assert_yaml_snapshot!("citadel_profile_friedrich_der_grosse", friedrich);

    let des_moines = profile("Des Moines");
    assert!(des_moines.shape.turtleback.is_none());
    insta::assert_yaml_snapshot!("citadel_profile_des_moines", des_moines);

    let zao = profile("Zao");
    assert!(zao.shape.turtleback.is_none());
    insta::assert_yaml_snapshot!("citadel_profile_zao", zao);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn prins_van_oranje_broadside_stacks_total_every_plate_to_the_citadel() {
    let profile = game()
        .citadel_profile(&CitadelProfileArgs { ship: "Prins van Oranje".to_owned(), ..Default::default() })
        .expect("Prins van Oranje has a citadel");
    let broadside = profile.approaches.first().expect("the profile has a broadside approach");

    for stack in &broadside.stacks {
        assert!(stack.plates.len() >= 2, "a side stack includes outer armor and the citadel");
        assert!(
            stack.plates.last().is_some_and(|plate| plate.zone == "Citadel"),
            "the first citadel plate ends the stack"
        );
        let listed_total: f32 = stack.plates.iter().map(|plate| plate.effective_mm.value()).sum();
        if stack.more_plates == 0 {
            assert!((stack.total_effective_mm.value() - listed_total).abs() < 0.1);
        } else {
            assert!(stack.total_effective_mm.value() >= listed_total);
        }
    }

    insta::assert_yaml_snapshot!(broadside);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn sap_traces_ignore_ifhe_and_destroyers_have_no_citadel() {
    let sap = game()
        .armor_trace(&ArmorTraceArgs {
            shooter: "Venezia".to_owned(),
            target_ship: "Des Moines".to_owned(),
            ammo: Some(Ammo::Sap),
            ifhe: Some(true),
            range_km: Some(Km::new(10.0)),
            ..Default::default()
        })
        .expect("Venezia fires SAP");
    let [sap] = sap.traces.as_slice() else {
        panic!("one trace for the ammo asked for");
    };
    assert_eq!(sap.weapon.ammo, Ammo::Sap);
    assert!(sap.weapon.penetration_mm.is_some());
    assert!(sap.verdict.starts_with("Main battery SAP"), "{}", sap.verdict);

    let destroyer = game().citadel_profile(&CitadelProfileArgs { ship: "Gearing".to_owned(), ..Default::default() });
    assert!(matches!(destroyer, Err(QueryError::NoCitadel { .. })), "{destroyer:?}");
}

/// SAP cannot ricochet off a plate its caliber overmatches. Bow-on, Venezia's
/// 203 mm SAP (overmatching 14 mm) ricochets off Des Moines' steep 27 mm bow,
/// while Marco Polo's 406 mm SAP (overmatching 28 mm) does not.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn sap_does_not_ricochet_off_an_overmatched_bow() {
    let trace = |shooter: &str| {
        let output = game()
            .armor_trace(&ArmorTraceArgs {
                shooter: shooter.to_owned(),
                target_ship: "Des Moines".to_owned(),
                ammo: Some(Ammo::Sap),
                range_km: Some(Km::new(10.0)),
                target_angle_deg: Some(Degrees::new(90.0)),
                ..Default::default()
            })
            .expect("the shooter fires SAP");
        let [trace] = output.traces.as_slice() else {
            panic!("one trace for the ammo asked for");
        };
        trace.clone()
    };
    let bow_ricochets = |trace: &AmmoTrace| {
        let bow = trace.zones.iter().find(|zone| zone.zone == "Bow").expect("rays strike the bow first");
        bow.outcomes.get(&RayOutcome::Ricochet).copied().unwrap_or(0)
    };

    let venezia = trace("Venezia");
    assert!(bow_ricochets(&venezia) > 0, "{:?}", venezia.zones);

    let marco_polo = trace("Marco Polo");
    assert_eq!(bow_ricochets(&marco_polo), 0, "{:?}", marco_polo.zones);
    insta::assert_yaml_snapshot!(json!({
        "venezia": { "weapon": venezia.weapon, "zones": venezia.zones },
        "marco_polo": { "weapon": marco_polo.weapon, "zones": marco_polo.zones, "examples": marco_polo.examples },
    }));
}

fn armor_assessment(ship: &str) -> ArmorAssessmentOutput {
    game()
        .armor_assessment(&ArmorAssessmentArgs { ship: ship.to_owned(), hull: None })
        .expect("the ship has an armor model")
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn armor_assessment_finds_plates_arming_destroyer_ap_fuses() {
    for (ship, snapshot) in [("Khabarovsk", "armor_assessment_khabarovsk"), ("Elbing", "armor_assessment_elbing")] {
        let assessment = armor_assessment(ship);
        assert!(
            assessment.properties.fuse_arming.iter().any(|arming| arming.group.starts_with("destroyer AP")),
            "{ship}: {:?}",
            assessment.properties.fuse_arming
        );
        insta::assert_yaml_snapshot!(snapshot, assessment);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn armor_assessment_of_battleships() {
    let friedrich = armor_assessment("Friedrich der Grosse");
    assert!(friedrich.properties.turtleback.is_some());
    insta::assert_yaml_snapshot!("armor_assessment_friedrich_der_grosse", friedrich);

    let yamato = armor_assessment("Yamato");
    assert!(yamato.properties.turtleback.is_none());
    insta::assert_yaml_snapshot!("armor_assessment_yamato", yamato);

    let kurfurst = armor_assessment("G. Kurfürst");
    assert!(kurfurst.armor_verdict.is_some(), "{kurfurst:#?}");
    insta::assert_yaml_snapshot!("armor_assessment_grosser_kurfurst", kurfurst);
}

/// Armor outliers read from the target's side: each leads with strength or
/// weakness and says what the outcome means for the ship.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn comparison_armor_outliers_lead_with_strength_or_weakness() {
    let output = game()
        .call_tool("ship_comparison", json!({ "class": "battleship", "tier": 10, "ships": ["G. Kurfürst"] }))
        .expect("tier 10 battleships compare");
    let outliers = output["ships"][0]["outliers"].as_array().expect("G. Kurfürst has outliers");
    for outlier in outliers {
        let text = outlier.as_str().expect("outlier text");
        assert!(text.starts_with("strength: ") || text.starts_with("weakness: "), "{text}");
    }
    insta::assert_yaml_snapshot!(outliers);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn armor_assessment_of_cruisers() {
    for (ship, snapshot) in [("Des Moines", "armor_assessment_des_moines"), ("Zao", "armor_assessment_zao")] {
        insta::assert_yaml_snapshot!(snapshot, armor_assessment(ship));
    }
}

/// Ships known for a turtleback and ships known without one, as the armor
/// model reads them.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn turtleback_detection_on_known_ships() {
    let ships = [
        "Bismarck",
        "Tirpitz",
        "Friedrich der Grosse",
        "Grosser Kurfurst",
        "Schlieffen",
        "Kremlin",
        "Sov. Soyuz",
        "Roma",
        "Iowa",
        "Montana",
        "Yamato",
        "Conqueror",
    ];
    let detected: Vec<(String, Option<Turtleback>)> = ships
        .into_iter()
        .map(|ship| {
            let profile = game()
                .citadel_profile(&CitadelProfileArgs { ship: ship.to_owned(), ..Default::default() })
                .expect("the ship has an armor model");
            (profile.ship, profile.shape.turtleback)
        })
        .collect();
    insta::assert_yaml_snapshot!(detected);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn turtleback_search_lists_tier_8_to_10_battleships() {
    let search = game()
        .turtleback_search(&TurtlebackSearchArgs {
            min_tier: Some(Tier(8)),
            max_tier: Some(Tier(10)),
            class: Some(ShipClass::Battleship),
            ..Default::default()
        })
        .expect("armor models are readable");
    assert!(search.ships.iter().any(|ship| ship.contains("F. der Grosse")), "{:?}", search.ships);
    insta::assert_yaml_snapshot!(search);
}

/// Owned ships GameParams marks as replicas (`originShipName`), counted per
/// series with a few examples each.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn replicas_follow_the_origin_field() {
    const EXAMPLES: usize = 4;

    let replicas = game().replicas();
    let mut by_series: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for replica in &replicas {
        let of = replica.replica_of.as_ref().expect("replicas name their original");
        let series = of.series.clone().unwrap_or_else(|| "none".to_owned());
        by_series.entry(series).or_default().push(format!("{} -> {}", replica.name, of.ship));
    }
    let series: serde_json::Map<String, serde_json::Value> = by_series
        .into_iter()
        .map(|(series, names)| {
            let examples: Vec<&String> = names.iter().take(EXAMPLES).collect();
            (series, json!({ "count": names.len(), "examples": examples }))
        })
        .collect();

    insta::assert_yaml_snapshot!(json!({ "total": replicas.len(), "series": series }));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn montana_lists_its_replicas() {
    let found =
        game().find_ships(&wows_query::ships::FindShipsArgs { name: Some("Montana".to_owned()), ..Default::default() });
    let listed: Vec<_> =
        found.ships.iter().map(|found| json!({ "ship": found.ship.name, "replicas": found.ship.replicas })).collect();
    let overview = game().ship_overview(&overview("Montana")).expect("Montana resolves");
    let variants: Vec<_> = overview
        .variants
        .iter()
        .map(|variant| json!({ "ship": variant.name, "group": variant.group, "replica_of": variant.replica_of }))
        .collect();
    let replica = game().ship_summary("BA Montana").expect("BA Montana resolves to itself").ship;

    insta::assert_yaml_snapshot!(json!({ "find_ships": listed, "variants": variants, "replica": replica }));
}

/// Des Moines bow AP overmatch counts with replicas left out and counted.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn penetration_search_leaves_out_replicas() {
    let counts: Vec<_> = [false, true]
        .into_iter()
        .map(|include| {
            let search = game()
                .call_tool(
                    "penetration_search",
                    json!({ "target_ship": "Des Moines", "zone": "bow", "ammo": "AP", "armament": ["main_battery"], "include_replicas": include }),
                )
                .expect("zone resolves");
            let group = &search["groups"][0];
            json!({
                "include_replicas": include,
                "considered": group["considered"],
                "qualifying": group["qualifying"],
                "share_pct_by_class": group["share_pct_by_class"],
            })
        })
        .collect();

    insta::assert_yaml_snapshot!(counts);
}

/// A replica ported to another nation resolves to itself, names its original
/// and its nation, and reports its own armament.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn replicas_resolve_to_themselves() {
    let ships: Vec<_> = ["Xuan Wu", "Iowa"]
        .into_iter()
        .map(|name| {
            let overview = game().ship_overview(&overview(name)).expect("ship resolves");
            json!({
                "ship": overview.ship.name,
                "nation": overview.ship.nation,
                "replica_of": overview.ship.replica_of,
                "main_battery": overview.armament.main_battery,
            })
        })
        .collect();

    insta::assert_yaml_snapshot!(ships);
}

/// A fragment of a replica's name lists its original, and a nation filter
/// that leaves out the original lists the replica itself.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn find_ships_reaches_replicas() {
    let names = |args: wows_query::ships::FindShipsArgs| -> Vec<String> {
        game().find_ships(&args).ships.into_iter().map(|found| found.ship.name).collect()
    };

    let by_fragment = names(wows_query::ships::FindShipsArgs { name: Some("ARP".to_owned()), ..Default::default() });
    assert!(by_fragment.iter().any(|name| name == "Yamato"), "{by_fragment:?}");

    let pan_asia = names(wows_query::ships::FindShipsArgs {
        nation: Some("Pan_Asia".to_owned()),
        class: Some(ShipClass::Battleship),
        ..Default::default()
    });
    assert!(pan_asia.iter().any(|name| name == "Xuan Wu"), "{pan_asia:?}");
}

/// Where a stat shipbuilds.com shows is found here.
enum Coverage {
    /// Compared by stat_search under this stat_names name.
    Searchable(&'static str),

    /// Shown on the ship_stats card but not compared, for this reason.
    CardOnly(&'static str),
}

/// Every stat the wowsbuilds code shows on build and compare pages
/// (`ShipStats::rows` through `analysis/stat_delta.rs`) or ranks in build
/// analysis (`analysis/reference.rs` `extract_stat`, `analysis/rules.rs`).
const SHIPBUILDS_STATS: &[(&str, Coverage)] = &[
    ("durability.health", Coverage::Searchable("durability.health")),
    ("durability.torpedo_protection", Coverage::Searchable("durability.torpedo_protection")),
    ("mobility.speed", Coverage::Searchable("mobility.speed")),
    ("mobility.turning_radius", Coverage::Searchable("mobility.turning_radius")),
    ("mobility.rudder_time", Coverage::Searchable("mobility.rudder_time")),
    ("armor.min", Coverage::Searchable("armor.min")),
    ("armor.max", Coverage::Searchable("armor.max")),
    ("battery.capacity", Coverage::Searchable("battery.capacity")),
    ("battery.regeneration", Coverage::Searchable("battery.regeneration")),
    ("artillery.reload_time", Coverage::Searchable("artillery.reload_time")),
    ("artillery.range", Coverage::Searchable("artillery.range")),
    ("artillery.dispersion", Coverage::Searchable("artillery.dispersion")),
    ("artillery.dispersion_vertical", Coverage::Searchable("artillery.dispersion_vertical")),
    ("artillery.dispersion_at_base", Coverage::Searchable("artillery.dispersion_at_stock_range")),
    ("artillery.dispersion_vertical_at_base", Coverage::Searchable("artillery.dispersion_vertical_at_stock_range")),
    ("artillery.ammo_switch_time", Coverage::Searchable("artillery.ammo_switch_time")),
    ("artillery.gun.caliber", Coverage::Searchable("artillery.gun.caliber")),
    ("artillery.gun.num_barrels", Coverage::Searchable("artillery.gun.num_barrels")),
    ("artillery.gun.num_guns", Coverage::Searchable("artillery.gun.num_guns")),
    ("artillery.gun.rotation_speed", Coverage::Searchable("artillery.gun.rotation_speed")),
    ("artillery.gun.rotation_time", Coverage::Searchable("artillery.gun.rotation_time")),
    ("artillery.shells.damage", Coverage::Searchable("artillery.shells.damage")),
    ("artillery.shells.caliber", Coverage::Searchable("artillery.shells.caliber")),
    ("artillery.shells.speed", Coverage::Searchable("artillery.shells.speed")),
    ("artillery.shells.penetration", Coverage::Searchable("artillery.shells.penetration")),
    ("artillery.shells.burn_chance", Coverage::Searchable("artillery.shells.burn_chance")),
    ("artillery.shells.flood_chance", Coverage::Searchable("artillery.shells.flood_chance")),
    ("artillery.shells.max_ammo", Coverage::Searchable("artillery.shells.max_ammo")),
    ("artillery.shells.disabled_underwater", Coverage::CardOnly("yes or no")),
    ("secondary.range", Coverage::Searchable("secondary.range")),
    ("secondary.reload_time", Coverage::Searchable("secondary.reload_time")),
    ("secondary.dispersion", Coverage::Searchable("secondary.dispersion")),
    ("secondary.dispersion_vertical", Coverage::Searchable("secondary.dispersion_vertical")),
    ("secondary.dispersion_at_base", Coverage::Searchable("secondary.dispersion_at_stock_range")),
    ("secondary.dispersion_vertical_at_base", Coverage::Searchable("secondary.dispersion_vertical_at_stock_range")),
    ("secondary.gun.caliber", Coverage::Searchable("secondary.gun.caliber")),
    ("secondary.gun.num_barrels", Coverage::Searchable("secondary.gun.num_barrels")),
    ("secondary.gun.num_guns", Coverage::Searchable("secondary.gun.num_guns")),
    ("secondary.gun.rotation_speed", Coverage::Searchable("secondary.gun.rotation_speed")),
    ("secondary.gun.rotation_time", Coverage::Searchable("secondary.gun.rotation_time")),
    ("secondary.shells.damage", Coverage::Searchable("secondary.shells.damage")),
    ("secondary.shells.caliber", Coverage::Searchable("secondary.shells.caliber")),
    ("secondary.shells.speed", Coverage::Searchable("secondary.shells.speed")),
    ("secondary.shells.penetration", Coverage::Searchable("secondary.shells.penetration")),
    ("secondary.shells.burn_chance", Coverage::Searchable("secondary.shells.burn_chance")),
    ("secondary.shells.flood_chance", Coverage::Searchable("secondary.shells.flood_chance")),
    ("secondary.shells.max_ammo", Coverage::Searchable("secondary.shells.max_ammo")),
    ("torpedoes.reload_time", Coverage::Searchable("torpedoes.reload_time")),
    ("torpedoes.launchers.rotation_speed", Coverage::Searchable("torpedoes.launchers.rotation_speed")),
    ("torpedoes.launchers.rotation_time", Coverage::Searchable("torpedoes.launchers.rotation_time")),
    ("torpedoes.launchers.num_barrels", Coverage::Searchable("torpedoes.launchers.num_barrels")),
    ("torpedoes.torpedoes.damage", Coverage::Searchable("torpedoes.torpedoes.damage")),
    ("torpedoes.torpedoes.speed", Coverage::Searchable("torpedoes.torpedoes.speed")),
    ("torpedoes.torpedoes.range", Coverage::Searchable("torpedoes.torpedoes.range")),
    ("torpedoes.torpedoes.visibility", Coverage::Searchable("torpedoes.torpedoes.visibility")),
    ("torpedoes.torpedoes.distance_of_max_damage", Coverage::Searchable("torpedoes.torpedoes.distance_of_max_damage")),
    ("torpedoes.torpedoes.is_damage_increasing", Coverage::CardOnly("yes or no")),
    ("torpedoes.torpedoes.disabled_underwater", Coverage::CardOnly("yes or no")),
    ("fire_control.max_dist", Coverage::Searchable("fire_control.max_dist")),
    ("consumable.reload_time", Coverage::Searchable("consumable.reload_time")),
    ("consumable.work_time", Coverage::Searchable("consumable.work_time")),
    ("consumable.charges", Coverage::Searchable("consumable.charges")),
    ("consumable.preparation_time", Coverage::Searchable("consumable.preparation_time")),
    ("consumable.max_capacity", Coverage::Searchable("consumable.capacity")),
    ("consumable.detection_radius", Coverage::Searchable("consumable.detection_radius")),
    ("consumable.regeneration_hp_speed", Coverage::Searchable("consumable.regeneration_hp_speed")),
    ("consumable.smoke_radius", Coverage::Searchable("consumable.smoke_radius")),
    ("consumable.smoke_lifetime", Coverage::Searchable("consumable.smoke_lifetime")),
    ("consumable.fighters_count", Coverage::Searchable("consumable.fighters_count")),
    ("consumable.call_fighters_radius", Coverage::Searchable("consumable.call_fighters_radius")),
    ("consumable.call_fighters_time_delay", Coverage::Searchable("consumable.call_fighters_time_delay")),
    ("consumable.call_fighters_time_from_heaven", Coverage::Searchable("consumable.call_fighters_time_from_heaven")),
    ("consumable.plane_regeneration_rate", Coverage::Searchable("consumable.plane_regeneration_rate")),
    ("visibility.sea_detection", Coverage::Searchable("visibility.sea_detection")),
    ("visibility.sea_detection_on_fire", Coverage::Searchable("visibility.sea_detection_on_fire")),
    ("visibility.air_detection", Coverage::Searchable("visibility.air_detection")),
    ("visibility.air_detection_on_fire", Coverage::Searchable("visibility.air_detection_on_fire")),
    ("visibility.detection_in_smoke", Coverage::Searchable("visibility.detection_in_smoke")),
    ("visibility.main_gun_range_detection", Coverage::Searchable("visibility.main_gun_range_detection")),
    ("visibility.secondary_range_detection", Coverage::Searchable("visibility.secondary_range_detection")),
    ("visibility.periscope_depth_detection", Coverage::Searchable("visibility.periscope_depth_detection")),
    ("reference GunDpm", Coverage::Searchable("artillery.shells.damage_per_minute")),
    ("reference GunAlphaSalvo", Coverage::Searchable("artillery.shells.salvo_damage")),
    ("reference GunFireChance", Coverage::Searchable("artillery.shells.burn_chance")),
    ("reference HePenetration", Coverage::Searchable("artillery.shells.penetration")),
    ("reference MainBatteryCaliber", Coverage::Searchable("artillery.gun.caliber")),
    ("reference TorpedoTubes", Coverage::Searchable("torpedoes.tubes")),
    ("reference TorpedoDetectability", Coverage::Searchable("torpedoes.torpedoes.visibility")),
    ("reference TorpedoRange", Coverage::Searchable("torpedoes.torpedoes.range")),
    ("reference TorpedoDamage", Coverage::Searchable("torpedoes.torpedoes.damage")),
    ("reference TorpedoSpeed", Coverage::Searchable("torpedoes.torpedoes.speed")),
    ("reference submarine_surveil_radius", Coverage::Searchable("consumable.detection_radius")),
    ("rules unburnable_consumable_charge cycle", Coverage::Searchable("consumable.all_charges_time")),
];

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stat_names_cover_the_shipbuilds_stats() {
    let output = game().stat_names(&StatNamesArgs {});
    let names: Vec<&str> =
        output.stats.iter().map(|line| line.split_once(" (").map_or(line.as_str(), |(name, _)| name)).collect();

    let table: Vec<String> = SHIPBUILDS_STATS
        .iter()
        .map(|(stat, coverage)| match coverage {
            Coverage::Searchable(name) => {
                assert!(names.contains(name), "{stat}: {name} is not in stat_names");
                format!("{stat}: {name}")
            }
            Coverage::CardOnly(reason) => {
                assert!(!names.contains(stat), "{stat} is in stat_names");
                format!("{stat}: ship_stats card only, {reason}")
            }
        })
        .collect();
    insta::assert_yaml_snapshot!(table);
}

/// Armor stats rank nominal plate thickness; their labels and rankings say
/// it is not protection.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn armor_stats_are_labelled_nominal() {
    let output = game().stat_names(&StatNamesArgs {});
    let armor: Vec<&String> = output.stats.iter().filter(|line| line.starts_with("armor.")).collect();
    insta::assert_yaml_snapshot!(armor);

    let search = game()
        .stat_search(&StatSearchArgs {
            rank: Some(StatRank {
                stat: "armor.citadel.max".to_owned(),
                order: RankOrder::Desc,
                qualifier: None,
                limit: Some(RankLimit(3)),
                build: None,
                condition: None,
            }),
            min_tier: Some(Tier(10)),
            max_tier: Some(Tier(10)),
            class: Some(ShipClass::Battleship),
            ..Default::default()
        })
        .expect("search runs");
    let ranking = search.ranking.expect("a rank was asked for");
    assert!(ranking.caution.is_some_and(|caution| caution.contains("armor_assessment")), "{:?}", ranking.caution);
}

/// Submarine top speed per depth with the skills and signals that change it,
/// and the ranking of tier 10 submarines by submerged speed.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn submarine_speed_per_depth() {
    for (ship, snapshot) in [("Balao", "submarine_speed_balao"), ("U-2501", "submarine_speed_u_2501")] {
        let stats =
            game().ship_stats(&ShipStatsArgs { ship: ship.to_owned(), ..Default::default() }).expect("ship exists");
        let speed = stats.build_stats.expect("no loadout gives build_stats").speed;
        insta::assert_yaml_snapshot!(snapshot, speed);
    }

    let search = game()
        .stat_search(&StatSearchArgs {
            rank: Some(StatRank {
                stat: "underwater_mobility.speed".to_owned(),
                order: RankOrder::Desc,
                qualifier: None,
                limit: Some(RankLimit(25)),
                build: None,
                condition: None,
            }),
            min_tier: Some(Tier(10)),
            max_tier: Some(Tier(10)),
            class: Some(ShipClass::Submarine),
            ..Default::default()
        })
        .expect("search runs");
    insta::assert_yaml_snapshot!("submarine_submerged_speed_ranking", search.ranking.expect("a rank was asked for"));
}

/// Top speed with the conditions that raise it only part of a battle:
/// Engine Boost, signals, conditional skills, special abilities and, with a
/// commander, talents. Each is an alternative; the best-fit build is
/// always on.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn conditional_speed_alternatives() {
    for (ship, commander, snapshot) in [
        ("Shimakaze", None, "conditional_speed_shimakaze"),
        ("Kleber", None, "conditional_speed_kleber"),
        ("Kleber", Some("Auboyneau"), "conditional_speed_kleber_auboyneau"),
        ("Smolensk", None, "conditional_speed_smolensk"),
        ("Yamato", None, "conditional_speed_yamato"),
        ("Orzel", None, "conditional_speed_orzel"),
        ("Halland", None, "conditional_speed_halland"),
        ("Affondatore", None, "conditional_speed_affondatore"),
    ] {
        let stats = game()
            .ship_stats(&ShipStatsArgs {
                ship: ship.to_owned(),
                commander: commander.map(str::to_owned),
                ..Default::default()
            })
            .expect("ship exists");
        let speed = stats.build_stats.expect("no loadout gives build_stats").speed;
        insta::assert_yaml_snapshot!(snapshot, speed);
    }
}

/// Tier 10 destroyers ranked by top speed with Engine Boost active; ships
/// without one rank on their always-on best build.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn speed_ranking_with_engine_boost() {
    let rank = |stat: &str, build: Option<RankBuild>| StatRank {
        stat: stat.to_owned(),
        order: RankOrder::Desc,
        qualifier: None,
        limit: Some(RankLimit(10)),
        build,
        condition: Some("Engine Boost".to_owned()),
    };
    let search = |rank: StatRank| {
        game().stat_search(&StatSearchArgs {
            rank: Some(rank),
            min_tier: Some(Tier(10)),
            max_tier: Some(Tier(10)),
            class: Some(ShipClass::Destroyer),
            ..Default::default()
        })
    };

    let ranking = search(rank("mobility.speed", None)).expect("search runs").ranking.expect("a rank was asked for");
    insta::assert_yaml_snapshot!("destroyer_speed_ranking_with_engine_boost", ranking);

    assert!(matches!(search(rank("visibility.sea_detection", None)), Err(QueryError::RankConditionNotSpeed { .. })));
    assert!(matches!(search(rank("mobility.speed", Some(RankBuild::Base))), Err(QueryError::RankConditionWithBuild)));
}

/// The fastest surface ships with every condition that can hold at once:
/// speed boost, conditional skills whose triggers hold together, signals
/// and special ability. Swift in Silence never stacks with Dazzle.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fastest_ships_with_every_condition() {
    let ranking = game()
        .stat_search(&StatSearchArgs {
            rank: Some(StatRank {
                stat: "mobility.speed".to_owned(),
                order: RankOrder::Desc,
                qualifier: None,
                limit: Some(RankLimit(5)),
                build: None,
                condition: Some("all".to_owned()),
            }),
            ..Default::default()
        })
        .expect("search runs")
        .ranking
        .expect("a rank was asked for");

    let kleber = game()
        .stat_search(&StatSearchArgs {
            rank: Some(StatRank {
                stat: "mobility.speed".to_owned(),
                order: RankOrder::Desc,
                qualifier: None,
                limit: Some(RankLimit(25)),
                build: None,
                condition: Some("all".to_owned()),
            }),
            min_tier: Some(Tier(10)),
            max_tier: Some(Tier(10)),
            class: Some(ShipClass::Destroyer),
            ..Default::default()
        })
        .expect("search runs")
        .ranking
        .expect("a rank was asked for")
        .ships
        .into_iter()
        .find(|ship| ship.ship.starts_with("T10 Kl") && ship.ship.contains("(destroyer, France)"))
        .expect("Kleber is ranked");
    assert_eq!(kleber.value, "58.7 kn");
    assert_eq!(kleber.build, ["Swift in Silence", "Engine Boost", "Sierra Mike signal"]);
    assert!(ranking.ships.iter().all(|ship| !(ship.build.iter().any(|item| item == "Dazzle")
        && ship.build.iter().any(|item| item == "Swift in Silence"))));
    insta::assert_yaml_snapshot!("fastest_surface_ships_with_every_condition", ranking);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ship_stats_of_each_class() {
    for (ship, snapshot) in [
        ("Shimakaze", "ship_stats_shimakaze"),
        ("Des Moines", "ship_stats_des_moines"),
        ("Yamato", "ship_stats_yamato"),
        ("Midway", "ship_stats_midway"),
        ("Balao", "ship_stats_balao"),
    ] {
        let stats = game()
            .ship_stats(&ShipStatsArgs { ship: ship.to_owned(), full_card: Some(true), ..Default::default() })
            .expect("ship exists");
        insta::assert_yaml_snapshot!(snapshot, stats.stats);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn derived_stats_change_with_skills_and_upgrades() {
    let stats = game()
        .ship_stats(&ShipStatsArgs {
            ship: "Des Moines".to_owned(),
            upgrades: Some(vec!["Main Battery Modification 3".to_owned(), "Aiming Systems Modification 1".to_owned()]),
            ..Default::default()
        })
        .expect("Des Moines takes both upgrades");
    insta::assert_yaml_snapshot!(stats.changes);
}

/// Every fire and flooding value released ships' hulls carry, by class and
/// tier: fire chance reduction, fire duration and damage per second.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fire_resistance_by_class_and_tier() {
    const RELEASED: &[&str] =
        &["upgradeable", "premium", "special", "specialUnsellable", "ultimate", "superShip", "start"];

    let mut table: BTreeMap<String, BTreeMap<_, BTreeSet<String>>> = BTreeMap::new();
    for param in game().provider().params().iter() {
        let (Some(vehicle), Some(species)) = (param.vehicle(), param.species().and_then(|species| species.known()))
        else {
            continue;
        };
        if !RELEASED.contains(&vehicle.group()) {
            continue;
        }
        let Some(components) = vehicle.ttx_components() else {
            continue;
        };

        for hull in components.hulls.values() {
            let (Some(fire), Some(flood)) = (hull.burn_nodes.first(), hull.flood_nodes.first()) else {
                continue;
            };
            let entry = format!(
                "fire chance reduction {:.2}%, {} fires of {} s at {}%/s; {} floods of {} s at {}%/s",
                100.0 * (1.0 - fire.probability),
                hull.burn_nodes.len(),
                fire.duration.value(),
                fire.damage_per_sec.value(),
                hull.flood_nodes.len(),
                flood.duration.value(),
                flood.damage_per_sec.value(),
            );
            table.entry(format!("{species:?}")).or_default().entry(vehicle.level()).or_default().insert(entry);
        }
    }
    insta::assert_yaml_snapshot!(table);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fire_and_flooding_of_yamato_and_kongo() {
    let fire_lines = |ship: &str, skills: &[&str], upgrades: &[&str]| {
        let to_strings =
            |names: &[&str]| (!names.is_empty()).then(|| names.iter().map(|name| (*name).to_owned()).collect());
        let stats = game()
            .ship_stats(&ShipStatsArgs {
                ship: ship.to_owned(),
                skills: to_strings(skills),
                upgrades: to_strings(upgrades),
                full_card: Some(true),
                ..Default::default()
            })
            .expect("ship and loadout resolve");
        let lines: Vec<String> = stats
            .stats
            .into_iter()
            .filter(|line| line.starts_with("Fire") || line.starts_with("Flood") || line.starts_with("Damage of one"))
            .collect();
        json!({ "ship": ship, "lines": lines, "changes": stats.changes })
    };

    insta::assert_yaml_snapshot!([
        fire_lines("Yamato", &[], &[]),
        fire_lines("Kongo", &[], &[]),
        fire_lines(
            "Yamato",
            &["Fire Prevention Expert", "Basics of Survivability"],
            &["Damage Control System Modification 1", "Damage Control System Modification 2"],
        ),
    ]);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_he_fire_chance_against_yamato_and_a_tier_6_cruiser() {
    let against = |target: &str, skills: &[&str]| {
        let arcs = game()
            .ship_firing_arcs(&FiringArcsArgs {
                ship: "Des Moines".to_owned(),
                target_ship: Some(target.to_owned()),
                target_skills: (!skills.is_empty()).then(|| skills.iter().map(|skill| (*skill).to_owned()).collect()),
                ..Default::default()
            })
            .expect("firing arcs resolve");
        let main = battery(&arcs, Weapon::MainBattery);
        let he = main.ammo.iter().find(|ammo| ammo.ammo == "HE").expect("Des Moines fires HE").clone();
        (arcs.target, he.projectiles, he.arcs.starboard, he.arcs.bow)
    };

    insta::assert_yaml_snapshot!([
        against("Yamato", &[]),
        against("Yamato", &["Fire Prevention Expert"]),
        against("Leander", &[]),
    ]);
}

/// Thunderer's HE salvo fire chance by arc, with no target: p is the shell's
/// own fire chance.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn thunderer_he_salvo_fire_chance() {
    let arcs = firing_arcs("Thunderer", None, &[]);
    let main = battery(&arcs, Weapon::MainBattery);
    let he = main.ammo.iter().find(|ammo| ammo.ammo == "HE").expect("Thunderer fires HE");

    insta::assert_yaml_snapshot!((&he.projectiles, &he.arcs));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn unique_commanders_show_improved_skills_and_talents() {
    for (commander, snapshot) in [
        ("Jean-Jacques Honore", "commander_honore"),
        ("Yamamoto Isoroku", "commander_yamamoto"),
        ("Halsey", "commander_halsey"),
        ("John Doe", "commander_john_doe"),
    ] {
        let output = game()
            .commander(&CommanderArgs { commander: commander.to_owned(), ship: None })
            .expect("commander resolves");
        insta::assert_yaml_snapshot!(snapshot, output);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn honore_improves_adrenaline_rush_on_colbert() {
    for (commander, snapshot) in
        [(None, "colbert_adrenaline_rush_standard"), (Some("Jean-Jacques Honore"), "colbert_adrenaline_rush_honore")]
    {
        let stats = game()
            .ship_stats(&ShipStatsArgs {
                ship: "Colbert".to_owned(),
                skills: Some(vec!["Adrenaline Rush".to_owned()]),
                upgrades: Some(vec!["Main Battery Modification 3".to_owned()]),
                hp_percent: Some(Percent::new(1.0)),
                commander: commander.map(str::to_owned),
                ..Default::default()
            })
            .expect("Colbert takes the skill and upgrade");
        insta::assert_yaml_snapshot!(snapshot, stats);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn yamamoto_talents_are_listed_as_alternatives_on_yamato() {
    let stats = game()
        .ship_stats(&ShipStatsArgs {
            ship: "Yamato".to_owned(),
            skills: Some(vec!["Adrenaline Rush".to_owned()]),
            commander: Some("Yamamoto Isoroku".to_owned()),
            ..Default::default()
        })
        .expect("Yamamoto commands Yamato");
    insta::assert_yaml_snapshot!(stats.talents);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn topete_talents_leave_damage_control_and_repair_party_reload_on_castilla() {
    let stats = game()
        .ship_stats(&ShipStatsArgs {
            ship: "Castilla".to_owned(),
            commander: Some("Pascual Cervera y Topete".to_owned()),
            ..Default::default()
        })
        .expect("Topete commands Castilla");
    insta::assert_yaml_snapshot!(stats.talents);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn commander_search_finds_hp_restoration() {
    let search = game()
        .commander_search(&CommanderSearchArgs { effect: Some("restores HP".to_owned()), ..Default::default() })
        .expect("search runs");
    insta::assert_yaml_snapshot!(search);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn arms_race_buffs_per_class_and_stack() {
    let buffs = game()
        .game_mode_buffs(&GameModeBuffsArgs { mode: Some("Arms Race".to_owned()), ship: None })
        .expect("Arms Race has buffs");
    insta::assert_yaml_snapshot!(buffs);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn akizuki_healing_sources() {
    let sources =
        game().healing_sources(&HealingSourcesArgs { ship: Some("Akizuki".to_owned()) }).expect("Akizuki exists");
    insta::assert_yaml_snapshot!(sources);
}

/// Republique and Montana, a French and an American battleship, saturate
/// alike; Henri IV is a French cruiser. Kleber's midsection saturates
/// (French destroyer saturation); Gearing's does not, nor does that of Aigle,
/// a French destroyer without it.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn hit_locations_saturate_by_class() {
    for ship in ["Republique", "Montana", "Henri IV", "Kleber", "Gearing", "Aigle"] {
        let sections = game()
            .ship_hit_locations(&ShipHitLocationsArgs { ship: ship.to_owned() })
            .unwrap_or_else(|error| panic!("{ship}: {error}"));
        insta::assert_yaml_snapshot!(format!("hit_locations_{}", ship.replace(' ', "_")), sections);
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn ships_that_heal_allies() {
    let sources = game().healing_sources(&HealingSourcesArgs { ship: None }).expect("search runs");
    insta::assert_yaml_snapshot!(sources);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn new_jersey_consumables_heal_allies() {
    let consumables = game()
        .ship_consumables(&ShipConsumablesArgs { ship: "New Jersey".to_owned(), ..Default::default() })
        .expect("New Jersey exists");
    insta::assert_yaml_snapshot!(consumables);
}

/// 20 de Julio's fourth slot holds a time-based Main Battery Reload Booster:
/// a pool of seconds of use with no charges or action time.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn twenty_de_julio_consumables_include_time_based_reload_booster() {
    let consumables =
        game().call_tool("ship_consumables", json!({ "ship": "20 de Julio" })).expect("20 de Julio exists");

    let booster = &consumables["slots"][3]["options"][0];
    assert_eq!(booster["type"], "artilleryBoosters", "{consumables:#}");
    assert_eq!(booster["capacity_s"], 80.0, "{booster:#}");
    assert!(booster.get("charges").is_none(), "{booster:#}");
    insta::assert_yaml_snapshot!(consumables);
}

/// AP damage rates of 20 de Julio with its time-based booster active, with
/// and without Combat Instructions, and of Kleber with its count-based one.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn main_battery_damage_with_reload_booster_active() {
    let ap_rates = |ship: &str, special_ability_active: bool| -> serde_json::Value {
        let stats = game()
            .call_tool("ship_stats", json!({ "ship": ship, "special_ability_active": special_ability_active }))
            .expect("ship exists");
        let ap = stats["build_stats"]["main_battery_damage"]
            .as_array()
            .expect("ship has main battery damage")
            .iter()
            .find(|ammo| ammo["ammo"] == "AP")
            .expect("ship fires AP")
            .clone();
        json!({ "special_ability": stats["special_ability"], "AP": ap })
    };

    let julio = ap_rates("20 de Julio", false);
    let julio_with_instructions = ap_rates("20 de Julio", true);
    for rates in [&julio, &julio_with_instructions] {
        let builds = &rates["AP"]["rates"]["builds"];
        let boosted = builds["alternatives"]
            .as_array()
            .expect("alternatives listed")
            .iter()
            .find(|alternative| alternative["label"] == "with Main Battery Reload Booster active")
            .expect("booster alternative listed");
        let best = builds["best"]["value"]["damage_per_minute"].as_f64().expect("best rate");
        let active = boosted["value"]["damage_per_minute"].as_f64().expect("booster rate");
        assert!((active / best - 2.0).abs() < 0.01, "{rates:#}");
    }
    insta::assert_yaml_snapshot!("twenty_de_julio_ap_damage_with_reload_booster", julio);
    insta::assert_yaml_snapshot!(
        "twenty_de_julio_ap_damage_with_instructions_and_reload_booster",
        julio_with_instructions
    );

    let kleber = game().call_tool("ship_stats", json!({ "ship": "Kleber" })).expect("Kleber exists");
    let alternatives = kleber["build_stats"]["main_battery_damage"][0]["rates"]["builds"]["alternatives"]
        .as_array()
        .expect("alternatives listed");
    assert!(
        alternatives.iter().any(|alternative| alternative["label"] == "with Main Battery Reload Booster active"),
        "{alternatives:#?}"
    );
}

/// Valparaiso's Repair Party: HP per charge and per battle on the stock and
/// best-fit builds.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn valparaiso_repair_party_heal() {
    let consumables = game()
        .call_tool("ship_consumables", json!({ "ship": "Valparaiso", "consumable": "Repair Party" }))
        .expect("Valparaiso has a Repair Party");
    let accented = game()
        .call_tool("ship_consumables", json!({ "ship": "Valpara\u{ed}so", "consumable": "Repair Party" }))
        .expect("the accented name resolves");
    assert_eq!(consumables["slots"], accented["slots"]);

    insta::assert_yaml_snapshot!(consumables);
}

/// Every squadron's ordnance states its fire chance, 0 when it cannot start
/// fires, with the best-fit build that raises it.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn carrier_ordnance_lists_its_fire_chance() {
    let lines = |ship: &str| -> Vec<serde_json::Value> {
        let aircraft = game().call_tool("ship_aircraft", json!({ "ship": ship })).expect("ship has aircraft");
        aircraft["squadrons"]
            .as_array()
            .expect("squadrons is a list")
            .iter()
            .map(|squadron| {
                json!({
                    "role": squadron["role"],
                    "aircraft": squadron["aircraft"],
                    "ordnance": squadron["ordnance"]["name"],
                    "kind": squadron["ordnance"]["kind"],
                    "fire_chance_pct": squadron["ordnance"]["fire_chance_pct"],
                    "fire_chance": squadron["fire_chance"]["builds"],
                })
            })
            .collect()
    };

    for (ship, snapshot) in [
        ("United States", "carrier_ordnance_fire_chance_united_states"),
        ("Midway", "carrier_ordnance_fire_chance_midway"),
        ("Saipan", "carrier_ordnance_fire_chance_saipan"),
        ("E. Loewenhardt", "carrier_ordnance_fire_chance_loewenhardt"),
    ] {
        insta::assert_yaml_snapshot!(snapshot, lines(ship));
    }
}

/// The ten bombs and rockets with the highest fire chance per projectile,
/// each naming the ordnance and what carries it.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn top_ten_aircraft_ordnance_by_fire_chance() {
    let search = game()
        .call_tool(
            "stat_search",
            json!({ "rank": { "stat": "aircraft.fire_chance", "order": "desc", "limit": 10 }, "exclude_test_ships": true }),
        )
        .expect("rank resolves");
    let ranking = &search["ranking"];
    let ships: Vec<String> = ranking["ships"]
        .as_array()
        .expect("ships is a list")
        .iter()
        .map(|ship| {
            format!(
                "{} {}: {} ({}), build {}",
                ship["rank"],
                ship["ship"].as_str().unwrap_or_default(),
                ship["value"].as_str().unwrap_or_default(),
                ship["row"].as_str().unwrap_or_default(),
                ship["build"],
            )
        })
        .collect();

    insta::assert_yaml_snapshot!(json!({ "stat": ranking["stat"], "ships": ships }));
}

/// AA bands, flak, priority sector and rating of an AA cruiser, a gun
/// cruiser, a battleship and a torpedo destroyer, on their best-fit builds.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn anti_aircraft_on_the_best_builds() {
    for name in ["Des Moines", "Yamato", "Shimakaze", "Worcester"] {
        let stats =
            game().ship_stats(&ShipStatsArgs { ship: name.to_owned(), ..Default::default() }).expect("ship exists");
        let anti_aircraft = stats.build_stats.and_then(|builds| builds.anti_aircraft).expect("the ship has AA");
        insta::assert_yaml_snapshot!(format!("anti_aircraft_{}", name.replace(' ', "_")), anti_aircraft);
    }
}

/// Tier 10 ships of each class ranked by AA rating, strongest and weakest.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tier_10_anti_aircraft_rating_by_class() {
    let rank = |class: ShipClass, order: RankOrder, limit: usize| -> Vec<String> {
        let search = game()
            .stat_search(&StatSearchArgs {
                rank: Some(StatRank {
                    stat: "aa.rating".to_owned(),
                    order,
                    qualifier: None,
                    limit: Some(RankLimit(limit)),
                    build: None,
                    condition: None,
                }),
                min_tier: Some(Tier(10)),
                max_tier: Some(Tier(10)),
                class: Some(class),
                ..Default::default()
            })
            .expect("search runs");
        let ranking = search.ranking.expect("a rank was asked for");

        ranking
            .ships
            .iter()
            .map(|ship| format!("{} {}: {} (base {})", ship.rank, ship.ship, ship.value, ship.base))
            .collect()
    };

    let mut table: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (label, class) in [
        ("destroyers", ShipClass::Destroyer),
        ("cruisers", ShipClass::Cruiser),
        ("battleships", ShipClass::Battleship),
        ("aircraft carriers", ShipClass::AircraftCarrier),
    ] {
        table.insert(format!("{label} strongest"), rank(class, RankOrder::Desc, 10));
        table.insert(format!("{label} weakest"), rank(class, RankOrder::Asc, 5));
    }
    insta::assert_yaml_snapshot!(table);
}
