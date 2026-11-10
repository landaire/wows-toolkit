//! The Replay Inspector tab over the real TSX bundle, every replay read as
//! the fabricated sample report: opening files and directories, the panel's
//! header, actions, chat and debug views, and the listing's options.

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;

use nsh_core::Points;
use nsh_core::Size;
use wows_toolkit_viewmodel::settings::keys;

const FIRST: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
const SECOND: &str = "20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay";
/// The sample report's tab title: its recording player's ship and its map.
const SAMPLE_TITLE: &str = "Shimakaze - Fault Line";
const GONE: Duration = Duration::from_secs(5);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-inspector-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test directory is created");
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

static TEST_STORAGE: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-inspector-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).expect("the test settings directory is created");
    path
});

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts")
}

fn configure_test_storage() {
    let path: &PathBuf = &TEST_STORAGE;
    wows_toolkit_config::storage_override::set(path.clone());
    // The first-run questions are the shell's tests' to answer; here they
    // would stand in front of every tab.
    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        for key in [keys::BUILD_CONSENT_SHOWN, keys::REPLAY_CONSENT_SHOWN, keys::LANGUAGE_SELECTION_SHOWN] {
            wows_toolkit_config::queries::set_setting_raw(&pool, key, "true").await.expect("a question is answered");
        }
        wows_toolkit_config::queries::set_setting_raw(&pool, keys::CHECK_FOR_UPDATES, "false")
            .await
            .expect("the startup update check is off");
    });
}

/// The stored JSON of `key`, `None` when it was never written.
fn stored(key: &str) -> Option<serde_json::Value> {
    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(&pool, key).await.expect("the setting reads")
    })
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> nsh_testing::Headless {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    nsh_testing::tsx(&entry, registries()).expect("the TSX app starts")
}

/// The app, every replay it opens read as the sample report.
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    wows_toolkit_nsh::replay_inspector::test_support::read_replays_as_sample();
    let mut app = start();
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

/// Makes the surface a Linux desktop's, which right-clicks and file drops need.
fn on_linux_desktop(app: &mut TestApp) {
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::Linux;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .expect("the desktop environment is applied");
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(name)
}

/// Opens `path` through the listing's Open menu.
fn open_file(app: &mut TestApp, path: PathBuf) {
    app.on_pick_paths(vec![path]).unwrap();
    app.press("label:Open").unwrap();
    app.press("Open Replay File...").unwrap();
}

fn open_sample(app: &mut TestApp) {
    open_file(app, fixture(FIRST));
    app.wait_for(SAMPLE_TITLE).unwrap();
}

#[test]
fn opening_a_replay_file_shows_its_table_header_and_chat() {
    let mut app = app();
    app.wait_for("Select a replay").unwrap();

    open_file(&mut app, fixture(FIRST));
    assert_eq!(app.screen().file_requests().len(), 1, "the Open Replay File item shows one file dialog");
    app.wait_for(SAMPLE_TITLE).unwrap();
    app.wait_for("text:/Team Damage/").unwrap();
    app.wait_for("Ally_Alpha").unwrap();
    app.wait_for("Enemy_Delta").unwrap();
    app.wait_for_gone("Select a replay", GONE).unwrap();

    app.press("label:Chat").unwrap();
    app.wait_for("Game Chat").unwrap();
    app.wait_for("focus the Yamato").unwrap();
    app.wait_for("cap is contested").unwrap();

    app.press("Copy all").unwrap();
    app.wait_until("the transcript on the clipboard", GONE, |screen| {
        screen.clipboard().is_some_and(|text| text.contains("focus the Yamato") && text.contains("gg well played"))
    })
    .unwrap();

    app.press("label:Copy message >> nth=1").unwrap();
    app.wait_for("Chat copied").unwrap();
    let copied = app.screen().clipboard().expect("a message was copied").to_owned();
    assert!(copied.contains("Ally_Bravo") && copied.contains("cap is contested"), "copied {copied:?}");

    app.press("label:Chat").unwrap();
    app.wait_for_gone("Game Chat", GONE).unwrap();
    app.press("label:Chat").unwrap();
    app.wait_for("Game Chat").unwrap();
    app.press("label:Close Panel").unwrap();
    app.wait_for_gone("Game Chat", GONE).unwrap();
}

#[test]
fn a_plain_open_replaces_the_shown_replay_and_a_closed_tab_goes() {
    let mut app = app();
    open_sample(&mut app);
    assert_eq!(app.screen().count(SAMPLE_TITLE), Ok(1));

    open_file(&mut app, fixture(SECOND));
    app.wait_for(SAMPLE_TITLE).unwrap();
    assert_eq!(app.screen().count(SAMPLE_TITLE), Ok(1), "a plain open replaces the replay on screen");

    app.press(&format!("label:Close {SAMPLE_TITLE}")).unwrap();
    app.wait_for("Select a replay").unwrap();
}

#[test]
fn rendering_a_replay_docks_one_renderer_beside_it() {
    const STEM: &str = "20260223_115252_PZSC718-Narai_s06_Atoll";
    let mut app = app();
    open_sample(&mut app);

    app.press("label:Actions").unwrap();
    app.press("Render Replay").unwrap();
    app.wait_for(STEM).unwrap();
    app.wait_for(&format!("label:Close {SAMPLE_TITLE}")).unwrap();

    // Asking again brings the open renderer forward rather than docking a second one.
    app.press(SAMPLE_TITLE).unwrap();
    app.press("label:Actions").unwrap();
    app.press("Render Replay").unwrap();
    assert_eq!(app.screen().count(&format!("label:Close {STEM}")), Ok(1));

    app.press(&format!("label:Close {STEM}")).unwrap();
    app.wait_for_gone(&format!("label:Close {STEM}"), GONE).unwrap();
    app.wait_for(&format!("label:Close {SAMPLE_TITLE}")).unwrap();
}

#[test]
fn debug_mode_offers_the_raw_metadata() {
    let mut app = app();
    open_sample(&mut app);

    app.press("label:Actions").unwrap();
    assert!(!app.shows("label:Raw Metadata"), "the debug items wait for debug mode");
    app.press("label:Actions").unwrap();
    app.wait_for_gone("Results: JSON", GONE).unwrap();

    app.press_key("mod+shift+d").unwrap();
    app.press("label:Actions").unwrap();
    app.press("label:Raw Metadata").unwrap();
    app.wait_for("text:/clientVersionFromXml/").unwrap();

    app.press("label:Actions").unwrap();
    app.press("label:Raw Results").unwrap();
    app.wait_for("text:/arenaUniqueID/").unwrap();

    app.press_key("mod+shift+d").unwrap();
    app.wait_for_gone("text:/arenaUniqueID/", GONE).unwrap();
}

#[test]
fn a_row_expands_and_offers_its_actions() {
    let mut app = app();
    open_sample(&mut app);

    app.press("label:Expand Ally_Alpha").unwrap();
    app.wait_for("label:Collapse Ally_Alpha").unwrap();
    app.press("label:Collapse Ally_Alpha").unwrap();
    app.wait_for("label:Expand Ally_Alpha").unwrap();

    app.press("label:Player actions >> nth=0").unwrap();
    app.wait_for("Copy Row").unwrap();
    app.wait_for("Copy Table").unwrap();
    app.press("Copy Row").unwrap();
    app.wait_until("the row on the clipboard", GONE, |screen| screen.clipboard().is_some_and(|text| !text.is_empty()))
        .unwrap();
}

#[test]
fn exporting_results_as_json_writes_the_file() {
    let mut app = app();
    open_sample(&mut app);

    let directory = TestDirectory::new("export");
    let target = directory.0.join("sample.json");
    app.on_pick_save_path(Some(target.clone())).unwrap();
    app.press("label:Actions").unwrap();
    app.press("Results: JSON").unwrap();
    app.wait_for("Results exported").unwrap();

    let written = std::fs::read_dir(&directory.0)
        .expect("the export directory reads")
        .map(|entry| entry.expect("an entry reads").path())
        .next()
        .expect("the export is written");
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&written).expect("the export reads")).expect("the export is JSON");
    assert!(exported.is_object(), "the export is one match object");
}

#[test]
fn a_directory_lists_its_replays_and_opens_them() {
    let directory = TestDirectory::new("listing");
    std::fs::copy(fixture(FIRST), directory.0.join(FIRST)).expect("the first fixture is copied");
    std::fs::copy(fixture(SECOND), directory.0.join(SECOND)).expect("the second fixture is copied");

    let mut app = app();
    on_linux_desktop(&mut app);
    app.on_pick_paths(vec![directory.0.clone()]).unwrap();
    app.press("label:Open").unwrap();
    app.press("Open Replay Directory...").unwrap();
    let requests = app.screen().file_requests();
    assert!(
        matches!(requests.last(), Some(nsh_core::file_dialog::FileRequest::OpenDialog(dialog)) if dialog.picks_folders()),
        "the directory item asks for a folder"
    );

    app.wait_for(&format!("label:{FIRST}")).unwrap();
    app.wait_for(&format!("label:{SECOND}")).unwrap();
    app.wait_for("label:/^Close directory /").unwrap();

    app.press(&format!("label:{FIRST}")).unwrap();
    app.press(&format!("label:{FIRST}")).unwrap();
    app.wait_for(SAMPLE_TITLE).unwrap();
    app.press(&format!("label:Close {SAMPLE_TITLE}")).unwrap();
    app.wait_for_gone(SAMPLE_TITLE, GONE).unwrap();

    app.right_click("label:/^Close directory /").unwrap();
    app.press("Pin directory").unwrap();
    app.wait_for("[Pinned]").unwrap();
    app.wait_for_gone("label:/^Close directory /", GONE).unwrap();
    app.right_click("[Pinned]").unwrap();
    app.press("Unpin directory").unwrap();
    app.wait_for("label:/^Close directory /").unwrap();

    app.press(&format!("label:{SECOND}")).unwrap();
    app.press_key("enter").unwrap();
    app.wait_for(SAMPLE_TITLE).unwrap();

    app.press("label:/^Close directory /").unwrap();
    app.wait_for_gone(&format!("label:{FIRST}"), GONE).unwrap();
}

#[test]
fn the_listing_options_change_grouping_columns_and_autoload() {
    let mut app = app();
    open_sample(&mut app);

    app.press("label:Listing Options").unwrap();
    app.press("label:Replay grouping: Ship").unwrap();
    app.wait_until("Ship grouping selected", GONE, |screen| screen.selected("label:Replay grouping: Ship") == Ok(true))
        .unwrap();
    assert_eq!(app.screen().selected("label:Replay grouping: Date"), Ok(false));

    let raw_xp_shown = app.shows("label:Raw XP");
    app.press("label:Column Filters: Raw XP").unwrap();
    app.wait_until("the Raw XP column toggled", GONE, |screen| screen.matches("label:Raw XP") == Ok(!raw_xp_shown))
        .unwrap();
    app.press("label:Column Filters: Raw XP").unwrap();
    app.wait_until("the Raw XP column back", GONE, |screen| screen.matches("label:Raw XP") == Ok(raw_xp_shown))
        .unwrap();

    let autoload = stored(keys::AUTO_LOAD_LATEST_REPLAY).and_then(|value| value.as_bool()).unwrap_or(true);
    app.press("label:Autoload Latest Replay").unwrap();
    app.wait_until("autoload saved", GONE, |_| {
        stored(keys::AUTO_LOAD_LATEST_REPLAY).and_then(|value| value.as_bool()) == Some(!autoload)
    })
    .unwrap();

    app.press("label:Replay grouping: Date").unwrap();
    app.wait_until("Date grouping selected", GONE, |screen| screen.selected("label:Replay grouping: Date") == Ok(true))
        .unwrap();
}

#[test]
fn the_listing_collapses_to_a_rail_and_comes_back() {
    let mut app = app();
    app.press("label:Collapse Replay Listing").unwrap();
    app.wait_for("label:Expand Replay Listing").unwrap();
    app.wait_for_gone("label:Listing Options", GONE).unwrap();
    app.press("label:Expand Replay Listing").unwrap();
    app.wait_for("label:Listing Options").unwrap();
    app.wait_for_gone("label:Expand Replay Listing", GONE).unwrap();
}

#[test]
fn a_dropped_replay_opens_in_the_inspector() {
    let mut app = app();
    on_linux_desktop(&mut app);
    let uri = url::Url::from_file_path(fixture(FIRST)).expect("the replay path converts to a file URL").to_string();
    app.drop_files("Replay Inspector", vec![uri]).unwrap();
    app.wait_for(SAMPLE_TITLE).unwrap();
}

#[test]
fn a_file_that_is_not_a_replay_fails_to_load() {
    let mut app = app();
    open_file(&mut app, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    app.wait_for("text:/is not a replay/").unwrap();
    app.wait_for("Failed to load replay").unwrap();
}

#[test]
fn the_palette_opens_a_replay_file() {
    let mut app = app();
    app.on_pick_paths(vec![fixture(FIRST)]).unwrap();
    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "Manually Open Replay File").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for(SAMPLE_TITLE).unwrap();
}

#[test]
fn a_rating_table_that_loads_later_rates_the_open_replay() {
    let mut app = app();
    wows_toolkit_nsh::replay_inspector::test_support::read_replays_as_unrated_sample();
    // The startup download may already have installed the published table.
    wows_toolkit_nsh::game_data::set_personal_rating(None);
    open_sample(&mut app);
    app.wait_for("text:/Team Damage/").unwrap();
    assert!(!app.shows("text:/PR: /"), "the replay was read without a rating table");

    wows_toolkit_nsh::game_data::set_personal_rating(Some(
        wows_toolkit_nsh::replay_inspector::test_support::sample_personal_rating(),
    ));
    app.wait_for("text:/PR: /").unwrap();
    app.wait_for(SAMPLE_TITLE).unwrap();
}
