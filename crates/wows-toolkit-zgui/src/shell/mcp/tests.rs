//! The MCP server as the shell runs it: started from its setting and the status bar, reported in
//! messages, and served to a real HTTP client.

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

use jiff::Timestamp;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use rust_i18n::t;
use serde_json::Value;
use serde_json::json;
use wows_core::game_types::ArenaId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_viewmodel::mcp::McpBindAddress;
use wows_toolkit_viewmodel::mcp::McpPort;
use wows_toolkit_viewmodel::mcp::McpSettings;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::shell;
use crate::services::mcp::McpStatus;
use crate::services::mcp::StartFailure;
use crate::shell::settings::Setting;

/// The tests share one settings database and bind real ports, so they run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

const STATUS: &str = "app-status-mcp";

/// The shell with the server never offered.
fn open() -> HeadlessWindow {
    let mut window = open_shell(HeadlessOptions::new(REVIEW_SIZE));
    shell().services().mcp.stop();
    shell().save_mcp(McpSettings::new());
    window.advance(Duration::from_millis(100));

    window
}

/// Runs frames until `done` holds, letting the server's thread report in real time.
fn wait_for(window: &mut HeadlessWindow, what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(25));
    }
    window.advance(Duration::from_millis(100));
}

fn status() -> McpStatus {
    shell().services().mcp.status_untracked()
}

fn running_url(window: &mut HeadlessWindow) -> String {
    wait_for(window, "the server to start", || matches!(status(), McpStatus::Running { .. }));
    let McpStatus::Running { endpoint } = status() else { unreachable!() };

    endpoint.url()
}

fn saved() -> McpSettings {
    shell().services().settings.read_untracked(|settings| settings.mcp).expect("the settings are read")
}

fn toasts(window: &HeadlessWindow) -> String {
    window.texts_by_class("lui-toasts").concat()
}

fn notes(window: &HeadlessWindow) -> String {
    window.texts_by_class("wt-settings__note").concat()
}

#[test]
fn enabling_starts_the_server_and_the_status_bar_stops_it() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut window = open();
    assert!(window.by_test_id(STATUS).is_none(), "a server never offered has no indicator");

    shell().set_mcp_enabled(true);
    let url = running_url(&mut window);

    let port = saved().port.expect("a port was chosen on the first enable");
    assert_eq!(url, format!("http://127.0.0.1:{port}/mcp"));
    assert!(toasts(&window).contains(&url), "the message names the URL: {}", toasts(&window));
    let indicator = window.by_test_id(STATUS).expect("the status bar shows the server");

    window.click(indicator).expect("the indicator can be pressed");
    wait_for(&mut window, "the server to stop", || status() == McpStatus::Stopped);
    assert!(saved().enabled, "stopping from the status bar leaves the server offered");
    wait_for(&mut window, "the port to be released", || TcpListener::bind(("127.0.0.1", port.get())).is_ok());

    let indicator = window.by_test_id(STATUS).expect("the indicator stays");
    window.click(indicator).expect("the indicator can be pressed");
    assert_eq!(running_url(&mut window), url, "the server starts again on the same port");

    shell().set_mcp_enabled(false);
    wait_for(&mut window, "the server to stop", || status() == McpStatus::Stopped);
    assert!(window.by_test_id(STATUS).is_none(), "a withdrawn server has no indicator");
}

#[test]
fn a_busy_port_is_reported_and_kept() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut window = open();
    let holder = TcpListener::bind("127.0.0.1:0").expect("a port can be held");
    let held = holder.local_addr().unwrap().port();
    let port = McpPort::parse(&held.to_string()).unwrap();
    shell().save_mcp(McpSettings { port: Some(port), ..McpSettings::new() });

    shell().set_mcp_enabled(true);
    wait_for(&mut window, "the start to fail", || matches!(status(), McpStatus::Failed { .. }));

    assert!(matches!(status(), McpStatus::Failed { failure: StartFailure::PortInUse { .. } }), "{:?}", status());
    let said = t!("ui.mcp.port_in_use", port = held, address = "127.0.0.1").into_owned();
    assert!(toasts(&window).contains(&said), "the message says the port is taken: {}", toasts(&window));
    assert_eq!(saved().port, Some(port), "no other port is chosen in its place");

    shell().settings_drawer.open_at(Setting::McpPort);
    window.advance(Duration::from_millis(600));
    assert!(notes(&window).contains(&t!("ui.settings.mcp.port_in_use").into_owned()), "the port row says so too");

    shell().set_mcp_enabled(false);
    drop(holder);
}

#[test]
fn the_port_persists_and_the_server_starts_with_the_program() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut window = open();
    shell().set_mcp_enabled(true);
    let url = running_url(&mut window);
    let port = saved().port;
    drop(window);

    let mut window = open_shell(HeadlessOptions::new(REVIEW_SIZE));
    assert_eq!(saved().port, port, "the next start reads the same port");
    assert_eq!(running_url(&mut window), url, "and the server starts with the program on it");

    shell().set_mcp_enabled(false);
}

#[test]
fn typed_addresses_and_ports_are_checked_inline() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut window = open();
    shell().settings_drawer.open_at(Setting::McpBindAddress);
    window.advance(Duration::from_millis(600));

    shell().commit_mcp_address("localhost:80".to_owned());
    shell().commit_mcp_port("70000".to_owned());
    window.advance(Duration::from_millis(200));

    let said = notes(&window);
    assert!(said.contains(&t!("ui.settings.mcp.address_invalid", input = "localhost:80").into_owned()), "{said}");
    assert!(said.contains(&t!("ui.settings.mcp.port_invalid", input = "70000").into_owned()), "{said}");
    assert_eq!(saved(), McpSettings::new(), "refused text changes nothing");

    shell().commit_mcp_address("::1".to_owned());
    shell().commit_mcp_port("40123".to_owned());
    window.advance(Duration::from_millis(200));
    let said = notes(&window);
    assert!(!said.contains(&t!("ui.settings.mcp.address_invalid", input = "localhost:80").into_owned()), "{said}");
    assert!(!said.contains(&t!("ui.settings.mcp.port_invalid", input = "70000").into_owned()), "{said}");
    assert_eq!(saved().port.map(McpPort::get), Some(40123));
}

#[test]
fn only_loopback_addresses_are_served() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut window = open();
    shell().settings_drawer.open_at(Setting::McpBindAddress);
    window.advance(Duration::from_millis(600));

    // A row saved with another address is reported, kept, and not served on.
    let exposed: McpBindAddress = serde_json::from_str(r#""0.0.0.0""#).unwrap();
    let port = McpPort::parse("40124").unwrap();
    shell().save_mcp(McpSettings { enabled: true, bind_address: exposed, port: Some(port), ..McpSettings::new() });
    shell().start_mcp();
    window.advance(Duration::from_millis(200));
    assert!(matches!(status(), McpStatus::Stopped), "no server listens on it: {:?}", status());
    let saved_note = t!("ui.settings.mcp.address_not_loopback", input = "0.0.0.0").into_owned();
    assert!(notes(&window).contains(&saved_note), "{}", notes(&window));

    shell().commit_mcp_address("192.168.1.20".to_owned());
    window.advance(Duration::from_millis(200));
    let typed_note = t!("ui.settings.mcp.address_not_loopback", input = "192.168.1.20").into_owned();
    assert!(notes(&window).contains(&typed_note), "{}", notes(&window));
    assert_eq!(saved().bind_address, exposed, "typed text is not adopted, and the saved row is not rewritten");

    shell().save_mcp(McpSettings::new());
}

/// A fixture replay indexed under `arena`, read from the repository's fixtures.
fn index_fixture(window: &mut HeadlessWindow, arena: i64) -> PathBuf {
    let replay = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/replays/20190420_125057_PASB017-Montana-1945_15_NE_north.wowsreplay")
        .canonicalize()
        .expect("the fixture replay is in the repository");
    let pool = shell().services().settings.pool().expect("the settings database is open");
    let runtime = shell().services().runtime().expect("the shell has a runtime");
    let root = replay.parent().unwrap().to_path_buf();
    let path = replay.clone();

    runtime.block_on(async move {
        let now = Timestamp::from_second(1_700_000_000).unwrap();
        let source = query::ensure_default_source(&pool, &root, now).await.expect("a source is recorded");
        let objective = ObjectiveMatch {
            arena_id: ArenaId::new(arena),
            timestamp: now,
            map: "North".into(),
            game_mode: "Domination".into(),
            game_mode_id: None,
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: None,
        };
        query::upsert_match(&pool, &objective).await.expect("a match is recorded");
        let record = ReplayRecord {
            arena_id: ArenaId::new(arena),
            source_id: source,
            replay_path: path,
            file_mtime: None,
            outcome: MatchOutcome::Win,
            self_account_id: None,
            self_ship_id: None,
            self_survived: Some(true),
            self_damage: Some(98_765),
            self_kills: Some(3),
            self_pr: None,
            results_available: false,
            indexed_at: now,
        };
        query::upsert_record(&pool, &record).await.expect("a record is recorded");
    });
    window.advance(Duration::from_millis(100));

    replay
}

/// A JSON-RPC session over the server's HTTP endpoint, as an MCP client holds one.
struct Client {
    http: reqwest::blocking::Client,
    url: String,
    session: Option<String>,
}

impl Client {
    fn open(url: String) -> Self {
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("an HTTP client is built");
        let mut client = Self { http, url, session: None };

        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "zgui-test", "version": "0"}}
        });
        let (session, _) = client.post(&initialize);
        client.session = session;
        client.post(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

        client
    }

    /// The session id the server named, and the JSON-RPC response when there is one.
    fn post(&self, body: &Value) -> (Option<String>, Option<Value>) {
        let mut request = self
            .http
            .post(&self.url)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-06-18")
            .json(body);
        if let Some(session) = &self.session {
            request = request.header("Mcp-Session-Id", session);
        }
        let response = request.send().expect("the server answers");
        assert!(response.status().is_success(), "{}: {:?}", response.status(), body);

        let session = response.headers().get("mcp-session-id").and_then(|id| id.to_str().ok()).map(str::to_owned);
        let text = response.text().expect("the body is read");
        let message = text
            .lines()
            .map(|line| line.strip_prefix("data:").unwrap_or(line).trim())
            .filter(|line| !line.is_empty())
            .find_map(|line| serde_json::from_str::<Value>(line).ok().filter(|value| value.get("id").is_some()));

        (session, message)
    }

    fn call(&self, id: u64, method: &str, params: Value) -> Value {
        let (_, message) = self.post(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let message = message.expect("a request is answered");
        assert!(message.get("error").is_none(), "{method} failed: {message}");

        message["result"].clone()
    }
}

#[test]
fn an_http_client_lists_and_calls_the_replay_tools() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut window = open();
    const ARENA: i64 = 4_242_424_242;
    let replay = index_fixture(&mut window, ARENA);
    shell().set_mcp_enabled(true);
    let url = running_url(&mut window);

    let client = Client::open(url);
    let listed = client.call(2, "tools/list", json!({}));
    let names: Vec<&str> =
        listed["tools"].as_array().expect("a tool list").iter().filter_map(|tool| tool["name"].as_str()).collect();
    for wanted in [
        "replay_index_search",
        "replay_index_players",
        "replay_header",
        "replay_battle_results",
        "replay_analysis",
        "game_builds",
        "game_files_list",
        "game_file_read",
    ] {
        assert!(names.contains(&wanted), "{wanted} is listed in {names:?}");
    }

    let found =
        client.call(3, "tools/call", json!({"name": "replay_index_search", "arguments": {"query": "map:North"}}));
    let matches = found["structuredContent"]["matches"].as_array().expect("a page of matches").clone();
    let hit = matches
        .iter()
        .find(|hit| hit["arena_id"] == json!(ARENA))
        .unwrap_or_else(|| panic!("the fixture is found: {found}"));
    assert_eq!(hit["damage"], json!(98_765));
    assert_eq!(PathBuf::from(hit["replay_path"].as_str().unwrap()), replay);

    let header =
        client.call(4, "tools/call", json!({"name": "replay_header", "arguments": {"replay": {"arena_id": ARENA}}}));
    assert_eq!(header["structuredContent"]["player_vehicle"], json!("PASB017-Montana-1945"), "{header}");

    // The test has no install holding the replay's build, so the results are refused with the
    // reason, as a tool failure the model reads rather than a protocol error.
    let results = client.call(
        5,
        "tools/call",
        json!({"name": "replay_battle_results", "arguments": {"replay": {"arena_id": ARENA}}}),
    );
    assert_eq!(results["isError"], json!(true), "{results}");
    assert!(results.to_string().contains("1427460"), "the reason names the replay's build: {results}");

    shell().set_mcp_enabled(false);
}

#[test]
fn the_game_data_tools_read_the_apps_own_provider() {
    let _serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let mut window = open();
    let provider = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider builds");
    let vfs = crate::game_art::testing::upright_class_icon(wowsunpack::game_params::types::Species::Battleship);
    let version = wowsunpack::data::Version::from_client_exe("0,13,0,7654321");
    let loaded = Arc::new(LoadedGameData::for_test_with_version(provider, vfs, version));
    shell().services().game_data.set_ready_for_test(Arc::clone(&loaded));
    assert!(shell().services().mcp.shared_build().is_none(), "nothing is indexed while no server runs");

    shell().set_mcp_enabled(true);
    running_url(&mut window);
    wait_for(&mut window, "the build to be shared", || shell().services().mcp.shared_build().is_some());

    let shared = shell().services().mcp.shared_build().unwrap();
    assert!(Arc::ptr_eq(shared.shared_provider(), loaded.provider()), "no second GameMetadataProvider is made");

    shell().set_mcp_enabled(false);
    wait_for(&mut window, "the index to be dropped", || shell().services().mcp.shared_build().is_none());
}
