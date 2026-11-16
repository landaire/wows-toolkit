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

/// How many nodes carry each name. A multiset, not a set: several sub-models can
/// share a name, and a set would let a surviving twin mask a dropped part.
fn node_name_counts(glb: &[u8]) -> std::collections::BTreeMap<String, usize> {
    let g = gltf::Gltf::from_slice(glb).expect("parse glb");
    let mut counts = std::collections::BTreeMap::new();
    for node in g.nodes() {
        if let Some(name) = node.name() {
            *counts.entry(name.to_string()).or_insert(0) += 1;
        }
    }
    counts
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
    // invariant under test is "no part disappears", so this compares
    // per-name node counts rather than a name set: several sub-models
    // (symmetric mounts, misc parts) share a name, and a plain set would let
    // a surviving twin mask a dropped one.
    let at_lod0 = node_name_counts(&lod0);
    let at_lod3 = node_name_counts(&lod3);
    // A name absent from lod3's map genuinely occurs zero times there, which
    // is exactly the drop this assertion exists to catch.
    let dropped: Vec<String> = at_lod0
        .iter()
        .filter(|(name, count)| at_lod3.get(*name).copied().unwrap_or(0) < **count)
        .map(|(name, count)| format!("{name} ({count} at lod0, {} at lod3)", at_lod3.get(name).copied().unwrap_or(0)))
        .collect();
    assert!(dropped.is_empty(), "a deep lod clamped these parts away instead of using their deepest: {dropped:?}");
    assert!(lod3.len() < lod0.len(), "a deep lod is smaller than lod 0");
}
