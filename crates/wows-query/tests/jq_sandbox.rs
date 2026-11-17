//! Filters cannot reach the host's environment or working directory. The
//! test runs itself again as the host, with a sentinel environment variable
//! and sentinel files in its working directory, and checks that no filter
//! that reads the environment, inputs or modules finds them.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use wows_query::error::JqError;
use wows_query::jq_worker::Evaluation;
use wows_query::jq_worker::Limits;
use wows_query::jq_worker::Stop;
use wows_query::raw_params::EntryPosition;
use wows_query::raw_params::Localization;
use wows_query::raw_params::RawParams;
use wowsunpack::Rc;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_types::GameParamId;
use wowsunpack::rpc::entitydefs::EntitySpec;

const SENTINEL_ENV: &str = "WOWS_QUERY_CANARY";
const SENTINEL: &str = "canary-3f9b1c";

/// Filters that read data outside their input in jq, after one that shows
/// the worker evaluates.
const PROBES: &[&str] = &[
    "keys",
    "env",
    "env.WOWS_QUERY_CANARY",
    "$ENV",
    "$ENV.WOWS_QUERY_CANARY",
    "input",
    "[inputs]",
    "input_filename",
    "$__prog_args",
    "include \"x\"; .",
    "import \"x\" as y; .",
    "import \"x\" as $y; $y",
    "include \"canary\"; canary",
];

#[derive(Serialize)]
struct Probe {
    filter: &'static str,
    outcome: Result<Vec<String>, String>,
}

/// A catalog without translations.
struct NoNames;

impl ResourceLoader for NoNames {
    fn localized_name_from_param(&self, _param: &Param) -> Option<String> {
        None
    }

    fn localized_name_from_id(&self, _id: &TranslationKey) -> Option<String> {
        None
    }

    fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
        None
    }

    fn entity_specs(&self) -> &[EntitySpec] {
        &[]
    }
}

fn worker() -> PathBuf {
    std::env::var_os("WOWS_QUERY_JQ_WORKER_BIN")
        .map(PathBuf::from)
        .or_else(|| option_env!("CARGO_BIN_EXE_wows-query-jq-worker").map(PathBuf::from))
        .expect("the jq worker binary is built with the tests")
}

/// A store of `values`, built under the temporary directory, and the
/// positions of its entries.
fn store(name: &str, values: Vec<serde_json::Value>) -> (Arc<RawParams>, Vec<EntryPosition>) {
    let kind_dir = std::env::temp_dir().join(format!("wows-query-jq-sandbox-{name}-{}", std::process::id()));
    let entries = values.into_iter().enumerate().map(|(position, value)| (format!("entry{position}"), value));
    let localization = Localization { language: "en", names: &NoNames };
    let store = RawParams::build(&kind_dir, 1, localization, entries).expect("the store builds");
    let positions = store.entries().map(|entry| entry.position()).collect();

    (store, positions)
}

fn limits() -> Limits {
    Limits {
        steps_per_input: 10_000,
        time: std::time::Duration::from_secs(5),
        output_bytes: 1 << 16,
        memory_bytes: 1 << 30,
    }
}

/// The outcome of `filter` on the first of the inputs `evaluate` runs it over.
fn probe(filter: &'static str, evaluate: impl Fn(&str) -> Result<Evaluation, JqError>) -> Probe {
    let outcome = match evaluate(filter) {
        Ok(evaluation) => {
            assert_eq!(evaluation.stopped, None, "{filter} stopped early");
            let first = evaluation.inputs.into_iter().next().expect("the first input was evaluated");
            match first.error {
                Some(error) => Err(error),
                None => Ok(first.values),
            }
        }
        Err(error) => Err(error.to_string()),
    };

    Probe { filter, outcome }
}

/// The host side, run in the child process `filters_cannot_reach_the_host`
/// starts. Prints the probes over stored and JSON inputs, one JSON line each.
#[test]
fn host() {
    if std::env::var_os(SENTINEL_ENV).is_none() {
        return;
    }

    wows_query::jq_worker::set_worker_program(worker());
    let (store, positions) = store("host", vec![json!({"a": 1}), json!({"b": 2})]);
    let stored: Vec<Probe> = PROBES
        .iter()
        .map(|filter| probe(filter, |filter| wows_query::jq_worker::evaluate(filter, &store, &positions, limits())))
        .collect();
    println!("PROBES-STORE {}", serde_json::to_string(&stored).expect("probes serialize"));

    let json: Vec<Probe> = PROBES
        .iter()
        .map(|filter| {
            probe(filter, |filter| {
                let inputs = [b"{\"a\": 1}".to_vec(), b"{\"b\": 2}".to_vec()];
                wows_query::jq_worker::evaluate_json(filter, inputs.into_iter(), limits())
            })
        })
        .collect();
    println!("PROBES-JSON {}", serde_json::to_string(&json).expect("probes serialize"));
}

#[test]
fn json_inputs_are_evaluated_in_order() {
    wows_query::jq_worker::set_worker_program(worker());
    let inputs = [b"{\"a\": 1}".to_vec(), b"{\"b\": 2}".to_vec(), b"{\"a\":".to_vec()];
    let evaluation =
        wows_query::jq_worker::evaluate_json(".a // .b", inputs.into_iter(), limits()).expect("the worker starts");

    assert_eq!(evaluation.stopped, None);
    let [first, second, third] = evaluation.inputs.try_into().expect("every input was evaluated");
    assert_eq!((first.values, first.error), (vec!["1".to_owned()], None));
    assert_eq!((second.values, second.error), (vec!["2".to_owned()], None));
    assert!(third.error.is_some_and(|error| error.starts_with("record is not JSON")));
}

/// More input than the pipe holds, so the parent is still writing when the
/// worker stops reading.
#[test]
fn json_output_stops_at_the_output_limit() {
    wows_query::jq_worker::set_worker_program(worker());
    let inputs = std::iter::repeat_n(b"\"abcdefgh\"".to_vec(), 100_000);
    let evaluation = wows_query::jq_worker::evaluate_json(".", inputs, Limits { output_bytes: 16, ..limits() })
        .expect("the worker starts");

    assert_eq!(evaluation.stopped, Some(Stop::OutputLimit));
    let values: Vec<Vec<String>> = evaluation.inputs.into_iter().map(|input| input.values).collect();
    assert_eq!(values, [vec!["\"abcdefgh\"".to_owned()], vec!["\"abcde".to_owned()]]);
}

#[test]
fn a_json_filter_stops_at_its_time_limit() {
    wows_query::jq_worker::set_worker_program(worker());
    let inputs = [b"null".to_vec(), b"null".to_vec()];
    let limits = Limits { steps_per_input: u64::MAX, time: Duration::from_millis(200), ..limits() };
    let evaluation = wows_query::jq_worker::evaluate_json("last(range(1e12))", inputs.into_iter(), limits)
        .expect("the worker starts");

    assert_eq!(evaluation.stopped, Some(Stop::TimeLimit));
    assert_eq!(evaluation.inputs.len(), 1, "the second input is not evaluated");
}

#[test]
fn unbounded_recursion_ends_only_the_worker() {
    wows_query::jq_worker::set_worker_program(worker());
    let (store, positions) = store("recursion", vec![json!(null)]);
    // The stack has to overflow before the time limit ends the worker, which on a loaded machine
    // takes longer than `limits()` allows.
    let limits = Limits { time: Duration::from_secs(60), ..limits() };
    let stored =
        wows_query::jq_worker::evaluate("def f: 1 + f; f", &store, &positions, limits).expect("the worker starts");
    let json = wows_query::jq_worker::evaluate_json("def f: 1 + f; f", [b"null".to_vec()].into_iter(), limits)
        .expect("the worker starts");

    assert_eq!(stored.stopped, Some(Stop::Crashed));
    assert_eq!(json.stopped, Some(Stop::Crashed));
}

/// The parent ends a worker over its memory limit, on systems where nothing
/// in the worker caps it. The string fits the worker's address space.
#[test]
fn a_worker_over_its_memory_limit_is_ended() {
    wows_query::jq_worker::set_worker_program(worker());
    let (store, positions) = store("memory", vec![json!(null)]);
    let filter = r#""x" * 1000000000 | length"#;

    let unlimited =
        wows_query::jq_worker::evaluate(filter, &store, &positions, Limits { memory_bytes: 4 << 30, ..limits() })
            .expect("the worker starts");
    assert_eq!(unlimited.stopped, None, "the string is built under a larger limit");

    let limited =
        wows_query::jq_worker::evaluate(filter, &store, &positions, Limits { memory_bytes: 256 << 20, ..limits() })
            .expect("the worker starts");
    assert_eq!(limited.stopped, Some(Stop::Crashed));

    let limited_json = wows_query::jq_worker::evaluate_json(
        filter,
        [b"null".to_vec()].into_iter(),
        Limits { memory_bytes: 256 << 20, ..limits() },
    )
    .expect("the worker starts");
    assert_eq!(limited_json.stopped, Some(Stop::Crashed));
}

#[test]
fn filters_cannot_reach_the_host() {
    let cwd = std::env::temp_dir().join(format!("wows-query-canary-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).expect("create the host's working directory");
    for name in ["x.jq", "x.json", "canary.jq"] {
        let text = match name {
            "canary.jq" => format!("def canary: \"{SENTINEL}\";"),
            _ => format!("\"{SENTINEL}\""),
        };
        std::fs::write(cwd.join(name), text).expect("write a sentinel file");
    }

    let output = Command::new(std::env::current_exe().expect("this test's executable"))
        .args(["--exact", "host", "--nocapture", "--test-threads=1"])
        .env(SENTINEL_ENV, SENTINEL)
        // Buck names the worker relative to the directory this test runs in.
        .env("WOWS_QUERY_JQ_WORKER_BIN", std::path::absolute(worker()).expect("the worker path is not empty"))
        .current_dir(&cwd)
        .output()
        .expect("run the host");
    let _ = std::fs::remove_dir_all(&cwd);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "host failed: {stdout}{}", String::from_utf8_lossy(&output.stderr));

    // libtest prints the test name on the same line as the first.
    let printed = |prefix: &str| {
        let line = stdout
            .lines()
            .find_map(|line| line.split_once(prefix).map(|(_, probes)| probes))
            .unwrap_or_else(|| panic!("the host printed no {prefix}: {stdout}"));
        assert!(!line.contains(SENTINEL), "a filter reached the sentinel: {line}");
        serde_json::from_str::<serde_json::Value>(line).expect("probes parse")
    };
    let probes = printed("PROBES-STORE ");
    assert_eq!(printed("PROBES-JSON "), probes, "JSON inputs reach what stored inputs reach");
    insta::assert_yaml_snapshot!(probes);
}
