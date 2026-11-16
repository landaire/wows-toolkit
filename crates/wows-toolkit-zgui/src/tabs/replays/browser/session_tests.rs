//! The session as the replays tab fills it: battles the game writes and, for a session since the
//! app opened, the replays the install already lists.

use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::HeadlessWindow;
use rust_i18n::t;
use wows_toolkit_replay_data::simulated_watch;
use wows_toolkit_viewmodel::replay_folders::FolderHistory;
use wows_toolkit_viewmodel::replay_folders::PinnedFolders;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::session::AppOpened;
use wows_toolkit_viewmodel::stats::session::SessionScope;
use wows_toolkit_viewmodel::stats::setting_keys;
use zgui::vocab::Role;

use super::folders::BrowserControl;
use super::folders::MADE;
use super::tests::FIXTURES;
use super::tests::adopt_game_dir;
use super::tests::fixture;
use super::tests::folder_id;
use super::tests::game_data_ready;
use super::tests::land;
use super::tests::open;
use super::tests::replay_dir;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::services::saved_for_test;
use crate::tabs::replays::session_stats::simulated;

/// When each fixture's battle started, as its header and file name give it.
const STARTED: [&str; 3] = ["01.01.2025 21:54:20", "07.01.2026 18:27:37", "14.02.2026 21:56:53"];

/// The run in these tests opened between the first fixture's battle and the second's.
fn opened() -> AppOpened {
    AppOpened::new(jiff::civil::date(2026, 1, 5).at(9, 0, 0, 0))
}

fn game(game_time: &str) -> PerGameStat {
    PerGameStat {
        ship_name: "Yamato".to_owned(),
        ship_id: 4_288_675_536_u64.into(),
        game_time: game_time.to_owned(),
        sort_key: wows_toolkit_viewmodel::stats::sortable_game_time(game_time),
        player_id: 1,
        damage: 80_000,
        spotting_damage: 0,
        frags: 1,
        raw_xp: 1_000,
        base_xp: 700,
        is_win: true,
        is_loss: false,
        is_draw: false,
        is_div: false,
        match_group: "pvp".to_owned(),
        achievements: Vec::new(),
        match_contribution: None,
    }
}

/// The shell `opened` over an install whose replays directory holds `listed` of the fixtures,
/// with game data loaded, a session for a run that opened at [`opened`] and a database of its own.
/// Each fixture landed or listed there reads as a game started when it did.
fn open_install(
    opened_shell: (HeadlessWindow, BrowserControl),
    listed: usize,
) -> (HeadlessWindow, BrowserControl, PathBuf, tempfile::TempDir) {
    let (mut window, control) = opened_shell;
    control.adopt_pinned_folders(PinnedFolders::default());
    control.adopt_folder_history(FolderHistory::default());
    let (dir, wows) = replay_dir(0);
    let replays = wows.join("replays");
    std::fs::create_dir(&replays).expect("the replays directory is made");
    for (name, started) in FIXTURES.iter().zip(STARTED).take(listed) {
        let path = replays.join(name);
        std::fs::copy(fixture(name), &path).expect("the replay is copied");
        simulated::stand_in(path, game(started));
    }
    for (name, started) in FIXTURES.iter().zip(STARTED) {
        simulated::stand_in(replays.join(format!("landed-{name}")), game(started));
    }

    adopt_game_dir(control, &wows);
    let services = control.shell().services();
    services.settings.use_private_database_for_test();
    services.session.choose_scope(SessionScope::Manual);
    services.session.restart_for_test(opened(), Vec::new());
    game_data_ready(control);
    settle_in_real_time(&mut window);

    (window, control, replays, dir)
}

fn session_times(control: BrowserControl, scope: SessionScope) -> Vec<String> {
    control.shell().services().session.game_times(scope)
}

fn choose_since_opened_in_the_listing(window: &mut HeadlessWindow) {
    let chip = window.by_test_id("replay-listing-session").expect("the header shows the session");
    window.click(chip).expect("the session chip is clicked");
    window.advance(Duration::from_millis(300));
    let since = window.by_test_id("replay-listing-session-scope-since-opened").expect("the scope choice is open");
    window.click(since).expect("the scope is chosen");
    settle_in_real_time(window);
}

fn chip_label(window: &mut HeadlessWindow) -> String {
    let chip = window.by_test_id("replay-listing-session").expect("the header shows the session");
    window.a11y(chip).and_then(|state| state.label).unwrap_or_default()
}

#[test]
fn battles_played_in_the_install_join_the_session_and_one_elsewhere_does_not() {
    let (mut window, control, replays, _dir) = open_install(open(), 0);
    let (_other_dir, other) = replay_dir(0);
    control.open_folder(other.clone());
    settle_in_real_time(&mut window);
    control.pin_folder(folder_id(control, &other), true);
    simulated::stand_in(other.join(format!("landed-{}", FIXTURES[2])), game(STARTED[2]));
    settle_in_real_time(&mut window);

    land(&replays, FIXTURES[0], &mut window);
    land(&replays, FIXTURES[1], &mut window);
    land(&other, FIXTURES[2], &mut window);
    settle_in_real_time(&mut window);

    assert_eq!(
        session_times(control, SessionScope::Manual),
        [STARTED[0], STARTED[1]],
        "every battle played in the install joins the saved session; the other folder's does not"
    );
    assert_eq!(
        session_times(control, SessionScope::SinceAppOpened),
        [STARTED[1]],
        "only the battle that started after the app opened is in the session since then"
    );

    // Under the session since the app opened, a battle played leaves the saved session alone.
    choose_since_opened_in_the_listing(&mut window);
    land(&replays, FIXTURES[2], &mut window);
    settle_in_real_time(&mut window);
    assert_eq!(session_times(control, SessionScope::SinceAppOpened), [STARTED[1], STARTED[2]]);
    assert_eq!(session_times(control, SessionScope::Manual), [STARTED[0], STARTED[1]], "the saved session is kept");
    assert_eq!(
        chip_label(&mut window),
        t!("ui.stats.session_chip_since", time = "09:00", battles = t!("ui.stats.session_battles_many", count = 2))
    );
}

#[test]
fn choosing_since_the_app_opened_takes_in_the_replays_already_listed() {
    let (mut window, control, _replays, _dir) = open_install(open(), 3);
    assert!(session_times(control, SessionScope::SinceAppOpened).is_empty(), "nothing is read under Manual");
    assert_eq!(
        chip_label(&mut window),
        t!("ui.stats.session_chip_manual", battles = t!("ui.stats.session_battles_many", count = 0))
    );

    choose_since_opened_in_the_listing(&mut window);

    assert_eq!(
        saved_for_test().iter().rfind(|(key, _)| *key == setting_keys::SESSION_SCOPE).map(|(_, json)| json.as_str()),
        Some("\"SinceAppOpened\""),
        "the scope is saved"
    );
    assert_eq!(
        session_times(control, SessionScope::SinceAppOpened),
        [STARTED[1], STARTED[2]],
        "the listed battles that started after the app opened join; the older one does not"
    );
    assert!(session_times(control, SessionScope::Manual).is_empty(), "the saved session is left alone");
    assert_eq!(
        chip_label(&mut window),
        t!("ui.stats.session_chip_since", time = "09:00", battles = t!("ui.stats.session_battles_many", count = 2))
    );
}

#[test]
fn set_and_add_say_why_they_are_off_in_a_session_since_the_app_opened() {
    let (mut window, _control, _replays, _dir) = open_install(open(), 1);
    choose_since_opened_in_the_listing(&mut window);
    window.press_key(zgui::vocab::NamedKey::Escape, zgui::vocab::Modifiers::NONE);
    window.advance(Duration::from_millis(300));

    let header = window.by_class("wt-row__group-label").expect("the replays are grouped by date");
    window.right_click(header).expect("the group header has a box");
    window.advance(Duration::from_millis(300));
    for key in ["ui.replay.context.set_session_stats_one", "ui.replay.context.add_session_stats_one"] {
        let item = window.by_role_and_label(Role::MenuItem, &t!(key)).unwrap_or_else(|| panic!("the menu lists {key}"));
        assert_eq!(window.a11y(item).map(|state| state.disabled), Some(true), "{key} is off");
    }
    assert!(window.by_test_id("replay-session-unavailable").is_some(), "the menu says why");
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the session control"]
fn the_listing_session_control_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "listing-session-dark"), (Appearance::Light, "listing-session-light")]
    {
        simulated_watch::install();
        let window = open_shell(review_options(appearance));
        let control = MADE.with(std::cell::Cell::get).expect("the replays tab made its browser");
        let (mut window, _control, _replays, _dir) = open_install((window, control), 3);
        choose_since_opened_in_the_listing(&mut window);
        println!("{}", save_shot(&mut window, name).display());
    }
}
