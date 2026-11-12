//! Armored buildings over a real install. Ignored: needs an install named by
//! `WOWS_DIR`; returns early when that directory is absent.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wowsunpack::export::building_armor::BuildingArmorIndex;
use wowsunpack::export::model_catalog::BuildingKind;
use wowsunpack::export::ship::ShipAssets;

#[test]
#[ignore = "requires a World of Warships install"]
fn forts_list_their_armored_mounts_with_thickness() {
    let dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    if !dir.join("bin").is_dir() {
        eprintln!("skipped: no install at {}", dir.display());
        return;
    }
    let assets = ShipAssets::from_game_dir(&dir).expect("ship assets");
    let library = assets.model_library().expect("model library");

    let started = std::time::Instant::now();
    let index = BuildingArmorIndex::build(assets.metadata(), &library);
    eprintln!("[measured] armored buildings found in {:?}", started.elapsed());

    let mut kinds: BTreeMap<&BuildingKind, Vec<&str>> = BTreeMap::new();
    for building in index.buildings() {
        kinds.entry(building.kind()).or_default().push(building.param().name());
        let armored: Vec<String> = building
            .mounts()
            .iter()
            .filter(|mount| mount.has_armor())
            .map(|mount| format!("{} {} (map: {})", mount.hardpoint(), mount.model().stem(), mount.armor().is_some()))
            .collect();
        eprintln!(
            "  {} {:?}: hull {:?} plates {}, hull map {}, armored mounts {armored:?}",
            building.param().name(),
            building.kind(),
            building.model().map(|model| model.stem()),
            building.hull_has_armor(),
            building.armor().is_some()
        );
    }
    for (kind, names) in &kinds {
        eprintln!("{kind:?}: {}", names.len());
    }
    assert!(!index.buildings().is_empty(), "some buildings carry armor");

    let fort = index
        .buildings()
        .iter()
        .find(|building| building.kind() == &BuildingKind::Complex)
        .expect("a fort carries armor");
    assert_eq!(index.building(fort.id()).map(|b| b.param()), Some(fort.param()));
    let mount = fort.mounts().iter().find(|mount| mount.has_armor()).expect("an armored fort mount");
    let thickest = mount
        .armor()
        .into_iter()
        .flat_map(|map| map.values())
        .flat_map(|layers| layers.values())
        .copied()
        .fold(0.0f32, f32::max);
    assert!(thickest > 0.0, "{} {} lists thickness", fort.param().name(), mount.hardpoint());
}
