use wows_toolkit_config::queries;

async fn count(pool: &sqlx::SqlitePool, table: &str) -> i64 {
    let (rows,): (i64,) =
        sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}")).fetch_one(pool).await.expect("the table is read");
    rows
}

#[tokio::test]
async fn clearing_settings_keeps_the_migration_flag_the_kept_keys_and_the_tracked_data() {
    let pool = wows_toolkit_config::test_pool().await;
    queries::set_setting(&pool, "zoom_factor", &1.5f32).await.expect("a setting saves");
    queries::set_setting(&pool, "legacy_blob", &"kept".to_owned()).await.expect("a setting saves");
    wows_toolkit_config::set_migrated(&pool).await.expect("the flag saves");
    sqlx::query("INSERT INTO session_stats (sort_key) VALUES ('2026-01-01 00:00:00')")
        .execute(&pool)
        .await
        .expect("a game saves");
    queries::upsert_chart_config(&pool, 1, "{}").await.expect("a chart saves");
    sqlx::query("INSERT INTO armor_viewer_defaults (id) VALUES (1)").execute(&pool).await.expect("the row saves");
    queries::save_render_options(&pool, "{}").await.expect("the options save");
    queries::save_dock_layout(&pool, "stats", "{}").await.expect("the layout saves");
    queries::save_mod_manager(&pool, "{}").await.expect("the record saves");
    queries::insert_sent_replay(&pool, "a.wowsreplay").await.expect("the replay is recorded");

    queries::clear_settings(&pool, &["legacy_blob"]).await.expect("the clear commits");

    assert_eq!(queries::get_setting::<f32>(&pool, "zoom_factor").await, None);
    assert_eq!(queries::get_setting::<String>(&pool, "legacy_blob").await.as_deref(), Some("kept"));
    assert!(wows_toolkit_config::is_migrated(&pool).await, "the next start does not migrate again");
    for table in
        ["session_stats", "chart_configs", "armor_viewer_defaults", "render_options", "dock_layouts", "mod_manager"]
    {
        assert_eq!(count(&pool, table).await, 0, "{table} is cleared");
    }
    assert_eq!(count(&pool, "sent_replays").await, 1, "the sent replays are kept");
}
