use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard, OnceLock};

use nsh_core::{Points, Size};

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test replay directory is created");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
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

static TEST_STORAGE: OnceLock<std::path::PathBuf> = OnceLock::new();

fn configure_test_storage() {
    let path = TEST_STORAGE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test settings directory is created");
        path
    });
    wows_toolkit_config::storage_override::set(path.clone());
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

#[cfg(feature = "buck")]
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    let path = std::env::var("NSH_BUNDLE_PATH").expect("Buck provides the generated TSX bundle");
    let source = std::fs::read_to_string(path).expect("the generated TSX bundle is readable");
    let config = nsh_engine::EngineConfig {
        ui_sets: registries(),
        ..nsh_engine::EngineConfig::new(nsh_engine::Bundle::source(source))
    };
    let mut app = nsh_testing::Headless::start(config).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

#[cfg(not(feature = "buck"))]
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

#[test]
fn replay_inspector_opens_and_displays_metadata() {
    const FILE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";

    let mut app = app();
    let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FILE);
    app.on_pick_paths(vec![replay]).unwrap();

    app.press("Open Replay File...").unwrap();
    assert_eq!(app.screen().file_requests().len(), 1, "the replay button must show one file dialog");
    app.wait_for(FILE).unwrap();
    app.wait_for("Players (15)").unwrap();
    app.wait_for("Battle").unwrap();
    app.wait_for("PVEBattle").unwrap();

    app.press("Settings").unwrap();
    app.press("Replay Inspector").unwrap();
    app.wait_for(FILE).unwrap();
    app.wait_for("Players (15)").unwrap();
    app.press("Raw metadata").unwrap();
    app.wait_for("text:/clientVersionFromXml/").unwrap();
    app.press("Overview").unwrap();
    app.wait_for("Battle").unwrap();

    let export_directory = TestDirectory::new("replay-export");
    let collision_selection = export_directory.0.join("narai");
    let collision_path = collision_selection.with_extension("json");
    std::fs::write(&collision_path, b"existing export").unwrap();
    app.on_pick_save_path(Some(collision_selection)).unwrap();
    app.press("Export Metadata...").unwrap();
    app.wait_for("text:/^could not write replay metadata to /").unwrap();
    assert_eq!(std::fs::read(&collision_path).unwrap(), b"existing export");

    let selected_path = export_directory.0.join("narai-export");
    let export_path = selected_path.with_extension("json");
    app.on_pick_save_path(Some(selected_path)).unwrap();
    app.press("Export Metadata...").unwrap();
    app.wait_for("text:/^Replay metadata exported to /").unwrap();
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(export_path).expect("the metadata export is written"))
            .expect("the metadata export is valid JSON");
    assert_eq!(exported["fileName"], FILE);
    assert_eq!(exported["mode"], "PVEBattle");
    assert_eq!(exported["players"].as_array().map(Vec::len), Some(15));
    assert!(exported.get("sourcePath").is_none());
    assert!(exported["players"][0]["shipId"].as_u64().is_some());

    let invalid = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    app.on_pick_paths(vec![invalid]).unwrap();
    app.press("Open Replay File...").unwrap();
    app.wait_for("text:/^could not parse replay /").unwrap();
}

#[test]
fn replay_directory_lists_readable_files_and_reports_failures() {
    const FIRST: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
    const SECOND: &str = "20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay";

    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays");
    let directory = TestDirectory::new("replay-browser");
    std::fs::copy(fixtures.join(FIRST), directory.0.join(FIRST)).expect("the first replay fixture is copied");
    std::fs::copy(fixtures.join(SECOND), directory.0.join(SECOND)).expect("the second replay fixture is copied");
    std::fs::write(directory.0.join("invalid.wowsreplay"), b"not a replay").expect("the invalid replay is written");

    let mut app = app();
    app.on_pick_paths(vec![directory.0.clone()]).unwrap();
    app.press("Open Replay Directory...").unwrap();

    let requests = app.screen().file_requests();
    assert_eq!(requests.len(), 1, "the directory button must show one file dialog");
    assert!(
        matches!(
            &requests[0],
            nsh_core::file_dialog::FileRequest::OpenDialog(dialog) if dialog.picks_folders()
        ),
        "the replay browser must request a directory"
    );
    app.wait_for("2 replay file(s)").unwrap();
    app.wait_for(FIRST).unwrap();
    app.wait_for("PVEBattle").unwrap();
    app.wait_for("1 replay file(s) could not be read.").unwrap();

    app.press(SECOND).unwrap();
    app.wait_for("PFSD210-Marceau").unwrap();
    assert_eq!(app.screen().count(SECOND), Ok(2), "the selected replay must also appear in the inspector heading");

    app.press_key("up").unwrap();
    assert_eq!(app.screen().count(FIRST), Ok(2), "keyboard selection must update the inspector");

    app.press("label:Sort by Replay").unwrap();
    assert_eq!(app.screen().count(FIRST), Ok(2), "sorting must preserve replay identity");

    app.press("label:Browse replay rows").unwrap();
    app.press_key("up").unwrap();
    assert_eq!(app.screen().count(SECOND), Ok(2), "keyboard selection must follow the sorted row order");

    app.set_viewport(Size::new(Points(500.0), Points(700.0))).expect("the compact replay viewport is applied");
    app.wait_for("Players (24)").unwrap();

    app.on_pick_paths(vec![fixtures.join(FIRST)]).unwrap();
    app.press("Open Replay File...").unwrap();
    app.wait_for_gone(
        &format!("wows-toolkit-nsh-replay-browser-{}", std::process::id()),
        std::time::Duration::from_secs(5),
    )
    .unwrap();

    let all_invalid = TestDirectory::new("replay-browser-all-invalid");
    std::fs::write(all_invalid.0.join("invalid.wowsreplay"), b"not a replay").expect("the invalid replay is written");
    app.on_pick_paths(vec![all_invalid.0.clone()]).unwrap();
    app.press("Open Replay Directory...").unwrap();
    app.wait_for("No readable replay files. 1 replay file(s) could not be read.").unwrap();
}

#[test]
fn settings_use_the_shared_database_and_nsh_default_theme() {
    let mut app = app();

    app.press_key("mod+,").unwrap();
    app.wait_for("NSH default").unwrap();
    app.wait_for("Save Settings").unwrap();
    app.clear("label:Collaboration display name").unwrap();
    app.type_text("label:Collaboration display name", "NSH Tester").unwrap();
    app.press("Save Settings").unwrap();
    app.wait_for("Settings saved.").unwrap();

    app.clear("label:Collaboration display name").unwrap();
    assert_eq!(app.screen().input_text("label:Collaboration display name").unwrap(), Some(""));
    app.press("Reload").unwrap();
    app.wait_until("the saved collaboration name returns", std::time::Duration::from_secs(5), |screen| {
        screen.input_text("label:Collaboration display name").ok().flatten() == Some("NSH Tester")
    })
    .unwrap();
}

#[test]
fn top_level_navigation_and_application_menu_work() {
    let mut app = app();

    app.wait_for("Replay Inspector").unwrap();
    app.press_key("mod+k").unwrap();
    app.wait_for("Command Palette").unwrap();
    app.press("Go to Settings").unwrap();
    app.wait_for("NSH default").unwrap();
    app.wait_for("Shared with the egui and GPUI applications").unwrap();

    app.press("label:Open application menu").unwrap();
    app.wait_for("Tactics Board").unwrap();
    app.press("About WoWs Toolkit").unwrap();
    app.wait_for("Tools for World of Warships replays, game data, statistics, rendering, and collaboration.").unwrap();
    app.press("Close").unwrap();
    app.wait_for_gone(
        "Tools for World of Warships replays, game data, statistics, rendering, and collaboration.",
        std::time::Duration::from_secs(5),
    )
    .unwrap();

    app.press("label:Open application menu").unwrap();
    app.press("Tactics Board").unwrap();
    app.wait_for("Select a map to start a tactics board.").unwrap();
    app.press("Close").unwrap();
    app.wait_for_gone("Select a map to start a tactics board.", std::time::Duration::from_secs(5)).unwrap();
}

#[test]
fn command_palette_supports_keyboard_navigation_and_compact_viewports() {
    let mut app = app();
    app.set_viewport(Size::new(Points(500.0), Points(360.0))).expect("the compact viewport is applied");

    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "About").unwrap();
    app.press_key("down").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("Tools for World of Warships replays, game data, statistics, rendering, and collaboration.").unwrap();
    app.press("Close").unwrap();

    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "nomatch").unwrap();
    app.wait_for("No matching commands.").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for("Go to Replay Inspector").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("Command Palette", std::time::Duration::from_secs(5)).unwrap();
}
