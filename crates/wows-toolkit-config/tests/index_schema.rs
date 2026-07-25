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
