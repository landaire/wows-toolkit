//! Headless interaction tests of the Armor Viewer tab over the real TSX bundle.
//! They run without game data, so they cover the controls that work before a
//! ship is loaded.

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::time::Duration;

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

static TEST_STORAGE: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-armor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).expect("the test settings directory is created");

    path
});

const GONE: Duration = Duration::from_secs(5);

fn configure_test_storage() {
    wows_toolkit_config::storage_override::set(TEST_STORAGE.clone());

    // The first-run questions would stand in front of the tab.
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            for key in ["build_consent_window_shown", "replay_consent_prompt_shown", "language_selection_shown"] {
                wows_toolkit_config::queries::set_setting_raw(&pool, key, "true").await.expect("a question is answered");
            }
            wows_toolkit_config::queries::set_setting_raw(&pool, "check_for_updates", "false")
                .await
                .expect("the startup update check is off");
        },
    );
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1600.0), Points(1000.0))).expect("the desktop viewport is applied");
    app.wait_for("role:tab[Armor Viewer]").expect("the tab strip is drawn");
    TestApp { app, _guard: guard }
}

/// The app with the Armor Viewer tab selected and its first view drawn.
fn armor_viewer() -> TestApp {
    let mut app = start();
    app.press("role:tab[Armor Viewer]").unwrap();
    app.wait_for("id:armor-options-panel").unwrap();
    app.wait_for("id:armor-sidebar-search").unwrap();
    app
}

#[test]
fn the_options_rail_collapses_to_an_edge_control_and_expands_from_it() {
    let mut app = armor_viewer();
    assert!(app.screen().matches("id:armor-show-hidden-button").unwrap());
    assert!(!app.screen().matches("id:armor-options-expand").unwrap());

    app.press("id:armor-options-collapse").unwrap();
    app.wait_for_gone("id:armor-options-panel", GONE).unwrap();
    assert!(!app.screen().matches("id:armor-show-hidden-button").unwrap());
    app.wait_for("id:armor-options-expand").unwrap();

    app.press("id:armor-options-expand").unwrap();
    app.wait_for("id:armor-options-panel").unwrap();
    app.wait_for("id:armor-show-hidden-button").unwrap();
    app.wait_for_gone("id:armor-options-expand", GONE).unwrap();
}

#[test]
fn the_ships_list_collapses_to_an_edge_control_and_expands_from_it() {
    let mut app = armor_viewer();

    app.press("id:armor-ships-collapse").unwrap();
    app.wait_for_gone("id:armor-sidebar-search", GONE).unwrap();
    assert!(!app.screen().matches("id:armor-ships-collapse").unwrap());
    app.wait_for("id:armor-ships-expand").unwrap();

    app.press("id:armor-ships-expand").unwrap();
    app.wait_for("id:armor-sidebar-search").unwrap();
    app.wait_for("id:armor-ships-collapse").unwrap();
    app.wait_for_gone("id:armor-ships-expand", GONE).unwrap();
}

#[test]
fn a_rail_section_is_built_only_while_it_is_expanded() {
    let mut app = armor_viewer();
    app.wait_for("id:armor-options-body-Display").unwrap();
    assert!(!app.screen().matches("id:armor-options-body-Visibility").unwrap());
    assert!(!app.screen().matches("id:armor-options-body-Hull").unwrap());

    app.press("id:armor-options-section-Visibility").unwrap();
    app.wait_for("id:armor-options-body-Visibility").unwrap();

    app.press("id:armor-options-section-Display").unwrap();
    app.wait_for_gone("id:armor-options-body-Display", GONE).unwrap();
    assert!(app.screen().matches("id:armor-options-body-Visibility").unwrap());
}

#[test]
fn typing_in_the_ship_search_records_the_query() {
    let mut app = armor_viewer();
    app.type_text("id:armor-sidebar-search", "yamato").unwrap();
    assert_eq!(app.screen().input_text("id:armor-sidebar-search"), Ok(Some("yamato")));
    app.press_key("backspace").unwrap();
    assert_eq!(app.screen().input_text("id:armor-sidebar-search"), Ok(Some("yamat")));
}

#[test]
fn the_penetration_panel_opens_from_its_toggle_and_closes_from_its_header() {
    let mut app = armor_viewer();
    assert!(!app.screen().matches("id:armor-pen-search").unwrap());

    app.press("id:armor-analysis-toggle").unwrap();
    app.wait_for("id:armor-pen-search").unwrap();
    app.type_text("id:armor-sidebar-search", "a").unwrap();
    assert!(app.screen().matches("id:armor-pen-search").unwrap(), "using the ships list must not dismiss the panel");

    app.press("id:armor-analysis-toggle").unwrap();
    app.wait_for_gone("id:armor-pen-search", GONE).unwrap();

    app.press("id:armor-analysis-toggle").unwrap();
    app.wait_for("id:armor-pen-search").unwrap();
    app.press("id:armor-pen-close").unwrap();
    app.wait_for_gone("id:armor-pen-search", GONE).unwrap();
}

#[test]
fn without_game_data_the_ships_list_says_why_it_is_empty() {
    let mut app = armor_viewer();
    app.wait_for(r"text:/^(Load game data in Settings to use the Armor Viewer\.|Loading ship catalog\.\.\.)$/").unwrap();

    // A host without a GPU stops at the viewport's own state rather than the ship prompt.
    app.wait_for(
        r"text:/^(Select a ship from the list|Initializing 3D viewport\.\.\.|Armor viewport failed to initialize: .*)$/",
    )
    .unwrap();

    assert!(!app.screen().enabled("id:armor-show-hidden-button").unwrap());
    app.press("id:armor-options-section-Visibility").unwrap();
    app.wait_for("id:armor-options-body-Visibility >> text:No ship loaded").unwrap();
}

#[test]
fn the_export_controls_do_nothing_without_a_ship() {
    let mut app = armor_viewer();
    assert!(!app.screen().enabled("id:armor-export-trigger").unwrap());
    assert!(app.press("id:armor-export-trigger").is_err(), "a disabled export must refuse the press");
    app.press_key("mod+k").unwrap();
    app.wait_for("Commands").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("Commands", GONE).unwrap();
    assert!(!app.screen().matches("id:armor-export-confirm-export").unwrap());
    assert!(!app.screen().matches("id:armor-export-confirm").unwrap());
    assert!(!app.shows("Export Ship Model"));
    assert!(app.screen().file_requests().is_empty(), "no save dialog may open without a ship");
}

#[test]
fn the_palette_opens_on_the_ship_list_in_the_armor_viewer() {
    let mut app = armor_viewer();
    app.press_key("mod+p").unwrap();
    app.wait_for("Open ship in Armor Viewer").unwrap();
    app.wait_for(r"text:/^(Loading the ship list\.\.\.|No ship list: .*)$/").unwrap();
    app.press("id:palette-back").unwrap();
    app.wait_for("Open ship in Armor Viewer...").unwrap();
}
