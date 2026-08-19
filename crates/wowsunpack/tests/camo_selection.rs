//! Selected camos, and only those, reach the GLB. Ignored: needs an install.

use std::path::PathBuf;

use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::ship::CamoSelection;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::ship::ShipModelContext;
use wowsunpack::game_params::types::GameParamProvider;

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn load(options: &ShipExportOptions) -> ShipModelContext {
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains("WSD011_Smaland_1955")).unwrap_or(false))
        .cloned()
        .expect("no vehicle for Smaland");
    assets.load_ship_from_vehicle(&vehicle, options).expect("ctx")
}

/// The `KHR_materials_variants` names declared at the glTF root.
fn variant_names(glb: &[u8]) -> Vec<String> {
    let g = gltf::Gltf::from_slice(glb).expect("parse glb");
    let json = g.document.into_json();
    let raw = serde_json::to_value(&json).expect("to json value");
    raw.pointer("/extensions/KHR_materials_variants/variants")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|e| e.get("name")?.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[test]
#[ignore = "requires a World of Warships install"]
fn base_only_emits_no_variants() {
    let options = ShipExportOptions { camos: CamoSelection::BaseOnly, ..Default::default() };
    let mut out = Vec::new();
    load(&options).export_glb(&mut out).expect("export");
    assert!(variant_names(&out).is_empty(), "base-only emits no variants extension");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn variants_emits_exactly_the_selected_ids() {
    let probe = load(&ShipExportOptions { textures: false, ..Default::default() });
    let source = probe.camo_texture_source().expect("source");
    let infos = source.scheme_infos();
    assert!(infos.len() >= 5, "Smaland should offer several schemes");

    // Non-adjacent ids, so a shifted enumeration cannot pass by accident.
    let picked = vec![infos[0].id, infos[3].id];
    let expected: Vec<String> =
        picked.iter().map(|id| infos.iter().find(|i| i.id == *id).unwrap().display_name.clone()).collect();

    let options = ShipExportOptions { camos: CamoSelection::Variants(picked), ..Default::default() };
    let mut out = Vec::new();
    load(&options).export_glb(&mut out).expect("export");

    let mut got = variant_names(&out);
    let mut want = expected;
    got.sort();
    want.sort();
    assert_eq!(got, want, "exactly the selected schemes reach the file");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn an_unknown_id_is_an_error_not_a_silent_drop() {
    let options = ShipExportOptions { camos: CamoSelection::Variants(vec![CamoSchemeId(9999)]), ..Default::default() };
    let mut out = Vec::new();
    let result = load(&options).export_glb(&mut out);
    assert!(result.is_err(), "an id that names no scheme is a caller bug, not a silent empty export");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn baking_emits_no_variants_and_changes_the_base() {
    let probe = load(&ShipExportOptions { textures: false, ..Default::default() });
    let source = probe.camo_texture_source().expect("source");
    let id = source.scheme_infos().first().expect("at least one scheme").id;

    let mut stock = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::BaseOnly, ..Default::default() })
        .export_glb(&mut stock)
        .expect("stock");

    let mut baked = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::Baked(id), ..Default::default() })
        .export_glb(&mut baked)
        .expect("baked");

    assert!(variant_names(&baked).is_empty(), "a baked camo needs no variants extension");
    assert_ne!(baked, stock, "baking changes the base material's textures");
}

#[test]
#[ignore = "requires a World of Warships install"]
fn baking_one_camo_beats_carrying_it_as_a_variant() {
    let probe = load(&ShipExportOptions { textures: false, ..Default::default() });
    let source = probe.camo_texture_source().expect("source");
    let id = source.scheme_infos().first().expect("at least one scheme").id;

    let mut baked = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::Baked(id), ..Default::default() })
        .export_glb(&mut baked)
        .expect("baked");
    let mut variant = Vec::new();
    load(&ShipExportOptions { camos: CamoSelection::Variants(vec![id]), ..Default::default() })
        .export_glb(&mut variant)
        .expect("variant");

    assert!(baked.len() < variant.len(), "baking carries one texture set, a variant carries two");
}
