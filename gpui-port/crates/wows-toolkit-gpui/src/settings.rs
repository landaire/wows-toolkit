//! Settings snapshot loaded from the shared SQLite config DB at startup.
//!
//! Read-only: the GPUI port does not write settings back to the database.

use std::path::PathBuf;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_config::queries;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

/// Zoom factor applied when the `zoom_factor` setting has never been saved,
/// matching the egui app's documented default.
pub const DEFAULT_ZOOM: f32 = 1.15;

/// Settings read from the shared config DB, applied once at startup.
pub struct GpuiSettings {
    pub zoom: f32,
    pub wows_dir: String,
    pub current_replay_path: PathBuf,
    pub replay: ReplaySettings,
    /// `None` when the `armor_viewer_defaults` table has no row yet (fresh DB).
    pub armor_defaults: Option<ArmorViewerDefaultsRow>,
}

impl GpuiSettings {
    /// Load all leaf settings this tab displays. Each `get_setting` miss falls
    /// back to that field's documented default rather than a sentinel value.
    pub async fn load(pool: &SqlitePool) -> Self {
        let zoom = queries::get_setting::<f32>(pool, "zoom_factor").await.unwrap_or(DEFAULT_ZOOM);
        let wows_dir = queries::get_setting::<String>(pool, "wows_dir").await.unwrap_or_default();
        let current_replay_path =
            queries::get_setting::<PathBuf>(pool, "current_replay_path").await.unwrap_or_default();
        let replay = queries::get_setting::<ReplaySettings>(pool, "replay_settings").await.unwrap_or_default();
        let armor_defaults = queries::get_armor_viewer_defaults(pool).await.ok().flatten();

        Self { zoom, wows_dir, current_replay_path, replay, armor_defaults }
    }
}
