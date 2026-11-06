//! Queries over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use serde_json::json;
use wows_core::units::Km;
use wows_core::units::Millimeters;
use wows_query::GameData;
use wows_query::QueryError;
use wows_query::consumables::ShipConsumablesArgs;
use wows_query::firing_arcs::BatteryArcs;
use wows_query::firing_arcs::FiringArcsArgs;
use wows_query::firing_arcs::ShipFiringArcs;
use wows_query::firing_arcs::Weapon;
use wows_query::penetration::PenetrationSearchArgs;
use wows_query::shells::Ammo;
use wows_query::shells::Battery;
use wows_query::ships::ShipArgs;
use wows_query::ships::ShipClass;
use wows_query::skills::CaptainSkillArgs;
use wows_query::stats::ShipStatsArgs;

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

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_bow_is_27_mm_above_water() {
    let armor = game().ship_armor(&ship("Des Moines")).expect("Des Moines exists");

    let bow: Vec<_> = armor.plates.iter().filter(|plate| plate.zone == "Bow").collect();
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
        let shell = shells
            .shells
            .iter()
            .find(|shell| shell.battery == battery && shell.ammo == Ammo::He && shell.caliber_mm == caliber)
            .expect("HE shell present");
        (shell.penetration_mm.map(Millimeters::value), shell.ifhe_penetration_mm.map(Millimeters::value))
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
            .find(|shell| shell.battery == Battery::Main && shell.ammo == Ammo::Ap && shell.caliber_mm == caliber)
            .and_then(|shell| shell.overmatches_up_to_mm)
    };
    assert_eq!(overmatch("Tashkent", Millimeters::new(130.0)), Some(Millimeters::new(9.0)));
    assert_eq!(overmatch("Dunkerque", Millimeters::new(330.0)), Some(Millimeters::new(23.0)));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fpe_names_fire_prevention_expert() {
    let skill = game().captain_skill(&CaptainSkillArgs { skill: "FPE".to_owned(), class: None }).expect("FPE resolves");
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
        .map(|name| game().ship_overview(&ship(name)).expect("ship exists"))
        .collect();

    insta::assert_yaml_snapshot!(overviews);
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
            upgrades: None,
        })
        .expect("loadout resolves");
    insta::assert_yaml_snapshot!(consumables);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn per_class_skill_names_resolve_by_class() {
    let submarine = game()
        .captain_skill(&CaptainSkillArgs { skill: "Adrenaline Rush".to_owned(), class: Some(ShipClass::Submarine) })
        .expect("submarines have Adrenaline Rush");
    let names: Vec<&str> = submarine.skills.iter().map(|skill| skill.internal_name.as_str()).collect();
    assert_eq!(names, ["ArmamentReloadSubmarine"]);

    let ifhe = game().captain_skill(&CaptainSkillArgs { skill: "ifhe".to_owned(), class: None }).expect("IFHE exists");
    insta::assert_yaml_snapshot!(ifhe);
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
    let resolved = |query: &str| game().ship_summary(query).map(|ship| ship.name);
    assert_eq!(resolved("Sekryu").expect("one typo"), "Sekiryu");
    assert_eq!(resolved("Des Moins").expect("one typo"), "Des Moines");
    assert_eq!(resolved("Kurfurts").expect("one transposition"), "G. Kurf\u{fc}rst");

    assert_eq!(resolved("Kitakazee").expect("whole name beats a word of a longer name"), "Kitakaze");

    let far = resolved("Smolensker").expect_err("two names are equally near");
    let skill = game()
        .captain_skill(&CaptainSkillArgs { skill: "Concealment Expertt".to_owned(), class: None })
        .map(|output| output.skills.into_iter().map(|skill| skill.name).collect::<Vec<_>>());
    let upgrade = game()
        .upgrade(&wows_query::upgrades::UpgradeArgs { upgrade: "Main Batery Modification 2".to_owned(), ship: None })
        .map(|output| output.upgrades.into_iter().map(|upgrade| upgrade.name).collect::<Vec<_>>());
    insta::assert_yaml_snapshot!((far.to_string(), skill.expect("one typo"), upgrade.expect("one typo")));
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
    .map(|query| (query, game().ship_summary(query).map(|ship| ship.name).unwrap_or_else(|error| error.to_string())))
    .collect();
    insta::assert_yaml_snapshot!(resolved);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn sekiryu_is_indexed() {
    let found =
        game().find_ships(&wows_query::ships::FindShipsArgs { name: Some("Seki".to_owned()), ..Default::default() });
    let names: Vec<(&str, &str, u32)> =
        found.ships.iter().map(|ship| (ship.name.as_str(), ship.index.as_str(), ship.tier.0)).collect();
    insta::assert_yaml_snapshot!(names);
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
