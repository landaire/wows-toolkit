use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::SourceKind;
use wows_toolkit_config::index::rows::VehicleRelation;

async fn mem_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

/// `ensure_default_source` only ever yields the single `Live` row, so a second
/// source is inserted directly. Phase 2 replaces this with `ensure_source`.
async fn make_source(pool: &SqlitePool, name: &str, kind: SourceKind, root: &str) -> SourceId {
    let id: (i64,) = sqlx::query_as(
        "INSERT INTO index_source (name, kind, root_path, added_at) VALUES (?1, ?2, ?3, ?4) RETURNING source_id",
    )
    .bind(name)
    .bind(kind.as_db_str())
    .bind(root)
    .bind(1_700_000_000i64)
    .fetch_one(pool)
    .await
    .unwrap();
    SourceId(id.0)
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

fn sample_record(arena: i64, source: SourceId, path: &str, account: Option<i64>) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(path),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: account.map(AccountId::from),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(123_456),
        self_kills: Some(2),
        self_pr: Some(1500.0),
        results_available: true,
        indexed_at: Timestamp::from_second(1_700_000_100).unwrap(),
    }
}

fn sample_vehicle(arena: i64, account: i64, division: Option<i64>, relation: VehicleRelation) -> IndexedVehicleRow {
    IndexedVehicleRow {
        arena_id: ArenaId::new(arena),
        account_id: AccountId::from(account),
        player_name: format!("player{account}"),
        clan: String::new(),
        realm: None,
        ship_id: GameParamId::from(999u64),
        ship_index: "PJSB018".into(),
        ship_name: "Yamato".into(),
        nation: "Japan".into(),
        species: "Battleship".into(),
        tier: 10,
        relation,
        division_id: division,
        survived: Some(true),
        damage: Some(123_456),
        kills: Some(2),
        spotting: Some(1000),
        potential: Some(2000),
        received: Some(3000),
        pr: Some(1500.0),
        is_test_ship: false,
        disconnected: Some(false),
        is_stream_sniper: None,
        sniper_twitch_login: None,
    }
}

#[tokio::test]
async fn row_summaries_resolve_division_through_the_account_join() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay", Some(7))).await.unwrap();
    query::upsert_vehicles(
        &pool,
        &[
            sample_vehicle(100, 7, Some(3), VehicleRelation::SelfPlayer),
            sample_vehicle(100, 8, Some(9), VehicleRelation::Enemy),
        ],
    )
    .await
    .unwrap();

    let summaries = query::row_summaries_for_source(&pool, src).await.unwrap();
    let row = summaries.get(&PathBuf::from("a.wowsreplay")).expect("summary for the recorded path");
    assert_eq!(row.division_id, Some(3), "division comes from the self account's roster row, not the enemy's");
    assert_eq!(row.outcome, MatchOutcome::Win);
    assert_eq!(row.self_damage, Some(123_456));
    assert_eq!(row.self_kills, Some(2));
    assert_eq!(row.self_survived, Some(true));
    assert_eq!(row.file_mtime, Some(42));
    assert!(row.results_available);
}

#[tokio::test]
async fn row_summaries_are_scoped_to_one_source() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let live = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let other = make_source(&pool, "Imported", SourceKind::ImportedDir, "D:/dump").await;

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, live, "live.wowsreplay", Some(7))).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, other, "other.wowsreplay", Some(7))).await.unwrap();

    let live_rows = query::row_summaries_for_source(&pool, live).await.unwrap();
    let other_rows = query::row_summaries_for_source(&pool, other).await.unwrap();

    assert_eq!(live_rows.len(), 1);
    assert_eq!(other_rows.len(), 1);
    assert!(live_rows.contains_key(&PathBuf::from("live.wowsreplay")));
    assert!(other_rows.contains_key(&PathBuf::from("other.wowsreplay")));
}

#[tokio::test]
async fn row_summaries_have_no_division_without_a_self_account() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    // A spectator recording: no self account, so no roster row can be its own.
    query::upsert_record(&pool, &sample_record(100, src, "spec.wowsreplay", None)).await.unwrap();
    query::upsert_vehicles(&pool, &[sample_vehicle(100, 7, Some(3), VehicleRelation::Ally)]).await.unwrap();

    let summaries = query::row_summaries_for_source(&pool, src).await.unwrap();
    let row = summaries.get(&PathBuf::from("spec.wowsreplay")).expect("summary for the recorded path");
    assert_eq!(row.division_id, None, "an absent self account must not borrow another player's division");
}

#[tokio::test]
async fn row_summaries_have_no_division_without_a_roster_row() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "a.wowsreplay", Some(7))).await.unwrap();
    // No upsert_vehicles at all.

    let summaries = query::row_summaries_for_source(&pool, src).await.unwrap();
    let row = summaries.get(&PathBuf::from("a.wowsreplay")).expect("summary for the recorded path");
    assert_eq!(row.division_id, None, "a missing roster must yield None, not a default of 0");
}

#[tokio::test]
async fn row_summaries_are_empty_for_an_unused_source() {
    let pool = mem_pool().await;
    let src = make_source(&pool, "Empty", SourceKind::AdHoc, "D:/empty").await;
    let summaries = query::row_summaries_for_source(&pool, src).await.unwrap();
    assert!(summaries.is_empty());
}

#[tokio::test]
async fn division_resolves_for_a_perspective_whose_roster_relation_was_overwritten() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

    query::upsert_match(&pool, &sample_match(100)).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "p7.wowsreplay", Some(7))).await.unwrap();
    query::upsert_record(&pool, &sample_record(100, src, "p8.wowsreplay", Some(8))).await.unwrap();

    // First index player 7's perspective: account 7 is `SelfPlayer` with division 3.
    query::upsert_vehicles(
        &pool,
        &[
            sample_vehicle(100, 7, Some(3), VehicleRelation::SelfPlayer),
            sample_vehicle(100, 8, None, VehicleRelation::Enemy),
        ],
    )
    .await
    .unwrap();

    // Then index player 8's perspective. `indexed_vehicle` upserts on
    // (arena_id, account_id, ship_id), so this overwrites account 7's
    // `relation` from `SelfPlayer` to `Enemy` while leaving `division_id = 3`
    // untouched. This is not redundant setup: without it, a `relation =
    // 'self'` join would resolve division_id correctly by accident, and the
    // regression this test guards against would go undetected.
    query::upsert_vehicles(
        &pool,
        &[
            sample_vehicle(100, 8, Some(9), VehicleRelation::SelfPlayer),
            sample_vehicle(100, 7, Some(3), VehicleRelation::Enemy),
        ],
    )
    .await
    .unwrap();

    let summaries = query::row_summaries_for_source(&pool, src).await.unwrap();
    let row7 = summaries.get(&PathBuf::from("p7.wowsreplay")).expect("summary for p7");
    let row8 = summaries.get(&PathBuf::from("p8.wowsreplay")).expect("summary for p8");
    assert_eq!(row7.division_id, Some(3), "must resolve through self_account_id, not the now-stale relation column");
    assert_eq!(row8.division_id, Some(9), "player 8's own perspective still resolves to their own division");
}
