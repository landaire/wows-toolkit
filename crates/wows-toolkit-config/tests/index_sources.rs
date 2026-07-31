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
async fn live_source_id_is_deterministic_when_rows_exist() {
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
