//! The Player Tracker tab driven in the headless shell, with its state seeded through its model.

use std::collections::HashMap;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use wows_replays::ReplayFile;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
use wows_toolkit_viewmodel::match_stats::Region;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentity;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use zgui::reactive::prelude::GetUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::tabs::player_tracker::built_model;
use crate::tabs::player_tracker::model::TrackerModel;
use crate::tabs::player_tracker::roster::sniper_hover_text;

const ARENA_INFO: &str = r#"{
    "gameMode": 7,
    "clientVersionFromExe": "13, 11, 0, 12668706",
    "mapDisplayName": "ocean",
    "mapId": 1,
    "clientVersionFromXml": "13, 11, 0, 12668706",
    "duration": 1200,
    "gameLogic": null,
    "name": "12x12",
    "scenario": "Domination",
    "playerID": 0,
    "vehicles": [{"shipId": 100, "relation": 0, "id": 1, "name": "Harvey635"},
                 {"shipId": 200, "relation": 2, "id": 2, "name": "Stranger99"}],
    "playersPerTeam": 12,
    "dateTime": "28.12.2023 00:52:26",
    "mapName": "spaces/00_CO_ocean",
    "playerName": "Harvey635",
    "scenarioConfigId": 1,
    "teamsCount": 2,
    "logic": null,
    "playerVehicle": "PFSD110-Kleber"
}"#;

/// The shell, open on the Player Tracker tab.
struct TrackerWindow {
    window: HeadlessWindow,
    model: TrackerModel,
}

impl TrackerWindow {
    fn open(options: HeadlessOptions) -> Self {
        let mut window = open_shell(options);
        let tab = window.by_test_id("player-tracker").expect("the strip has a Player Tracker tab");
        window.click(tab).expect("the tab can be pressed");
        // The tests share one settings database, so a filter an earlier test typed is read back
        // here; it is cleared once that read has landed.
        settle_in_real_time(&mut window);
        let model = built_model().expect("the Player Tracker tab was built");
        model.set_filter(String::new());
        window.advance(Duration::from_millis(300));
        Self { window, model }
    }

    fn settle(&mut self) {
        self.window.advance(Duration::from_millis(400));
    }

    fn has(&self, test_id: &str) -> bool {
        self.window.by_test_id(test_id).is_some()
    }

    fn press(&mut self, test_id: &str) {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
        self.settle();
    }

    fn top(&self, test_id: &str) -> f32 {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.bounds(element).expect("a shown row has bounds").origin.y.0
    }

    fn label(&mut self, test_id: &str) -> String {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.a11y(element).and_then(|state| state.label).unwrap_or_else(|| panic!("{test_id} has a label"))
    }

    fn seed_two_players(&mut self) {
        let facet = |id: i64, name: &str, count: i64| PlayerFacet {
            account_id: AccountId(id),
            latest_name: name.to_owned(),
            clan: "WTK".to_owned(),
            match_count: count,
        };
        // Met an hour and three hours ago, so the counts and the Last Encountered column have
        // something to show in the default period.
        let now = jiff::Timestamp::now().as_second();
        let met = |seconds_ago: i64| jiff::Timestamp::from_second(now - seconds_ago).expect("a valid timestamp");
        let tracked: HashMap<AccountId, TrackedPlayer> = [(
            AccountId(7),
            TrackedPlayer {
                last_name: "Harvey635".to_owned(),
                names: ["Harvey635".to_owned(), "Harvey42".to_owned()].into_iter().collect(),
                timestamps: [met(60 * 60), met(3 * 60 * 60)].into_iter().collect(),
                arena_ids: [ArenaId::new(1), ArenaId::new(2)].into_iter().collect(),
                ..TrackedPlayer::new(AccountId(7))
            },
        )]
        .into_iter()
        .collect();
        self.model.seed(vec![facet(7, "Harvey635", 3), facet(8, "Abel", 1)], tracked);
        self.settle();
    }

    fn show_current_match(&mut self) {
        let label = rust_i18n::t!("ui.player_tracker.subtab_current_match").into_owned();
        let tab = self.window.by_role_and_label(Role::Tab, &label).expect("the dock lists the Current Match section");
        self.window.click(tab).expect("the section's tab can be pressed");
        self.settle();
    }

    fn seed_battle(&mut self) -> LiveMatch {
        let replay = ReplayFile::from_decrypted_parts(ARENA_INFO.as_bytes().to_vec(), Vec::new())
            .expect("the arena info parses");
        let live = LiveMatch::from_meta(&replay.meta).expect("the arena info carries a date");
        self.model.live.seed(live.clone());
        self.settle();
        live
    }

    /// Names the ally Harvey635 as account 4242 and answers the lookup for him.
    fn seed_harvey_stats(&mut self) {
        let account = AccountId(4242);
        let identity = LiveIdentity {
            account_id: account,
            region: Some(Region::Eu),
            clan: Some("WTK".to_owned()),
            clan_color: None,
        };
        let identities = LiveIdentities { by_name: [("harvey635".to_owned(), identity)].into_iter().collect() };
        let stats = vec![PlayerStatsOut {
            account_id: account,
            region: "eu".to_owned(),
            ship_id: 100u64.into(),
            status: PlayerStatsStatus::Ok,
            battles: Some(1234),
            overall_win_rate: Some(54.25),
            overall_avg_damage: None,
            ship_win_rate: None,
            ship_battles: None,
            ship_avg_damage: None,
            ship_pr: None,
            pr: Some(1650.0),
        }];
        self.model.live.seed_stats(identities, stats);
        self.settle();
    }
}

#[test]
fn the_chip_hover_names_each_login_and_its_sightings() {
    let hover = sniper_hover_text(&[
        SniperCandidate { login: "harvey635".to_owned(), minutes: vec![-1, 5] },
        SniperCandidate { login: "harvey_635".to_owned(), minutes: vec![3] },
    ]);
    assert!(hover.contains("harvey635") && hover.contains("harvey_635"), "every login is named: {hover}");
    assert!(hover.contains("-1, 5") && hover.contains('3'), "with the minutes each was seen: {hover}");
}

#[test]
fn clicking_a_column_sorts_by_it_and_clicking_again_reverses() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    tracker.seed_two_players();

    tracker.press("tracker-sort-Name");
    let (harvey, abel) = (tracker.top("tracker-player-7"), tracker.top("tracker-player-8"));
    let ascending = abel < harvey;
    tracker.press("tracker-sort-Name");
    let (harvey, abel) = (tracker.top("tracker-player-7"), tracker.top("tracker-player-8"));
    assert_eq!(abel < harvey, !ascending, "the second press reverses the order");
}

#[test]
fn the_filter_narrows_the_rows_to_matching_names() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    tracker.seed_two_players();

    tracker.press("tracker-filter");
    tracker.window.type_text("abe");
    tracker.settle();
    assert!(tracker.has("tracker-player-8"), "the matching player stays");
    assert!(!tracker.has("tracker-player-7"), "the other goes");

    tracker.window.press_key(NamedKey::Escape, Modifiers::NONE);
    tracker.settle();
    assert!(tracker.has("tracker-player-7"), "Escape empties the filter and the other comes back");
}

/// A kept row marks what the filter matched in its name or its tag, and leaves the other plain.
#[test]
fn the_filter_marks_names_and_tags() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    tracker.seed_two_players();

    tracker.model.set_filter("abe".to_owned());
    tracker.settle();
    let marked = tracker.window.texts_by_class(landaire_ui::MARK_CLASS);
    assert!(marked.contains(&"Abe".to_owned()), "{marked:?}");

    tracker.model.set_filter("wtk".to_owned());
    tracker.settle();
    let marked = tracker.window.texts_by_class(landaire_ui::MARK_CLASS);
    assert_eq!(marked.iter().filter(|piece| *piece == "WTK").count(), 2, "kept by the tag: {marked:?}");
    assert!(!marked.contains(&"Abe".to_owned()), "the names are plain");
}

/// The words beside the division mates box tick and untick it, as the box itself does.
#[test]
fn the_division_mates_label_toggles_its_box() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let before = tracker.model.show_division_mates.get_untracked();

    tracker.press("tracker-show-division-mates-label");
    assert_eq!(tracker.model.show_division_mates.get_untracked(), !before, "the label ticks the box");
    tracker.press("tracker-show-division-mates-label");
    assert_eq!(tracker.model.show_division_mates.get_untracked(), before, "and unticks it");
}

#[test]
fn a_historical_row_opens_on_its_aliases_and_account_id() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    tracker.seed_two_players();
    assert!(!tracker.has("tracker-player-detail-7"), "rows start closed");

    tracker.press("tracker-player-expand-7");
    assert!(tracker.has("tracker-player-detail-7"), "the caret opens the row");
    assert!(tracker.has("tracker-account-id-7"), "the account id is offered to copy");
    let mut window = tracker.window;
    assert!(
        window.by_label(&rust_i18n::t!("ui.player_tracker.aliases_hover", names = "Harvey42")).is_some(),
        "the alias is listed"
    );
}

#[test]
fn a_players_note_opens_from_its_row_and_keeps_what_is_typed() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    tracker.seed_two_players();

    tracker.press("tracker-note-8");
    assert!(tracker.has("tracker-player-detail-8"), "writing a note opens the row it belongs to");
    tracker.press("tracker-note-input");
    tracker.window.type_text("camps the cap");
    tracker.window.press_key(NamedKey::Enter, Modifiers::NONE);
    tracker.settle();
    assert_eq!(tracker.model.note_for(AccountId(8)), "camps the cap", "Enter keeps the note");
}

#[test]
fn finding_a_players_matches_opens_the_search_tab() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    tracker.seed_two_players();

    tracker.press("tracker-find-player-7");
    assert!(tracker.has("search-tab"), "the Search tab is shown with the query");
}

#[test]
fn a_scanned_identity_joins_its_player_to_the_looked_up_statistics() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    tracker.seed_battle();
    tracker.show_current_match();
    assert!(tracker.has("tracker-roster-ally-0"), "the battle's roster is listed");
    assert!(!tracker.has("tracker-roster-ally-0-status-0-pr"), "no account, no figures");

    tracker.seed_harvey_stats();

    let mut window = tracker.window;
    assert!(window.by_label("[WTK] Harvey635").is_some(), "the scan's clan tag reaches the name");
    assert!(window.by_test_id("tracker-roster-ally-0-status-0-pr").is_some(), "the rating chip is drawn");
}

#[test]
fn a_chat_login_that_names_a_player_puts_a_chip_on_their_row() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let live = tracker.seed_battle();
    tracker.show_current_match();
    assert!(!tracker.has("tracker-twitch-ally-0"), "no observations, no chip");

    tracker
        .model
        .live
        .seed_chat(vec![("harvey_635".to_owned(), live.started_at), ("someoneelse".to_owned(), live.started_at)]);
    tracker.settle();
    assert!(tracker.has("tracker-twitch-ally-0"), "the login that names the ally chips them");
    assert!(!tracker.has("tracker-twitch-enemy-0"), "the enemy nobody named keeps no chip");

    let long_after = jiff::Timestamp::from_second(live.started_at.as_second() + 60 * 60).expect("a valid timestamp");
    tracker.model.live.seed_chat(vec![("harvey_635".to_owned(), long_after)]);
    tracker.settle();
    assert!(!tracker.has("tracker-twitch-ally-0"), "an hour later is not this battle");

    // Copying one of two logins that both name the ally would be a guess, so the chip opens a
    // picker instead.
    tracker
        .model
        .live
        .seed_chat(vec![("harvey_635".to_owned(), live.started_at), ("harvey635x".to_owned(), live.started_at)]);
    tracker.settle();
    assert!(!tracker.has("tracker-twitch-pick-ally-0-0"), "the picker opens from the chip");
    tracker.press("tracker-twitch-ally-0");
    assert_eq!(
        tracker.label("tracker-twitch-pick-ally-0-0"),
        "harvey635x",
        "the picker lists the candidates alphabetically"
    );
    assert_eq!(tracker.label("tracker-twitch-pick-ally-0-1"), "harvey_635");
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn the_tracker_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let mut tracker = TrackerWindow::open(review_options(appearance));
        tracker.seed_two_players();
        tracker.press("tracker-player-expand-7");
        println!("{}", save_shot(&mut tracker.window, &format!("tracker-players-{name}")).display());

        let live = tracker.seed_battle();
        tracker.seed_harvey_stats();
        tracker.model.live.seed_chat(vec![("harvey_635".to_owned(), live.started_at)]);
        tracker.show_current_match();
        println!("{}", save_shot(&mut tracker.window, &format!("tracker-live-{name}")).display());
    }
}
