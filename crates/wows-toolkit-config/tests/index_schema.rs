use sqlx::sqlite::SqlitePoolOptions;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::VehicleRelation;

/// Applying all migrations against a fresh in-memory DB must create the index tables.
#[tokio::test]
async fn index_migration_creates_tables() {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    for table in ["index_source", "indexed_match", "replay_record", "indexed_vehicle"] {
        let found: Option<(String,)> = sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' AND name=?1")
            .bind(table)
            .fetch_optional(&pool)
            .await
            .unwrap();
        assert_eq!(found.map(|(n,)| n).as_deref(), Some(table), "missing table {table}");
    }
}

/// The query bar's ship value lookup reads a few hundred ships back out of
/// `indexed_vehicle`, which holds one row per player per match. The plan has
/// to answer it from the covering index: reading the table instead is tens of
/// seconds once a library has been imported.
#[tokio::test]
async fn the_ship_lookup_reads_its_covering_index_rather_than_the_table() {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN \
         SELECT v.ship_id, MAX(v.ship_name) AS ship_name, COUNT(DISTINCT v.arena_id) AS match_count \
           FROM indexed_vehicle v \
          WHERE LOWER(v.ship_name) LIKE '%' || LOWER(?1) || '%' \
          GROUP BY v.ship_id ORDER BY match_count DESC LIMIT ?2",
    )
    .bind("yam")
    .bind(50i64)
    .fetch_all(&pool)
    .await
    .unwrap();

    let detail = plan.iter().map(|(_, _, _, detail)| detail.as_str()).collect::<Vec<_>>().join("; ");
    assert!(detail.contains("idx_vehicle_ship_name"), "the ship lookup reads something else now: {detail}");
}

#[test]
fn outcome_and_relation_roundtrip_db_strings() {
    for o in [MatchOutcome::Win, MatchOutcome::Loss, MatchOutcome::Draw, MatchOutcome::Unknown] {
        assert_eq!(MatchOutcome::from_db_str(o.as_db_str()), Some(o));
    }
    for r in [VehicleRelation::SelfPlayer, VehicleRelation::Ally, VehicleRelation::Enemy] {
        assert_eq!(VehicleRelation::from_db_str(r.as_db_str()), Some(r));
    }
    assert!(MatchOutcome::from_db_str("nonsense").is_none());
    // Default filter constrains nothing.
    let f = MatchFilter::default();
    assert!(f.outcome.is_none() && f.source_ids.is_none() && f.player_present.is_none());
}
