//! `gun_module_matchups` over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use wows_query::GameData;
use wows_query::gun_module_matchups::GunModuleMatchupsArgs;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
        .expect("dump root has a readable builds.toml");
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

/// G. Kurfurst's stock 406 mm and alternative 420 mm guns against the tier
/// 10 battleships, its own class and tier, at the default 12 km.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn grosser_kurfurst_gun_modules_against_tier_10_battleships() {
    let output = GAME
        .gun_module_matchups(&GunModuleMatchupsArgs { ship: "GK".to_owned(), ..Default::default() })
        .expect("both gun modules trace");

    assert_eq!(output.modules.len(), 2);
    assert!(output.modules[0].stock);
    insta::assert_yaml_snapshot!(output);
}

/// A ship with one gun module has nothing to compare it with.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn single_gun_module_has_no_differences() {
    let output = GAME
        .gun_module_matchups(&GunModuleMatchupsArgs { ship: "Yamato".to_owned(), ..Default::default() })
        .expect("the gun module traces");

    assert_eq!(output.modules.len(), 1);
    assert!(output.differences.is_empty());
    insta::assert_yaml_snapshot!(output);
}
