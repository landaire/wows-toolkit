//! Queries over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use serde_json::json;
use wows_core::units::Km;
use wows_core::units::Millimeters;
use wows_query::GameData;
use wows_query::QueryError;
use wows_query::armor_trace::ArmorTraceArgs;
use wows_query::armor_trace::TraceWeapon;
use wows_query::citadel::CitadelProfileArgs;
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
use wows_query::ships::ShipOverviewArgs;
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

    let far = resolved("Smolensker").expect_err("two names are equally near");
    let skill = game()
        .captain_skill(&CaptainSkillArgs { skill: "Concealment Expertt".to_owned(), ..Default::default() })
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
