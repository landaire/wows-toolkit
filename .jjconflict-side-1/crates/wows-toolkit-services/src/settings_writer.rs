//! Writing settings back to the shared config database.
//!
//! Every save carries the whole value, so the newest save of a key is the only
//! one that matters. Writes run one at a time, and a write that a newer save of
//! the same key has overtaken is dropped: two writes committing out of order
//! would otherwise leave the older value in the row. The armor viewer's
//! defaults row, a table of its own, is ordered the same way.

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

/// What the armor viewer's defaults row is ordered under. No setting is saved
/// under this key, since the row is a table of its own.
const ARMOR_DEFAULTS_KEY: &str = "armor_viewer_defaults";

/// The pool settings edits are written through, and the order they commit in.
/// Cheap to clone.
#[derive(Clone)]
pub struct SettingsWriter {
    pool: SqlitePool,
    writes: Arc<Writes>,
}

impl SettingsWriter {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, writes: Arc::default() }
    }

    /// The open config pool, for the readers that query it directly.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Claims the next place in `key`'s order for `json`.
    ///
    /// Called where the edit is made, so saves are ordered as they were made
    /// whenever the returned write commits.
    pub fn begin(&self, key: &'static str, json: String) -> PendingWrite {
        self.claim(key, Value::Setting(json))
    }

    /// Claims the next place in the armor viewer defaults row's order for
    /// `row`, as [`Self::begin`] does for a key.
    pub fn begin_armor_defaults(&self, row: ArmorViewerDefaultsRow) -> PendingWrite {
        self.claim(ARMOR_DEFAULTS_KEY, Value::ArmorDefaults(row))
    }

    fn claim(&self, key: &'static str, value: Value) -> PendingWrite {
        let generation = self.writes.next(key);

        PendingWrite { writer: self.clone(), key, value, generation }
    }
}

/// One save, waiting for its turn.
pub struct PendingWrite {
    writer: SettingsWriter,
    key: &'static str,
    value: Value,
    generation: u64,
}

impl PendingWrite {
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// Waits until every earlier write has committed. `None` when a newer save
    /// of the same key has been made since, which leaves nothing to write.
    /// Runs on any executor.
    pub async fn turn(self) -> Option<TurnedWrite> {
        let order = Arc::clone(&self.writer.writes.order).lock_owned().await;
        if !self.writer.writes.is_latest(self.key, self.generation) {
            return None;
        }

        Some(TurnedWrite { pending: self, _order: order })
    }
}

/// A save whose turn has come. Holds the write order until it is committed or
/// dropped.
pub struct TurnedWrite {
    pending: PendingWrite,
    _order: futures::lock::OwnedMutexGuard<()>,
}

impl TurnedWrite {
    /// Writes the value. Runs on the tokio runtime the pool was opened on.
    pub async fn commit(self) -> Result<(), sqlx::Error> {
        let pending = &self.pending;
        let pool = &pending.writer.pool;

        match &pending.value {
            Value::Setting(json) => wows_toolkit_config::queries::set_setting_raw(pool, pending.key, json).await,
            Value::ArmorDefaults(row) => wows_toolkit_config::queries::save_armor_viewer_defaults(pool, row).await,
        }
    }
}

/// What one save writes.
enum Value {
    /// A JSON value under its key in the settings table.
    Setting(String),
    ArmorDefaults(ArmorViewerDefaultsRow),
}

/// The order settings writes commit in.
#[derive(Default)]
struct Writes {
    /// The newest save of each key.
    latest: std::sync::Mutex<HashMap<&'static str, u64>>,
    /// Held for the length of one write.
    order: Arc<futures::lock::Mutex<()>>,
}

impl Writes {
    fn next(&self, key: &'static str) -> u64 {
        let mut latest = self.latest.lock().expect("settings write order lock");
        let generation = latest.entry(key).or_insert(0);
        *generation += 1;
        *generation
    }

    fn is_latest(&self, key: &'static str, generation: u64) -> bool {
        self.latest.lock().expect("settings write order lock").get(key) == Some(&generation)
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;
    use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

    use super::SettingsWriter;

    fn row(armor_opacity: f64) -> ArmorViewerDefaultsRow {
        ArmorViewerDefaultsRow {
            show_plate_edges: true,
            show_waterline: true,
            show_zero_mm: false,
            armor_opacity,
            waterline_opacity: 0.3,
            hull_opaque: false,
            hull_all_visible: true,
            armor_all_visible: true,
            show_splash_boxes: false,
            show_legend: true,
            legend_collapsed: false,
            legend_pos_x: None,
            legend_pos_y: None,
        }
    }

    /// Two saves of the defaults row whose writes run in either order: only the
    /// newer is written, so the older can never land last.
    #[test]
    fn an_overtaken_armor_defaults_row_is_not_written() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime starts");
        let _entered = runtime.enter();
        let pool = SqlitePoolOptions::new().connect_lazy("sqlite::memory:").expect("an in-memory pool opens");
        let writer = SettingsWriter::new(pool);

        let older = writer.begin_armor_defaults(row(0.2));
        let setting = writer.begin("model_viewer_show_armor", "true".to_owned());
        let newer = writer.begin_armor_defaults(row(0.8));

        assert!(futures::executor::block_on(newer.turn()).is_some(), "the newer row is written");
        assert!(futures::executor::block_on(older.turn()).is_none(), "the older row is dropped");
        assert!(futures::executor::block_on(setting.turn()).is_some(), "a setting saved between them is written");
    }
}
