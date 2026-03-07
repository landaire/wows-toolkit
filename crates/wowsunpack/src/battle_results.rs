//! Utilities for resolving raw battle results from positional arrays into named
//! JSON objects using lookup tables from `constants.json`.
//!
//! The game server sends battle results as compact positional arrays. The
//! `constants.json` file (fetched from wows-constants) provides the mapping
//! from field names to array indices (`CLIENT_PUBLIC_RESULTS_INDICES`) and the
//! ordered field name lists for sub-structures (`COMMON_RESULTS`,
//! `CLIENT_VEH_INTERACTION_DETAILS`).

use serde_json::Value;

/// Resolve raw battle results into named JSON objects.
///
/// # Input shape
///
/// ```json
/// {
///   "commonList": [v0, v1, ...],
///   "playersPublicInfo": {
///     "db_id": [v0, v1, ..., {"victim_id": [...], ...}],
///     ...
///   }
/// }
/// ```
///
/// # Output shape
///
/// ```json
/// {
///   "commonList": { "winner_team_id": v, ... },
///   "playersPublicInfo": {
///     "db_id": { "damage": v, ..., "interactions": { "victim_id": { "fires": v, ... } } },
///     ...
///   }
/// }
/// ```
///
/// The `constants` argument must contain at least:
/// - `/COMMON_RESULTS` — ordered field names for `commonList`
/// - `/CLIENT_PUBLIC_RESULTS_INDICES` — `{ "field_name": index, ... }` for per-player arrays
/// - `/CLIENT_VEH_INTERACTION_DETAILS` — ordered field names for per-victim interaction arrays
pub fn resolve_battle_results(mut results: Value, constants: &Value) -> Value {
    // Resolve commonList: array → object using COMMON_RESULTS names
    if let Some(common_names) = constants.pointer("/COMMON_RESULTS").and_then(|v| v.as_array())
        && let Some(common_arr) = results.get("commonList").and_then(|v| v.as_array())
    {
        results["commonList"] = Value::Object(resolve_array(common_names, common_arr));
    }

    // Resolve each player in playersPublicInfo: array → object using CLIENT_PUBLIC_RESULTS_INDICES
    let indices = constants
        .pointer("/CLIENT_PUBLIC_RESULTS_INDICES")
        .and_then(|v| v.as_object())
        .cloned();
    let interaction_fields = constants
        .pointer("/CLIENT_VEH_INTERACTION_DETAILS")
        .and_then(|v| v.as_array())
        .cloned();

    if let (Some(indices), Some(players)) = (
        indices.as_ref(),
        results
            .get_mut("playersPublicInfo")
            .and_then(|v| v.as_object_mut()),
    ) {
        for (_db_id, player_val) in players.iter_mut() {
            if let Some(arr) = player_val.as_array() {
                let mut obj = serde_json::Map::new();
                for (name, idx_val) in indices {
                    if let Some(idx) = idx_val.as_u64().map(|i| i as usize)
                        && let Some(value) = arr.get(idx)
                    {
                        obj.insert(name.clone(), value.clone());
                    }
                }

                // Resolve interactions: each victim's array → object
                if let Some(fields) = interaction_fields.as_ref()
                    && let Some(interactions) = obj.get_mut("interactions").and_then(|v| v.as_object_mut())
                {
                    for (_victim_id, victim_val) in interactions.iter_mut() {
                        if let Some(victim_arr) = victim_val.as_array() {
                            *victim_val = Value::Object(resolve_array(fields, victim_arr));
                        }
                    }
                }

                *player_val = Value::Object(obj);
            }
        }
    }

    results
}

/// Convert a positional array to a named object using a parallel names array.
/// `names[i]` provides the key for `values[i]`.
fn resolve_array(names: &[Value], values: &[Value]) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    for (i, name_val) in names.iter().enumerate() {
        if let Some(name) = name_val.as_str()
            && let Some(value) = values.get(i)
        {
            map.insert(name.to_string(), value.clone());
        }
    }
    map
}
