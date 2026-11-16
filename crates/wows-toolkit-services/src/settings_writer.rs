//! Writing settings back to the shared config database.
//!
//! Every save carries the whole value, so the newest save of a key is the only
//! one that matters. Writes commit one at a time in the order they were made,
//! and a write that a newer save of the same key has overtaken is dropped. The
//! armor viewer's defaults row, a table of its own, is ordered the same way, and
//! so is a clear of every setting: a save made before it never lands after it.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::Shared;
use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

/// What the armor viewer's defaults row is ordered under. No setting is saved
/// under this key, since the row is a table of its own.
const ARMOR_DEFAULTS_KEY: &str = "armor_viewer_defaults";

/// What a clear of every setting is ordered under.
const CLEAR_KEY: &str = "settings_clear";

/// The pool settings edits are written through, and the order they commit in.
/// Cheap to clone.
#[derive(Clone)]
pub struct SettingsWriter {
    pool: SqlitePool,
    writes: Arc<Writes>,
}

impl SettingsWriter {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, writes: Arc::new(Writes::new()) }
    }

    /// The open config pool, for the readers that query it directly.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Claims the next place in the write order for `json` under `key`.
    ///
    /// Called where the edit is made, so saves commit in the order they were
    /// made whenever the returned write is turned.
    pub fn begin(&self, key: &'static str, json: String) -> PendingWrite {
        self.claim(key, Value::Setting(json))
    }

    /// Claims the next place in the write order for the armor viewer defaults
    /// row, as [`Self::begin`] does for a key.
    pub fn begin_armor_defaults(&self, row: ArmorViewerDefaultsRow) -> PendingWrite {
        self.claim(ARMOR_DEFAULTS_KEY, Value::ArmorDefaults(row))
    }

    /// Claims the next place in the write order for putting every setting but
    /// `kept` back to its default ([`wows_toolkit_config::queries::clear_settings`]).
    /// Saves claimed before it commit first, and saves claimed after it last.
    pub fn begin_clear(&self, kept: Vec<&'static str>) -> PendingWrite {
        self.claim(CLEAR_KEY, Value::Clear(kept))
    }

    fn claim(&self, key: &'static str, value: Value) -> PendingWrite {
        let generation = self.writes.next(key);
        let (done, after) = self.writes.enqueue();

        PendingWrite { writer: self.clone(), key, value, generation, after, _done: done }
    }
}

/// One save, waiting for its turn.
pub struct PendingWrite {
    writer: SettingsWriter,
    key: &'static str,
    value: Value,
    generation: u64,
    /// Resolves once the write claimed just before this one has committed or
    /// been dropped.
    after: Turn,
    /// Dropped once this write has committed or been dropped, which lets the
    /// next one go.
    _done: oneshot::Sender<()>,
}

impl PendingWrite {
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// Waits until every earlier write has committed. `None` when a newer save
    /// of the same key has been made since, which leaves nothing to write.
    /// Runs on any executor.
    pub async fn turn(self) -> Option<TurnedWrite> {
        // Either answer means the earlier write is finished with.
        let _ = self.after.clone().await;
        if !self.writer.writes.is_latest(self.key, self.generation) {
            return None;
        }

        Some(TurnedWrite { pending: self })
    }
}

/// A save whose turn has come. Holds the write order until it is dropped.
pub struct TurnedWrite {
    pending: PendingWrite,
}

impl TurnedWrite {
    /// Writes the value. Runs on the tokio runtime the pool was opened on.
    pub async fn commit(&self) -> Result<(), sqlx::Error> {
        let pending = &self.pending;
        let pool = &pending.writer.pool;

        match &pending.value {
            Value::Setting(json) => wows_toolkit_config::queries::set_setting_raw(pool, pending.key, json).await,
            Value::ArmorDefaults(row) => wows_toolkit_config::queries::save_armor_viewer_defaults(pool, row).await,
            Value::Clear(kept) => wows_toolkit_config::queries::clear_settings(pool, kept).await,
        }
    }
}

/// What one save writes.
enum Value {
    /// A JSON value under its key in the settings table.
    Setting(String),
    ArmorDefaults(ArmorViewerDefaultsRow),
    /// Every setting but these back to its default.
    Clear(Vec<&'static str>),
}

/// Resolves once a write has committed or been dropped.
type Turn = Shared<oneshot::Receiver<()>>;

/// The order settings writes commit in.
struct Writes {
    /// The newest save of each key.
    latest: Mutex<HashMap<&'static str, u64>>,
    /// The turn of the write claimed last, which the next claim waits for.
    tail: Mutex<Turn>,
}

impl Writes {
    fn new() -> Self {
        let (done, first) = oneshot::channel();
        drop(done);

        Self { latest: Mutex::default(), tail: Mutex::new(first.shared()) }
    }

    fn next(&self, key: &'static str) -> u64 {
        let mut latest = self.latest.lock().expect("settings write order lock");
        let generation = latest.entry(key).or_insert(0);
        *generation += 1;
        *generation
    }

    fn is_latest(&self, key: &'static str, generation: u64) -> bool {
        self.latest.lock().expect("settings write order lock").get(key) == Some(&generation)
    }

    /// Joins the end of the order: the turn to wait for, and what to drop when done.
    fn enqueue(&self) -> (oneshot::Sender<()>, Turn) {
        let (done, turn) = oneshot::channel();
        let after = std::mem::replace(&mut *self.tail.lock().expect("settings write order lock"), turn.shared());

        (done, after)
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
            lighting: None,
        }
    }

    /// Two saves of the defaults row: only the newer is written, so the older can
    /// never land last.
    #[test]
    fn an_overtaken_armor_defaults_row_is_not_written() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime starts");
        let _entered = runtime.enter();
        let pool = SqlitePoolOptions::new().connect_lazy("sqlite::memory:").expect("an in-memory pool opens");
        let writer = SettingsWriter::new(pool);

        let older = writer.begin_armor_defaults(row(0.2));
        let setting = writer.begin("model_viewer_show_armor", "true".to_owned());
        let newer = writer.begin_armor_defaults(row(0.8));

        assert!(futures::executor::block_on(older.turn()).is_none(), "the older row is dropped");
        assert!(futures::executor::block_on(setting.turn()).is_some(), "a setting saved between them is written");
        assert!(futures::executor::block_on(newer.turn()).is_some(), "the newer row is written");
    }

    /// A save made before a clear commits before it, so the clear removes it; a
    /// save made after the clear waits for it, so the clear never removes it.
    #[test]
    fn a_clear_commits_between_the_saves_made_before_and_after_it() {
        const BEFORE: &str = "debug_mode";
        const AFTER: &str = "theme";

        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime starts");
        let pool = runtime.block_on(async {
            // One connection: each in-memory connection is a database of its own.
            let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.expect("opens");
            let tables = [
                "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
                "CREATE TABLE session_stats (id INTEGER)",
                "CREATE TABLE chart_configs (id INTEGER)",
                "CREATE TABLE armor_viewer_defaults (id INTEGER)",
                "CREATE TABLE render_options (id INTEGER)",
                "CREATE TABLE dock_layouts (id INTEGER)",
                "CREATE TABLE mod_manager (id INTEGER)",
            ];
            for table in tables {
                sqlx::query(table).execute(&pool).await.expect("the table is made");
            }
            pool
        });
        let writer = SettingsWriter::new(pool.clone());

        let before = writer.begin(BEFORE, "true".to_owned());
        let clear = writer.begin_clear(Vec::new());
        let after = writer.begin(AFTER, "\"Dark\"".to_owned());

        // Turned newest first, as tasks on a runtime may be: each still waits for those before it.
        let committed = runtime.block_on(async {
            let commit = |pending: super::PendingWrite| async move {
                let turned = pending.turn().await.expect("nothing overtakes it");
                turned.commit().await.expect("the write lands");
            };
            futures::join!(commit(after), commit(clear), commit(before));
            (
                wows_toolkit_config::queries::get_setting::<bool>(&pool, BEFORE).await,
                wows_toolkit_config::queries::get_setting::<String>(&pool, AFTER).await,
            )
        });

        assert_eq!(committed.0, None, "the save made before the clear is cleared");
        assert_eq!(committed.1.as_deref(), Some("Dark"), "the save made after the clear stays");
    }
}
