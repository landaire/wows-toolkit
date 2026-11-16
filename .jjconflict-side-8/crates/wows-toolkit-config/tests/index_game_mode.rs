//! Behaviour tests for the numeric `game_mode_id` column against a real
//! SQLite database. A row's id is a point-in-time fact recorded at index
//! time, so the property under test throughout is that nothing can replace
//! one once it is set -- the same guarantee `pr` and `self_pr` already give.

use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameMode;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::ObjectiveMatch;

async fn mem_pool() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn a_match(arena: i64, game_mode_id: Option<i32>) -> ObjectiveMatch {
    ObjectiveMatch {
        arena_id: ArenaId::new(arena),
        timestamp: Timestamp::from_second(1_700_000_000 + arena).unwrap(),
        map: "spaces/13_OC_new_dawn".into(),
        game_mode: "ArmsRace".into(),
        game_mode_id,
        game_type: "pvp".into(),
        match_group: "pvp".into(),
        version_build: Some(1234),
    }
}

async fn stored_game_mode_id(pool: &sqlx::SqlitePool, arena: i64) -> Option<i32> {
    sqlx::query("SELECT game_mode_id FROM indexed_match WHERE arena_id = ?1")
        .bind(arena)
        .fetch_one(pool)
        .await
        .unwrap()
        .try_get("game_mode_id")
        .unwrap()
}

#[tokio::test]
async fn indexing_a_match_stores_its_numeric_game_mode() {
    let pool = mem_pool().await;
    query::upsert_match(&pool, &a_match(1, Some(GameMode::ArmsRace.id()))).await.unwrap();
    assert_eq!(stored_game_mode_id(&pool, 1).await, Some(GameMode::ArmsRace.id()));
}

/// A row indexed with no numeric mode -- an old row, or a build the id table
/// does not cover -- must read back as `None`, not `0` or any other stand-in.
#[tokio::test]
async fn a_row_indexed_with_no_game_mode_id_reads_back_as_none() {
    let pool = mem_pool().await;
    query::upsert_match(&pool, &a_match(1, None)).await.unwrap();
    assert_eq!(stored_game_mode_id(&pool, 1).await, None, "an absent mode must stay NULL, not default to a mode");
}

/// The COALESCE guarantee: a re-index that carries a different id must not
/// overwrite the one already stored, exactly as `pr` and `self_pr` behave in
/// the same upsert statement. Verified to actually discriminate by dropping
/// `COALESCE(game_mode_id, ?5)` down to a plain `?5` in `upsert_match` and
/// re-running: the assertion then fails with
/// `assertion `left == right` failed: old wins: a re-index must not restamp
/// a recorded mode / left: Some(7) / right: Some(15)`, i.e. the second
/// upsert's Domination id clobbered the first ArmsRace id. Restored before
/// committing.
#[tokio::test]
async fn a_reindex_with_a_different_game_mode_id_keeps_the_first() {
    let pool = mem_pool().await;
    query::upsert_match(&pool, &a_match(1, Some(GameMode::ArmsRace.id()))).await.unwrap();
    query::upsert_match(&pool, &a_match(1, Some(GameMode::Domination.id()))).await.unwrap();
    assert_eq!(
        stored_game_mode_id(&pool, 1).await,
        Some(GameMode::ArmsRace.id()),
        "old wins: a re-index must not restamp a recorded mode"
    );
}

/// Old-wins must not mean nothing is ever written: a row with no id yet still
/// takes the one a later re-index brings.
#[tokio::test]
async fn a_reindex_fills_a_row_that_has_no_game_mode_id() {
    let pool = mem_pool().await;
    query::upsert_match(&pool, &a_match(1, None)).await.unwrap();
    query::upsert_match(&pool, &a_match(1, Some(GameMode::ArmsRace.id()))).await.unwrap();
    assert_eq!(stored_game_mode_id(&pool, 1).await, Some(GameMode::ArmsRace.id()));
}

/// The count the search UI surfaces as a re-index hint: only rows the
/// migration left NULL, not the whole table.
#[tokio::test]
async fn missing_game_mode_count_counts_only_null_rows() {
    let pool = mem_pool().await;
    query::upsert_match(&pool, &a_match(1, None)).await.unwrap();
    query::upsert_match(&pool, &a_match(2, Some(GameMode::ArmsRace.id()))).await.unwrap();
    query::upsert_match(&pool, &a_match(3, None)).await.unwrap();
    assert_eq!(query::matches_missing_game_mode_count(&pool).await.unwrap(), 2);
}

/// Once every row has been re-indexed the count must read zero, not merely
/// fall to some smaller nonzero number: a real zero is what tells the search
/// UI the hint can stop showing.
#[tokio::test]
async fn missing_game_mode_count_is_zero_once_every_row_has_one() {
    let pool = mem_pool().await;
    query::upsert_match(&pool, &a_match(1, Some(GameMode::ArmsRace.id()))).await.unwrap();
    query::upsert_match(&pool, &a_match(2, Some(GameMode::Domination.id()))).await.unwrap();
    assert_eq!(query::matches_missing_game_mode_count(&pool).await.unwrap(), 0);
}
