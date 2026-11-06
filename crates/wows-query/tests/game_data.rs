//! Queries over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use serde_json::json;
use wows_core::units::Millimeters;
use wows_query::GameData;
use wows_query::consumables::ShipConsumablesArgs;
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

    let groups: Vec<_> = search.groups.iter().map(|group| (group.method, group.total)).collect();
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
