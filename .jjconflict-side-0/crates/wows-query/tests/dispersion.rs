//! `dispersion_at_range` over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use wows_core::units::Km;
use wows_query::GameData;
use wows_query::dispersion::CompareGroup;
use wows_query::dispersion::DispersionAtRangeArgs;
use wows_query::dispersion::DispersionBattery;
use wows_query::ships::ShipClass;
use wows_query::ships::Tier;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
        .expect("dump root has a readable builds.toml");
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

fn args(ship: &str, range_km: f32, battery: DispersionBattery) -> DispersionAtRangeArgs {
    DispersionAtRangeArgs {
        ship: ship.to_owned(),
        range_km: Km::new(range_km),
        battery: Some(battery),
        modules: None,
        build: None,
        compare: None,
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn main_battery_dispersion_with_and_without_range_modifiers() {
    for ship in ["Yamato", "Des Moines"] {
        for range_km in [12.0, 15.0] {
            let output = GAME
                .dispersion_at_range(&args(ship, range_km, DispersionBattery::MainBattery))
                .expect("dispersion resolves");
            insta::assert_yaml_snapshot!(format!("{}_main_battery_at_{range_km}_km", ship.replace(' ', "_")), output);
        }
    }
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn secondary_dispersion_with_and_without_modifiers() {
    for range_km in [6.0, 10.0] {
        let output = GAME
            .dispersion_at_range(&args("Hannover", range_km, DispersionBattery::Secondary))
            .expect("dispersion resolves");
        insta::assert_yaml_snapshot!(format!("Hannover_secondary_at_{range_km}_km"), output);
    }
}

/// Montana against Yamato at 12 km differs in sigma as well as ellipse size.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn montana_main_battery_at_12_km() {
    let output =
        GAME.dispersion_at_range(&args("Montana", 12.0, DispersionBattery::MainBattery)).expect("dispersion resolves");
    insta::assert_yaml_snapshot!(output);
}

/// Bismarck carries two secondary calibers; the conclusion names the one it reads.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn secondary_dispersion_per_caliber() {
    let output =
        GAME.dispersion_at_range(&args("Bismarck", 6.0, DispersionBattery::Secondary)).expect("dispersion resolves");
    insta::assert_yaml_snapshot!(output);
}

/// Cleveland's upgraded fire control module lengthens main battery range.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn fire_control_module_is_a_range_source() {
    let output = GAME
        .dispersion_at_range(&args("Cleveland", 10.0, DispersionBattery::MainBattery))
        .expect("dispersion resolves");
    insta::assert_yaml_snapshot!(output);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn range_past_every_build_is_reported_as_out_of_reach() {
    let output = GAME
        .dispersion_at_range(&args("Des Moines", 25.0, DispersionBattery::MainBattery))
        .expect("dispersion resolves");
    insta::assert_yaml_snapshot!(output.conclusion);
}

fn tier_10_battleships() -> CompareGroup {
    CompareGroup { class: ShipClass::Battleship, min_tier: Tier(10), max_tier: Tier(10) }
}

/// 24 km is slightly past Stalingrad's range with the range upgrade, so the
/// ellipse is taken at its max range and ranked among tier 10 battleships.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn stalingrad_with_range_mod_against_tier_10_battleships_at_24_km() {
    let output = GAME
        .dispersion_at_range(&DispersionAtRangeArgs {
            build: Some("range mod".to_owned()),
            compare: Some(tier_10_battleships()),
            ..args("Stalingrad", 24.0, DispersionBattery::MainBattery)
        })
        .expect("dispersion resolves");
    insta::assert_yaml_snapshot!(output);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn yamato_against_tier_10_battleships_at_12_km() {
    let output = GAME
        .dispersion_at_range(&DispersionAtRangeArgs {
            compare: Some(tier_10_battleships()),
            ..args("Yamato", 12.0, DispersionBattery::MainBattery)
        })
        .expect("dispersion resolves");
    insta::assert_yaml_snapshot!(output);
}
