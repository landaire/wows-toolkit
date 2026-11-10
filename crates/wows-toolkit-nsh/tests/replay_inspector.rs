//! The Replay Inspector tab over the real TSX bundle, every replay read as
//! the fabricated sample report: opening files and directories, the panel's
//! header, actions, chat and debug views, and the listing's options.

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use nsh_core::Points;
use nsh_core::Size;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_nsh::game_data::GameDataState;
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
    /// Dropped before the lock, so the next test starts without it.
    _install: Option<InstallSettings>,
    _guard: MutexGuard<'static, ()>,
}

/// A WoWs directory and autoload choice saved in the settings and bound by
/// the game-data service, both put back when dropped: the other tests start
/// with no WoWs directory.
struct InstallSettings {
    /// The stored autoload JSON before, `None` when it was never written.
    autoload_before: Option<serde_json::Value>,
}

impl InstallSettings {
    fn bind(install: &Path, autoload: bool) -> Self {
        let autoload_before = stored(keys::AUTO_LOAD_LATEST_REPLAY);
        save_setting(keys::WOWS_DIR, &install.display().to_string());
        save_setting(keys::AUTO_LOAD_LATEST_REPLAY, &autoload);
        wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");
        Self { autoload_before }
    }
}

impl Drop for InstallSettings {
    fn drop(&mut self) {
        save_setting(keys::WOWS_DIR, &String::new());
        match &self.autoload_before {
            Some(value) => save_setting(keys::AUTO_LOAD_LATEST_REPLAY, value),
            None => forget_setting(keys::AUTO_LOAD_LATEST_REPLAY),
        }
        if let Err(error) = wows_toolkit_nsh::game_data::reload_settings() {
            eprintln!("the restored settings were not read: {error}");
        }
    }
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

fn save_setting<T: serde::Serialize>(key: &str, value: &T) {
    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        wows_toolkit_config::queries::set_setting(&pool, key, value).await.expect("the setting is saved");
    });
}

fn forget_setting(key: &str) {
    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        sqlx::query("DELETE FROM settings WHERE key = ?1")
            .bind(key)
            .execute(&pool)
            .await
            .expect("the setting is forgotten");
    });
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> nsh_testing::Headless {
    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    nsh_testing::tsx(&entry, registries()).expect("the TSX app starts")
}

/// Takes the copies the app makes in place of the system clipboard, so no
/// test writes the user's clipboard.
struct RecordingClipboard {
    copied: Mutex<Vec<Vec<PathBuf>>>,
    refuse: AtomicBool,
}

impl RecordingClipboard {
    fn last(&self) -> Option<Vec<PathBuf>> {
        self.copied.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).last().cloned()
    }
}

impl wows_toolkit_nsh::clipboard::FileClipboard for RecordingClipboard {
    fn copy_files(&self, files: &[PathBuf]) -> Result<(), arboard::Error> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(arboard::Error::ClipboardNotSupported);
        }
        self.copied.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(files.to_vec());
        Ok(())
    }
}

static CLIPBOARD: RecordingClipboard =
    RecordingClipboard { copied: Mutex::new(Vec::new()), refuse: AtomicBool::new(false) };

/// Sends the app's file copies to [`CLIPBOARD`], emptied and accepting.
fn record_clipboard() {
    wows_toolkit_nsh::clipboard::replace(&CLIPBOARD);
    CLIPBOARD.refuse.store(false, Ordering::SeqCst);
    CLIPBOARD.copied.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
}

/// The app, every replay it opens read as the sample report.
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    record_clipboard();
    wows_toolkit_nsh::replay_inspector::test_support::read_replays_as_sample();
    let mut app = start();
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _install: None, _guard: guard }
}

/// A WoWs directory with no installed builds and `replays` in its replay
/// directory.
fn install_with(name: &str, replays: &[&str]) -> TestDirectory {
    let install = TestDirectory::new(name);
    std::fs::create_dir(install.0.join("bin")).expect("the bin directory is created");
    let dir = install.0.join("replays");
    std::fs::create_dir(&dir).expect("the replays directory is created");
    for replay in replays {
        std::fs::copy(fixture(replay), dir.join(replay)).expect("the fixture replay is copied");
    }
    install
}

/// The app over `install`'s replays, every replay it opens read as the
/// sample report. The download offer for the replays' builds is declined; a
/// build is offered once per process, so each test lists builds of its own.
fn app_with_install(install: &Path, autoload: bool) -> TestApp {
    let mut app = app_bound_to(install, autoload);
    app.wait_for_within("Download Game Data", Duration::from_secs(90)).unwrap();
    app.press("Cancel").unwrap();
    app.wait_for_gone("Download Game Data", GONE).unwrap();
    app
}

/// The app over `install`, every replay it opens read as the sample report.
fn app_bound_to(install: &Path, autoload: bool) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    record_clipboard();
    wows_toolkit_nsh::replay_inspector::test_support::read_replays_as_sample();
    let settings = InstallSettings::bind(install, autoload);
    let mut app = start();
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _install: Some(settings), _guard: guard }
}

/// Writes an index row for `replay` in the live source, as a pass that read
/// it would.
fn index_row_for(replay: &Path) {
    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        let root = replay.parent().expect("the replay is in a directory");
        let source =
            query::ensure_default_source(&pool, root, jiff::Timestamp::now()).await.expect("the live source exists");
        write_index_row(&pool, replay, source).await;
    });
}

/// Writes an index row for `replay` in `source`.
async fn write_index_row(pool: &sqlx::SqlitePool, replay: &Path, source: SourceId) {
    let now = jiff::Timestamp::now();
    let arena_id = ArenaId::new(982_003);
    let objective = ObjectiveMatch {
        arena_id,
        timestamp: now,
        map: "Atoll".into(),
        game_mode: "Domination".into(),
        game_mode_id: None,
        game_type: "pvp".into(),
        match_group: "pvp".into(),
        version_build: Some(1234),
    };
    query::upsert_match(pool, &objective).await.expect("the match is written");
    let record = ReplayRecord {
        arena_id,
        source_id: source,
        replay_path: replay.to_path_buf(),
        file_mtime: Some(now.as_second()),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId(99_011)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(123_456),
        self_kills: Some(2),
        self_pr: None,
        results_available: true,
        indexed_at: now,
    };
    query::upsert_record(pool, &record).await.expect("the replay record is written");
}

/// Set when a landed replay's held index row may be written.
static LANDED_RELEASED: AtomicBool = AtomicBool::new(false);

/// Writes a landed replay's index row once [`LANDED_RELEASED`] is set, so a
/// test sees the listing before the row and after it.
fn index_landed_once_released(replay: &Path, source: SourceId) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !LANDED_RELEASED.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }

    runtime().block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
        write_index_row(&pool, replay, source).await;
    });
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

/// Opens `directory` as a workspace of its own through the Open menu.
fn open_directory(app: &mut TestApp, directory: &Path) -> OpenedDirectory {
    app.on_pick_paths(vec![directory.to_path_buf()]).unwrap();
    app.press("label:Open").unwrap();
    app.press("Open Replay Directory...").unwrap();

    OpenedDirectory(wows_toolkit_replay_data::directories::canonical_root(directory.to_path_buf()))
}

/// A directory opened as a workspace, closed when dropped: the registry is
/// the process's, and a tab left open would stand in the next test.
struct OpenedDirectory(PathBuf);

impl Drop for OpenedDirectory {
    fn drop(&mut self) {
        // A test that closed the directory itself leaves nothing to close.
        let _ = wows_toolkit_nsh::game_data::GameData::close_directory(self.0.display().to_string());
    }
}

fn open_sample(app: &mut TestApp) {
    open_file(app, fixture(FIRST));
    app.wait_for(SAMPLE_TITLE).unwrap();
}

/// A selector name that matches `text` exactly, read as a regex with its
/// special characters escaped.
fn exactly(text: &str) -> String {
    let escaped: String = text
        .chars()
        .flat_map(|c| r"\.+*?()|[]{}^$".contains(c).then_some('\\').into_iter().chain([c]))
        .collect();
    format!("/^{escaped}$/")
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
    let _opened = open_directory(&mut app, &directory.0);
    let requests = app.screen().file_requests();
    assert!(
        matches!(requests.last(), Some(nsh_core::file_dialog::FileRequest::OpenDialog(dialog)) if dialog.picks_folders()),
        "the directory item asks for a folder"
    );

    app.wait_for(&format!("label:{FIRST}")).unwrap();
    app.wait_for(&format!("label:{SECOND}")).unwrap();
    app.wait_for("label:/^Close directory /").unwrap();

    // The directory is an app tab of its own, right after the Replay Inspector's.
    // Its label is a path, which a selector would read as a regex, so it is
    // matched as one.
    let root = wows_toolkit_replay_data::directories::canonical_root(directory.0.clone());
    let tab = format!("role:tab[{}]", exactly(&wows_toolkit_replay_data::directories::shortened_path(&root)));
    assert!(app.screen().selected(&tab).unwrap(), "the opened directory's tab is shown");
    app.press("role:tab[Replay Inspector]").unwrap();
    app.wait_for_gone(&format!("label:{FIRST}"), GONE).unwrap();
    app.press("role:tab >> nth=1").unwrap();
    assert!(app.screen().selected(&tab).unwrap(), "the directory's tab follows the Replay Inspector's");
    app.wait_for(&format!("label:{FIRST}")).unwrap();

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
    app.wait_for_gone(&tab, GONE).unwrap();
    assert!(
        app.screen().selected("role:tab[Replay Inspector]").unwrap(),
        "closing the shown directory shows the Replay Inspector"
    );
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
    open_sample(&mut app);
    app.wait_for("text:/Team Damage/").unwrap();
    assert!(!app.shows("text:/PR: /"), "the replay was read without a rating table");

    wows_toolkit_nsh::game_data::set_personal_rating(
        wows_toolkit_nsh::replay_inspector::test_support::sample_personal_rating(),
    );
    app.wait_for("text:/PR: /").unwrap();
    app.wait_for(SAMPLE_TITLE).unwrap();
}

/// With autoload on, the newest replay of the game's directory opens when the
/// inspector first lists it.
#[test]
fn autoload_opens_the_newest_listed_replay_at_startup() {
    const OLDER: &str = "20251001_145225_PBSA710-Ocean_28_naval_mission.wowsreplay";
    const NEWER: &str = "20260127_185500_PASD610-Forrest-Sherman_56_AngelWings.wowsreplay";

    let install = install_with("autoload", &[NEWER, OLDER]);
    let mut app = app_with_install(&install.0, true);
    on_linux_desktop(&mut app);

    app.wait_for(SAMPLE_TITLE).unwrap();
    assert_eq!(app.screen().count(&format!("label:Close {SAMPLE_TITLE}")), Ok(1), "one replay is opened");
    // The tab's hint names the file it reads.
    let newest = install.0.join("replays").join(NEWER).display().to_string();
    app.hover(&format!("label:Close {SAMPLE_TITLE}")).unwrap();
    app.wait_until("the newest replay's path is shown", GONE, |screen| screen.shows(&newest)).unwrap();
}

#[test]
fn without_autoload_nothing_opens_at_startup() {
    const REPLAY: &str = "20250206_020938_PASD720-Hull_47_Sleeping_Giant.wowsreplay";

    let install = install_with("no-autoload", &[REPLAY]);
    let mut app = app_with_install(&install.0, false);

    app.wait_for(&format!("label:{REPLAY}")).unwrap();
    app.idle(Duration::from_millis(500)).unwrap();
    assert!(app.shows("Select a replay"), "no replay is opened");
}

/// A finished index pass reads the index again, so a listed replay the index
/// now holds stops reading as not indexed.
#[test]
fn the_listing_reads_the_index_again_after_a_pass() {
    const REPLAY: &str = "20241112_172819_PFSC510-Colbert_44_Path_warrior.wowsreplay";

    let install = install_with("index-pass", &[REPLAY]);
    let mut app = app_with_install(&install.0, false);
    app.wait_for("text:/not indexed/").unwrap();

    index_row_for(&install.0.join("replays").join(REPLAY));
    app.press_key("mod+k").unwrap();
    app.press("Index All Replays").unwrap();
    app.wait_for_within("text:/^Indexed 0 replays, 1 were already in/", Duration::from_secs(30)).unwrap();
    app.wait_for_gone("text:/not indexed/", GONE).unwrap();
}

/// "Copy Replay" puts the file itself on the clipboard, and says so when the
/// clipboard refuses it.
#[test]
fn copying_a_listed_replay_puts_the_file_on_the_clipboard() {
    let directory = TestDirectory::new("copy");
    std::fs::copy(fixture(FIRST), directory.0.join(FIRST)).expect("the fixture is copied");

    let mut app = app();
    on_linux_desktop(&mut app);
    let _opened = open_directory(&mut app, &directory.0);
    let row = format!("label:{FIRST}");
    app.wait_for(&row).unwrap();

    app.right_click(&row).unwrap();
    app.press("Copy Replay").unwrap();
    app.wait_for("role:alert[Copy Replay]").unwrap();
    let root = wows_toolkit_replay_data::directories::canonical_root(directory.0.clone());
    assert_eq!(CLIPBOARD.last(), Some(vec![root.join(FIRST)]), "the replay file is on the clipboard");

    CLIPBOARD.refuse.store(true, Ordering::SeqCst);
    app.right_click(&row).unwrap();
    app.press("Copy Replay").unwrap();
    app.wait_for("role:alert[/^the replay files could not be copied: /]").unwrap();
}

/// "Check game data" offers the builds an opened directory's replays need,
/// again after the offer was declined, and says so when none is missing.
#[test]
fn checking_game_data_offers_the_directory_builds() {
    const REPLAY: &str = "20190420_125057_PASB017-Montana-1945_15_NE_north.wowsreplay";

    let empty = TestDirectory::new("check-empty");
    let needing = TestDirectory::new("check-needing");
    std::fs::copy(fixture(REPLAY), needing.0.join(REPLAY)).expect("the fixture is copied");

    let mut app = app();
    on_linux_desktop(&mut app);
    let _empty = open_directory(&mut app, &empty.0);
    app.wait_for("No replays found").unwrap();
    app.press("Check game data").unwrap();
    app.wait_for("role:alert[Game data is available for the listed replays.]").unwrap();

    let _needing = open_directory(&mut app, &needing.0);
    app.wait_for(&format!("label:{REPLAY}")).unwrap();
    for _ in 0..2 {
        app.press("Check game data").unwrap();
        app.wait_for_within("Download Game Data", Duration::from_secs(90)).unwrap();
        app.press("Cancel").unwrap();
        app.wait_for_gone("Download Game Data", GONE).unwrap();
    }
}

/// An install whose build does not load says why, once as a notice and
/// above the listing.
#[test]
fn game_data_that_fails_to_load_says_why() {
    let install = install_with("load-fails", &[]);
    let mut app = app_bound_to(&install.0, false);

    let failure = || match wows_toolkit_nsh::game_data::status() {
        GameDataState::Failed { error } => Some(error.to_string()),
        _ => None,
    };
    app.wait_until("the load fails", Duration::from_secs(30), |_| failure().is_some()).unwrap();
    let reason = failure().expect("the load failed");

    app.wait_for(&format!("role:alert[{}]", exactly(&reason))).unwrap();
    app.wait_for(&format!("text:{}", exactly(&format!("Game data failed to load: {reason}")))).unwrap();
}

/// Without a WoWs directory the inspector says so above the listing, and the
/// listing says why it is empty.
#[test]
fn without_a_wows_directory_the_inspector_says_so() {
    let mut app = app();

    app.wait_for("Game data failed to load: World of Warships directory is not set").unwrap();
    app.wait_for(&format!("text:{}", exactly("World of Warships directory is not set"))).unwrap();
}

/// A battle that lands in an opened directory is indexed there, and its row
/// stops reading as not indexed.
#[test]
fn a_replay_landing_in_an_opened_directory_is_indexed() {
    const REPLAY: &str = "20250117_004534_PWSD108-Oland_15_NE_north.wowsreplay";

    let directory = TestDirectory::new("landed");

    let mut app = app();
    LANDED_RELEASED.store(false, Ordering::SeqCst);
    let _hook = LandedIndexHook::install(index_landed_once_released);
    on_linux_desktop(&mut app);
    let _opened = open_directory(&mut app, &directory.0);
    app.wait_for("No replays found").unwrap();

    // The game renames a finished battle into place.
    let staged = directory.0.join("landing.part");
    std::fs::copy(fixture(REPLAY), &staged).expect("the fixture is copied");
    std::fs::rename(&staged, directory.0.join(REPLAY)).expect("the replay is renamed into place");
    app.wait_for(&format!("label:{REPLAY}")).unwrap();
    app.wait_for("text:/not indexed/").unwrap();

    LANDED_RELEASED.store(true, Ordering::SeqCst);
    app.wait_for_gone("text:/not indexed/", Duration::from_secs(30)).unwrap();
}

/// A landed replay's index rows stored through a test function, the parse
/// put back when dropped.
struct LandedIndexHook;

impl LandedIndexHook {
    fn install(index: wows_toolkit_nsh::game_data::test_support::LandedIndex) -> Self {
        wows_toolkit_nsh::game_data::test_support::index_landed_with(Some(index));
        Self
    }
}

impl Drop for LandedIndexHook {
    fn drop(&mut self) {
        wows_toolkit_nsh::game_data::test_support::index_landed_with(None);
    }
}
