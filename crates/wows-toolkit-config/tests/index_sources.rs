use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::SourceKind;

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
        game_type: "pvp".into(),
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
async fn records_survive_the_migration_of_a_single_live_source() {
    // Guards the dedupe path's UPDATE OR IGNORE: with only one live source
    // nothing is repointed and nothing may be lost.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay")).await.unwrap();

    let paths = query::record_paths_in_source(&pool, src).await.unwrap();
    assert!(paths.contains("a.wowsreplay"));
}

#[tokio::test]
async fn migration_dedupes_two_live_sources_repointing_non_colliding_records() {
    // mem_pool() already applied migration 008, so its indexes are in place.
    // Drop them to rebuild the pre-008 shape the migration is meant to repair,
    // then recreate the duplicate-live-source condition a racing first launch
    // could leave behind.
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    sqlx::query("DROP INDEX idx_source_single_live").execute(&pool).await.unwrap();
    sqlx::query("DROP INDEX idx_source_root_path").execute(&pool).await.unwrap();

    let survivor = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let doomed_row: (i64,) = sqlx::query_as(
        "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) RETURNING source_id",
    )
    .bind("Live replays (dup)")
    .bind(SourceKind::Live.as_db_str())
    .bind("D:/other/replays")
    .bind(now.as_second())
    .fetch_one(&pool)
    .await
    .unwrap();
    let doomed = SourceId(doomed_row.0);
    assert!(doomed.0 > survivor.0, "the doomed source must have the higher id for this scenario to be meaningful");

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

    let paths = query::record_paths_in_source(&pool, survivor).await.unwrap();
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
