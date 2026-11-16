//! Filters cannot reach the host's environment or working directory. The
//! test runs itself again as the host, with a sentinel environment variable
//! and sentinel files in its working directory, and checks that no filter
//! that reads the environment, inputs or modules finds them.

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;
use wows_query::jq_worker::Limits;

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

fn worker() -> PathBuf {
    std::env::var_os("WOWS_QUERY_JQ_WORKER_BIN")
        .map(PathBuf::from)
        .or_else(|| option_env!("CARGO_BIN_EXE_wows-query-jq-worker").map(PathBuf::from))
        .expect("the jq worker binary is built with the tests")
}

fn probe(filter: &'static str) -> Probe {
    let limits = Limits { steps_per_input: 10_000, time: std::time::Duration::from_secs(5), output_bytes: 1 << 16 };
    let inputs = [b"{\"a\": 1}".to_vec(), b"{\"b\": 2}".to_vec()];
    let outcome = match wows_query::jq_worker::evaluate(filter, inputs.into_iter(), limits) {
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
/// starts. Prints the probes as one JSON line.
#[test]
fn host() {
    if std::env::var_os(SENTINEL_ENV).is_none() {
        return;
    }

    wows_query::jq_worker::set_worker_program(worker());
    let probes: Vec<Probe> = PROBES.iter().map(|filter| probe(filter)).collect();
    println!("PROBES {}", serde_json::to_string(&probes).expect("probes serialize"));
}

#[test]
fn unbounded_recursion_ends_only_the_worker() {
    wows_query::jq_worker::set_worker_program(worker());
    let limits = Limits { steps_per_input: 10_000, time: std::time::Duration::from_secs(5), output_bytes: 1 << 16 };
    let evaluation = wows_query::jq_worker::evaluate("def f: 1 + f; f", [b"null".to_vec()].into_iter(), limits)
        .expect("the worker starts");

    assert_eq!(evaluation.stopped, Some(wows_query::jq_worker::Stop::Crashed));
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

    // libtest prints the test name on the same line.
    let line = stdout
        .lines()
        .find_map(|line| line.split_once("PROBES ").map(|(_, probes)| probes))
        .unwrap_or_else(|| panic!("the host printed no probes: {stdout}"));
    assert!(!line.contains(SENTINEL), "a filter reached the sentinel: {line}");
    let probes: serde_json::Value = serde_json::from_str(line).expect("probes parse");
    insta::assert_yaml_snapshot!(probes);
}
