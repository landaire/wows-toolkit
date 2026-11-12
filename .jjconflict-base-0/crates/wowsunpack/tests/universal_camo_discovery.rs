//! Integration test for universal-camo discovery via MSkin + isTileflage.
//!
//! Gated `#[ignore]`: requires a real game install. Run with:
//!   cargo test -p wowsunpack --test universal_camo_discovery -- --ignored --nocapture
//!
//! Install resolution (mirrors ttx_real_provider.rs): `$WOWS_DIR` is the
//! `World_of_Warships` root, defaulting to `E:\WoWs\World_of_Warships`.

use std::path::Path;

use wowsunpack::export::gltf_export::CamoOrigin;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::export::ship::ShipExportOptions;

// Model directory name (assets.bin path component), not a GameParams index.
// Confirmed against the current install: `export-ship "PJSB018_Yamato_1944"` no
// longer resolves there ("No ship found..."), but this model dir does.
const SHIP: &str = "JSB039_Yamato_1945";

fn load_test_ship_assets() -> ShipAssets {
    let wows_dir = std::env::var("WOWS_DIR").unwrap_or_else(|_| r"E:\WoWs\World_of_Warships".to_string());
    let wows_dir = Path::new(&wows_dir);
    if !wows_dir.is_dir() {
        panic!("WoWs dir not found: {}; set WOWS_DIR to your World_of_Warships install", wows_dir.display());
    }
    eprintln!("loading ShipAssets from {}", wows_dir.display());
    ShipAssets::from_game_dir(wows_dir).expect("failed to load ShipAssets from game install")
}

#[test]
#[ignore = "requires a real game install; run with --ignored"]
fn universal_camos_use_mskin_tileflage() {
    let assets = load_test_ship_assets();
    let options = ShipExportOptions { textures: false, ..Default::default() };
    let ctx = assets.load_ship(SHIP, &options).expect("load ship");
    let infos = ctx.camo_texture_source().expect("camo source").scheme_infos();

    assert!(!infos.is_empty(), "Yamato should offer camo schemes");
    eprintln!("Yamato schemes ({}):", infos.len());
    for i in &infos {
        eprintln!("  {} ({:?})", i.display_name, i.origin);
    }

    assert!(
        infos.iter().any(|i| i.origin == CamoOrigin::Universal),
        "expected at least one universal (MSkin + isTileflage) scheme, got: {:?}",
        infos.iter().map(|i| (&i.display_name, i.origin)).collect::<Vec<_>>()
    );
    assert!(
        !infos.iter().any(|i| i.display_name.contains("ShipDestruction")),
        "death skins must not be listed as camos"
    );
}
