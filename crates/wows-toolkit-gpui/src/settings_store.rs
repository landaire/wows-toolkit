//! Writing settings back to the shared config database.
//!
//! The startup read closes over its own pool; persisting an edit needs one
//! that outlives it, so the pool is held here for the life of the process and
//! writes are submitted to the tokio runtime like any other sqlx work.

use gpui_kit::AsyncApp;
use gpui_kit::{App, Global};
use serde::Serialize;
use sqlx::sqlite::SqlitePool;

use crate::runtime;

/// Holds the pool settings edits are written through.
struct SettingsPool(SqlitePool);

impl Global for SettingsPool {}

/// Installs the pool. Called once, after the startup read has opened it.
pub fn init(pool: SqlitePool, cx: &mut App) {
    cx.set_global(SettingsPool(pool));
}

/// Persists one setting.
///
/// Fire and forget: the control has already moved, and a failed write is
/// logged rather than surfaced, matching the egui app, whose save task also
/// reports failures to the log rather than to the settings tab. The value is
/// serialized on the caller's thread so the future owns no borrow.
pub fn save<T: Serialize>(key: &'static str, value: &T, cx: &mut App) {
    let Some(pool) = cx.try_global::<SettingsPool>().map(|held| held.0.clone()) else {
        tracing::warn!("settings: {key} was not saved because the config database is not open");
        return;
    };

    let json = match serde_json::to_string(value) {
        Ok(json) => json,
        Err(err) => {
            tracing::error!("settings: {key} could not be serialized: {err}");
            return;
        }
    };

    cx.spawn(async move |cx: &mut AsyncApp| {
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
