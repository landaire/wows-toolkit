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
use wows_query::armor_assessment::ArmorAssessmentArgs;
use wows_query::armor_assessment::ArmorAssessmentOutput;
use wows_query::armor_trace::AmmoTrace;
use wows_query::armor_trace::ArmorTraceArgs;
use wows_query::armor_trace::RayOutcome;
use wows_query::armor_trace::TraceWeapon;
use wows_query::citadel::CitadelProfileArgs;
use wows_query::citadel::Turtleback;
use wows_query::commanders::CommanderArgs;
use wows_query::commanders::CommanderSearchArgs;
use wows_query::consumables::ShipConsumablesArgs;
use wows_query::firing_arcs::BatteryArcs;
use wows_query::firing_arcs::FiringArcsArgs;
use wows_query::firing_arcs::ShipFiringArcs;
use wows_query::firing_arcs::Weapon;
use wows_query::game_modes::GameModeBuffsArgs;
use wows_query::healing::HealingSourcesArgs;
use wows_query::penetration::PenetrationSearchArgs;
use wows_query::shells::Ammo;
use wows_query::shells::Battery;
use wows_query::ships::ShipArgs;
use wows_query::ships::ShipClass;
use wows_query::ships::ShipOverviewArgs;
use wows_query::ships::Tier;
use wows_query::skills::CaptainSkillArgs;
use wows_query::skills::SkillTreeArgs;
use wows_query::stat_search::StatNamesArgs;
use wows_query::stats::ShipStatsArgs;
use wows_query::tech_tree::LineOrder;
use wows_query::tech_tree::TechTreeLinesArgs;
use wows_query::turtleback::TurtlebackSearchArgs;
use wowsunpack::game_params::types::GameParamProvider;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
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
            ..Default::default()
        })
        .expect("zone resolves");
    assert!(search.groups[0].ships.iter().any(|ship| ship.ends_with(", overmatches")), "{:?}", search.groups[0].ships);
    insta::assert_yaml_snapshot!(search);
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
        .penetration_search(&PenetrationSearchArgs { thickness_mm: Some(Millimeters::new(57.0)), ..Default::default() })
        .expect("thickness is valid");

    let groups: Vec<_> = search
        .groups
        .iter()
        .map(|group| json!({ "method": group.method, "qualifying": group.qualifying, "considered": group.considered }))
        .collect();
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
    let costs: Vec<_> = ["Zao", "Kurama", "Montana", "Maine"]
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
        .tech_tree_lines(&TechTreeLinesArgs { limit: Some(5), ..Default::default() })
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
            min_tier: Some(Tier(4)),
            ..Default::default()
        })
        .expect("tiers 4 to 10 are valid");

    insta::assert_yaml_snapshot!(lines);
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

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn penetration_search_states_shares_per_class() {
    // Through call_tool, which prints each f32 share at its own precision.
    let search = game()
        .call_tool(
            "penetration_search",
            json!({ "target_ship": "Des Moines", "zone": "bow", "ammo": "AP", "min_tier": 8, "max_tier": 11 }),
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
                    found.ship.tier.0,
                    found.ship.name,
                    found.ship.index,
                    found.ship.category,
                    found.name_match
                )
            })
            .collect()
    };
    let listed: Vec<String> = [("Essex", false), ("Essex", true), ("old Essex", false), ("Zao", true), ("[Zao]", false)]
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
    for name in ["Yamato", "Shimakaze", "Smaland", "Orzel", "Barfleur"] {
        insta::assert_yaml_snapshot!(format!("weapon_rates_{name}"), rates(name));
    }
}

/// Barfleur's one loader serves both launchers; Orzel's bow and stern groups
/// reload two launchers each; Shimakaze reloads every launcher at once.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn torpedoes_per_minute_follow_the_loaders() {
    let base = |name: &str| {
        let rate = rates(name).torpedoes_per_minute.expect("the ship has torpedoes");
        (rate.tubes.0, rate.rate.builds.base.value())
    };

    assert_eq!(base("Barfleur"), (10, 5.26));
    assert_eq!(base("Orzel"), (12, 7.74));
    assert_eq!(base("Shimakaze"), (15, 6.0));
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

/// The 15 ships that start the most fires on their best build, each
/// armament with its own best value, against the 15 with the highest main
/// battery shell fire chance, and the 15 cruisers that start the most fires.
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
    // "T10 Des Moines (cruiser, USA)" names Des Moines.
    let name = |ship: &serde_json::Value| -> String {
        let label = ship["ship"].as_str().expect("ship is a label");
        let (_, rest) = label.split_once(' ').expect("tier prefix");
        rest.rsplit_once(" (").map_or(rest, |(name, _)| name).to_owned()
    };

    let by_fires: Vec<serde_json::Value> = rank("fires_per_minute.total", None)
        .iter()
        .map(|ship| {
            let split: Vec<String> = fire_rates(&name(ship))
                .sources
                .iter()
                .map(|source| format!("{}: {}", source.source, source.rate.builds.best.value.fires_per_minute.value()))
                .collect();
            json!({ "rank": ship["rank"], "ship": ship["ship"], "fires_per_minute": ship["value"], "by_armament": split })
        })
        .collect();
    let line = |ship: &serde_json::Value| {
        format!("{} {} {}", ship["rank"], ship["ship"].as_str().unwrap_or_default(), ship["value"])
    };
    let cruisers: Vec<String> = rank("fires_per_minute.total", Some("cruiser")).iter().map(line).collect();
    let by_shell: Vec<String> = rank("artillery.shells.burn_chance", None).iter().map(line).collect();

    assert!(cruisers.iter().any(|ship| ship.contains("Prins van Oranje")), "{cruisers:?}");
    insta::assert_yaml_snapshot!(json!({
        "by_fire_starts": by_fires,
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
fn grosser_kurfurst_resolves_from_abbreviations() {
    let resolved: Vec<_> = ["GK", "grober", "Grober Kurfurst", "Groer Kurfurst"]
        .into_iter()
        .map(|name| {
            let skill = game()
                .captain_skill(&CaptainSkillArgs {
                    skill: "IFA".to_owned(),
                    ship: Some(name.to_owned()),
                    ..Default::default()
                })
                .expect("names resolve");
            json!({ "ship": skill.ship.map(|ship| ship.name), "interpretations": skill.interpretations })
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
fn stat_search_needing_a_skill_flags_its_trigger() {
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

fn undetermined(search: &serde_json::Value) -> u64 {
    search.get("undetermined").and_then(|ships| ships.as_array()).map_or(0, |ships| ships.len() as u64)
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
                    json!({ "target_ship": "Des Moines", "zone": "bow", "ammo": "AP", "include_replicas": include }),
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
    ("consumable.max_capacity", Coverage::Searchable("consumable.max_capacity")),
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
        (arcs.target, he.projectiles, he.all_barrels)
    };

    insta::assert_yaml_snapshot!([
        against("Yamato", &[]),
        against("Yamato", &["Fire Prevention Expert"]),
        against("Leander", &[]),
    ]);
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
