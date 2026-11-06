//! `gameparams_query` over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.
//! The first test to run builds the raw GameParams store under the system
//! temporary directory, which takes several seconds. Filters run in the
//! `wows-query-jq-worker` binary, named by `WOWS_QUERY_JQ_WORKER_BIN` under
//! Buck.

use std::path::PathBuf;
use std::sync::LazyLock;

use serde_json::Value;
use serde_json::json;
use wows_query::GameData;

static GAME: LazyLock<GameData> = LazyLock::new(|| {
    let worker = std::env::var_os("WOWS_QUERY_JQ_WORKER_BIN")
        .map(PathBuf::from)
        .or_else(|| option_env!("CARGO_BIN_EXE_wows-query-jq-worker").map(PathBuf::from))
        .expect("the jq worker binary is built with the tests");
    wows_query::jq_worker::set_worker_program(worker);

    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let cache = std::env::temp_dir().join("wows-query-test-cache");
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
    let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
    GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
});

fn query(arguments: Value) -> Value {
    GAME.call_tool("gameparams_query", arguments).expect("query succeeds")
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn des_moines_main_battery_dispersion_fields() {
    let output = query(json!({
        "select": { "ship": "Des Moines" },
        "filter": ".A1_Artillery | {sigmaCount, taperDist, minDistH, minDistV}",
    }));

    assert_eq!(output["results"][0]["values"][0]["sigmaCount"], 2.05);
    insta::assert_yaml_snapshot!(output);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn keys_list_the_fields_of_an_entry() {
    let by_index = query(json!({ "select": { "param": "PASC020" }, "filter": "keys" }));
    let by_id = query(json!({ "select": { "param": "4273911792" }, "filter": "keys" }));

    assert_eq!(by_index, by_id);
    insta::assert_yaml_snapshot!(by_index);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn sets_are_capped_and_output_is_cut_to_the_budget() {
    let names = query(json!({
        "select": { "set": { "type": "Ship", "nation": "usa", "species": "Cruiser", "tier": 10 } },
        "filter": ".name",
        "limit": 3,
    }));
    insta::assert_yaml_snapshot!("capped_set", names);

    let whole = query(json!({
        "select": { "set": { "type": "Ship", "nation": "USA", "species": "Cruiser", "tier": 10 } },
        "filter": ".A_Hull | {health, maxSpeed, rudderTime, visibilityFactor}",
        "max_bytes": 500,
    }));
    let results = whole["results"].to_string().len();
    let partial = whole["partial"]["text"].as_str().map_or(0, str::len);

    assert_eq!(whole["truncated"], true);
    assert!(results + partial <= 500, "{results} + {partial} bytes exceed the budget");
    insta::assert_yaml_snapshot!("budget", whole);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn filter_errors_reach_the_model() {
    let messages: Vec<String> = [
        json!({ "select": { "ship": "Des Moines" }, "filter": ".A1_Artillery | {sigmaCount" }),
        json!({ "select": { "ship": "Des Moines" }, "filter": "__range(0; 10; 1)" }),
        json!({ "select": { "param": "PASC999" }, "filter": "." }),
    ]
    .into_iter()
    .map(|arguments| GAME.call_tool("gameparams_query", arguments).expect_err("query fails").to_string())
    .collect();
    insta::assert_yaml_snapshot!("failures", messages);

    // jaq quotes the whole hull object in this error; the answer cuts it.
    let runtime = query(json!({ "select": { "ship": "Des Moines" }, "filter": ".A_Hull - 1" }));
    insta::assert_yaml_snapshot!("runtime_error", runtime);
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn runaway_filters_end_in_the_worker() {
    let recursion = query(json!({ "select": { "ship": "Des Moines" }, "filter": "def f: 1 + f; f" }));
    assert_eq!(recursion["stopped"], "crashed");

    // Iteration over data spends no loop steps; only the worker deadline ends it.
    let started = std::time::Instant::now();
    let product = query(json!({
        "select": { "ship": "Des Moines" },
        "filter": "first((tojson | explode)[] as $a | (tojson | explode)[] | select(false))",
    }));
    assert_eq!(product["stopped"], "time_limit");
    assert!(started.elapsed() < std::time::Duration::from_secs(15));

    insta::assert_yaml_snapshot!([recursion, product]);
}
