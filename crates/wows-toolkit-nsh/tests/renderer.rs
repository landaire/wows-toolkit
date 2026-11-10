//! Headless tests of the replay renderer's controls, docked in the replay
//! inspector of the real TSX app.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;

use nsh_core::{Points, Size};

const FIXTURE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
const SAMPLE_TITLE: &str = "Shimakaze - Fault Line";

static TEST_APP_LOCK: Mutex<()> = Mutex::new(());

struct TestApp {
    app: nsh_testing::Headless,
    _guard: MutexGuard<'static, ()>,
}

impl std::ops::Deref for TestApp {
    type Target = nsh_testing::Headless;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl std::ops::DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> nsh_testing::Headless {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    nsh_testing::tsx(&entry, registries()).expect("the TSX app starts")
}

/// The app, every replay the inspector opens read as the sample report, so a
/// renderer can be asked for without a game install.
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let storage = std::env::temp_dir().join(format!("wows-toolkit-nsh-renderer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&storage);
    std::fs::create_dir(&storage).expect("the test settings directory is created");
    wows_toolkit_config::storage_override::set(storage);
    wows_toolkit_nsh::replay_inspector::test_support::read_replays_as_sample();
    let mut app = start();
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

/// Opens `replay` in the inspector and docks a renderer for it.
fn render(app: &mut TestApp, replay: PathBuf) {
    app.on_pick_paths(vec![replay]).unwrap();
    app.press("label:Open").unwrap();
    app.press("Open Replay File...").unwrap();
    app.wait_for(SAMPLE_TITLE).unwrap();
    app.press("label:Actions").unwrap();
    app.press("Render Replay").unwrap();
}

#[test]
fn a_replay_that_cannot_be_read_says_so_and_refuses_the_transport() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let broken = dir.path().join("broken.wowsreplay");
    std::fs::write(&broken, b"not a replay").expect("the file is written");

    let mut app = app();
    render(&mut app, broken);

    app.wait_for("This replay could not be played back").unwrap();
    app.wait_for("text:/the replay could not be read/").unwrap();
    app.wait_for("Play").unwrap();
}

#[test]
fn the_transport_and_popovers_are_offered_for_a_replay() {
    let replay = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FIXTURE);

    let mut app = app();
    render(&mut app, replay);

    app.wait_for("Replay renderer").unwrap();
    for control in ["Seek", "Zoom", "Clock", "Jump to start", "Play", "Events"] {
        app.wait_for(control).unwrap();
    }
    app.press("Events").unwrap();
    app.wait_for("Event Timeline").unwrap();
    app.press("Open in its own window").unwrap();
    app.wait_for("Back to the tab").unwrap();
    app.press("Back to the tab").unwrap();
    app.wait_for("Open in its own window").unwrap();
}
