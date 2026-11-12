//! Writing settings back to the shared config database.
//!
//! Every save carries the whole value, so the newest save of a key is the only
//! one that matters. Writes run one at a time, and a write that a newer save of
//! the same key has overtaken is dropped: two writes committing out of order
//! would otherwise leave the older value in the row.

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::sqlite::SqlitePool;

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
        let generation = self.writes.next(key);

        PendingWrite { writer: self.clone(), key, json, generation }
    }
}

/// One save, waiting for its turn.
pub struct PendingWrite {
    writer: SettingsWriter,
    key: &'static str,
    json: String,
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

        wows_toolkit_config::queries::set_setting_raw(&pending.writer.pool, pending.key, &pending.json).await
    }
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
