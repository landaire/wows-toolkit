//! Writing settings back to the shared config database.
//!
//! The startup read closes over its own pool; persisting an edit needs one
//! that outlives it, so the pool is held here for the life of the process and
//! writes are submitted to the tokio runtime like any other sqlx work.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::App;
use gpui_kit::AsyncApp;
use gpui_kit::Global;
use serde::Serialize;
use sqlx::sqlite::SqlitePool;

use crate::runtime;

/// Holds the pool settings edits are written through, and the order they
/// commit in.
struct SettingsPool(SqlitePool, Arc<Writes>);

impl Global for SettingsPool {}

/// Installs the pool. Called once, after the startup read has opened it.
pub fn init(pool: SqlitePool, cx: &mut App) {
    cx.set_global(SettingsPool(pool, Arc::default()));
}

/// The open config pool, for the tabs that query it directly.
///
/// `None` before the startup read has installed it, which is a real state:
/// the tabs render an appropriate status rather than assuming a database.
pub fn pool(cx: &App) -> Option<SqlitePool> {
    cx.try_global::<SettingsPool>().map(|held| held.0.clone())
}

/// Persists one setting.
///
/// Fire and forget: the control has already moved, and a failed write is
/// logged rather than surfaced, matching the egui app, whose save task also
/// reports failures to the log rather than to the settings tab. The value is
/// serialized on the caller's thread so the future owns no borrow.
///
/// Writes run one at a time, and a write that a newer save of the same key has
/// overtaken is dropped: every save carries the whole value, so the newest one
/// is the only one that matters, and two writes committing out of order would
/// otherwise leave the older value in the row.
pub fn save<T: Serialize>(key: &'static str, value: &T, cx: &mut App) {
    let json = match serde_json::to_string(value) {
        Ok(json) => json,
        Err(err) => {
            tracing::error!("settings: {key} could not be serialized: {err}");
            return;
        }
    };
    #[cfg(test)]
    SAVED.with(|saved| saved.borrow_mut().push((key, json.clone())));

    let Some(held) = cx.try_global::<SettingsPool>() else {
        tracing::warn!("settings: {key} was not saved because the config database is not open");
        return;
    };
    let pool = held.0.clone();
    let writes = held.1.clone();
    let generation = writes.next(key);

    cx.spawn(async move |cx: &mut AsyncApp| {
        let _order = writes.order.lock().await;
        if !writes.is_latest(key, generation) {
            return;
        }
        let written =
            runtime::spawn(cx, async move { wows_toolkit_config::queries::set_setting_raw(&pool, key, &json).await })
                .await;

        match written {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::error!("settings: {key} could not be written: {err}"),
            Err(err) => tracing::error!("settings: the write of {key} did not complete: {err}"),
        }
    })
    .detach();
}

/// The order settings writes commit in.
#[derive(Default)]
struct Writes {
    /// The newest save of each key.
    latest: std::sync::Mutex<HashMap<&'static str, u64>>,
    /// Held for the length of one write.
    order: futures::lock::Mutex<()>,
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
thread_local! {
    /// Every save asked for on this thread, as its key and its JSON: a test
    /// window has no database to read them back from.
    static SAVED: std::cell::RefCell<Vec<(&'static str, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Every save asked for on this thread so far, oldest first.
#[cfg(test)]
pub(crate) fn saved_for_test() -> Vec<(&'static str, String)> {
    SAVED.with(|saved| saved.borrow().clone())
}
