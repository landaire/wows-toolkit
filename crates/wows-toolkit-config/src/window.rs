//! Persisted window geometry shared between the egui app and GPUI port.
//!
//! The egui-specific constructors/appliers live in the egui app as an
//! extension trait; this crate holds the serializable data and the read and
//! write of the one settings row both apps keep it in.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use sqlx::sqlite::SqlitePool;

/// Identifies which type of window settings to persist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowKind {
    Main,
    ReplayRenderer,
    TacticsBoard,
    ArmorViewer,
}

/// Persisted window geometry, modeled after `egui_winit::WindowSettings`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub inner_size_points: Option<[f32; 2]>,
    pub outer_position_pixels: Option<[f32; 2]>,
    pub fullscreen: bool,
    pub maximized: bool,
}

/// The settings row both apps keep every window's geometry in.
const KEY: &str = "window_sizes";

/// What is remembered about each kind of window.
pub async fn load_all(pool: &SqlitePool) -> HashMap<WindowKind, WindowSettings> {
    crate::queries::get_setting(pool, KEY).await.unwrap_or_default()
}

/// Remembers `settings` for `kind`, leaving every other kind as it was.
///
/// Read and write in one immediate transaction, because the row holds every
/// window at once: each app knows only the windows it opens, so writing the map
/// it has in hand would drop the kinds only the other one opens. The
/// transaction is what makes a write from each of them at the same moment
/// resolve to one or the other rather than to a mixture.
pub async fn store(pool: &SqlitePool, kind: WindowKind, settings: WindowSettings) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *tx).await.ok();

    let stored: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = ?1").bind(KEY).fetch_optional(&mut *tx).await?;

    let mut sizes: HashMap<WindowKind, WindowSettings> = stored
        .and_then(|(json,)| match serde_json::from_str(&json) {
            Ok(sizes) => Some(sizes),
            // A row that cannot be read is replaced rather than kept: geometry
            // is worth one window's placement, not a refusal to remember
            // anything ever again.
            Err(err) => {
                tracing::warn!("window geometry: the stored sizes could not be read, starting again: {err}");
                None
            }
        })
        .unwrap_or_default();

    sizes.insert(kind, settings);

    let json = serde_json::to_string(&sizes).map_err(|err| sqlx::Error::Protocol(err.to_string()))?;
    sqlx::query("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2")
        .bind(KEY)
        .bind(&json)
        .execute(&mut *tx)
        .await?;

    tx.commit().await
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    /// A write remembers one kind without forgetting the kinds this app never
    /// opens: the row is shared with the other front end, which opens windows
    /// this one has no equivalent of.
    #[tokio::test]
    async fn storing_one_kind_keeps_the_others() {
        let pool = crate::test_pool().await;
        let board = WindowSettings { inner_size_points: Some([640., 480.]), ..WindowSettings::default() };
        crate::queries::set_setting(&pool, KEY, &HashMap::from([(WindowKind::TacticsBoard, board)]))
            .await
            .expect("the other app's row is written");

        let main =
            WindowSettings { inner_size_points: Some([1200., 800.]), maximized: true, ..WindowSettings::default() };
        store(&pool, WindowKind::Main, main).await.expect("the write lands");

        let remembered = load_all(&pool).await;
        assert_eq!(remembered.get(&WindowKind::Main), Some(&main));
        assert_eq!(
            remembered.get(&WindowKind::TacticsBoard),
            Some(&board),
            "the other app's window is still remembered"
        );
    }

    /// Nothing remembered is an empty map rather than a failure, which is what a
    /// first launch reads.
    #[tokio::test]
    async fn an_unwritten_row_remembers_nothing() {
        let pool = crate::test_pool().await;
        assert!(load_all(&pool).await.is_empty());
    }
}
