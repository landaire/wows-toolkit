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
fn dropping_a_replay_opens_it_in_the_inspector() {
    const FILE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
    let replay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays").join(FILE);
    let uri = url::Url::from_file_path(replay).expect("the replay path converts to a file URL").to_string();
    let mut app = app();
    app.set_environment("desktop", |environment| {
        environment.os = nsh_core::platform::Os::Linux;
        environment.idiom = nsh_core::platform::Idiom::Desktop;
    })
    .unwrap();

    app.drop_files("Replay Inspector", vec![uri]).unwrap();
    app.wait_for("PVEBattle").unwrap();
    app.wait_for("Players (15)").unwrap();
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

/// Runs `work` against the test database.
fn with_test_database<T>(work: impl AsyncFnOnce(&sqlx::SqlitePool) -> T) -> T {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            work(&pool).await
        },
    )
}

/// One encounter a seeded tracked player had: its arena, how long ago, and
/// whether it was a division one.
struct SeededEncounter {
    arena: i64,
    hours_ago: i64,
    division: bool,
}

/// One player the seeded index and tracker both know.
struct SeededPlayer {
    account: i64,
    name: &'static str,
    aliases: &'static [&'static str],
    clan: &'static str,
    encounters: &'static [SeededEncounter],
}

/// The tracker's fixture: two players met today, one met only as a division
/// mate, and one met three days ago, both in the replay index and in the
/// shared tracked-player tables. The tracker's saved view and the WoWs
/// directory are reset, so each test opens on the defaults.
fn seed_player_tracker() {
    use wows_core::game_types::AccountId;
    use wows_core::game_types::ArenaId;
    use wows_core::game_types::GameParamId;
    use wows_toolkit_config::index::rows::IndexedVehicleRow;
    use wows_toolkit_config::index::rows::ObjectiveMatch;
    use wows_toolkit_config::index::rows::VehicleRelation;
    use wows_toolkit_viewmodel::player_tracker::tracked::Pending;
    use wows_toolkit_viewmodel::player_tracker::tracked::TrackedPlayer;
    use wows_toolkit_viewmodel::player_tracker::tracked::ViewModes;

    const PLAYERS: &[SeededPlayer] = &[
        SeededPlayer {
            account: 99_001,
            name: "TrackedCaptain",
            aliases: &["OldCaptainName"],
            clan: "TEST",
            encounters: &[SeededEncounter { arena: 982_001, hours_ago: 0, division: false }],
        },
        SeededPlayer {
            account: 99_002,
            name: "OtherOpponent",
            aliases: &[],
            clan: "NAVY",
            encounters: &[
                SeededEncounter { arena: 982_001, hours_ago: 0, division: false },
                SeededEncounter { arena: 982_003, hours_ago: 1, division: false },
                SeededEncounter { arena: 982_004, hours_ago: 2, division: false },
            ],
        },
        SeededPlayer {
            account: 99_003,
            name: "DivisionMate",
            aliases: &[],
            clan: "TEST",
            encounters: &[SeededEncounter { arena: 982_001, hours_ago: 0, division: true }],
        },
        SeededPlayer {
            account: 99_004,
            name: "AncientFoe",
            aliases: &[],
            clan: "OLD",
            encounters: &[SeededEncounter { arena: 982_002, hours_ago: 72, division: false }],
        },
    ];

    let now = jiff::Timestamp::from_second(jiff::Timestamp::now().as_second()).expect("now is a valid timestamp");
    let at = |hours_ago: i64| now - jiff::SignedDuration::from_hours(hours_ago);

    with_test_database(async |pool| {
        for table in ["replay_record", "indexed_vehicle", "indexed_match"] {
            sqlx::query(&format!("DELETE FROM {table}")).execute(pool).await.expect("old index rows are cleared");
        }
        sqlx::query("DELETE FROM settings WHERE key = 'player_tracker'")
            .execute(pool)
            .await
            .expect("the saved tracker view is cleared");
        wows_toolkit_config::queries::set_setting(pool, wows_toolkit_viewmodel::settings::keys::WOWS_DIR, &"")
            .await
            .expect("the WoWs directory is cleared");
        wows_toolkit_viewmodel::player_tracker::store::store_view_modes(pool, ViewModes::default())
            .await
            .expect("the Current Match modes are reset");
        wows_toolkit_config::tracker::clear_tracker(pool).await.expect("old tracked players are cleared");

        // The index holds the two newest battles: the one everyone met in
        // and the old one. The others only the tracker remembers.
        for (arena, hours_ago) in [(982_001, 0), (982_002, 72)] {
            wows_toolkit_config::index::query::upsert_match(
                pool,
                &ObjectiveMatch {
                    arena_id: ArenaId::new(arena),
                    timestamp: at(hours_ago),
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
        }
        let vehicles: Vec<IndexedVehicleRow> = PLAYERS
            .iter()
            .map(|player| IndexedVehicleRow {
                arena_id: ArenaId::new(player.encounters[0].arena.min(982_002)),
                account_id: AccountId(player.account),
                player_name: player.name.into(),
                clan: player.clan.into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(900u64 + player.account as u64),
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
            })
            .collect();
        wows_toolkit_config::index::query::upsert_vehicles(pool, &vehicles)
            .await
            .expect("the indexed players are written");

        let tracked: std::collections::HashMap<AccountId, TrackedPlayer> = PLAYERS
            .iter()
            .map(|seeded| {
                let mut player = TrackedPlayer {
                    last_name: seeded.name.to_string(),
                    db_id: AccountId(seeded.account),
                    names: std::iter::once(seeded.name).chain(seeded.aliases.iter().copied()).map(String::from).collect(),
                    clan: seeded.clan.to_string(),
                    ..TrackedPlayer::default()
                };
                for encounter in seeded.encounters {
                    let arena = ArenaId::new(encounter.arena);
                    player.arena_ids.insert(arena);
                    player.timestamps.insert(at(encounter.hours_ago));
                    if encounter.division {
                        player.division_encounters.mark(arena, at(encounter.hours_ago));
                    }
                }
                (AccountId(seeded.account), player)
            })
            .collect();
        let mut pending = Pending::default();
        pending.everything_changed();
        wows_toolkit_viewmodel::player_tracker::store::save(pool, &pending, &tracked)
            .await
            .expect("the tracked players are written");
    });
}

/// Where `text` first appears on screen, for checking row order.
fn position(app: &TestApp, text: &str) -> usize {
    app.screen().texts().iter().position(|shown| *shown == text).unwrap_or_else(|| panic!("{text} is shown"))
}

fn open_player_tracker(app: &mut TestApp) {
    app.press("Player Tracker").unwrap();
    app.wait_for("OtherOpponent").unwrap();
    app.wait_for("TrackedCaptain").unwrap();
}

#[test]
fn player_tracker_changes_period_filters_and_cycles_the_sort() {
    seed_player_tracker();
    let mut app = app();
    open_player_tracker(&mut app);

    // The past day, with division mates hidden, most-met first.
    assert!(!app.shows("AncientFoe"), "a player met three days ago is outside the past day");
    assert!(!app.shows("DivisionMate"), "a player met only in division is hidden");
    assert!(position(&app, "OtherOpponent") < position(&app, "TrackedCaptain"));
    app.wait_for("Encounters in Time Range (desc)").unwrap();

    // A new column opens on its own order, the same one flips, a third press
    // puts the table back the way it opened.
    app.press("role:columnheader[Player Name]").unwrap();
    app.wait_for("Player Name (asc)").unwrap();
    assert!(position(&app, "OtherOpponent") < position(&app, "TrackedCaptain"));
    app.press("role:columnheader[Player Name]").unwrap();
    app.wait_for("Player Name (desc)").unwrap();
    assert!(position(&app, "TrackedCaptain") < position(&app, "OtherOpponent"));
    app.press("role:columnheader[Player Name]").unwrap();
    app.wait_for("Encounters in Time Range (desc)").unwrap();
    assert!(position(&app, "OtherOpponent") < position(&app, "TrackedCaptain"));

    app.type_text("label:Player Filter", "Other").unwrap();
    app.wait_for_gone("TrackedCaptain", std::time::Duration::from_secs(5)).unwrap();
    app.wait_for("OtherOpponent").unwrap();
    app.clear("label:Player Filter").unwrap();
    app.wait_for("TrackedCaptain").unwrap();

    app.press("label:Time period").unwrap();
    app.press("All Time").unwrap();
    app.wait_for("AncientFoe").unwrap();
    app.wait_for("OtherOpponent").unwrap();
}

#[test]
fn player_tracker_division_toggle_and_expanded_detail() {
    seed_player_tracker();
    let mut app = app();
    open_player_tracker(&mut app);

    app.press("label:Show division mates").unwrap();
    app.wait_for("DivisionMate").unwrap();
    app.press("label:Show division mates").unwrap();
    app.wait_for_gone("DivisionMate", std::time::Duration::from_secs(5)).unwrap();

    app.press("label:Expand TrackedCaptain").unwrap();
    app.wait_for("Also known as: OldCaptainName").unwrap();
    app.wait_for("1 matches recorded").unwrap();
    app.wait_for("99001").unwrap();
    app.press("label:Click to copy the WG ID").unwrap();
    app.wait_until("the account id is copied", std::time::Duration::from_secs(5), |screen| {
        screen.clipboard() == Some("99001")
    })
    .unwrap();
    app.press("label:Collapse TrackedCaptain").unwrap();
    app.wait_for_gone("Also known as: OldCaptainName", std::time::Duration::from_secs(5)).unwrap();
}

#[test]
fn player_tracker_notes_are_written_to_the_shared_tables() {
    use wows_core::game_types::AccountId;

    seed_player_tracker();
    let mut app = app();
    open_player_tracker(&mut app);

    app.press("label:Notes on this player TrackedCaptain").unwrap();
    app.wait_for("Also known as: OldCaptainName").unwrap();
    app.type_text("label:Notes on this player", "flanks early").unwrap();
    app.submit("label:Notes on this player").unwrap();
    app.wait_for("text:flanks early").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let stored = with_test_database(async |pool| {
            wows_toolkit_viewmodel::player_tracker::store::load(pool).await.expect("the tracked players read back")
        });
        if stored.get(&AccountId(99_001)).is_some_and(|player| player.notes == "flanks early") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the note reaches the shared tables");
        app.wait(std::time::Duration::from_millis(100)).unwrap();
    }
}

#[test]
fn player_tracker_clans_table_lists_members_met() {
    seed_player_tracker();
    let mut app = app();
    open_player_tracker(&mut app);

    app.press("id:tab-clans").unwrap();
    app.wait_for("NAVY").unwrap();
    app.wait_for("TEST").unwrap();
    app.press("label:Expand NAVY").unwrap();
    app.wait_for("seen in 3 matches").unwrap();
    app.press("label:Find matches OtherOpponent").unwrap();
    app.wait_for("Search").unwrap();
    app.wait_until("the search holds the player's query", std::time::Duration::from_secs(5), |screen| {
        screen.input_text("label:Replay search query").ok().flatten().is_some_and(|query| query.contains("99002"))
    })
    .unwrap();
}

#[test]
fn player_tracker_clears_only_after_confirmation() {
    seed_player_tracker();
    let mut app = app();
    open_player_tracker(&mut app);
    let tracked_count =
        || with_test_database(async |pool| wows_toolkit_viewmodel::player_tracker::store::load(pool).await.unwrap().len());

    app.on_dialog(|_| nsh_testing::DialogAnswer::Press("Cancel".to_owned()));
    app.press("Clear Stats").unwrap();
    app.wait(std::time::Duration::from_millis(500)).unwrap();
    assert_eq!(tracked_count(), 4, "a cancelled clear keeps every tracked player");

    app.on_dialog(|_| nsh_testing::DialogAnswer::Press("Clear Stats".to_owned()));
    app.press("Clear Stats").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while tracked_count() != 0 {
        assert!(std::time::Instant::now() < deadline, "the confirmed clear empties the shared tables");
        app.wait(std::time::Duration::from_millis(100)).unwrap();
    }
    // The index still names the players; the tracker no longer counts them.
    app.wait_for_gone("Also known as: OldCaptainName", std::time::Duration::from_secs(5)).unwrap();
    app.press("label:Expand TrackedCaptain").unwrap();
    app.wait_for("Click to copy the WG ID").unwrap();
    assert!(!app.shows("1 matches recorded"));
}

#[test]
fn player_tracker_view_round_trips_through_the_shared_settings_row() {
    use wows_toolkit_viewmodel::player_tracker::SortColumn;
    use wows_toolkit_viewmodel::player_tracker::SortOrder;
    use wows_toolkit_viewmodel::player_tracker::TimePeriod;
    use wows_toolkit_viewmodel::player_tracker::TrackerSettings;

    seed_player_tracker();
    {
        let mut app = app();
        open_player_tracker(&mut app);
        app.press("role:columnheader[Player Name]").unwrap();
        app.wait_for("Player Name (asc)").unwrap();
        app.press("label:Time period").unwrap();
        app.press("All Time").unwrap();
        app.wait_for("AncientFoe").unwrap();
        app.type_text("label:Player Filter", "Foe").unwrap();
        app.wait_for_gone("TrackedCaptain", std::time::Duration::from_secs(5)).unwrap();
        app.press("id:tab-current-match").unwrap();
        app.press("Compact").unwrap();
        app.wait_for("Overall WR").unwrap();
        app.wait(std::time::Duration::from_millis(300)).unwrap();
    }

    let saved = with_test_database(async |pool| {
        wows_toolkit_config::queries::get_setting::<TrackerSettings>(pool, "player_tracker").await
    })
    .expect("the view is saved under the key the GPUI tracker reads");
    assert_eq!(saved.period, TimePeriod::AllTime);
    assert_eq!(saved.sort.column, SortColumn::Name);
    assert_eq!(saved.sort.order, SortOrder::Ascending);
    assert_eq!(saved.filter, "Foe");
    let modes = with_test_database(async |pool| wows_toolkit_viewmodel::player_tracker::store::load_view_modes(pool).await);
    assert_eq!(modes.current_match_view_mode, wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode::Compact);

    let mut app = app();
    app.press("Player Tracker").unwrap();
    app.wait_for("AncientFoe").unwrap();
    app.wait_for("Player Name (asc)").unwrap();
    assert_eq!(app.screen().input_text("label:Player Filter").unwrap(), Some("Foe"));
    assert!(!app.shows("TrackedCaptain"));
}

#[test]
fn player_tracker_current_match_lists_the_battle_in_progress() {
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
                     {"shipId": 200, "relation": 2, "id": 2, "name": "OtherOpponent"}],
        "playersPerTeam": 12,
        "dateTime": "28.12.2023 00:52:26",
        "mapName": "spaces/00_CO_ocean",
        "playerName": "Harvey635",
        "scenarioConfigId": 1,
        "teamsCount": 2,
        "logic": null,
        "playerVehicle": "PFSD110-Kleber"
    }"#;

    seed_player_tracker();
    let wows_dir = TestDirectory::new("tracker-live");
    std::fs::create_dir(wows_dir.0.join("replays")).expect("the replays directory is created");
    std::fs::write(wows_dir.0.join("replays/tempArenaInfo.json"), ARENA_INFO).expect("the arena info is written");
    let dir = wows_dir.0.display().to_string();
    with_test_database(async |pool| {
        wows_toolkit_config::queries::set_setting(pool, wows_toolkit_viewmodel::settings::keys::WOWS_DIR, &dir)
            .await
            .expect("the WoWs directory is set");
    });

    let mut app = app();
    open_player_tracker(&mut app);
    app.press("id:tab-current-match").unwrap();
    app.wait_for_within("Harvey635", std::time::Duration::from_secs(10)).unwrap();
    app.wait_for("Allies").unwrap();
    app.wait_for("Enemies").unwrap();
    app.wait_for("text:/^Stats unavailable/").unwrap();
    // The index met the enemy in three battles.
    app.wait_for_within("label:/^Seen 3 times total/", std::time::Duration::from_secs(10)).unwrap();

    std::fs::remove_file(wows_dir.0.join("replays/tempArenaInfo.json")).expect("the battle ends");
    app.wait_for_within("No battle in progress", std::time::Duration::from_secs(10)).unwrap();
    with_test_database(async |pool| {
        wows_toolkit_config::queries::set_setting(pool, wows_toolkit_viewmodel::settings::keys::WOWS_DIR, &"")
            .await
            .expect("the WoWs directory is cleared");
    });
}

#[test]
fn search_queries_the_shared_replay_index() {
    const FILE: &str = "20260223_115252_PZSC718-Narai_s06_Atoll.wowsreplay";
    const OTHER_FILE: &str = "20260213_203056_PFSD210-Marceau_22_tierra_del_fuego.wowsreplay";
    let mut app = app();
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/replays");
    let replay = fixtures.join(FILE);
    let other_replay = fixtures.join(OTHER_FILE);
    let indexed_replay = replay.clone();
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            use std::path::Path;

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

    app.press("Search").unwrap();
    app.wait_for("Search indexed replays").unwrap();
    app.type_text("label:Replay search query", "map:Ocean and outcome:win").unwrap();
    app.press("Run Search").unwrap();
    app.wait_for("Harugumo").unwrap();
    app.wait_for("123456").unwrap();
    app.wait_for("1 matches").unwrap();

    app.press(&format!("label:Open replay {}", replay.display())).unwrap();
    app.wait_for("PVEBattle").unwrap();
    app.wait_for("Players (15)").unwrap();
    app.on_pick_paths(vec![other_replay]).unwrap();
    app.press("Open Replay File...").unwrap();
    app.wait_for("Players (24)").unwrap();
    app.press("Search").unwrap();
    app.press(&format!("label:Open replay {}", replay.display())).unwrap();
    app.wait_for("Players (15)").unwrap();
    app.press("Search").unwrap();
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
