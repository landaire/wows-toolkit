//! The replay worker over a real 15.4.0 replay. Run with
//! `WOWS_GAME_DATA=<wows-replay-data root> cargo test -p wows-query`. The
//! worker is the `wows-query-replay-worker` binary, named by
//! `WOWS_QUERY_REPLAY_WORKER_BIN` under Buck.

use std::path::PathBuf;

use replayshark::PacketFilter;
use wows_query::replay_worker::BuildSource;
use wows_query::replay_worker::ByteCount;
use wows_query::replay_worker::PacketLimits;
use wows_query::replay_worker::RecordCount;

const REPLAY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../wows-replay-insights/tests/fixtures/replays/",
    "1003_1780609677_20260604_172336_PASD008-Benson-1945_52_Britain.wowsreplay"
);

/// The replay's bytes, with the worker program registered.
fn replay() -> Vec<u8> {
    let worker = std::env::var_os("WOWS_QUERY_REPLAY_WORKER_BIN")
        .map(PathBuf::from)
        .or_else(|| option_env!("CARGO_BIN_EXE_wows-query-replay-worker").map(PathBuf::from))
        .expect("the replay worker binary is built with the tests");
    wows_query::replay_worker::set_worker_program(worker);

    std::fs::read(REPLAY).expect("the fixture replay is readable")
}

/// The 15.4.0 build of the dump root named by `WOWS_GAME_DATA`.
fn build() -> BuildSource {
    let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
    let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
    let entry = index.find_by_build(12506899).expect("dump root lists build 12506899");

    BuildSource {
        dump_dir: root.join(&entry.dir),
        cache_root: Some(std::env::temp_dir().join("wows-query-test-cache")),
    }
}

#[test]
fn header_names_the_build() {
    let header = wows_query::replay_worker::header(&replay()).expect("the header reads");

    insta::assert_yaml_snapshot!(header);
}

#[test]
#[cfg_attr(not(has_build_12506899), ignore)]
fn analysis_of_a_random_battle() {
    let analysis = wows_query::replay_worker::analyze(&replay(), &build()).expect("the replay analyzes");

    insta::assert_yaml_snapshot!(analysis);
}

#[test]
#[cfg_attr(not(has_build_12506899), ignore)]
fn chat_packets() {
    let filter = PacketFilter { kinds: vec!["Chat".to_owned()], ..PacketFilter::default() };
    let limits = PacketLimits { records: RecordCount::new(3), bytes: ByteCount::from_mib(1) };
    let selection =
        wows_query::replay_worker::packets(&replay(), &build(), &filter, limits).expect("the packets decode");

    insta::assert_yaml_snapshot!(selection);
}

#[test]
#[cfg_attr(not(has_build_12506899), ignore)]
fn byte_limit_withholds_records_but_counts_matches() {
    let limits = PacketLimits { records: RecordCount::new(10), bytes: ByteCount::new(1) };
    let selection = wows_query::replay_worker::packets(&replay(), &build(), &PacketFilter::default(), limits)
        .expect("the packets decode");

    assert!(selection.records.is_empty());
    assert_eq!(selection.scan.matched, selection.scan.packets);
}
