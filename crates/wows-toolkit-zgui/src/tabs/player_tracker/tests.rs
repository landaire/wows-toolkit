//! The Player Tracker tab driven in the headless shell, with its state seeded through its model.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::FoldingRegion;
use landaire_ui::RegionExtent;
use landaire_ui::SavedLayout;
use landaire_ui::Share;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use sqlx::sqlite::SqlitePool;
use tokio::runtime::Runtime;
use wows_replays::ReplayFile;
use wows_replays::analyzer::arena_scan::ArenaState;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::VehicleRelation;
use wows_toolkit_config::queries;
use wows_toolkit_replay_data::live_match::ARENA_INFO_FILE;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::match_stats::PlayerStatsOut;
use wows_toolkit_viewmodel::match_stats::PlayerStatsStatus;
use wows_toolkit_viewmodel::match_stats::Region;
use wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentities;
use wows_toolkit_viewmodel::player_tracker::live::LiveIdentity;
use wows_toolkit_viewmodel::player_tracker::live::LiveMatch;
use wows_toolkit_viewmodel::player_tracker::live::StatsState;
use wows_toolkit_viewmodel::player_tracker::store;
use wows_toolkit_viewmodel::player_tracker::tracked;
use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
use wows_toolkit_viewmodel::twitch::SniperCandidate;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::WithUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use crate::interaction_tests::REVIEW_SIZE;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::returning_reader;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::interaction_tests::shell;
use crate::services::GameDataRequest;
use crate::tabs::player_tracker::built_dock;
use crate::tabs::player_tracker::built_model;
use crate::tabs::player_tracker::layout::DOCK_LAYOUT_KEY;
use crate::tabs::player_tracker::layout::allow_persistence;
use crate::tabs::player_tracker::live::take_stats_lookups;
use crate::tabs::player_tracker::model::TrackerModel;
use crate::tabs::player_tracker::roster::sniper_hover_text;

pub(crate) const ARENA_INFO: &str = r#"{
    "gameMode": 7,
    "matchGroup": "pvp",
    "gameType": "RandomBattle",
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

/// The ally's rating chip in each scope, and the caption Detailed labels the ship line with.
const OVERALL_PR: &str = "tracker-roster-ally-0-status-0-pr";
const SHIP_PR: &str = "tracker-roster-ally-0-status-1-pr";
const SHIP_STATS: &str = "tracker-roster-ally-0-ship-stats";

/// Held by the tests that save the view settings: they share one database, and a test that opens
/// the tab reads back what another saved.
static VIEW_MODES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The settings database the tests share, read and written outside any window.
struct SavedView {
    runtime: Runtime,
    pool: SqlitePool,
}

impl SavedView {
    fn open() -> Self {
        let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
        returning_reader(&runtime);
        let pool = runtime.block_on(wows_toolkit_config::open_db()).expect("the settings database opens");
        Self { runtime, pool }
    }

    fn mode(&self) -> CurrentMatchViewMode {
        self.runtime.block_on(store::load_view_modes(&self.pool)).current_match_view_mode
    }

    fn set(&self, mode: CurrentMatchViewMode) {
        let write = queries::set_setting(&self.pool, tracked::CURRENT_MATCH_VIEW_MODE_KEY, &mode);
        self.runtime.block_on(write).expect("the view is saved");
    }

    /// Runs `window`'s frames until the database holds `mode`: the tab saves on the tokio runtime.
    fn wait_for(&self, window: &mut HeadlessWindow, mode: CurrentMatchViewMode) {
        for _ in 0..100 {
            if self.mode() == mode {
                return;
            }
            window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("the tab did not save {mode:?}");
    }
}

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

    fn left(&self, test_id: &str) -> f32 {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.bounds(element).expect("a shown row has bounds").origin.x.0
    }

    fn height(&self, test_id: &str) -> f32 {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.bounds(element).expect("a shown row has bounds").size.height.0
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
            overall_avg_damage: Some(48_210),
            ship_win_rate: Some(61.0),
            ship_battles: Some(120),
            ship_avg_damage: Some(83_266),
            ship_pr: Some(2834.0),
            pr: Some(1650.0),
        }];
        self.model.live.seed_stats(identities, stats);
        self.settle();
    }

    /// The tab on a battle with the ally's figures in, open on Current Match.
    fn open_on_battle() -> Self {
        let mut tracker = Self::open(HeadlessOptions::new(REVIEW_SIZE));
        tracker.seed_battle();
        tracker.show_current_match();
        tracker.seed_harvey_stats();
        tracker
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
    let _modes = VIEW_MODES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

/// Detailed stacks the ship's figures under the account's on a second line, as the egui app does;
/// Compact shows the selected scope alone on one line.
#[test]
fn detailed_stacks_ship_figures_under_overall_and_compact_shows_one_scope() {
    let _modes = VIEW_MODES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut tracker = TrackerWindow::open_on_battle();

    tracker.press("tracker-view-mode-Detailed");
    tracker.press("tracker-win-rate-mode-Overall");
    assert!(tracker.has(SHIP_STATS), "Detailed labels the ship line");
    assert!(tracker.has(OVERALL_PR) && tracker.has(SHIP_PR), "and shows both scopes");
    assert_eq!(tracker.left(SHIP_PR), tracker.left(OVERALL_PR), "in the same column");
    assert!(tracker.top(SHIP_PR) > tracker.top(OVERALL_PR), "the ship's under the account's");
    let detailed = tracker.height("tracker-roster-ally-0");

    tracker.press("tracker-view-mode-Compact");
    assert!(!tracker.has(SHIP_STATS), "Compact has no ship line");
    assert!(tracker.has(OVERALL_PR) && !tracker.has(SHIP_PR), "only the selected scope");
    let compact = tracker.height("tracker-roster-ally-0");
    assert!(compact < detailed, "a Compact row ({compact}) is one line, a Detailed row ({detailed}) two");

    tracker.press("tracker-win-rate-mode-Ship");
    assert!(!tracker.has(OVERALL_PR) && tracker.has(SHIP_PR), "the scope selector picks the figures");

    // The scope still colours the rows and picks the team averages, so Detailed keeps its selector.
    tracker.press("tracker-view-mode-Detailed");
    assert!(tracker.has("tracker-win-rate-mode-Overall"), "Detailed keeps the scope selector");
    assert!(tracker.has(OVERALL_PR) && tracker.has(SHIP_PR), "and still shows both scopes");
}

/// The view chosen is saved where the egui app keeps its own, and the tab opens on it.
#[test]
fn the_current_match_opens_on_the_saved_view() {
    let _modes = VIEW_MODES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    let saved = SavedView::open();

    for (mode, other, detailed) in [
        (CurrentMatchViewMode::Compact, CurrentMatchViewMode::Detailed, false),
        (CurrentMatchViewMode::Detailed, CurrentMatchViewMode::Compact, true),
    ] {
        saved.set(other);
        let mut chooser = TrackerWindow::open_on_battle();
        chooser.press(&format!("tracker-view-mode-{mode:?}"));
        saved.wait_for(&mut chooser.window, mode);
        drop(chooser);

        let tracker = TrackerWindow::open_on_battle();
        assert_eq!(tracker.model.view_mode.get_untracked(), mode, "the saved view is read back");
        assert_eq!(tracker.has(SHIP_STATS), detailed, "and drawn: {mode:?}");
    }
}

#[test]
fn the_dock_layout_is_saved_and_put_back() {
    let _persists = allow_persistence();
    let saved = SavedView::open();
    let extent = RegionExtent::Share(Share::new(31.0).expect("a valid share"));

    let mut first = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let dock = built_dock().expect("the tab built its dock");
    dock.set_region_extent(FoldingRegion::Left, extent);
    let stored = || {
        let read = queries::try_get_setting::<SavedLayout>(&saved.pool, DOCK_LAYOUT_KEY);
        saved.runtime.block_on(read).expect("the layout row reads")
    };
    for _ in 0..100 {
        if stored().is_some_and(|layout| layout.extents.left == extent) {
            break;
        }
        first.window.advance(Duration::from_millis(100));
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(stored().map(|layout| layout.extents.left), Some(extent), "the change is saved");
    drop(first);

    let _second = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let dock = built_dock().expect("the tab built its dock again");
    assert_eq!(dock.region_extent(FoldingRegion::Left), extent, "the saved layout is put back");
    assert_eq!(dock.panels_untracked().len(), 3, "with every section");
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

/// Takes a game directory whose replays directory holds `files` as the settings' own, without
/// saving it, and reloads the game data from it. Nothing is installed there, so no build loads.
fn adopt_install(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory can be made");
    let wows = std::fs::canonicalize(dir.path()).expect("the directory resolves").join("wows");
    let replays = wows.join("replays");
    std::fs::create_dir_all(&replays).expect("the replays directory is made");
    for (name, bytes) in files {
        std::fs::write(replays.join(name), bytes).expect("the file is written");
    }

    let services = shell().services();
    let pool = services.settings.pool().expect("the settings database is open");
    let runtime = services.runtime().expect("the runtime is up");
    let mut settings = runtime.block_on(StartupSettings::load(&pool));
    settings.wows_dir = wows.display().to_string();
    services.settings.adopt_unsaved(settings);
    services.game_data.adopt(GameDataRequest {
        wows_dir: Some(wows),
        cache_dir: String::new(),
        locale: None,
        auto_dump: false,
    });

    dir
}

/// Runs frames in real time until the shell's live watch reports a battle.
fn wait_for_battle(window: &mut HeadlessWindow) -> LiveMatch {
    for _ in 0..10 {
        if let Some(live) = shell().live.live_match.get_untracked() {
            return live;
        }
        settle_in_real_time(window);
    }
    panic!("the live watch did not report the battle");
}

/// The shell watches for a battle from startup: one under way is found and its roster offered for
/// a lookup without the Player Tracker being opened, and the lookup is sent only once the Current
/// Match section is shown.
#[test]
fn the_live_watch_runs_without_the_tab_and_looks_up_stats_once_shown() {
    let mut window = open_shell(HeadlessOptions::new(REVIEW_SIZE));
    take_stats_lookups();
    let _install = adopt_install(&[(ARENA_INFO_FILE, ARENA_INFO.as_bytes())]);

    let live = wait_for_battle(&mut window);
    let names: Vec<&str> = live.players.iter().map(|player| player.name.as_str()).collect();
    assert_eq!(names, ["Harvey635", "Stranger99"], "the battle's roster is read");
    assert!(window.by_test_id("player-tracker-tab").is_none(), "the tab was never built");

    let arena = ArenaId::new(4242);
    shell().live.seed_scan(shell().services(), ArenaState { arena_id: arena, players: Vec::new() });
    settle_in_real_time(&mut window);
    assert!(take_stats_lookups().is_empty(), "no lookup while the section has never been shown");
    assert!(matches!(shell().live.stats.get_untracked(), StatsState::Held), "the lookup is held");

    let tab = window.by_test_id("player-tracker").expect("the strip has a Player Tracker tab");
    window.click(tab).expect("the tab can be pressed");
    settle_in_real_time(&mut window);
    assert!(take_stats_lookups().is_empty(), "the tab opens on the players, which leaves the lookup held");

    let mut tracker = TrackerWindow { window, model: built_model().expect("the Player Tracker tab was built") };
    tracker.show_current_match();
    settle_in_real_time(&mut tracker.window);
    assert_eq!(take_stats_lookups(), [arena], "showing the section sends the held lookup once");
    assert!(matches!(tracker.model.live.stats.get_untracked(), StatsState::Ready(_)), "and its answer is shown");
}

/// In debug mode the Current Match section offers the replays in the watched directory, and the
/// one chosen is taken as the battle in progress. Outside debug mode it is not offered.
#[test]
fn the_debug_replay_picker_shows_in_debug_mode_and_seeds_the_battle() {
    const REPLAY: &str = "10000_1736020571_20250101_215420_PVSB018-Ipiranga_41_Conquest.wowsreplay";
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wows-replay-insights/tests/fixtures/replays").join(REPLAY);
    let bytes = std::fs::read(&fixture).expect("the fixture reads");
    let meta = ReplayFile::meta_from_file(&fixture).expect("the fixture's header reads");
    let expected = LiveMatch::from_meta(&meta).expect("the fixture carries a date");

    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let _install = adopt_install(&[(REPLAY, &bytes)]);
    settle_in_real_time(&mut tracker.window);
    tracker.show_current_match();
    assert!(!tracker.has("tracker-debug-replay"), "the picker is hidden outside debug mode");

    shell().debug_mode.set(true);
    // The directory is listed off the UI thread.
    settle_in_real_time(&mut tracker.window);
    assert!(tracker.has("tracker-debug-replay"), "debug mode shows the picker");
    assert!(shell().live.live_match.get_untracked().is_none(), "no battle is under way");

    tracker.press("tracker-debug-replay-trigger");
    tracker.press("tracker-debug-replay-0");
    settle_in_real_time(&mut tracker.window);
    let live = shell().live.live_match.get_untracked().expect("the chosen replay is taken as the battle");
    assert_eq!(live.started_at, expected.started_at);
    assert_eq!(live.players.len(), expected.players.len());
    assert!(tracker.has("tracker-roster-ally-0"), "its roster is listed");

    shell().debug_mode.set(false);
    tracker.settle();
    assert!(!tracker.has("tracker-debug-replay"), "leaving debug mode hides the picker");
}

/// One battle in the shared index: `recorder` in division 1 with `mate`, against `enemy` of clan
/// `clan`, fought five minutes ago so every period shows it.
fn index_division_battle(
    arena_id: ArenaId,
    recorder: AccountId,
    mate: AccountId,
    enemy: AccountId,
    clan: &str,
) -> jiff::Timestamp {
    let pool = shell().services().settings.pool().expect("the settings database is open");
    let runtime = shell().services().runtime().expect("the shell has a runtime");
    let at = jiff::Timestamp::from_second(jiff::Timestamp::now().as_second() - 5 * 60).expect("a valid timestamp");
    let vehicle = |account: AccountId, relation: VehicleRelation, division_id: Option<i64>| IndexedVehicleRow {
        arena_id,
        account_id: account,
        player_name: format!("Player{}", account.raw()),
        clan: clan.to_owned(),
        realm: None,
        ship_id: 100u64.into(),
        ship_index: "PFSD110".into(),
        ship_name: "Kleber".into(),
        nation: "france".into(),
        species: "Destroyer".into(),
        tier: 10,
        relation,
        division_id,
        survived: None,
        damage: None,
        kills: None,
        spotting: None,
        potential: None,
        received: None,
        pr: None,
        mc: None,
        mc_version: None,
        mc_torpedoes_observed: false,
        is_test_ship: false,
        disconnected: None,
        is_stream_sniper: None,
        sniper_twitch_login: None,
    };
    let vehicles = [
        vehicle(recorder, VehicleRelation::SelfPlayer, Some(1)),
        vehicle(mate, VehicleRelation::Ally, Some(1)),
        vehicle(enemy, VehicleRelation::Enemy, None),
    ];

    runtime.block_on(async move {
        let root = std::path::Path::new("C:/tracker-populate/replays");
        let source = query::ensure_default_source(&pool, root, at).await.expect("a source is recorded");
        let objective = ObjectiveMatch {
            arena_id,
            timestamp: at,
            map: "Ocean".into(),
            game_mode: "Domination".into(),
            game_mode_id: None,
            game_type: "pvp".into(),
            match_group: "pvp".into(),
            version_build: None,
        };
        query::upsert_match(&pool, &objective).await.expect("a match is recorded");
        query::upsert_vehicles(&pool, &vehicles).await.expect("the roster is recorded");
        let record = ReplayRecord {
            arena_id,
            source_id: source,
            replay_path: root.join(format!("{}.wowsreplay", arena_id.raw())),
            file_mtime: None,
            outcome: MatchOutcome::Win,
            self_account_id: Some(recorder),
            self_ship_id: None,
            self_survived: None,
            self_damage: None,
            self_kills: None,
            self_pr: None,
            results_available: true,
            indexed_at: at,
        };
        query::upsert_record(&pool, &record).await.expect("a record is recorded");
    });

    at
}

/// Populate records the index's battles into the tracker: the Players table counts and dates the
/// opponent, the Clans table lists their clan, the division mate is marked, and the recorder is
/// left out.
#[test]
fn populate_fills_the_tables_from_the_index() {
    let mut tracker = TrackerWindow::open(HeadlessOptions::new(REVIEW_SIZE));
    let (recorder, mate, enemy) = (AccountId(970_001), AccountId(970_002), AccountId(970_003));
    let arena = ArenaId::new(97_000_001);
    let at = index_division_battle(arena, recorder, mate, enemy, "PPLT");

    tracker.press("tracker-repopulate");
    let mut row = None;
    for _ in 0..10 {
        row = tracker.model.rows.get_untracked().into_iter().find(|row| row.facet.account_id == enemy);
        if row.as_ref().is_some_and(|row| row.total_encounters.is_some()) {
            break;
        }
        settle_in_real_time(&mut tracker.window);
    }

    let row = row.expect("the opponent has a row");
    assert_eq!(row.total_encounters, Some(1), "Total counts the battle");
    assert_eq!(row.last_seen, Some(at), "Last seen is the battle's time");
    assert!(tracker.has(&format!("tracker-player-{}", enemy.raw())), "the row is shown");
    let clan = tracker.model.clans.get_untracked().into_iter().find(|clan| clan.clan == "PPLT");
    assert_eq!(clan.map(|clan| clan.matches), Some(1), "the Clans table counts the opponent's clan");
    tracker.model.tracked.with_untracked(|tracked| {
        assert!(!tracked.contains_key(&recorder), "the recorder is not someone they met");
        assert!(tracked[&mate].arena_in_division(arena), "the division mate is marked");
        assert!(!tracked[&enemy].arena_in_division(arena));
    });
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn the_tracker_renders_in_dark_and_light() {
    let _modes = VIEW_MODES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (appearance, name) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let mut tracker = TrackerWindow::open(review_options(appearance));
        tracker.seed_two_players();
        tracker.press("tracker-player-expand-7");
        println!("{}", save_shot(&mut tracker.window, &format!("tracker-players-{name}")).display());

        let live = tracker.seed_battle();
        tracker.seed_harvey_stats();
        tracker.model.live.seed_chat(vec![("harvey_635".to_owned(), live.started_at)]);
        tracker.show_current_match();
        for mode in CurrentMatchViewMode::ALL {
            tracker.press(&format!("tracker-view-mode-{mode:?}"));
            println!("{}", save_shot(&mut tracker.window, &format!("tracker-live-{mode:?}-{name}")).display());
        }
    }
}
