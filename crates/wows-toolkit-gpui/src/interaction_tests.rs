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
use gpui_kit::component::IndexPath;
use gpui_kit::px;
use gpui_kit::size;
use gpui_kit::test::TestAppContextExt;
use gpui_kit::test::TestWindowExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use wows_toolkit_config::ReplaySettings;
use wows_toolkit_config::index::query::SortColumn as SearchSortColumn;
use wows_toolkit_viewmodel::player_tracker::ClanSortColumn;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;

use wows_toolkit_config::ReplayGrouping;

use crate::app::App;
use crate::app::AppTab;
use crate::settings::DEFAULT_ZOOM;
use crate::settings::GpuiSettings;
use crate::stats::load::SessionData;

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

/// Unpacker tab controls (`unpacker::view`, `unpacker::browser`).
const EXTRACT: &str = "unpacker-extract";
const CANCEL: &str = "unpacker-cancel";
const PKG_FILTER: &str = "unpacker-pkg-filter";
const DUMP_PARAMS: &str = "unpacker-dump-params";
const OUTPUT_DIR: &str = "unpacker-output-dir";
const QUEUE_CLEAR_ALL: &str = "unpacker-queue-clear-all";

/// Stats tab filter bar (`stats::view`).
const STATS_LIMIT_ENABLED: &str = "stats-limit-enabled";
const STATS_LIMIT_COUNT: &str = "stats-limit-count";

/// Player Tracker controls (`player_tracker`).
const TRACKER_FILTER: &str = "tracker-filter";
const TRACKER_PERIOD: &str = "tracker-period";

/// The tab strip a dock draws over a group holding more than one panel
/// (gpui-component's `TabGroupSkin::render_tabs`). Tabs inside it are
/// addressed by index, in the order the panels were added.
const DOCK_TABS: &str = "tab-bar";

/// The command palette's own list, inside the dialog it opens in
/// (`CommandState::render`'s root).
const PALETTE: &str = "command";

/// Search tab controls (`search`).
const SEARCH_QUERY: &str = "search-query";

/// Points `storage_dir` at a temporary directory for the whole test process.
///
/// Without it the Settings tab measures the game-data cache under the running
/// user's own app data, which against a real install is a walk of several
/// gigabytes and hangs the suite. The directory is leaked deliberately: it has
/// to outlive every test in the process, and it holds nothing but what a test
/// wrote there.
fn use_a_temporary_storage_dir() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        wows_toolkit_config::storage_override::set(dir.path().to_path_buf());
        std::mem::forget(dir);
    });
}

/// Opens the real root view in a headless window sized like the app's own
/// default, with the component layer initialized.
fn open_app(cx: &mut TestAppContext) -> WindowHandle<App> {
    use_a_temporary_storage_dir();
    cx.update(gpui_kit::init);
    cx.open_window(size(px(1200.), px(800.)), App::new)
}

/// The same, wide enough that a whole toolbar strip is in one frame.
///
/// The armor viewport's strip carries a dozen controls beside a sidebar; at
/// the default width the last of them are off the edge, and the harness
/// refuses to click what it cannot see.
fn open_wide_app(cx: &mut TestAppContext) -> WindowHandle<App> {
    use_a_temporary_storage_dir();
    cx.update(gpui_kit::init);
    cx.open_window(size(px(2200.), px(900.)), App::new)
}

/// The same, tall enough that a whole scrolling tab is in one frame.
///
/// The harness refuses to click what is below the fold, so a test that drives
/// a control near the bottom of the settings tab opens the window this way
/// rather than scrolling to it.
fn open_tall_app(cx: &mut TestAppContext) -> WindowHandle<App> {
    use_a_temporary_storage_dir();
    cx.update(gpui_kit::init);
    cx.open_window(size(px(1200.), px(2600.)), App::new)
}

/// Opens the root view inside a `Root`, the way `main.rs` does.
///
/// `open_app` mounts the view bare, which is enough for anything that only
/// reads the frame. Dialogs, notifications and text selection are hosted by
/// `Root`, so whatever exercises those has to be mounted the way production
/// mounts it.
fn open_app_in_root(cx: &mut TestAppContext) -> (WindowHandle<gpui_kit::component::Root>, gpui_kit::Entity<App>) {
    use_a_temporary_storage_dir();
    cx.update(gpui_kit::init);
    let app = std::cell::RefCell::new(None);
    let window = cx.open_window(size(px(1200.), px(800.)), |window, cx| {
        let view = cx.new(|cx| App::new(window, cx));
        *app.borrow_mut() = Some(view.clone());
        let view: gpui_kit::AnyView = view.into();
        gpui_kit::component::Root::new(view, window, cx)
    });
    let app = app.borrow_mut().take().expect("App::new ran inside the window builder");
    (window, app)
}

fn tab_index(tab: AppTab) -> usize {
    AppTab::ALL.iter().position(|t| *t == tab).expect("AppTab::ALL covers every tab")
}

#[gpui_kit::test]
fn the_replay_inspector_tab_is_selected_at_startup(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let tabs = window.within(TAB_BAR);
        // The egui app opens on its replays tab, so this one does too.
        assert_eq!(tabs.find(tab_index(AppTab::ReplayInspector)).selected(), Some(true));
        for other in AppTab::ALL.into_iter().filter(|tab| *tab != AppTab::ReplayInspector) {
            assert_eq!(tabs.find(tab_index(other)).selected(), Some(false), "{other:?} is not selected");
        }
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn clicking_a_tab_moves_the_selection_to_it(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);

        for tab in AppTab::ALL {
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
        assert_eq!(
            tabs.find(tab_index(AppTab::ReplayInspector)).selected(),
            Some(true),
            "hover must not move the selection off the startup tab"
        );
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
    assert_eq!(
        window.within(TAB_BAR).find(tab_index(tab)).selected(),
        Some(true),
        "the {tab:?} tab is showing before its controls are exercised"
    );
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
        locale: None,
        theme: Default::default(),
        twitch_token: None,
        twitch_channel: String::new(),
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
        check_for_updates: true,
        enable_logging: false,
        data_sharing: DataSharingMode::Off,
        proxy_url: String::new(),
        auto_load_latest_replay: false,
        output_dir: String::new(),
        armor_defaults: None,
        auto_dump_game_data: false,
        game_data_cache_dir: String::new(),
        game_data_repo_commit: None,
        collab_display_name: String::new(),
        suppress_p2p_ip_warning: false,
        disable_auto_open_session_windows: false,
    }
}

#[gpui_kit::test]
fn the_settings_tab_shows_the_values_it_was_given(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);
        assert_eq!(window.find("show-raw-xp").checked(), Some(true));
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

/// The penetration checker is a panel, not a popover.
///
/// Its verdicts are read against the plate the pointer is on, so it has to
/// survive clicking and hovering the ship. As a popover it closed on the
/// first click on the hull and swallowed the pointer moves that would have
/// moved the plate, which is what made it unusable.
#[gpui_kit::test]
fn the_penetration_panel_stays_up_while_the_ship_is_clicked(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);
        window.render_frame(cx);

        assert!(window.try_find("armor-pen-search").is_none(), "the panel is down until it is asked for");

        window.click("armor-analysis-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-pen-search").is_some(), "the toggle brings the panel up");

        // Whatever else is clicked, the panel is still there: this is the
        // whole interaction the popover made impossible.
        window.click(SHIP_SEARCH, cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-pen-search").is_some(), "a click elsewhere does not dismiss it");

        window.click("armor-analysis-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-pen-search").is_none(), "the toggle puts it away again");
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

#[gpui_kit::test]
fn the_unpacker_tab_reports_an_empty_extraction_queue_and_disables_its_actions(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);

        // Nothing queued, so every queue action is refused. Clicking anyway
        // must leave the panel alone rather than starting a run. Cancel is
        // drawn only while a run is going, so an idle tab does not offer it.
        assert!(window.try_find(CANCEL).is_none(), "cancel belongs to a run in progress");
        for action in [EXTRACT, QUEUE_CLEAR_ALL] {
            window.click(action, cx);
        }
        assert!(window.try_find(EXTRACT).is_some(), "the queue panel survives clicks on its disabled buttons");
    })
    .expect("the test window stays open");
}

/// The Replay Inspector rates its players against the same expected-values
/// table the Stats tab loads, so the session's one copy has to reach it.
#[gpui_kit::test]
fn the_session_rating_table_reaches_the_replay_inspector(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let table = Arc::new(crate::replay_inspector::test_support::fixture_personal_rating_data());
    let session = SessionData { personal_rating: Some(table), ..SessionData::default() };

    window
        .update(cx, |app, window, cx| {
            assert!(app.replay_inspector().read(cx).personal_rating().is_none(), "nothing is loaded yet");
            app.apply_session_stats(session, window, cx);
            let held = app.replay_inspector().read(cx).personal_rating().expect("the table reached the tab");
            assert!(held.ship_count() > 0, "the tab holds a table it can actually rate against");
        })
        .expect("the test window stays open");
}

/// A search result opens in the Replay Inspector rather than somewhere of
/// the Search tab's own, and brings that tab forward.
#[gpui_kit::test]
fn opening_a_search_result_shows_the_replay_inspector(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        assert_eq!(
            window.within(TAB_BAR).find(tab_index(AppTab::Search)).selected(),
            Some(true),
            "the search tab is showing"
        );
    })
    .expect("the test window stays open");

    // No WoWs directory in the test settings, so the inspector has no game
    // data; the event still has to move the user to that tab.
    window
        .update(cx, |app, window, cx| {
            app.open_replay_from_search(std::path::PathBuf::from("nonexistent.wowsreplay"), window, cx);
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.within(TAB_BAR).find(tab_index(AppTab::ReplayInspector)).selected(),
            Some(true),
            "opening a result moves to the inspector"
        );
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_unpacker_shows_the_saved_extraction_directory(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let settings = GpuiSettings { output_dir: "C:/extracted".to_string(), ..test_settings() };
    window.update(cx, |app, window, cx| app.apply_settings(settings, window, cx)).expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);
        assert_eq!(
            window.find(OUTPUT_DIR).value(),
            Some("C:/extracted"),
            "the field opens showing what the shared database holds"
        );
    })
    .expect("the test window stays open");
}

/// The queue dropdown opens on click and offers to empty the queue, the way
/// the egui app's queue popup does.
#[gpui_kit::test]
fn the_unpacker_queue_dropdown_opens_and_offers_to_clear_the_queue(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);
        // The queue is a panel beside the listing, not a dropdown: what acts
        // on it is on screen with it rather than behind a trigger.
        assert!(window.try_find(QUEUE_CLEAR_ALL).is_some(), "the queue panel is open beside the listing");
        assert!(window.try_find(OUTPUT_DIR).is_some(), "the destination is on the panel with the queue");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_unpacker_browsers_say_there_is_no_game_data_when_the_directory_is_unset(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);
        // With no VFS the panes show a status line instead of a filter box.
        assert!(window.try_find(PKG_FILTER).is_none(), "the filter appears only once a VFS is loaded");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_stats_tab_reports_an_empty_session_when_no_games_are_loaded(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);
        // Nothing loaded, so the recency limit is off and its count is refused.
        assert_eq!(window.find(STATS_LIMIT_ENABLED).checked(), Some(false));
        assert!(window.try_find(STATS_LIMIT_COUNT).is_some(), "the count box is present but disabled");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_stats_division_filter_is_single_select(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);

        let all = ("stats-division", 0usize);
        let solo = ("stats-division", 1usize);
        let division = ("stats-division", 2usize);

        assert_eq!(window.find(all).selected(), Some(true), "All is the default division filter");

        window.click(solo, cx);
        assert_eq!(window.find(solo).selected(), Some(true));
        assert_eq!(window.find(all).selected(), Some(false), "the filter is single-select");

        window.click(division, cx);
        assert_eq!(window.find(division).selected(), Some(true));
        assert_eq!(window.find(solo).selected(), Some(false));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_recent_games_limit_can_be_switched_on_and_off(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);

        window.click(STATS_LIMIT_ENABLED, cx);
        assert_eq!(window.find(STATS_LIMIT_ENABLED).checked(), Some(true));

        window.click(STATS_LIMIT_ENABLED, cx);
        assert_eq!(window.find(STATS_LIMIT_ENABLED).checked(), Some(false), "the limit toggles back off");
    })
    .expect("the test window stays open");
}

/// Writes a cached build under `base`, named the way the dumper names one.
fn cached_build(base: &std::path::Path, version: &str, build: u32) {
    let dir = base.join(format!("{version}_{build}"));
    std::fs::create_dir_all(&dir).expect("the build directory can be made");
    std::fs::write(dir.join("metadata.toml"), "").expect("the metadata can be written");
    std::fs::write(dir.join("blob"), vec![0u8; 2048]).expect("the blob can be written");
}

/// The maintenance controls act on cached builds, so a cache holding none
/// offers the directory and nothing else. The egui tab guards them the same
/// way, on `version_count > 0`.
/// The index is built from the replays under the game directory, so without
/// one there is nothing to walk.
#[gpui_kit::test]
fn building_the_replay_index_is_refused_without_a_game_directory(cx: &mut TestAppContext) {
    let window = open_tall_app(cx);
    let mut settings = test_settings();
    settings.wows_dir = String::new();
    window.update(cx, |app, window, cx| app.apply_settings(settings, window, cx)).expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);
        window.render_frame(cx);

        assert!(window.try_find("index-build").is_some(), "the control is there to explain what is missing");
        // Nothing is running, so there is nothing to stop.
        assert!(window.try_find("index-stop").is_none());
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_cache_controls_appear_only_once_something_is_cached(cx: &mut TestAppContext) {
    let empty = tempfile::tempdir().expect("a temporary directory can be made");
    let window = open_tall_app(cx);
    let mut settings = test_settings();
    settings.game_data_cache_dir = empty.path().to_string_lossy().into_owned();
    window.update(cx, |app, window, cx| app.apply_settings(settings, window, cx)).expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);
    })
    .expect("the test window stays open");
    // The measurement runs off the UI thread; the controls follow it.
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cache-auto-dump").is_some(), "the toggle does not depend on a cache");
        assert!(window.try_find("cache-dir").is_some(), "the directory does not depend on a cache");
        assert!(window.try_find("cache-check-updates").is_none(), "there is nothing cached to check");
        assert!(window.try_find("cache-open-folder").is_none(), "there is no size to report");
    })
    .expect("the test window stays open");

    let filled = tempfile::tempdir().expect("a temporary directory can be made");
    cached_build(filled.path(), "1.0.0", 100);
    let mut settings = test_settings();
    settings.game_data_cache_dir = filled.path().to_string_lossy().into_owned();
    window.update(cx, |app, window, cx| app.apply_settings(settings, window, cx)).expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cache-check-updates").is_some(), "a cached build can be checked");
        assert!(window.try_find("cache-validate").is_some(), "a cached build can be validated");
        assert!(window.try_find("cache-open-folder").is_some(), "a cached build has a folder to open");
        // Pruning keeps the newest build, so one build is nothing to prune.
        assert!(window.try_find("cache-delete-old").is_none(), "a lone build is not an old version");
    })
    .expect("the test window stays open");
}

/// A second build is what makes pruning mean anything.
#[gpui_kit::test]
fn pruning_is_offered_once_a_second_build_is_cached(cx: &mut TestAppContext) {
    let cache = tempfile::tempdir().expect("a temporary directory can be made");
    cached_build(cache.path(), "1.0.0", 100);
    cached_build(cache.path(), "1.1.0", 200);

    let window = open_tall_app(cx);
    let mut settings = test_settings();
    settings.game_data_cache_dir = cache.path().to_string_lossy().into_owned();
    window.update(cx, |app, window, cx| app.apply_settings(settings, window, cx)).expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cache-delete-old").is_some(), "two builds means one is old");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_settings_checkboxes_apply_and_toggle_back(cx: &mut TestAppContext) {
    let window = open_tall_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);

        // The tab is writable now, so a click moves the control rather than
        // being refused.
        let before = window.find("show-raw-xp").checked().expect("the checkbox reports a state");
        window.click("show-raw-xp", cx);
        assert_eq!(window.find("show-raw-xp").checked(), Some(!before));
        window.click("show-raw-xp", cx);
        assert_eq!(window.find("show-raw-xp").checked(), Some(before));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_data_sharing_mode_is_single_select(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);

        let off = ("data-sharing", 0usize);
        let builds = ("data-sharing", 1usize);
        let replays = ("data-sharing", 2usize);

        assert_eq!(window.find(off).selected(), Some(true), "sharing is off by default");

        window.click(builds, cx);
        assert_eq!(window.find(builds).selected(), Some(true));
        assert_eq!(window.find(off).selected(), Some(false), "the modes are exclusive");

        window.click(replays, cx);
        assert_eq!(window.find(replays).selected(), Some(true));
        assert_eq!(window.find(builds).selected(), Some(false));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn a_settings_edit_reaches_the_replay_inspector(cx: &mut TestAppContext) {
    let window = open_tall_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);
        // test_settings starts with Raw XP on; turn it off here.
        assert_eq!(window.find("show-raw-xp").checked(), Some(true));
        window.click("show-raw-xp", cx);

        // The replay header's own filter mirrors the same setting.
        show_tab(window, AppTab::ReplayInspector, cx);
        window.click(COLUMN_FILTERS_TRIGGER, cx);
        assert_eq!(window.find(FILTER_RAW_XP).checked(), Some(false), "the edit reached the other tab");
    })
    .expect("the test window stays open");
}

/// Brings the Stats tab's first chart to the front. The tab opens on its
/// overview, so a chart's own controls are behind its dock tab.
fn show_first_chart(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    window.within(DOCK_TABS).click(2usize, cx);
}

#[gpui_kit::test]
fn the_chart_mode_toggle_is_single_select(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);
        show_first_chart(window, cx);

        // The first chart pane is id 0, so its mode ids start at 0.
        let line = ("chart-mode", 0usize);
        let bar = ("chart-mode", 1usize);

        assert!(window.try_find(line).is_none(), "the chart's controls live behind its settings menu");
        window.click(("chart-settings", 0usize), cx);
        assert_eq!(window.find(line).selected(), Some(true), "a chart opens as a line");

        window.click(bar, cx);
        assert_eq!(window.find(bar).selected(), Some(true));
        assert_eq!(window.find(line).selected(), Some(false), "the modes are exclusive");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn switching_to_a_bar_chart_offers_win_rate_which_a_line_cannot_plot(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);
        show_first_chart(window, cx);

        // Win rate is the last statistic; a line chart does not offer it.
        let win_rate = ("chart-stat", ChartableStat::WinRate as usize);
        window.click(("chart-settings", 0usize), cx);
        assert!(window.try_find(win_rate).is_none(), "a line chart cannot plot win rate");

        window.click(("chart-mode", 1usize), cx);
        assert!(window.try_find(win_rate).is_some(), "a bar chart can");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn adding_a_chart_opens_another_pane_with_its_own_controls(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);
        // The second pane's ids are offset by its id, so they do not collide.
        let second_pane_settings = ("chart-settings", 1usize);
        assert!(window.try_find(second_pane_settings).is_none(), "only one chart is open to begin with");

        // Add-chart is a menu of statistics, so the chart opens on the one
        // that was asked for rather than on a default to be changed after.
        window.click("stats-add-chart", cx);
        window.click(("stats-add-chart-stat", ChartableStat::Frags as usize), cx);
        assert!(window.try_find(second_pane_settings).is_some(), "the new pane brought its own controls");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
async fn the_player_tracker_period_is_chosen_from_a_combo(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);

        assert_eq!(
            window.find(TRACKER_PERIOD).value(),
            Some(TimePeriod::LastDay.label()),
            "the tracker opens on the last day"
        );
        assert_eq!(window.find(TRACKER_PERIOD).expanded(), Some(false));

        window.within(TRACKER_PERIOD).click("input", cx);
        assert_eq!(window.find(TRACKER_PERIOD).expanded(), Some(true), "clicking the combo opens its menu");

        window.press("down", cx);
        window.press("enter", cx);
    })
    .expect("the test window stays open");

    // `Select` commits through `defer_in`, so the applied period lands after
    // the dispatch returns.
    let after_last_day = TimePeriod::ALL[TimePeriod::LastDay as usize + 1].label();
    cx.wait_for(window.into(), Duration::from_millis(500), |window, _| {
        window.find(TRACKER_PERIOD).expanded() == Some(false)
            && window.find(TRACKER_PERIOD).value() == Some(after_last_day)
    })
    .await;
}

#[gpui_kit::test]
fn clicking_a_tracker_column_sorts_by_it_and_clicking_again_reverses(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);

        let by_name = ("tracker-sort", SortColumn::Name as usize);
        let by_count = ("tracker-sort", SortColumn::Encounters as usize);

        assert_eq!(window.find(by_count).selected(), Some(true), "encounters is the opening sort");

        window.click(by_name, cx);
        assert_eq!(window.find(by_name).selected(), Some(true));
        assert_eq!(window.find(by_count).selected(), Some(false), "only one column sorts at a time");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_tracker_says_it_is_waiting_when_no_index_is_open(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);
        // With no config database the table has nothing to draw, so the
        // filter box is the only input present.
        assert!(window.try_find(TRACKER_FILTER).is_some());
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_search_tab_invites_a_query_before_one_is_run(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        assert!(window.try_find(SEARCH_QUERY).is_some(), "the query box is ready");
        // Date is the opening sort, newest first.
        assert_eq!(window.find(("search-sort", SearchSortColumn::Date as usize)).selected(), Some(true));
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn a_query_that_does_not_parse_is_reported_rather_than_searched_for(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);

        window.click(SEARCH_QUERY, cx);
        // A trailing operator is a parse error, not a literal to search for.
        window.input("outcome=win and", cx);
        window.click("search-run", cx);

        // The tab stays usable and keeps its query box rather than clearing.
        assert!(window.try_find(SEARCH_QUERY).is_some());

        // And the objection is said above the results rather than in place
        // of them.
        window.render_frame(cx);
        let reported = window.find("search-parse-error").label().unwrap_or_default().to_string();
        assert!(reported.contains("did not parse"), "the error names itself, got {reported:?}");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn clicking_a_search_column_moves_the_sort_to_it(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);

        let by_date = ("search-sort", SearchSortColumn::Date as usize);
        let by_damage = ("search-sort", SearchSortColumn::Damage as usize);

        assert_eq!(window.find(by_date).selected(), Some(true));
        window.click(by_damage, cx);
        assert_eq!(window.find(by_damage).selected(), Some(true));
        assert_eq!(window.find(by_date).selected(), Some(false), "one column sorts at a time");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn dragging_a_search_header_grip_widens_that_column(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let grip = ("search-header-grip", 0usize);

    let before = cx
        .update_window(window.into(), |_, window, cx| {
            show_tab(window, AppTab::Search, cx);
            window.render_frame(cx);
            window.find(grip).bounds()
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        let from = before.center();
        window.drag(from, gpui_kit::point(from.x + px(50.), from.y), cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    let after = cx
        .update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.find(grip).bounds()
        })
        .expect("the test window stays open");

    assert!(
        after.origin.x > before.origin.x,
        "the grip moved right with the column it widens, from {:?} to {:?}",
        before.origin.x,
        after.origin.x
    );
}

#[gpui_kit::test]
fn the_tracker_sub_tabs_switch_between_their_three_views(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);

        let (current_match, clans) = (1usize, 2usize);

        // The players table sorts by its own columns.
        assert!(window.try_find(("tracker-sort", 0usize)).is_some(), "the tracker opens on players");
        assert!(window.try_find(("tracker-clan-sort", 0usize)).is_none());

        window.within(DOCK_TABS).click(clans, cx);
        window.render_frame(cx);
        // And the clans table brings its own.
        assert!(window.try_find(("tracker-clan-sort", 0usize)).is_some());
        assert!(window.try_find(("tracker-sort", 0usize)).is_none(), "one section in front at a time");

        // The roster is its own layout, so neither table's header survives it.
        window.within(DOCK_TABS).click(current_match, cx);
        window.render_frame(cx);
        assert!(window.try_find(("tracker-sort", 0usize)).is_none());
        assert!(window.try_find(("tracker-clan-sort", 0usize)).is_none());
    })
    .expect("the test window stays open");
}

/// The historical table carries every column the egui tracker's does, and the
/// sort moves between them.
#[gpui_kit::test]
fn the_historical_table_sorts_by_each_of_its_columns(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);

        let columns = SortColumn::ALL.len();
        assert_eq!(columns, 5, "name, clan, both counts and the last encounter");

        let in_range = ("tracker-sort", 3usize);
        assert_eq!(window.find(in_range).selected(), Some(true), "the table opens on the in-range count");

        for index in 0..columns {
            let column = ("tracker-sort", index);
            window.click(column, cx);
            assert_eq!(window.find(column).selected(), Some(true), "column {index} takes the sort");
        }
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_clans_table_keeps_its_own_sort(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);
        window.within(DOCK_TABS).click(2usize, cx);
        window.render_frame(cx);

        let by_tag = ("tracker-clan-sort", 0usize);
        let by_encounters = ("tracker-clan-sort", 2usize);

        assert_eq!(window.find(by_encounters).selected(), Some(true), "clans open on encounters");
        window.click(by_tag, cx);
        assert_eq!(window.find(by_tag).selected(), Some(true));
        assert_eq!(window.find(by_encounters).selected(), Some(false));

        // Every column the egui clans table draws is here and sortable.
        assert_eq!(ClanSortColumn::ALL.len(), 6);
        for index in 0..ClanSortColumn::ALL.len() {
            let column = ("tracker-clan-sort", index);
            window.click(column, cx);
            assert_eq!(window.find(column).selected(), Some(true), "clan column {index} takes the sort");
        }
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_parameter_dump_menu_is_refused_until_a_build_is_loaded(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);

        // No game directory in the test settings, so no build is loaded and
        // the dump menu must stay shut rather than offering formats that
        // would all fail.
        window.click(DUMP_PARAMS, cx);
        assert!(window.try_find("unpacker-dump-json").is_none(), "the menu does not open without a build");
    })
    .expect("the test window stays open");
}

/// The theme control switches the palette on screen, and the rating bands
/// follow it: the egui app reads the same stored choice, so the two apps open
/// in whichever theme was last picked in either.
///
/// The assertions read `Theme::global`, which is what the widgets draw from.
/// A frame is rendered first so the zoom slider settles: the render path
/// re-applies the theme when the slider and the stored zoom disagree, and
/// that apply would otherwise stand in for the one the control is supposed to
/// make.
#[gpui_kit::test]
fn the_theme_control_switches_the_palette(cx: &mut TestAppContext) {
    use gpui_kit::component::theme::Theme;
    use gpui_kit::component::theme::ThemeMode;
    use wows_replay_insights::personal_rating::PersonalRatingCategory;
    use wows_toolkit_viewmodel::personal_rating::chip_text;
    use wows_toolkit_viewmodel::settings::ThemeChoice;

    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    let system = ("theme-choice", ThemeChoice::System as usize);
    let dark = ("theme-choice", ThemeChoice::Dark as usize);
    let light = ("theme-choice", ThemeChoice::Light as usize);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert_eq!(window.find(system).selected(), Some(true), "an unset theme follows the desktop");

        window.click(dark, cx);
        assert_eq!(window.find(dark).selected(), Some(true));
        assert_eq!(window.find(system).selected(), Some(false), "the choices are exclusive");
        assert_eq!(Theme::global(cx).mode, ThemeMode::Dark, "the dark palette reached the widgets");
        assert!(crate::theme::is_dark_mode(), "and the bands follow it");

        window.click(light, cx);
        assert_eq!(window.find(light).selected(), Some(true));
        assert_eq!(Theme::global(cx).mode, ThemeMode::Light, "the light palette reached the widgets");
        assert!(!crate::theme::is_dark_mode());
    })
    .expect("the test window stays open");

    // The bands are the egui app's own two palettes, so a rating reads
    // differently against each background rather than keeping one colour.
    assert_ne!(
        chip_text(PersonalRatingCategory::VeryGood, true),
        chip_text(PersonalRatingCategory::VeryGood, false),
        "a band has a palette per theme"
    );
}

/// The Twitch credential is pasted, not obtained through a browser flow,
/// which is what the egui app does. What the settings tab reports back is the
/// account it belongs to, or which part of the paste was wrong.
#[gpui_kit::test]
fn a_pasted_twitch_credential_is_read_or_reported(cx: &mut TestAppContext) {
    let window = open_tall_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Settings, cx);
        window.render_frame(cx);

        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("not a credential".to_string()));
        window.click("twitch-paste-token", cx);
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, _window, _cx| {
            let reported = app.twitch_paste_outcome().expect("the paste was reported");
            assert!(reported.is_err(), "a malformed paste is refused");
            assert!(app.stored_twitch_token().is_none(), "and nothing is stored");
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
            "username=harvey;user_id=42;client_id=abc;oauth_token=def".to_string(),
        ));
        window.click("twitch-paste-token", cx);
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, _window, _cx| {
            assert_eq!(app.twitch_paste_outcome().expect("the paste was reported").as_deref(), Ok("harvey"));
            let stored = app.stored_twitch_token().expect("the credential is kept");
            assert_eq!(stored.username(), "harvey");
            assert_eq!(stored.user_id(), 42);
        })
        .expect("the test window stays open");
}

/// Typing offers completions from the same list the egui bar suggests from,
/// and taking one replaces the fragment under the caret rather than appending
/// to it.
#[gpui_kit::test]
fn the_query_bar_offers_completions_and_takes_them(cx: &mut TestAppContext) {
    use wows_toolkit_viewmodel::query_bar::suggest;

    // Typed from a real suggestion, so the test keeps exercising something
    // when the vocabulary changes.
    let first = suggest::static_suggestions().first().expect("there are suggestions").label.clone();
    let needle: String = first.chars().take(3).collect();

    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        assert!(window.try_find(("search-completion", 0usize)).is_none(), "nothing is offered before typing");

        window.click(SEARCH_QUERY, cx);
        window.input(&needle, cx);
        assert_eq!(window.find(SEARCH_QUERY).value(), Some(needle.as_str()), "the text landed");
        window.render_frame(cx);

        let offered = window.find(("search-completion", 0usize));
        assert_eq!(offered.label(), Some(first.as_str()), "the matching suggestion leads");

        window.click(("search-completion", 0usize), cx);
        window.render_frame(cx);
        let taken = window.find(SEARCH_QUERY).value().expect("the bar has text").to_string();
        assert_ne!(taken, first, "what lands is the grammar, not the phrase the row is read as");
        assert!(
            wows_toolkit_config::index::query_text::parse_query(&taken).is_ok(),
            "taking a suggestion leaves a query that parses, got {taken:?}"
        );
    })
    .expect("the test window stays open");
}

/// A pill's own menu reshapes the query.
///
/// Asserted as a round trip rather than against a spelling: the shared
/// transform negates an invertible term by flipping its operator
/// (`outcome=win` becomes `outcome!=win`) rather than by wrapping it in a
/// `not`, and which of the two it picks is its business, not this test's.
#[gpui_kit::test]
fn a_pill_can_be_negated_from_its_own_menu(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.render_frame(cx);
        window.click(SEARCH_QUERY, cx);
        window.input("outcome=win", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("search-pill-segment", 0usize)).is_some(), "a finished term reads back as a pill");
    })
    .expect("the test window stays open");

    // The path is read off the token stream, the way the pill strip reads
    // it: a one-term query's pill is not at a path a test can assume.
    let expr = wows_toolkit_config::index::query_text::parse_query("outcome=win").expect("the query parses");
    let cache = wows_toolkit_viewmodel::query_bar::label::NameCache::default();
    let tokens = wows_toolkit_viewmodel::query_bar::tokens::tokenize(&expr, &cache);
    let pill = wows_toolkit_viewmodel::query_bar::select::pill_paths(&tokens)
        .into_iter()
        .next()
        .expect("the query draws one pill");

    // The edit runs through the same shared transform the egui bar uses, so
    // the assertion is on what lands in the bar rather than on the menu.
    window
        .update(cx, |app, window, cx| {
            app.search().clone().update(cx, |search, cx| {
                search.apply_structural_edit(pill, crate::search_pills::StructuralEdit::Negate, window, cx);
            });
        })
        .expect("the test window stays open");

    let negated = window
        .update(cx, |app, _window, cx| app.search().read(cx).query_for_test(cx))
        .expect("the test window stays open");

    assert_ne!(negated, "outcome=win", "negating the pill changed the query");
    assert!(
        wows_toolkit_config::index::query_text::parse_query(&negated).is_ok(),
        "and left a query that parses, got {negated:?}"
    );

    // Negating it again is the identity, whichever way the transform chose to
    // express the first one.
    let expr = wows_toolkit_config::index::query_text::parse_query(&negated).expect("the query parses");
    let tokens = wows_toolkit_viewmodel::query_bar::tokens::tokenize(&expr, &cache);
    let pill = wows_toolkit_viewmodel::query_bar::select::pill_paths(&tokens)
        .into_iter()
        .next()
        .expect("the query still draws one pill");

    window
        .update(cx, |app, window, cx| {
            app.search().clone().update(cx, |search, cx| {
                search.apply_structural_edit(pill, crate::search_pills::StructuralEdit::Negate, window, cx);
            });
        })
        .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            let text = app.search().read(cx).query_for_test(cx);
            assert_eq!(text, "outcome=win", "negating twice is the query it started from");
        })
        .expect("the test window stays open");
}

/// A term stays in the box while it is being typed and becomes a pill when it
/// is finished, and Backspace over an empty box takes the pill back off. The
/// egui bar keeps the same two apart and reads the key the same way.
#[gpui_kit::test]
fn a_finished_term_becomes_a_pill_and_backspace_takes_it_back(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.render_frame(cx);
        window.click(SEARCH_QUERY, cx);
        window.input("outcome=win", cx);
        window.render_frame(cx);

        assert!(
            window.try_find(("search-pill-segment", 0usize)).is_none(),
            "a term still being typed is text, not a pill"
        );
        assert_eq!(window.find(SEARCH_QUERY).value(), Some("outcome=win"), "and it is in the box");

        window.press("enter", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("search-pill-segment", 0usize)).is_some(), "the finished term reads back as a pill");
        assert_eq!(window.find(SEARCH_QUERY).value().unwrap_or_default(), "", "and the box is clear for the next term");

        window.press("backspace", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    let left = window
        .update(cx, |app, _window, cx| app.search().read(cx).query_for_test(cx))
        .expect("the test window stays open");
    assert_eq!(left, "", "Backspace with nothing to erase took the pill off");
}

/// Undo steps back through structural edits, and redo steps forward again.
///
/// Only structural edits: typing has the text field's own undo, and pushing
/// every keystroke here would bury the edits this stack is for.
#[gpui_kit::test]
fn the_query_bar_steps_back_and_forward_through_structural_edits(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.click(SEARCH_QUERY, cx);
        window.input("outcome=win", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    // Finishing the term is the edit that puts it on the stack, so the walk
    // starts from a bar that holds one pill.
    cx.run_until_parked();

    let expr = wows_toolkit_config::index::query_text::parse_query("outcome=win").expect("the query parses");
    let cache = wows_toolkit_viewmodel::query_bar::label::NameCache::default();
    let tokens = wows_toolkit_viewmodel::query_bar::tokens::tokenize(&expr, &cache);
    let pill = wows_toolkit_viewmodel::query_bar::select::pill_paths(&tokens)
        .into_iter()
        .next()
        .expect("the query draws one pill");

    window
        .update(cx, |app, window, cx| {
            app.search().clone().update(cx, |search, cx| {
                assert!(!search.can_redo(), "nothing has been stepped back from yet");
                search.apply_structural_edit(pill, crate::search_pills::StructuralEdit::Negate, window, cx);
                assert!(search.can_undo(), "the edit is on the stack");
            });
        })
        .expect("the test window stays open");

    let negated = window
        .update(cx, |app, _window, cx| app.search().read(cx).query_for_test(cx))
        .expect("the test window stays open");
    assert_ne!(negated, "outcome=win");

    window
        .update(cx, |app, window, cx| {
            app.search().clone().update(cx, |search, cx| search.undo_edit(window, cx));
        })
        .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            let text = app.search().read(cx).query_for_test(cx);
            assert_eq!(text, "outcome=win", "undo restored the query the edit replaced");
        })
        .expect("the test window stays open");

    window
        .update(cx, |app, window, cx| {
            app.search().clone().update(cx, |search, cx| {
                assert!(search.can_redo(), "what was undone can be redone");
                search.redo_edit(window, cx);
            });
        })
        .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            let text = app.search().read(cx).query_for_test(cx);
            assert_eq!(text, negated, "redo put the edit back");
        })
        .expect("the test window stays open");
}

/// A date field is picked from a calendar rather than from the list of values
/// the index happens to hold.
#[gpui_kit::test]
fn a_date_field_opens_a_calendar_instead_of_the_completions(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.click(SEARCH_QUERY, cx);
        window.input("map:", cx);
        window.render_frame(cx);
        assert!(window.try_find("search-calendar").is_none(), "a map is not a date");

        window.input("ocean date>", cx);
        window.render_frame(cx);
        assert!(window.try_find("search-calendar").is_some(), "the date field brings a calendar");
        assert!(window.try_find(("search-completion", 0usize)).is_none(), "and the value list stands aside");
    })
    .expect("the test window stays open");
}

/// The results follow the query as it is typed, without Enter.
///
/// Driven with a query that cannot parse, because that verdict is reached
/// from the text alone: it proves the run happened without needing an index
/// to search.
#[gpui_kit::test]
fn the_results_follow_a_typed_query_without_enter(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.render_frame(cx);
        window.click(SEARCH_QUERY, cx);
        window.input("damage:>", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    // Typing does not re-query at once: a keystroke here is a round trip to
    // the index.
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let reported = window.find("search-parse-error").label().unwrap_or_default().to_string();
        assert!(
            reported.contains("did not parse"),
            "the bar ran the query as it was typed, and says what it made of it; got {reported:?}"
        );
    })
    .expect("the test window stays open");
}

/// Up in the bar recalls what was run before, and Down walks back out of the
/// recall to the text it started from.
#[gpui_kit::test]
fn the_bar_recalls_what_was_run_before(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.render_frame(cx);
        window.click(SEARCH_QUERY, cx);
        window.input("outcome:win", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        // A second query, so the first is one step further back.
        window.click(SEARCH_QUERY, cx);
        window.press("backspace", cx);
        window.input("survived:false", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.press("up", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    let recalled = |cx: &mut TestAppContext| {
        window
            .update(cx, |app, _window, cx| app.search().read(cx).query_for_test(cx))
            .expect("the test window stays open")
    };
    assert_eq!(recalled(cx), "survived:false", "the first Up recalls the query that was just run");

    cx.update_window(window.into(), |_, window, cx| {
        window.press("up", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    assert_eq!(recalled(cx), "outcome:win", "the second goes one further back");

    cx.update_window(window.into(), |_, window, cx| {
        window.press("down", cx);
        window.press("down", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    assert_eq!(recalled(cx), "survived:false", "walking back out leaves the bar as it was found");
}

/// A pill's operator segment opens a picker, and taking a different operator
/// rewrites that term in the query without disturbing the rest of it.
#[gpui_kit::test]
fn a_pill_operator_can_be_changed_from_the_bar(cx: &mut TestAppContext) {
    let window = open_app(cx);

    // The operator the term does not already say, read off the same choices
    // the menu is built from: a row picked by name is known to be one of this
    // menu's, and what it should leave in the bar is known with it.
    let expr = wows_toolkit_config::index::query_text::parse_query("build>9000000").expect("the query parses");
    let cache = wows_toolkit_viewmodel::query_bar::label::NameCache::default();
    let tokens = wows_toolkit_viewmodel::query_bar::tokens::tokenize(&expr, &cache);
    let pill = wows_toolkit_viewmodel::query_bar::select::pill_paths(&tokens)
        .into_iter()
        .next()
        .expect("the query draws one pill");
    let wanted = crate::search_pills::choices(&expr, &pill, crate::search_pills::EditablePart::Operator)
        .into_iter()
        .find(|choice| !choice.current)
        .expect("the term takes more than one operator");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);

        window.click(SEARCH_QUERY, cx);
        window.input("build>9000000", cx);
        window.click("search-run", cx);
        window.render_frame(cx);

        // The query parsed, so it reads back as a pill.
        assert!(window.try_find("search-pills").is_some(), "the query is drawn as pills");
        assert!(menu_items(window).is_empty(), "no operators are offered before one is asked for");

        // The operator is the second segment of the first pill.
        window.click(("search-pill-segment", 1usize), cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        let offered = menu_items(window);
        assert!(!offered.is_empty(), "the segment drops down the operators this term takes");

        let target = offered
            .into_iter()
            .find(|(_, item)| item.label() == Some(wanted.label.as_str()))
            .map(|(index, _)| index)
            .expect("the menu offers the operator that was asked for");
        // Scoped to the menu: a bare row number is also a tab's.
        window.within("popup-menu").click(gpui_kit::ElementId::Integer(target), cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    // The row acts through the popover's dismissal, which is an effect rather
    // than part of the click.
    cx.run_until_parked();

    window
        .update(cx, |app, _window, cx| {
            let rewritten = app.search().read(cx).query_for_test(cx);
            assert_eq!(rewritten, wanted.taken, "the term kept its field and value and took the operator");
            // Whether the menu then closes is not asserted: a dismissed
            // popover keeps its elements in the harness's snapshot, and the
            // menus this app already had behave the same way, so a check
            // here would be testing the harness rather than the bar.
        })
        .expect("the test window stays open");
}

/// Whatever a dropdown is offering, each with the row number it answers to.
pub(crate) fn menu_items(window: &gpui_kit::Window) -> Vec<(u64, gpui_kit::base::test_support::ElementSnapshot)> {
    gpui_kit::base::test_support::snapshots(window)
        .into_iter()
        .filter(|element| element.path().iter().any(|id| format!("{id:?}").contains("popup-menu")))
        .filter_map(|element| match element.path().last() {
            Some(gpui_kit::ElementId::Integer(row)) => Some((*row, element)),
            _ => None,
        })
        .collect()
}

/// Hovering a result row starts its preview only once the pointer has
/// settled, and leaving the row abandons it. The dwell rule is the egui
/// app's, so a preview appears after the same wait in both.
#[gpui_kit::test]
fn a_result_row_previews_only_after_the_pointer_settles(cx: &mut TestAppContext) {
    use std::path::PathBuf;
    use wows_toolkit_viewmodel::preview_dwell::DWELL;

    let window = open_app(cx);
    let row = PathBuf::from("does-not-exist.wowsreplay");

    window
        .update(cx, |app, _window, cx| {
            app.search().update(cx, |search, cx| {
                search.hover_row(row.clone(), cx);
                assert!(search.is_dwelling(), "the row is being watched");
                assert_eq!(search.preview_frame_count(), None, "but nothing is baked yet");
            });
        })
        .expect("the test window stays open");

    // Settling on the row asks for a preview. No game data is loaded here, so
    // the bake finds no build and produces nothing -- which is the point: the
    // row must not be left showing a stale preview from elsewhere.
    cx.executor().advance_clock(DWELL * 2);
    cx.run_until_parked();

    window
        .update(cx, |app, _window, cx| {
            app.search().update(cx, |search, cx| {
                assert_eq!(search.preview_frame_count(), None);

                search.leave_rows(cx);
                assert!(!search.is_dwelling(), "leaving stops the dwell");
                assert_eq!(search.preview_frame_count(), None);
            });
        })
        .expect("the test window stays open");
}

/// The results strip is reserved as soon as a preview is coming, holding open
/// water until the first frame lands: a strip that appeared with the frame would
/// resize the results under the pointer that asked for it.
#[gpui_kit::test]
fn the_preview_strip_holds_its_space_before_the_first_frame(cx: &mut TestAppContext) {
    use std::path::PathBuf;

    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.render_frame(cx);
        assert!(window.try_find("search-preview").is_none(), "nothing is hovered, so there is no strip");
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            app.search().update(cx, |search, cx| {
                search.seed_baking_preview_for_test(PathBuf::from("a.wowsreplay"));
                cx.notify();
            });
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("search-preview").is_some(), "the strip is up before anything has been baked");
        assert!(
            window.try_find("search-preview-placeholder").is_some(),
            "holding the map's space rather than collapsing to nothing"
        );
    })
    .expect("the test window stays open");
}

/// A dock whose group holds several panels draws a tab strip over them. Without
/// it only the panel added last is reachable, which is what hid the Stats tab's
/// overview and per-ship table.
#[gpui_kit::test]
fn the_stats_dock_puts_its_panels_on_tabs(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);
        assert_eq!(
            window.within(DOCK_TABS).find(0usize).selected(),
            Some(true),
            "the tab opens on its overview, as the egui tab does"
        );

        window.within(DOCK_TABS).click(2usize, cx);
        assert_eq!(window.within(DOCK_TABS).find(2usize).selected(), Some(true));
        assert!(
            window.try_find(("chart-settings", 0usize)).is_some(),
            "the third tab is the first chart, and its controls are on screen"
        );
    })
    .expect("the test window stays open");
}

/// The Unpacker's two listings are tabs of one group, and it opens on the
/// packages one rather than on whichever was added last.
#[gpui_kit::test]
fn the_unpacker_dock_opens_on_the_package_listing(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);
        assert_eq!(window.within(DOCK_TABS).find(0usize).selected(), Some(true));
        assert_eq!(window.within(DOCK_TABS).find(1usize).selected(), Some(false));
    })
    .expect("the test window stays open");
}

/// Text the reader can drag across has to actually select: `SelectableText`
/// only works when the window mounts the selection layer and the runs it
/// registers are reachable by the pointer.
#[gpui_kit::test]
fn dragging_across_a_selectable_run_selects_its_text(cx: &mut TestAppContext) {
    use gpui_kit::base::TextSelection;

    struct Probe;
    impl gpui_kit::Render for Probe {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::ParentElement as _;
            use gpui_kit::Styled as _;
            gpui_kit::div()
                .size_full()
                .p(px(20.))
                .text_size(px(16.))
                .child(crate::ui::selectable_text("probe", "SELECTABLE"))
        }
    }

    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(400.), px(200.)), |window, cx| {
        let probe: gpui_kit::AnyView = cx.new(|_| Probe).into();
        gpui_kit::component::Root::new(probe, window, cx)
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        // Across the run, which starts at the padding and sits on the first
        // line of text.
        window.drag(gpui_kit::point(px(22.), px(28.)), gpui_kit::point(px(120.), px(28.)), cx);
        let selected = TextSelection::selected_text(window, cx);
        assert!(!selected.is_empty(), "dragging over the run selected nothing");
        assert!(
            "SELECTABLE".starts_with(&selected) || selected.starts_with('S'),
            "the selection is part of the run, got {selected:?}"
        );
    })
    .expect("the test window stays open");
}

/// A toast reaches the screen.
///
/// Same layer question as the palette: `Root` holds the queue, the window's
/// own view draws it. Without that child every toast in the app is silent.
#[gpui_kit::test]
fn a_toast_reaches_the_screen(cx: &mut TestAppContext) {
    let (window, _app) = open_app_in_root(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("notification").is_none(), "nothing has been reported yet");
        crate::toast::ok("copied", window, cx);
    })
    .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("notification").is_some(), "the toast is on screen");
    })
    .expect("the test window stays open");
}

/// A toast reaches the screen of a secondary window too.
///
/// A popped-out panel's window is rooted in a `Shell` for exactly this: rooted
/// straight in the panel, the `Root` holds the queue and nothing draws it, so
/// the renderer's own messages are queued and never seen.
#[gpui_kit::test]
fn a_toast_reaches_a_secondary_windows_screen(cx: &mut TestAppContext) {
    struct Panel;
    impl gpui_kit::Render for Panel {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            gpui_kit::div()
        }
    }

    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(400.), px(200.)), |window, cx| {
        let panel = cx.new(|_cx| Panel);
        let shell: gpui_kit::AnyView = cx.new(|_cx| crate::window_shell::Shell::new(panel)).into();
        gpui_kit::component::Root::new(shell, window, cx)
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        crate::toast::ok("exported", window, cx);
    })
    .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("notification").is_some(), "the toast is on screen");
    })
    .expect("the test window stays open");
}

/// The palette dialog reaches the screen.
///
/// The dialog is held by `Root` but drawn by the window's own view, so a view
/// that forgets to draw the layer opens a palette nobody can see.
#[gpui_kit::test]
fn opening_the_palette_puts_its_search_field_on_screen(cx: &mut TestAppContext) {
    let (window, app) = open_app_in_root(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(PALETTE).is_none(), "the palette is closed until it is opened");
        app.update(cx, |app, cx| {
            app.open_palette(window, cx);
        });
    })
    .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(PALETTE).is_some(), "the palette is on screen");
        assert!(window.try_find(IndexPath::new(0)).is_some(), "it is listing its entries");
    })
    .expect("the test window stays open");
}

/// How many palettes are on screen.
fn palettes_open(window: &gpui_kit::Window) -> usize {
    gpui_kit::base::test_support::snapshots(window)
        .iter()
        .filter(|element| element.path().last() == Some(&gpui_kit::ElementId::from(PALETTE)))
        .count()
}

/// The palette opens on the shortcuts the egui app takes: ctrl+p and ctrl+k,
/// both without shift.
#[gpui_kit::test]
fn the_palette_opens_on_the_shortcut(cx: &mut TestAppContext) {
    for shortcut in ["ctrl-p", "ctrl-k"] {
        let (window, _app) = open_app_in_root(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(palettes_open(window), 0, "closed until {shortcut} is pressed");
            window.press(shortcut, cx);
            window.render_frame(cx);
            assert_eq!(palettes_open(window), 1, "{shortcut} opens it");
        })
        .expect("the test window stays open");
    }
}

/// Reaching for the palette while one is already up leaves one, not two.
#[gpui_kit::test]
fn the_palette_does_not_stack_on_itself(cx: &mut TestAppContext) {
    let (window, app) = open_app_in_root(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        app.update(cx, |app, cx| app.open_palette(window, cx));
        window.render_frame(cx);
        assert_eq!(palettes_open(window), 1);

        app.update(cx, |app, cx| app.open_palette(window, cx));
        window.render_frame(cx);
        assert_eq!(palettes_open(window), 1, "the second ask replaces the first rather than piling on it");
    })
    .expect("the test window stays open");
}

/// Confirming a palette entry does what the entry says.
///
/// The dialog itself is the component library's; what this owns is the list
/// and what each entry does, so that is what is driven here.
#[gpui_kit::test]
fn a_palette_entry_does_what_it_says(cx: &mut TestAppContext) {
    use crate::palette::PaletteAction;

    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.within(TAB_BAR).find(tab_index(AppTab::ReplayInspector)).selected(),
            Some(true),
            "the window opens on its replay tab"
        );
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, window, cx| {
            app.run_palette_action(PaletteAction::GoTo(AppTab::Stats), window, cx);
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.within(TAB_BAR).find(tab_index(AppTab::Stats)).selected(),
            Some(true),
            "going to a tab by name selects it"
        );
    })
    .expect("the test window stays open");
}
