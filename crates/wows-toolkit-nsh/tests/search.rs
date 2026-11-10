//! Headless behavior of the Search tab and the command palette over the real
//! TSX bundle.

use std::ops::{Deref, DerefMut};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use nsh_core::{Points, Size};

const FILE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
const QUERY: &str = "label:Replay search query";
const GONE: Duration = Duration::from_secs(5);

struct TestApp {
    app: nsh_testing::Headless,
    _guard: MutexGuard<'static, ()>,
}

impl Deref for TestApp {
    type Target = nsh_testing::Headless;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl DerefMut for TestApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

static TEST_APP_LOCK: Mutex<()> = Mutex::new(());

static TEST_STORAGE: LazyLock<std::path::PathBuf> = LazyLock::new(|| {
    let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir(&path).expect("the test settings directory is created");
    path
});

fn configure_test_storage() {
    wows_toolkit_config::storage_override::set(TEST_STORAGE.clone());
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

#[cfg(feature = "buck")]
fn start() -> nsh_testing::Headless {
    let path = std::env::var("NSH_BUNDLE_PATH").expect("Buck provides the generated TSX bundle");
    let source = std::fs::read_to_string(path).expect("the generated TSX bundle is readable");
    let config = nsh_engine::EngineConfig {
        ui_sets: registries(),
        ..nsh_engine::EngineConfig::new(nsh_engine::Bundle::source(source))
    };
    nsh_testing::Headless::start(config).expect("the TSX app starts")
}

#[cfg(not(feature = "buck"))]
fn start() -> nsh_testing::Headless {
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    nsh_testing::tsx(&entry, registries()).expect("the TSX app starts")
}

/// The app over an index holding one battle, with no saved Search state.
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    seed_index();
    let mut app = start();
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

/// [`app`] as a Linux desktop, which has a mouse to right-click and hover
/// with.
fn desktop_app() -> TestApp {
    let mut app = app();
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::Linux;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .expect("the desktop environment is applied");
    app
}

fn replay() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FILE)
}

fn seed_index() {
    let indexed_replay = replay();
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            use std::path::Path;

            use wows_core::game_types::{AccountId, ArenaId, GameParamId};
            use wows_toolkit_config::index::rows::{
                IndexedVehicleRow, MatchOutcome, ObjectiveMatch, ReplayRecord, VehicleRelation,
            };

            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            for table in ["replay_record", "indexed_vehicle", "indexed_match"] {
                sqlx::query(&format!("DELETE FROM {table}")).execute(&pool).await.expect("old rows are cleared");
            }
            sqlx::query("DELETE FROM settings WHERE key IN ('search', 'search_column_widths')")
                .execute(&pool)
                .await
                .expect("old Search state is cleared");
            let timestamp = jiff::Timestamp::now();
            let arena_id = ArenaId::new(982_002);
            let source =
                wows_toolkit_config::index::query::ensure_default_source(&pool, Path::new("/tmp/replays"), timestamp)
                    .await
                    .expect("the test replay source exists");
            wows_toolkit_config::index::query::upsert_match(
                &pool,
                &ObjectiveMatch {
                    arena_id,
                    timestamp,
                    map: "Ocean".into(),
                    game_mode: "Domination".into(),
                    game_mode_id: None,
                    game_type: "pvp".into(),
                    match_group: "pvp".into(),
                    version_build: Some(1234),
                },
            )
            .await
            .expect("the searchable match is written");
            wows_toolkit_config::index::query::upsert_vehicles(
                &pool,
                &[IndexedVehicleRow {
                    arena_id,
                    account_id: AccountId(99_010),
                    player_name: "SearchCaptain".into(),
                    clan: "TEST".into(),
                    realm: Some("na".into()),
                    ship_id: GameParamId::from(999u64),
                    ship_index: "PJSD018".into(),
                    ship_name: "Harugumo".into(),
                    nation: "japan".into(),
                    species: "Destroyer".into(),
                    tier: 10,
                    relation: VehicleRelation::SelfPlayer,
                    division_id: None,
                    survived: Some(true),
                    damage: Some(123_456),
                    kills: Some(2),
                    spotting: Some(10_000),
                    potential: Some(500_000),
                    received: Some(5_000),
                    pr: Some(1500.0),
                    is_test_ship: false,
                    disconnected: Some(false),
                    is_stream_sniper: None,
                    sniper_twitch_login: None,
                }],
            )
            .await
            .expect("the searchable roster is written");
            wows_toolkit_config::index::query::upsert_record(
                &pool,
                &ReplayRecord {
                    arena_id,
                    source_id: source,
                    replay_path: indexed_replay,
                    file_mtime: Some(timestamp.as_second()),
                    outcome: MatchOutcome::Win,
                    self_account_id: Some(AccountId(99_010)),
                    self_ship_id: Some(GameParamId::from(999u64)),
                    self_survived: Some(true),
                    self_damage: Some(123_456),
                    self_kills: Some(2),
                    self_pr: Some(1500.0),
                    results_available: true,
                    indexed_at: timestamp,
                },
            )
            .await
            .expect("the searchable replay record is written");
        },
    );
}

#[test]
fn a_typed_query_finds_and_opens_an_indexed_replay() {
    let mut app = app();

    app.press("Search").unwrap();
    // The tab opens on the saved query, which is empty: every indexed battle.
    app.wait_for("Harugumo").unwrap();
    app.type_text(QUERY, "map:Ocean outcome:win").unwrap();
    app.submit(QUERY).unwrap();
    app.wait_for("id:search-pill-0").unwrap();
    app.wait_for("123,456").unwrap();
    app.wait_for("1 match(es)").unwrap();

    app.press(&format!("label:Open replay {}", replay().display())).unwrap();
    app.wait_for("Players (15)").unwrap();
}

#[test]
fn a_query_that_does_not_parse_is_reported_above_the_results() {
    let mut app = app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.type_text(QUERY, "outcome=win and").unwrap();
    app.press("Run Search").unwrap();
    app.wait_for("text:/^That query did not parse/").unwrap();
    assert!(app.shows("Harugumo"), "the results the last good query found stay");
}

#[test]
fn the_results_follow_a_typed_query_without_enter() {
    let mut app = app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.type_text(QUERY, "damage:>").unwrap();
    app.wait_for("text:/^That query did not parse/").unwrap();
}

#[test]
fn a_finished_term_becomes_a_pill_and_backspace_takes_it_back() {
    let mut app = app();

    app.press("Search").unwrap();
    app.type_text(QUERY, "outcome=win").unwrap();
    assert!(!app.screen().matches("id:search-pill-0").unwrap(), "a term being typed is text");
    app.submit(QUERY).unwrap();
    app.wait_for("id:search-pill-0").unwrap();
    assert_eq!(app.screen().input_text(QUERY).unwrap(), Some(""), "the box is clear for the next term");

    app.press_key("backspace").unwrap();
    app.wait_for_gone("id:search-pill-0", GONE).unwrap();
}

#[test]
fn the_bar_offers_completions_and_takes_them() {
    use wows_toolkit_viewmodel::query_bar::suggest::SuggestionKind;

    // A field rather than a preset: taking it leaves the field's operator for
    // a value to be typed after.
    let first = wows_toolkit_viewmodel::query_bar::suggest::static_suggestions()
        .into_iter()
        .find(|suggestion| matches!(suggestion.kind, SuggestionKind::MatchField(_)))
        .expect("there are field suggestions")
        .label
        .clone();
    let needle: String = first.chars().take(3).collect();
    let mut app = app();

    app.press("Search").unwrap();
    assert!(!app.screen().matches("id:search-completions").unwrap(), "nothing is offered before typing");
    app.type_text(QUERY, &needle).unwrap();
    app.wait_for("id:search-completion-0").unwrap();
    assert!(app.screen().matches(&format!("label:{first}")).unwrap(), "the matching suggestion is offered");

    app.press(&format!("label:{first}")).unwrap();
    let taken = app.screen().input_text(QUERY).unwrap().unwrap_or_default().to_owned();
    assert_ne!(taken, first, "what lands is the grammar, not the phrase");
    assert!(taken.ends_with(['=', ':', '>', '<']), "a field completion leaves its operator for a value: {taken:?}");
}

#[test]
fn a_date_field_opens_a_calendar() {
    let mut app = app();

    app.press("Search").unwrap();
    app.type_text(QUERY, "map:").unwrap();
    assert!(!app.screen().matches("id:search-calendar").unwrap(), "a map is not a date");
    app.type_text(QUERY, "Ocean date>").unwrap();
    app.wait_for("id:search-calendar").unwrap();
    assert!(!app.screen().matches("id:search-completions").unwrap(), "the value list stands aside");
}

#[test]
fn a_pill_menu_negates_and_undo_restores() {
    let mut app = desktop_app();

    app.press("Search").unwrap();
    app.type_text(QUERY, "outcome=win").unwrap();
    app.submit(QUERY).unwrap();
    app.wait_for("id:search-pill-0").unwrap();
    let before: Vec<String> = app.screen().texts().into_iter().map(str::to_owned).collect();

    app.right_click("id:search-pill-0").unwrap();
    app.choose_menu_item("Negate").unwrap();
    app.wait_until("the negated pill", GONE, |screen| screen.texts().into_iter().map(str::to_owned).collect::<Vec<_>>() != before)
        .unwrap();

    app.press("Undo the last change to the query").unwrap();
    app.wait_until("the original pill", GONE, |screen| screen.texts().into_iter().map(str::to_owned).collect::<Vec<_>>() == before)
        .unwrap();
}

#[test]
fn up_recalls_the_queries_that_were_run() {
    let mut app = app();

    app.press("Search").unwrap();
    app.type_text(QUERY, "outcome:win").unwrap();
    app.submit(QUERY).unwrap();
    app.wait_for("id:search-pill-0").unwrap();
    app.press_key("backspace").unwrap();
    app.wait_for_gone("id:search-pill-0", GONE).unwrap();
    app.type_text(QUERY, "map:Ocean").unwrap();
    app.submit(QUERY).unwrap();
    app.press_key("backspace").unwrap();
    app.wait_for_gone("id:search-pill-0", GONE).unwrap();

    app.press_key("up").unwrap();
    app.wait_for("label:/^Filter .*Ocean/").unwrap();
    app.press_key("up").unwrap();
    app.wait_for_gone("label:/^Filter .*Ocean/", GONE).unwrap();
    app.wait_for("id:search-pill-0").unwrap();
}

#[test]
fn a_header_press_sorts_by_its_column() {
    let mut app = app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    assert!(app.screen().selected("id:search-sort-date").unwrap(), "date is the opening sort");
    app.press("id:search-sort-map").unwrap();
    app.wait_until("the map sort", GONE, |screen| screen.selected("id:search-sort-map").unwrap_or(false)).unwrap();
    assert!(!app.screen().selected("id:search-sort-date").unwrap());
    app.wait_for("Harugumo").unwrap();
}

#[test]
fn a_row_menu_copies_the_replay_path() {
    let mut app = desktop_app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.right_click(&format!("label:Open replay {}", replay().display())).unwrap();
    app.choose_menu_item("Copy Path").unwrap();
    let path = replay().to_string_lossy().into_owned();
    app.wait_until("the copied path", GONE, |screen| screen.clipboard() == Some(path.as_str())).unwrap();
}

#[test]
fn hovering_a_row_shows_its_battle() {
    let mut app = desktop_app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.hover(&format!("label:Open replay {}", replay().display())).unwrap();
    app.wait_for(&format!("label:Preview {}", replay().display())).unwrap();
    app.hover(QUERY).unwrap();
    app.wait_for_gone(&format!("label:Preview {}", replay().display()), GONE).unwrap();
}

#[test]
fn the_palette_lists_every_tab_and_its_seeded_searches() {
    let mut app = app();

    app.press_key("mod+k").unwrap();
    app.wait_for("Commands").unwrap();
    app.wait_for("Go to: Settings").unwrap();
    app.type_text("label:Command palette", "won").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for_gone("Commands", GONE).unwrap();
    // The seeded search lands in the Search tab as a pill and runs.
    app.wait_for("id:search-pill-0").unwrap();
    app.wait_for("Harugumo").unwrap();
}

#[test]
fn a_palette_layer_searches_players_and_leads_back() {
    let mut app = app();

    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "Search players").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("id:palette-back").unwrap();
    app.wait_for("text:/^\\[TEST\\] SearchCaptain/").unwrap();

    // Escape clears a typed query before it leaves the layer.
    app.type_text("label:Command palette", "zzz").unwrap();
    app.wait_for("No matches").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for("text:/^\\[TEST\\] SearchCaptain/").unwrap();
    assert!(app.screen().matches("id:palette-back").unwrap(), "clearing stays in the layer");
    app.press_key("escape").unwrap();
    app.wait_for_gone("id:palette-back", GONE).unwrap();
    app.wait_for("Go to: Settings").unwrap();

    // Backspace in the empty field of a layer also returns to the root.
    app.type_text("label:Command palette", "Search players").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("id:palette-back").unwrap();
    app.press_key("backspace").unwrap();
    app.wait_for_gone("id:palette-back", GONE).unwrap();

    app.type_text("label:Command palette", "Search players").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("text:/^\\[TEST\\] SearchCaptain/").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for_gone("id:palette-back", GONE).unwrap();
    app.wait_for("id:search-pill-0").unwrap();
    app.wait_for("Harugumo").unwrap();
}
