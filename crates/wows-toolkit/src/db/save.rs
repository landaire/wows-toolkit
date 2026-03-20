//! Background save task — persists app state to SQLite when notified
//! and periodically for window geometry, decoupled from UI painting.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use parking_lot::RwLock;
use sqlx::SqlitePool;
use tracing::error;
use tracing::info;

use crate::tab_state::SharedPersistedState;
use crate::tab_state::SharedWindowSettings;
use crate::tab_state::WindowKind;
use crate::tab_state::WindowSettings;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::SortOrder;

/// Shared references the save task needs, cloned from `TabState` at launch.
#[derive(Clone)]
pub struct SaveContext {
    pub persisted: SharedPersistedState,
    pub player_tracker: Arc<RwLock<PlayerTracker>>,
    pub sent_replays: Arc<RwLock<HashSet<String>>>,
    pub replay_sort: Arc<Mutex<SortOrder>>,
    pub window_settings: SharedWindowSettings,
    pub active_viewports: Arc<Mutex<Vec<(WindowKind, egui::ViewportId)>>>,
    pub save_notify: Arc<tokio::sync::Notify>,
}

/// Spawn the background save task on the tokio runtime.
///
/// The task saves immediately when notified via `save_notify`, with a short
/// debounce to coalesce rapid changes. It also runs a periodic timer for
/// window geometry capture.
///
/// Sends a final save on `shutdown_rx` before exiting.
pub fn spawn_save_task(
    runtime: &Arc<tokio::runtime::Runtime>,
    pool: SqlitePool,
    ctx: SaveContext,
    egui_ctx: egui::Context,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    runtime.spawn(async move {
        // Periodic timer for capturing window geometry (which changes
        // continuously during resize/move and has no discrete event).
        let mut geometry_interval = tokio::time::interval(Duration::from_secs(30));
        // Skip the first immediate tick.
        geometry_interval.tick().await;

        loop {
            tokio::select! {
                _ = ctx.save_notify.notified() => {
                    // Debounce: wait a moment to coalesce rapid changes,
                    // then drain any further notifications that arrived.
                    tokio::time::sleep(Duration::from_secs(1)).await;

                    capture_window_settings(&egui_ctx, &ctx);
                    if let Err(e) = do_save(&pool, &ctx).await {
                        error!("Save (notified) failed: {e}");
                    }
                }
                _ = geometry_interval.tick() => {
                    capture_window_settings(&egui_ctx, &ctx);
                    if let Err(e) = do_save(&pool, &ctx).await {
                        error!("Background save failed: {e}");
                    }
                }
                _ = &mut shutdown_rx => {
                    info!("Save task received shutdown signal, performing final save...");
                    capture_window_settings(&egui_ctx, &ctx);
                    if let Err(e) = do_save(&pool, &ctx).await {
                        error!("Final save failed: {e}");
                    }
                    break;
                }
            }
        }
        info!("Save task exited");
    })
}

/// Capture viewport geometry for all known windows.
fn capture_window_settings(egui_ctx: &egui::Context, ctx: &SaveContext) {
    let viewports = ctx.active_viewports.lock().clone();
    let mut tracker = ctx.window_settings.lock();

    // Main viewport.
    let main_info = egui_ctx.input(|i| i.viewport().clone());
    tracker.settings.insert(WindowKind::Main, WindowSettings::from_viewport_info(&main_info));

    // Secondary viewports.
    for &(kind, viewport_id) in &viewports {
        let info = egui_ctx.input_for(viewport_id, |i| i.viewport().clone());
        tracker.settings.insert(kind, WindowSettings::from_viewport_info(&info));
    }
}

/// Perform the actual save — writes all state to SQLite.
async fn do_save(pool: &SqlitePool, ctx: &SaveContext) -> Result<(), sqlx::Error> {
    super::migrate_ron::save_state_to_db(pool, ctx).await?;
    // Ensure the migration flag is set so the next launch knows to load from
    // SQLite. This is a no-op after the first successful save (idempotent upsert).
    super::set_migrated(pool).await?;
    Ok(())
}
