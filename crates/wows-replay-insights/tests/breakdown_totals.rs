//! Per-type damage lines in the normalized goldens must add up to the totals
//! they break down, so no damage is listed under two keys.
//!
//! Dealt lines are checked against the server's own `damage` total. Received
//! lines are checked against the damage every other player's per-victim
//! breakdown reports dealing to that player.

use std::path::PathBuf;

use serde_json::Value;

fn normalized_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("normalized")
}

fn sum_values(map: &Value) -> u64 {
    map.as_object().expect("breakdown is an object").values().map(|v| v.as_u64().expect("breakdown value")).sum()
}

fn load_goldens() -> Vec<(String, Value)> {
    let mut goldens = Vec::new();
    for entry in std::fs::read_dir(normalized_dir()).expect("normalized fixture directory") {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).expect("read golden");
        goldens.push((name, serde_json::from_slice(&bytes).expect("parse golden")));
    }
    assert!(!goldens.is_empty(), "no normalized goldens found");

    goldens
}

#[test]
fn dealt_breakdown_sums_to_damage_total() {
    for (name, golden) in load_goldens() {
        for player in golden["players"].as_array().expect("players") {
            let server = &player["server_results"];
            let Some(total) = server["damage"].as_u64() else {
                continue;
            };

            let listed = sum_values(&server["damage_by_type"]);
            assert_eq!(listed, total, "{name}: {} dealt breakdown vs damage total", player["name"]);
        }
    }
}

#[test]
fn received_breakdown_sums_to_damage_dealt_by_others() {
    for (name, golden) in load_goldens() {
        let players = golden["players"].as_array().expect("players");
        for victim in players {
            let server = &victim["server_results"];
            if server.is_null() {
                continue;
            }

            let victim_id = victim["db_id"].to_string();
            let dealt_by_others: u64 = players
                .iter()
                .filter_map(|attacker| attacker["server_results"]["damage_interactions"].get(&victim_id))
                .map(|interaction| interaction["damage_dealt"].as_u64().expect("damage_dealt"))
                .sum();

            let listed = sum_values(&server["received_damage_by_type"]);
            assert_eq!(listed, dealt_by_others, "{name}: {} received breakdown vs damage dealt to them", victim["name"]);
        }
    }
}
