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
fn grosser_kurfurst_range_mod_comparison_defaults_to_12_km() {
    let request: DispersionAtRangeArgs = serde_json::from_value(serde_json::json!({
        "ship": "GK",
        "build": "range mod",
    }))
    .expect("the default range is accepted");
    assert_eq!(request.range_km, Km::new(12.0));

    let output = GAME.dispersion_at_range(&request).expect("range mod dispersion resolves");
    assert_eq!(output.headline.as_deref(), Some("Gun Fire Control System Modification 2"));
    insta::assert_yaml_snapshot!(output.conclusion);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn base_build_request_does_not_compare_base_with_itself() {
    let output = GAME
        .dispersion_at_range(&DispersionAtRangeArgs {
            build: Some("base".to_owned()),
            ..args("GK", 12.0, DispersionBattery::MainBattery)
        })
        .expect("base dispersion resolves");

    assert_eq!(output.headline.as_deref(), Some("base"));
    assert!(!output.conclusion.contains("without base"));
    insta::assert_yaml_snapshot!(output.conclusion);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn range_mod_comparison_names_different_clamped_distances() {
    let output = GAME
        .dispersion_at_range(&DispersionAtRangeArgs {
            build: Some("range mod".to_owned()),
            ..args("GK", 21.0, DispersionBattery::MainBattery)
        })
        .expect("range mod dispersion resolves");

    assert!(output.conclusion.contains("builds are evaluated at different distances"));
    insta::assert_yaml_snapshot!(output.conclusion);
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

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn selected_skill_uses_the_normal_effects_pipeline() {
    let skill = "Manual Secondary Battery Aiming";
    let request =
        DispersionAtRangeArgs { build: Some(skill.to_owned()), ..args("Hannover", 6.0, DispersionBattery::Secondary) };
    let selected = GAME.dispersion_at_range_with_skill(&request, &GAME).expect("selected skill dispersion resolves");
    let normal = GAME.dispersion_at_range(&request).expect("normal dispersion resolves");
    let normal_row = normal
        .builds
        .iter()
        .find(|row| row.build == skill && row.condition.is_some())
        .expect("normal output lists the active skill");

    assert_eq!(&selected.builds[1], normal_row);
    assert_eq!(
        selected.builds[1].condition.as_deref(),
        Some("at the priority target within secondary range, after sustained fire")
    );
    assert_eq!(selected.headline.as_deref(), Some(skill));
    assert!(selected.mechanics_version.is_some());
    // Today's copy of the skill is not combined with the selected one.
    assert!(selected.builds.iter().all(|row| row.build.matches(skill).count() <= 1), "{:#?}", selected.builds);
    assert!(
        selected.conclusion.contains("Condition: at the priority target within secondary range, after sustained fire")
    );
}

/// A removed skill on today's ship also gets a row on top of each range and
/// accuracy build: Thor's longer range with Gun Fire Control System
/// Modification 2 lowers vertical dispersion at 18 km, with Dead Eye as
/// without it.
#[test]
#[cfg_attr(not(all(has_build_13533378, has_build_3747819)), ignore)]
fn removed_skill_combines_with_range_builds() {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml")).expect("readable builds.toml");
    let entry = index.find_by_build(3747819).expect("dump root lists build 3747819");
    let dead_eye_build =
        GameData::load_dump(&root.join(&entry.dir), Some(&std::env::temp_dir().join("wows-query-test-cache")))
            .expect("dump loads");
    let request = DispersionAtRangeArgs {
        build: Some("Dead Eye".to_owned()),
        ..args("Thor", 18.0, DispersionBattery::MainBattery)
    };

    let output = GAME.dispersion_at_range_with_skill(&request, &dead_eye_build).expect("Dead Eye dispersion resolves");

    insta::assert_snapshot!(output.conclusion, @"At 18 km, Thor's main battery dispersion without Dead Eye is 240 m horizontal and 135.8 m vertical; with Dead Eye it is 216 m horizontal and 122.2 m vertical (-10% horizontal, -10% vertical). Condition: Increases the firing accuracy of the main battery if there are no visible enemy ships within your ship's standard detectability range. Changes to ship detectability caused by firing main guns, fires onboard, and other combat events are not taken into account. With Gun Fire Control System Modification 2 + Dead Eye (25.17 km range): 216 m horizontal and 117.3 m vertical. With Aiming Systems Modification 1 + Dead Eye (21.7 km range): 200.9 m horizontal and 113.7 m vertical. With Gun Fire Control System Modification 2 + Aiming Systems Modification 1 + Dead Eye (25.17 km range): 200.9 m horizontal and 109.1 m vertical.");
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
