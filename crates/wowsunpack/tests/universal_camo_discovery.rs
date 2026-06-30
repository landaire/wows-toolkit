//! Integration test for universal-camo discovery via MSkin + isTileflage.
//!
//! Gated `#[ignore]`: requires a real game install. Run with:
//!   cargo test -p wowsunpack --test universal_camo_discovery -- --ignored --nocapture
//!
//! Install resolution (mirrors ttx_real_provider.rs): `$WOWS_DIR` is the
//! `World_of_Warships` root, defaulting to `E:\WoWs\World_of_Warships`.

use std::path::Path;

use wowsunpack::export::ship::ShipAssets;

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
    let schemes = assets.list_texture_schemes("PJSB018_Yamato_1944").expect("Yamato schemes");
    eprintln!("Yamato schemes ({}):", schemes.len());
    for s in &schemes {
        eprintln!("  {s}");
    }
    assert!(
        schemes.iter().any(|s| s.contains("universal")),
        "expected at least one universal scheme, got: {schemes:?}"
    );
    assert!(!schemes.iter().any(|s| s.contains("ShipDestruction")), "death skins must not be listed as camos");
}
