//! Headless tests of the windows other than the main one: the main window
//! asks its host for them, and the bundle draws the tactics board or a replay
//! renderer alone when it is launched as one.

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use nsh_core::Points;
use nsh_core::Size;

/// What the app asked its host for: each window request, and each release.
/// While `hold` is set, window requests stay unanswered in `held`.
#[derive(Default)]
struct HostRecord {
    windows: Mutex<Vec<serde_json::Value>>,
    released: Mutex<u32>,
    hold: AtomicBool,
    held: Mutex<Vec<nsh_engine::HostCall>>,
}

/// The bundle running as one window, with what it asked its host for.
struct TestWindow {
    app: nsh_testing::Headless,
    record: Arc<HostRecord>,
    _guard: MutexGuard<'static, ()>,
}

impl Deref for TestWindow {
    type Target = nsh_testing::Headless;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl DerefMut for TestWindow {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

impl TestWindow {
    fn wait_until(&mut self, what: &str, done: impl Fn(&HostRecord) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !done(&self.record) {
            assert!(std::time::Instant::now() < deadline, "{what} did not happen");
            self.app.settle();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Serializes the tests: they share this process's settings database.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// This process's settings directory, its database created and migrated once,
/// with the first-run questions answered so none stands in front of a window.
static TEST_STORAGE: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-windows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).expect("the test settings directory is created");
    wows_toolkit_config::storage_override::set(path.clone());

    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database is created");
            for key in ["build_consent_window_shown", "replay_consent_prompt_shown", "language_selection_shown"] {
                wows_toolkit_config::queries::set_setting_raw(&pool, key, "true")
                    .await
                    .expect("a question is answered");
            }
            wows_toolkit_config::queries::set_setting_raw(&pool, "check_for_updates", "false")
                .await
                .expect("the startup update check is off");
        },
    );

    path
});

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

/// Starts the bundle as the desktop runner does, with `window` as the launch
/// parameter naming what it shows (`None`: the main window).
fn start(window: Option<serde_json::Value>) -> TestWindow {
    let guard = TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    wows_toolkit_config::storage_override::set(TEST_STORAGE.clone());

    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/main.tsx");
    let bundle = nsh_testing::compile_tsx(&entry, &registries()).expect("the TSX app builds");

    let host = nsh_engine::HostChannel::new();
    if let Some(window) = window {
        let mut params = serde_json::Map::new();
        params.insert("window".to_owned(), window);
        host.set_params(params);
    }
    let record = Arc::new(HostRecord::default());
    let asked = Arc::clone(&record);
    host.handle("shell.openWindow", move |call| {
        asked.windows.lock().unwrap().push(call.data().clone());
        if asked.hold.load(Ordering::SeqCst) {
            asked.held.lock().unwrap().push(call);
        } else {
            call.resolve(serde_json::Value::Null);
        }
    });
    let released = Arc::clone(&record);
    host.handle("window.released", move |_call| *released.released.lock().unwrap() += 1);

    let mut app = nsh_testing::Headless::start(nsh_engine::EngineConfig {
        ui_sets: registries(),
        host,
        ..nsh_engine::EngineConfig::new(bundle)
    })
    .expect("the engine starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");

    TestWindow { app, record, _guard: guard }
}

/// The palette's Tactics Board asks the host for the board's window and
/// draws no board over the main window.
#[test]
fn the_main_window_asks_for_the_board_in_a_window_of_its_own() {
    let mut window = start(None);

    window.wait_for("Replay Inspector").unwrap();
    window.press_key("mod+k").unwrap();
    window.wait_for("Commands").unwrap();
    window.type_text("label:Command palette", "Tactics Board").unwrap();
    window.press("text:Tactics Board").unwrap();

    window.wait_until("the window request", |record| !record.windows.lock().unwrap().is_empty());
    assert_eq!(*window.record.windows.lock().unwrap(), [serde_json::json!({ "kind": "tacticsBoard" })]);
    assert!(!window.screen().matches("label:Ranges").unwrap(), "the board is not drawn in the main window");
}

/// Launched as the board's window, the bundle draws the board alone, which
/// takes up and puts down tools, and lets it go when the window closes.
#[test]
fn the_board_window_draws_the_board_and_lets_it_go_on_close() {
    let mut window = start(Some(serde_json::json!({ "kind": "tacticsBoard" })));

    // Whether a build loads depends on the machine; the toolbar shows either way.
    window.wait_for("label:Ranges").unwrap();
    assert!(!window.screen().matches("Replay Inspector").unwrap(), "the main window's tabs are not drawn");

    window.press("label:Line").unwrap();
    window.wait_for("label:Undo").unwrap();
    window.press_key("escape").unwrap();
    window.press_key("mod+2").unwrap();
    window.press("label:Thicker stroke").unwrap();
    window.wait_for("text:3").unwrap();
    window.press_key("[").unwrap();
    window.wait_for("text:2").unwrap();

    window.press("label:Keyboard Shortcuts").unwrap();
    window.wait_for("Multi-select").unwrap();
    window.close_menu().ok();

    window.send_host_event("window.closing", serde_json::Value::Null).unwrap();
    window.wait_for_gone("label:Ranges", Duration::from_secs(5)).unwrap();
    window.wait_until("the release", |record| *record.released.lock().unwrap() == 1);
}

/// A docked renderer popped out asks for a window on its live viewport,
/// which outlives the docked view; the window takes it up as it was and
/// closes it when the window closes.
#[test]
fn a_popped_out_renderer_moves_its_live_viewport_into_its_window() {
    const SAMPLE_TITLE: &str = "Shimakaze - Fault Line";
    const FAILED: &str = "This replay could not be played back";

    let dir = tempfile::tempdir().expect("a temporary directory");
    let broken = dir.path().join("broken.wowsreplay");
    std::fs::write(&broken, b"not a replay").expect("the file is written");
    wows_toolkit_nsh::replay_inspector::test_support::read_replays_as_sample();

    let request = {
        let mut main = start(None);
        main.on_pick_paths(vec![broken.clone()]).unwrap();
        main.press("label:Open").unwrap();
        main.press("Open Replay File...").unwrap();
        main.wait_for(SAMPLE_TITLE).unwrap();
        main.press("label:Actions").unwrap();
        main.press("Render Replay").unwrap();
        main.wait_for(FAILED).unwrap();

        // A second press while the window is being opened asks for no second window.
        main.record.hold.store(true, Ordering::SeqCst);
        main.press("label:Open in its own window").unwrap();
        main.wait_until("the window request", |record| !record.held.lock().unwrap().is_empty());
        main.press("label:Open in its own window").unwrap();
        main.settle();
        assert_eq!(main.record.windows.lock().unwrap().len(), 1, "one viewport goes to one window");
        for call in main.record.held.lock().unwrap().drain(..) {
            call.resolve(serde_json::Value::Null);
        }
        main.wait_for_gone(FAILED, Duration::from_secs(5)).unwrap();
        main.record.windows.lock().unwrap()[0].clone()
    };
    assert_eq!(request["kind"], "replayRenderer");
    assert_eq!(request["path"], broken.to_str().unwrap());
    let renderer = || serde_json::from_value(request["renderer"].clone()).expect("the request names a viewport");
    assert!(wows_toolkit_nsh::ReplayRenderer::view(renderer()).is_ok(), "the docked view leaves the viewport open");

    let mut window = start(Some(request.clone()));
    window.wait_for(FAILED).unwrap();
    assert!(!window.screen().matches("Replay Inspector").unwrap(), "the main window's tabs are not drawn");

    window.send_host_event("window.closing", serde_json::Value::Null).unwrap();
    window.wait_for_gone(FAILED, Duration::from_secs(5)).unwrap();
    window.wait_until("the release", |record| *record.released.lock().unwrap() == 1);
    assert!(wows_toolkit_nsh::ReplayRenderer::view(renderer()).is_err(), "the window closes the viewport");
}
