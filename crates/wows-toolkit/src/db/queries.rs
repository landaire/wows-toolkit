//! Typed read/write functions for each database table.

use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// settings (key-value)
// ---------------------------------------------------------------------------

/// Get a JSON-encoded setting by key.
pub async fn get_setting<T: DeserializeOwned>(pool: &SqlitePool, key: &str) -> Option<T> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = ?1").bind(key).fetch_optional(pool).await.ok()?;
    row.and_then(|(json,)| serde_json::from_str(&json).ok())
}

/// Set a JSON-encoded setting by key (upsert).
pub async fn set_setting<T: Serialize>(pool: &SqlitePool, key: &str, value: &T) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(value).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    set_setting_raw(pool, key, &json).await
}

/// Write a pre-serialized JSON string to the settings table.
pub async fn set_setting_raw(pool: &SqlitePool, key: &str, json: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2")
        .bind(key)
        .bind(json)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// session_stats
// ---------------------------------------------------------------------------

/// A row from the session_stats table.
#[derive(Debug, sqlx::FromRow)]
pub struct SessionStatRow {
    #[allow(dead_code)]
    pub id: i64,
    pub sort_key: String,
    pub ship_name: String,
    pub ship_id: i64,
    pub player_id: i64,
    pub game_time: String,
    pub match_group: String,
    pub damage: i64,
    pub spotting_damage: i64,
    pub frags: i64,
    pub raw_xp: i64,
    pub base_xp: i64,
    pub is_win: bool,
    pub is_loss: bool,
    pub is_draw: bool,
    pub is_div: bool,
    pub achievements: String,
}

/// Insert a single session stat row.
pub async fn insert_session_stat(pool: &SqlitePool, row: &SessionStatRow) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO session_stats (sort_key, ship_name, ship_id, player_id, game_time, match_group, \
         damage, spotting_damage, frags, raw_xp, base_xp, is_win, is_loss, is_draw, is_div, achievements) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )
    .bind(&row.sort_key)
    .bind(&row.ship_name)
    .bind(row.ship_id)
    .bind(row.player_id)
    .bind(&row.game_time)
    .bind(&row.match_group)
    .bind(row.damage)
    .bind(row.spotting_damage)
    .bind(row.frags)
    .bind(row.raw_xp)
    .bind(row.base_xp)
    .bind(row.is_win)
    .bind(row.is_loss)
    .bind(row.is_draw)
    .bind(row.is_div)
    .bind(&row.achievements)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Load all session stats ordered by sort_key.
pub async fn get_all_session_stats(pool: &SqlitePool) -> Result<Vec<SessionStatRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM session_stats ORDER BY sort_key ASC").fetch_all(pool).await
}

/// Delete all session stats (used during migration to re-import).
pub async fn clear_session_stats(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM session_stats").execute(pool).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// sent_replays
// ---------------------------------------------------------------------------

/// Insert a sent replay path.
pub async fn insert_sent_replay(pool: &SqlitePool, path: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO sent_replays (replay_path) VALUES (?1)").bind(path).execute(pool).await?;
    Ok(())
}

/// Load all sent replay paths.
pub async fn get_all_sent_replays(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT replay_path FROM sent_replays").fetch_all(pool).await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// Clear all sent replays.
pub async fn clear_sent_replays(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sent_replays").execute(pool).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// chart_configs
// ---------------------------------------------------------------------------

/// Upsert a chart configuration (JSON blob).
pub async fn upsert_chart_config(pool: &SqlitePool, chart_id: i64, config_json: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO chart_configs (chart_id, config) VALUES (?1, ?2) \
         ON CONFLICT(chart_id) DO UPDATE SET config = ?2",
    )
    .bind(chart_id)
    .bind(config_json)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load all chart configs.
pub async fn get_all_chart_configs(pool: &SqlitePool) -> Result<Vec<(i64, String)>, sqlx::Error> {
    sqlx::query_as("SELECT chart_id, config FROM chart_configs").fetch_all(pool).await
}

/// Delete all chart configs (for migration).
pub async fn clear_chart_configs(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM chart_configs").execute(pool).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// armor_viewer_defaults
// ---------------------------------------------------------------------------

/// A row from the armor_viewer_defaults table.
#[derive(Debug, sqlx::FromRow)]
pub struct ArmorViewerDefaultsRow {
    pub show_plate_edges: bool,
    pub show_waterline: bool,
    pub show_zero_mm: bool,
    pub armor_opacity: f64,
    pub waterline_opacity: f64,
    pub hull_opaque: bool,
    pub hull_all_visible: bool,
    pub armor_all_visible: bool,
    pub show_splash_boxes: bool,
}

/// Save armor viewer defaults (upsert single row).
pub async fn save_armor_viewer_defaults(pool: &SqlitePool, d: &ArmorViewerDefaultsRow) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO armor_viewer_defaults \
         (id, show_plate_edges, show_waterline, show_zero_mm, armor_opacity, waterline_opacity, \
          hull_opaque, hull_all_visible, armor_all_visible, show_splash_boxes) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(id) DO UPDATE SET \
         show_plate_edges=?1, show_waterline=?2, show_zero_mm=?3, armor_opacity=?4, \
         waterline_opacity=?5, hull_opaque=?6, hull_all_visible=?7, armor_all_visible=?8, \
         show_splash_boxes=?9",
    )
    .bind(d.show_plate_edges)
    .bind(d.show_waterline)
    .bind(d.show_zero_mm)
    .bind(d.armor_opacity)
    .bind(d.waterline_opacity)
    .bind(d.hull_opaque)
    .bind(d.hull_all_visible)
    .bind(d.armor_all_visible)
    .bind(d.show_splash_boxes)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load armor viewer defaults.
pub async fn get_armor_viewer_defaults(pool: &SqlitePool) -> Result<Option<ArmorViewerDefaultsRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT show_plate_edges, show_waterline, show_zero_mm, armor_opacity, waterline_opacity, \
                    hull_opaque, hull_all_visible, armor_all_visible, show_splash_boxes \
                    FROM armor_viewer_defaults WHERE id = 1",
    )
    .fetch_optional(pool)
    .await
}

// ---------------------------------------------------------------------------
// render_options
// ---------------------------------------------------------------------------

/// Save render options as JSON blob.
pub async fn save_render_options(pool: &SqlitePool, json: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO render_options (id, data) VALUES (1, ?1) \
         ON CONFLICT(id) DO UPDATE SET data = ?1",
    )
    .bind(json)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load render options JSON blob.
pub async fn get_render_options(pool: &SqlitePool) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT data FROM render_options WHERE id = 1").fetch_optional(pool).await?;
    Ok(row.map(|(d,)| d))
}

// ---------------------------------------------------------------------------
// dock_layouts
// ---------------------------------------------------------------------------

/// Save a dock layout by name.
pub async fn save_dock_layout(pool: &SqlitePool, name: &str, layout_json: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO dock_layouts (name, layout) VALUES (?1, ?2) \
         ON CONFLICT(name) DO UPDATE SET layout = ?2",
    )
    .bind(name)
    .bind(layout_json)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load a dock layout by name.
pub async fn get_dock_layout(pool: &SqlitePool, name: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT layout FROM dock_layouts WHERE name = ?1").bind(name).fetch_optional(pool).await?;
    Ok(row.map(|(l,)| l))
}

// ---------------------------------------------------------------------------
// mod_manager
// ---------------------------------------------------------------------------

/// Save mod manager state as JSON blob.
pub async fn save_mod_manager(pool: &SqlitePool, json: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO mod_manager (id, data) VALUES (1, ?1) \
         ON CONFLICT(id) DO UPDATE SET data = ?1",
    )
    .bind(json)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load mod manager state JSON blob.
pub async fn get_mod_manager(pool: &SqlitePool) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT data FROM mod_manager WHERE id = 1").fetch_optional(pool).await?;
    Ok(row.map(|(d,)| d))
}

// ---------------------------------------------------------------------------
// cap_layouts
// ---------------------------------------------------------------------------

/// Upsert a single cap layout (rkyv blob).
pub async fn upsert_cap_layout(
    pool: &SqlitePool,
    map_id: i64,
    scenario_config_id: i64,
    layout_data: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO cap_layouts (map_id, scenario_config_id, layout_data) VALUES (?1, ?2, ?3) \
         ON CONFLICT(map_id, scenario_config_id) DO UPDATE SET layout_data = ?3",
    )
    .bind(map_id)
    .bind(scenario_config_id)
    .bind(layout_data)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load all cap layout rows as (map_id, scenario_config_id, rkyv_blob).
pub async fn get_all_cap_layouts(pool: &SqlitePool) -> Result<Vec<(i64, i64, Vec<u8>)>, sqlx::Error> {
    sqlx::query_as("SELECT map_id, scenario_config_id, layout_data FROM cap_layouts").fetch_all(pool).await
}
