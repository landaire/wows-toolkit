use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, MutexGuard, OnceLock};

use nsh_core::{Points, Size};

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test directory is created");
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

/// Points the settings database at this process's own directory. Every test
/// path that opens the database goes through here first, so no test can
/// reach the user's real database. The database is created once, under the
/// cell's lock, so parallel tests never race its migrations.
fn configure_test_storage() {
    let path = TEST_STORAGE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("wows-toolkit-nsh-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("the test settings directory is created");
        wows_toolkit_config::storage_override::set(path.clone());
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a database runtime starts")
            .block_on(wows_toolkit_config::open_db())
            .expect("the test database is created");
        path
    });
    wows_toolkit_config::storage_override::set(path.clone());
    // The first-run questions are the shell's tests' to answer; here they
    // would stand in front of every tab.
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a database runtime starts").block_on(
        async {
            let pool = wows_toolkit_config::open_db().await.expect("the test database opens");
            for key in ["build_consent_window_shown", "replay_consent_prompt_shown", "language_selection_shown"] {
                wows_toolkit_config::queries::set_setting_raw(&pool, key, "true")
                    .await
                    .expect("a question is answered");
            }
            wows_toolkit_config::queries::set_setting_raw(&pool, "check_for_updates", "false")
                .await
                .expect("the startup update check is off");
        },
    );
    assert!(wows_toolkit_config::db_path().starts_with(path), "the tests open their own database, never the user's");
}

fn registries() -> Vec<nsh_core::element::Registry> {
    vec![nsh_base::registry(), wows_toolkit_nsh::registry()]
}

fn start() -> nsh_testing::Headless {
    configure_test_storage();
    let entry = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/App.tsx");
    let mut app = nsh_testing::tsx(&entry, registries()).expect("the TSX app starts");
    app.set_viewport(Size::new(Points(1400.0), Points(1000.0))).expect("the desktop viewport is applied");
    app
}

fn app() -> TestApp {
    let guard = TEST_APP_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    TestApp { app: start(), _guard: guard }
}

impl TestApp {
    /// Closes the app and starts it again, holding the test lock throughout
    /// so no other test writes the shared database in between.
    fn restart(self) -> Self {
        let TestApp { app, _guard } = self;
        drop(app);
        TestApp { app: start(), _guard }
    }
}

/// The session the Stats tests read: two Yamato games and a Montana game,
/// one of them ranked, with the tab's saved filters, charts and layout reset
/// so each test opens on the defaults.
fn seed_stats() {
    use wows_toolkit_config::queries::SessionStatRow;

    with_test_database(async |pool| {
        wows_toolkit_config::queries::clear_session_stats(pool).await.expect("old test statistics are cleared");
        for key in [
            "stats_charts",
            "stats_charts_dock_layout",
            "session_stats_limit_enabled",
            "session_stats_game_count",
            "session_stats_division_filter",
            "session_stats_game_mode_filter",
        ] {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(key)
                .execute(pool)
                .await
                .expect("a saved stats row is reset");
        }
        let game = |sort_key: &str, ship_name: &str, ship_id: i64, damage: i64, is_win: bool, match_group: &str| {
            SessionStatRow {
                id: 0,
                sort_key: sort_key.to_owned(),
                ship_name: ship_name.to_owned(),
                ship_id,
                player_id: 7,
                game_time: format!("{sort_key} 12:00:00"),
                match_group: match_group.to_owned(),
                damage,
                spotting_damage: 10_000,
                frags: 2,
                raw_xp: 1_500,
                base_xp: 1_000,
                is_win,
                is_loss: !is_win,
                is_draw: false,
                is_div: false,
                achievements: "[]".to_owned(),
            }
        };
        for row in [
            game("2026-01-01", "Yamato", 42, 100_000, true, "pvp"),
            game("2026-01-02", "Montana", 43, 80_000, false, "ranked"),
            game("2026-01-03", "Yamato", 42, 120_000, true, "pvp"),
        ] {
            wows_toolkit_config::queries::add_session_stat(pool, &row).await.expect("a test statistic is recorded");
        }
    });
}

/// A saved row's raw JSON, `None` while it is not written.
fn saved_setting(key: &str) -> Option<String> {
    let key = key.to_owned();
    with_test_database(async move |pool| {
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(&key)
            .fetch_optional(pool)
            .await
            .expect("the settings table reads")
    })
}

fn wait_for_setting(app: &mut TestApp, key: &str, check: impl Fn(&str) -> bool) {
    // The rows are written off the screen's timeline (a debounced save), so
    // they are polled rather than waited for on a screen change.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !saved_setting(key).is_some_and(|value| check(&value)) {
        assert!(std::time::Instant::now() < deadline, "the {key} row was not saved; saved: {:?}", saved_setting(key));
        app.settle();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn open_stats(app: &mut TestApp) {
    app.press("Stats").unwrap();
    app.wait_for("2W/1L (66.7%)").unwrap();
}

#[test]
fn stats_overview_summarizes_the_shared_session() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);

    app.wait_for("6 frags").unwrap();
    app.wait_for("Max DMG: Yamato (120,000)").unwrap();
    app.wait_for("Best: Yamato (2 kills)").unwrap();
    app.wait_for("100.0%").unwrap();
    app.wait_for("110,000").unwrap();
    app.wait_for("0.0%").unwrap();
}

#[test]
fn stats_ships_expand_copy_and_search() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);

    app.press("Ships").unwrap();
    app.press("label:Yamato 2W/0L (100%)").unwrap();
    app.wait_for("Spotting Damage").unwrap();
    app.wait_for("220,000").unwrap();

    app.press("label:Copy this table: Yamato").unwrap();
    app.press("Copy as Markdown").unwrap();
    app.wait_until("the table is copied", std::time::Duration::from_secs(5), |screen| {
        screen.clipboard().is_some_and(|copied| copied.starts_with("**Yamato 2W/0L (100%)**"))
    })
    .unwrap();

    app.type_text("label:Search ships...", "mont").unwrap();
    app.wait_for("1/2").unwrap();
    app.wait_for_gone("label:Yamato 2W/0L (100%)", std::time::Duration::from_secs(5)).unwrap();
    app.wait_for("label:Montana 0W/1L (0%)").unwrap();
}

#[test]
fn stats_clearing_a_ship_asks_again_unless_ctrl_is_held() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);
    app.press("Ships").unwrap();

    app.press("label:Remove all Montana games (Ctrl+Click to skip confirmation)").unwrap();
    app.wait_for("Press again to forget this ship's games").unwrap();
    app.press(
        "label:Press again to forget this ship's games: Remove all Montana games (Ctrl+Click to skip confirmation)",
    )
    .unwrap();
    app.wait_for("1/1").unwrap();
    app.wait_for_gone("Montana", std::time::Duration::from_secs(5)).unwrap();

    app.hold_keys("ctrl", true).unwrap();
    app.press("label:Remove all Yamato games (Ctrl+Click to skip confirmation)").unwrap();
    app.hold_keys("ctrl", false).unwrap();
    app.wait_for("No games recorded for the current filters").unwrap();
    let remaining = with_test_database(async |pool| {
        wows_toolkit_config::queries::get_all_session_stats(pool).await.expect("the session reads").len()
    });
    assert_eq!(remaining, 0);
}

#[test]
fn stats_filters_narrow_the_session_and_are_saved_for_the_other_apps() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);

    app.press("label:Mode: All").unwrap();
    app.press("Ranked").unwrap();
    app.wait_for("0W/1L (0.0%)").unwrap();
    wait_for_setting(&mut app, "session_stats_game_mode_filter", |value| value == r#"["ranked"]"#);
    app.press("label:Mode: All").unwrap();
    app.wait_for("2W/1L (66.7%)").unwrap();

    app.press("label:Limit to recent games").unwrap();
    wait_for_setting(&mut app, "session_stats_limit_enabled", |value| value == "true");
    wait_for_setting(&mut app, "session_stats_game_count", |value| value == "25");

    app.press("label:Division: Division").unwrap();
    wait_for_setting(&mut app, "session_stats_division_filter", |value| value == r#""DivOnly""#);
    app.wait_for("No games recorded for the current filters").unwrap();
}

#[test]
fn stats_charts_are_added_configured_and_closed_in_the_shared_rows() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);
    app.wait_for("label:Settings: Damage").unwrap();

    app.press("label:Add chart").unwrap();
    app.press("Frags").unwrap();
    app.wait_for("label:Settings: Frags").unwrap();
    wait_for_setting(&mut app, "stats_charts", |value| value.contains(r#""stat":"Frags""#));
    wait_for_setting(&mut app, "stats_charts_dock_layout", |value| value.contains(r#""id":1"#));

    app.press("label:Settings: Frags").unwrap();
    app.press("Bar").unwrap();
    app.wait_for("label:Settings: Avg Frags").unwrap();
    wait_for_setting(&mut app, "stats_charts", |value| value.contains(r#""mode":"Bar""#));

    app.press("label:Close Avg Frags").unwrap();
    app.wait_for_gone("label:Settings: Avg Frags", std::time::Duration::from_secs(5)).unwrap();
    wait_for_setting(&mut app, "stats_charts", |value| !value.contains("Frags"));
    wait_for_setting(&mut app, "stats_charts_dock_layout", |value| !value.contains(r#""id":1"#));
}

#[test]
fn stats_chart_ship_filter_selects_ships() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);

    app.press("label:Filter chart ships: 2 of 2 selected").unwrap();
    app.press("label:Montana").unwrap();
    app.wait_for("label:Filter chart ships: 1 of 2 selected").unwrap();
    wait_for_setting(&mut app, "stats_charts", |value| value.contains(r#""selected_ships":[42]"#));
    app.press("label:Select all matching ships").unwrap();
    app.wait_for("label:Filter chart ships: 2 of 2 selected").unwrap();
}

#[test]
fn stats_session_clears_only_after_confirmation() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);

    app.press("Clear").unwrap();
    app.wait_for("This will clear all session stats. Continue?").unwrap();
    app.press("Cancel").unwrap();
    app.wait_for("2W/1L (66.7%)").unwrap();

    app.press("Clear").unwrap();
    app.press("id:stats-clear-confirm").unwrap();
    app.wait_for("No games recorded for the current filters").unwrap();
}

#[test]
fn stats_chart_dock_collapses_to_a_rail_and_expands() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);
    app.wait_for("label:Settings: Damage").unwrap();

    app.press("label:Collapse Panel").unwrap();
    app.wait_for_gone("label:Settings: Damage", std::time::Duration::from_secs(5)).unwrap();
    wait_for_setting(&mut app, "stats_charts_dock_layout", |value| value.contains(r#""open":false"#));

    app.press("label:Expand Panel").unwrap();
    app.wait_for("label:Settings: Damage").unwrap();
    assert!(!app.shows("label:Expand Panel"), "the rail goes once the dock is open");
}

#[test]
fn a_language_change_rewords_stats_tracker_and_search() {
    let mut app = app();
    seed_stats();
    open_stats(&mut app);
    app.wait_for("label:Limit to recent games").unwrap();
    app.press("label:Division: Division").unwrap();
    wait_for_setting(&mut app, "session_stats_division_filter", |value| value == r#""DivOnly""#);
    app.press("Player Tracker").unwrap();
    app.wait_for("label:Player Filter").unwrap();

    app.press("Settings").unwrap();
    app.press("label:Language").unwrap();
    app.press("Deutsch").unwrap();

    app.press("text:/^(Stats|Statistiken)$/").unwrap();
    app.wait_for("label:Auf letzte Gefechte begrenzen").unwrap();
    // Rewording reads the scope again; it neither changes nor saves the filters.
    assert_eq!(saved_setting("session_stats_division_filter").as_deref(), Some(r#""DivOnly""#));
    app.press("text:/^(Player Tracker|Spieler-Tracker)$/").unwrap();
    app.wait_for("label:Spielerfilter").unwrap();
    app.press("text:/^(Search|Suche)$/").unwrap();
    app.wait_for("id:search-run >> text:Suche").unwrap();

    // The app writes the choice in the background. A reset made before that
    // write lands is overwritten, and the next test starts in German.
    wait_for_setting(&mut app, "locale", |value| value == r#""de""#);
    // The next test starts in English.
    with_test_database(async |pool| {
        wows_toolkit_config::queries::set_setting_raw(pool, "locale", r#""en""#).await.expect("the locale is reset");
    });
}

/// Runs `work` against the test database.
fn with_test_database<T>(work: impl AsyncFnOnce(&sqlx::SqlitePool) -> T) -> T {
    configure_test_storage();
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

    configure_test_storage();
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
                    names: std::iter::once(seeded.name)
                        .chain(seeded.aliases.iter().copied())
                        .map(String::from)
                        .collect(),
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

/// Waits until the players table lists `first` then `second` at its top. The
/// rows are read by list position, since a list's recycled cells keep no
/// tree order; a header shows its new order before the reloaded rows land.
fn wait_for_order(app: &mut TestApp, first: &str, second: &str) {
    app.wait_until(&format!("{first} is listed above {second}"), std::time::Duration::from_secs(5), |screen| {
        let at =
            |row: usize, name: &str| screen.matches(&format!("id:player-row-{row} >> text:{name}")).unwrap_or(false);
        at(0, first) && at(1, second)
    })
    .unwrap();
}

fn open_player_tracker(app: &mut TestApp) {
    app.press("Player Tracker").unwrap();
    app.wait_for("OtherOpponent").unwrap();
    app.wait_for("TrackedCaptain").unwrap();
}

#[test]
fn player_tracker_changes_period_filters_and_cycles_the_sort() {
    let mut app = app();
    seed_player_tracker();
    open_player_tracker(&mut app);

    // The past day, with division mates hidden, most-met first.
    assert!(!app.shows("AncientFoe"), "a player met three days ago is outside the past day");
    assert!(!app.shows("DivisionMate"), "a player met only in division is hidden");
    wait_for_order(&mut app, "OtherOpponent", "TrackedCaptain");
    app.wait_for("role:columnheader[Encounters in Time Range] >> id:sort-descending").unwrap();

    // A new column opens on its own order, the same one flips, a third press
    // puts the table back the way it opened.
    app.press("role:columnheader[Player Name]").unwrap();
    app.wait_for("role:columnheader[Player Name] >> id:sort-ascending").unwrap();
    wait_for_order(&mut app, "OtherOpponent", "TrackedCaptain");
    app.press("role:columnheader[Player Name]").unwrap();
    app.wait_for("role:columnheader[Player Name] >> id:sort-descending").unwrap();
    wait_for_order(&mut app, "TrackedCaptain", "OtherOpponent");
    app.press("role:columnheader[Player Name]").unwrap();
    app.wait_for("role:columnheader[Encounters in Time Range] >> id:sort-descending").unwrap();
    wait_for_order(&mut app, "OtherOpponent", "TrackedCaptain");

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
    let mut app = app();
    seed_player_tracker();
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

    let mut app = app();
    seed_player_tracker();
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
    let mut app = app();
    seed_player_tracker();
    open_player_tracker(&mut app);

    app.press("Clans").unwrap();
    app.wait_for("NAVY").unwrap();
    app.wait_for("TEST").unwrap();
    app.press("label:Expand NAVY").unwrap();
    app.wait_for("seen in 3 matches").unwrap();
    app.press("label:Find matches OtherOpponent").unwrap();
    app.wait_for("Search").unwrap();
    app.wait_for("id:search-pill-0 >> text:#99002").unwrap();
}

#[test]
fn player_tracker_clears_only_after_confirmation() {
    let mut app = app();
    seed_player_tracker();
    open_player_tracker(&mut app);
    let tracked_count = || {
        with_test_database(async |pool| wows_toolkit_viewmodel::player_tracker::store::load(pool).await.unwrap().len())
    };

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

    let mut app = app();
    seed_player_tracker();
    open_player_tracker(&mut app);
    app.press("role:columnheader[Player Name]").unwrap();
    app.wait_for("role:columnheader[Player Name] >> id:sort-ascending").unwrap();
    app.press("label:Time period").unwrap();
    app.press("All Time").unwrap();
    app.wait_for("AncientFoe").unwrap();
    app.type_text("label:Player Filter", "Foe").unwrap();
    app.wait_for_gone("TrackedCaptain", std::time::Duration::from_secs(5)).unwrap();
    app.press("Current Match").unwrap();
    app.press("Compact").unwrap();
    // Compact offers the scope selector; with no battle there is no roster.
    app.wait_for("Ship").unwrap();
    app.wait(std::time::Duration::from_millis(300)).unwrap();

    let saved = with_test_database(async |pool| {
        wows_toolkit_config::queries::get_setting::<TrackerSettings>(pool, "player_tracker").await
    })
    .expect("the view is saved under the key the GPUI tracker reads");
    assert_eq!(saved.period, TimePeriod::AllTime);
    assert_eq!(saved.sort.column, SortColumn::Name);
    assert_eq!(saved.sort.order, SortOrder::Ascending);
    assert_eq!(saved.filter, "Foe");
    let modes =
        with_test_database(async |pool| wows_toolkit_viewmodel::player_tracker::store::load_view_modes(pool).await);
    assert_eq!(
        modes.current_match_view_mode,
        wows_toolkit_viewmodel::player_tracker::live::CurrentMatchViewMode::Compact
    );

    let mut app = app.restart();
    app.press("Player Tracker").unwrap();
    app.wait_for("AncientFoe").unwrap();
    app.wait_for("role:columnheader[Player Name] >> id:sort-ascending").unwrap();
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

    let mut app = app();
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

    open_player_tracker(&mut app);
    app.press("Current Match").unwrap();
    app.wait_for_within("Harvey635", std::time::Duration::from_secs(10)).unwrap();
    app.wait_for("Allies").unwrap();
    app.wait_for("Enemies").unwrap();
    // No fixture build is reachable: the directory holds no game, so the
    // battle's build does not load and the ships and figures say why.
    app.wait_for_within(
        "text:/^Stats unavailable: this battle's game data did not load: /",
        std::time::Duration::from_secs(10),
    )
    .unwrap();
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
fn top_level_navigation_and_application_menu_work() {
    let mut app = app();

    app.wait_for("Replay Inspector").unwrap();
    app.press_key("mod+k").unwrap();
    app.wait_for("Commands").unwrap();
    app.press("Go to: Settings").unwrap();
    app.wait_for("Application Settings").unwrap();

    app.press("label:File").unwrap();
    app.wait_for("Tactics Board").unwrap();
    app.press("About").unwrap();
    app.wait_for("Made by landaire.").unwrap();
    app.press("Cancel").unwrap();
    app.wait_for_gone("Made by landaire.", std::time::Duration::from_secs(5)).unwrap();

    app.press("label:File").unwrap();
    app.press("Tactics Board").unwrap();
    // The board opens in a window of its own, which only the desktop runner
    // makes; without it the request fails and says so.
    app.wait_for("The tactics board window could not be opened").unwrap();
    assert!(!app.screen().matches("label:Ranges").unwrap(), "the board is not drawn in the main window");
}

#[test]
fn command_palette_supports_keyboard_navigation_and_compact_viewports() {
    let mut app = app();
    app.set_viewport(Size::new(Points(500.0), Points(360.0))).expect("the compact viewport is applied");

    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "About").unwrap();
    app.press_key("down").unwrap();
    app.press_key("enter").unwrap();
    app.wait_for("Made by landaire.").unwrap();
    app.press("Cancel").unwrap();

    app.press_key("mod+k").unwrap();
    app.type_text("label:Command palette", "nomatch").unwrap();
    app.wait_for("No matches").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for("Go to: Replay Inspector").unwrap();
    app.press_key("escape").unwrap();
    app.wait_for_gone("Commands", std::time::Duration::from_secs(5)).unwrap();
}
