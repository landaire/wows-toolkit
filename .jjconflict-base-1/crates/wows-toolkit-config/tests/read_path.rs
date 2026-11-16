use sqlx::sqlite::SqlitePoolOptions;
use wows_toolkit_config::queries;

#[tokio::test]
async fn reads_zoom_and_wows_dir_from_settings_table() {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)").execute(&pool).await.unwrap();
    queries::set_setting(&pool, "zoom_factor", &1.25f32).await.unwrap();
    queries::set_setting(&pool, "wows_dir", &"C:/Games/WoWs".to_string()).await.unwrap();

    let zoom: Option<f32> = queries::get_setting(&pool, "zoom_factor").await;
    let dir: Option<String> = queries::get_setting(&pool, "wows_dir").await;
    assert_eq!(zoom, Some(1.25));
    assert_eq!(dir.as_deref(), Some("C:/Games/WoWs"));
}
