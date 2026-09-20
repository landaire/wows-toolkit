//! UI interaction tests for the app shell.
//!
//! These drive real pointer and keyboard events at a headless window through
//! GPUI Kit's test harness and assert on what the frame reports, so the
//! behaviors the egui app has -- clicking a tab selects it, a disabled
//! control refuses a click, the debug shortcut shows the debug strip -- are
//! checked rather than assumed. Element state is read back through
//! `ElementSnapshot`, so a test fails when a control stops being clickable,
//! not only when its backing field stops changing.

use gpui_kit::AppContext;
use gpui_kit::TestAppContext;
use gpui_kit::WindowHandle;
use gpui_kit::px;
use gpui_kit::size;
use gpui_kit::test::TestAppContextExt;
use gpui_kit::test::TestWindowExt;
use std::path::PathBuf;
use std::time::Duration;

use wows_toolkit_config::ReplaySettings;

use wows_toolkit_config::ReplayGrouping;

use crate::app::App;
use crate::app::AppTab;
use crate::settings::DEFAULT_ZOOM;
use crate::settings::GpuiSettings;

/// The app tab bar's element id (`App::render`). Tabs inside it are addressed
/// by index, matching `AppTab::ALL`'s order.
const TAB_BAR: &str = "app-tabs";

/// The debug-build strip shown at the bottom while debug mode is on.
const DEBUG_NOTICE: &str = "app-debug-notice";

/// Replay Inspector header controls (`replay_inspector::view::Render`).
const AUTOLOAD: &str = "replay-header-auto-load-latest";
const COLUMN_FILTERS_TRIGGER: &str = "replay-header-column-filters-trigger";
const FILTER_RAW_XP: &str = "replay-header-filter-raw-xp";
const FILTER_HEALS: &str = "replay-header-filter-heals";
const GROUPING: &str = "replay-header-grouping";

/// Armor Viewer sidebar ship search (`armor_viewer::sidebar`).
const SHIP_SEARCH: &str = "armor-sidebar-search";

/// Opens the real root view in a headless window sized like the app's own
/// default, with the component layer initialized.
fn open_app(cx: &mut TestAppContext) -> WindowHandle<App> {
    cx.update(gpui_kit::init);
    cx.open_window(size(px(1200.), px(800.)), App::new)
}

fn tab_index(tab: AppTab) -> usize {
    AppTab::ALL.iter().position(|t| *t == tab).expect("AppTab::ALL covers every tab")
}

#[gpui_kit::test]
fn the_settings_tab_is_selected_at_startup(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let tabs = window.within(TAB_BAR);
        assert_eq!(tabs.find(tab_index(AppTab::Settings)).selected(), Some(true));
        assert_eq!(tabs.find(tab_index(AppTab::ReplayInspector)).selected(), Some(false));
        assert_eq!(tabs.find(tab_index(AppTab::ArmorViewer)).selected(), Some(false));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn clicking_a_tab_moves_the_selection_to_it(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);

        for tab in [AppTab::ReplayInspector, AppTab::ArmorViewer, AppTab::Settings] {
            window.within(TAB_BAR).click(tab_index(tab), cx);
            for other in AppTab::ALL {
                let selected = window.within(TAB_BAR).find(tab_index(other)).selected();
                assert_eq!(
                    selected,
                    Some(other == tab),
                    "after clicking {:?}, {:?} selection",
                    tab.label(),
                    other.label()
                );
            }
        }
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn hovering_a_tab_leaves_the_selection_alone(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.within(TAB_BAR).hover(tab_index(AppTab::ArmorViewer), cx);

        let tabs = window.within(TAB_BAR);
        assert_eq!(tabs.find(tab_index(AppTab::Settings)).selected(), Some(true), "hover must not select");
        assert_eq!(tabs.find(tab_index(AppTab::ArmorViewer)).selected(), Some(false));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_debug_shortcut_toggles_the_debug_strip(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(DEBUG_NOTICE).is_none(), "the debug strip is hidden until the shortcut fires");

        window.press("ctrl-shift-d", cx);
        assert!(window.try_find(DEBUG_NOTICE).is_some(), "ctrl-shift-d shows the debug strip");

        window.press("ctrl-shift-d", cx);
        assert!(window.try_find(DEBUG_NOTICE).is_none(), "the shortcut toggles rather than latching");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn an_unmodified_d_does_not_toggle_the_debug_strip(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.press("d", cx);
        window.press("ctrl-d", cx);
        window.press("shift-d", cx);
        assert!(window.try_find(DEBUG_NOTICE).is_none(), "only ctrl-shift-d is the debug shortcut");

        // Positive control: absence above has to mean hidden, not unobservable.
        window.press("ctrl-shift-d", cx);
        assert!(window.try_find(DEBUG_NOTICE).is_some(), "the strip is observable when shown");
    })
    .expect("the test window stays open");
}

/// Switches to `tab` and renders, so the tab's own controls are in the frame.
fn show_tab(window: &mut gpui_kit::Window, tab: AppTab, cx: &mut gpui_kit::App) {
    window.render_frame(cx);
    window.within(TAB_BAR).click(tab_index(tab), cx);
}

/// `Select` commits through `defer_in` after the dispatch callback returns, so
/// the assertion on the committed value has to wait for the executor.
#[gpui_kit::test]
async fn the_grouping_combo_opens_and_applies_a_choice(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);

        assert_eq!(window.find(GROUPING).value(), Some("Group: Date"), "Date is the default grouping");
        assert_eq!(window.find(GROUPING).expanded(), Some(false));

        window.within(GROUPING).click("input", cx);
        assert_eq!(window.find(GROUPING).expanded(), Some(true), "clicking the combo opens its menu");

        window.press("escape", cx);
        assert_eq!(window.find(GROUPING).expanded(), Some(false), "escape closes it without choosing");
        assert_eq!(window.find(GROUPING).value(), Some("Group: Date"));

        window.within(GROUPING).click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .expect("the test window stays open");

    cx.wait_for(window.into(), Duration::from_millis(500), |window, _| {
        window.find(GROUPING).expanded() == Some(false) && window.find(GROUPING).value() == Some("Group: Ship")
    })
    .await;

    // The combo's own value is `SelectState`'s, so it would read "Group: Ship"
    // even with nothing wired to the browser. Assert the browser applied it.
    window
        .update(cx, |app, _window, cx| {
            assert_eq!(app.replay_inspector().read(cx).grouping(cx), ReplayGrouping::Ship);
        })
        .expect("the test window stays open");
}

#[gpui_kit::test]
fn clicking_the_autoload_checkbox_toggles_it(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);

        let before = window.find(AUTOLOAD).checked().expect("the autoload checkbox reports a checked state");
        window.click(AUTOLOAD, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(!before));
        window.click(AUTOLOAD, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(before), "a second click returns it");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_column_filters_popover_opens_and_toggles_only_the_column_clicked(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);

        assert!(window.try_find(FILTER_RAW_XP).is_none(), "the filters live behind the dropdown");
        window.click(COLUMN_FILTERS_TRIGGER, cx);

        let raw_xp_before = window.find(FILTER_RAW_XP).checked().expect("checkbox reports state");
        let heals_before = window.find(FILTER_HEALS).checked().expect("checkbox reports state");

        window.click(FILTER_RAW_XP, cx);
        assert_eq!(window.find(FILTER_RAW_XP).checked(), Some(!raw_xp_before));
        assert_eq!(window.find(FILTER_HEALS).checked(), Some(heals_before), "the other filters are untouched");
    })
    .expect("the test window stays open");
}

/// A settings snapshot with distinguishable values, so an assertion on a
/// checkbox proves it read the snapshot rather than a coincidental default.
/// The WoWs directory is deliberately empty: `apply_settings` then takes its
/// "directory is not set" branch and starts no directory scan or game-data
/// load, which keeps these tests off the filesystem.
fn test_settings() -> GpuiSettings {
    GpuiSettings {
        zoom: DEFAULT_ZOOM,
        wows_dir: String::new(),
        current_replay_path: PathBuf::new(),
        replay: ReplaySettings {
            show_raw_xp: true,
            show_observed_damage: false,
            grouping: ReplayGrouping::Ship,
            ..ReplaySettings::default()
        },
        debug_mode: false,
        auto_load_latest_replay: false,
        armor_defaults: None,
    }
}

#[gpui_kit::test]
fn the_settings_tab_shows_the_values_it_was_given(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("show-raw-xp").checked(), Some(true));
        assert_eq!(window.find("show-observed-damage").checked(), Some(false));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_read_only_settings_checkboxes_refuse_a_click(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("show-raw-xp", cx);
        window.click("show-observed-damage", cx);
        assert_eq!(window.find("show-raw-xp").checked(), Some(true), "the settings tab is read-only in this port");
        assert_eq!(window.find("show-observed-damage").checked(), Some(false));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_autoload_checkbox_reflects_the_loaded_setting(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(false), "seeded from the settings snapshot");
        window.click(AUTOLOAD, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(true), "and it is still live after seeding");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn typing_in_the_ship_search_records_the_query(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);

        window.click(SHIP_SEARCH, cx);
        window.input("yamato", cx);
        assert_eq!(window.find(SHIP_SEARCH).value(), Some("yamato"));

        window.press("backspace", cx);
        assert_eq!(window.find(SHIP_SEARCH).value(), Some("yamat"), "the search field takes editing keys too");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_loaded_grouping_reaches_the_browser_and_the_combo(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            assert_eq!(app.replay_inspector().read(cx).grouping(cx), ReplayGrouping::Ship, "seeded from settings");
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);
        assert_eq!(window.find(GROUPING).value(), Some("Group: Ship"), "and the combo mirrors it");
    })
    .expect("the test window stays open");
}
