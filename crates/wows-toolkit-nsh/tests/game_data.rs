//! Headless behavior tests for the game-data status indicator and the
//! Settings sections in `ui/game_data.tsx`.

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::time::Duration;

use nsh_core::Points;
use nsh_core::Size;
use wows_toolkit_viewmodel::settings::keys;

const FIXTURE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-game-data-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the test directory is created");
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

static TEST_STORAGE: OnceLock<PathBuf> = OnceLock::new();

fn configure_test_storage() {
    let path = TEST_STORAGE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-game-data-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test settings directory is created");
        path
    });
    wows_toolkit_config::storage_override::set(path.clone());
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

/// Starts the app with `wows_dir` saved as the WoWs directory and the service
/// rebound to it.
#[cfg(not(feature = "buck"))]
fn app_with(wows_dir: Option<&Path>) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    save_setting(keys::WOWS_DIR, &wows_dir.map(|dir| dir.display().to_string()).unwrap_or_default());
    wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");

    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

#[cfg(feature = "buck")]
fn app_with(wows_dir: Option<&Path>) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    save_setting(keys::WOWS_DIR, &wows_dir.map(|dir| dir.display().to_string()).unwrap_or_default());
    wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");

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

fn database<T>(work: impl AsyncFnOnce(&sqlx::SqlitePool) -> T) -> T {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime for the settings database");
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        let result = work(&pool).await;
        pool.close().await;
        result
    })
}

fn save_setting<T: serde::Serialize>(key: &str, value: &T) {
    database(async |pool| wows_toolkit_config::queries::set_setting(pool, key, value).await)
        .expect("the setting is saved");
}

fn read_setting<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    database(async |pool| wows_toolkit_config::queries::try_get_setting::<T>(pool, key).await)
        .expect("the setting is read")
}

/// A WoWs directory with no installed builds, and the fixture replay in its
/// replay directory.
fn install_without_builds(name: &str) -> TestDirectory {
    let install = TestDirectory::new(name);
    std::fs::create_dir_all(install.0.join("bin")).expect("the bin directory is created");
    std::fs::create_dir_all(install.0.join("replays")).expect("the replays directory is created");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FIXTURE);
    std::fs::copy(fixture, install.0.join("replays").join(FIXTURE)).expect("the fixture replay is copied");
    install
}

#[test]
fn without_a_game_directory_the_sections_say_what_is_missing() {
    let mut app = app_with(None);

    app.wait_for("text:Game data: no World of Warships directory").unwrap();
    app.press("Settings").unwrap();
    app.wait_for("Game data cache").unwrap();
    app.wait_for("Replay index").unwrap();
    app.wait_for("Replay directories").unwrap();
    app.wait_for("Set the World of Warships directory to index its replays.").unwrap();
    app.wait_for("No replay directories are open.").unwrap();
}

#[test]
fn a_replay_directory_is_added_pinned_saved_and_closed() {
    let mut app = app_with(None);
    let directory = TestDirectory::new("listing");
    let root = wows_toolkit_replay_data::directories::canonical_root(directory.0.clone());
    let label = wows_toolkit_replay_data::directories::shortened_path(&root);

    app.press("Settings").unwrap();
    app.on_pick_paths(vec![directory.0.clone()]).unwrap();
    app.press("Add Directory...").unwrap();
    app.wait_for(&format!("text:{}", root.display())).unwrap();

    app.press(&format!("text:Pin {label}")).unwrap();
    app.wait_for(&format!("text:{label} [Pinned]")).unwrap();
    let saved: Vec<PathBuf> = read_setting(wows_toolkit_replay_data::directories::PINNED_DIRECTORIES_KEY)
        .expect("the pinned directories are saved");
    assert_eq!(saved, vec![root.clone()]);

    // A pinned directory stays open.
    let _ = app.press(&format!("text:Close {label}"));
    assert!(app.shows(&format!("{label} [Pinned]")));

    app.press(&format!("text:Unpin {label}")).unwrap();
    app.wait_for(&format!("text:Pin {label}")).unwrap();
    let saved: Vec<PathBuf> = read_setting(wows_toolkit_replay_data::directories::PINNED_DIRECTORIES_KEY)
        .expect("the pinned directories are saved");
    assert!(saved.is_empty(), "an unpinned directory is no longer saved");

    app.press(&format!("text:Close {label}")).unwrap();
    app.wait_for("No replay directories are open.").unwrap();
}

#[test]
fn the_cache_settings_are_saved_and_the_cache_is_measured() {
    let mut app = app_with(None);
    let cache_dir = TestDirectory::new("cache");

    app.press("Settings").unwrap();
    app.wait_for("Game data cache").unwrap();
    app.press("label:Cache game data automatically").unwrap();
    app.wait_until("the auto-dump flag is saved", Duration::from_secs(5), |_| {
        read_setting::<bool>(keys::AUTO_DUMP_GAME_DATA) == Some(true)
    })
    .unwrap();

    app.on_pick_paths(vec![cache_dir.0.clone()]).unwrap();
    app.press("Browse...").unwrap();
    app.wait_for(&format!("text:Directory: {}", cache_dir.0.display())).unwrap();
    app.wait_for("text:0 B in 0 cached builds").unwrap();
    assert_eq!(read_setting::<String>(keys::GAME_DATA_CACHE_DIR), Some(cache_dir.0.display().to_string()));

    save_setting(keys::AUTO_DUMP_GAME_DATA, &false);
    save_setting(keys::GAME_DATA_CACHE_DIR, &String::new());
    wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");
}

#[test]
fn an_install_without_builds_fails_the_load_and_indexes_nothing() {
    let install = install_without_builds("index");
    let mut app = app_with(Some(&install.0));

    app.wait_for("text:/^Game data failed: no installed builds found under /").unwrap();
    app.press("Settings").unwrap();
    app.press("Build Index").unwrap();
    app.wait_for_within("text:Indexed 0 replays, 1 could not be read.", Duration::from_secs(30)).unwrap();
}

#[test]
fn a_replay_on_a_build_nothing_has_is_offered_for_download() {
    let install = install_without_builds("offer");
    let mut app = app_with(Some(&install.0));

    app.press("Settings").unwrap();
    app.press("Check Game Data for Replays").unwrap();
    app.wait_for("Download Game Data").unwrap();
    app.wait_for("text:/1 replays/").unwrap();

    app.press("Cancel").unwrap();
    app.wait_for_gone("Download Game Data", Duration::from_secs(5)).unwrap();

    // A build is put to the reader once a session.
    app.press("Check Game Data for Replays").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(!app.shows("Download Game Data"));
}
