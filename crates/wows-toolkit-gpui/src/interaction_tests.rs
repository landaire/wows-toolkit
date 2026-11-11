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
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::dock::DockState;
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
use crate::app::SettingsSection;
use crate::settings::GpuiSettings;
use crate::stats::load::SessionData;
use crate::zoom::ZoomFactor;

/// The app tab bar's element id (`App::render`). Tabs inside it are addressed
/// by index, matching `AppTab::ALL`'s order.
const TAB_BAR: &str = "app-tabs";

/// The gear at the end of the tab strip, and the drawer it opens
/// (`app::settings_drawer`).
const SETTINGS_BUTTON: &str = "settings-open";
const SETTINGS_DRAWER: &str = "settings-drawer";

/// The debug-build strip shown at the bottom while debug mode is on.
const DEBUG_NOTICE: &str = "app-debug-notice";

/// Replay Inspector listing header controls (`replay_inspector::view`).
const LISTING_OPTIONS: &str = "replay-listing-options";
const LISTING_OPEN: &str = "replay-listing-open";
const AUTOLOAD: &str = "replay-listing-auto-load-latest";
const FILTER_RAW_XP: &str = "replay-listing-filter-raw-xp";
const FILTER_HEALS: &str = "replay-listing-filter-heals";

/// One grouping's choice in the listing options.
fn grouping_choice(grouping: ReplayGrouping) -> (&'static str, usize) {
    ("replay-listing-grouping", grouping as usize)
}

/// Armor Viewer sidebar ship search (`armor_viewer::sidebar`).
const SHIP_SEARCH: &str = "armor-sidebar-search";

/// Unpacker tab controls (`unpacker::view`, `unpacker::browser`).
const EXTRACT: &str = "unpacker-extract";
const CANCEL: &str = "unpacker-cancel";
const PKG_FILTER: &str = "unpacker-pkg-filter";
const DUMP_PARAMS: &str = "unpacker-dump-params";
const OUTPUT_DIR: &str = "unpacker-output-dir";
const QUEUE_CLEAR_ALL: &str = "unpacker-queue-clear-all";
const QUEUE_TOGGLE: &str = "unpacker-dock-toggle-right";
const PKG_TREE_COLLAPSE: &str = "unpacker-pkg-tree-collapse";
const PKG_TREE_RAIL: &str = "unpacker-pkg-tree-rail";
const PKG_EXTRACT_LISTED: &str = "unpacker-pkg-extract-listed";

/// Stats tab filter bar (`stats::view`).
const STATS_LIMIT_ENABLED: &str = "stats-limit-enabled";
const STATS_LIMIT_COUNT: &str = "stats-limit-count";
const STATS_LEFT_TOGGLE: &str = "stats-dock-toggle-left";
const STATS_RIGHT_TOGGLE: &str = "stats-dock-toggle-right";

/// The first chart's settings button (`stats::chart_panel`), drawn with the
/// chart whether or not it has anything to plot.
const FIRST_CHART_SETTINGS: (&str, usize) = ("chart-settings", 0);

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
/// Without it the Settings drawer measures the game-data cache under the running
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
/// The armor viewer puts the ships list, the viewport and the options rail
/// side by side; at the default width parts of them are off the edge, and the
/// harness refuses to click what it cannot see.
fn open_wide_app(cx: &mut TestAppContext) -> WindowHandle<App> {
    use_a_temporary_storage_dir();
    cx.update(gpui_kit::init);
    cx.open_window(size(px(2200.), px(900.)), App::new)
}

/// The same, tall enough that a whole scrolling tab is in one frame.
///
/// The harness refuses to click what is below the fold, so a test that drives
/// a control near the bottom of a section of the Settings drawer opens the
/// window this way rather than scrolling to it.
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
pub(crate) fn open_app_in_root(
    cx: &mut TestAppContext,
) -> (WindowHandle<gpui_kit::component::Root>, gpui_kit::Entity<App>) {
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

/// Opens the Settings drawer from the gear and jumps to `section`, so its
/// controls are in the frame.
///
/// Motion is turned off first: a control is clicked where it comes to rest,
/// not part way through the slide. The second frame is the one the jump is
/// measured against.
pub(crate) fn show_settings(window: &mut gpui_kit::Window, section: SettingsSection, cx: &mut gpui_kit::App) {
    cx.set_reduce_motion(true);
    window.render_frame(cx);
    if window.try_find(SETTINGS_DRAWER).is_none() {
        window.click(SETTINGS_BUTTON, cx);
    }
    window.render_frame(cx);
    window.click(("settings-category", section as usize), cx);
    window.render_frame(cx);
    window.render_frame(cx);
}

#[gpui_kit::test]
fn a_grouping_chosen_in_the_listing_options_reaches_the_browser(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);

        assert!(window.try_find(grouping_choice(ReplayGrouping::Date)).is_none(), "the options live behind the cog");
        window.click(LISTING_OPTIONS, cx);
        assert_eq!(window.find(grouping_choice(ReplayGrouping::Date)).selected(), Some(true), "Date is the default");

        window.click(grouping_choice(ReplayGrouping::Ship), cx);
        assert_eq!(window.find(grouping_choice(ReplayGrouping::Ship)).selected(), Some(true));
        assert_eq!(
            window.find(grouping_choice(ReplayGrouping::Date)).selected(),
            Some(false),
            "the choices are exclusive"
        );
    })
    .expect("the test window stays open");

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
        window.click(LISTING_OPTIONS, cx);

        let before = window.find(AUTOLOAD).checked().expect("the autoload checkbox reports a checked state");
        window.click(AUTOLOAD, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(!before));
        window.click(AUTOLOAD, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(before), "a second click returns it");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_listing_options_toggle_only_the_column_clicked(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);

        assert!(window.try_find(FILTER_RAW_XP).is_none(), "the filters live behind the cog");
        window.click(LISTING_OPTIONS, cx);

        let raw_xp_before = window.find(FILTER_RAW_XP).checked().expect("checkbox reports state");
        let heals_before = window.find(FILTER_HEALS).checked().expect("checkbox reports state");

        window.click(FILTER_RAW_XP, cx);
        assert_eq!(window.find(FILTER_RAW_XP).checked(), Some(!raw_xp_before));
        assert_eq!(window.find(FILTER_HEALS).checked(), Some(heals_before), "the other filters are untouched");
    })
    .expect("the test window stays open");
}

/// Each item of the listing's open menu asks for its own kind of dialog.
#[gpui_kit::test]
fn the_listing_open_menu_asks_for_a_file_or_a_directory(cx: &mut TestAppContext) {
    let window = open_app(cx);
    crate::dialog::take_asked();

    cx.update_window(window.into(), |_, window, cx| show_tab(window, AppTab::ReplayInspector, cx))
        .expect("the test window stays open");

    for (row, expected) in [(0u64, crate::dialog::Asked::File), (1u64, crate::dialog::Asked::Folder)] {
        cx.update_window(window.into(), |_, window, cx| {
            window.click(LISTING_OPEN, cx);
            window.render_frame(cx);
            let rows: Vec<u64> = menu_items(window).into_iter().map(|(row, _)| row).collect();
            assert!(rows.contains(&row), "the open menu offers row {row}, got {rows:?}");
            window.within("popup-menu").click(gpui_kit::ElementId::Integer(row), cx);
        })
        .expect("the test window stays open");
        // The menu dismisses itself through a deferred effect, which has to
        // land before its trigger opens it again.
        cx.run_until_parked();
        assert_eq!(crate::dialog::take_asked(), vec![expected], "row {row} asked for its dialog");
    }
}

/// A directory dialog that could not be opened says so, with the reason; one
/// the reader dismissed says nothing.
#[gpui_kit::test]
fn a_directory_dialog_that_cannot_open_says_why(cx: &mut TestAppContext) {
    const DIRECTORY_ROW: u64 = 1;

    let window = open_app(cx);
    cx.update_window(window.into(), |_, window, cx| show_tab(window, AppTab::ReplayInspector, cx))
        .expect("the test window stays open");
    let failed =
        rust_i18n::t!("ui.dialogs.file_dialog_failed", reason = crate::dialog::DialogError::Refused).into_owned();
    assert!(failed.contains(&crate::dialog::DialogError::Refused.to_string()), "the message names the reason");

    for refused in [false, true] {
        if refused {
            crate::dialog::refuse_next();
        }
        let before = crate::toast::said_for_test().len();
        cx.update_window(window.into(), |_, window, cx| {
            window.click(LISTING_OPEN, cx);
            window.render_frame(cx);
            window.within("popup-menu").click(gpui_kit::ElementId::Integer(DIRECTORY_ROW), cx);
        })
        .expect("the test window stays open");
        cx.run_until_parked();

        let said = crate::toast::said_for_test().split_off(before);
        let expected = if refused { vec![failed.clone()] } else { Vec::new() };
        assert_eq!(said, expected, "refused {refused}");
    }
}

/// A settings snapshot with distinguishable values, so an assertion on a
/// checkbox proves it read the snapshot rather than a coincidental default.
/// The WoWs directory is deliberately empty: `apply_settings` then takes its
/// "directory is not set" branch and starts no directory scan or game-data
/// load, which keeps these tests off the filesystem.
pub(crate) fn test_settings() -> GpuiSettings {
    GpuiSettings {
        locale: None,
        theme: Default::default(),
        twitch_token: None,
        twitch_channel: String::new(),
        zoom: ZoomFactor::DEFAULT,
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
        // Asked and answered, so a test window does not open on a dialog.
        build_consent_shown: true,
        replay_consent_shown: true,
        language_selection_shown: true,
        suppress_gpu_encoder_warning: false,
        constants_commit: None,
        code_integrity: Default::default(),
    }
}

#[gpui_kit::test]
fn the_settings_drawer_shows_the_values_it_was_given(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_settings(window, SettingsSection::Replays, cx);
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
        window.click(LISTING_OPTIONS, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(false), "seeded from the settings snapshot");
        window.click(AUTOLOAD, cx);
        assert_eq!(window.find(AUTOLOAD).checked(), Some(true), "and it is still live after seeding");
    })
    .expect("the test window stays open");
}

/// The penetration checker is a section of the options rail, not a panel of
/// its own beside the viewport.
///
/// Its verdicts are read against the plate the pointer is on, so it has to
/// survive clicking and hovering the ship. A section of the rail does, and
/// takes no width from the viewport that the rail does not already take.
#[gpui_kit::test]
fn pen_check_is_a_section_of_the_options_rail(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);
        window.render_frame(cx);

        assert!(window.try_find("armor-analysis-toggle").is_none(), "there is no separate panel to bring up");
        assert!(window.try_find("armor-pen-search").is_none(), "the section starts collapsed");

        window.click("armor-options-section-PenCheck", cx);
        window.render_frame(cx);
        assert!(
            window.within("armor-options-panel").try_find("armor-pen-search").is_some(),
            "its header opens it inside the rail"
        );

        // Whatever else is clicked, the section is still there.
        window.click(SHIP_SEARCH, cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-pen-search").is_some(), "a click elsewhere does not put it away");

        window.click("armor-options-section-PenCheck", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-pen-search").is_none(), "and its header closes it again");
    })
    .expect("the test window stays open");
}

/// The viewport tools and the visibility, hull and display options are a
/// rail beside the viewport, which the status bar shows and hides.
///
/// The viewport has no strip of its own, so the tools leave with the rail.
#[gpui_kit::test]
fn the_status_bar_puts_the_options_rail_away_and_brings_it_back(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-panel").is_some(), "the rail is up without being asked for");
        assert!(window.try_find("armor-show-hidden-button").is_some(), "the tools are on the rail");

        window.click("armor-options-toggle", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    // The rail slides shut before it leaves the tree.
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("armor-options-panel").is_none(), "the toggle puts the rail away");
        assert!(window.try_find("armor-show-hidden-button").is_none(), "and the tools with it");

        window.click("armor-options-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-panel").is_some(), "and brings it back");
    })
    .expect("the test window stays open");
}

/// The status bar shows and hides the replay listing, and carries the
/// session control on the replay tabs only: a session belongs to the replay
/// workspace it was started from.
#[gpui_kit::test]
fn the_status_bar_holds_the_listing_toggle_and_the_session_on_replay_tabs(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);
        window.render_frame(cx);
        assert!(window.try_find("replay-browser-rows").is_some(), "the listing is up");
        assert!(window.try_find("collab-session-toggle").is_some(), "the session control is on the bar");

        window.click("replay-listing-toggle", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    // The listing slides shut before it leaves.
    cx.executor().advance_clock(std::time::Duration::from_millis(400));

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("replay-browser-rows").is_none(), "the toggle puts the listing away");

        window.click("replay-listing-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("replay-browser-rows").is_some(), "and brings it back");

        show_tab(window, AppTab::Launcher, cx);
        window.render_frame(cx);
        assert!(window.try_find("collab-session-toggle").is_none(), "no session control away from the replays");
    })
    .expect("the test window stays open");
}

/// Opening the session popover with no session offers a name and the two ways
/// to start one, and starts neither until a name is given: every peer reads
/// that name, so an unnamed session makes a roster nobody can follow.
#[gpui_kit::test]
fn the_session_popover_starts_nothing_without_a_display_name(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);
        window.render_frame(cx);
        window.click("collab-session-toggle", cx);
        window.render_frame(cx);

        assert!(window.try_find("collab-display-name").is_some(), "a name is asked for first");
        assert!(window.try_find("collab-host").is_some());
        assert!(window.try_find("collab-token").is_some());

        window.click("collab-host", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            let inspector = app.replay_inspector().read(cx);
            assert!(
                !crate::collab_popover::SessionHost::collab(inspector).is_active(),
                "nothing started without a name to start it under"
            );
        })
        .expect("the test window stays open");
}

/// One job is named on the bar with how far it has got; several are counted.
/// A later report from one owner replaces its earlier one, and the same kind
/// of work from two owners is two jobs, each finished on its own.
#[gpui_kit::test]
fn the_status_bar_names_one_job_and_counts_several(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        let first = cx.new(|_cx| ());
        let second = cx.new(|_cx| ());
        let kind = crate::status_bar::JobKind::Contribute;
        window.render_frame(cx);
        assert!(window.try_find("app-status-work").is_none(), "nothing is running");

        let step = crate::status_bar::Progress::new(1, 4);
        crate::status_bar::working(first.downgrade().into(), kind, "First job", step, cx);
        window.render_frame(cx);
        assert_eq!(window.find("app-status-work").label(), Some("First job 1 of 4"));

        let step = crate::status_bar::Progress::new(2, 4);
        crate::status_bar::working(first.downgrade().into(), kind, "First job", step, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("app-status-work").label(),
            Some("First job 2 of 4"),
            "a later report replaces the earlier one"
        );

        crate::status_bar::working(second.downgrade().into(), kind, "Second job", None, cx);
        window.render_frame(cx);
        assert_eq!(window.find("app-status-work").label(), Some("2 tasks running"));

        crate::status_bar::finished(first.entity_id(), kind, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("app-status-work").label(),
            Some("Second job"),
            "finishing one owner's work leaves the other's"
        );

        crate::status_bar::finished(second.entity_id(), kind, cx);
        window.render_frame(cx);
        assert!(window.try_find("app-status-work").is_none(), "finished work leaves the bar");
    })
    .expect("the test window stays open");
}

/// Work whose owner is dropped leaves the bar: a replay tab closed in the
/// middle of a pass drops the pass before it can report itself finished.
#[gpui_kit::test]
fn work_leaves_the_status_bar_with_its_owner(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        let owner = cx.new(|_cx| ());
        let kind = crate::status_bar::JobKind::Contribute;
        crate::status_bar::working(owner.downgrade().into(), kind, "Contributing", None, cx);
        window.render_frame(cx);
        assert!(window.try_find("app-status-work").is_some(), "the work is on the bar");

        drop(owner);
    })
    .expect("the test window stays open");
    // A dropped entity is released once the update that dropped it ends.
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("app-status-work").is_none(), "and leaves with its owner");
    })
    .expect("the test window stays open");
}

/// The session popover opens above its button. The button is on the status
/// bar at the bottom of the window, so a popover opened below it would be
/// pushed back over the button and the bar.
#[gpui_kit::test]
fn the_session_popover_opens_above_the_status_bar(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ReplayInspector, cx);
        window.render_frame(cx);
        window.click("collab-session-toggle", cx);
        window.render_frame(cx);

        let toggle = window.find("collab-session-toggle").bounds();
        let popup = window.find("collab-session-body").bounds();
        let viewport = window.viewport_size();
        assert!(popup.bottom() <= toggle.top(), "the popover {popup:?} is above its button {toggle:?}");
        assert!(
            popup.top() >= px(0.) && popup.left() >= px(0.) && popup.right() <= viewport.width,
            "the popover {popup:?} is inside the window {viewport:?}"
        );
    })
    .expect("the test window stays open");
}

/// The status bar shows and hides the ships list.
#[gpui_kit::test]
fn the_status_bar_puts_the_ships_list_away_and_brings_it_back(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);
        window.render_frame(cx);
        assert!(window.try_find(SHIP_SEARCH).is_some(), "the list is up without being asked for");

        window.click("armor-ships-toggle", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");

    // The list slides shut before it leaves the tree.
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(SHIP_SEARCH).is_none(), "the toggle puts the list away");

        window.click("armor-ships-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find(SHIP_SEARCH).is_some(), "and brings it back");
    })
    .expect("the test window stays open");
}

/// A collapsed section is not built at all.
///
/// The rail renders on every window repaint and a hull hover notifies, so the
/// two trees -- which clone the visibility maps and emit a row per zone, part
/// and plate -- start collapsed.
#[gpui_kit::test]
fn a_rail_section_is_built_only_while_it_is_expanded(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);
        window.render_frame(cx);

        assert!(window.try_find("armor-options-body-Display").is_some(), "the display section starts expanded");
        assert!(window.try_find("armor-options-body-Visibility").is_none(), "the visibility tree starts collapsed");

        window.click("armor-options-section-Visibility", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-body-Visibility").is_some(), "its header expands it");

        window.click("armor-options-section-Display", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-body-Display").is_none(), "and collapses one that was open");
    })
    .expect("the test window stays open");
}

/// The rail's filter keeps the controls whose names match and drops the
/// rest, and says so when nothing is left.
#[gpui_kit::test]
fn the_options_filter_keeps_only_the_matching_controls(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-show-hidden").is_some());

        window.click("armor-options-filter", cx);
        window.input("pen ch", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-section-PenCheck").is_some(), "Pen Check matches");
        assert!(window.try_find("armor-show-hidden").is_none(), "Show Hidden does not");
        assert!(
            window.try_find("armor-options-section-Display").is_none(),
            "a section with nothing matched is not drawn"
        );

        window.input("zzz", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-section-PenCheck").is_none(), "nothing is left");
        let said = rust_i18n::t!("ui.armor.no_matching_options");
        assert_eq!(window.find("armor-options-no-match").label(), Some(&*said), "the rail says nothing matched");
    })
    .expect("the test window stays open");
}

/// While filtering, every section is held open, so a click on a section's
/// head changes nothing on screen; clearing the filter brings back the open
/// state the reader left.
#[gpui_kit::test]
fn a_section_head_clicked_while_filtering_keeps_its_open_state(cx: &mut TestAppContext) {
    let window = open_wide_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::ArmorViewer, cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-body-Display").is_some(), "the display section starts expanded");

        let query = rust_i18n::t!("ui.armor.display").to_lowercase();
        window.click("armor-options-filter", cx);
        window.input(&query, cx);
        window.render_frame(cx);
        window.click("armor-options-section-Display", cx);
        window.render_frame(cx);
        assert!(window.try_find("armor-options-body-Display").is_some(), "a filtered section stays open");

        window.click("armor-options-filter", cx);
        window.press("end", cx);
        for _ in query.chars() {
            window.press("backspace", cx);
        }
        window.render_frame(cx);
        assert!(window.try_find("armor-options-body-Display").is_some(), "the section is open as it was left");
    })
    .expect("the test window stays open");
}

/// A number typed into a slider's field is taken when focus moves off it by
/// a click, not only on Enter. The field is unfocused for a frame before it
/// is told so, and that frame must not put the old value back.
#[gpui_kit::test]
fn a_typed_slider_value_is_kept_when_focus_moves_away(cx: &mut TestAppContext) {
    use crate::slider_field::SliderField;
    use crate::slider_field::SliderRange;
    use gpui_kit::component::input::Input;
    use gpui_kit::component::input::InputState;

    struct Probe {
        field: gpui_kit::Entity<SliderField>,
        other: gpui_kit::Entity<InputState>,
    }
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
                .flex()
                .flex_col()
                .p(px(20.))
                .gap_4()
                .child(self.field.clone())
                .child(Input::new(&self.other).id("probe-other"))
        }
    }

    cx.update(gpui_kit::init);
    let field = cx.new(|cx| SliderField::new(SliderRange::new(0.0, 10.0, 1.0), 5.0, 5.0, cx));
    let shown_field = field.clone();
    let window = cx.open_window(size(px(400.), px(200.)), move |window, cx| {
        let other = cx.new(|cx| InputState::new(window, cx));
        let probe: gpui_kit::AnyView = cx.new(|_| Probe { field: shown_field, other }).into();
        gpui_kit::component::Root::new(probe, window, cx)
    });

    cx.update_window(window.into(), |_, window, cx| {
        // Focus and blur are only reported to an active window.
        window.activate_window();
        window.render_frame(cx);
        window.click("slider-field-number", cx);
        window.press("end", cx);
        window.press("backspace", cx);
        window.input("7", cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("slider-field-number").value(), Some("7"));

        window.click("probe-other", cx);
        window.render_frame(cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(field.read(cx).value(cx), 7.0, "the typed value reached the slider");
        assert_eq!(window.find("slider-field-number").value(), Some("7"), "and the field still shows it");
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
fn the_loaded_grouping_reaches_the_browser_and_the_listing_options(cx: &mut TestAppContext) {
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
        window.click(LISTING_OPTIONS, cx);
        assert_eq!(
            window.find(grouping_choice(ReplayGrouping::Ship)).selected(),
            Some(true),
            "and the options show it"
        );
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

/// A package VFS holding one directory with one file, enough for the folder
/// tree and the listing to draw.
fn one_file_vfs() -> wowsunpack::vfs::VfsPath {
    let vfs: wowsunpack::vfs::VfsPath = wowsunpack::vfs::MemoryFS::new().into();
    vfs.join("gui").and_then(|dir| dir.create_dir()).expect("a memory VFS takes a directory");
    vfs.join("gui/readme.txt").and_then(|file| file.create_file()).expect("a memory VFS takes a file");

    vfs
}

#[gpui_kit::test]
fn the_unpacker_folder_tree_collapses_to_a_rail_and_expands_again(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.apply_settings(test_settings(), window, cx);
            let browser = app.unpacker().read(cx).pkg_browser().clone();
            browser.update(cx, |browser, cx| browser.set_vfs(one_file_vfs(), window, cx));
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);
        window.render_frame(cx);
        assert!(window.try_find(PKG_FILTER).is_some(), "the tree sidebar is up once a VFS is loaded");
        assert!(window.try_find(PKG_TREE_RAIL).is_none(), "no rail while the sidebar is up");

        window.click(PKG_TREE_COLLAPSE, cx);
        window.render_frame(cx);
        assert!(window.try_find(PKG_FILTER).is_none(), "collapsing puts the sidebar away");
        assert!(window.try_find(PKG_EXTRACT_LISTED).is_some(), "the listing stays when the sidebar goes");

        window.click(PKG_TREE_RAIL, cx);
        window.render_frame(cx);
        assert!(window.try_find(PKG_FILTER).is_some(), "the rail brings the sidebar back");
        assert!(window.try_find(PKG_TREE_RAIL).is_none(), "and goes away with it");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_status_bar_closes_the_unpacker_queue_dock_and_opens_it_again(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Unpacker, cx);
        window.render_frame(cx);
        assert!(window.try_find(QUEUE_CLEAR_ALL).is_some(), "the queue is up");

        window.click(QUEUE_TOGGLE, cx);
        window.render_frame(cx);
        assert!(window.try_find(QUEUE_CLEAR_ALL).is_none(), "the toggle closes the queue dock");

        window.click(QUEUE_TOGGLE, cx);
        window.render_frame(cx);
        assert!(window.try_find(QUEUE_CLEAR_ALL).is_some(), "and opens it again");
    })
    .expect("the test window stays open");
}

/// A saved layout may leave the charts in a closed left dock. The status bar
/// has to offer a way to open it again, and no toggle for a dock with nothing
/// in it.
#[gpui_kit::test]
fn a_restored_closed_left_chart_dock_reopens_from_the_status_bar(cx: &mut TestAppContext) {
    let window = open_app(cx);
    window
        .update(cx, |app, window, cx| {
            app.stats().clone().update(cx, |stats, cx| {
                let mut state = stats.dock_area().read(cx).dump(cx);
                let right = state.right_dock.take().expect("the default layout holds the chart in the right dock");
                state.left_dock = Some(DockState::new(right.panel().clone(), DockPlacement::Left, right.size(), false));
                assert!(
                    stats.restore_layout(state, window, cx),
                    "a closed left chart dock is a layout restore accepts"
                );
            });
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);
        window.render_frame(cx);
        assert!(window.try_find(STATS_LEFT_TOGGLE).is_some(), "the closed left dock has a toggle");
        assert!(window.try_find(STATS_RIGHT_TOGGLE).is_none(), "the empty right dock does not");
        assert!(window.try_find(FIRST_CHART_SETTINGS).is_none(), "the chart is not on screen");

        window.click(STATS_LEFT_TOGGLE, cx);
        window.render_frame(cx);
        assert!(window.try_find(FIRST_CHART_SETTINGS).is_some(), "the toggle opens the left dock");

        window.click(STATS_LEFT_TOGGLE, cx);
        window.render_frame(cx);
        assert!(window.try_find(FIRST_CHART_SETTINGS).is_none(), "and closes it again");
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
        show_settings(window, SettingsSection::ReplayIndex, cx);
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
        show_settings(window, SettingsSection::GameDataCache, cx);
    })
    .expect("the test window stays open");
    // The measurement runs off the UI thread; the controls follow it.
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cache-auto-dump").is_some(), "the toggle does not depend on a cache");
        assert!(window.try_find("cache-dir").is_some(), "the directory does not depend on a cache");
        assert!(window.try_find("cache-actions").is_none(), "there is nothing cached to check");
        assert!(window.try_find("cache-open-folder").is_none(), "there is no size to report");
    })
    .expect("the test window stays open");

    let filled = tempfile::tempdir().expect("a temporary directory can be made");
    cached_build(filled.path(), "1.0.0", 100);
    let mut settings = test_settings();
    settings.game_data_cache_dir = filled.path().to_string_lossy().into_owned();
    window.update(cx, |app, window, cx| app.apply_settings(settings, window, cx)).expect("the test window stays open");
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cache-actions").is_some(), "a cached build has maintenance actions");
        assert!(window.try_find("cache-open-folder").is_some(), "a cached build has a folder to open");
        // Pruning keeps the newest build, so one build is nothing to prune.
        assert!(window.try_find("cache-delete-old").is_none(), "a lone build is not an old version");
        window.click("cache-actions", cx);
        window.render_frame(cx);
        let offered = menu_items(window);
        for label in ["Check for Updates", "Validate Cache"] {
            assert!(offered.iter().any(|(_, item)| item.label() == Some(label)), "missing cache action: {label}");
        }
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
        show_settings(window, SettingsSection::GameDataCache, cx);
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
fn the_settings_switches_apply_and_toggle_back(cx: &mut TestAppContext) {
    let window = open_tall_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_settings(window, SettingsSection::Replays, cx);

        // The drawer is writable now, so a click moves the control rather
        // than being refused.
        let before = window.find("show-raw-xp").checked().expect("the switch reports a state");
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
        show_settings(window, SettingsSection::General, cx);

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
        show_settings(window, SettingsSection::Replays, cx);
        // test_settings starts with Raw XP on; turn it off here.
        assert_eq!(window.find("show-raw-xp").checked(), Some(true));
        window.click("show-raw-xp", cx);

        // The replay header's own filter mirrors the same setting.
        show_tab(window, AppTab::ReplayInspector, cx);
        window.click(LISTING_OPTIONS, cx);
        assert_eq!(window.find(FILTER_RAW_XP).checked(), Some(false), "the edit reached the other tab");
    })
    .expect("the test window stays open");
}

#[gpui_kit::test]
fn the_chart_mode_toggle_is_single_select(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Stats, cx);
        assert!(window.try_find(("chart-settings", 0usize)).is_some(), "the first chart is visible beside the table");

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
        assert!(window.try_find(("chart-settings", 0usize)).is_some(), "the first chart is visible beside the table");

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
            Some(period_name(TimePeriod::LastDay).as_str()),
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
    let after_last_day = period_name(TimePeriod::ALL[TimePeriod::LastDay as usize + 1]);
    cx.wait_for(window.into(), Duration::from_millis(500), |window, _| {
        window.find(TRACKER_PERIOD).expanded() == Some(false)
            && window.find(TRACKER_PERIOD).value() == Some(after_last_day.as_str())
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

/// The completions are an answer to typing, not a panel that sits over the
/// results: they arrive with the first edit made in the box and go away on a
/// press anywhere off them.
#[gpui_kit::test]
fn the_query_bar_offers_completions_only_once_it_is_typed_in(cx: &mut TestAppContext) {
    let window = open_app(cx);

    // Typed from a real suggestion, so the test keeps exercising something
    // when the vocabulary changes.
    let first = wows_toolkit_viewmodel::query_bar::suggest::static_suggestions()
        .first()
        .expect("there are suggestions")
        .label
        .clone();
    let needle: String = first.chars().take(3).collect();

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.render_frame(cx);
        assert!(window.try_find("search-completions").is_none(), "nothing is offered before the box is typed in");

        window.click(SEARCH_QUERY, cx);
        window.input(&needle, cx);
        window.render_frame(cx);
        assert!(window.try_find("search-completions").is_some(), "typing offers what may follow it");

        // Anywhere off the dropdown, which includes the box that opened it.
        window.click(SEARCH_QUERY, cx);
        window.render_frame(cx);
        assert!(window.try_find("search-completions").is_none(), "a press off the dropdown puts it away");
    })
    .expect("the test window stays open");
}

/// Running a query is an answer to what the dropdown was offering, so it does
/// not come back over the results: committing empties the box, and an empty
/// box matches every suggestion there is.
#[gpui_kit::test]
fn running_a_query_does_not_leave_the_completions_over_the_results(cx: &mut TestAppContext) {
    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.click(SEARCH_QUERY, cx);
        window.input("outcome=win", cx);
        window.render_frame(cx);

        window.click("search-run", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("search-completions").is_none(),
            "the query was run, so the dropdown is not still offering what might follow it"
        );

        // And the same for Enter, which is the other way to run one.
        window.click(SEARCH_QUERY, cx);
        window.input("outcome=loss", cx);
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(window.try_find("search-completions").is_none(), "nor after Enter");
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

/// A warning the reader switched off does not stand between them and the work;
/// one they have not answered does, and answering it lets the work through.
#[gpui_kit::test]
fn a_suppressed_warning_does_not_stand_in_the_way(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    use std::cell::Cell;
    use std::rc::Rc;

    // Mounted in a `Root`, which is what hosts a dialog.
    let (window, _app) = open_app_in_root(cx);

    // Switched off: hosting a session goes ahead without asking.
    let went = Rc::new(Cell::new(false));
    let ran = Rc::clone(&went);
    let put_up = cx
        .update_window(window.into(), move |_, window, cx| {
            crate::notices::adopt(true, false, cx);
            crate::notices::before_revealing_address(window, cx, move |_window, _cx| ran.set(true));
            window.has_active_dialog(cx)
        })
        .expect("the test window stays open");
    assert!(went.get(), "a suppressed warning runs the work itself");
    assert!(!put_up, "and puts nothing up");

    // Not switched off: the warning is put, and the work waits for it.
    let waited = Rc::new(Cell::new(false));
    let held = Rc::clone(&waited);
    let put_up = cx
        .update_window(window.into(), move |_, window, cx| {
            crate::notices::adopt(false, false, cx);
            crate::notices::before_revealing_address(window, cx, move |_window, _cx| held.set(true));
            window.has_active_dialog(cx)
        })
        .expect("the test window stays open");
    assert!(!waited.get(), "nothing is hosted until the reader has read the warning");
    assert!(put_up, "which is what the dialog is for");
}

/// A warning the reader switched off runs the work it guards after the view that
/// asked for it has been released, not inside it.
///
/// Every caller asks from a listener, which holds that view leased for the length
/// of the call; work that re-enters the same view there is a double lease, which
/// gpui answers with a panic rather than a wrong result.
#[gpui_kit::test]
fn a_suppressed_warning_runs_the_work_outside_the_callers_lease(cx: &mut TestAppContext) {
    use std::cell::Cell;
    use std::rc::Rc;

    let (window, app) = open_app_in_root(cx);

    let ran = Rc::new(Cell::new(false));
    let inside = Rc::clone(&ran);
    let asked = app.clone();
    let leased = app.clone();
    window
        .update(cx, move |_root, window, cx| {
            // Asked from inside an update of the same view, which is the shape
            // every caller has: a listener holds its view leased for the length of
            // the call.
            leased.update(cx, |_app, cx| {
                crate::notices::adopt(true, false, cx);
                crate::notices::before_revealing_address(window, cx, move |_window, cx| {
                    asked.update(cx, |_app, _cx| inside.set(true));
                });
            });
        })
        .expect("the test window stays open");

    cx.run_until_parked();
    assert!(ran.get(), "the work runs, rather than panicking on the lease it was asked from");
}

/// A file dragged over the window says what letting go would do, and the scrim
/// goes once the drag does.
#[gpui_kit::test]
fn a_hovering_file_is_named_on_the_scrim(cx: &mut TestAppContext) {
    let window = open_app(cx);

    window
        .update(cx, |app, _window, cx| {
            assert!(app.drop_scrim().is_none(), "nothing hovers, so nothing is drawn over the window");
            app.note_hovering_files(vec![PathBuf::from("G:/replays/20260928_105501_PJSB018-Yamato.wowsreplay")], cx);
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let said = window.find("app-drop-scrim").label().unwrap_or_default().to_string();
        assert!(said.contains("Yamato"), "the file it would open is named, got {said:?}");
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            app.note_hovering_files(vec![PathBuf::from("one.wowsreplay"), PathBuf::from("two.wowsreplay")], cx);
        })
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let said = window.find("app-drop-scrim").label().unwrap_or_default().to_string();
        assert!(!said.contains("one.wowsreplay"), "two files are not an open, got {said:?}");
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, _window, cx| {
            app.forget_hovering_files(cx);
            assert!(app.drop_scrim().is_none(), "and the scrim goes with the drag");
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
///
/// Not `theme::is_dark_mode`: that mirror is a process-global every test's own
/// render writes, so reading it while the others run is a race.
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
        show_settings(window, SettingsSection::Appearance, cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert_eq!(window.find(system).selected(), Some(true), "an unset theme follows the desktop");

        window.click(dark, cx);
        assert_eq!(window.find(dark).selected(), Some(true));
        assert_eq!(window.find(system).selected(), Some(false), "the choices are exclusive");
        assert_eq!(Theme::global(cx).mode, ThemeMode::Dark, "the dark palette reached the widgets");

        window.click(light, cx);
        assert_eq!(window.find(light).selected(), Some(true));
        assert_eq!(Theme::global(cx).mode, ThemeMode::Light, "the light palette reached the widgets");
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
/// which is what the egui app does. What the Settings drawer reports back is the
/// account it belongs to, or which part of the paste was wrong.
#[gpui_kit::test]
fn a_pasted_twitch_credential_is_read_or_reported(cx: &mut TestAppContext) {
    let window = open_tall_app(cx);
    window
        .update(cx, |app, window, cx| app.apply_settings(test_settings(), window, cx))
        .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        show_settings(window, SettingsSection::Twitch, cx);
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
fn period_name(period: TimePeriod) -> String {
    rust_i18n::t!(period.label_key()).into_owned()
}

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
                search.hover_row(row.clone(), gpui_kit::Point::default(), cx);
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

/// The popup goes up as soon as a preview is coming, holding open water until
/// the first frame lands, so the map arrives in place rather than the popup
/// growing around it.
#[gpui_kit::test]
fn the_preview_popup_holds_its_space_before_the_first_frame(cx: &mut TestAppContext) {
    use std::path::PathBuf;

    let window = open_app(cx);

    cx.update_window(window.into(), |_, window, cx| {
        show_tab(window, AppTab::Search, cx);
        window.render_frame(cx);
        assert!(window.try_find("search-preview").is_none(), "nothing is hovered, so there is no popup");
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
        assert!(window.try_find("search-preview").is_some(), "the popup is up before anything has been baked");
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

        assert!(window.try_find(("chart-settings", 0usize)).is_some(), "the chart is visible beside the overview");
        window.within(DOCK_TABS).click(1usize, cx);
        assert_eq!(window.within(DOCK_TABS).find(1usize).selected(), Some(true));
        assert_eq!(window.within(DOCK_TABS).find(0usize).selected(), Some(false));
        assert!(
            window.try_find(("chart-settings", 0usize)).is_some(),
            "the chart stays visible beside the ship listing"
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

/// A selection clears when the reader clicks somewhere else, even when the
/// text sits in a cell whose click handler stops the release, as the replay
/// table's cells do.
#[gpui_kit::test]
fn clicking_elsewhere_clears_a_selection_made_in_a_clickable_cell(cx: &mut TestAppContext) {
    use gpui_kit::base::TextSelection;

    struct Probe;
    impl gpui_kit::Render for Probe {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::InteractiveElement as _;
            use gpui_kit::ParentElement as _;
            use gpui_kit::StatefulInteractiveElement as _;
            use gpui_kit::Styled as _;
            use gpui_kit::base::TestSupportExt as _;
            gpui_kit::div()
                .size_full()
                .p(px(20.))
                .text_size(px(16.))
                .child(
                    gpui_kit::div()
                        .id("cell")
                        .on_click(|_event, _window, cx| cx.stop_propagation())
                        .child(crate::ui::selectable_text("probe", "SELECTABLE")),
                )
                .child(gpui_kit::div().id("elsewhere").test_support().h(px(40.)).w_full())
        }
    }

    cx.update(gpui_kit::init);
    let window = cx.open_window(size(px(400.), px(200.)), |window, cx| {
        let probe: gpui_kit::AnyView = cx.new(|_| Probe).into();
        gpui_kit::component::Root::new(probe, window, cx)
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.drag(gpui_kit::point(px(22.), px(28.)), gpui_kit::point(px(120.), px(28.)), cx);
        assert!(TextSelection::has_selection(window, cx), "dragging over the run selected nothing");

        window.click("elsewhere", cx);
        assert!(!TextSelection::has_selection(window, cx), "the click elsewhere left the selection in place");
    })
    .expect("the test window stays open");
}

/// A toast reaches the screen.
///
/// Same layer question as the palette: `Root` holds the queue and draws it.
/// Without that layer every toast in the app is silent.
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

/// A crash report is dozens of lines of backtrace. It scrolls inside its own
/// box, so the dialog and its buttons stay inside the window.
#[gpui_kit::test]
fn a_long_crash_report_scrolls_inside_the_dialog(cx: &mut TestAppContext) {
    let (window, app) = open_app_in_root(cx);
    let report: String = (0..200).map(|line| format!("  {line}: frame::in::the::backtrace\n")).collect();

    cx.update_window(window.into(), |_, window, cx| {
        app.update(cx, |app, cx| app.show_crash_report(report, window, cx));
        window.render_frame(cx);

        let details = window.find("crash-report-scroll").bounds();
        assert!(
            details.size.height <= px(super::app::CRASH_REPORT_HEIGHT + 0.5),
            "the report box is held to its height, got {:?}",
            details.size.height
        );
        assert!(details.bottom() < window.viewport_size().height, "and the dialog ends inside the window");
    })
    .expect("the test window stays open");
}

/// A battle landing in the replays directory says so. It says the battle was
/// opened only when a tab did open, and names it as the replay tab would when
/// the listing holds it.
#[gpui_kit::test]
fn a_landed_battle_is_announced_whether_or_not_it_is_opened(cx: &mut TestAppContext) {
    use crate::replay_inspector::browser_view::ReplayBrowserEvent;

    let (window, app) = open_app_in_root(cx);
    let inspector = cx.update(|cx| app.read(cx).replay_inspector().clone());
    let browser = cx.update(|cx| inspector.read(cx).browser_for_test().clone());
    let listed = PathBuf::from("/replays/listed.wowsreplay");
    browser.update(cx, |browser, cx| browser.seed_listing_for_test(listed.clone(), cx));
    // No ship is known without game data, and the map is the raw space name.
    let listed_battle = "Spectator - spaces/00_CO_ocean";

    let announce = |path: PathBuf, cx: &mut TestAppContext| -> Vec<String> {
        let before = crate::toast::said_for_test().len();
        browser.update(cx, |_browser, cx| cx.emit(ReplayBrowserEvent::ReplayAppeared(path)));
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
        crate::toast::said_for_test().split_off(before)
    };

    // Autoload is on, but with no game data nothing can be opened, and a file
    // the listing does not hold is named by its file name.
    inspector.update(cx, |view, cx| view.adopt_autoload(true, cx));
    let said = announce(PathBuf::from("/replays/unlisted.wowsreplay"), cx);
    assert_eq!(said.first(), Some(&rust_i18n::t!("ui.replay.new_battle_listed", battle = "unlisted").into_owned()));

    // Autoload off: listed, under the listing's name for it.
    inspector.update(cx, |view, cx| view.adopt_autoload(false, cx));
    let said = announce(listed.clone(), cx);
    assert_eq!(said.first(), Some(&rust_i18n::t!("ui.replay.new_battle_listed", battle = listed_battle).into_owned()));

    // With game data to read it, autoload opens it.
    inspector.update(cx, |view, cx| {
        view.seed_game_data("G:/does-not-exist");
        view.adopt_autoload(true, cx);
    });
    let said = announce(listed, cx);
    assert_eq!(said.first(), Some(&rust_i18n::t!("ui.replay.opened_new_battle", battle = listed_battle).into_owned()));
}

/// A state the reader has to fix stays on screen, and one per state: the egui
/// app makes these permanent and closable, and arms each once, so an app that
/// retries does not stack a column of the same complaint.
#[gpui_kit::test]
async fn a_stuck_state_stays_up_and_does_not_stack(cx: &mut TestAppContext) {
    let (window, _app) = open_app_in_root(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        crate::toast::stuck("wows-dir", "the directory is not an install", window, cx);
        crate::toast::stuck("wows-dir", "the directory is not an install", window, cx);
        // A different state is its own message, not a replacement.
        crate::toast::stuck("twitch-token", "the credential was refused", window, cx);
    })
    .expect("the test window stays open");

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        // The toast itself, not the list it is in nor the parts it is made of.
        let showing = gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .filter(|element| element.path().last().is_some_and(|id| format!("{id:?}") == "Name(\"notification\")"))
            .count();
        assert_eq!(showing, 2, "a repeat of one state replaces it; a different state is its own message");

        // Fixed: that one comes down, the other stays.
        crate::toast::resolved("wows-dir", window, cx);
    })
    .expect("the test window stays open");

    // Taking one down animates it out, so the wait is for it to be gone rather
    // than for the next frame.
    cx.wait_for(window.into(), Duration::from_millis(500), |window, _| {
        gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .filter(|element| element.path().last().is_some_and(|id| format!("{id:?}") == "Name(\"notification\")"))
            .count()
            == 1
    })
    .await;
}

/// A toast reaches the screen of a secondary window too.
///
/// A popped-out panel's window is rooted in a `Shell` around the panel, and the
/// `Root` above it must still draw the renderer's own messages.
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
/// The dialog is held and drawn by `Root`; without that layer the palette
/// opens where nobody can see it.
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

/// The header of the palette's second layer, which the root list lacks.
const PALETTE_BACK: &str = "palette-back";

/// Draws `window`, lets deferred work and spawned focus run, then draws again.
fn settle(cx: &mut TestAppContext, window: WindowHandle<gpui_kit::component::Root>) {
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
}

/// Ctrl+P, the ship entry, a ship's name and Enter open that ship in the Armor
/// Viewer. On the Armor Viewer, ctrl+p goes straight to the ship list, and
/// Escape or Backspace in its empty field goes back to the root list.
#[gpui_kit::test]
fn the_palette_opens_any_ship_in_the_armor_viewer(cx: &mut TestAppContext) {
    use crate::palette::ArmorShipRow;
    use crate::palette::TestArmorShips;
    use wowsunpack::game_params::types::Species;

    let (window, _app) = open_app_in_root(cx);
    cx.update(|cx| {
        cx.set_global(TestArmorShips {
            rows: vec![
                ArmorShipRow::new("PASB018".into(), "Montana".into(), 10, Species::Battleship, "U.S.A."),
                ArmorShipRow::new("PJSB018".into(), "Yamato".into(), 10, Species::Battleship, "Japan"),
                ArmorShipRow::new("PZSD109".into(), "Sm\u{e5}land".into(), 10, Species::Destroyer, "Europe"),
            ],
            opened: Vec::new(),
        })
    });
    // Each step ends its own update: a typed query reaches the list when
    // effects flush, which is after the closure returns.
    let step = |cx: &mut TestAppContext, act: &dyn Fn(&mut gpui_kit::Window, &mut gpui_kit::App)| {
        cx.update_window(window.into(), |_, window, cx| act(window, cx)).expect("the test window stays open");
        settle(cx, window);
    };

    step(cx, &|window, cx| window.press("ctrl-p", cx));
    step(cx, &|window, cx| {
        assert!(window.try_find(PALETTE_BACK).is_none(), "ctrl+p elsewhere opens the root list");
        window.input("open ship", cx);
    });
    step(cx, &|window, cx| window.press("enter", cx));

    step(cx, &|window, cx| {
        assert_eq!(palettes_open(window), 1, "the ship list replaces the root list");
        assert!(window.try_find(PALETTE_BACK).is_some(), "the ship entry opens the second layer");
        assert!(window.try_find(IndexPath::new(2)).is_some(), "an empty search lists every ship");
        window.input("smal", cx);
    });
    step(cx, &|window, cx| {
        assert!(
            window.find(IndexPath::new(0)).label().is_some_and(|label| label.starts_with("Sm\u{e5}land")),
            "the search folds diacritics"
        );
        assert!(window.try_find(IndexPath::new(1)).is_none(), "and lists only the match");
        // Escape clears a typed query before it leaves the layer.
        window.press("escape", cx);
    });
    step(cx, &|window, cx| {
        assert!(window.try_find(PALETTE_BACK).is_some(), "clearing the query stays in the ship list");
        window.input("yamat", cx);
    });
    step(cx, &|window, cx| window.press("enter", cx));

    step(cx, &|window, cx| {
        assert_eq!(palettes_open(window), 0, "choosing a ship closes the palette");
        assert_eq!(
            window.within(TAB_BAR).find(tab_index(AppTab::ArmorViewer)).selected(),
            Some(true),
            "the Armor Viewer is brought forward"
        );
        assert_eq!(cx.global::<TestArmorShips>().opened, ["PJSB018"], "the chosen ship is the one opened");
        window.press("ctrl-p", cx);
    });
    step(cx, &|window, cx| {
        assert!(window.try_find(PALETTE_BACK).is_some(), "on the Armor Viewer ctrl+p opens the ship list");
        window.press("escape", cx);
    });
    step(cx, &|window, cx| {
        assert_eq!(palettes_open(window), 1, "Escape leaves a palette up");
        assert!(window.try_find(PALETTE_BACK).is_none(), "Escape in the ship list returns to the root list");
        window.press("escape", cx);
    });
    step(cx, &|window, cx| {
        assert_eq!(palettes_open(window), 0, "Escape in the root list closes it");
        window.press("ctrl-k", cx);
    });
    step(cx, &|window, cx| {
        assert_eq!(palettes_open(window), 1);
        assert!(window.try_find(PALETTE_BACK).is_none(), "ctrl+k opens the root list on any tab");
        window.press("escape", cx);
    });
    step(cx, &|window, cx| window.press("ctrl-p", cx));
    step(cx, &|window, cx| {
        assert!(window.try_find(PALETTE_BACK).is_some());
        window.press("backspace", cx);
    });
    step(cx, &|window, _cx| {
        assert_eq!(palettes_open(window), 1);
        assert!(window.try_find(PALETTE_BACK).is_none(), "Backspace in an empty ship search returns to the root list");
    });
}

/// A directory's stats open as one tab beside the session's. Asking again for
/// the same directory, under another spelling of its path, goes back to that
/// tab rather than opening a second read of the same replays.
#[gpui_kit::test]
fn a_second_request_for_a_directorys_stats_focuses_its_tab(cx: &mut TestAppContext) {
    let window = open_app(cx);
    let replays = tempfile::tempdir().expect("a temporary directory can be made");
    std::fs::create_dir(replays.path().join("sub")).expect("a subdirectory can be made");
    let alias = replays.path().join("sub").join("..");
    let directory_tab = tab_index(AppTab::Stats) + 1;

    window
        .update(cx, |app, window, cx| {
            let game_data = crate::replay_inspector::load::GameDataCache::without_install();
            app.open_directory_stats_reading(replays.path().to_path_buf(), game_data, window, cx);
        })
        .expect("the test window stays open");
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let tabs = window.within(TAB_BAR);
        assert_eq!(tabs.find(directory_tab).selected(), Some(true), "the new tab opens beside Stats, selected");
        window.within(TAB_BAR).click(tab_index(AppTab::Stats), cx);
    })
    .expect("the test window stays open");

    window
        .update(cx, |app, window, cx| {
            let game_data = crate::replay_inspector::load::GameDataCache::without_install();
            app.open_directory_stats_reading(alias.clone(), game_data, window, cx);
        })
        .expect("the test window stays open");
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let tabs = window.within(TAB_BAR);
        assert_eq!(tabs.find(directory_tab).selected(), Some(true), "the existing tab is selected again");
        assert!(
            tabs.try_find(AppTab::ALL.len() + 1).is_none(),
            "one tab for the directory, however its path is spelled"
        );
    })
    .expect("the test window stays open");
}

/// Installs the background runtime and an empty index, as startup does.
pub(crate) fn open_index(cx: &mut TestAppContext) -> sqlx::SqlitePool {
    cx.update(|cx| crate::runtime::init(cx).expect("the runtime starts"));
    let runtime = cx.update(|cx| crate::runtime::runtime(cx)).expect("the runtime is installed");
    let pool = runtime.block_on(wows_toolkit_config::test_pool());
    cx.update(|cx| crate::settings_store::init(pool.clone(), cx));
    pool
}

/// A listing row in the replay browser, by its place in the tree.
fn browser_row(row: usize) -> (&'static str, usize) {
    ("replay-browser-row", row)
}

/// Picking rows with ctrl and shift and asking the menu to re-index them reads
/// exactly the rows picked, without a question: their build is installed.
///
/// The files are not real replays, so each read fails and lands in the
/// unreadable-file ledger, which is what shows which files were read. They are
/// not under the install's replays directory, and the index's own source is
/// rooted there all the same.
#[gpui_kit::test]
fn a_forced_reindex_of_chosen_rows_reads_exactly_those_rows(cx: &mut TestAppContext) {
    use crate::replay_inspector::GameDataStatus;
    use wows_toolkit_config::index::unindexable::Unindexable;

    let (window, app) = open_app_in_root(cx);
    let pool = open_index(cx);
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let wows = dir.path().join("wows");
    let mut settings = test_settings();
    settings.wows_dir = wows.to_string_lossy().into_owned();
    app.update(cx, |app, _cx| app.seed_settings_for_test(settings));

    std::fs::create_dir_all(wows.join("bin").join("100")).expect("the build directory can be made");
    let replays = dir.path().join("replays");
    std::fs::create_dir_all(&replays).expect("the replays directory can be made");
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|name| {
        let path = replays.join(format!("{name}.wowsreplay"));
        std::fs::write(&path, b"not a replay").expect("the file can be written");
        path
    });

    let inspector = cx.update(|cx| app.read(cx).replay_inspector().clone());
    inspector.update(cx, |view, _cx| view.seed_game_data(&wows.to_string_lossy()));
    let browser = cx.update(|cx| inspector.read(cx).browser_for_test().clone());
    browser.update(cx, |browser, cx| {
        // Rows are listed once the game data has settled, whichever way.
        browser.set_game_data(&GameDataStatus::Failed("no install in a test".to_owned()), cx);
        for (path, played) in [
            (&a, "01.01.2026 10:00:00"),
            (&b, "02.01.2026 10:00:00"),
            (&c, "03.01.2026 10:00:00"),
            (&d, "04.01.2026 10:00:00"),
        ] {
            browser.seed_release_for_test(path.clone(), "1.0.0", 100, played, cx);
        }
    });

    // Plain click on a, shift-click on c marks a to c, ctrl-click takes b out.
    let row = |path: &PathBuf, cx: &mut TestAppContext| -> (&'static str, usize) {
        let row = cx.update(|cx| browser.read(cx).row_for_test(path, cx)).expect("the replay is on a row");
        browser_row(row)
    };
    let (row_a, row_b, row_c) = (row(&a, cx), row(&b, cx), row(&c, cx));
    let shift = gpui_kit::Modifiers { shift: true, ..gpui_kit::Modifiers::default() };
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.within("replay-browser-rows").click(row_a, cx);
        window.render_frame(cx);
        window.within("replay-browser-rows").click_with_modifiers(row_c, shift, cx);
        window.render_frame(cx);
        let secondary = gpui_kit::Modifiers::secondary_key(cx);
        window.within("replay-browser-rows").click_with_modifiers(row_b, secondary, cx);
        window.render_frame(cx);
        window.within("replay-browser-rows").right_click(row_a, cx);
    })
    .expect("the test window stays open");
    cx.run_until_parked();

    let label = rust_i18n::t!("ui.reindex.context_many", count = 2).into_owned();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let offered = menu_items(window);
        let labels: Vec<Option<String>> = offered.iter().map(|(_, item)| item.label().map(str::to_owned)).collect();
        let (row, _) = offered
            .into_iter()
            .find(|(_, item)| item.label() == Some(label.as_str()))
            .unwrap_or_else(|| panic!("the row menu offers {label}, offered {labels:?}"));
        window.within("popup-menu").click(gpui_kit::ElementId::Integer(row), cx);
    })
    .expect("the test window stays open");

    let before = crate::toast::said_for_test().len();
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    assert!(
        cx.update_window(window.into(), |_, window, _cx| window.try_find("force-reindex-summary").is_none())
            .expect("the test window stays open"),
        "rows the reader picked, all readable here, are re-indexed without a question"
    );
    let runtime = cx.update(|cx| crate::runtime::runtime(cx)).expect("the runtime is installed");
    let ledger = runtime.block_on(Unindexable::load(&pool));
    assert!(ledger.contains(&a) && ledger.contains(&c), "both picked rows were read");
    assert!(!ledger.contains(&b), "the row ctrl-clicked out of the range was not");
    assert!(!ledger.contains(&d), "nor the row outside the range");
    assert_eq!(ledger.len(), 2);
    let sources = runtime.block_on(wows_toolkit_config::index::query::list_sources(&pool)).expect("the sources read");
    let roots: Vec<Option<PathBuf>> = sources.into_iter().map(|source| source.root_path).collect();
    assert_eq!(roots, [Some(wows.join("replays"))], "the source is the install's, not the chosen rows' directory");

    let said = crate::toast::said_for_test().split_off(before);
    assert!(
        said.contains(&rust_i18n::t!("ui.reindex.done", indexed = 0, failed = 2, skipped = 0).into_owned()),
        "the end of the pass is announced with its counts, said {said:?}"
    );
}

/// The palette lists the listing's game versions newest first, each with its
/// replay count, and choosing one asks first with the count and the dates.
#[gpui_kit::test]
fn the_palette_offers_versions_newest_first_and_asks_before_reindexing(cx: &mut TestAppContext) {
    use crate::replay_inspector::GameDataStatus;

    let (window, app) = open_app_in_root(cx);
    open_index(cx);

    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let wows = dir.path().join("wows");
    std::fs::create_dir_all(wows.join("bin").join("300")).expect("the build directory can be made");
    let mut settings = test_settings();
    settings.wows_dir = wows.to_string_lossy().into_owned();
    app.update(cx, |app, _cx| app.seed_settings_for_test(settings));

    let inspector = cx.update(|cx| app.read(cx).replay_inspector().clone());
    inspector.update(cx, |view, _cx| view.seed_game_data(&wows.to_string_lossy()));
    let browser = cx.update(|cx| inspector.read(cx).browser_for_test().clone());
    browser.update(cx, |browser, cx| {
        browser.set_game_data(&GameDataStatus::Failed("no install in a test".to_owned()), cx);
        for (name, version, build, played) in [
            ("old", "0.11.0", 50, "01.01.2022 12:00:00"),
            ("mid-a", "13.5.1", 200, "15.12.2024 20:15:00"),
            ("mid-b", "13.5.1", 200, "16.12.2024 20:15:00"),
            ("new-a", "14.2.0", 300, "01.04.2025 09:30:00"),
            ("new-b", "14.2.0", 300, "02.04.2025 18:00:00"),
        ] {
            browser.seed_release_for_test(dir.path().join(format!("{name}.wowsreplay")), version, build, played, cx);
        }
    });

    cx.update_window(window.into(), |_, window, cx| {
        app.update(cx, |app, cx| app.open_palette_mode(crate::palette::PaletteMode::ReindexVersions, window, cx));
    })
    .expect("the test window stays open");
    settle(cx, window);

    cx.update_window(window.into(), |_, window, cx| {
        let row = |ix: usize| window.find(IndexPath::new(ix)).label().map(str::to_owned);
        assert_eq!(row(0), Some(rust_i18n::t!("ui.reindex.version_row", version = "14.2.0", count = 2).into_owned()));
        assert_eq!(row(1), Some(rust_i18n::t!("ui.reindex.version_row", version = "13.5.1", count = 2).into_owned()));
        assert_eq!(row(2), Some(rust_i18n::t!("ui.reindex.version_row", version = "0.11.0", count = 1).into_owned()));
        assert!(window.try_find(IndexPath::new(3)).is_none(), "one row per version");
        window.press("enter", cx);
    })
    .expect("the test window stays open");
    for _ in 0..3 {
        settle(cx, window);
    }

    cx.update_window(window.into(), |_, window, _cx| {
        let summary = window.find("force-reindex-summary");
        let expected = [
            rust_i18n::t!("ui.reindex.confirm_count", count = 2).into_owned(),
            rust_i18n::t!("ui.reindex.confirm_range", oldest = "2025-04-01 09:30", newest = "2025-04-02 18:00")
                .into_owned(),
        ]
        .join("\n");
        assert_eq!(summary.label(), Some(expected.as_str()), "a palette command always asks, with the count and dates");
    })
    .expect("the test window stays open");
}
