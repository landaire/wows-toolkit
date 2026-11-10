//! Headless behavior tests for the game-data Settings sections, the job
//! reports and status strip, and the download offer in `ui/game_data.tsx`.

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
fn app_with(wows_dir: Option<&Path>) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    save_setting(keys::WOWS_DIR, &wows_dir.map(|dir| dir.display().to_string()).unwrap_or_default());
    // The first-run questions and the startup update check are the shell's
    // tests' to exercise; here they would stand in front of the sections.
    for answered in [keys::BUILD_CONSENT_SHOWN, keys::REPLAY_CONSENT_SHOWN, keys::LANGUAGE_SELECTION_SHOWN] {
        save_setting(answered, &true);
    }
    save_setting(keys::CHECK_FOR_UPDATES, &false);
    wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");

    let entry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
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

    app.press("Settings").unwrap();
    app.wait_for("Game Data Cache").unwrap();
    app.wait_for("Replay Index").unwrap();
    app.wait_for("Set the World of Warships directory to index its replays.").unwrap();
    assert!(!app.shows("Replay Directories"), "open directories are managed from their tabs, not Settings");
}

#[test]
fn the_cache_directory_is_picked_typed_and_cleared() {
    let mut app = app_with(None);
    let picked = TestDirectory::new("cache-picked");
    let typed = TestDirectory::new("cache-typed");

    app.press("Settings").unwrap();
    app.wait_for("Game Data Cache").unwrap();
    app.press("label:Cache game data for old replay compatibility").unwrap();
    app.wait_until("the auto-dump flag is saved", Duration::from_secs(5), |_| {
        read_setting::<bool>(keys::AUTO_DUMP_GAME_DATA) == Some(true)
    })
    .unwrap();

    app.on_pick_paths(vec![picked.0.clone()]).unwrap();
    app.press("text:Browse").unwrap();
    app.wait_until("the picked directory is saved", Duration::from_secs(5), |_| {
        read_setting::<String>(keys::GAME_DATA_CACHE_DIR) == Some(picked.0.display().to_string())
    })
    .unwrap();

    app.clear("label:Game data cache directory").unwrap();
    app.type_text("label:Game data cache directory", &typed.0.display().to_string()).unwrap();
    app.submit("label:Game data cache directory").unwrap();
    app.wait_until("the typed directory is saved", Duration::from_secs(5), |_| {
        read_setting::<String>(keys::GAME_DATA_CACHE_DIR) == Some(typed.0.display().to_string())
    })
    .unwrap();

    // An empty field is the default location.
    app.clear("label:Game data cache directory").unwrap();
    app.submit("label:Game data cache directory").unwrap();
    app.wait_until("the default location is saved", Duration::from_secs(5), |_| {
        read_setting::<String>(keys::GAME_DATA_CACHE_DIR) == Some(String::new())
    })
    .unwrap();

    save_setting(keys::AUTO_DUMP_GAME_DATA, &false);
    wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");
}

/// Without a WoWs directory the bound cache still reads the builds dumped into
/// the configured cache directory, which is what renders a replay with no
/// install. The installed build still needs one.
#[test]
fn without_a_game_directory_the_configured_cache_reads_dumped_builds() {
    let _guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    let cache_dir = TestDirectory::new("cache-only");
    let dump = cache_dir.0.join("14.1.0_12345");
    std::fs::create_dir_all(&dump).expect("the dump directory is created");
    std::fs::write(dump.join("metadata.toml"), "").expect("the dump metadata is written");
    save_setting(keys::WOWS_DIR, &String::new());
    save_setting(keys::GAME_DATA_CACHE_DIR, &cache_dir.0.display().to_string());
    wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");

    let cache = wows_toolkit_nsh::game_data::cache().expect("a cache is bound without an install");
    assert_eq!(cache.dump_dir_for(12345, None), Some(dump));
    assert!(matches!(
        wows_toolkit_nsh::game_data::wait_current_build(),
        Err(wows_toolkit_nsh::game_data::GameDataError::NotConfigured)
    ));

    save_setting(keys::GAME_DATA_CACHE_DIR, &String::new());
    wows_toolkit_nsh::game_data::reload_settings().expect("the settings are read");
}

#[test]
fn an_index_pass_from_the_palette_reports_on_the_tab_shown() {
    // No replays, so no build is missing and no download offer stands in front.
    let install = TestDirectory::new("index");
    std::fs::create_dir_all(install.0.join("bin")).expect("the bin directory is created");
    std::fs::create_dir_all(install.0.join("replays")).expect("the replays directory is created");
    let mut app = app_with(Some(&install.0));

    app.wait_for("text:/^Game data failed to load: no installed builds found under /").unwrap();
    app.press_key("mod+k").unwrap();
    app.press("Index All Replays").unwrap();
    app.wait_for_within("text:Indexed 0 replays, 0 could not be read", Duration::from_secs(30)).unwrap();
    assert!(!app.shows("Replay Index"), "the report is not only on the Settings tab");
}

#[test]
fn a_replay_on_a_build_nothing_has_is_offered_for_download_on_any_tab() {
    let install = install_without_builds("offer");
    let mut app = app_with(Some(&install.0));

    // The inspector's listing finds the replay and asks; the offer opens over
    // the inspector without the Settings tab ever being shown.
    app.wait_for_within("Download Game Data", Duration::from_secs(90)).unwrap();
    app.wait_for("text:/needed by 1 replay\\(s\\)/").unwrap();

    app.press("Cancel").unwrap();
    app.wait_for_gone("Download Game Data", Duration::from_secs(5)).unwrap();
}
