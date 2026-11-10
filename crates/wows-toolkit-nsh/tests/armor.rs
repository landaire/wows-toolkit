//! Headless interaction tests of the Armor Viewer tab over the real TSX bundle.
//! They run without game data, so they cover the controls that work before a
//! ship is loaded; the views that need a ship run over a fake service state
//! (`tests/armor/*.tsx`).

use std::ops::Deref;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
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
                wows_toolkit_config::queries::set_setting_raw(&pool, key, "true")
                    .await
                    .expect("a question is answered");
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

fn launch(entry: PathBuf) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    // The armor service outlives each app, so a panel the last test opened would still be open.
    wows_toolkit_nsh::armor::test_support::reset_panels();
    wows_toolkit_nsh::armor::test_support::take_pending_ship();
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1600.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

fn start() -> TestApp {
    let mut app = launch(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx"));
    app.wait_for("role:tab[Armor Viewer]").expect("the tab strip is drawn");
    app
}

/// An armor view over the fake service state of `tests/armor/<name>`, on a
/// desktop where a press gives a view the keyboard focus (the headless
/// platform moves it to views only as macOS and the web do).
fn fixture(name: &str) -> TestApp {
    let mut app = launch(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/armor").join(name));
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::MacOs;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .expect("the desktop environment is applied");
    app
}

/// The ships list over a small catalog, its tree made on the desktop.
fn ships_tree() -> TestApp {
    let mut app = fixture("ships_tree.tsx");
    app.press("id:fixture-load-catalog").unwrap();
    app.wait_for("role:treeitem[U.S.A.]").unwrap();
    app
}

fn wait_selected(app: &mut TestApp, label: &str) {
    let selector = format!("role:treeitem[{label}]");
    app.wait_for(&selector).unwrap();
    let deadline = Instant::now() + GONE;
    while !app.screen().selected(&selector).unwrap() {
        assert!(Instant::now() < deadline, "the {label} row is never selected");
        app.wait(Duration::from_millis(20)).unwrap();
    }
}

/// The ship the viewer was asked to open, by its param index.
fn wait_pending_ship(app: &mut TestApp) -> String {
    let deadline = Instant::now() + GONE;
    loop {
        if let Some(ship) = wows_toolkit_nsh::armor::test_support::take_pending_ship() {
            return ship;
        }
        assert!(Instant::now() < deadline, "no ship is opened");
        app.wait(Duration::from_millis(20)).unwrap();
    }
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
    app.wait_for(r"text:/^(Load game data in Settings to use the Armor Viewer\.|Loading ship catalog\.\.\.)$/")
        .unwrap();

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

#[test]
fn the_pane_sharing_menu_holds_only_its_three_checkboxes() {
    let mut app = armor_viewer();
    app.press("id:armor-sidebar-common").unwrap();
    app.wait_for("id:armor-pane-mirror-cameras").unwrap();
    assert!(app.screen().matches("id:armor-pane-stack").unwrap());
    assert!(app.screen().matches("id:armor-pane-sync-options").unwrap());
    assert!(!app.shows("Common settings"), "the menu carries no heading of its own");
}

#[test]
fn the_penetration_panel_scrolls_only_its_ship_list() {
    let mut app = armor_viewer();
    app.press("id:armor-analysis-toggle").unwrap();
    app.wait_for("id:armor-pen-ships").unwrap();
    app.wait_for("id:armor-pen-ships >> text:Search and add ships above to compare penetration").unwrap();
    assert!(
        !app.screen().matches("id:armor-pen-ships >> id:armor-pen-search").unwrap(),
        "the search sits above the list"
    );
    assert!(!app.screen().matches("id:armor-pen-clear").unwrap(), "Clear all needs a ship to clear");
}

#[test]
fn a_partly_shown_zone_draws_its_mixed_checkbox() {
    let mut app = fixture("options_rail.tsx");
    app.press("id:armor-options-section-Visibility").unwrap();
    app.wait_for("id:armor-vis-zone-Citadel").unwrap();
    app.wait_for("id:armor-vis-zone-Citadel >> text:-").unwrap();
}

#[test]
fn camos_take_changes_only_while_the_export_holds_the_mesh() {
    let mut app = fixture("export_dialog.tsx");
    app.wait_for("id:armor-export-camo-1").unwrap();
    assert!(!app.screen().enabled("id:armor-export-camo-1").unwrap());
    assert!(!app.screen().enabled("id:armor-export-camo-2").unwrap());
    assert!(app.screen().enabled("id:armor-export-contents-mesh").unwrap(), "the contents stay choosable");

    app.press("id:fixture-export-mesh").unwrap();
    let deadline = Instant::now() + GONE;
    while !app.screen().enabled("id:armor-export-camo-1").unwrap() {
        assert!(Instant::now() < deadline, "a mesh export never takes camo changes");
        app.wait(Duration::from_millis(20)).unwrap();
    }
    assert!(app.screen().enabled("id:armor-export-camo-2").unwrap());
}

#[test]
fn the_arrow_keys_walk_the_ships_tree_and_return_opens_a_ship() {
    let mut app = ships_tree();
    app.press("role:treeitem[U.S.A.]").unwrap();
    app.wait_for("role:treeitem[Destroyers]").unwrap();
    wait_selected(&mut app, "U.S.A.");

    app.press_key("down").unwrap();
    wait_selected(&mut app, "Destroyers");
    // A folded branch opens, then Right goes to its first child.
    app.press_key("right").unwrap();
    app.wait_for("role:treeitem[II Sampson]").unwrap();
    assert!(app.screen().selected("role:treeitem[Destroyers]").unwrap());
    app.press_key("right").unwrap();
    wait_selected(&mut app, "II Sampson");
    app.press_key("down").unwrap();
    wait_selected(&mut app, "III Wickes");
    // Left goes to the parent, then folds it.
    app.press_key("left").unwrap();
    wait_selected(&mut app, "Destroyers");
    app.press_key("left").unwrap();
    app.wait_for_gone("role:treeitem[II Sampson]", GONE).unwrap();
    app.press_key("up").unwrap();
    wait_selected(&mut app, "U.S.A.");
    app.press_key("left").unwrap();
    app.wait_for_gone("role:treeitem[Destroyers]", GONE).unwrap();
    assert!(wows_toolkit_nsh::armor::test_support::take_pending_ship().is_none(), "moving opens no ship");

    app.press_key("down").unwrap();
    wait_selected(&mut app, "Japan");
    for _ in 0..3 {
        app.press_key("right").unwrap();
    }
    app.wait_for("role:treeitem[III Kawachi]").unwrap();
    app.press_key("right").unwrap();
    wait_selected(&mut app, "III Kawachi");
    app.press_key("enter").unwrap();
    assert_eq!(wait_pending_ship(&mut app), "PJSB001");
}

#[test]
fn a_press_on_a_ship_opens_it_and_gives_the_tree_the_keys() {
    let mut app = ships_tree();
    app.press("role:treeitem[U.S.A.]").unwrap();
    app.press("role:treeitem[Destroyers]").unwrap();
    app.press("id:armor-ship-PASD001").unwrap();
    assert_eq!(wait_pending_ship(&mut app), "PASD001");
    wait_selected(&mut app, "II Sampson");

    app.press_key("down").unwrap();
    wait_selected(&mut app, "III Wickes");
}

#[test]
fn a_ship_opened_elsewhere_becomes_the_selected_row() {
    let mut app = ships_tree();
    app.press("role:treeitem[Japan]").unwrap();
    app.press("role:treeitem[Battleships]").unwrap();
    app.press("role:treeitem[U.S.A.]").unwrap();
    wait_selected(&mut app, "U.S.A.");

    app.press("id:fixture-show-kawachi").unwrap();
    wait_selected(&mut app, "III Kawachi");
}
