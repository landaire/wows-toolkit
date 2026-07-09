//! Lazy camo source parity against the eager builder. Ignored: needs an install.

use std::path::PathBuf;

use wowsunpack::export::ship::{ShipAssets, ShipExportOptions};
use wowsunpack::game_params::types::GameParamProvider;

fn game_dir() -> PathBuf {
    std::env::var_os("WOWS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"E:\WoWs\World_of_Warships"))
}

fn load(ship: &str) -> wowsunpack::export::ship::ShipModelContext {
    let assets = ShipAssets::from_game_dir(&game_dir()).expect("assets");
    let vehicle = assets
        .metadata()
        .params()
        .iter()
        .filter_map(|p| p.vehicle())
        .find(|v| v.model_path().map(|mp| mp.contains(ship)).unwrap_or(false))
        .cloned()
        .unwrap_or_else(|| panic!("no vehicle for {ship}"));
    assets
        .load_ship_from_vehicle(&vehicle, &ShipExportOptions { lod: 0, textures: false, ..Default::default() })
        .expect("ctx")
}

#[test]
#[ignore = "requires a World of Warships install"]
fn lazy_decode_matches_eager() {
    let ctx = load("WSD011_Smaland_1955");
    let eager = ctx.build_full_texture_set().expect("eager");
    let source = ctx.camo_texture_source().expect("source");
    let infos = source.scheme_infos();

    // Every eager scheme (matched by display name + origin) must decode identically.
    for (name, eager_tex) in &eager.camo_schemes {
        let info = infos
            .iter()
            .find(|i| &i.display_name == name)
            .unwrap_or_else(|| panic!("lazy source missing scheme {name}"));
        let lazy_tex = source.decode(info.id).expect("decode");
        assert_eq!(&lazy_tex, eager_tex, "texture mismatch for scheme {name}");
    }
}

#[test]
#[ignore = "requires a World of Warships install"]
fn scheme_infos_do_not_decode() {
    // Enumerating metadata must be cheap: it returns a non-empty list for a
    // camo-bearing ship and never panics without touching textures.
    let ctx = load("WSD011_Smaland_1955");
    let source = ctx.camo_texture_source().expect("source");
    let infos = source.scheme_infos();
    assert!(!infos.is_empty(), "Smaland has camos");
}
