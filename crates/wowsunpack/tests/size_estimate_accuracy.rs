//! The estimate must land close to a real export. Ignored: needs an install.

use std::path::PathBuf;

use wowsunpack::export::ship::CamoSelection;
use wowsunpack::export::ship::ExportContents;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::export::texture::MaxEdge;
use wowsunpack::export::texture::TextureLod;
use wowsunpack::game_params::types::GameParamProvider;

/// The tolerance the PNG ratio is calibrated to hold. Widening this is not the
/// fix for a miss; the constant or the model is.
const TOLERANCE: f64 = 0.25;

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn check(ship: &str, options: ShipExportOptions) {
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(ship)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle for {ship}"));
    let ctx = assets.load_ship_from_vehicle(&vehicle, &options).expect("ctx");

    let estimate = ctx.size_model().expect("size model").estimate(&options);
    let mut actual = Vec::new();
    ctx.export_glb(&mut actual).expect("export");

    let est = estimate.total() as f64;
    let act = actual.len() as f64;
    let error = (est - act).abs() / act;
    assert!(
        error <= TOLERANCE,
        "{ship}: estimated {est:.0} bytes ({} geometry, {} textures) against an actual {act:.0}, off by {:.1}%",
        estimate.geometry_bytes,
        estimate.texture_bytes,
        error * 100.0
    );
}

#[test]
#[ignore = "requires a World of Warships install"]
fn stock_at_full_resolution_is_close() {
    check("WSD011_Smaland_1955", ShipExportOptions { camos: CamoSelection::BaseOnly, ..Default::default() });
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_capped_texture_export_is_close() {
    check(
        "WSD011_Smaland_1955",
        ShipExportOptions {
            camos: CamoSelection::BaseOnly,
            texture_lod: TextureLod::Capped(MaxEdge::new(1024).unwrap()),
            ..Default::default()
        },
    );
}

#[test]
#[ignore = "requires a World of Warships install"]
fn an_armor_only_export_is_exact() {
    check(
        "WSD011_Smaland_1955",
        ShipExportOptions { contents: ExportContents::Armor, textures: false, ..Default::default() },
    );
}

#[test]
#[ignore = "requires a World of Warships install"]
fn a_deep_lod_export_is_close() {
    check("WSD011_Smaland_1955", ShipExportOptions { lod: 2, camos: CamoSelection::BaseOnly, ..Default::default() });
}

/// Baking runs above stock: compositing per stem destroys the cross-stem image
/// dedup a shared base/`ImageCache` gives, and a composited pattern compresses
/// worse than a flat albedo. `estimate` prices `Baked` identically to `BaseOnly`
/// (see `size_estimate.rs`), so this case is the one most likely to blow the
/// tolerance and is what calibrated the final `PNG_RATIO`.
#[test]
#[ignore = "requires a World of Warships install"]
fn a_baked_camo_export_is_close() {
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains("WSD011_Smaland_1955")).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle for WSD011_Smaland_1955"));
    let probe = assets
        .load_ship_from_vehicle(&vehicle, &ShipExportOptions { textures: false, ..Default::default() })
        .expect("ctx");
    let id = probe.camo_texture_source().expect("source").scheme_infos().first().expect("at least one scheme").id;

    check("WSD011_Smaland_1955", ShipExportOptions { camos: CamoSelection::Baked(id), ..Default::default() });
}
