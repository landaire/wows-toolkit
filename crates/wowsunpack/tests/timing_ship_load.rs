//! Ad-hoc profiling harness: times each model-load step with game data resident.
//! Run: WOWS_DIR=E:\WoWs\World_of_Warships SHIP=BSB009_Conqueror_1949 \
//!   cargo test -p wowsunpack --test timing_ship_load -- --ignored --nocapture

use std::path::PathBuf;
use std::time::Instant;

use wowsunpack::export::ship::{ShipAssets, ShipExportOptions};
use wowsunpack::game_params::types::GameParamProvider;

#[test]
#[ignore = "requires a World of Warships install; profiling only"]
fn time_ship_load() {
    let game_dir =
        std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"));
    let ship = std::env::var("SHIP").unwrap_or_else(|_| "BSB009_Conqueror_1949".to_string());

    let t = Instant::now();
    let assets = ShipAssets::from_game_dir(&game_dir).expect("load ship assets");
    eprintln!("[from_game_dir] {:?}", t.elapsed());

    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(&ship)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle matching {ship}"));

    let opts = ShipExportOptions { lod: 0, textures: false, ..Default::default() };
    let t = Instant::now();
    let ctx = assets.load_ship_from_vehicle(&vehicle, &opts).expect("ctx");
    eprintln!("[load_ship_from_vehicle] {:?}", t.elapsed());

    let t = Instant::now();
    let hull_meshes = ctx.interactive_hull_meshes().expect("hull meshes");
    eprintln!("[interactive_hull_meshes] {:?} ({} meshes)", t.elapsed(), hull_meshes.len());

    let t = Instant::now();
    let albedos = ctx.hull_base_albedos(&hull_meshes);
    eprintln!("[hull_base_albedos] {:?} ({} textures)", t.elapsed(), albedos.len());

    let t = Instant::now();
    let source = ctx.camo_texture_source().expect("camo source");
    let infos = source.scheme_infos();
    eprintln!("[camo_texture_source + scheme_infos] {:?} ({} schemes, 0 decoded)", t.elapsed(), infos.len());
}
