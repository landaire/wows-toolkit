//! quantity_search and quantity_values over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.

use std::path::PathBuf;
use std::sync::LazyLock;

use serde_json::Value;
use serde_json::json;
use wows_query::GameData;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
        .expect("dump root has a readable builds.toml");
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

/// Through the tool registry, which prints each f32 at its own precision.
fn search(query: &str) -> Value {
    GAME.call_tool("quantity_search", json!({ "query": query, "limit": 5 })).expect("search runs")
}

fn values(args: Value) -> Value {
    GAME.call_tool("quantity_values", args).expect("quantities evaluate")
}

/// The first value of the first quantity of the first ship.
fn first_value(output: &Value) -> f64 {
    output["ships"][0]["quantities"][0]["values"][0]["value"].as_f64().expect("a value")
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn search_torpedo_travel_time() {
    let output = search("torpedo travel time");

    assert_eq!(output["quantities"][0]["name"], "torpedo.travel_time");
    insta::assert_yaml_snapshot!(output);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn search_torpedo_spotted() {
    let output = search("torpedo spotted");

    assert_eq!(output["quantities"][0]["name"], "torpedo.detectability");
    insta::assert_yaml_snapshot!(output);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn search_submarine_splash_damage() {
    let output = search("submarine splash damage");

    assert!(output["quantities"][0]["name"].as_str().is_some_and(|name| name.starts_with("underwater.")));
    insta::assert_yaml_snapshot!(output);
}

/// 10 km at 62 game knots takes about 60 s, not the 313 s nautical knots give.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn shimakaze_torpedo_travel_time_to_10_km() {
    let output = values(json!({
        "quantities": [{ "name": "torpedo.travel_time", "params": { "distance_km": 10 } }],
        "ships": ["Shimakaze"],
    }));

    insta::assert_yaml_snapshot!(output);
}

/// 100% up to 8 km, 50% from 8.1 km: half of 19533 at 12 km.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn chateaurenault_torpedo_damage_at_12_km() {
    let output = values(json!({
        "quantities": [{ "name": "torpedo.damage_at_distance", "params": { "distance_km": 12 } }],
        "ships": ["Chateaurenault"],
    }));

    assert_eq!(first_value(&output), 9767.0);
    insta::assert_yaml_snapshot!(output);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn yueyang_torpedo_detectability_with_and_without_vigilance() {
    let without = values(json!({
        "quantities": [{ "name": "torpedo.detectability" }],
        "ships": ["Yueyang"],
    }));
    let with = values(json!({
        "quantities": [{ "name": "torpedo.detectability", "params": { "spotter_class": "battleship" } }],
        "ships": ["Yueyang"],
    }));

    assert!(first_value(&with) > first_value(&without));
    insta::assert_yaml_snapshot!("yueyang_torpedo_detectability_without_vigilance", without);
    insta::assert_yaml_snapshot!("yueyang_torpedo_detectability_with_vigilance", with);
}

/// ship_comparison gives Des Moines flight_time_he 7.55 s at its 12 km
/// cruiser reference range.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_he_flight_time_at_12_km() {
    let output = values(json!({
        "quantities": [{ "name": "shell.flight_time", "params": { "range_km": 12, "ammo": "HE" } }],
        "ships": ["Des Moines"],
    }));
    let factor = values(json!({
        "quantities": [{ "name": "comparison.flight_time_he" }],
        "ships": ["Des Moines"],
    }));

    assert_eq!(first_value(&output), 7.55);
    assert_eq!(first_value(&factor), 7.55);
    insta::assert_yaml_snapshot!(output);
}

/// ship_comparison over tier 8-10 cruisers at a 10 km reference range has
/// 138 ships with flight_time_he, mean 5.48 s and median 5.45 s.
#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn tier_8_to_10_cruiser_he_flight_time_to_10_km() {
    let output = values(json!({
        "quantities": [{ "name": "shell.flight_time", "params": { "range_km": 10, "ammo": "HE" } }],
        "group": { "class": "cruiser", "min_tier": 8, "max_tier": 10 },
    }));

    let summary = &output["group"]["quantities"][0];
    assert_eq!(summary["counted"], 138);
    assert_eq!(summary["mean"], 5.48);
    assert_eq!(summary["median"], 5.45);
    insta::assert_yaml_snapshot!(output);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn grosser_kurfurst_ap_penetration_and_fuse_threshold() {
    let shells = values(json!({
        "quantities": [{ "name": "shell.raw_penetration", "params": { "range_km": 12, "ammo": "AP" } }],
        "ships": ["G. Kurfurst"],
    }));
    let shells_at_10_km = values(json!({
        "quantities": [{ "name": "shell.raw_penetration", "params": { "range_km": 10, "ammo": "AP" } }],
        "ships": ["G. Kurfurst"],
    }));
    let factor = values(json!({
        "quantities": [{ "name": "comparison.ap_penetration" }],
        "ships": ["G. Kurfurst"],
    }));
    let fuse_factor = values(json!({
        "quantities": [{ "name": "comparison.ap_fuse_threshold" }],
        "ships": ["G. Kurfurst"],
    }));

    let shell_traits = GAME.call_tool("ship_shells", json!({ "ship": "G. Kurfurst" })).expect("shells resolve");
    let mut thresholds: Vec<(f64, f64)> = shell_traits["shells"]
        .as_array()
        .expect("shells are listed")
        .iter()
        .filter(|shell| shell["battery"] == "main" && shell["ammo"] == "AP")
        .map(|shell| {
            (
                shell["caliber_mm"].as_f64().expect("AP caliber"),
                shell["fuse_threshold_mm"].as_f64().expect("AP fuse threshold"),
            )
        })
        .collect();

    thresholds.sort_by(|left, right| left.0.total_cmp(&right.0));
    assert_eq!(thresholds, [(406.0, 68.0), (420.0, 70.0)]);
    assert_eq!(first_value(&factor), 554.58);
    assert_eq!(first_value(&fuse_factor), 70.0);
    insta::assert_yaml_snapshot!(shells);
    insta::assert_yaml_snapshot!("grosser_kurfurst_ap_penetration_at_10_km", shells_at_10_km);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn underwater_fields_of_venezia_and_des_moines() {
    let output = values(json!({
        "quantities": [
            { "name": "underwater.splash_coefficient", "params": { "battery": "main" } },
            { "name": "underwater.splash_radius", "params": { "battery": "main" } },
            { "name": "underwater.max_depth", "params": { "battery": "main" } },
            { "name": "underwater.depth_state_damage", "params": { "battery": "main" } },
            { "name": "underwater.class_damage_multiplier", "params": { "battery": "main", "target_class": "submarine" } },
        ],
        "ships": ["Venezia", "Des Moines"],
    }));

    insta::assert_yaml_snapshot!(output);
}
