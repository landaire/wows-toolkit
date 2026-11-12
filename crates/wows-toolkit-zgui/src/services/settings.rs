//! The settings snapshot read from the shared config database, and the writing back of an edit.
//!
//! An edit is written to the same row the egui and GPUI apps read, so a setting changed in any of
//! them holds in all of them.

use std::sync::Arc;

use serde::Serialize;
use sqlx::sqlite::SqlitePool;
use tokio::runtime::Runtime;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_services::settings_writer::PendingWrite;
use wows_toolkit_services::settings_writer::SettingsWriter;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::spawn_detached;

use crate::services::on_tokio;

/// Where the settings read has got.
pub enum SettingsState {
    Loading,
    /// Boxed: the settings are far larger than the other variants.
    Loaded(Box<StartupSettings>),
    /// The database could not be opened or read; the reason is what the drawer says.
    Failed(String),
}

/// The settings, and where an edit to them is written.
#[derive(Clone, Copy)]
pub struct Settings {
    state: RwSignal<SettingsState, LocalStorage>,
    /// `None` until the startup read has opened the database.
    writer: StoredValue<Option<SettingsWriter>>,
    runtime: StoredValue<Option<Arc<Runtime>>>,
}

impl Settings {
    pub(super) fn new(runtime: StoredValue<Option<Arc<Runtime>>>) -> Self {
        Self { state: RwSignal::new_local(SettingsState::Loading), writer: StoredValue::new(None), runtime }
    }

    /// Takes the startup read, and the pool later edits are written through.
    pub fn adopt(self, settings: StartupSettings, pool: SqlitePool) {
        self.writer.set_value(Some(SettingsWriter::new(pool)));
        self.state.set(SettingsState::Loaded(Box::new(settings)));
    }

    /// Takes settings that have no database behind them, as a test does.
    #[cfg(test)]
    pub fn adopt_unsaved(self, settings: StartupSettings) {
        self.state.set(SettingsState::Loaded(Box::new(settings)));
    }

    pub fn fail(self, reason: String) {
        self.state.set(SettingsState::Failed(reason));
    }

    /// Reads the settings, subscribing to every change. `None` while they are not loaded.
    pub fn read<R>(self, read: impl FnOnce(&StartupSettings) -> R) -> Option<R> {
        self.state.with(|state| match state {
            SettingsState::Loaded(settings) => Some(read(settings)),
            SettingsState::Loading | SettingsState::Failed(_) => None,
        })
    }

    /// [`Self::read`] without subscribing.
    pub fn read_untracked<R>(self, read: impl FnOnce(&StartupSettings) -> R) -> Option<R> {
        self.state.with_untracked(|state| match state {
            SettingsState::Loaded(settings) => Some(read(settings)),
            SettingsState::Loading | SettingsState::Failed(_) => None,
        })
    }

    /// Whether the read has finished, and how.
    pub fn with_state<R>(self, read: impl FnOnce(&SettingsState) -> R) -> R {
        self.state.with(read)
    }

    /// Changes the loaded settings with `apply`, and saves what it returns under `key`. Nothing
    /// happens while the settings are not loaded, since there is nothing to change yet.
    pub fn edit<T: Serialize>(self, key: &'static str, apply: impl FnOnce(&mut StartupSettings) -> T) {
        let mut value = None;
        self.state.update(|state| {
            if let SettingsState::Loaded(settings) = state {
                value = Some(apply(settings));
            }
        });

        if let Some(value) = value {
            self.save(key, &value);
        }
    }

    /// Writes `value` under `key`.
    ///
    /// Fire and forget: the control has already moved, and a failed write is logged rather than
    /// shown, as the other front ends do. Saves of one key commit in the order they were made.
    pub fn save<T: Serialize>(self, key: &'static str, value: &T) {
        let json = match serde_json::to_string(value) {
            Ok(json) => json,
            Err(err) => {
                tracing::error!("settings: {key} could not be serialized: {err}");
                return;
            }
        };

        #[cfg(test)]
        SAVED.with(|saved| saved.borrow_mut().push((key, json.clone())));

        let Some(writer) = self.writer.get_value() else {
            tracing::warn!("settings: {key} was not saved because the config database is not open");
            return;
        };

        self.commit_detached(writer.begin(key, json));
    }

    /// Writes the armor viewer's defaults row, which is a table of its own rather than a key, and
    /// keeps the snapshot in step with it. Saves of the row commit in the order they were made.
    pub fn save_armor_defaults(self, row: ArmorViewerDefaultsRow) {
        self.state.update(|state| {
            if let SettingsState::Loaded(settings) = state {
                settings.armor_defaults = Some(row.clone());
            }
        });

        #[cfg(test)]
        SAVED.with(|saved| saved.borrow_mut().push((ARMOR_DEFAULTS, format!("{row:?}"))));

        let Some(writer) = self.writer.get_value() else {
            tracing::warn!("settings: the armor viewer defaults were not saved because the database is not open");
            return;
        };

        self.commit_detached(writer.begin_armor_defaults(row));
    }

    /// Commits `pending` once its turn comes, unless a newer save has overtaken it.
    ///
    /// Detached: a save must land even when the view that made it goes away first.
    fn commit_detached(self, pending: PendingWrite) {
        let runtime = self.runtime.get_value();
        spawn_detached(async move {
            let key = pending.key();
            let Some(write) = pending.turn().await else { return };
            match on_tokio(runtime, write.commit()).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::error!("settings: {key} could not be written: {err}"),
                Err(err) => tracing::error!("settings: the write of {key} did not complete: {err}"),
            }
        });
    }

    /// The open config pool, for the readers that query it directly. `None` before the startup
    /// read has opened it, and once the services are gone: a secondary window can outlive the main
    /// window that owns them.
    pub fn pool(self) -> Option<SqlitePool> {
        self.writer.try_get_value().flatten().map(|writer| writer.pool().clone())
    }
}

/// What [`saved_for_test`] names a write of the armor viewer defaults row.
#[cfg(test)]
pub const ARMOR_DEFAULTS: &str = "armor_viewer_defaults";

#[cfg(test)]
thread_local! {
    /// Every save asked for on this thread, as its key and its JSON: a test window has no database
    /// to read them back from.
    static SAVED: std::cell::RefCell<Vec<(&'static str, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Every save asked for on this thread so far, oldest first.
#[cfg(test)]
pub fn saved_for_test() -> Vec<(&'static str, String)> {
    SAVED.with(|saved| saved.borrow().clone())
}
