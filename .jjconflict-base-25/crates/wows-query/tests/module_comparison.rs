//! `module_comparison` over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use wows_query::GameData;
use wows_query::module_comparison::ModuleComparisonArgs;
use wows_query::modules::ModuleKind;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
        .expect("dump root has a readable builds.toml");
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

/// G. Kurfurst's stock 406 mm guns against its 420 mm guns.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn grosser_kurfurst_guns() {
    let output = GAME
        .module_comparison(&ModuleComparisonArgs {
            ship: "GK".to_owned(),
            kind: Some(ModuleKind::Guns),
            ..Default::default()
        })
        .expect("both gun modules compare");

    insta::assert_yaml_snapshot!(output);
}

/// Shimakaze's torpedo modules.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn shimakaze_torpedoes() {
    let output = GAME
        .module_comparison(&ModuleComparisonArgs {
            ship: "Shimakaze".to_owned(),
            kind: Some(ModuleKind::Torpedoes),
            ..Default::default()
        })
        .expect("the torpedo modules compare");

    insta::assert_yaml_snapshot!(output);
}

/// Phoenix's hulls carry different main batteries and torpedo launchers.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn phoenix_hulls() {
    let output = GAME
        .module_comparison(&ModuleComparisonArgs {
            ship: "Phoenix".to_owned(),
            kind: Some(ModuleKind::Hull),
            ..Default::default()
        })
        .expect("the hulls compare");

    insta::assert_yaml_snapshot!(output);
}
