//! The drawer opened in the whole shell, headlessly, and driven the way a reader drives it.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance as DesktopAppearance;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::Element;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::ZoomFactor;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::shell::AppRootProps;
use crate::shell::Shell;
use crate::shell::settings::Setting;
use crate::shell::settings::SettingsSection;
use crate::shell::settings::category_test_id;
use crate::shell::settings::row_test_id;
use crate::shell::settings::section_test_id;

/// The size the GPUI shell is reviewed at.
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
fn test_settings() -> StartupSettings {
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
        output_dir: String::new(),
        armor_defaults: None,
        model_show_armor: true,
        model_xray_highlight: Default::default(),
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

/// A switch writes its row to the shared database, where the next start reads it.
#[test]
fn a_switch_is_saved_to_the_shared_database() {
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    let mut window = open(Some(Arc::clone(&runtime)), None, HeadlessOptions::new(REVIEW_SIZE));
    // The settings are read on the tokio runtime, in real time rather than the window's clock.
    let loaded = wait(&mut window, || shell().services().settings.read_untracked(|_| ()).is_some());
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
    let saved = wait(&mut window, || read() == Some(!before));
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

/// Ctrl+, opens and shuts the drawer, and Escape shuts it from the search.
#[test]
fn the_shortcut_toggles_the_drawer_and_escape_shuts_it() {
    let mut window = open_unsaved();

    window.press_character(',', Modifiers::PRIMARY);
    settle(&mut window);
    assert!(window.by_test_id("settings-search").is_some(), "the shortcut opens it");
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(window.by_test_id("settings-search").is_none(), "escape shuts it, from the search");

    window.press_character(',', Modifiers::PRIMARY);
    settle(&mut window);
    window.press_character(',', Modifiers::PRIMARY);
    settle(&mut window);
    assert!(window.by_test_id("settings-search").is_none(), "the shortcut shuts it as well");
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

/// An auto-export directory that is not there is marked once it has been looked for.
#[test]
fn a_missing_auto_export_directory_is_marked() {
    let mut settings = test_settings();
    settings.replay.auto_export_data = true;
    settings.replay.auto_export_path = "/nonexistent/auto export".to_owned();
    let mut window = open(None, Some(settings), HeadlessOptions::new(REVIEW_SIZE));
    shell().settings_drawer.open_at(Setting::AutoExport);
    settle(&mut window);

    let path = find(&window, "settings-auto-export-path");
    // The directory is looked for off the UI thread.
    for _ in 0..80 {
        if window.by_attribute("data-error", "") == Some(path) {
            break;
        }
        window.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(window.by_attribute("data-error", ""), Some(path), "the directory is marked");
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

    let invalid = shell().settings_drawer.wows_dir_invalid;
    assert!(wait(&mut window, move || invalid.get_untracked()), "the gear is marked");
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
fn wait(window: &mut HeadlessWindow, mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..80 {
        if done() {
            return true;
        }
        window.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(25));
    }

    done()
}

/// What a shot of the drawer shows.
#[derive(Clone, Copy)]
enum Shot {
    Top,
    Searched(&'static str),
    At(Setting),
}

/// Opens the drawer over settings read from the database and writes it to
/// `target/zgui-shots/<name>.png`.
fn shoot(desktop: DesktopAppearance, shot: Shot, name: &str) -> PathBuf {
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    let options =
        HeadlessOptions { drawing: Drawing::Device, desktop: Some(desktop), ..HeadlessOptions::new(REVIEW_SIZE) };
    let mut window = open(Some(runtime), None, options);
    let loaded = wait(&mut window, || shell().services().settings.read_untracked(|_| ()).is_some());
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
    ] {
        println!("{}", shoot(desktop, shot, name).display());
    }
}
