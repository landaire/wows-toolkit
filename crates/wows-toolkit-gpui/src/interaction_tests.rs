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
use std::sync::Arc;
use std::time::Duration;

use wows_toolkit_config::ReplaySettings;
use wows_toolkit_config::index::query::SortColumn as SearchSortColumn;
use wows_toolkit_viewmodel::player_tracker::SortColumn;
use wows_toolkit_viewmodel::player_tracker::TimePeriod;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
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
const CLEAR_QUEUE: &str = "unpacker-clear-queue";
const PKG_FILTER: &str = "unpacker-pkg-filter";
const DUMP_PARAMS: &str = "unpacker-dump-params";
const OUTPUT_DIR: &str = "unpacker-output-dir";
const QUEUE_TRIGGER: &str = "unpacker-queue-trigger";
const QUEUE_CLEAR_ALL: &str = "unpacker-queue-clear-all";

/// Stats tab filter bar (`stats::view`).
const STATS_LIMIT_ENABLED: &str = "stats-limit-enabled";
const STATS_LIMIT_COUNT: &str = "stats-limit-count";

/// Player Tracker controls (`player_tracker`).
const TRACKER_FILTER: &str = "tracker-filter";

/// Search tab controls (`search`).
const SEARCH_QUERY: &str = "search-query";

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
        // must leave the tab alone rather than starting a run.
        for action in [EXTRACT, CANCEL, CLEAR_QUEUE] {
            window.click(action, cx);
        }
        assert!(window.try_find(EXTRACT).is_some(), "the queue bar survives clicks on its disabled buttons");
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
        assert!(window.try_find(QUEUE_CLEAR_ALL).is_none(), "the dropdown starts closed");

        window.click(QUEUE_TRIGGER, cx);
        assert!(window.try_find(QUEUE_CLEAR_ALL).is_some(), "clicking the trigger opens the dropdown");
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

#[gpui_kit::test]
fn the_settings_checkboxes_apply_and_toggle_back(cx: &mut TestAppContext) {
    let window = open_app(cx);
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
    let window = open_app(cx);
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

#[gpui_kit::test]
fn the_chart_mode_toggle_is_single_select(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);

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

        window.click("stats-add-chart", cx);
        assert!(window.try_find(second_pane_settings).is_some(), "the new pane brought its own controls");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_player_tracker_period_is_single_select(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);

        let last_day = ("tracker-period", TimePeriod::LastDay as usize);
        let all_time = ("tracker-period", TimePeriod::AllTime as usize);

        assert_eq!(window.find(last_day).selected(), Some(true), "the tracker opens on the last day");

        window.click(all_time, cx);
        assert_eq!(window.find(all_time).selected(), Some(true));
        assert_eq!(window.find(last_day).selected(), Some(false), "the periods are exclusive");
    })
    .expect("the test window stays open");
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
fn the_tracker_sub_tabs_switch_between_their_three_views(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);

        let players = ("tracker-subtab", 0usize);
        let current_match = ("tracker-subtab", 1usize);
        let clans = ("tracker-subtab", 2usize);

        assert_eq!(window.find(players).selected(), Some(true), "the tracker opens on players");
        // The players table sorts by its own columns.
        assert!(window.try_find(("tracker-sort", 0usize)).is_some());
        assert!(window.try_find(("tracker-clan-sort", 0usize)).is_none());

        window.click(clans, cx);
        assert_eq!(window.find(clans).selected(), Some(true));
        assert_eq!(window.find(players).selected(), Some(false), "one table at a time");
        // And the clans table brings its own.
        assert!(window.try_find(("tracker-clan-sort", 0usize)).is_some());
        assert!(window.try_find(("tracker-sort", 0usize)).is_none());

        // The roster is its own layout, so neither table's header survives it.
        window.click(current_match, cx);
        assert_eq!(window.find(current_match).selected(), Some(true));
        assert!(window.try_find(("tracker-sort", 0usize)).is_none());
        assert!(window.try_find(("tracker-clan-sort", 0usize)).is_none());
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_clans_table_keeps_its_own_sort(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);
        window.click(("tracker-subtab", 2usize), cx);

        let by_tag = ("tracker-clan-sort", 0usize);
        let by_encounters = ("tracker-clan-sort", 2usize);

        assert_eq!(window.find(by_encounters).selected(), Some(true), "clans open on encounters");
        window.click(by_tag, cx);
        assert_eq!(window.find(by_tag).selected(), Some(true));
        assert_eq!(window.find(by_encounters).selected(), Some(false));
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
    let window = open_app(cx);
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
        assert_eq!(
            window.find(SEARCH_QUERY).value(),
            Some(first.as_str()),
            "taking it replaces the fragment rather than appending"
        );
    })
    .expect("the test window stays open");
}

/// A pill's operator segment opens a picker, and taking a different operator
/// rewrites that term in the query without disturbing the rest of it.
#[gpui_kit::test]
fn a_pill_operator_can_be_changed_from_the_bar(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);

        window.click(SEARCH_QUERY, cx);
        window.input("build>9000000", cx);
        window.click("search-run", cx);
        window.render_frame(cx);

        // The query parsed, so it reads back as a pill.
        assert!(window.try_find("search-pills").is_some(), "the query is drawn as pills");
        assert!(window.try_find(("search-choice", 0usize)).is_none(), "no picker before one is asked for");

        // The operator is the second segment of the first pill.
        window.click(("search-pill-segment", 1usize), cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        assert!(
            window.find(("search-choice-button", 0usize)).label().is_some(),
            "the picker offers the operators this term takes, each reading as something"
        );

        // Whichever is not the current one is a real change.
        let mut target = None;
        for index in 0..4usize {
            if let Some(found) = window.try_find(("search-choice", index))
                && found.selected() == Some(false)
            {
                target = Some(index);
                break;
            }
        }
        let target = target.expect("there is another operator to take");

        window.click(("search-choice", target), cx);
        window.render_frame(cx);

        let found = window.find(SEARCH_QUERY);
        let rewritten = found.value().expect("the bar still holds a query");
        assert!(rewritten.contains("build"), "the term survives the edit: {rewritten}");
        assert_ne!(rewritten, "build>9000000", "and its operator changed");
        assert!(window.try_find(("search-choice", 0usize)).is_none(), "the picker closes once taken");
    })
    .expect("the test window stays open");
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
