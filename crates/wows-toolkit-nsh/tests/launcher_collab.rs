//! Headless tests of the Launcher tab and the collaboration session popover
//! over the real TSX bundle.

use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use nsh_core::Points;
use nsh_core::Size;

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

fn test_storage() -> &'static std::path::Path {
    TEST_STORAGE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-launcher-collab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test settings directory is created");
        path
    })
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn app() -> TestApp {
    app_after(|| {})
}

/// Starts the app once `prepare` has run against the test settings database,
/// under the same lock, so no other test changes the database in between.
#[cfg(feature = "buck")]
fn app_after(prepare: impl FnOnce()) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    wows_toolkit_config::storage_override::set(test_storage().to_path_buf());
    prepare();
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
fn app_after(prepare: impl FnOnce()) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    wows_toolkit_config::storage_override::set(test_storage().to_path_buf());
    prepare();
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

/// A directory removed when the test ends.
struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-{name}-{}", std::process::id()));
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

/// The launcher's saved settings row, once `ready` accepts it. The row is
/// written by a background task, so it is read until it lands.
fn saved_launcher_settings(ready: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let row = runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(&pool, "launcher")
                .await
                .expect("the launcher row is readable")
        });
        if let Some(row) = &row
            && ready(row)
        {
            return row.clone();
        }
        assert!(Instant::now() < deadline, "the launcher settings were not saved: {row:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn launcher_shows_its_sections_and_adds_a_manual_install() {
    let mut app = app();
    let install = TestDirectory::new("manual-install");

    app.press("Launcher").unwrap();
    app.wait_for("Account").unwrap();
    app.wait_for("Installs").unwrap();
    app.wait_for("Launch options").unwrap();
    app.wait_for("Install a game").unwrap();
    app.wait_for("Not signed in.").unwrap();

    app.on_pick_paths(vec![install.0.clone()]).unwrap();
    app.press("Add install...").unwrap();
    let shown = install.0.display().to_string();
    app.wait_for(&shown).unwrap();
    app.wait_for("Manual").unwrap();
    app.wait_for("Use for toolkit").unwrap();

    let saved = saved_launcher_settings(|row| {
        row["manual_installs"].as_array().is_some_and(|installs| installs.iter().any(|path| path == &shown))
    });
    assert!(saved["manual_installs"].as_array().is_some_and(|installs| !installs.is_empty()));
}

#[test]
fn launcher_saves_launch_options_and_proton_paths() {
    let mut app = app();
    let proton = TestDirectory::new("proton");
    let binary = proton.0.join("proton");
    std::fs::write(&binary, b"").unwrap();

    app.press("Launcher").unwrap();
    app.wait_for("Launch options").unwrap();

    app.type_text("label:Extra launch args (one per line)", "-safe_arg").unwrap();
    // Leaving a field commits it.
    app.type_text("label:Environment variables (KEY=VALUE, one per line)", "DXVK_HUD=1").unwrap();
    app.type_text("label:Extra launch args (one per line)", "").unwrap();
    saved_launcher_settings(|row| {
        row["extra_args"].as_array().is_some_and(|args| args.iter().any(|arg| arg == "-safe_arg"))
            && row["env_vars"].as_array().is_some_and(|vars| !vars.is_empty())
    });

    app.on_pick_paths(vec![binary.clone()]).unwrap();
    app.press("text:Browse... >> nth=0").unwrap();
    let shown = binary.display().to_string();
    app.wait_for(&shown).unwrap();
    saved_launcher_settings(|row| row["proton_binary"] == shown.as_str());
}

#[test]
fn launcher_sign_in_offers_every_realm() {
    let mut app = app();

    app.press("Launcher").unwrap();
    app.wait_for("Not signed in.").unwrap();
    app.press("Sign in").unwrap();
    for realm in ["EU", "NA", "Asia", "RU (experimental)"] {
        app.wait_for(realm).unwrap();
    }
    app.wait_for("label:Username").unwrap();
    app.wait_for("label:Password").unwrap();
    app.press("NA").unwrap();
}

#[test]
fn session_popover_needs_a_name_and_refuses_a_foreign_token() {
    let mut app = app();

    app.press("label:Session").unwrap();
    app.wait_for("Display name:").unwrap();
    app.wait_for("Host a Session").unwrap();
    app.wait_for("Join a Session").unwrap();
    app.wait_for("Enter a display name first").unwrap();

    app.type_text("label:Display name:", "Tester").unwrap();
    app.wait_for_gone("Enter a display name first", Duration::from_secs(5)).unwrap();

    app.type_text("label:Session Token:", "https://example.com/not-a-token").unwrap();
    app.press("Paste token & join").unwrap();
    // The address warning stands in front of joining.
    app.wait_for("Network Warning").unwrap();
    app.press("Continue").unwrap();
    app.wait_for("Invalid token: toolkit-").unwrap();
}

/// The saved WoWs directory, once it is `expected`.
fn wait_for_saved_wows_dir(expected: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let saved = runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            wows_toolkit_config::queries::try_get_setting::<String>(&pool, "wows_dir")
                .await
                .expect("the directory row is readable")
        });
        if saved.as_deref() == Some(expected) {
            return;
        }
        assert!(Instant::now() < deadline, "the WoWs directory was not saved: {saved:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Empties the saved manual installs.
fn forget_manual_installs() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        let row = wows_toolkit_config::queries::try_get_setting::<serde_json::Value>(&pool, "launcher")
            .await
            .expect("the launcher row is readable");
        if let Some(mut row) = row {
            row["manual_installs"] = serde_json::json!([]);
            wows_toolkit_config::queries::set_setting(&pool, "launcher", &row).await.expect("the launcher row is saved");
        }
    });
}

#[test]
fn launcher_install_is_used_for_the_toolkit_and_game_data_reloads() {
    // Other tests in this process save installs too; with none saved, the one
    // added here is the only row with the button.
    let mut app = app_after(forget_manual_installs);
    let install = TestDirectory::new("toolkit-install");
    std::fs::create_dir(install.0.join("bin")).expect("the bin directory is created");

    app.press("Launcher").unwrap();
    app.on_pick_paths(vec![install.0.clone()]).unwrap();
    app.press("Add install...").unwrap();
    let shown = install.0.display().to_string();
    app.wait_for(&shown).unwrap();
    app.press("Use for toolkit").unwrap();

    wait_for_saved_wows_dir(&shown);
    // The install holds no build, so the reloaded game data says it failed.
    app.wait_for_within("text:/^Game data failed to load: /", Duration::from_secs(30)).unwrap();
}
