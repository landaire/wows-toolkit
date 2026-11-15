//! `gameparams_search` ranking over a real 15.10.0 dump. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`.
//! The first test to run builds the raw GameParams store under the system
//! temporary directory, which takes several seconds.

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

/// The top results of each query, one line each: param, name, how it
/// matched and score.
fn rankings(queries: &[(&str, Option<&str>)]) -> Vec<(String, Vec<String>)> {
    queries
        .iter()
        .map(|(query, kind)| {
            let mut arguments = json!({ "query": query, "limit": 5 });
            if let Some(kind) = kind {
                arguments["type"] = json!(kind);
            }
            let output = GAME.call_tool("gameparams_search", arguments).expect("search succeeds");
            let label = match kind {
                Some(kind) => format!("{query} (type {kind})"),
                None => (*query).to_owned(),
            };

            (label, output["results"].as_array().expect("results is an array").iter().map(line).collect())
        })
        .collect()
}

fn line(result: &Value) -> String {
    format!(
        "{} [{}] {} {}:{} {} {}",
        result["param"].as_str().unwrap_or_default(),
        result["type"].as_str().unwrap_or_default(),
        result["name"].as_str().unwrap_or("-"),
        result["field"].as_str().unwrap_or_default(),
        result["method"].as_str().unwrap_or_default(),
        result["score"],
        result["matched"],
    )
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn misspelled_ship_names() {
    insta::assert_yaml_snapshot!(rankings(&[
        ("des moins", None),
        ("yamto", None),
        ("shimakze", Some("Ship")),
        ("Gross Kurfurst", Some("Ship")),
        ("montanna", None),
    ]));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn abbreviations() {
    insta::assert_yaml_snapshot!(rankings(&[("DM", None), ("GK", None), ("DCP", None), ("dsmoins", None),]));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn partial_names() {
    insta::assert_yaml_snapshot!(rankings(&[
        ("moines", None),
        ("kurf", Some("Ship")),
        ("damage control", None),
        ("concealment", Some("Modernization")),
    ]));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn index_ids_and_param_names() {
    insta::assert_yaml_snapshot!(rankings(&[
        ("PASC020", None),
        ("pasc02", None),
        ("4273911792", None),
        ("PJSB018_Yamato_1944", None),
    ]));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn community_names() {
    insta::assert_yaml_snapshot!(rankings(&[
        ("Yamamoto", None),
        ("hydro", None),
        ("radar", Some("Ability")),
        ("super colombo", None),
    ]));
}

#[test]
#[cfg_attr(not(has_build_13533378), ignore)]
fn nothing_matches_with_a_hint() {
    let output = GAME.call_tool("gameparams_search", json!({ "query": "qqqqzzzz" })).expect("search succeeds");
    let too_long =
        GAME.call_tool("gameparams_search", json!({ "query": "a".repeat(101) })).expect_err("query is refused");

    assert_eq!(too_long.to_string(), "the search query is 101 characters long; give at most 100");
    insta::assert_yaml_snapshot!(output);
}
