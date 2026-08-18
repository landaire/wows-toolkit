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

fn node_count(glb: &[u8]) -> usize {
    let g = gltf::Gltf::from_slice(glb).expect("parse glb");
    g.nodes().count()
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

    // Equality does not hold on Smaland's hull sub-model: its own LOD 0 entry
    // is a dedup stub (0 primitives, superseded by the Bow/MidBack/MidFront/
    // Stern regions), while its LOD 1 entry is a distinct low-poly far mesh.
    // Clamping into that entry at a deep request legitimately adds a node
    // instead of dropping one, so the invariant under test is "never fewer",
    // not "always equal".
    assert!(node_count(&lod3) >= node_count(&lod0), "a deep lod clamps parts, it does not drop them");
    assert!(lod3.len() < lod0.len(), "a deep lod is smaller than lod 0");
}
