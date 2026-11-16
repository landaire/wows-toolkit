//! The drawer opened in the whole shell, headlessly, and driven the way a reader drives it.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance as DesktopAppearance;
use landaire_ui::MARK_CLASS;
use landaire_ui::MotionPreference;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use rust_i18n::t;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_notices::Notices;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::stats::session::SessionScope;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Rect;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use crate::i18n::LanguageCode;
use crate::shell::AppRootProps;
use crate::shell::Shell;
use crate::shell::settings::Setting;
use crate::shell::settings::SettingsSection;
use crate::shell::settings::category_test_id;
use crate::shell::settings::row_test_id;
use crate::shell::settings::section_test_id;

/// The size the shell is reviewed at.
const REVIEW_SIZE: WindowSize = WindowSize { width: 1280.0, height: 800.0 };

thread_local! {
    /// The shell of the window a test on this thread opened, as its drawer was built.
    static CAPTURED: Cell<Option<Shell>> = const { Cell::new(None) };
}

/// Notes the shell the drawer was built in, for the test that opened it.
pub fn capture(shell: Shell) {
    CAPTURED.set(Some(shell));
}

fn shell() -> Shell {
    CAPTURED.get().expect("the drawer was built in the window this test opened")
}

/// A settings snapshot with values that tell a read of it from a default.
pub(crate) fn test_settings() -> StartupSettings {
    StartupSettings {
        zoom: ZoomFactor::DEFAULT,
        theme: ThemeChoice::Dark,
        twitch_token: None,
        twitch_channel: String::new(),
        // Empty, so no game data is loaded and no directory is read.
        wows_dir: String::new(),
        locale: None,
        current_replay_path: PathBuf::new(),
        replay: ReplaySettings { show_raw_xp: true, show_observed_damage: false, ..ReplaySettings::default() },
        debug_mode: false,
        check_for_updates: true,
        enable_logging: false,
        data_sharing: DataSharingMode::Off,
        proxy_url: String::new(),
        auto_load_latest_replay: false,
        session_scope: SessionScope::Manual,
        output_dir: String::new(),
        armor_defaults: None,
        model_show_armor: true,
        model_xray_highlight: Default::default(),
        armor_options_layout: Default::default(),
        auto_dump_game_data: false,
        game_data_cache_dir: String::new(),
        game_data_repo_commit: None,
        collab_display_name: String::new(),
        suppress_p2p_ip_warning: false,
        disable_auto_open_session_windows: false,
        build_consent_shown: true,
        replay_consent_shown: true,
        language_selection_shown: true,
        suppress_gpu_encoder_warning: false,
        constants_commit: None,
        code_integrity: Default::default(),
        mcp: Default::default(),
    }
}

/// Opens the shell with the storage directory moved to a temporary one, and, without a runtime,
/// the settings in `settings` rather than a database.
fn open(
    runtime: Option<Arc<tokio::runtime::Runtime>>,
    settings: Option<StartupSettings>,
    options: HeadlessOptions,
) -> HeadlessWindow {
    let seeding = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
    crate::interaction_tests::returning_reader(&seeding);

    let mut window = HeadlessWindow::open(options, move || {
        let runtime = runtime.clone();
        view! { AppRoot(runtime = runtime) }
    })
    .expect("the shell opens headlessly");
    window.advance(Duration::from_millis(50));

    if let Some(settings) = settings {
        // What `startup::apply` takes from the settings before it adopts them.
        shell().appearance.theme.set(settings.theme);
        shell().appearance.zoom.set(settings.zoom);
        shell().services().settings.adopt_unsaved(settings);
        window.advance(Duration::from_millis(50));
    }

    window
}

/// The shell with `test_settings`, which no database holds.
fn open_unsaved() -> HeadlessWindow {
    open(None, Some(test_settings()), HeadlessOptions::new(REVIEW_SIZE))
}

/// Lets a click or a key reach the views, and the drawer finish sliding.
fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(400));
}

fn find(window: &HeadlessWindow, id: &str) -> Element {
    window.by_test_id(id).unwrap_or_else(|| panic!("{id} is drawn"))
}

fn click(window: &mut HeadlessWindow, id: &str) {
    let element = find(window, id);
    window.click(element).unwrap_or_else(|err| panic!("{id} can be clicked: {err}"));
    settle(window);
}

fn open_drawer(window: &mut HeadlessWindow) {
    click(window, "settings-open");
    assert!(window.by_test_id("settings-drawer-scroll").is_some(), "the drawer lists the settings");
}

fn label(window: &mut HeadlessWindow, id: &str) -> Option<String> {
    let element = find(window, id);
    window.a11y(element).and_then(|state| state.label)
}

fn top(window: &HeadlessWindow, id: &str) -> f32 {
    let element = find(window, id);
    window.bounds(element).unwrap_or_else(|| panic!("{id} is laid out")).origin.y.0
}

fn at_top_of_scroll(window: &HeadlessWindow, id: &str) -> bool {
    (top(window, id) - top(window, "settings-drawer-scroll")).abs() < 1.5
}

/// Whether the element `id` lies wholly inside the scrolled list: a row near the end of the list
/// cannot be scrolled to its top.
fn in_view_of_scroll(window: &HeadlessWindow, id: &str) -> bool {
    let element = find(window, id);
    let row = window.bounds(element).unwrap_or_else(|| panic!("{id} is laid out"));
    let scroll = find(window, "settings-drawer-scroll");
    let port = window.bounds(scroll).expect("the list is laid out");

    row.top().0 >= port.top().0 - 1.0 && row.bottom().0 <= port.bottom().0 + 1.0
}

/// Scrolls the licenses page to its first notice, as a reader does, and opens it.
fn open_first_notice(window: &mut HeadlessWindow) {
    let first = window.all_by_class("wt-licenses__entry-trigger")[0];
    let scroll = find(window, "licenses-scroll");
    for _ in 0..40 {
        let row = window.bounds(first).expect("the notice is laid out");
        let port = window.bounds(scroll).expect("the page is laid out");
        if row.top().0 >= port.top().0 - 1.0 && row.bottom().0 <= port.bottom().0 + 1.0 {
            window.click(first).expect("a notice's title can be clicked");
            settle(window);
            return;
        }
        window.wheel(scroll, 200.0).expect("the page can be scrolled");
        settle(window);
    }
    panic!("the first notice never scrolled into view");
}

/// The search reads names and descriptions across every section, and says how many settings it
/// kept.
#[test]
fn the_search_filters_every_section_and_counts_what_it_kept() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    let search = find(&window, "settings-search");
    assert_eq!(window.a11y(search).map(|state| state.focused), Some(true), "the search is ready for typing");
    assert!(window.by_test_id("settings-count").is_none(), "nothing is counted before a search");

    // Only in the descriptions of the four column switches.
    window.type_text("results table");
    settle(&mut window);
    assert_eq!(label(&mut window, "settings-count").as_deref(), Some("4 settings"));
    for kept in [Setting::ShowRawXp, Setting::ShowObservedDamage, Setting::ShowEntityId, Setting::ShowHeals] {
        assert!(window.by_test_id(&row_test_id(kept)).is_some(), "{kept:?} is kept");
    }
    assert!(window.by_test_id(&row_test_id(Setting::Language)).is_none(), "the rest are not");
    assert!(window.by_test_id(&section_test_id(SettingsSection::General)).is_none());
    assert!(window.by_test_id(&section_test_id(SettingsSection::Replays)).is_some());
    let general = find(&window, &category_test_id(SettingsSection::General));
    assert_eq!(
        window.a11y(general).map(|state| state.disabled),
        Some(true),
        "a section with nothing left cannot be jumped to"
    );

    click(&mut window, "settings-search-clear");
    assert!(window.by_test_id("settings-count").is_none(), "the clear button empties the search");
    // A title, in another case than it is written in.
    window.type_text("RAW XP");
    settle(&mut window);
    assert_eq!(label(&mut window, "settings-count").as_deref(), Some("1 setting"));
    assert!(window.by_test_id(&row_test_id(Setting::ShowRawXp)).is_some());

    window.type_text("zzz");
    settle(&mut window);
    assert_eq!(label(&mut window, "settings-count").as_deref(), Some("0 settings"));
    assert!(window.by_test_id("settings-no-match").is_some(), "an empty result says so");
}

/// A kept row marks the letters of its name or description the search matched, and an empty
/// search marks nothing.
#[test]
fn the_search_marks_what_it_matched() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    assert!(window.texts_by_class(MARK_CLASS).is_empty(), "nothing is marked before a search");

    window.type_text("raw xp");
    settle(&mut window);
    let marked = window.texts_by_class(MARK_CLASS);
    assert!(!marked.is_empty(), "the kept row is marked");
    assert!(marked.iter().all(|piece| piece.eq_ignore_ascii_case("raw xp")), "{marked:?}");

    click(&mut window, "settings-search-clear");
    assert!(window.texts_by_class(MARK_CLASS).is_empty(), "clearing the search drops the marks");
}

/// A switch writes its row to the shared database, where the next start reads it.
#[test]
fn a_switch_is_saved_to_the_shared_database() {
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    let mut window = open(Some(Arc::clone(&runtime)), None, HeadlessOptions::new(REVIEW_SIZE));
    // The settings are read on the tokio runtime, in real time rather than the window's clock.
    let loaded = wait(&mut window, |_| shell().services().settings.read_untracked(|_| ()).is_some());
    assert!(loaded, "the settings load");
    let before = shell()
        .services()
        .settings
        .read_untracked(|settings| settings.suppress_p2p_ip_warning)
        .expect("the settings are loaded");

    // Opened at the switch, which is further down than the drawer is tall.
    shell().settings_drawer.open_at(Setting::SuppressIpWarning);
    settle(&mut window);
    settle(&mut window);
    click(&mut window, "session-suppress-ip-warning");
    assert_eq!(
        shell().services().settings.read_untracked(|settings| settings.suppress_p2p_ip_warning),
        Some(!before),
        "the switch moves the setting"
    );

    let read = || {
        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            wows_toolkit_config::queries::get_setting::<bool>(&pool, keys::SUPPRESS_P2P_IP_WARNING).await
        })
    };
    let saved = wait(&mut window, |_| read() == Some(!before));
    assert!(saved, "the row reads {} once written", !before);
}

/// A theme tile applies its palette to the window and saves it; one under the pointer previews its
/// palette until the pointer leaves.
#[test]
fn a_theme_tile_previews_and_applies_its_palette() {
    let mut window = open_unsaved();
    // The tiles are further down than the drawer opens to.
    shell().settings_drawer.open_at(Setting::Theme);
    settle(&mut window);
    settle(&mut window);
    let appearance = shell().appearance;
    assert_eq!(appearance.theme.get_untracked(), ThemeChoice::Dark);

    let nord = find(&window, "theme-choice-Nord");
    window.hover(nord).expect("the tile can be pointed at");
    settle(&mut window);
    assert_eq!(
        appearance.theme_preview.get_untracked(),
        Some(ThemeChoice::Nord),
        "the tile under the pointer previews"
    );
    let search = find(&window, "settings-search");
    window.hover(search).expect("the search can be pointed at");
    settle(&mut window);
    assert_eq!(appearance.theme_preview.get_untracked(), None, "leaving it ends the preview");

    click(&mut window, "theme-choice-Light");
    assert_eq!(appearance.theme.get_untracked(), ThemeChoice::Light, "the window is drawn in the palette");
    assert_eq!(shell().services().settings.read_untracked(|settings| settings.theme), Some(ThemeChoice::Light));
    let light = serde_json::to_string(&ThemeChoice::Light).expect("a theme serializes");
    assert!(crate::services::saved_for_test().contains(&(keys::THEME, light)), "and it is saved under the shared key");
    let tile = find(&window, "theme-choice-Light");
    assert_eq!(window.a11y(tile).and_then(|state| state.selected), Some(true), "its tile is marked");
}

/// Every theme tile shows its whole name: a full line box inside the tile, and no ellipsis.
#[test]
fn theme_tile_names_are_not_clipped() {
    let mut window = open_unsaved();
    shell().settings_drawer.open_at(Setting::Theme);
    settle(&mut window);
    settle(&mut window);

    for choice in ThemeChoice::ALL {
        let tile = find(&window, &format!("theme-choice-{choice:?}"));
        let name = find(&window, &format!("theme-name-{choice:?}"));
        let tile = window.bounds(tile).expect("the tile is laid out");
        let name = window.bounds(name).expect("the name is laid out");
        let (tile_top, tile_bottom) = (tile.origin.y.0, tile.origin.y.0 + tile.size.height.0);
        let (name_top, name_bottom) = (name.origin.y.0, name.origin.y.0 + name.size.height.0);

        assert!(name.size.height.0 >= 14.0, "{choice:?}: the name has a whole line box, {name:?}");
        assert!(
            name_top >= tile_top && name_bottom <= tile_bottom,
            "{choice:?}: the name is inside {tile:?}, {name:?}"
        );
        // A name cut short with an ellipsis fills the tile's content box, inside a 1px border and
        // 6px of padding on each side.
        assert!(name.size.width.0 + 14.5 < tile.size.width.0, "{choice:?}: the name fits, {name:?} in {tile:?}");
    }
}

/// A zoom change rescales the list, and the list scrolls so the slider stays where it was drawn.
#[test]
fn the_zoom_slider_stays_in_place_across_a_zoom_change() {
    let mut window = open_unsaved();
    // Down the list, with the rows above the slider laid out across the change.
    shell().settings_drawer.open_at(Setting::Theme);
    settle(&mut window);
    settle(&mut window);
    let before = top(&window, "zoom-slider");

    for zoom in [1.5, 2.0, 1.0, 0.75] {
        shell().set_zoom(ZoomFactor::new(zoom));
        settle(&mut window);
        let after = top(&window, "zoom-slider");
        assert!((after - before).abs() < 1.5, "at {zoom}x the slider moved from {before} to {after}");
    }
}

/// A section's button in the strip scrolls its heading to the top of the list, and is marked.
#[test]
fn a_category_button_scrolls_its_section_to_the_top() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    let section = SettingsSection::Collaboration;
    let heading = section_test_id(section);
    assert!(!at_top_of_scroll(&window, &heading), "the section starts further down");

    click(&mut window, &category_test_id(section));
    settle(&mut window);
    assert!(at_top_of_scroll(&window, &heading), "its heading is at the top of the list");
    let button = find(&window, &category_test_id(section));
    assert_eq!(window.a11y(button).and_then(|state| state.selected), Some(true), "and its button is marked");
}

/// The licenses page names this build and its license, lists every notice with its text folded
/// away, opens one on a click, and its search narrows the list.
#[test]
fn the_licenses_page_shows_and_searches_the_notices() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    click(&mut window, "settings-licenses-open");
    assert!(window.by_test_id("settings-drawer-scroll").is_none(), "the page replaces the settings list");

    let version = t!("ui.labels.app_version", version = env!("CARGO_PKG_VERSION")).into_owned();
    assert_eq!(window.texts_by_class("wt-licenses__version"), vec![version]);
    let project = window.texts_by_class("wt-licenses__project");
    assert!(
        project.iter().any(|text| text.contains("Permission is hereby granted, free of charge")),
        "the MIT license text is shown: {project:?}"
    );

    let notices = Notices::embedded();
    let titles = window.texts_by_class("wt-licenses__entry-title");
    assert_eq!(titles.len(), notices.entries().len(), "every notice is listed");
    assert!(titles.iter().any(|title| title.starts_with("MIT License")), "{titles:?}");
    assert!(
        window.texts_by_class("wt-licenses__entry-body").is_empty(),
        "no notice text is laid out before one is opened"
    );

    let search = find(&window, "licenses-search");
    assert_eq!(window.a11y(search).map(|state| state.focused), Some(true), "the search is ready for typing");
    window.type_text("Apache License");
    settle(&mut window);
    let kept = window.texts_by_class("wt-licenses__entry-title");
    let expected: Vec<String> = notices.matching("Apache License").map(|notice| notice.title().to_owned()).collect();
    assert!(!kept.is_empty() && kept.len() < titles.len(), "the search narrows the list: {kept:?}");
    assert_eq!(kept, expected);
    assert!(
        window.texts_by_class("wt-licenses__project").is_empty(),
        "the matches come first, without the project's license"
    );

    open_first_notice(&mut window);
    let opened = notices.matching("Apache License").next().expect("a notice matched");
    assert_eq!(window.texts_by_class("wt-licenses__entry-body"), vec![opened.body().to_owned()], "its text is shown");

    click(&mut window, "licenses-search-clear");
    window.type_text("no notice holds this text");
    settle(&mut window);
    assert!(window.texts_by_class("wt-licenses__entry-title").is_empty());
    assert!(window.by_test_id("licenses-no-match").is_some(), "an empty result says so");

    click(&mut window, "settings-licenses-back");
    assert!(window.by_test_id("settings-drawer-scroll").is_some(), "the back button returns to the settings");
}

/// Ctrl+, opens and shuts the drawer. Escape in the search empties typed text first, then shuts
/// the drawer.
#[test]
fn the_shortcut_toggles_the_drawer_and_escape_shuts_it() {
    let mut window = open_unsaved();

    window.press_character(',', Modifiers::PRIMARY);
    settle(&mut window);
    let search = window.by_test_id("settings-search").expect("the shortcut opens it");
    let typed = |window: &mut HeadlessWindow| window.a11y(search).and_then(|state| state.value).unwrap_or_default();
    window.type_text("zoom");
    settle(&mut window);
    assert_eq!(typed(&mut window), "zoom", "the search takes focus as the drawer opens");
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert_eq!(typed(&mut window), "", "escape empties the typed search");
    assert!(window.by_test_id("settings-search").is_some(), "and leaves the drawer open");
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(window.by_test_id("settings-search").is_none(), "escape in the empty search shuts it");

    window.press_character(',', Modifiers::PRIMARY);
    settle(&mut window);
    window.press_character(',', Modifiers::PRIMARY);
    settle(&mut window);
    assert!(window.by_test_id("settings-search").is_none(), "the shortcut shuts it as well");
}

/// A press on a theme tile, which restyles the whole window, leaves Escape closing the drawer.
#[test]
fn escape_shuts_the_drawer_after_a_theme_tile_is_pressed() {
    let mut window = open_unsaved();
    shell().settings_drawer.open_at(Setting::Theme);
    settle(&mut window);
    settle(&mut window);

    // The pointer rests on the tile across the press, as a reader's does.
    let tile = find(&window, "theme-choice-Light");
    window.hover(tile).expect("the tile can be pointed at");
    window.advance(Duration::from_secs(2));
    click(&mut window, "theme-choice-Light");
    window.advance(Duration::from_secs(2));
    assert_eq!(shell().appearance.theme.get_untracked(), ThemeChoice::Light);
    assert!(window.by_test_id("settings-search").is_some(), "the press leaves the drawer open");
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(window.by_test_id("settings-search").is_none(), "escape shuts the drawer");
}

/// Choosing a tab on the strip shuts the drawer, as choosing one from the palette does.
#[test]
fn choosing_a_tab_shuts_the_drawer() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    let other = shell()
        .tab_order()
        .into_iter()
        .find(|tab| Some(*tab) != shell().selected_untracked())
        .expect("there is another tab");

    click(&mut window, &other.value());

    assert_eq!(shell().selected_untracked(), Some(other), "the tab is chosen");
    assert!(window.by_test_id("settings-search").is_none(), "and the drawer is shut");
}

/// The warning about the game directory goes once its button opens the drawer at the setting.
#[test]
fn the_game_directory_warning_goes_with_its_button() {
    let mut window = open_unsaved();
    crate::shell::startup::warn_wows_dir_invalid(shell());
    settle(&mut window);

    press_warning_button(&mut window, &t!("ui.messages.wows_dir_invalid"), Setting::WowsDirectory);
}

/// The warning about a Twitch credential Twitch refused goes once its button opens the drawer at
/// the setting, and a later refusal puts it up again.
#[test]
fn the_twitch_warning_goes_with_its_button() {
    let mut window = open_unsaved();
    let warning = t!("ui.messages.twitch_token_invalid").into_owned();
    shell().twitch_refused();
    settle(&mut window);

    press_warning_button(&mut window, &warning, Setting::TwitchToken);

    shell().twitch_refused();
    settle(&mut window);
    assert!(window.texts_by_class("lui-toasts").concat().contains(&warning), "a later refusal warns again");
}

/// Presses the button on the warning that says `warning`, and checks that the drawer opens with
/// `setting` at the top and the warning is gone.
fn press_warning_button(window: &mut HeadlessWindow, warning: &str, setting: Setting) {
    assert!(window.texts_by_class("lui-toasts").concat().contains(warning), "the warning is up");
    let open = window.by_role_and_label(Role::Button, &t!("ui.settings.panel.open")).expect("the warning has a button");
    window.click(open).expect("the button can be pressed");
    settle(window);
    settle(window);

    assert!(window.by_test_id("settings-search").is_some(), "the button opens the drawer");
    assert!(in_view_of_scroll(window, &row_test_id(setting)), "with the setting it is about in view");
    assert!(!window.texts_by_class("lui-toasts").concat().contains(warning), "and the warning is gone");
}

/// A message about a setting opens the drawer with that setting at the top.
#[test]
fn opening_at_a_setting_brings_it_to_the_top() {
    let mut window = open_unsaved();
    shell().settings_drawer.open_at(Setting::WowsDirectory);
    settle(&mut window);
    settle(&mut window);
    assert!(at_top_of_scroll(&window, &row_test_id(Setting::WowsDirectory)), "at the setting it is about");
}

/// Opening the drawer at a setting highlights its row for a moment. Opening it there again
/// highlights the row again: after the highlight has gone, while it is still up, and after the
/// drawer was shut.
#[test]
fn opening_at_a_setting_highlights_its_row_each_time() {
    let mut window = open_unsaved();
    let control = shell().settings_drawer;
    let id = row_test_id(Setting::WowsDirectory);
    control.open_at(Setting::WowsDirectory);
    settle(&mut window);
    let row = find(&window, &id);
    assert!(window.attribute(row, "data-attention").is_some(), "the row is highlighted");
    assert!(window.running_animations(row) > 0, "and the highlight fades");
    window.advance(Duration::from_millis(1000));
    assert_eq!(window.attribute(row, "data-attention"), None, "the highlight goes after its time");

    control.open_at(Setting::WowsDirectory);
    window.advance(Duration::from_millis(600));
    let first = window.attribute(row, "data-attention");
    assert!(first.is_some(), "a second opening highlights it again");
    control.open_at(Setting::WowsDirectory);
    window.advance(Duration::from_millis(100));
    let second = window.attribute(row, "data-attention");
    assert!(second.is_some() && second != first, "one while it is up starts the animation over");
    // Past the end of the earlier highlight, short of the later one's.
    window.advance(Duration::from_millis(900));
    assert!(window.attribute(row, "data-attention").is_some(), "and its time over");
    assert!(window.running_animations(row) > 0, "with the animation still running");
    window.advance(Duration::from_millis(400));
    assert_eq!(window.attribute(row, "data-attention"), None);

    control.close();
    settle(&mut window);
    control.open_at(Setting::WowsDirectory);
    settle(&mut window);
    let row = find(&window, &id);
    assert!(window.attribute(row, "data-attention").is_some(), "opening the shut drawer there highlights it");
}

/// Opening the drawer at a setting the search hides clears the search, and brings the setting's
/// row into view highlighted.
#[test]
fn opening_at_a_searched_away_setting_clears_the_search() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    window.type_text("zoom");
    settle(&mut window);
    let id = row_test_id(Setting::TwitchToken);
    assert!(window.by_test_id(&id).is_none(), "the search hides the setting");

    shell().settings_drawer.open_at(Setting::TwitchToken);
    settle(&mut window);
    settle(&mut window);
    let search = find(&window, "settings-search");
    assert_eq!(window.a11y(search).and_then(|state| state.value).unwrap_or_default(), "", "the search is cleared");
    assert!(in_view_of_scroll(&window, &id), "the setting is in view");
    let row = find(&window, &id);
    assert!(window.attribute(row, "data-attention").is_some(), "and highlighted");
}

/// Under reduced motion the highlight holds still, and goes after the same time.
#[test]
fn under_reduced_motion_the_highlight_holds_still() {
    let mut window = open_unsaved();
    shell().appearance.motion.set(MotionPreference::Reduced);
    shell().settings_drawer.open_at(Setting::WowsDirectory);
    settle(&mut window);
    let row = find(&window, &row_test_id(Setting::WowsDirectory));
    assert!(window.attribute(row, "data-attention").is_some(), "the row is highlighted");
    assert_eq!(window.running_animations(row), 0, "without moving");

    window.advance(Duration::from_millis(1000));
    assert_eq!(window.attribute(row, "data-attention"), None, "the highlight goes after its time");
}

/// The drawer's name follows a change of language.
#[test]
fn the_drawer_label_follows_the_language() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    let english = label(&mut window, "settings-drawer");

    let locale = shell().services().locale;
    locale.choose(LanguageCode::new("de"));
    settle(&mut window);
    let german = t!("ui.tabs.settings").into_owned();
    let shown = label(&mut window, "settings-drawer");
    locale.choose(LanguageCode::new("en"));

    assert_ne!(english.as_deref(), Some(german.as_str()), "German names the drawer differently");
    assert_eq!(shown, Some(german));
}

/// The controls' words follow a change of language made with the drawer open, as their rows'
/// names do.
#[test]
fn the_controls_follow_the_language() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    let english = window.texts_by_class("wt-settings__card").concat();

    let locale = shell().services().locale;
    locale.choose(LanguageCode::new("de"));
    settle(&mut window);
    let german = t!("ui.settings.wows.cache.open_folder").into_owned();
    let shown = window.texts_by_class("wt-settings__card").concat();
    let searched = label(&mut window, "settings-search");
    let asked = t!("ui.settings.panel.search_placeholder").into_owned();
    locale.choose(LanguageCode::new("en"));

    assert!(!english.contains(&german), "German words the button differently");
    assert!(shown.contains(&german), "the button says {german}");
    assert_eq!(searched, Some(asked), "the search asks in German");
}

/// An auto-export directory that is not there marks the path and its row once it has been looked
/// for, while exporting is on.
#[test]
fn a_missing_auto_export_directory_is_marked() {
    let mut settings = test_settings();
    settings.replay.auto_export_data = true;
    settings.replay.auto_export_path = "/nonexistent/auto export".to_owned();
    let mut window = open(None, Some(settings), HeadlessOptions::new(REVIEW_SIZE));
    shell().settings_drawer.open_at(Setting::AutoExport);
    settle(&mut window);

    let path = find(&window, "settings-auto-export-path");
    let row = find(&window, &row_test_id(Setting::AutoExport));
    // The directory is looked for off the UI thread.
    let marked = wait(&mut window, |window| window.attribute(path, "data-error").is_some());
    assert!(marked, "the directory is marked");
    assert!(window.attribute(row, "data-error").is_some(), "and so is its row");
    let note = t!("ui.settings.replay.export_path_missing").into_owned();
    assert!(window.texts_by_class("wt-settings__note").contains(&note), "whose note says why");

    click(&mut window, "auto-export-data");
    assert_eq!(window.attribute(row, "data-error"), None, "nothing is lost while exporting is off");
}

/// A path too long for its line gives way in the middle: the directory it names stays as wide as
/// it is on a line with room, and its start is cut short.
#[test]
fn a_long_path_keeps_its_last_directory_in_view() {
    let roomy = auto_export_path_parts("/replays/exported battles");
    let cut = auto_export_path_parts(
        "/home/someone/games/world of warships/an/archive/of/replays/kept/for/a/long/time/exported battles",
    );

    assert_eq!(cut.end_text, ["/exported battles"]);
    assert!((cut.end.size.width.0 - roomy.end.size.width.0).abs() < 0.5, "the last directory is whole: {cut:?}");
    assert!(
        cut.end.origin.x.0 + cut.end.size.width.0 <= cut.line.origin.x.0 + cut.line.size.width.0 + 0.5,
        "and ends inside the line: {cut:?}"
    );
    assert!(cut.start.size.width.0 < roomy.line.size.width.0, "the start gave way: {cut:?}");
}

/// Where the auto-export path's line and its two parts are drawn.
#[derive(Debug)]
struct PathParts {
    line: Rect<DevicePx, Device>,
    start: Rect<DevicePx, Device>,
    end: Rect<DevicePx, Device>,
    end_text: Vec<String>,
}

fn auto_export_path_parts(path: &str) -> PathParts {
    let mut settings = test_settings();
    settings.replay.auto_export_path = path.to_owned();
    let mut window = open(None, Some(settings), HeadlessOptions::new(REVIEW_SIZE));
    shell().settings_drawer.open_at(Setting::AutoExport);
    settle(&mut window);

    let bounds = |class: &str| {
        let element = window.by_class(class).unwrap_or_else(|| panic!("{class} is drawn"));
        window.bounds(element).expect("the part has a box")
    };

    PathParts {
        line: window.bounds(find(&window, "settings-auto-export-path")).expect("the path has a box"),
        start: bounds("wt-settings__path-start"),
        end: bounds("wt-settings__path-end"),
        end_text: window.texts_by_class("wt-settings__path-end"),
    }
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the drawer"]
fn long_paths_render_cut_in_the_middle() {
    const LONG: &str =
        "/home/someone/games/world of warships/an/archive/of/replays/kept/for/a/long/time/exported battles";

    for (setting, name) in
        [(Setting::AutoExport, "settings-long-export-path"), (Setting::CacheDirectory, "settings-long-cache-path")]
    {
        let mut settings = test_settings();
        settings.replay.auto_export_path = LONG.to_owned();
        settings.game_data_cache_dir = LONG.to_owned();
        let options = HeadlessOptions {
            drawing: Drawing::Device,
            desktop: Some(DesktopAppearance::Dark),
            ..HeadlessOptions::new(REVIEW_SIZE)
        };
        let mut window = open(None, Some(settings), options);
        shell().settings_drawer.open_at(setting);
        settle(&mut window);
        settle(&mut window);

        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"));
        window.save_png(&path).expect("the drawer is drawn to a PNG");
        println!("{}", path.display());
    }
}

/// A setting in error is marked as a whole, along with its section's button and the gear, until
/// the error clears.
#[test]
fn a_setting_in_error_is_marked_until_the_error_clears() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    let row = find(&window, &row_test_id(Setting::WowsDirectory));
    let section = find(&window, &category_test_id(SettingsSection::Game));
    let gear = find(&window, "settings-open");
    let marks =
        |window: &HeadlessWindow| [row, section, gear].map(|element| window.attribute(element, "data-error").is_some());
    assert_eq!(marks(&window), [false; 3], "nothing is marked while the setting works");

    shell().settings_drawer.wows_dir_invalid.set(true);
    settle(&mut window);
    assert_eq!(marks(&window), [true; 3], "the row, its section's button and the gear are marked");
    assert!(window.by_test_id("settings-row-fault").is_some(), "the row's title carries the error icon");
    let note = t!("ui.messages.wows_dir_invalid").into_owned();
    assert!(window.texts_by_class("wt-settings__note").contains(&note), "and its note says why");
    let named = t!("ui.settings.panel.in_error", name = SettingsSection::Game.name()).into_owned();
    assert_eq!(label(&mut window, &category_test_id(SettingsSection::Game)), Some(named));

    shell().settings_drawer.wows_dir_invalid.set(false);
    settle(&mut window);
    assert_eq!(marks(&window), [false; 3], "every mark goes with the error");
    assert!(window.by_test_id("settings-row-fault").is_none(), "and so does the icon");
}

/// A Twitch credential Twitch refused, and a paste that held none, each mark the credential's row.
#[test]
fn a_refused_twitch_credential_marks_its_row() {
    let mut window = open_unsaved();
    open_drawer(&mut window);
    let row = find(&window, &row_test_id(Setting::TwitchToken));

    shell().twitch_refused();
    settle(&mut window);
    assert!(window.attribute(row, "data-error").is_some(), "a refused credential marks the row");
    shell().restart_twitch_poll();
    settle(&mut window);
    assert_eq!(window.attribute(row, "data-error"), None, "with no credential stored nothing is refused");

    // Nothing is on the headless window's clipboard, so the paste finds no credential.
    shell().paste_twitch_token();
    settle(&mut window);
    assert!(window.attribute(row, "data-error").is_some(), "a paste that held no credential marks the row");
    let note = t!("ui.collab.clipboard_empty").into_owned();
    assert!(window.texts_by_class("wt-settings__note").contains(&note), "and its note says why");
}

/// A game directory read at startup that holds no install marks the gear once it has been looked
/// for.
#[test]
fn a_startup_game_directory_with_no_install_is_marked() {
    let wows_dir = "/nonexistent/World_of_Warships".to_owned();
    let mut settings = test_settings();
    settings.wows_dir = wows_dir.clone();
    let mut window = open(None, Some(settings), HeadlessOptions::new(REVIEW_SIZE));
    crate::shell::startup::check_wows_dir(shell(), wows_dir);

    let gear = find(&window, "settings-open");
    assert!(wait(&mut window, |window| window.attribute(gear, "data-error").is_some()), "the gear is marked");
}

/// A game directory refused while the startup directory is still being looked for stays marked
/// once the startup look answers.
#[test]
fn a_late_startup_check_leaves_a_later_refusal_marked() {
    let install = tempfile::tempdir().expect("a temporary directory can be made");
    std::fs::create_dir(install.path().join("bin")).expect("the bin directory can be made");
    let wows_dir = install.path().to_string_lossy().into_owned();
    let mut settings = test_settings();
    settings.wows_dir = wows_dir.clone();
    let mut window = open(None, Some(settings), HeadlessOptions::new(REVIEW_SIZE));

    crate::shell::startup::check_wows_dir(shell(), wows_dir);
    shell().apply_wows_dir("/nonexistent/World_of_Warships".to_owned());
    for _ in 0..10 {
        window.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(shell().settings_drawer.wows_dir_invalid.get_untracked(), "the refused directory stays marked");
}

/// Advances the window in real time until `done` holds, for work on the tokio runtime.
fn wait(window: &mut HeadlessWindow, mut done: impl FnMut(&HeadlessWindow) -> bool) -> bool {
    for _ in 0..80 {
        if done(window) {
            return true;
        }
        window.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(25));
    }

    done(window)
}

/// What a shot of the drawer shows.
#[derive(Clone, Copy)]
enum Shot {
    Top,
    Searched(&'static str),
    At(Setting),
    /// The licenses page, searched for the text when there is one, with the first notice opened.
    Licenses(Option<&'static str>),
}

/// Opens the drawer over settings read from the database and writes it to
/// `target/zgui-shots/<name>.png`.
fn shoot(desktop: DesktopAppearance, shot: Shot, name: &str) -> PathBuf {
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    let options =
        HeadlessOptions { drawing: Drawing::Device, desktop: Some(desktop), ..HeadlessOptions::new(REVIEW_SIZE) };
    let mut window = open(Some(runtime), None, options);
    let loaded = wait(&mut window, |_| shell().services().settings.read_untracked(|_| ()).is_some());
    assert!(loaded, "the settings load");

    match shot {
        Shot::Top => open_drawer(&mut window),
        Shot::Searched(typed) => {
            open_drawer(&mut window);
            window.type_text(typed);
            settle(&mut window);
        }
        Shot::At(setting) => {
            shell().settings_drawer.open_at(setting);
            settle(&mut window);
            settle(&mut window);
        }
        Shot::Licenses(typed) => {
            open_drawer(&mut window);
            click(&mut window, "settings-licenses-open");
            if let Some(typed) = typed {
                window.type_text(typed);
                settle(&mut window);
                open_first_notice(&mut window);
            }
        }
    }

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"));
    window.save_png(&path).expect("the drawer is drawn to a PNG");
    path
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the drawer"]
fn the_drawer_renders_in_dark_and_light_and_searched() {
    for (desktop, shot, name) in [
        (DesktopAppearance::Dark, Shot::Top, "settings-dark"),
        (DesktopAppearance::Light, Shot::Top, "settings-light"),
        (DesktopAppearance::Dark, Shot::Searched("replay"), "settings-search-dark"),
        (DesktopAppearance::Dark, Shot::At(Setting::Theme), "settings-appearance-dark"),
        (DesktopAppearance::Dark, Shot::Licenses(None), "licenses-dark"),
        (DesktopAppearance::Light, Shot::Licenses(Some("Apache License")), "licenses-search-light"),
    ] {
        println!("{}", shoot(desktop, shot, name).display());
    }
}
