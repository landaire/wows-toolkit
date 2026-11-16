//! Mesh, armor, and both are three distinct outputs. Ignored: needs an install.

use std::path::PathBuf;

use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::game_params::types::GameParamProvider;

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn export(contents: ExportContents, textures: bool) -> Vec<u8> {
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains("WSD011_Smaland_1955")).unwrap_or(false))
        .cloned()
        .expect("no vehicle for Smaland");
    let options = ShipExportOptions { lod: 0, textures, contents, ..Default::default() };
    let ctx = assets.load_ship_from_vehicle(&vehicle, &options).expect("ctx");
    let mut out = Vec::new();
    ctx.export_glb(&mut out).expect("export");
    out
}

fn mesh_names(glb: &[u8]) -> Vec<String> {
    let g = gltf::Gltf::from_slice(glb).expect("parse glb");
    g.meshes().filter_map(|m| m.name().map(str::to_string)).collect()
}

#[test]
#[ignore = "requires a World of Warships install"]
fn armor_only_carries_armor_and_no_textures() {
    let glb = export(ExportContents::Armor, true);
    let g = gltf::Gltf::from_slice(&glb).expect("parse glb");
    assert_eq!(g.images().count(), 0, "armor is untextured, so an armor-only export embeds no images");
    assert!(mesh_names(&glb).iter().any(|n| n.contains("Armor")), "armor meshes are present");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn mesh_only_carries_no_armor() {
    let glb = export(ExportContents::Mesh, false);
    assert!(!mesh_names(&glb).iter().any(|n| n.contains("Armor")), "a mesh-only export has no armor meshes");
    assert!(!mesh_names(&glb).is_empty(), "a mesh-only export still has parts");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn both_is_the_union() {
    let both = mesh_names(&export(ExportContents::MeshAndArmor, false));
    let mesh = mesh_names(&export(ExportContents::Mesh, false));
    let armor = mesh_names(&export(ExportContents::Armor, false));
    assert_eq!(both.len(), mesh.len() + armor.len());
}
