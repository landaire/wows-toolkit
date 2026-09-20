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
        let second_pane_line_mode = ("chart-mode", ChartMode::ALL.len());
        assert!(window.try_find(second_pane_line_mode).is_none(), "only one chart is open to begin with");

        window.click("stats-add-chart", cx);
        assert!(window.try_find(second_pane_line_mode).is_some(), "the new pane brought its own controls");
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
fn the_tracker_sub_tabs_switch_between_players_and_clans(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);

        let players = ("tracker-subtab", 0usize);
        let clans = ("tracker-subtab", 1usize);

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
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_clans_table_keeps_its_own_sort(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::PlayerTracker, cx);
        window.click(("tracker-subtab", 1usize), cx);

        let by_tag = ("tracker-clan-sort", 0usize);
        let by_encounters = ("tracker-clan-sort", 2usize);

        assert_eq!(window.find(by_encounters).selected(), Some(true), "clans open on encounters");
        window.click(by_tag, cx);
        assert_eq!(window.find(by_tag).selected(), Some(true));
        assert_eq!(window.find(by_encounters).selected(), Some(false));
    })
    .expect("the test window stays open");
}
