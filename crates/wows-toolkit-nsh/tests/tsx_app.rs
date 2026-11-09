use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard, OnceLock};

use nsh_core::{Points, Size};

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test replay directory is created");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
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

static TEST_STORAGE: OnceLock<std::path::PathBuf> = OnceLock::new();

fn configure_test_storage() {
    let path = TEST_STORAGE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test settings directory is created");
        path
    });
    wows_toolkit_config::storage_override::set(path.clone());
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

#[cfg(feature = "buck")]
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    let path = std::env::var("NSH_BUNDLE_PATH").expect("Buck provides the generated TSX bundle");
    let source = std::fs::read_to_string(path).expect("the generated TSX bundle is readable");
    let config = nsh_engine::EngineConfig {
        ui_sets: registries(),
        ..nsh_engine::EngineConfig::new(nsh_engine::Bundle::source(source))
    };
    let mut app = nsh_testing::Headless::start(config).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

#[cfg(not(feature = "buck"))]
fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    configure_test_storage();
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    TestApp { app, _guard: guard }
}

#[test]
fn replay_inspector_opens_and_displays_metadata() {
    const FILE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";

    let mut app = app();
    let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FILE);
    app.on_pick_paths(vec![replay]).unwrap();

    app.press("Open Replay File...").unwrap();
    assert_eq!(app.screen().file_requests().len(), 1, "the replay button must show one file dialog");
    app.wait_for(FILE).unwrap();
    app.wait_for("Players (15)").unwrap();
    app.wait_for("Battle").unwrap();
    app.wait_for("PVEBattle").unwrap();

    app.press("Settings").unwrap();
    app.press("Replay Inspector").unwrap();
    app.wait_for(FILE).unwrap();
    app.wait_for("Players (15)").unwrap();
    app.press("Raw metadata").unwrap();
    app.wait_for("text:/clientVersionFromXml/").unwrap();
    app.press("Overview").unwrap();
    app.wait_for("Battle").unwrap();

    let export_directory = TestDirectory::new("replay-export");
    let collision_selection = export_directory.0.join("narai");
    let collision_path = collision_selection.with_extension("json");
    std::fs::write(&collision_path, b"existing export").unwrap();
    app.on_pick_save_path(Some(collision_selection)).unwrap();
    app.press("Export Metadata...").unwrap();
    app.wait_for("text:/^could not write replay metadata to /").unwrap();
    assert_eq!(std::fs::read(&collision_path).unwrap(), b"existing export");

    let selected_path = export_directory.0.join("narai-export");
    let export_path = selected_path.with_extension("json");
    app.on_pick_save_path(Some(selected_path)).unwrap();
    app.press("Export Metadata...").unwrap();
    app.wait_for("text:/^Replay metadata exported to /").unwrap();
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(export_path).expect("the metadata export is written"))
            .expect("the metadata export is valid JSON");
    assert_eq!(exported["fileName"], FILE);
    assert_eq!(exported["mode"], "PVEBattle");
    assert_eq!(exported["players"].as_array().map(Vec::len), Some(15));
    assert!(exported.get("sourcePath").is_none());
    assert!(exported["players"][0]["shipId"].as_u64().is_some());

    let invalid = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    app.on_pick_paths(vec![invalid]).unwrap();
    app.press("Open Replay File...").unwrap();
    app.wait_for("text:/^could not parse replay /").unwrap();
}

#[test]
fn replay_directory_lists_readable_files_and_reports_failures() {
    const FIRST: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
    const SECOND: &str = "20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay";

    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays");
    let directory = TestDirectory::new("replay-browser");
    std::fs::copy(fixtures.join(FIRST), directory.0.join(FIRST)).expect("the first replay fixture is copied");
    std::fs::copy(fixtures.join(SECOND), directory.0.join(SECOND)).expect("the second replay fixture is copied");
    std::fs::write(directory.0.join("invalid.wowsreplay"), b"not a replay").expect("the invalid replay is written");

    let mut app = app();
    app.on_pick_paths(vec![directory.0.clone()]).unwrap();
    app.press("Open Replay Directory...").unwrap();

    let requests = app.screen().file_requests();
    assert_eq!(requests.len(), 1, "the directory button must show one file dialog");
    assert!(
        matches!(
            &requests[0],
            nsh_core::file_dialog::FileRequest::OpenDialog(dialog) if dialog.picks_folders()
        ),
        "the replay browser must request a directory"
    );
    app.wait_for("2 replay file(s)").unwrap();
    app.wait_for(FIRST).unwrap();
    app.wait_for("PVEBattle").unwrap();
    app.wait_for("1 replay file(s) could not be read.").unwrap();

    app.press(SECOND).unwrap();
    app.wait_for("PFSD210-Marceau").unwrap();
    assert_eq!(app.screen().count(SECOND), Ok(2), "the selected replay must also appear in the inspector heading");

    app.press_key("up").unwrap();
    assert_eq!(app.screen().count(FIRST), Ok(2), "keyboard selection must update the inspector");

    app.press("label:Sort by Replay").unwrap();
    assert_eq!(app.screen().count(FIRST), Ok(2), "sorting must preserve replay identity");

    app.press("label:Browse replay rows").unwrap();
    app.press_key("up").unwrap();
    assert_eq!(app.screen().count(SECOND), Ok(2), "keyboard selection must follow the sorted row order");

    app.set_viewport(Size::new(Points(500.0), Points(700.0))).expect("the compact replay viewport is applied");
    app.wait_for("Players (24)").unwrap();

    app.on_pick_paths(vec![fixtures.join(FIRST)]).unwrap();
    app.press("Open Replay File...").unwrap();
    app.wait_for_gone(
        &format!("wows-toolkit-nsh-replay-browser-{}", std::process::id()),
        std::time::Duration::from_secs(5),
    )
    .unwrap();

    let all_invalid = TestDirectory::new("replay-browser-all-invalid");
    std::fs::write(all_invalid.0.join("invalid.wowsreplay"), b"not a replay").expect("the invalid replay is written");
    app.on_pick_paths(vec![all_invalid.0.clone()]).unwrap();
    app.press("Open Replay Directory...").unwrap();
    app.wait_for("No readable replay files. 1 replay file(s) could not be read.").unwrap();
}

#[test]
fn settings_use_the_shared_database_and_nsh_default_theme() {
    let mut app = app();

    app.press_key("mod+,").unwrap();
    app.wait_for("NSH default").unwrap();
    app.wait_for("Save Settings").unwrap();
    app.clear("label:Collaboration display name").unwrap();
    app.type_text("label:Collaboration display name", "NSH Tester").unwrap();
    app.press("Save Settings").unwrap();
    app.wait_for("Settings saved.").unwrap();

    app.clear("label:Collaboration display name").unwrap();
    assert_eq!(app.screen().input_text("label:Collaboration display name").unwrap(), Some(""));
    app.press("Reload").unwrap();
    app.wait_until("the saved collaboration name returns", std::time::Duration::from_secs(5), |screen| {
        screen.input_text("label:Collaboration display name").ok().flatten() == Some("NSH Tester")
    })
    .unwrap();
}

#[test]
fn statistics_aggregate_the_shared_session_database() {
    struct Battle {
        sort_key: &'static str,
        game_time: &'static str,
        damage: i64,
        base_xp: i64,
        frags: i64,
        is_win: bool,
        is_loss: bool,
    }

    let mut app = app();
    app.set_viewport(Size::new(Points(500.0), Points(700.0))).expect("the compact viewport is applied");
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            wows_toolkit_config::queries::clear_session_stats(&pool).await.expect("old test statistics are cleared");
            for battle in [
                Battle {
                    sort_key: "a",
                    game_time: "2026-01-01T00:00:00Z",
                    damage: 1_000,
                    base_xp: 500,
                    frags: 1,
                    is_win: true,
                    is_loss: false,
                },
                Battle {
                    sort_key: "b",
                    game_time: "2026-01-02T00:00:00Z",
                    damage: 2_000,
                    base_xp: 1_000,
                    frags: 2,
                    is_win: false,
                    is_loss: true,
                },
            ] {
                wows_toolkit_config::queries::add_session_stat(
                    &pool,
                    &wows_toolkit_config::queries::SessionStatRow {
                        id: 0,
                        sort_key: battle.sort_key.to_owned(),
                        ship_name: "Test Ship".to_owned(),
                        ship_id: 42,
                        player_id: 7,
                        game_time: battle.game_time.to_owned(),
                        match_group: "pvp".to_owned(),
                        damage: battle.damage,
                        spotting_damage: 0,
                        frags: battle.frags,
                        raw_xp: battle.base_xp,
                        base_xp: battle.base_xp,
                        is_win: battle.is_win,
                        is_loss: battle.is_loss,
                        is_draw: false,
                        is_div: false,
                        achievements: "[]".to_owned(),
                    },
                )
                .await
                .expect("a test statistic is recorded");
            }
        },
    );

    app.press("Stats").unwrap();
    app.wait_for("Recorded battles from the shared session database").unwrap();
    app.wait_for("Test Ship").unwrap();
    app.wait_for("50.0%").unwrap();
    app.wait_for("1500").unwrap();
    app.wait_for("750").unwrap();
}

#[test]
fn player_tracker_reads_and_filters_indexed_players() {
    let mut app = app();
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            use wows_core::game_types::{AccountId, ArenaId, GameParamId};
            use wows_toolkit_config::index::rows::{IndexedVehicleRow, ObjectiveMatch, VehicleRelation};

            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            sqlx::query("DELETE FROM replay_record").execute(&pool).await.expect("old replay records are cleared");
            sqlx::query("DELETE FROM indexed_vehicle").execute(&pool).await.expect("old indexed players are cleared");
            sqlx::query("DELETE FROM indexed_match").execute(&pool).await.expect("old indexed matches are cleared");
            let arena_id = ArenaId::new(982_001);
            wows_toolkit_config::index::query::upsert_match(
                &pool,
                &ObjectiveMatch {
                    arena_id,
                    timestamp: jiff::Timestamp::now(),
                    map: "Ocean".into(),
                    game_mode: "Domination".into(),
                    game_mode_id: None,
                    game_type: "pvp".into(),
                    match_group: "pvp".into(),
                    version_build: Some(1234),
                },
            )
            .await
            .expect("the indexed match is written");
            let player = |account_id: i64, name: &str, clan: &str| IndexedVehicleRow {
                arena_id,
                account_id: AccountId(account_id),
                player_name: name.into(),
                clan: clan.into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(900u64 + account_id as u64),
                ship_index: "PJSD018".into(),
                ship_name: "Harugumo".into(),
                nation: "japan".into(),
                species: "Destroyer".into(),
                tier: 10,
                relation: VehicleRelation::Enemy,
                division_id: None,
                survived: Some(true),
                damage: Some(50_000),
                kills: Some(1),
                spotting: Some(10_000),
                potential: Some(500_000),
                received: Some(5_000),
                pr: Some(1500.0),
                is_test_ship: false,
                disconnected: Some(false),
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            wows_toolkit_config::index::query::upsert_vehicles(
                &pool,
                &[player(99_001, "TrackedCaptain", "TEST"), player(99_002, "OtherOpponent", "NAVY")],
            )
            .await
            .expect("the indexed players are written");
        },
    );

    app.press("Player Tracker").unwrap();
    app.wait_for("TrackedCaptain").unwrap();
    app.wait_for("OtherOpponent").unwrap();
    app.wait_for("Showing 2 of 2 players").unwrap();
    app.type_text("label:Filter tracked players", "Other").unwrap();
    app.wait_for_gone("TrackedCaptain", std::time::Duration::from_secs(5)).unwrap();
    app.wait_for("OtherOpponent").unwrap();
    app.wait_for("Showing 1 of 1 players").unwrap();
    app.clear("label:Filter tracked players").unwrap();
    app.type_text("label:Filter tracked players", "%").unwrap();
    app.wait_for("No matching players").unwrap();
}

#[test]
fn search_queries_the_shared_replay_index() {
    let mut app = app();
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            use std::path::{Path, PathBuf};

            use wows_core::game_types::{AccountId, ArenaId, GameParamId};
            use wows_toolkit_config::index::rows::{
                IndexedVehicleRow, MatchOutcome, ObjectiveMatch, ReplayRecord, VehicleRelation,
            };

            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            sqlx::query("DELETE FROM replay_record").execute(&pool).await.expect("old replay records are cleared");
            sqlx::query("DELETE FROM indexed_vehicle").execute(&pool).await.expect("old indexed players are cleared");
            sqlx::query("DELETE FROM indexed_match").execute(&pool).await.expect("old indexed matches are cleared");
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
                    replay_path: PathBuf::from("/tmp/search-test.wowsreplay"),
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

    app.press("Search").unwrap();
    app.wait_for("Search indexed replays").unwrap();
    app.type_text("label:Replay search query", "map:Ocean and outcome:win").unwrap();
    app.press("Run Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.wait_for("123456").unwrap();
    app.wait_for("1 matches").unwrap();

    app.clear("label:Replay search query").unwrap();
    app.type_text("label:Replay search query", "(").unwrap();
    app.press("Run Search").unwrap();
    app.wait_for("Search unavailable").unwrap();
}

#[test]
fn top_level_navigation_and_application_menu_work() {
    let mut app = app();

    app.wait_for("Replay Inspector").unwrap();
    app.press_key("mod+k").unwrap();
    app.wait_for("Command Palette").unwrap();
    app.press("Go to Settings").unwrap();
    app.wait_for("NSH default").unwrap();
    app.wait_for("Shared with the egui and GPUI applications").unwrap();

    app.press("label:Open application menu").unwrap();
    app.wait_for("Tactics Board").unwrap();
    app.press("About WoWs Toolkit").unwrap();
    app.wait_for("Tools for World of Warships replays, game data, statistics, rendering, and collaboration.").unwrap();
    app.press("Close").unwrap();
    app.wait_for_gone(
        "Tools for World of Warships replays, game data, statistics, rendering, and collaboration.",
        std::time::Duration::from_secs(5),
    )
    .unwrap();

    app.press("label:Open application menu").unwrap();
    app.press("Tactics Board").unwrap();
    app.wait_for("Select a map to start a tactics board.").unwrap();
    app.press("Close").unwrap();
    app.wait_for_gone("Select a map to start a tactics board.", std::time::Duration::from_secs(5)).unwrap();
}

#[test]
fn command_palette_supports_keyboard_navigation_and_compact_viewports() {
    let mut app = app();
    app.set_viewport(Size::new(Points(500.0), Points(360.0))).expect("the compact viewport is applied");

    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "About").unwrap();
    app.press_key("down").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("Tools for World of Warships replays, game data, statistics, rendering, and collaboration.").unwrap();
    app.press("Close").unwrap();

    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "nomatch").unwrap();
    app.wait_for("No matching commands.").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for("Go to Replay Inspector").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("Command Palette", std::time::Duration::from_secs(5)).unwrap();
}
