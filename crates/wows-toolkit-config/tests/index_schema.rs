use sqlx::sqlite::SqlitePoolOptions;

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
