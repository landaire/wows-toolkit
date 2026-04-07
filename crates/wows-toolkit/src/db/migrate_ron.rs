//! One-time migration from `app.ron` (eframe persistence) to SQLite.
//!
//! This reads all the persisted state from `TabState` (already deserialized by
//! eframe from `app.ron`) and writes it into the SQLite database.

use sqlx::SqlitePool;
use tracing::error;
use tracing::info;
use tracing::trace;

use crate::tab_state::TabState;

use super::queries;
use super::save::SaveContext;

/// Write all persisted state to SQLite using shared references.
///
/// This is idempotent: it clears destination tables before writing, so calling
/// it repeatedly with the same data is safe. Used by both the one-time migration
/// and the periodic background save.
pub async fn save_state_to_db(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    // Note: we do NOT hold a single read guard across all awaits because
    // `parking_lot::RwLockReadGuard` is not `Send`. Instead, each sub-function
    // takes its own short-lived guard.
    save_settings(pool, ctx).await?;
    save_session_stats(pool, ctx).await?;
    save_tracked_players(pool, ctx).await?;
    save_sent_replays(pool, ctx).await?;
    save_chart_configs(pool, ctx).await?;
    save_armor_viewer_defaults(pool, ctx).await?;
    save_render_options(pool, ctx).await?;
    save_dock_layout(pool, ctx).await?;
    save_mod_manager(pool, ctx).await?;
    Ok(())
}

/// Write all persisted state from `TabState` into SQLite.
///
/// Convenience wrapper that builds a `SaveContext` from `TabState` for the
/// one-time migration path where we still have access to TabState directly.
pub async fn save_tab_state_to_db(pool: &SqlitePool, ts: &TabState) -> Result<(), sqlx::Error> {
    let ctx = SaveContext {
        persisted: ts.persisted.clone(),
        player_tracker: ts.player_tracker.clone(),
        sent_replays: ts.sent_replays.clone(),
        replay_sort: ts.replay_sort.clone(),
        window_settings: ts.window_settings.clone(),
        active_viewports: ts.active_viewports.clone(),
        save_notify: ts.save_notify.clone(),
    };
    save_state_to_db(pool, &ctx).await
}

/// One-time migration from `app.ron` to SQLite. Calls [`save_tab_state_to_db`]
/// and then sets the `migration_completed` flag.
pub async fn migrate_tab_state_to_db(pool: &SqlitePool, tab_state: &TabState) -> Result<(), sqlx::Error> {
    info!("Starting migration from app.ron to SQLite...");
    save_tab_state_to_db(pool, tab_state).await?;
    super::set_migrated(pool).await?;
    info!("Migration from app.ron to SQLite completed successfully");
    Ok(())
}

/// Save scalar settings into the k/v `settings` table.
async fn save_settings(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    // Snapshot all values under a single short-lived read guard, then drop it
    // before the first `.await` (parking_lot guards are not Send).
    let (
        current_replay_path,
        wows_dir,
        has_052_fix,
        constants_commit,
        locale,
        check_updates,
        debug_mode,
        consent_shown,
        lang_shown,
        suppress_gpu,
        enable_log,
        send_replay,
        twitch_tok,
        twitch_ch,
        stats_limit,
        stats_count,
        stats_div,
        stats_modes,
        collab_name,
        collab_ip_warn,
        collab_auto_open,
        zoom_factor,
        replay_settings_json,
        output_dir,
        auto_load,
        next_chart_id,
        auto_dump_game_data,
        game_data_cache_dir,
    ) = {
        let p = ctx.persisted.read();
        let s = &p.settings;
        (
            serde_json::to_string(&s.game.current_replay_path).unwrap_or_default(),
            s.game.wows_dir.clone(),
            s.game.has_052_game_params_fix,
            s.game.constants_file_commit.clone(),
            s.app.locale.clone(),
            s.app.check_for_updates,
            s.app.debug_mode,
            s.app.build_consent_window_shown,
            s.app.language_selection_shown,
            s.app.suppress_gpu_encoder_warning,
            s.app.enable_logging,
            s.integrations.send_replay_data,
            serde_json::to_string(&s.integrations.twitch_token).unwrap_or_default(),
            s.integrations.twitch_monitored_channel.clone(),
            s.stats_filters.limit_enabled,
            s.stats_filters.game_count,
            serde_json::to_string(&s.stats_filters.division_filter).unwrap_or_default(),
            serde_json::to_string(&s.stats_filters.game_mode_filter).unwrap_or_default(),
            s.collab.display_name.clone(),
            s.collab.suppress_p2p_ip_warning,
            s.collab.disable_auto_open_session_windows,
            s.app.zoom_factor,
            serde_json::to_string(&s.replay).unwrap_or_default(),
            p.output_dir.clone(),
            p.auto_load_latest_replay,
            p.next_chart_tab_id,
            s.game.auto_dump_game_data,
            s.game.game_data_cache_dir.clone(),
        )
    };

    // Now write everything to the DB without holding any locks.
    queries::set_setting_raw(pool, "current_replay_path", &current_replay_path).await?;
    queries::set_setting(pool, "wows_dir", &wows_dir).await?;
    queries::set_setting(pool, "has_052_game_params_fix", &has_052_fix).await?;
    queries::set_setting(pool, "constants_file_commit", &constants_commit).await?;
    queries::set_setting(pool, "locale", &locale).await?;
    queries::set_setting(pool, "check_for_updates", &check_updates).await?;
    queries::set_setting(pool, "debug_mode", &debug_mode).await?;
    queries::set_setting(pool, "build_consent_window_shown", &consent_shown).await?;
    queries::set_setting(pool, "language_selection_shown", &lang_shown).await?;
    queries::set_setting(pool, "suppress_gpu_encoder_warning", &suppress_gpu).await?;
    queries::set_setting(pool, "enable_logging", &enable_log).await?;
    queries::set_setting(pool, "send_replay_data", &send_replay).await?;
    queries::set_setting_raw(pool, "twitch_token", &twitch_tok).await?;
    queries::set_setting(pool, "twitch_monitored_channel", &twitch_ch).await?;
    queries::set_setting(pool, "session_stats_limit_enabled", &stats_limit).await?;
    queries::set_setting(pool, "session_stats_game_count", &stats_count).await?;
    queries::set_setting_raw(pool, "session_stats_division_filter", &stats_div).await?;
    queries::set_setting_raw(pool, "session_stats_game_mode_filter", &stats_modes).await?;
    queries::set_setting(pool, "collab_display_name", &collab_name).await?;
    queries::set_setting(pool, "suppress_p2p_ip_warning", &collab_ip_warn).await?;
    queries::set_setting(pool, "disable_auto_open_session_windows", &collab_auto_open).await?;
    queries::set_setting(pool, "zoom_factor", &zoom_factor).await?;
    queries::set_setting_raw(pool, "replay_settings", &replay_settings_json).await?;
    queries::set_setting(pool, "output_dir", &output_dir).await?;
    queries::set_setting(pool, "auto_load_latest_replay", &auto_load).await?;
    let replay_sort = *ctx.replay_sort.lock();
    queries::set_setting(pool, "replay_sort", &replay_sort).await?;
    queries::set_setting(pool, "next_chart_tab_id", &next_chart_id).await?;
    queries::set_setting(pool, "auto_dump_game_data", &auto_dump_game_data).await?;
    queries::set_setting(pool, "game_data_cache_dir", &game_data_cache_dir).await?;

    // Window sizes/geometry.
    let sizes = ctx.window_settings.lock().settings.clone();
    queries::set_setting(pool, "window_sizes", &sizes).await?;

    // Player tracker UI state.
    let filter_period = {
        let pt = ctx.player_tracker.read();
        serde_json::to_string(&pt.filter_time_period).unwrap_or_default()
    };
    queries::set_setting_raw(pool, "player_tracker.filter_time_period", &filter_period).await?;

    trace!("  saved settings");
    Ok(())
}

/// Save session stats games.
async fn save_session_stats(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    // Snapshot rows under a short-lived read guard.
    let rows: Vec<queries::SessionStatRow> = {
        let p = ctx.persisted.read();
        p.session_stats
            .games
            .iter()
            .map(|game| queries::SessionStatRow {
                id: 0,
                sort_key: game.sort_key.clone(),
                ship_name: game.ship_name.clone(),
                ship_id: game.ship_id.raw() as i64,
                player_id: game.player_id,
                game_time: game.game_time.clone(),
                match_group: game.match_group.clone(),
                damage: game.damage as i64,
                spotting_damage: game.spotting_damage as i64,
                frags: game.frags,
                raw_xp: game.raw_xp,
                base_xp: game.base_xp,
                is_win: game.is_win,
                is_loss: game.is_loss,
                is_draw: game.is_draw,
                is_div: game.is_div,
                achievements: serde_json::to_string(&game.achievements).unwrap_or_else(|_| "[]".to_string()),
            })
            .collect()
    };

    // Use a transaction so a crash mid-save doesn't wipe the table.
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM session_stats").execute(&mut *tx).await?;
    for row in &rows {
        sqlx::query(
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
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    trace!("  saved {} session stats", rows.len());
    Ok(())
}

/// Save tracked players.
async fn save_tracked_players(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    let json = {
        let pt = ctx.player_tracker.read();
        serde_json::to_string(&*pt).ok()
    };

    match json {
        Some(json) => {
            queries::set_setting(pool, "player_tracker_data", &json).await?;
        }
        None => {
            error!("Failed to serialize player tracker");
        }
    }

    trace!("  saved player tracker");
    Ok(())
}

/// Save sent replays set.
async fn save_sent_replays(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    let paths: Vec<String> = ctx.sent_replays.read().iter().cloned().collect();

    // Upsert current paths, then remove any that were deleted.
    for path in &paths {
        queries::insert_sent_replay(pool, path).await?;
    }
    queries::delete_stale_sent_replays(pool, &paths).await?;

    trace!("  saved {} sent replays", paths.len());
    Ok(())
}

/// Save chart configs.
async fn save_chart_configs(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    let configs: Vec<(i64, String)> = {
        let p = ctx.persisted.read();
        p.chart_configs
            .iter()
            .filter_map(|(&id, config)| serde_json::to_string(config).ok().map(|json| (id as i64, json)))
            .collect()
    };

    // Upsert current configs, then remove any that were deleted.
    let current_ids: Vec<i64> = configs.iter().map(|(id, _)| *id).collect();
    for (chart_id, json) in &configs {
        queries::upsert_chart_config(pool, *chart_id, json).await?;
    }
    queries::delete_stale_chart_configs(pool, &current_ids).await?;

    trace!("  saved {} chart configs", configs.len());
    Ok(())
}

/// Save armor viewer defaults.
async fn save_armor_viewer_defaults(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    let row = {
        let p = ctx.persisted.read();
        let d = &p.armor_viewer_defaults;
        queries::ArmorViewerDefaultsRow {
            show_plate_edges: d.show_plate_edges,
            show_waterline: d.show_waterline,
            show_zero_mm: d.show_zero_mm,
            armor_opacity: d.armor_opacity as f64,
            waterline_opacity: d.waterline_opacity as f64,
            hull_opaque: d.hull_opaque,
            hull_all_visible: d.hull_all_visible,
            armor_all_visible: d.armor_all_visible,
            show_splash_boxes: d.show_splash_boxes,
        }
    };
    queries::save_armor_viewer_defaults(pool, &row).await?;

    trace!("  saved armor viewer defaults");
    Ok(())
}

/// Save render options.
async fn save_render_options(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    let json = {
        let p = ctx.persisted.read();
        serde_json::to_string(&p.settings.renderer).ok()
    };

    match json {
        Some(json) => queries::save_render_options(pool, &json).await?,
        None => error!("Failed to serialize render options"),
    }

    trace!("  saved render options");
    Ok(())
}

/// Save stats dock layout.
async fn save_dock_layout(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    let json = {
        let p = ctx.persisted.read();
        serde_json::to_string(&p.stats_dock_state).ok()
    };

    match json {
        Some(json) => queries::save_dock_layout(pool, "stats", &json).await?,
        None => error!("Failed to serialize dock layout"),
    }

    trace!("  saved dock layout");
    Ok(())
}

/// Save mod manager state.
async fn save_mod_manager(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    let json = {
        let p = ctx.persisted.read();
        serde_json::to_string(&p.mod_manager_info).ok()
    };

    match json {
        Some(json) => queries::save_mod_manager(pool, &json).await?,
        None => error!("Failed to serialize mod manager info"),
    }

    trace!("  saved mod manager info");
    Ok(())
}
