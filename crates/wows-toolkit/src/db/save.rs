//! Background save task — persists app state to SQLite on a timer,
//! decoupled from UI painting.

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
}

/// Spawn the background save task on the tokio runtime.
///
/// The task runs a 30-second interval timer. On each tick it captures window
/// geometry from `egui::Context`, reads all shared state, and writes to SQLite.
///
/// Sends a final save on `shutdown_rx` before exiting.
pub fn spawn_save_task(
    runtime: &Arc<tokio::runtime::Runtime>,
    pool: SqlitePool,
    ctx: SaveContext,
    egui_ctx: egui::Context,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    runtime.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        // The first tick fires immediately — skip it so we don't save right after load.
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
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
    });
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
    super::migrate_ron::save_state_to_db(pool, ctx).await
}
