//! Behaviour tests for the personal-rating repair against a real SQLite
//! database. A stored rating is a point-in-time value, so the property under
//! test throughout is that nothing here can replace one.

use std::path::Path;
use std::path::PathBuf;

use jiff::Timestamp;
use sqlx::Row;
use sqlx::sqlite::SqlitePoolOptions;
use wows_core::game_types::AccountId;
use wows_core::game_types::ArenaId;
use wows_core::game_types::GameParamId;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::PrRepair;
use wows_toolkit_config::index::rows::PrTarget;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::VehicleRelation;

async fn mem_pool() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn a_match(arena: i64) -> ObjectiveMatch {
    ObjectiveMatch {
        arena_id: ArenaId::new(arena),
        timestamp: Timestamp::from_second(1_700_000_000 + arena).unwrap(),
        map: "spaces/13_OC_new_dawn".into(),
        game_mode: "Domination".into(),
        game_type: "pvp".into(),
        match_group: "pvp".into(),
        version_build: Some(1234),
    }
}

fn a_record(arena: i64, source: SourceId, self_pr: Option<f64>) -> ReplayRecord {
    ReplayRecord {
        arena_id: ArenaId::new(arena),
        source_id: source,
        replay_path: PathBuf::from(format!("{arena}.wowsreplay")),
        file_mtime: Some(42),
        outcome: MatchOutcome::Win,
        self_account_id: Some(AccountId(7)),
        self_ship_id: Some(GameParamId::from(999u64)),
        self_survived: Some(true),
        self_damage: Some(123_456),
        self_kills: Some(2),
        self_pr,
        results_available: true,
        indexed_at: Timestamp::from_second(1_700_000_100).unwrap(),
    }
}

fn a_vehicle(arena: i64, account: i64, pr: Option<f64>) -> IndexedVehicleRow {
    IndexedVehicleRow {
        arena_id: ArenaId::new(arena),
        account_id: AccountId(account),
        player_name: format!("Player{account}"),
        clan: "CLAN".into(),
        realm: Some("na".into()),
        ship_id: GameParamId::from(900u64 + account as u64),
        ship_index: "PJSD018".into(),
        ship_name: "Harugumo".into(),
        nation: "japan".into(),
        species: "Destroyer".into(),
        tier: 10,
        relation: VehicleRelation::SelfPlayer,
        division_id: None,
        survived: Some(true),
        damage: Some(50_000),
        kills: Some(1),
        spotting: Some(10_000),
        potential: Some(500_000),
        received: Some(5_000),
        pr,
        is_test_ship: false,
        disconnected: Some(false),
        is_stream_sniper: None,
        sniper_twitch_login: None,
    }
}

async fn source(pool: &sqlx::SqlitePool) -> SourceId {
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    query::ensure_default_source(pool, Path::new("C:/wows/replays"), now).await.unwrap()
}

async fn stored_record_pr(pool: &sqlx::SqlitePool, arena: i64) -> Option<f64> {
    sqlx::query("SELECT self_pr FROM replay_record WHERE arena_id = ?1")
        .bind(arena)
        .fetch_one(pool)
        .await
        .unwrap()
        .try_get("self_pr")
        .unwrap()
}

async fn stored_vehicle_pr(pool: &sqlx::SqlitePool, arena: i64, account: i64) -> Option<f64> {
    sqlx::query("SELECT pr FROM indexed_vehicle WHERE arena_id = ?1 AND account_id = ?2")
        .bind(arena)
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
        .try_get("pr")
        .unwrap()
}

#[tokio::test]
async fn only_rows_without_a_rating_are_reported_as_gaps() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    query::upsert_match(&pool, &a_match(2)).await.unwrap();
    // Arena 1 has no rating anywhere; arena 2 has one on both rows.
    query::upsert_record(&pool, &a_record(1, src, None)).await.unwrap();
    query::upsert_record(&pool, &a_record(2, src, Some(1500.0))).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, None), a_vehicle(2, 8, Some(1200.0))]).await.unwrap();

    let gaps = query::pr_gaps(&pool).await.unwrap();
    let targets: Vec<PrTarget> = gaps.iter().map(|g| g.target).collect();
    assert_eq!(targets.len(), 2, "only the two rows without a rating are gaps: {gaps:?}");
    assert!(
        targets.iter().any(|t| matches!(t, PrTarget::Vehicle { arena_id, .. } if *arena_id == ArenaId::new(1))),
        "the unrated roster row is a gap: {targets:?}"
    );
    assert!(targets.iter().any(|t| matches!(t, PrTarget::Record(_))), "the unrated record is a gap: {targets:?}");

    // The gap's inputs must be the row's own, not another row's.
    let vehicle_gap =
        gaps.iter().find(|g| matches!(g.target, PrTarget::Vehicle { .. })).expect("a roster gap was reported");
    assert_eq!(vehicle_gap.inputs.ship_id, GameParamId::from(907u64));
    assert_eq!(vehicle_gap.inputs.damage, 50_000);
    assert_eq!(vehicle_gap.inputs.kills, 1);
    assert!(vehicle_gap.inputs.is_win, "the arena's chosen record was a win");
}

#[tokio::test]
async fn a_lost_battle_is_not_reported_as_a_win() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    let mut record = a_record(1, src, None);
    record.outcome = MatchOutcome::Loss;
    query::upsert_record(&pool, &record).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, None)]).await.unwrap();

    let gaps = query::pr_gaps(&pool).await.unwrap();
    assert!(!gaps.is_empty());
    assert!(gaps.iter().all(|g| !g.inputs.is_win), "a loss must not be rated as a win: {gaps:?}");
}

#[tokio::test]
async fn a_row_with_no_damage_is_not_a_gap() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    let mut record = a_record(1, src, None);
    record.self_damage = None;
    query::upsert_record(&pool, &record).await.unwrap();
    let mut vehicle = a_vehicle(1, 7, None);
    vehicle.damage = None;
    query::upsert_vehicles(&pool, &[vehicle]).await.unwrap();

    let gaps = query::pr_gaps(&pool).await.unwrap();
    assert!(gaps.is_empty(), "a rating from an absent damage figure would be a fiction: {gaps:?}");
}

#[tokio::test]
async fn a_stored_rating_is_never_replaced() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    query::upsert_record(&pool, &a_record(1, src, Some(1500.0))).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, Some(1200.0))]).await.unwrap();

    let record_id: i64 =
        sqlx::query("SELECT record_id FROM replay_record").fetch_one(&pool).await.unwrap().try_get(0).unwrap();

    // Aim the repair straight at the rated rows, as a stale read of an
    // already-filled row would.
    let repairs = [
        PrRepair { target: PrTarget::Record(wows_toolkit_config::index::rows::RecordId(record_id)), pr: 42.0 },
        PrRepair {
            target: PrTarget::Vehicle {
                arena_id: ArenaId::new(1),
                account_id: AccountId(7),
                ship_id: GameParamId::from(907u64),
            },
            pr: 42.0,
        },
    ];
    let changed = query::apply_pr_repairs(&pool, &repairs).await.unwrap();

    assert_eq!(changed, 0, "a row that already has a rating must not be counted as repaired");
    assert_eq!(stored_record_pr(&pool, 1).await, Some(1500.0));
    assert_eq!(stored_vehicle_pr(&pool, 1, 7).await, Some(1200.0));
}

#[tokio::test]
async fn a_missing_rating_is_filled() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    query::upsert_record(&pool, &a_record(1, src, None)).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, None)]).await.unwrap();

    let gaps = query::pr_gaps(&pool).await.unwrap();
    let repairs: Vec<PrRepair> = gaps.iter().map(|g| PrRepair { target: g.target, pr: 1337.0 }).collect();
    let changed = query::apply_pr_repairs(&pool, &repairs).await.unwrap();

    assert_eq!(changed, 2);
    assert_eq!(stored_record_pr(&pool, 1).await, Some(1337.0));
    assert_eq!(stored_vehicle_pr(&pool, 1, 7).await, Some(1337.0));
    assert!(query::pr_gaps(&pool).await.unwrap().is_empty(), "a filled row is no longer a gap");
}

#[tokio::test]
async fn a_reindex_that_carries_no_rating_keeps_the_stored_one() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    query::upsert_record(&pool, &a_record(1, src, Some(1500.0))).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, Some(1200.0))]).await.unwrap();

    // Re-index with the expected values unavailable, as a launch that reads the
    // replay before the expected values load does.
    query::upsert_record(&pool, &a_record(1, src, None)).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, None)]).await.unwrap();

    assert_eq!(stored_record_pr(&pool, 1).await, Some(1500.0), "the record's stored rating survived a re-index");
    assert_eq!(stored_vehicle_pr(&pool, 1, 7).await, Some(1200.0), "the roster row's stored rating survived");
}

/// A rebuilt report always recomputes its rating against whatever expected
/// values are loaded today, so every re-index carries a number. If that number
/// won, one "Index all replays" would restamp the whole database and the same
/// battle would report a different rating month to month.
#[tokio::test]
async fn a_reindex_that_carries_a_rating_does_not_restamp_the_stored_one() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    query::upsert_record(&pool, &a_record(1, src, Some(1500.0))).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, Some(1200.0))]).await.unwrap();

    // Today's expected values would give this battle a different number.
    query::upsert_record(&pool, &a_record(1, src, Some(1600.0))).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, Some(1300.0))]).await.unwrap();

    assert_eq!(stored_record_pr(&pool, 1).await, Some(1500.0), "the record kept the rating it was first given");
    assert_eq!(stored_vehicle_pr(&pool, 1, 7).await, Some(1200.0), "the roster row kept its original rating");
}

/// Old-wins must not mean nothing is ever written: a row that has no rating
/// still takes the one a re-index brings, which is the arm `fill_missing_pr`
/// populates.
#[tokio::test]
async fn a_reindex_fills_a_row_that_has_no_rating() {
    let pool = mem_pool().await;
    let src = source(&pool).await;
    query::upsert_match(&pool, &a_match(1)).await.unwrap();
    query::upsert_record(&pool, &a_record(1, src, None)).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, None)]).await.unwrap();

    query::upsert_record(&pool, &a_record(1, src, Some(1600.0))).await.unwrap();
    query::upsert_vehicles(&pool, &[a_vehicle(1, 7, Some(1300.0))]).await.unwrap();

    assert_eq!(stored_record_pr(&pool, 1).await, Some(1600.0));
    assert_eq!(stored_vehicle_pr(&pool, 1, 7).await, Some(1300.0));
}
