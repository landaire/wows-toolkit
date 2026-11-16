//! Ship part listings and loaded ship parts over a real install. Ignored: needs
//! an install named by `WOWS_DIR`; returns early when that directory is absent.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

use wowsunpack::export::hardpoint::HardpointKind;
use wowsunpack::export::model_library::ModelLibrary;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::ship_parts::MiscParent;
use wowsunpack::export::ship_parts::ShipPart;
use wowsunpack::export::ship_parts::ShipPartKey;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Vehicle;

const IOWA_DIR: &str = "ASB028_Iowa_1945";

struct Fixture {
    assets: ShipAssets,
    library: ModelLibrary,
}

static FIXTURE: LazyLock<Option<Fixture>> = LazyLock::new(|| {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return None;
    }
    let assets = ShipAssets::from_game_dir(&dir).expect("ship assets");
    let library = assets.model_library().expect("model library");
    Some(Fixture { assets, library })
});

fn find_vehicle<'a>(assets: &'a ShipAssets, model_dir: &str) -> Option<&'a Vehicle> {
    let needle = format!("/{model_dir}/");
    assets
        .metadata()
        .params()
        .iter()
        .filter_map(|param| param.vehicle())
        .find(|vehicle| vehicle.model_path().is_some_and(|path| path.contains(&needle)))
}

fn vehicle<'a>(assets: &'a ShipAssets, model_dir: &str) -> &'a Vehicle {
    find_vehicle(assets, model_dir).unwrap_or_else(|| panic!("{model_dir} not in GameParams"))
}

#[test]
#[ignore = "requires a World of Warships install"]
fn iowa_lists_its_hull_sections_mounts_and_miscs() {
    let Some(fixture) = FIXTURE.as_ref() else {
        return;
    };
    let iowa = vehicle(&fixture.assets, IOWA_DIR);

    let listing = fixture.library.list_ship_parts(iowa, None, &HashMap::new()).expect("Iowa lists");

    let mut text = String::new();
    for section in &listing.hull_sections {
        writeln!(text, "hull {section}").expect("write");
    }
    for mount in &listing.mounts {
        writeln!(text, "mount {} {:?} {}", mount.hp, mount.group, mount.model).expect("write");
    }
    for misc in &listing.miscs {
        let parent = match &misc.parent {
            MiscParent::HullSection(section) => section.stem().to_string(),
            MiscParent::Mount(hp) => hp.to_string(),
        };
        writeln!(text, "misc {parent} {} {}", misc.node, misc.model).expect("write");
    }
    insta::assert_snapshot!(text);
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_loaded_ship_reports_the_listed_parts() {
    let Some(fixture) = FIXTURE.as_ref() else {
        return;
    };
    let iowa = vehicle(&fixture.assets, IOWA_DIR);
    let listing = fixture.library.list_ship_parts(iowa, None, &HashMap::new()).expect("Iowa lists");
    let options = ShipExportOptions { textures: false, ..ShipExportOptions::default() };
    let ship = fixture.assets.load_ship_from_vehicle(iowa, &options).expect("Iowa loads");
    assert_same_parts(IOWA_DIR, &listing.keys().collect(), ship.parts());

    let turret = ship
        .parts()
        .iter()
        .find(|part| part.key == ShipPartKey::Mount(wowsunpack::export::hardpoint::HardpointName::new("HP_AGM_1")))
        .expect("forward turret");
    let placement = turret.placement.expect("a mount is placed");
    let hull_node = ship
        .parts()
        .iter()
        .filter(|part| matches!(part.key, ShipPartKey::HullSection(_)))
        .flat_map(|part| &part.hardpoints)
        .find(|hardpoint| hardpoint.name.as_str() == "HP_AGM_1")
        .expect("a hull section carries the turret's hardpoint");
    assert_eq!(hull_node.position(), [placement[12], placement[13], placement[14]], "the mount sits on its node");
    assert!(
        turret.hardpoints.iter().any(|hardpoint| hardpoint.kind == HardpointKind::Mount),
        "the turret carries its own mount nodes"
    );
    let misc =
        ship.parts().iter().find(|part| matches!(part.key, ShipPartKey::Misc { .. })).expect("Iowa carries miscs");
    assert!(misc.model.is_some(), "a misc names its model");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn every_ship_lists_its_parts_quickly() {
    let Some(fixture) = FIXTURE.as_ref() else {
        return;
    };
    let vehicles: Vec<&Vehicle> = fixture
        .assets
        .metadata()
        .params()
        .iter()
        .filter_map(|param| param.vehicle())
        .filter(|vehicle| vehicle.model_path().is_some())
        .collect();

    let started = std::time::Instant::now();
    let mut parts = 0usize;
    let mut failures = Vec::new();
    for vehicle in &vehicles {
        match fixture.library.list_ship_parts(vehicle, None, &HashMap::new()) {
            Ok(listing) => parts += listing.keys().count(),
            Err(error) => failures.push(format!("{:?}: {error}", vehicle.model_path())),
        }
    }
    let elapsed = started.elapsed();
    eprintln!("[measured] {} ships, {parts} parts listed in {elapsed:?}", vehicles.len());

    assert!(failures.is_empty(), "{failures:#?}");
    assert!(elapsed.as_secs_f32() / (vehicles.len() as f32) < 0.01, "listing must stay cheap per ship");
}

/// The listing and the loaded ship agree on every part, with no key twice.
fn assert_same_parts(ship: &str, listed: &BTreeSet<ShipPartKey>, parts: &[ShipPart]) {
    let loaded: BTreeSet<ShipPartKey> = parts.iter().map(|part| part.key.clone()).collect();
    assert_eq!(parts.len(), loaded.len(), "{ship}: part keys are unique");

    let only_listed: Vec<_> = listed.difference(&loaded).collect();
    let only_loaded: Vec<_> = loaded.difference(listed).collect();
    assert!(
        only_listed.is_empty() && only_loaded.is_empty(),
        "{ship}: listed only {only_listed:#?}\nloaded only {only_loaded:#?}"
    );
}

#[test]
#[ignore = "requires a World of Warships install"]
fn listings_match_loads_across_classes() {
    let Some(fixture) = FIXTURE.as_ref() else {
        return;
    };
    let options = ShipExportOptions { textures: false, ..ShipExportOptions::default() };
    let ships = [
        "JSB039_Yamato_1945",
        "JSD012_Shimakaze_1943",
        "ASC011_Helena_1944",
        "GSA001_Graf_Zeppelin_1945",
        "GSS009_U_2501_1945",
    ];
    for model_dir in ships {
        let Some(vehicle) = find_vehicle(&fixture.assets, model_dir) else {
            eprintln!("{model_dir} is not in this build");
            continue;
        };
        let listing = fixture.library.list_ship_parts(vehicle, None, &HashMap::new()).expect("lists");
        let ship = fixture.assets.load_ship_from_vehicle(vehicle, &options).expect("loads");
        assert_same_parts(model_dir, &listing.keys().collect(), ship.parts());
    }

    // Every 40th vehicle in GameParams order, so each build covers a spread of nations and classes.
    let sample = fixture
        .assets
        .metadata()
        .params()
        .iter()
        .filter_map(|param| param.vehicle())
        .filter(|vehicle| vehicle.model_path().is_some())
        .step_by(40);
    for vehicle in sample {
        let name = vehicle.model_path().unwrap_or_default().to_string();
        let listing = fixture.library.list_ship_parts(vehicle, None, &HashMap::new()).expect("lists");
        // Some event and test vehicles have no loadable hull in a given build.
        let Ok(ship) = fixture.assets.load_ship_from_vehicle(vehicle, &options) else {
            eprintln!("{name} does not load");
            continue;
        };
        assert_same_parts(&name, &listing.keys().collect(), ship.parts());
    }
}

/// Benson's hull components attach different deck fittings
/// (`customSkeletonExtenders.battle`), so its two hulls list different miscs.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_hull_component_adds_its_battle_skeleton_extenders() {
    let Some(fixture) = FIXTURE.as_ref() else {
        return;
    };
    let benson = vehicle(&fixture.assets, "ASD049_Benson_1944");
    let hull_miscs = |hull: &str| -> BTreeSet<String> {
        let listing = fixture.library.list_ship_parts(benson, Some(hull), &HashMap::new()).expect("Benson lists");
        listing
            .miscs
            .iter()
            .filter(|misc| matches!(misc.parent, MiscParent::HullSection(_)))
            .map(|misc| misc.node.to_string())
            .collect()
    };

    let mut upgrades: Vec<&String> = benson.hull_upgrades().expect("Benson has hull upgrades").keys().collect();
    upgrades.sort();
    let [first, second] = upgrades[..] else {
        panic!("Benson has two hulls, got {upgrades:?}");
    };
    let a = hull_miscs(first);
    let b = hull_miscs(second);
    assert!(a.iter().any(|node| !b.contains(node)), "hull A attaches fittings hull B lacks");
    assert!(b.iter().any(|node| !a.contains(node)), "hull B attaches fittings hull A lacks");

    let options = ShipExportOptions { textures: false, hull: Some(second.clone()), ..ShipExportOptions::default() };
    let ship = fixture.assets.load_ship_from_vehicle(benson, &options).expect("Benson loads");
    let loaded: BTreeSet<String> = ship
        .parts()
        .iter()
        .filter_map(|part| match &part.key {
            ShipPartKey::Misc { node, parent: MiscParent::HullSection(_) } => Some(node.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(loaded, b, "the loader attaches what the listing names");
}
