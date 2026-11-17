use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::Row;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_core::game_types::MatchContributionVersion;
use wows_core::rating_scope::RatedBattle;
use wows_core::rating_scope::RatingScope;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::IndexWriteMode;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::ResultsWrite;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::SourceKind;
use wows_toolkit_config::index::rows::VehicleRelation;

async fn mem_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn sample_match(arena: i64) -> ObjectiveMatch {
    ObjectiveMatch {
        arena_id: ArenaId::new(arena),
        timestamp: Timestamp::from_second(1_700_000_000).unwrap(),
        map: "Ocean".into(),
        game_mode: "Domination".into(),
        game_mode_id: None,
        game_type: "RandomBattle".into(),
        match_group: "pvp".into(),
        version_build: Some(1234),
    }
}

fn sample_record(arena: i64, source: SourceId, path: &str) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(path),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId::from(7)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(123_456),
        self_kills: Some(2),
        self_pr: Some(1500.0),
        results_available: true,
        indexed_at: Timestamp::from_second(1_700_000_100).unwrap(),
    }
}

fn sample_vehicle(arena: i64) -> IndexedVehicleRow {
    IndexedVehicleRow {
        arena_id: ArenaId::new(arena),
        account_id: AccountId::from(7),
        player_name: "Player".into(),
        clan: "CLAN".into(),
        realm: None,
        ship_id: GameParamId::from(999u64),
        ship_index: "PASA001".into(),
        ship_name: "Test Ship".into(),
        nation: "USA".into(),
        species: "Cruiser".into(),
        tier: 8,
        relation: VehicleRelation::SelfPlayer,
        division_id: None,
        survived: Some(true),
        damage: Some(123_456),
        kills: Some(2),
        spotting: None,
        potential: None,
        received: None,
        pr: Some(1500.0),
        is_test_ship: false,
        disconnected: Some(false),
        is_stream_sniper: None,
        sniper_twitch_login: None,
        mc: None,
        mc_version: None,
        mc_torpedoes_observed: false,
    }
}

/// Like [`sample_record`], but with the fields the migration's collision
/// preference actually compares (`results_available`, `indexed_at`) and a
/// distinguishing `self_damage` so a test can tell which of two colliding
/// rows won.
fn record_with_results(
    arena: i64,
    source: SourceId,
    path: &str,
    results_available: bool,
    self_damage: u64,
    indexed_at: i64,
) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(path),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId::from(7)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(self_damage),
        self_kills: Some(2),
        self_pr: Some(1500.0),
        results_available,
        indexed_at: Timestamp::from_second(indexed_at).unwrap(),
    }
}

/// Rebuilds the pre-008 shape inside an already-migrated `mem_pool()` (its
/// migration 008 indexes are dropped) and creates a second `live` source at a
/// higher id than the default one, reproducing the duplicate-live-source
/// condition a racing first launch could leave behind. Returns
/// `(survivor, doomed)`.
async fn two_live_sources(pool: &SqlitePool, now: Timestamp) -> (SourceId, SourceId) {
    sqlx::query("DROP INDEX idx_source_single_live").execute(pool).await.unwrap();
    sqlx::query("DROP INDEX idx_source_root_path").execute(pool).await.unwrap();

    let survivor = query::ensure_default_source(pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let doomed_row: (i64,) = sqlx::query_as(
        "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) RETURNING source_id",
    )
    .bind("Live replays (dup)")
    .bind(SourceKind::Live.as_db_str())
    .bind("D:/other/replays")
    .bind(now.as_second())
    .fetch_one(pool)
    .await
    .unwrap();
    let doomed = SourceId(doomed_row.0);
    assert!(doomed.0 > survivor.0, "the doomed source must have the higher id for this scenario to be meaningful");
    (survivor, doomed)
}

#[tokio::test]
async fn a_second_live_source_cannot_be_inserted() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    // The uniqueness constraint is what makes the two-writer race harmless.
    let second: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> =
        sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4)")
            .bind("Live replays")
            .bind(SourceKind::Live.as_db_str())
            .bind("D:/other/replays")
            .bind(now.as_second())
            .execute(&pool)
            .await;

    assert!(second.is_err(), "a second live source must violate the unique index");
}

#[tokio::test]
async fn two_sources_cannot_share_a_root_path() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    let dup: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> =
        sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4)")
            .bind("Imported")
            .bind(SourceKind::ImportedDir.as_db_str())
            .bind("C:/wows/replays")
            .bind(now.as_second())
            .execute(&pool)
            .await;

    assert!(dup.is_err(), "root_path must be unique across sources");
}

#[tokio::test]
async fn several_sources_may_have_a_null_root_path() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    for name in ["a", "b"] {
        sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, NULL, ?3)")
            .bind(name)
            .bind(SourceKind::AdHoc.as_db_str())
            .bind(now.as_second())
            .execute(&pool)
            .await
            .expect("NULL root_path must not participate in the unique index");
    }
}

#[tokio::test]
async fn ensure_default_source_id_round_trips_through_live_source_id() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let created = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let read = query::live_source_id(&pool).await.unwrap();
    assert_eq!(read, Some(created));
}

#[tokio::test]
async fn single_live_source_read_write_round_trips_after_migration_008() {
    // With only one live source the dedupe statements have nothing to do;
    // this only proves migration 008 does not break the ordinary insert/read
    // path. It does not exercise the dedupe logic itself -- see
    // migration_dedupes_two_live_sources_repointing_non_colliding_records and
    // its siblings for that.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(paths.contains("a.wowsreplay"));
}

#[tokio::test]
async fn an_old_index_revision_is_reparsed_once_within_the_scanned_directory() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("C:/wows/replays");
    let source = query::ensure_default_source(&pool, root, now).await.unwrap();
    let record = sample_record(100, source, "C:/wows/replays/a.wowsreplay");
    let elsewhere = sample_record(200, source, "D:/old/replays/b.wowsreplay");
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &record).await.unwrap();
    query::upsert_record(&pool, &elsewhere).await.unwrap();
    sqlx::query("UPDATE replay_record SET index_revision = 0").execute(&pool).await.unwrap();

    assert!(query::record_paths_in_source(&pool, source, None).await.unwrap().is_empty());
    assert!(query::has_stale_replay_records(&pool, root, None).await.unwrap());

    query::upsert_record(&pool, &record).await.unwrap();
    assert!(query::record_paths_in_source(&pool, source, None).await.unwrap().contains("C:/wows/replays/a.wowsreplay"));
    assert!(!query::has_stale_replay_records(&pool, root, None).await.unwrap());

    sqlx::query("UPDATE replay_record SET index_revision = 0").execute(&pool).await.unwrap();
    query::mark_replay_records_current(&pool, source, root, None).await.unwrap();
    assert!(!query::has_stale_replay_records(&pool, root, None).await.unwrap());
    let current = query::record_paths_in_source(&pool, source, None).await.unwrap();
    assert!(!current.contains("D:/old/replays/b.wowsreplay"), "a directory that was not scanned stays stale");
}

#[tokio::test]
async fn a_reparse_rule_refreshes_only_the_builds_it_names() {
    const BUILD_15_9: u32 = 13_357_625;
    const BUILD_15_10: u32 = 13_533_378;

    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("C:/wows/replays");
    let source = query::ensure_default_source(&pool, root, now).await.unwrap();
    let older = sample_record(100, source, "C:/wows/replays/15_9.wowsreplay");
    let newer = sample_record(200, source, "C:/wows/replays/15_10.wowsreplay");
    query::upsert_match(&pool, &ObjectiveMatch { version_build: Some(BUILD_15_9), ..sample_match(100) }).await.unwrap();
    query::upsert_match(&pool, &ObjectiveMatch { version_build: Some(BUILD_15_10), ..sample_match(200) })
        .await
        .unwrap();
    query::upsert_record(&pool, &older).await.unwrap();
    query::upsert_record(&pool, &newer).await.unwrap();
    // Both rows as the 1.0.3 indexer left them.
    sqlx::query("UPDATE replay_record SET index_revision = 1").execute(&pool).await.unwrap();

    let current = query::record_paths_in_source(&pool, source, None).await.unwrap();
    assert!(current.contains("C:/wows/replays/15_9.wowsreplay"), "a 15.9 row needs no reparse");
    assert!(!current.contains("C:/wows/replays/15_10.wowsreplay"), "a 15.10 row from the old indexer is reparsed");

    query::upsert_record(&pool, &newer).await.unwrap();
    assert!(
        query::record_paths_in_source(&pool, source, None).await.unwrap().contains("C:/wows/replays/15_10.wowsreplay")
    );
    assert!(!query::has_stale_replay_records(&pool, root, None).await.unwrap());
}

async fn store_stale_ship(pool: &SqlitePool) {
    query::upsert_match(pool, &sample_match(100)).await.unwrap();
    let mut stale = sample_vehicle(100);
    stale.ship_id = GameParamId::from(111u64);
    stale.ship_index = "STALE".to_owned();
    stale.is_stream_sniper = Some(true);
    stale.sniper_twitch_login = Some("viewer".to_owned());
    query::upsert_vehicles(pool, &[stale]).await.unwrap();
}

async fn write_corrected(pool: &SqlitePool, vehicle: IndexedVehicleRow, mode: IndexWriteMode, results: ResultsWrite) {
    write_replay(pool, vehicle, mode, results, RatingScope::Rated(RatedBattle::Random)).await;
}

async fn write_replay(
    pool: &SqlitePool,
    vehicle: IndexedVehicleRow,
    mode: IndexWriteMode,
    results: ResultsWrite,
    rating_scope: RatingScope,
) {
    let source = query::ensure_default_source(pool, Path::new("C:/wows/replays"), Timestamp::now()).await.unwrap();
    let record = sample_record(100, source, "C:/wows/replays/a.wowsreplay");
    query::write_indexed_replay(pool, &sample_match(100), &[vehicle], &record, mode, results, rating_scope)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_untrusted_rewrite_moves_stored_results_to_a_corrected_ship() {
    let pool = mem_pool().await;
    store_stale_ship(&pool).await;

    let mut corrected = sample_vehicle(100);
    corrected.damage = None;
    corrected.kills = None;
    corrected.pr = None;
    write_corrected(&pool, corrected, IndexWriteMode::Replace, ResultsWrite::Keep).await;

    let rows =
        sqlx::query("SELECT ship_id, damage, kills, pr, is_stream_sniper FROM indexed_vehicle WHERE arena_id = 100")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<i64, _>("ship_id"), 999);
    assert_eq!(rows[0].get::<i64, _>("damage"), 123_456);
    assert_eq!(rows[0].get::<i64, _>("kills"), 2);
    assert_eq!(rows[0].get::<f64, _>("pr"), 1500.0);
    assert_eq!(rows[0].get::<Option<bool>, _>("is_stream_sniper"), Some(true));
}

#[tokio::test]
async fn an_incremental_write_replaces_a_corrected_ship_and_keeps_its_rating() {
    let pool = mem_pool().await;
    store_stale_ship(&pool).await;

    let mut corrected = sample_vehicle(100);
    corrected.damage = Some(5);
    corrected.pr = Some(900.0);
    corrected.is_stream_sniper = Some(false);
    write_corrected(&pool, corrected, IndexWriteMode::Incremental, ResultsWrite::Store).await;

    let rows = sqlx::query(
        "SELECT ship_id, damage, pr, is_stream_sniper, sniper_twitch_login FROM indexed_vehicle WHERE arena_id = 100",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<i64, _>("ship_id"), 999);
    assert_eq!(rows[0].get::<i64, _>("damage"), 5);
    assert_eq!(rows[0].get::<f64, _>("pr"), 1500.0, "a stored rating is not restamped");
    assert_eq!(rows[0].get::<Option<bool>, _>("is_stream_sniper"), Some(false));
    assert_eq!(rows[0].get::<Option<String>, _>("sniper_twitch_login"), None);
}

#[tokio::test]
async fn migration_dedupes_two_live_sources_repointing_non_colliding_records() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_match(&pool, &sample_match(300)).await.unwrap();

    // Already on the survivor; the migration must leave it alone.
    query::upsert_record(&pool, &sample_record(100, survivor, "a.wowsreplay")).await.unwrap();
    // Only on the doomed source; the migration must repoint it onto the survivor.
    query::upsert_record(&pool, &sample_record(200, doomed, "b.wowsreplay")).await.unwrap();
    // Both sources independently indexed the same physical file (the two-writer
    // race the migration is repairing): same arena and path, one row per source.
    // The doomed copy must be dropped rather than repointed, because repointing
    // it would collide with the survivor's own row at (source_id, replay_path).
    query::upsert_record(&pool, &sample_record(300, survivor, "shared.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(300, doomed, "shared.wowsreplay")).await.unwrap();

    // Run the real migration body, not a paraphrase, so this test cannot drift
    // from what actually ships.
    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let live_sources: Vec<(i64,)> = sqlx::query_as("SELECT source_id FROM index_source WHERE kind = ?1")
        .bind(SourceKind::Live.as_db_str())
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(live_sources, vec![(survivor.0,)], "exactly one live source must remain, at the lowest id");

    let paths = query::record_paths_in_source(&pool, survivor, None).await.unwrap();
    assert!(paths.contains("a.wowsreplay"), "the survivor's own record must be untouched");
    assert!(paths.contains("b.wowsreplay"), "the non-colliding doomed record must be repointed onto the survivor");
    assert!(paths.contains("shared.wowsreplay"), "the survivor's copy of the colliding record must remain");

    let shared_rows: Vec<(i64, i64)> =
        sqlx::query_as("SELECT source_id, arena_id FROM replay_record WHERE replay_path = 'shared.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        shared_rows,
        vec![(survivor.0, 300)],
        "the doomed source's colliding copy must be dropped, not repointed"
    );

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM replay_record").fetch_one(&pool).await.unwrap();
    assert_eq!(total.0, 3, "one duplicate record must be dropped; the other three must remain");
}

/// One row's `self_damage` after the migration, for the single-path fixtures
/// the collision-preference tests use.
async fn self_damage_at(pool: &SqlitePool, path: &str) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT self_damage FROM replay_record WHERE replay_path = ?1")
        .bind(path)
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

#[tokio::test]
async fn migration_dedupe_prefers_the_doomed_row_when_it_has_results_and_the_survivor_does_not() {
    // The two live sources are populated by separate threads at different
    // times, so a record present under both can disagree: one thread indexed
    // it before WG post-battle results were written, the other after.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, survivor, "x.wowsreplay", false, 111, 1_700_000_000))
        .await
        .unwrap();
    query::upsert_record(&pool, &record_with_results(100, doomed, "x.wowsreplay", true, 999, 1_700_000_050))
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT source_id FROM replay_record WHERE replay_path = 'x.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![(survivor.0,)], "exactly one row must remain, repointed onto the survivor");
    assert_eq!(
        self_damage_at(&pool, "x.wowsreplay").await,
        999,
        "the results-bearing doomed row must win over the results-absent survivor row"
    );
}

#[tokio::test]
async fn migration_dedupe_keeps_the_survivor_row_when_it_already_has_results() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, survivor, "x.wowsreplay", true, 999, 1_700_000_050))
        .await
        .unwrap();
    query::upsert_record(&pool, &record_with_results(100, doomed, "x.wowsreplay", false, 111, 1_700_000_000))
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT source_id FROM replay_record WHERE replay_path = 'x.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![(survivor.0,)], "exactly one row must remain, already on the survivor");
    assert_eq!(
        self_damage_at(&pool, "x.wowsreplay").await,
        999,
        "the survivor's already-results-bearing row must be kept over the results-absent doomed row"
    );
}

#[tokio::test]
async fn migration_dedupe_prefers_results_over_recency() {
    // The other two preference tests covary results_available and indexed_at,
    // so neither pins the ordering between them: a predicate that compared
    // only indexed_at would pass both. Here the fields disagree -- the
    // survivor is more recent but results-absent, the doomed row is older but
    // results-bearing -- so only "results beats recency" picks the doomed row.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, survivor, "x.wowsreplay", false, 111, 1_700_000_100))
        .await
        .unwrap();
    query::upsert_record(&pool, &record_with_results(100, doomed, "x.wowsreplay", true, 999, 1_700_000_000))
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT source_id FROM replay_record WHERE replay_path = 'x.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![(survivor.0,)], "exactly one row must remain, repointed onto the survivor");
    assert_eq!(
        self_damage_at(&pool, "x.wowsreplay").await,
        999,
        "the older, results-bearing doomed row must win over the newer, results-absent survivor row"
    );
}

#[tokio::test]
async fn migration_dedupe_prefers_newer_indexed_at_when_results_available_ties() {
    // Both rows have results_available: true, so the first preference clause
    // (results-bearing beats results-absent) cannot distinguish them; only
    // the second clause (newer indexed_at wins) can pick a winner here.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (survivor, doomed) = two_live_sources(&pool, now).await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, survivor, "x.wowsreplay", true, 111, 10)).await.unwrap();
    query::upsert_record(&pool, &record_with_results(100, doomed, "x.wowsreplay", true, 999, 99)).await.unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let remaining: Vec<(i64,)> =
        sqlx::query_as("SELECT source_id FROM replay_record WHERE replay_path = 'x.wowsreplay'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![(survivor.0,)], "exactly one row must remain, repointed onto the survivor");
    assert_eq!(
        self_damage_at(&pool, "x.wowsreplay").await,
        999,
        "the doomed row with the newer indexed_at must win when results_available ties"
    );
}

#[tokio::test]
async fn migration_nulls_root_path_for_all_but_the_lowest_id_among_a_group_sharing_one() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    sqlx::query("DROP INDEX idx_source_single_live").execute(&pool).await.unwrap();
    sqlx::query("DROP INDEX idx_source_root_path").execute(&pool).await.unwrap();

    // Three ad-hoc sources sharing one root_path: the non-obvious case is
    // whether the correlated subquery picking MIN(source_id) per root_path
    // still holds across a group larger than two. A fourth, unrelated source
    // with its own root_path proves the migration does not touch rows outside
    // any duplicate group.
    let mut shared_ids = Vec::new();
    for name in ["shared-a", "shared-b", "shared-c"] {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) RETURNING source_id",
        )
        .bind(name)
        .bind(SourceKind::AdHoc.as_db_str())
        .bind("E:/shared")
        .bind(now.as_second())
        .fetch_one(&pool)
        .await
        .unwrap();
        shared_ids.push(row.0);
    }
    let lowest_shared = *shared_ids.iter().min().unwrap();

    sqlx::query("INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4)")
        .bind("unrelated")
        .bind(SourceKind::AdHoc.as_db_str())
        .bind("E:/other")
        .bind(now.as_second())
        .execute(&pool)
        .await
        .unwrap();

    const DEDUPE_SQL: &str = include_str!("../migrations/008_source_uniqueness.sql");
    sqlx::raw_sql(DEDUPE_SQL).execute(&pool).await.unwrap();

    let rows: Vec<(i64, Option<String>)> =
        sqlx::query_as("SELECT source_id, root_path FROM index_source ORDER BY source_id")
            .fetch_all(&pool)
            .await
            .unwrap();

    for (id, root_path) in &rows {
        if *id == lowest_shared {
            assert_eq!(root_path.as_deref(), Some("E:/shared"), "the lowest id in the group must keep the root_path");
        } else if shared_ids.contains(id) {
            assert_eq!(root_path.as_deref(), None, "every non-lowest id in the group must have root_path nulled out");
        } else {
            assert_eq!(root_path.as_deref(), Some("E:/other"), "a source outside the group must be untouched");
        }
    }
}

async fn row_count_for_root(pool: &SqlitePool, root: &str) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM index_source WHERE root_path = ?1")
        .bind(root)
        .fetch_one(pool)
        .await
        .unwrap();
    row.0
}

async fn name_for_root(pool: &SqlitePool, root: &str) -> String {
    let row: (String,) =
        sqlx::query_as("SELECT name FROM index_source WHERE root_path = ?1").bind(root).fetch_one(pool).await.unwrap();
    row.0
}

#[tokio::test]
async fn ensure_source_is_idempotent_for_a_repeated_root() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("D:/dump/replays");
    let first = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, root, now).await.unwrap();
    let second = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, root, now).await.unwrap();
    assert_eq!(first, second, "the same root must resolve to the same source");
}

#[tokio::test]
async fn ensure_source_distinguishes_roots() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let a = query::ensure_source(&pool, "A", SourceKind::ImportedDir, Path::new("D:/a"), now).await.unwrap();
    let b = query::ensure_source(&pool, "B", SourceKind::ImportedDir, Path::new("D:/b"), now).await.unwrap();
    assert_ne!(a, b);
}

#[tokio::test]
async fn ensure_source_returns_the_existing_row_rather_than_erroring_on_the_unique_index() {
    // The insert races another thread; recovering by re-selecting is what makes
    // the two-writer path safe rather than merely constrained.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("D:/dump/replays");
    let existing = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, root, now).await.unwrap();

    // A differently-named call for the same root must still resolve, not fail.
    let again = query::ensure_source(&pool, "Renamed", SourceKind::ImportedDir, root, now).await.unwrap();
    assert_eq!(existing, again);

    // The differing name proves the second insert really did collide on
    // root_path (a call with identical arguments could resolve to the same id
    // via a check-then-insert that never touches the conflict arm at all).
    // A single surviving row, still named "Dump", proves the conflict was
    // absorbed by ON CONFLICT DO NOTHING and recovered via the re-select,
    // rather than by inserting a second row or upserting over the first.
    assert_eq!(row_count_for_root(&pool, "D:/dump/replays").await, 1, "only one row may exist for the root");
    assert_eq!(
        name_for_root(&pool, "D:/dump/replays").await,
        "Dump",
        "the original row's name must survive untouched; DO NOTHING must not upsert"
    );
}

#[tokio::test]
async fn ensure_default_source_still_yields_the_live_source() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let via_default = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    assert_eq!(query::live_source_id(&pool).await.unwrap(), Some(via_default));
}

#[tokio::test]
async fn ensure_default_source_adopts_an_existing_source_for_the_same_root() {
    // If a directory is already registered as an imported_dir source before any
    // live source exists, ensure_default_source must adopt that existing source
    // rather than fail or create a second row for the same root_path (which the
    // unique index would reject anyway).
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let root = Path::new("C:/wows/replays");
    let imported = query::ensure_source(&pool, "Imported", SourceKind::ImportedDir, root, now).await.unwrap();

    let via_default = query::ensure_default_source(&pool, root, now).await.unwrap();
    assert_eq!(via_default, imported, "ensure_default_source must adopt the pre-existing source for this root");

    // No live source was created: the conflict on root_path prevented the
    // live-kind insert from ever landing a row.
    assert_eq!(
        query::live_source_id(&pool).await.unwrap(),
        None,
        "adopting an existing source must not create a live-kind row"
    );
    assert_eq!(row_count_for_root(&pool, "C:/wows/replays").await, 1, "only one row may exist for the root");
}

#[tokio::test]
async fn ensure_source_resolves_an_existing_live_source_at_a_different_root() {
    // The unqualified ON CONFLICT DO NOTHING also absorbs a conflict on the
    // single-live-source index, not just the root_path index. When a second
    // Live insert loses to that index, the row it should resolve to lives at
    // a DIFFERENT root_path than the one just passed in, so a re-select
    // scoped to root_path alone finds nothing; ensure_source must still
    // resolve to the existing Live source rather than erroring.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let first = query::ensure_source(&pool, "Live replays", SourceKind::Live, Path::new("D:/a"), now).await.unwrap();

    let second = query::ensure_source(&pool, "Live replays", SourceKind::Live, Path::new("D:/b"), now).await.unwrap();

    assert_eq!(second, first, "a second Live root must resolve to the existing Live source, not error");
    assert_eq!(row_count_for_root(&pool, "D:/b").await, 0, "no row may be created at the losing root");
    assert_eq!(query::live_source_id(&pool).await.unwrap(), Some(first), "the original Live source must be untouched");
}

/// A real directory `real/install/replays` and a link `link` to `real`.
#[cfg(unix)]
fn linked_install() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let real = tmp.path().join("real");
    std::fs::create_dir_all(real.join("install/replays")).expect("the install is made");
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("the link is made");

    (tmp, real, link)
}

#[cfg(unix)]
#[tokio::test]
async fn ensure_source_reuses_the_source_of_a_linked_spelling() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (_tmp, real, link) = linked_install();
    let stored = real.join("install/replays");
    let existing = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, &stored, now).await.unwrap();

    let linked = link.join("install/replays");
    let again = query::ensure_source(&pool, "Linked", SourceKind::ImportedDir, &linked, now).await.unwrap();

    assert_eq!(again, existing, "the linked spelling reuses the existing source");
    assert_eq!(row_count_for_root(&pool, &linked.to_string_lossy()).await, 0, "no row is made at the linked spelling");
    let found = query::source_at_root(&pool, &linked).await.unwrap().expect("the linked spelling finds the source");
    assert_eq!(found.id, existing);
    assert_eq!(found.root_path.as_deref(), Some(stored.as_path()), "the source keeps its spelling");
}

#[cfg(unix)]
#[tokio::test]
async fn an_unresolved_root_matches_only_as_written() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (tmp, real, _link) = linked_install();
    query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, &real.join("install/replays"), now).await.unwrap();
    let unmounted = tmp.path().join("unmounted/replays");

    assert!(query::source_at_root(&pool, &unmounted).await.unwrap().is_none());
    let made = query::ensure_source(&pool, "Unmounted", SourceKind::ImportedDir, &unmounted, now).await.unwrap();
    let found = query::source_at_root(&pool, &unmounted).await.unwrap().expect("the source is made");
    assert_eq!(found.id, made);
    assert_eq!(found.root_path.as_deref(), Some(unmounted.as_path()), "the given spelling is kept");
}

#[cfg(unix)]
#[tokio::test]
async fn a_linked_install_takes_the_live_source_spelling() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let (tmp, real, link) = linked_install();
    let linked = link.join("install");
    assert_eq!(query::live_install_spelling(&pool, &linked).await.unwrap(), linked, "no Live source keeps it");

    query::ensure_default_source(&pool, &real.join("install/replays"), now).await.unwrap();

    assert_eq!(query::live_install_spelling(&pool, &linked).await.unwrap(), real.join("install"));
    let unmounted = tmp.path().join("unmounted/install");
    assert_eq!(query::live_install_spelling(&pool, &unmounted).await.unwrap(), unmounted, "an unresolved one is kept");
    let other = tmp.path().join("other");
    std::fs::create_dir(&other).expect("the other directory is made");
    assert_eq!(query::live_install_spelling(&pool, &other).await.unwrap(), other, "another directory is kept");
}

#[tokio::test]
async fn relocate_source_rewrites_matching_prefixes_only() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/old/a.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "E:/elsewhere/b.wowsreplay")).await.unwrap();

    let moved = query::relocate_source(&pool, src, Path::new("D:/old"), Path::new("F:/new")).await.unwrap();
    assert_eq!(moved, 1, "only the record under the old root is repointed");

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(paths.contains("F:/new/a.wowsreplay"), "prefix rewritten: {paths:?}");
    assert!(paths.contains("E:/elsewhere/b.wowsreplay"), "non-matching path untouched: {paths:?}");

    let sources = query::list_sources(&pool).await.unwrap();
    let updated = sources.iter().find(|s| s.id == src).expect("source still present");
    assert_eq!(updated.root_path.as_deref(), Some(Path::new("F:/new")));
}

#[tokio::test]
async fn relocate_source_accepts_a_trailing_separator_on_old_root() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/old/a.wowsreplay")).await.unwrap();

    let moved = query::relocate_source(&pool, src, Path::new("D:/old/"), Path::new("F:/new")).await.unwrap();
    assert_eq!(moved, 1, "a trailing separator on old_root must not prevent the match");

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(paths.contains("F:/new/a.wowsreplay"), "prefix rewritten despite trailing separator: {paths:?}");
}

#[tokio::test]
async fn relocate_source_leaves_a_sibling_directory_with_a_shared_name_prefix_untouched() {
    // "D:/oldarchive" shares the literal characters "D:/old" with old_root
    // but is a different, sibling directory: relocating D:/old must not
    // touch a record that lives there.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/old/a.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "D:/oldarchive/b.wowsreplay")).await.unwrap();

    let moved = query::relocate_source(&pool, src, Path::new("D:/old"), Path::new("F:/new")).await.unwrap();
    assert_eq!(moved, 1, "only the record actually under D:/old is repointed");

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(paths.contains("F:/new/a.wowsreplay"), "the record under old_root is rewritten: {paths:?}");
    assert!(
        paths.contains("D:/oldarchive/b.wowsreplay"),
        "the sibling directory's record must be untouched: {paths:?}"
    );
}

#[tokio::test]
async fn relocate_source_rejects_relocating_into_its_own_subdirectory() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/replays"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/replays/x.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "D:/replays/archive/x.wowsreplay")).await.unwrap();

    let err =
        query::relocate_source(&pool, src, Path::new("D:/replays"), Path::new("D:/replays/archive")).await.unwrap_err();
    match err {
        IndexError::RelocationNested { old_root, new_root } => {
            assert_eq!(old_root, PathBuf::from("D:/replays"));
            assert_eq!(new_root, PathBuf::from("D:/replays/archive"));
        }
        other => panic!("expected RelocationNested, got {other:?}"),
    }

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(paths.contains("D:/replays/x.wowsreplay"), "a rejected relocate must leave records untouched: {paths:?}");
    assert!(
        paths.contains("D:/replays/archive/x.wowsreplay"),
        "a rejected relocate must leave records untouched: {paths:?}"
    );
}

#[tokio::test]
async fn relocate_source_rejects_relocating_out_to_its_own_ancestor() {
    // The reverse direction from the subdirectory test: new_root is an
    // ancestor of old_root.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/replays/archive"), now)
        .await
        .unwrap();

    let err =
        query::relocate_source(&pool, src, Path::new("D:/replays/archive"), Path::new("D:/replays")).await.unwrap_err();
    match err {
        IndexError::RelocationNested { old_root, new_root } => {
            assert_eq!(old_root, PathBuf::from("D:/replays/archive"));
            assert_eq!(new_root, PathBuf::from("D:/replays"));
        }
        other => panic!("expected RelocationNested, got {other:?}"),
    }
}

#[tokio::test]
async fn relocate_source_allows_a_sibling_name_that_is_not_actually_nested() {
    // "D:/replays" and "D:/replaysold" share a literal prefix but neither is
    // an ancestor of the other, so this must be allowed, unlike the two
    // nesting-rejection tests above.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/replays"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/replays/a.wowsreplay")).await.unwrap();

    let moved = query::relocate_source(&pool, src, Path::new("D:/replays"), Path::new("D:/replaysold")).await.unwrap();
    assert_eq!(moved, 1, "a merely-prefix-sharing sibling root must not be rejected as nested");
}

#[tokio::test]
async fn relocate_source_rejects_relocating_to_its_own_current_root() {
    // old_root == new_root is treated as containment (a directory trivially
    // contains itself), so it is rejected the same way as any other nested
    // relocation rather than silently succeeding as a no-op.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/replays"), now).await.unwrap();

    let err = query::relocate_source(&pool, src, Path::new("D:/replays"), Path::new("D:/replays")).await.unwrap_err();
    match err {
        IndexError::RelocationNested { old_root, new_root } => {
            assert_eq!(old_root, PathBuf::from("D:/replays"));
            assert_eq!(new_root, PathBuf::from("D:/replays"));
        }
        other => panic!("expected RelocationNested, got {other:?}"),
    }
}

#[tokio::test]
async fn relocate_source_precheck_does_not_treat_a_sibling_directory_as_colliding() {
    // Without the precheck's own r1 boundary check (a separate copy of the
    // same rule from the UPDATE's WHERE clause), "D:/oldarchive/x.wowsreplay"
    // -- a sibling of D:/old, out of scope -- would be computed as if it were
    // moving to "F:/new" + "archive/x.wowsreplay" = "F:/newarchive/x.wowsreplay",
    // which happens to already exist here as an unrelated record. That would
    // be a false positive RelocationCollision. With the boundary check, the
    // sibling is correctly out of scope, nothing here collides, and the
    // relocation succeeds moving nothing.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/oldarchive/x.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "F:/newarchive/x.wowsreplay")).await.unwrap();

    let moved = query::relocate_source(&pool, src, Path::new("D:/old"), Path::new("F:/new")).await.unwrap();
    assert_eq!(moved, 0, "neither record is actually under D:/old, so nothing moves and nothing collides");

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(paths.contains("D:/oldarchive/x.wowsreplay"), "untouched: {paths:?}");
    assert!(paths.contains("F:/newarchive/x.wowsreplay"), "untouched: {paths:?}");
}

#[tokio::test]
async fn relocate_source_precheck_still_detects_a_collision_that_shares_old_roots_literal_prefix() {
    // new_root ("D:/oldish") happens to share old_root's ("D:/old") literal
    // characters without being nested under it (no path separator right after
    // "D:/old" in "D:/oldish"), so a real mover's rewritten target can land on
    // a path that itself starts with the literal characters of old_root.
    // Without the precheck's r2 boundary check (a separate copy of the same
    // rule from the UPDATE's WHERE clause), the existing record at that path
    // would be wrongly treated as "also under old_root", excluded as a
    // collision partner, and the real collision would slip past the precheck
    // and surface as a raw constraint violation instead of RelocationCollision.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/old/a.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "D:/oldish/a.wowsreplay")).await.unwrap();

    let err = query::relocate_source(&pool, src, Path::new("D:/old"), Path::new("D:/oldish")).await.unwrap_err();
    match err {
        IndexError::RelocationCollision { path } => {
            assert_eq!(path, PathBuf::from("D:/oldish/a.wowsreplay"), "the error names the colliding destination path");
        }
        other => panic!("expected RelocationCollision, got {other:?}"),
    }

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(paths.contains("D:/old/a.wowsreplay"), "a rejected relocate must leave records untouched: {paths:?}");
    assert!(paths.contains("D:/oldish/a.wowsreplay"), "a rejected relocate must leave records untouched: {paths:?}");
}

#[tokio::test]
async fn relocate_source_rejects_a_collision_and_leaves_the_database_unchanged() {
    // "F:/new/a.wowsreplay" already names a different record (arena 200) that
    // is not itself under D:/old, so it will not move. Relocating D:/old to
    // F:/new would rewrite arena 100's record onto that exact path.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_match(&pool, &sample_match(200)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/old/a.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(200, src, "F:/new/a.wowsreplay")).await.unwrap();

    let err = query::relocate_source(&pool, src, Path::new("D:/old"), Path::new("F:/new")).await.unwrap_err();
    match err {
        IndexError::RelocationCollision { path } => {
            assert_eq!(path, PathBuf::from("F:/new/a.wowsreplay"), "the error names the colliding destination path");
        }
        other => panic!("expected RelocationCollision, got {other:?}"),
    }

    let paths = query::record_paths_in_source(&pool, src, None).await.unwrap();
    assert!(
        paths.contains("D:/old/a.wowsreplay"),
        "failed relocate must leave the moving record's path intact: {paths:?}"
    );
    assert!(
        paths.contains("F:/new/a.wowsreplay"),
        "failed relocate must leave the untouched record's path intact: {paths:?}"
    );
    assert_eq!(paths.len(), 2, "no record may be dropped or duplicated by a failed relocate");

    let sources = query::list_sources(&pool).await.unwrap();
    let unchanged = sources.iter().find(|s| s.id == src).expect("source still present");
    assert_eq!(
        unchanged.root_path.as_deref(),
        Some(Path::new("D:/old")),
        "a failed relocate must not update root_path"
    );
}

#[tokio::test]
async fn relocate_source_onto_anothers_root_fails_and_leaves_both_sources_untouched() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let a = query::ensure_source(&pool, "A", SourceKind::ImportedDir, Path::new("D:/a"), now).await.unwrap();
    let b = query::ensure_source(&pool, "B", SourceKind::ImportedDir, Path::new("D:/b"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, a, "D:/a/x.wowsreplay")).await.unwrap();

    // Nothing on b would collide at the record level (b has no records);
    // this exercises the root-ownership check that rejects the request
    // before any record is touched, because b already owns D:/b.
    let err = query::relocate_source(&pool, a, Path::new("D:/a"), Path::new("D:/b")).await.unwrap_err();
    match err {
        IndexError::RootAlreadyOwned { root_path, owner } => {
            assert_eq!(root_path, PathBuf::from("D:/b"));
            assert_eq!(owner, b, "the error names the source that already owns the destination root");
        }
        other => panic!("expected RootAlreadyOwned, got {other:?}"),
    }

    let paths = query::record_paths_in_source(&pool, a, None).await.unwrap();
    assert!(paths.contains("D:/a/x.wowsreplay"), "a rejected relocate must never rewrite a record's path: {paths:?}");

    let sources = query::list_sources(&pool).await.unwrap();
    let src_a = sources.iter().find(|s| s.id == a).expect("source a still present");
    assert_eq!(src_a.root_path.as_deref(), Some(Path::new("D:/a")), "source a's root_path must be untouched");
    let src_b = sources.iter().find(|s| s.id == b).expect("source b still present");
    assert_eq!(src_b.root_path.as_deref(), Some(Path::new("D:/b")), "source b's root_path must be untouched");
}

#[tokio::test]
async fn forget_source_removes_its_records() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/old/a.wowsreplay")).await.unwrap();

    query::forget_source(&pool, src).await.unwrap();

    assert!(query::list_sources(&pool).await.unwrap().iter().all(|s| s.id != src));
    assert!(query::record_paths_in_source(&pool, src, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn forget_source_leaves_other_sources_alone() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let keep = query::ensure_source(&pool, "Keep", SourceKind::ImportedDir, Path::new("D:/keep"), now).await.unwrap();
    let drop = query::ensure_source(&pool, "Drop", SourceKind::ImportedDir, Path::new("D:/drop"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, keep, "D:/keep/a.wowsreplay")).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, drop, "D:/drop/a.wowsreplay")).await.unwrap();

    query::forget_source(&pool, drop).await.unwrap();

    assert!(query::record_paths_in_source(&pool, keep, None).await.unwrap().contains("D:/keep/a.wowsreplay"));
}

#[tokio::test]
async fn forget_source_leaves_shared_match_and_vehicle_rows_intact() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_source(&pool, "Dump", SourceKind::ImportedDir, Path::new("D:/old"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "D:/old/a.wowsreplay")).await.unwrap();
    query::upsert_vehicles(&pool, &[sample_vehicle(100)]).await.unwrap();

    query::forget_source(&pool, src).await.unwrap();

    let match_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM indexed_match WHERE arena_id = 100").fetch_one(&pool).await.unwrap();
    assert_eq!(match_count.0, 1, "indexed_match is shared across sources and must survive forget_source");

    let vehicle_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM indexed_vehicle WHERE arena_id = 100").fetch_one(&pool).await.unwrap();
    assert_eq!(vehicle_count.0, 1, "indexed_vehicle is shared across sources and must survive forget_source");
}

/// A roster row's stored MC, the formula revision it came from, and whether it
/// includes the torpedo warning part.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StoredBi {
    mc: Option<f64>,
    version: Option<MatchContributionVersion>,
    torpedoes_observed: bool,
}

fn stored_mc(mc: Option<f64>, version: Option<u32>) -> StoredBi {
    StoredBi { mc, version: version.map(MatchContributionVersion), torpedoes_observed: false }
}

/// An MC rated from a replay of the player's own team.
fn observed_mc(mc: f64, version: u32) -> StoredBi {
    StoredBi { mc: Some(mc), version: Some(MatchContributionVersion(version)), torpedoes_observed: true }
}

/// The two write paths that merge a pass's roster row into a stored one.
#[derive(Debug, Clone, Copy)]
enum RosterWrite {
    Upsert,
    Replay,
}

struct MergeCase {
    name: &'static str,
    stored: StoredBi,
    incoming: StoredBi,
    expected: StoredBi,
}

/// Stores `stored` as an earlier pass left it in arena 100.
async fn store_earlier_mc(pool: &SqlitePool, stored: StoredBi) {
    query::upsert_match(pool, &sample_match(100)).await.unwrap();
    let earlier = IndexedVehicleRow {
        mc: stored.mc,
        mc_version: stored.version,
        mc_torpedoes_observed: stored.torpedoes_observed,
        ..sample_vehicle(100)
    };
    query::upsert_vehicles_with_mode(pool, &[earlier], IndexWriteMode::Replace, ResultsWrite::Store).await.unwrap();
}

/// The roster row's MC in arena 100 as stored.
async fn read_stored_mc(pool: &SqlitePool) -> StoredBi {
    let row = sqlx::query("SELECT mc, mc_version, mc_torpedoes_observed FROM indexed_vehicle WHERE arena_id = 100")
        .fetch_one(pool)
        .await
        .unwrap();
    let version: Option<i64> = row.get("mc_version");
    StoredBi {
        mc: row.get("mc"),
        version: version.map(|version| MatchContributionVersion(u32::try_from(version).unwrap())),
        torpedoes_observed: row.get("mc_torpedoes_observed"),
    }
}

fn incoming_row(incoming: StoredBi) -> IndexedVehicleRow {
    IndexedVehicleRow {
        mc: incoming.mc,
        mc_version: incoming.version,
        mc_torpedoes_observed: incoming.torpedoes_observed,
        ..sample_vehicle(100)
    }
}

/// Stores `case.stored` as an earlier pass left it, writes `case.incoming` over
/// it incrementally through `write`, and reads the merged MC back.
async fn mc_after_merge(write: RosterWrite, case: &MergeCase, results: ResultsWrite) -> StoredBi {
    let pool = mem_pool().await;
    store_earlier_mc(&pool, case.stored).await;

    let later = incoming_row(case.incoming);
    match write {
        RosterWrite::Upsert => {
            query::upsert_vehicles_with_mode(&pool, &[later], IndexWriteMode::Incremental, results).await.unwrap()
        }
        RosterWrite::Replay => write_corrected(&pool, later, IndexWriteMode::Incremental, results).await,
    }

    read_stored_mc(&pool).await
}

async fn assert_mc_merges(cases: &[MergeCase], results: ResultsWrite) {
    for write in [RosterWrite::Upsert, RosterWrite::Replay] {
        for case in cases {
            assert_eq!(mc_after_merge(write, case, results).await, case.expected, "{} via {write:?}", case.name);
        }
    }
}

#[tokio::test]
async fn a_trusted_incremental_write_replaces_only_a_missing_older_or_partial_mc() {
    let cases = [
        MergeCase {
            name: "same revision",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: stored_mc(Some(900.0), Some(1)),
            expected: stored_mc(Some(1500.0), Some(1)),
        },
        MergeCase {
            name: "older revision",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: stored_mc(Some(900.0), Some(2)),
            expected: stored_mc(Some(900.0), Some(2)),
        },
        MergeCase {
            name: "newer stored revision",
            stored: stored_mc(Some(1500.0), Some(2)),
            incoming: stored_mc(Some(900.0), Some(1)),
            expected: stored_mc(Some(1500.0), Some(2)),
        },
        MergeCase {
            name: "unrated at the same revision",
            stored: stored_mc(None, Some(1)),
            incoming: stored_mc(Some(900.0), Some(1)),
            expected: stored_mc(Some(900.0), Some(1)),
        },
        MergeCase {
            name: "never computed",
            stored: stored_mc(None, None),
            incoming: stored_mc(Some(900.0), Some(1)),
            expected: stored_mc(Some(900.0), Some(1)),
        },
        MergeCase {
            name: "pass without expected values",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: stored_mc(None, None),
            expected: stored_mc(Some(1500.0), Some(1)),
        },
        MergeCase {
            name: "same revision, only the pass observed torpedo warnings",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: observed_mc(900.0, 1),
            expected: observed_mc(900.0, 1),
        },
        MergeCase {
            name: "same revision, both observed torpedo warnings",
            stored: observed_mc(1500.0, 1),
            incoming: observed_mc(900.0, 1),
            expected: observed_mc(1500.0, 1),
        },
        MergeCase {
            name: "same revision, only the stored row observed torpedo warnings",
            stored: observed_mc(1500.0, 1),
            incoming: stored_mc(Some(900.0), Some(1)),
            expected: observed_mc(1500.0, 1),
        },
    ];
    assert_mc_merges(&cases, ResultsWrite::Store).await;
}

#[tokio::test]
async fn an_untrusted_write_fills_only_a_missing_mc_and_takes_its_revision() {
    let cases = [
        MergeCase {
            name: "never computed",
            stored: stored_mc(None, None),
            incoming: stored_mc(Some(900.0), Some(1)),
            expected: stored_mc(Some(900.0), Some(1)),
        },
        MergeCase {
            name: "older revision with results suppressed",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: stored_mc(None, Some(2)),
            expected: stored_mc(Some(1500.0), Some(2)),
        },
        MergeCase {
            name: "older revision with a value",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: stored_mc(Some(900.0), Some(2)),
            expected: stored_mc(Some(1500.0), Some(2)),
        },
        MergeCase {
            name: "pass without expected values",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: stored_mc(None, None),
            expected: stored_mc(Some(1500.0), Some(1)),
        },
        MergeCase {
            name: "never computed, pass observed torpedo warnings",
            stored: stored_mc(None, None),
            incoming: observed_mc(900.0, 1),
            expected: observed_mc(900.0, 1),
        },
        MergeCase {
            name: "stored value keeps its unobserved torpedo warnings",
            stored: stored_mc(Some(1500.0), Some(1)),
            incoming: observed_mc(900.0, 2),
            expected: stored_mc(Some(1500.0), Some(2)),
        },
        MergeCase {
            name: "stored value keeps its observed torpedo warnings",
            stored: observed_mc(1500.0, 1),
            incoming: stored_mc(None, Some(2)),
            expected: observed_mc(1500.0, 2),
        },
    ];
    assert_mc_merges(&cases, ResultsWrite::Keep).await;
}

/// A battle type MC does not apply to keeps no MC from an earlier pass, even
/// where the write could not vouch for its results or rated nothing at all.
#[tokio::test]
async fn a_replay_write_of_an_unrated_battle_clears_its_stored_mc() {
    let passes = [("rated", stored_mc(None, Some(2))), ("without expected values", stored_mc(None, None))];
    for results in [ResultsWrite::Store, ResultsWrite::Keep] {
        for mode in [IndexWriteMode::Incremental, IndexWriteMode::Replace] {
            for (name, incoming) in passes {
                let pool = mem_pool().await;
                store_earlier_mc(&pool, observed_mc(1500.0, 1)).await;

                write_replay(&pool, incoming_row(incoming), mode, results, RatingScope::Unrated).await;

                let stored = read_stored_mc(&pool).await;
                let context = format!("{name}, {mode:?}, {results:?}");
                assert_eq!(stored.mc, None, "{context}");
                assert!(!stored.torpedoes_observed, "{context}");
            }
        }
    }
}

async fn store_rated_replay(
    pool: &SqlitePool,
    source: SourceId,
    arena: i64,
    path: &str,
    version: Option<MatchContributionVersion>,
) {
    query::upsert_match(pool, &sample_match(arena)).await.unwrap();
    query::upsert_record(pool, &sample_record(arena, source, path)).await.unwrap();
    let vehicle = IndexedVehicleRow { mc: version.map(|_| 1500.0), mc_version: version, ..sample_vehicle(arena) };
    query::upsert_vehicles(pool, &[vehicle]).await.unwrap();
}

#[tokio::test]
async fn mc_from_another_revision_makes_a_record_stale_only_for_a_pass_that_can_rate() {
    const RATED: MatchContributionVersion = MatchContributionVersion(1);
    const NEWER: MatchContributionVersion = MatchContributionVersion(2);
    const RATED_PATH: &str = "C:/wows/replays/rated.wowsreplay";
    const UNRATED_PATH: &str = "C:/wows/replays/unrated.wowsreplay";
    const ELSEWHERE_PATH: &str = "D:/old/replays/elsewhere.wowsreplay";

    let pool = mem_pool().await;
    let root = Path::new("C:/wows/replays");
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let source = query::ensure_default_source(&pool, root, now).await.unwrap();
    store_rated_replay(&pool, source, 100, RATED_PATH, Some(RATED)).await;
    store_rated_replay(&pool, source, 200, UNRATED_PATH, None).await;
    store_rated_replay(&pool, source, 300, ELSEWHERE_PATH, Some(RATED)).await;
    let bot = IndexedVehicleRow { account_id: AccountId::from(0), mc: None, mc_version: None, ..sample_vehicle(100) };
    query::upsert_vehicles(&pool, &[bot]).await.unwrap();

    let without_table = query::record_paths_in_source(&pool, source, None).await.unwrap();
    assert!(without_table.contains(UNRATED_PATH), "a pass that cannot rate does not reparse for MC");
    assert!(!query::has_stale_replay_records(&pool, root, None).await.unwrap());

    let at_rated = query::record_paths_in_source(&pool, source, Some(RATED)).await.unwrap();
    assert!(at_rated.contains(RATED_PATH), "MC from the pass's revision is current, and the bot row is skipped");
    assert!(!at_rated.contains(UNRATED_PATH), "a roster row never rated is reparsed");

    let at_newer = query::record_paths_in_source(&pool, source, Some(NEWER)).await.unwrap();
    assert!(!at_newer.contains(RATED_PATH), "MC from an older revision is reparsed");
    assert!(query::has_stale_replay_records(&pool, root, Some(NEWER)).await.unwrap());

    query::mark_replay_records_current(&pool, source, root, Some(NEWER)).await.unwrap();
    assert!(!query::has_stale_replay_records(&pool, root, Some(NEWER)).await.unwrap());
    let marked = query::record_paths_in_source(&pool, source, Some(NEWER)).await.unwrap();
    assert!(marked.contains(RATED_PATH));
    assert!(marked.contains(UNRATED_PATH));
    assert!(!marked.contains(ELSEWHERE_PATH), "a directory that was not scanned stays stale");

    let kept: Option<f64> =
        sqlx::query_scalar("SELECT mc FROM indexed_vehicle WHERE arena_id = 100 AND account_id = 7")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kept, Some(1500.0), "marking stamps the revision and keeps the stored MC");
}
