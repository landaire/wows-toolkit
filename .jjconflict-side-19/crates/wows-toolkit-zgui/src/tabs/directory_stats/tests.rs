//! The directory stats view read over a directory on disk, and its header drawn.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use rust_i18n::t;
use wows_toolkit_replay_data::directory_stats::ReadState;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetProgress;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::view;
use zgui::vocab::Role;

use super::DirectoryHeaderProps;
use super::DirectoryRead;
use super::DirectoryStatsProps;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::shell;
use crate::services::Services;
use crate::theme::SemanticTokensProps;
use crate::theme::palette_choice;

const HEADER_SIZE: WindowSize = WindowSize { width: 900.0, height: 80.0 };

/// A directory holding one file that is not a replay, a live recording and an unrelated file.
fn broken_directory() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    std::fs::write(dir.path().join("broken.wowsreplay"), b"not a replay").expect("the file can be written");
    std::fs::write(dir.path().join("temp.wowsreplay"), b"in progress").expect("the file can be written");
    std::fs::write(dir.path().join("notes.txt"), b"unrelated").expect("the file can be written");
    dir
}

fn open_view(root: &Path) -> HeadlessWindow {
    let root = root.to_path_buf();
    HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 1280.0, height: 800.0 }), move || {
        Services::provide(None);
        let root = root.clone();
        view! {
            ThemeRoot {
                DirectoryStats(root = root, game_data = Signal::stored_local(Some(GameDataCache::without_install())))
            }
        }
    })
    .expect("the view opens headlessly")
}

/// Runs frames in real time until the read has run, since it runs on other threads.
fn settle(window: &mut HeadlessWindow) {
    for _ in 0..30 {
        window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn text_of(window: &mut HeadlessWindow, id: &str) -> Option<String> {
    let element = window.by_test_id(id)?;
    window.a11y(element).and_then(|state| state.label)
}

/// A file that is not a replay is counted skipped, the live recording is left out, and the read
/// ends offering a reload.
#[test]
fn a_finished_read_reports_what_it_skipped_and_offers_a_reload() {
    let dir = broken_directory();
    let mut window = open_view(dir.path());
    settle(&mut window);

    assert!(window.by_test_id("directory-stats-status").is_none(), "the read has finished");
    assert!(window.by_test_id("directory-stats-progress").is_none());
    assert_eq!(text_of(&mut window, "directory-stats-skipped").as_deref(), Some("1 replays skipped"));
    let reload = window.by_test_id("directory-stats-reload").expect("a finished read offers a reload");

    // A replay added since is picked up by the reload.
    std::fs::write(dir.path().join("another.wowsreplay"), b"not a replay either").expect("the file can be written");
    window.click(reload).expect("the reload button is clicked");
    settle(&mut window);
    assert_eq!(text_of(&mut window, "directory-stats-skipped").as_deref(), Some("2 replays skipped"));
}

/// An empty directory skips nothing and shows no skipped count.
#[test]
fn an_empty_directory_shows_no_skipped_count() {
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let mut window = open_view(dir.path());
    settle(&mut window);

    assert!(window.by_test_id("directory-stats-reload").is_some());
    assert!(window.by_test_id("directory-stats-skipped").is_none());
}

fn dock_tab_selected(window: &mut HeadlessWindow, label: &str) -> Option<bool> {
    let tab = window.by_role_and_label(Role::Tab, label)?;
    window.a11y(tab).and_then(|state| state.selected)
}

/// Choosing a dock tab of a directory opened from the replay browser shows that panel.
#[test]
fn a_dock_tab_chosen_in_a_directory_tab_is_shown() {
    let dir = broken_directory();
    let mut window = open_shell(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    shell().open_directory_stats(dir.path().to_path_buf());
    settle(&mut window);
    assert!(window.by_test_id("directory-stats-reload").is_some(), "the read has finished");

    let ships = t!("ui.stats.ships").into_owned();
    let tab = window.by_role_and_label(Role::Tab, &ships).expect("the Ships tab is in the dock");
    window.click(tab).expect("the Ships tab is clicked");
    settle(&mut window);
    assert_eq!(dock_tab_selected(&mut window, &ships), Some(true));
}

fn shoot_header(appearance: Appearance, read: DirectoryRead, name: &str) -> PathBuf {
    let choice = match appearance {
        Appearance::Dark => ThemeChoice::Dark,
        Appearance::Light => ThemeChoice::Light,
    };
    let options = HeadlessOptions { size: HEADER_SIZE, ..review_options(appearance) };
    let mut window = HeadlessWindow::open(options, move || {
        Services::provide(None);
        view! {
            ThemeRoot(choice = palette_choice(choice)) {
                SemanticTokens(choice = choice)
                DirectoryHeader(
                    root = PathBuf::from("C:/Games/World_of_Warships/replays"),
                    read = Signal::stored_local(read),
                    on_reload = UnsyncCallback::new(|()| {})
                )
            }
        }
    })
    .expect("the header opens headlessly");
    window.advance(Duration::from_millis(500));

    save_shot(&mut window, name)
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the header"]
fn the_header_renders_in_dark_and_light() {
    let skips = ReplaySetSkips { unreadable: 2, missing_build: 14, no_results: 1 };
    let reading = DirectoryRead { state: ReadState::Reading(ReplaySetProgress { done: 120, total: 310 }), skips };
    let finished = DirectoryRead { state: ReadState::Finished, skips };
    for (appearance, theme) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        for (read, state) in [(reading, "reading"), (finished, "finished")] {
            let path = shoot_header(appearance, read, &format!("directory-stats-header-{state}-{theme}"));
            println!("{}", path.display());
        }
    }
}
