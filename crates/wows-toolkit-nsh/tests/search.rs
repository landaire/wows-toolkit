//! Headless behavior of the Search tab and the command palette over the real
//! TSX bundle.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use nsh_core::{MountBatch, MountItem, NodeId, Points, Size};
use wows_toolkit_nsh::replay_inspector::constants::BuildNumber;
use wows_toolkit_nsh::ship_names::{BuildReach, ShipKey, ShipNamer};

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
    use wows_toolkit_viewmodel::settings::keys;

    wows_toolkit_config::storage_override::set(TEST_STORAGE.clone());

    // The first-run questions would stand in front of the tab, and the startup
    // update check would ask the network and open its dialog whenever it is
    // answered.
    with_test_database(async |pool| {
        for key in [keys::BUILD_CONSENT_SHOWN, keys::REPLAY_CONSENT_SHOWN, keys::LANGUAGE_SELECTION_SHOWN] {
            wows_toolkit_config::queries::set_setting_raw(pool, key, "true").await.expect("a question is answered");
        }
        wows_toolkit_config::queries::set_setting_raw(pool, keys::CHECK_FOR_UPDATES, "false")
            .await
            .expect("the startup update check is off");
    });
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> nsh_testing::Headless {
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    nsh_testing::tsx(&entry, registries()).expect("the TSX app starts")
}

/// Takes the copies the app makes in place of the system clipboard, so no
/// test writes the user's clipboard.
struct RecordingClipboard {
    copied: Mutex<Vec<Vec<std::path::PathBuf>>>,
    refuse: AtomicBool,
}

impl RecordingClipboard {
    fn last(&self) -> Option<Vec<std::path::PathBuf>> {
        self.copied.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).last().cloned()
    }
}

impl wows_toolkit_nsh::clipboard::FileClipboard for RecordingClipboard {
    fn copy_files(&self, files: &[std::path::PathBuf]) -> Result<(), arboard::Error> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(arboard::Error::ClipboardNotSupported);
        }
        self.copied.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(files.to_vec());
        Ok(())
    }
}

static CLIPBOARD: RecordingClipboard =
    RecordingClipboard { copied: Mutex::new(Vec::new()), refuse: AtomicBool::new(false) };

/// The app over an index holding one battle, with no saved Search state.
fn app() -> TestApp {
    launch(None, None)
}

/// [`app`], its ships named by `namer` in place of the game data, with every
/// frame the app lays out recorded in `layout`.
fn launch(namer: Option<ShipNamer>, layout: Option<Arc<Mutex<Layout>>>) -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    wows_toolkit_nsh::clipboard::replace(&CLIPBOARD);
    CLIPBOARD.copied.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    wows_toolkit_nsh::ship_names::set_namer(namer);
    seed_index();
    let mut app = start();
    if let Some(layout) = layout {
        app.observe(move |_, batch| layout.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).record(batch));
    }
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

/// Each view's parent, frame and test id, as the batches applied so far set
/// them.
#[derive(Default)]
struct Layout {
    parents: HashMap<NodeId, NodeId>,
    frames: HashMap<NodeId, nsh_core::Bounds<Points>>,
    tagged: HashMap<String, NodeId>,
}

/// A view's edges in the root's points.
#[derive(Debug, Clone, Copy)]
struct Edges {
    left: f32,
    top: f32,
    bottom: f32,
}

impl Layout {
    fn record(&mut self, batch: &MountBatch) {
        for item in &batch.items {
            match item {
                MountItem::Insert { parent, child, .. } => {
                    self.parents.insert(*child, *parent);
                }
                MountItem::Remove { id } | MountItem::Delete { id } => {
                    self.parents.remove(id);
                }
                MountItem::Frame { id, frame } => {
                    self.frames.insert(*id, *frame);
                }
                MountItem::ViewProps { id, props } => {
                    if let Some(test_id) = &props.test_id {
                        self.tagged.insert(test_id.to_string(), *id);
                    }
                }
                _ => {}
            }
        }
    }

    /// The edges of the view tagged `test_id` in the root, its frame moved
    /// by those of the views around it. Scroll offsets are left out: none of
    /// the views measured here is scrolled.
    fn edges(&self, test_id: &str) -> Edges {
        let id = self.tagged[test_id];
        let frame = self.frames[&id];
        let mut left = frame.origin.x.0;
        let mut top = frame.origin.y.0;
        let mut at = id;
        while let Some(parent) = self.parents.get(&at) {
            if let Some(around) = self.frames.get(parent) {
                left += around.origin.x.0;
                top += around.origin.y.0;
            }
            at = *parent;
        }

        Edges { left, top, bottom: top + frame.size.height.0 }
    }
}

/// [`app`] as a Linux desktop, which has a mouse to right-click and hover
/// with.
fn desktop_app() -> TestApp {
    as_desktop(app())
}

fn as_desktop(mut app: TestApp) -> TestApp {
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

/// Runs `work` against the test database.
fn with_test_database<T>(work: impl AsyncFnOnce(&sqlx::SqlitePool) -> T) -> T {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            work(&pool).await
        },
    )
}

/// Waits until the stored locale row holds `json`.
fn wait_for_saved_locale(app: &mut TestApp, json: &str) {
    let saved = || {
        with_test_database(async |pool| {
            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'locale'")
                .fetch_optional(pool)
                .await
                .expect("the settings table reads")
        })
    };
    let deadline = std::time::Instant::now() + GONE;
    while saved().as_deref() != Some(json) {
        assert!(std::time::Instant::now() < deadline, "the locale row was not saved; saved: {:?}", saved());
        app.settle();
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_typed_query_finds_and_opens_an_indexed_replay() {
    let mut app = app();
    // The inspector reads every replay as the sample report, titled by its ship and map.
    wows_toolkit_nsh::replay_inspector::test_support::read_replays_as_sample();

    app.press("Search").unwrap();
    // The tab opens on the saved query, which is empty: every indexed battle.
    app.wait_for("Harugumo").unwrap();
    app.type_text(QUERY, "map:Ocean outcome:win").unwrap();
    app.submit(QUERY).unwrap();
    app.wait_for("id:search-pill-0").unwrap();
    app.wait_for("123,456").unwrap();
    app.wait_for("1 match(es)").unwrap();

    // A single press only selects; a double click opens, as in GPUI.
    let row = format!("label:Open replay {}", replay().display());
    app.press(&row).unwrap();
    assert!(!app.shows("Shimakaze - Fault Line"), "one press does not open the replay");
    app.press(&row).unwrap();
    app.wait_for("Shimakaze - Fault Line").unwrap();
}

#[test]
fn a_query_that_does_not_parse_is_reported_above_the_results() {
    let mut app = app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.type_text(QUERY, "outcome=win and").unwrap();
    app.press("id:search-run").unwrap();
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
    app.wait_until("the negated pill", GONE, |screen| {
        screen.texts().into_iter().map(str::to_owned).collect::<Vec<_>>() != before
    })
    .unwrap();

    app.press_key("mod+z").unwrap();
    app.wait_until("the original pill", GONE, |screen| {
        screen.texts().into_iter().map(str::to_owned).collect::<Vec<_>>() == before
    })
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
fn a_row_menu_copies_the_replay_file() {
    let mut app = desktop_app();
    CLIPBOARD.refuse.store(false, Ordering::SeqCst);

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.right_click(&format!("label:Open replay {}", replay().display())).unwrap();
    app.choose_menu_item("Copy Replay").unwrap();
    app.wait_until("the copied replay", GONE, |_| CLIPBOARD.last() == Some(vec![replay()])).unwrap();
}

#[test]
fn a_row_menu_reports_a_clipboard_failure() {
    let mut app = desktop_app();
    CLIPBOARD.refuse.store(true, Ordering::SeqCst);

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.right_click(&format!("label:Open replay {}", replay().display())).unwrap();
    app.choose_menu_item("Copy Replay").unwrap();
    app.wait_for("text:/^could not copy the replay to the clipboard: /").unwrap();
    CLIPBOARD.refuse.store(false, Ordering::SeqCst);
    assert_eq!(CLIPBOARD.last(), None, "a refused copy records nothing");
}

#[test]
fn hovering_a_row_shows_its_battle() {
    let mut app = desktop_app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.hover(&format!("label:Open replay {}", replay().display())).unwrap();
    app.wait_for(&format!("label:Preview {}", replay().display())).unwrap();
    app.wait_for("label:Minimap preview").unwrap();
    app.hover(QUERY).unwrap();
    app.wait_for_gone(&format!("label:Preview {}", replay().display()), GONE).unwrap();
    app.wait_for_gone("label:Minimap preview", GONE).unwrap();
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

#[test]
fn a_language_change_rewords_the_rows_on_screen() {
    let mut app = app();

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.wait_for("Win").unwrap();

    app.press("Settings").unwrap();
    app.press("label:Language").unwrap();
    app.press("Deutsch").unwrap();
    app.press("text:/^(Search|Suche)$/").unwrap();
    app.wait_for("Sieg").unwrap();
    assert!(app.shows("Harugumo"), "the same rows stay");

    // The app writes the choice in the background. A reset made before that
    // write lands is overwritten, and the next test starts in German.
    wait_for_saved_locale(&mut app, r#""de""#);
    // The next test starts in English.
    with_test_database(async |pool| {
        wows_toolkit_config::queries::set_setting_raw(pool, "locale", r#""en""#).await.expect("the locale is reset");
    });
}

/// The seeded battle's ship as its build knows it.
fn seeded_ship(key: ShipKey) -> bool {
    key.build == BuildNumber(1234) && key.ship == wows_core::game_types::GameParamId::from(999u64)
}

/// Names the seeded ship as if its build were loaded.
fn name_loaded(key: ShipKey, _reach: BuildReach) -> Option<String> {
    seeded_ship(key).then(|| "Harugumo (live)".to_owned())
}

/// Names the seeded ship only by loading its build: none is loaded.
fn name_by_loading(key: ShipKey, reach: BuildReach) -> Option<String> {
    (reach == BuildReach::Load && seeded_ship(key)).then(|| "Harugumo (loaded)".to_owned())
}

/// How many times [`new_game_data_names_the_rows_again`] has changed the
/// game data, which [`name_by_binding`] names the ship after.
static BINDINGS: AtomicU8 = AtomicU8::new(0);

/// Names the seeded ship after the game data the test has bound.
fn name_by_binding(key: ShipKey, _reach: BuildReach) -> Option<String> {
    seeded_ship(key).then(|| format!("Harugumo ({})", BINDINGS.load(Ordering::SeqCst)))
}

/// Holds every lookup of loaded names while set, so a search stays running.
static HOLD_SEARCH: AtomicBool = AtomicBool::new(false);

/// Names nothing, waiting while [`HOLD_SEARCH`] is set.
fn name_when_released(_key: ShipKey, reach: BuildReach) -> Option<String> {
    let deadline = std::time::Instant::now() + GONE;
    while reach == BuildReach::Loaded && HOLD_SEARCH.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

/// Saves `value` in settings row `key` and rebinds the game data to it.
fn rebind_game_data(key: &str, value: &str) {
    with_test_database(async |pool| {
        wows_toolkit_config::queries::set_setting_raw(pool, key, value).await.expect("the game data setting is saved");
    });
    wows_toolkit_nsh::game_data::reload_settings().expect("the game data rebinds");
}

/// Puts the game data settings a test changed back, however it ends.
struct RestoreGameData;

impl Drop for RestoreGameData {
    fn drop(&mut self) {
        rebind_game_data(wows_toolkit_viewmodel::settings::keys::AUTO_DUMP_GAME_DATA, "false");
        rebind_game_data(wows_toolkit_viewmodel::settings::keys::LOCALE, r#""en""#);
        BINDINGS.store(0, Ordering::SeqCst);
    }
}

#[test]
fn a_loaded_build_names_the_ship_column() {
    let mut app = launch(Some(name_loaded), None);

    app.press("Search").unwrap();
    app.wait_for("Harugumo (live)").unwrap();
    assert!(!app.shows("Harugumo"), "the build's name replaces the one stored at index time");
}

#[test]
fn hovering_a_row_names_its_ship_from_its_build() {
    let mut app = as_desktop(launch(Some(name_by_loading), None));

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.hover(&format!("label:Open replay {}", replay().display())).unwrap();
    app.wait_for(&format!("label:Preview {}", replay().display())).unwrap();
    app.wait_until("the row and its preview name the loaded ship", GONE, |screen| {
        screen.count("text:Harugumo (loaded)") == Ok(2)
    })
    .unwrap();
    assert!(!app.shows("Harugumo"), "the stored name is gone from the row and the preview");
}

#[test]
fn new_game_data_names_the_rows_again() {
    BINDINGS.store(0, Ordering::SeqCst);
    let mut app = launch(Some(name_by_binding), None);
    let _restore = RestoreGameData;

    app.press("Search").unwrap();
    app.wait_for("Harugumo (0)").unwrap();

    // The auto-dump flag is fixed per cache, so a change replaces the cache.
    BINDINGS.store(1, Ordering::SeqCst);
    rebind_game_data(wows_toolkit_viewmodel::settings::keys::AUTO_DUMP_GAME_DATA, "true");
    app.wait_for("Harugumo (1)").unwrap();
    assert!(!app.shows("Harugumo (0)"), "no name read from the replaced game data stays");

    // A language change keeps the cache and rewords what it names.
    BINDINGS.store(2, Ordering::SeqCst);
    rebind_game_data(wows_toolkit_viewmodel::settings::keys::LOCALE, r#""de""#);
    app.wait_for("Harugumo (2)").unwrap();
}

#[test]
fn a_running_search_replaces_the_rows() {
    HOLD_SEARCH.store(false, Ordering::SeqCst);
    let mut app = launch(Some(name_when_released), None);

    app.press("Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    HOLD_SEARCH.store(true, Ordering::SeqCst);
    app.press("id:search-run").unwrap();
    app.wait_for("Searching...").unwrap();
    assert!(!app.shows("Harugumo"), "the earlier rows give way while the search runs");
    HOLD_SEARCH.store(false, Ordering::SeqCst);
    app.wait_for("Harugumo").unwrap();
}

#[test]
fn the_completions_open_under_a_wrapped_pill_box() {
    use wows_toolkit_viewmodel::query_bar::suggest::SuggestionKind;

    let field = wows_toolkit_viewmodel::query_bar::suggest::static_suggestions()
        .into_iter()
        .find(|suggestion| matches!(suggestion.kind, SuggestionKind::MatchField(_)))
        .expect("there are field suggestions")
        .label
        .clone();
    let needle: String = field.chars().take(3).collect();
    let layout = Arc::new(Mutex::new(Layout::default()));
    let mut app = launch(None, Some(Arc::clone(&layout)));
    // Narrow, so the pills take more than one line.
    app.set_viewport(Size::new(Points(520.0), Points(900.0))).expect("the narrow viewport is applied");

    app.press("Search").unwrap();
    app.type_text(QUERY, "outcome:win map:Ocean outcome:win map:Ocean outcome:win map:Ocean").unwrap();
    app.submit(QUERY).unwrap();
    // Six pills and the five connectors between them.
    app.wait_for("id:search-pill-10").unwrap();
    app.type_text(QUERY, &needle).unwrap();
    app.wait_for("id:search-completion-0").unwrap();

    let layout = layout.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let first_pill = layout.edges("search-pill-0");
    let last_pill = layout.edges("search-pill-10");
    let bar = layout.edges("search-pills");
    let completion = layout.edges("search-completion-0");
    assert!(last_pill.top >= first_pill.bottom, "the pills wrap onto a second line: {first_pill:?}, {last_pill:?}");
    // The list's own frame and padding sit between the bar and its first row.
    assert!(
        completion.top >= bar.bottom && completion.top <= bar.bottom + 16.0,
        "the list opens just under the bar: {bar:?}, {completion:?}"
    );
    assert!(
        (completion.left - bar.left).abs() <= 8.0,
        "the list starts at the bar's left edge: {bar:?}, {completion:?}"
    );
}
