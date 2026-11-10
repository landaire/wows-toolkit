//! The application shell over the real TSX bundle: settings written as they
//! are edited, the first-run questions, the menu, dialogs, toasts and zoom.

use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;

use nsh_core::Points;
use nsh_core::Size;
use wows_toolkit_viewmodel::settings::keys;

struct TestApp {
    app: nsh_testing::Headless,
    _guard: MutexGuard<'static, ()>,
}

impl Deref for TestApp {
    type Target = nsh_testing::Headless;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

static TEST_APP_LOCK: Mutex<()> = Mutex::new(());

static TEST_STORAGE: LazyLock<std::path::PathBuf> = LazyLock::new(|| {
    let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-shell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).expect("the test settings directory is created");
    wows_toolkit_config::storage_override::set(path.clone());
    path
});

fn storage() -> &'static std::path::PathBuf {
    &TEST_STORAGE
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts")
}

/// Writes `rows` (key, JSON) to the shared settings before the app reads them.
fn seed(rows: &[(&str, &str)]) {
    storage();
    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        for (key, json) in rows {
            wows_toolkit_config::queries::set_setting_raw(&pool, key, json).await.expect("a test setting is written");
        }
    });
}

/// The stored JSON of `key`, `None` when it was never written.
fn stored(key: &str) -> Option<serde_json::Value> {
    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(&pool, key).await.expect("the setting reads")
    })
}

/// Answered first-run questions, so the dialog stays down for tests about something else.
fn answered() {
    seed(&[
        (keys::BUILD_CONSENT_SHOWN, "true"),
        (keys::REPLAY_CONSENT_SHOWN, "true"),
        (keys::LANGUAGE_SELECTION_SHOWN, "true"),
        (keys::CHECK_FOR_UPDATES, "false"),
    ]);
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> nsh_testing::Headless {
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    nsh_testing::tsx(&entry, registries()).expect("the TSX app starts")
}

/// Starts the app over the seeded settings. The lock is taken before seeding
/// by the caller through [`lock`].
fn app(guard: MutexGuard<'static, ()>) -> TestApp {
    let mut app = start();
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

fn lock() -> MutexGuard<'static, ()> {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    storage();
    guard
}

fn wait_for_setting(key: &str, expected: serde_json::Value) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if stored(key) == Some(expected.clone()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("{key} was {:?}, expected {expected}", stored(key));
}

#[test]
fn settings_are_written_as_they_are_edited() {
    let guard = lock();
    answered();
    let mut app = app(guard);

    app.press_key("mod+,").unwrap();
    app.wait_for("Application Settings").unwrap();
    app.wait_for("Session Settings").unwrap();
    app.wait_for("Replay Settings").unwrap();
    app.wait_for("Twitch Settings").unwrap();
    app.wait_for("Armor Viewer Defaults").unwrap();

    app.clear("label:Display Name").unwrap();
    app.type_text("label:Display Name", "NSH Tester").unwrap();
    app.submit("label:Display Name").unwrap();
    wait_for_setting(keys::COLLAB_DISPLAY_NAME, serde_json::json!("NSH Tester"));

    app.press("label:Dracula").unwrap();
    wait_for_setting(keys::THEME, serde_json::json!("Dracula"));

    app.press("label:Replays").unwrap();
    wait_for_setting(keys::DATA_SHARING_MODE, serde_json::json!("replays"));
    wait_for_setting(keys::SEND_REPLAY_DATA, serde_json::json!(true));

    app.press("label:Show Heals").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while stored(keys::REPLAY_SETTINGS).and_then(|replay| replay["show_heals"].as_bool()) != Some(false) {
        assert!(std::time::Instant::now() < deadline, "the replay row's heals column is written");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_game_directory_without_an_install_is_refused_and_flagged() {
    let guard = lock();
    answered();
    let mut app = app(guard);

    app.press_key("mod+,").unwrap();
    app.wait_for("World of Warships Settings").unwrap();
    app.clear("label:World of Warships Directory").unwrap();
    app.type_text("label:World of Warships Directory", "/no/such/install").unwrap();
    app.submit("label:World of Warships Directory").unwrap();
    app.wait_for("text:/^World of Warships directory is not valid/").unwrap();
    assert_ne!(
        stored(keys::WOWS_DIR),
        Some(serde_json::json!("/no/such/install")),
        "a directory that is not an install is not saved"
    );
    app.wait_for("text:! Settings").unwrap();
}

#[test]
fn the_palette_asks_before_refreshing_persisted_data() {
    let guard = lock();
    answered();
    let mut app = app(guard);

    app.press_key("mod+k").unwrap();
    app.press("Refresh Persisted Replay Data...").unwrap();
    app.wait_for("text:/^Re-parse every replay/").unwrap();
    app.press("Cancel").unwrap();
    app.wait_for_gone("text:/^Re-parse every replay/", Duration::from_secs(5)).unwrap();
}

#[test]
fn the_palette_opens_a_replay_directory_in_the_inspector() {
    let guard = lock();
    answered();
    let directory = storage().join("palette-directory");
    std::fs::create_dir_all(&directory).expect("the replay directory is created");
    let root = wows_toolkit_replay_data::directories::canonical_root(directory.clone());
    let mut app = app(guard);

    app.press_key("mod+,").unwrap();
    app.wait_for("Application Settings").unwrap();
    app.on_pick_paths(vec![directory]).unwrap();
    app.press_key("mod+k").unwrap();
    app.press("Open Replay Directory...").unwrap();
    app.wait_for_gone("Application Settings", Duration::from_secs(5)).unwrap();
    let shown = root.display().to_string();
    app.wait_until("the directory opens in the inspector", Duration::from_secs(5), |screen| screen.shows(&shown))
        .unwrap();
}

#[test]
fn sending_all_replays_from_the_palette_shows_the_inspector() {
    let guard = lock();
    answered();
    let mut app = app(guard);

    app.press_key("mod+,").unwrap();
    app.wait_for("Application Settings").unwrap();
    app.press_key("mod+k").unwrap();
    app.press("Send All Replays to ShipBuilds").unwrap();
    app.wait_for_gone("Application Settings", Duration::from_secs(5)).unwrap();
}

#[test]
fn a_fresh_run_asks_what_to_share_first() {
    let guard = lock();
    seed(&[
        (keys::BUILD_CONSENT_SHOWN, "false"),
        (keys::REPLAY_CONSENT_SHOWN, "false"),
        (keys::LANGUAGE_SELECTION_SHOWN, "true"),
        (keys::CHECK_FOR_UPDATES, "false"),
    ]);
    let mut app = app(guard);

    app.wait_for("Build Collection Consent").unwrap();
    app.press("Send Build Data Only").unwrap();
    wait_for_setting(keys::DATA_SHARING_MODE, serde_json::json!("build_data"));
    wait_for_setting(keys::BUILD_CONSENT_SHOWN, serde_json::json!(true));
    wait_for_setting(keys::REPLAY_CONSENT_SHOWN, serde_json::json!(true));
    app.wait_for_gone("Build Collection Consent", Duration::from_secs(5)).unwrap();
}

#[test]
fn a_reader_who_consented_before_replays_is_offered_them() {
    let guard = lock();
    seed(&[
        (keys::BUILD_CONSENT_SHOWN, "true"),
        (keys::REPLAY_CONSENT_SHOWN, "false"),
        (keys::DATA_SHARING_MODE, "\"build_data\""),
        (keys::SEND_REPLAY_DATA, "true"),
        (keys::CHECK_FOR_UPDATES, "false"),
    ]);
    let mut app = app(guard);

    app.wait_for("Switch to Replay Sharing").unwrap();
    app.press("Keep Current Setting").unwrap();
    wait_for_setting(keys::REPLAY_CONSENT_SHOWN, serde_json::json!(true));
    assert_eq!(
        stored(keys::DATA_SHARING_MODE),
        Some(serde_json::json!("build_data")),
        "keeping the setting changes nothing"
    );
}

#[test]
fn the_menu_opens_about_and_links() {
    let guard = lock();
    answered();
    let mut app = app(guard);

    app.press("label:File").unwrap();
    app.press("About").unwrap();
    app.wait_for("Made by landaire.").unwrap();
    app.wait_for("text:https://github.com/landaire/wows-toolkit").unwrap();
    app.press("Cancel").unwrap();
    app.wait_for_gone("Made by landaire.", Duration::from_secs(5)).unwrap();

    app.press("label:File").unwrap();
    app.press("Create Issue").unwrap();
    app.wait_until("the issue page opens", Duration::from_secs(5), |screen| {
        screen.opened_urls().iter().any(|url| url.contains("/issues/new"))
    })
    .unwrap();
}

#[test]
fn the_zoom_is_applied_through_the_window_and_saved() {
    let guard = lock();
    answered();
    seed(&[(keys::ZOOM_FACTOR, "1.5")]);
    let zooms = Arc::new(Mutex::new(Vec::<f64>::new()));
    let mut app = app(guard);
    let heard = Arc::clone(&zooms);
    app.host().handle("shell.zoom", move |call| {
        if let Some(zoom) = call.data().get("zoom").and_then(serde_json::Value::as_f64) {
            heard.lock().unwrap().push(zoom);
        }
        call.resolve(serde_json::Value::Null);
    });

    app.press_key("mod+,").unwrap();
    app.wait_for("1.50").unwrap();
    app.press("Reset").unwrap();
    app.wait_for("1.15").unwrap();
    wait_for_setting(keys::ZOOM_FACTOR, serde_json::json!(1.15));
    assert!(
        zooms.lock().unwrap().iter().any(|zoom| (zoom - 1.15).abs() < 1e-6),
        "the window was asked to draw at the default zoom"
    );
}

#[test]
fn dropping_something_that_is_not_a_replay_says_so() {
    let guard = lock();
    answered();
    let mut app = app(guard);
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::Linux;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .unwrap();
    let file = url::Url::from_file_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();

    app.drop_files("Replay Inspector", vec![file.to_string()]).unwrap();
    app.wait_for("That is not a replay file").unwrap();
}

#[test]
fn debug_mode_toggles_for_the_session() {
    let guard = lock();
    answered();
    let mut app = app(guard);

    assert!(!app.shows("Debug build"));
    app.press_key("mod+shift+d").unwrap();
    app.wait_for("Debug build").unwrap();
    app.press_key("mod+shift+d").unwrap();
    app.wait_for_gone("Debug build", Duration::from_secs(5)).unwrap();
}

#[test]
fn each_running_job_has_a_spinner_beside_it() {
    let guard = lock();
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/shell/status_strip.tsx");
    let mut app = TestApp { app: nsh_testing::tsx(&entry, registries()).expect("the status strip starts"), _guard: guard };

    app.wait_for("Start").unwrap();
    app.press("Start").unwrap();
    app.wait_for("text:Sampling 1 of 3").unwrap();
    // The spinner beside the job is named for it.
    app.wait_for("id:status-job-sample >> label:Sampling").unwrap();

    app.press("Finish").unwrap();
    app.wait_for_gone("id:status-job-sample", Duration::from_secs(5)).unwrap();
}

#[test]
fn the_drop_scrim_says_what_the_hovered_files_will_do() {
    let guard = lock();
    answered();
    let mut app = app(guard);
    let replay = |name: &str| format!("file:///replays/{name}");

    app.send_host_event("shell.hoveredFiles", serde_json::json!({ "uris": [replay("a.wowsreplay")] })).unwrap();
    app.wait_for("Drop to load a.wowsreplay").unwrap();

    app.send_host_event("shell.hoveredFiles", serde_json::json!({ "uris": [replay("a.wowsreplay"), replay("b.wowsreplay")] }))
        .unwrap();
    app.wait_for("Only one replay at a time, please").unwrap();

    app.send_host_event("shell.hoveredFiles", serde_json::json!({ "uris": [replay("notes.txt")] })).unwrap();
    app.wait_for("That is not a replay file").unwrap();

    app.send_host_event("shell.hoveredFiles", serde_json::Value::Null).unwrap();
    app.wait_for_gone("That is not a replay file", Duration::from_secs(5)).unwrap();

    // A drop ends the drag, and a file list the shell read late for it does
    // not bring the scrim back; the next drag's does.
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::Linux;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .unwrap();
    app.send_host_event("shell.hoveredFiles", serde_json::json!({ "uris": [replay("a.wowsreplay")] })).unwrap();
    app.wait_for("Drop to load a.wowsreplay").unwrap();
    let file = url::Url::from_file_path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    app.drop_files("Replay Inspector", vec![file.to_string()]).unwrap();
    app.wait_for_gone("Drop to load a.wowsreplay", Duration::from_secs(5)).unwrap();

    app.send_host_event("shell.hoveredFiles", serde_json::json!({ "uris": [replay("a.wowsreplay")] })).unwrap();
    assert!(
        app.wait_for_within("Drop to load a.wowsreplay", Duration::from_millis(500)).is_err(),
        "the late list for the dropped drag is not shown"
    );
    app.send_host_event("shell.hoveredFiles", serde_json::Value::Null).unwrap();
    app.send_host_event("shell.hoveredFiles", serde_json::json!({ "uris": [replay("b.wowsreplay")] })).unwrap();
    app.wait_for("Drop to load b.wowsreplay").unwrap();
}

#[test]
fn the_last_runs_crash_is_reported_once() {
    let guard = lock();
    answered();
    std::fs::write(wows_toolkit_viewmodel::logging::panic_log_path(), "panicked at src/main.rs:1:1\nboom")
        .expect("a crash report is left behind");
    let mut app = app(guard);

    app.wait_for("Application Crash Detected").unwrap();
    app.wait_for("text:/boom/").unwrap();
    app.press("Copy").unwrap();
    app.wait_for("Copied to clipboard").unwrap();
    assert!(!wows_toolkit_viewmodel::logging::panic_log_path().exists(), "the report is taken as it is read");
}

#[test]
fn the_palette_sets_the_theme_and_copies_the_bug_report() {
    let guard = lock();
    answered();
    seed(&[(keys::THEME, "\"System\"")]);
    let mut app = app(guard);

    app.press_key("mod+k").unwrap();
    app.press("Theme: Light").unwrap();
    wait_for_setting(keys::THEME, serde_json::json!("Light"));

    app.press_key("mod+k").unwrap();
    app.press("Copy Bug Report Info").unwrap();
    app.wait_for("Bug report information copied.").unwrap();
    let copied = app.screen().clipboard().expect("the report is on the clipboard").to_owned();
    assert!(copied.contains(env!("CARGO_PKG_VERSION")), "the report names this build: {copied}");
}

#[test]
fn importing_constants_without_a_loaded_build_is_refused() {
    let guard = lock();
    answered();
    let mut app = app(guard);

    app.press_key("mod+k").unwrap();
    app.wait_for("Import constants.json File...").unwrap();
    app.press("Import constants.json File...").unwrap();
    app.wait_for("Load a game build first").unwrap();
    assert!(app.screen().file_requests().is_empty(), "no mapping is asked for without a build to install it for");
}

#[test]
fn a_cleared_twitch_credential_written_by_the_other_front_ends_loads() {
    let guard = lock();
    answered();
    seed(&[("twitch_token", "null")]);
    let mut app = app(guard);

    app.press_key("mod+,").unwrap();
    app.wait_for("Twitch Settings").unwrap();
    assert!(!app.shows("Failed to load settings"));
}
