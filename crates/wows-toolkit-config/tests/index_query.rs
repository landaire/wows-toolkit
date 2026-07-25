use std::path::Path;

use jiff::Timestamp;
use sqlx::sqlite::SqlitePoolOptions;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::SourceKind;

async fn mem_pool() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn ensure_default_source_is_idempotent() {
    let pool = mem_pool().await;
    let now = Timestamp::from_second(1_700_000_000).unwrap();
    let a = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    let b = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();
    assert_eq!(a, b, "second call must return the same source id");

    let sources = query::list_sources(&pool).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].kind, SourceKind::Live);
    assert_eq!(sources[0].id, a);
}
