//! The shell opened headlessly.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use rust_i18n::t;
use tokio::runtime::Runtime;
use wows_toolkit_viewmodel::settings::keys;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;
use zgui::vocab::Modifiers;

use crate::services::Job;
use crate::services::Progress;
use crate::shell::AppRootProps;
use crate::shell::Shell;
use crate::shell::tabs::TabKey;

const DEBUG_NOTICE: &str = "app-debug-notice";
const WORK: &str = "app-status-work";
const SCRIM: &str = "app-drop-scrim";

/// The size the GPUI shell is reviewed at.
pub(crate) const REVIEW_SIZE: WindowSize = WindowSize { width: 1280.0, height: 800.0 };

thread_local! {
    /// The shell of the window a test on this thread opened last.
    static SHELL: std::cell::Cell<Option<Shell>> = const { std::cell::Cell::new(None) };
}

/// Notes the shell a window built, for the test that opened it.
pub(crate) fn capture_shell(shell: Shell) {
    SHELL.set(Some(shell));
}

/// The shell of the window this test opened.
pub(crate) fn shell() -> Shell {
    SHELL.get().expect("this test opened a window with the shell in it")
}

/// Points `storage_dir` at a temporary directory for the whole process, with every first-run
/// question answered and the update check off, so the shell opens as a returning reader sees it.
/// The directory is leaked: it has to outlive every test in the process.
pub(crate) fn returning_reader(runtime: &Runtime) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        wows_toolkit_config::storage_override::set(dir.path().to_path_buf());
        std::mem::forget(dir);

        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            for key in [keys::BUILD_CONSENT_SHOWN, keys::REPLAY_CONSENT_SHOWN, keys::LANGUAGE_SELECTION_SHOWN] {
                wows_toolkit_config::queries::set_setting(&pool, key, &true).await.expect("a setting is written");
            }
            wows_toolkit_config::queries::set_setting(&pool, keys::CHECK_FOR_UPDATES, &false)
                .await
                .expect("a setting is written");
        });
    });
}

/// Opens the shell headlessly as a returning reader sees it, and lets the settings load.
pub(crate) fn open_shell(options: HeadlessOptions) -> HeadlessWindow {
    let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
    returning_reader(&runtime);
    // As `main` does: anything that fetches over TLS needs a provider.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut window = HeadlessWindow::open(options, move || {
        let runtime = Some(Arc::clone(&runtime));
        view! { AppRoot(runtime = runtime) }
    })
    .expect("the shell opens headlessly");

    // The settings are read on the tokio runtime, in real time rather than the window's clock.
    settle_in_real_time(&mut window);

    window
}

/// Opens the shell headlessly with no tokio runtime, so no settings are read and nothing reaches
/// the network: what the shell does is driven by the test alone.
pub(crate) fn open_bare_shell() -> HeadlessWindow {
    let seeding = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
    returning_reader(&seeding);

    let mut window = HeadlessWindow::open(HeadlessOptions::new(REVIEW_SIZE), || view! { AppRoot(runtime = None) })
        .expect("the shell opens headlessly");
    window.advance(Duration::from_millis(50));

    window
}

/// Runs two seconds of frames, letting work on the tokio runtime land as it goes.
pub(crate) fn settle_in_real_time(window: &mut HeadlessWindow) {
    for _ in 0..20 {
        window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The shell's review size, drawn on the graphics device with the desktop in `appearance`.
pub(crate) fn review_options(appearance: Appearance) -> HeadlessOptions {
    HeadlessOptions { drawing: Drawing::Device, desktop: Some(appearance), ..HeadlessOptions::new(REVIEW_SIZE) }
}

/// Writes what `window` shows to `target/zgui-shots/<name>.png`.
pub(crate) fn save_shot(window: &mut HeadlessWindow, name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"));
    window.save_png(&path).expect("the window is drawn to a PNG");

    path
}

/// Opens the shell on the graphics device with the desktop in `appearance`, and writes it to
/// `target/zgui-shots/<name>.png`.
fn shoot(appearance: Appearance, name: &str) -> PathBuf {
    let mut window = open_shell(review_options(appearance));

    save_shot(&mut window, name)
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the shell"]
fn the_shell_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "shell-dark"), (Appearance::Light, "shell-light")] {
        let path = shoot(appearance, name);
        println!("{}", path.display());
    }
}

fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(400));
}

/// What a screen reader is told the element `id` is called.
fn label(window: &mut HeadlessWindow, id: &str) -> Option<String> {
    let element = window.by_test_id(id)?;
    window.a11y(element)?.label
}

fn tab_selected(window: &mut HeadlessWindow, tab: TabKey) -> Option<bool> {
    let element = window.by_test_id(&tab.value())?;
    window.a11y(element)?.selected
}

#[test]
fn clicking_a_tab_moves_the_selection_to_it() {
    let mut window = open_bare_shell();
    let tabs = shell().tab_order();

    for &tab in &tabs {
        let element = window.by_test_id(&tab.value()).expect("every tab is on the strip");
        window.click(element).expect("the tab is shown");
        settle(&mut window);
        for &other in &tabs {
            assert_eq!(tab_selected(&mut window, other), Some(other == tab), "after clicking {tab:?}, {other:?}");
        }
    }
}

#[test]
fn hovering_a_tab_leaves_the_selection_alone() {
    let mut window = open_bare_shell();
    let startup = shell().selected_untracked().expect("a tab is selected at startup");
    let other = shell().tab_order().into_iter().find(|tab| *tab != startup).expect("there is another tab");

    let element = window.by_test_id(&other.value()).expect("the tab is on the strip");
    window.hover(element).expect("the tab is shown");
    settle(&mut window);

    assert_eq!(tab_selected(&mut window, startup), Some(true), "hover must not move the selection");
    assert_eq!(tab_selected(&mut window, other), Some(false));
}

#[test]
fn clicking_the_app_menu_button_opens_the_menu() {
    let mut window = open_bare_shell();
    assert!(window.by_test_id("app-menu-tactics-board").is_none(), "the menu is closed at startup");

    let trigger = window.by_test_id("app-menu").expect("the app menu button is on the strip");
    window.click(trigger).expect("the app menu button is shown");
    settle(&mut window);

    assert!(window.by_test_id("app-menu-tactics-board").is_some(), "the click opens the menu");
}

#[test]
fn the_debug_shortcut_toggles_the_debug_strip() {
    let mut window = open_bare_shell();
    assert!(window.by_test_id(DEBUG_NOTICE).is_none(), "the debug strip is hidden until the shortcut fires");

    window.press_character('d', Modifiers::PRIMARY | Modifiers::SHIFT);
    settle(&mut window);
    assert!(window.by_test_id(DEBUG_NOTICE).is_some(), "the shortcut shows the debug strip");

    window.press_character('d', Modifiers::PRIMARY | Modifiers::SHIFT);
    settle(&mut window);
    assert!(window.by_test_id(DEBUG_NOTICE).is_none(), "the shortcut toggles rather than latching");
}

#[test]
fn an_unmodified_d_does_not_toggle_the_debug_strip() {
    let mut window = open_bare_shell();

    for modifiers in [Modifiers::NONE, Modifiers::PRIMARY, Modifiers::SHIFT] {
        window.press_character('d', modifiers);
        settle(&mut window);
    }
    assert!(window.by_test_id(DEBUG_NOTICE).is_none(), "only the primary modifier with shift is the shortcut");

    // Absence above has to mean hidden, not unobservable.
    window.press_character('d', Modifiers::PRIMARY | Modifiers::SHIFT);
    settle(&mut window);
    assert!(window.by_test_id(DEBUG_NOTICE).is_some());
}

#[test]
fn the_status_bar_names_one_job_and_counts_several() {
    let mut window = open_bare_shell();
    let jobs = shell().services().jobs;
    assert!(window.by_test_id(WORK).is_none(), "nothing is running");

    let first = jobs.start(Job { name: "First job".to_owned(), progress: Progress::new(1, 4) });
    settle(&mut window);
    let one_of_four = format!("First job {}", t!("ui.app.status_of", done = 1, total = 4));
    assert_eq!(label(&mut window, WORK), Some(one_of_four));

    first.report(Job { name: "First job".to_owned(), progress: Progress::new(2, 4) });
    settle(&mut window);
    let two_of_four = format!("First job {}", t!("ui.app.status_of", done = 2, total = 4));
    assert_eq!(label(&mut window, WORK), Some(two_of_four), "a later report replaces the earlier one");

    let second = jobs.start(Job { name: "Second job".to_owned(), progress: None });
    settle(&mut window);
    assert_eq!(label(&mut window, WORK), Some(t!("ui.app.status_tasks", count = 2).into_owned()));

    drop(first);
    settle(&mut window);
    assert_eq!(label(&mut window, WORK).as_deref(), Some("Second job"), "finishing one job leaves the other");

    drop(second);
    settle(&mut window);
    assert!(window.by_test_id(WORK).is_none(), "finished work leaves the bar");
}

/// A task dropped with its view drops its handle, and the work leaves the bar with it.
#[test]
fn work_leaves_the_status_bar_with_its_owner() {
    let mut window = open_bare_shell();
    let owner = zgui::reactive::Owner::new();
    owner.with(|| {
        let handle = shell().services().jobs.start(Job { name: "Contributing".to_owned(), progress: None });
        zgui::reactive::on_cleanup_local(move || drop(handle));
    });
    settle(&mut window);
    assert!(window.by_test_id(WORK).is_some(), "the work is on the bar");

    drop(owner);
    settle(&mut window);
    assert!(window.by_test_id(WORK).is_none(), "and leaves with its owner");
}

#[test]
fn a_hovering_file_is_named_on_the_scrim() {
    let mut window = open_bare_shell();
    assert!(window.by_test_id(SCRIM).is_none(), "nothing hovers, so nothing is drawn over the window");

    shell().drag_entered(vec![PathBuf::from("G:/replays/20260928_105501_PJSB018-Yamato.wowsreplay")]);
    settle(&mut window);
    let said = label(&mut window, SCRIM).expect("the scrim names what letting go would do");
    assert!(said.contains("Yamato"), "the file it would open is named, got {said:?}");

    shell().drag_entered(vec![PathBuf::from("one.wowsreplay"), PathBuf::from("two.wowsreplay")]);
    settle(&mut window);
    let said = label(&mut window, SCRIM).expect("the scrim says two files are refused");
    assert!(!said.contains("one.wowsreplay"), "two files are not an open, got {said:?}");

    shell().drag_left();
    settle(&mut window);
    assert!(window.by_test_id(SCRIM).is_none(), "and the scrim goes with the drag");
}

/// The zoom shortcuts are the shell's, and still reach it with the palette open over it.
#[test]
fn the_zoom_shortcuts_work_from_inside_the_palette() {
    let mut window = open_shell(HeadlessOptions::new(REVIEW_SIZE));
    let before = shell().appearance.zoom.get_untracked();

    window.press_character('k', Modifiers::PRIMARY);
    settle(&mut window);
    assert!(window.by_test_id("palette").is_some(), "the palette is open");

    window.press_character('=', Modifiers::PRIMARY);
    settle(&mut window);
    assert_eq!(shell().appearance.zoom.get_untracked(), before.zoomed_in());

    // The zoom is saved to settings every test in the process reads.
    shell().set_zoom(before);
    settle_in_real_time(&mut window);
}
