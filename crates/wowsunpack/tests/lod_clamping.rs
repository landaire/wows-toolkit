//! A deep LoD request must not drop parts. Ignored: needs an install.

use std::path::PathBuf;

use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::game_params::types::GameParamProvider;

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn load(ship: &str, options: &ShipExportOptions) -> ShipModelContext {
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(ship)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle for {ship}"));
    assets.load_ship_from_vehicle(&vehicle, options).expect("ctx")
}

/// Names of every node in the file, so a part that vanished can be identified
/// rather than merely counted.
fn node_names(glb: &[u8]) -> std::collections::BTreeSet<String> {
    let g = gltf::Gltf::from_slice(glb).expect("parse glb");
    g.nodes().filter_map(|n| n.name().map(str::to_string)).collect()
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_deep_lod_keeps_every_part() {
    let base = ShipExportOptions { lod: 0, textures: false, ..Default::default() };
    let mut lod0 = Vec::new();
    load("WSD011_Smaland_1955", &base).export_glb(&mut lod0).expect("lod 0");

    let deep = ShipExportOptions { lod: 3, textures: false, ..Default::default() };
    let mut lod3 = Vec::new();
    load("WSD011_Smaland_1955", &deep).export_glb(&mut lod3).expect("lod 3");

    // Node counts alone don't hold: Smaland's hull sub-model has an empty LOD
    // 0 entry (a dedup stub, superseded by the Bow/MidBack/MidFront/Stern
    // regions) and a non-empty LOD 1 entry (a distinct low-poly far mesh), so
    // clamping into it at a deep request legitimately adds a node. The
    // invariant under test is "no part disappears", which is why this
    // compares node names rather than counts.
    let at_lod0 = node_names(&lod0);
    let at_lod3 = node_names(&lod3);
    let dropped: Vec<&String> = at_lod0.difference(&at_lod3).collect();
    assert!(dropped.is_empty(), "a deep lod clamped these parts away instead of using their deepest: {dropped:?}");
    assert!(lod3.len() < lod0.len(), "a deep lod is smaller than lod 0");
}
