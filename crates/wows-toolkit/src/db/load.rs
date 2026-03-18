//! Load persisted state from SQLite back into `TabState`.
//!
//! This is the reverse of `migrate_ron`: it reads tables and populates the
//! in-memory app state that was previously loaded from `app.ron`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use sqlx::SqlitePool;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::data::session_stats::PerGameStat;
use crate::data::session_stats::SerializableAchievement;
use crate::data::session_stats::SessionStats;
use crate::tab_state::TabState;
use crate::tab_state::WindowKind;
use crate::tab_state::WindowSettings;
use crate::ui::player_tracker::PlayerTracker;

use super::queries;

/// Load all persisted state from SQLite into `tab_state`.
///
/// Fields that are not found in the database keep their current (default) values.
pub async fn load_tab_state_from_db(pool: &SqlitePool, tab_state: &mut TabState) -> Result<(), sqlx::Error> {
    info!("Loading state from SQLite...");

    load_settings(pool, tab_state).await?;
    load_session_stats(pool, tab_state).await?;
    load_tracked_players(pool, tab_state).await?;
    load_sent_replays(pool, tab_state).await?;
    load_chart_configs(pool, tab_state).await?;
    load_armor_viewer_defaults(pool, tab_state).await?;
    load_render_options(pool, tab_state).await?;
    load_dock_layout(pool, tab_state).await?;
    load_mod_manager(pool, tab_state).await?;

    info!("State loaded from SQLite");
    Ok(())
}

/// Load scalar settings from the k/v table.
#[allow(clippy::await_holding_lock)]
async fn load_settings(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let mut p = ts.persisted.write();
    let s = &mut p.settings;

    if let Some(v) = queries::get_setting::<PathBuf>(pool, "current_replay_path").await {
        s.game.current_replay_path = v;
    }
    if let Some(v) = queries::get_setting::<String>(pool, "wows_dir").await {
        s.game.wows_dir = v;
    }
    if let Some(v) = queries::get_setting::<Option<String>>(pool, "locale").await {
        s.app.locale = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "check_for_updates").await {
        s.app.check_for_updates = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "send_replay_data").await {
        s.integrations.send_replay_data = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "has_052_game_params_fix").await {
        s.game.has_052_game_params_fix = v;
    }
    // twitch_token: Option<Token> — stored as JSON
    if let Some(v) = queries::get_setting(pool, "twitch_token").await {
        s.integrations.twitch_token = v;
    }
    if let Some(v) = queries::get_setting::<String>(pool, "twitch_monitored_channel").await {
        s.integrations.twitch_monitored_channel = v;
    }
    if let Some(v) = queries::get_setting::<Option<String>>(pool, "constants_file_commit").await {
        s.game.constants_file_commit = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "debug_mode").await {
        s.app.debug_mode = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "build_consent_window_shown").await {
        s.app.build_consent_window_shown = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "language_selection_shown").await {
        s.app.language_selection_shown = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "session_stats_limit_enabled").await {
        s.stats_filters.limit_enabled = v;
    }
    if let Some(v) = queries::get_setting::<usize>(pool, "session_stats_game_count").await {
        s.stats_filters.game_count = v;
    }
    if let Some(v) = queries::get_setting(pool, "session_stats_division_filter").await {
        s.stats_filters.division_filter = v;
    }
    if let Some(v) = queries::get_setting::<BTreeSet<String>>(pool, "session_stats_game_mode_filter").await {
        s.stats_filters.game_mode_filter = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "suppress_gpu_encoder_warning").await {
        s.app.suppress_gpu_encoder_warning = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "enable_logging").await {
        s.app.enable_logging = v;
    }
    if let Some(v) = queries::get_setting::<f32>(pool, "zoom_factor").await {
        s.app.zoom_factor = v;
    }
    if let Some(v) = queries::get_setting::<String>(pool, "collab_display_name").await {
        s.collab.display_name = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "suppress_p2p_ip_warning").await {
        s.collab.suppress_p2p_ip_warning = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "disable_auto_open_session_windows").await {
        s.collab.disable_auto_open_session_windows = v;
    }

    // Nested struct: replay_settings
    if let Some(v) = queries::get_setting(pool, "replay_settings").await {
        s.replay = v;
    }

    // Fields that moved to PersistedState.
    if let Some(v) = queries::get_setting::<String>(pool, "output_dir").await {
        p.output_dir = v;
    }
    if let Some(v) = queries::get_setting::<bool>(pool, "auto_load_latest_replay").await {
        p.auto_load_latest_replay = v;
    }
    if let Some(v) = queries::get_setting::<u64>(pool, "next_chart_tab_id").await {
        p.next_chart_tab_id = v;
    }

    // Drop the write guard before accessing ts fields directly.
    drop(p);

    if let Some(v) = queries::get_setting(pool, "replay_sort").await {
        *ts.replay_sort.lock() = v;
    }

    // Window sizes/geometry.
    if let Some(sizes) =
        queries::get_setting::<std::collections::HashMap<WindowKind, WindowSettings>>(pool, "window_sizes").await
    {
        ts.window_settings.lock().settings = sizes;
    }

    info!("  loaded settings");
    Ok(())
}

/// Load session stats from the database.
async fn load_session_stats(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let rows = queries::get_all_session_stats(pool).await?;
    let mut games = Vec::with_capacity(rows.len());

    for row in rows {
        let achievements: Vec<SerializableAchievement> = serde_json::from_str(&row.achievements).unwrap_or_default();

        games.push(PerGameStat {
            ship_name: row.ship_name,
            ship_id: (row.ship_id as u64).into(),
            game_time: row.game_time,
            sort_key: row.sort_key,
            player_id: row.player_id,
            damage: row.damage as u64,
            spotting_damage: row.spotting_damage as u64,
            frags: row.frags,
            raw_xp: row.raw_xp,
            base_xp: row.base_xp,
            is_win: row.is_win,
            is_loss: row.is_loss,
            is_draw: row.is_draw,
            is_div: row.is_div,
            match_group: row.match_group,
            achievements,
        });
    }

    let mut p = ts.persisted.write();
    p.session_stats = SessionStats { games, ..Default::default() };

    info!("  loaded {} session stats", p.session_stats.games.len());
    Ok(())
}

/// Load tracked players from the database.
async fn load_tracked_players(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    // We stored the entire PlayerTracker as a JSON blob for simplicity
    // (private fields, complex nested structure).
    if let Some(json) = queries::get_setting::<String>(pool, "player_tracker_data").await {
        match serde_json::from_str::<PlayerTracker>(&json) {
            Ok(pt) => {
                *ts.player_tracker.write() = pt;
            }
            Err(e) => {
                error!("Failed to deserialize player tracker from DB: {e}");
            }
        }
    }

    // Restore filter_time_period from settings.
    if let Some(v) = queries::get_setting(pool, "player_tracker.filter_time_period").await {
        ts.player_tracker.write().filter_time_period = v;
    }

    info!("  loaded player tracker");
    Ok(())
}

/// Load sent replays.
async fn load_sent_replays(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let paths = queries::get_all_sent_replays(pool).await?;
    let mut set = ts.sent_replays.write();
    set.clear();
    for p in &paths {
        set.insert(p.clone());
    }
    info!("  loaded {} sent replays", set.len());
    Ok(())
}

/// Load chart configs.
async fn load_chart_configs(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    let rows = queries::get_all_chart_configs(pool).await?;
    let mut p = ts.persisted.write();
    p.chart_configs.clear();
    for (chart_id, json) in rows {
        match serde_json::from_str(&json) {
            Ok(config) => {
                p.chart_configs.insert(chart_id as u64, config);
            }
            Err(e) => {
                warn!("Failed to deserialize chart config {chart_id}: {e}");
            }
        }
    }
    info!("  loaded {} chart configs", p.chart_configs.len());
    Ok(())
}

/// Load armor viewer defaults.
async fn load_armor_viewer_defaults(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(row) = queries::get_armor_viewer_defaults(pool).await? {
        let mut p = ts.persisted.write();
        p.armor_viewer_defaults.show_plate_edges = row.show_plate_edges;
        p.armor_viewer_defaults.show_waterline = row.show_waterline;
        p.armor_viewer_defaults.show_zero_mm = row.show_zero_mm;
        p.armor_viewer_defaults.armor_opacity = row.armor_opacity as f32;
        p.armor_viewer_defaults.waterline_opacity = row.waterline_opacity as f32;
        p.armor_viewer_defaults.hull_opaque = row.hull_opaque;
        p.armor_viewer_defaults.hull_all_visible = row.hull_all_visible;
        p.armor_viewer_defaults.armor_all_visible = row.armor_all_visible;
        p.armor_viewer_defaults.show_splash_boxes = row.show_splash_boxes;
    }
    info!("  loaded armor viewer defaults");
    Ok(())
}

/// Load render options.
async fn load_render_options(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(json) = queries::get_render_options(pool).await? {
        match serde_json::from_str(&json) {
            Ok(opts) => ts.persisted.write().settings.renderer = opts,
            Err(e) => warn!("Failed to deserialize render options: {e}"),
        }
    }
    info!("  loaded render options");
    Ok(())
}

/// Load stats dock layout.
async fn load_dock_layout(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(json) = queries::get_dock_layout(pool, "stats").await? {
        match serde_json::from_str(&json) {
            Ok(layout) => ts.persisted.write().stats_dock_state = layout,
            Err(e) => warn!("Failed to deserialize dock layout: {e}"),
        }
    }
    info!("  loaded dock layout");
    Ok(())
}

/// Load mod manager state.
async fn load_mod_manager(pool: &SqlitePool, ts: &mut TabState) -> Result<(), sqlx::Error> {
    if let Some(json) = queries::get_mod_manager(pool).await? {
        match serde_json::from_str(&json) {
            Ok(info) => ts.persisted.write().mod_manager_info = info,
            Err(e) => warn!("Failed to deserialize mod manager info: {e}"),
        }
    }
    info!("  loaded mod manager info");
    Ok(())
}
