//! The settings snapshot read from the shared config database, and the writing back of an edit.
//!
//! An edit is written to the same row the egui app reads, so a setting changed in any of
//! them holds in all of them.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sqlx::sqlite::SqlitePool;
use tokio::runtime::Runtime;
use tokio_util::task::TaskTracker;
use wows_core::pseudonym::NameDisplay;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_services::settings_writer::PendingWrite;
use wows_toolkit_services::settings_writer::SettingsWriter;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::task::spawn_detached;

use crate::services::TokioError;

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
    /// The writes this window's services started and that have not committed yet.
    writes: StoredValue<TaskTracker>,
}

impl Settings {
    pub(super) fn new(runtime: StoredValue<Option<Arc<Runtime>>>) -> Self {
        let writes = TaskTracker::new();
        // The services can go without the app quitting, as a test reopening the shell does, and
        // the runtime goes with them, cancelling any write still running. A shell opened next would
        // read the settings without it.
        if let Some(runtime) = runtime.get_value() {
            let writes = writes.clone();
            on_cleanup_local(move || wait_for_writes(&runtime, &writes, WRITE_GRACE));
        }
        Self {
            state: RwSignal::new_local(SettingsState::Loading),
            writer: StoredValue::new(None),
            runtime,
            writes: StoredValue::new(writes),
        }
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

    /// Writes this window's saves to an empty database of its own: a test that changes a setting
    /// or the session must not change what other windows in the process read.
    #[cfg(test)]
    pub fn use_private_database_for_test(self) {
        let runtime = self.runtime.get_value().expect("the test runtime is up");
        let pool = runtime.block_on(wows_toolkit_config::test_pool());
        self.writer.set_value(Some(SettingsWriter::new(pool)));
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

    /// Whether the read is still running, without subscribing.
    pub fn is_loading_untracked(self) -> bool {
        self.state.with_untracked(|state| matches!(state, SettingsState::Loading))
    }

    /// How player names and clan tags are shown, subscribing to changes. Real names until the
    /// settings are loaded, which is the default the setting itself has.
    pub fn name_display(self) -> NameDisplay {
        self.read(|settings| NameDisplay::from_anonymize(settings.replay.anonymize_player_names))
            .unwrap_or(NameDisplay::Real)
    }

    /// [`Self::name_display`] without subscribing.
    pub fn name_display_untracked(self) -> NameDisplay {
        self.read_untracked(|settings| NameDisplay::from_anonymize(settings.replay.anonymize_player_names))
            .unwrap_or(NameDisplay::Real)
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
        self.save_reported(key, value, |_| {});
    }

    /// [`Self::save`] for a caller that shows a failed write; the failure is logged either way.
    /// `report` gets how the write ended, unless a newer save of `key` has been made by then:
    /// that save's result is the one that holds.
    pub fn save_reported<T: Serialize>(
        self,
        key: &'static str,
        value: &T,
        report: impl FnOnce(Result<(), SaveError>) + 'static,
    ) {
        let json = match serde_json::to_string(value) {
            Ok(json) => json,
            Err(err) => return report(Err(logged(key, SaveError::Serialize(err)))),
        };

        #[cfg(test)]
        SAVED.with(|saved| saved.borrow_mut().push((key, json.clone())));

        let Some(writer) = self.writer.get_value() else {
            return report(Err(logged(key, SaveError::DatabaseClosed)));
        };

        self.commit_detached(key, writer.begin(key, json), report);
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

        self.commit_detached("the armor viewer defaults", writer.begin_armor_defaults(row), |_| {});
    }

    /// Puts the persisted settings back to their defaults, as the egui crash window's Clear
    /// Settings does, and reads them again.
    ///
    /// In the write order, so a save made before it never lands after it, and waited for at quit as
    /// a save is. A failure is logged, as a failed save is.
    pub fn clear(self) {
        #[cfg(test)]
        SAVED.with(|saved| saved.borrow_mut().push((CLEARED, String::new())));

        let Some(writer) = self.writer.get_value() else {
            tracing::warn!("settings: nothing was cleared because the config database is not open");
            return;
        };
        let Some(runtime) = self.runtime.get_value() else {
            tracing::error!("settings: nothing was cleared because no tokio runtime is running");
            return;
        };

        // The tracker blob is tracked players not yet imported, not a setting.
        let pending = writer.begin_clear(vec![wows_toolkit_viewmodel::player_tracker::tracked::SETTING_KEY]);
        let pool = writer.pool().clone();
        let (reloaded, read) = futures::channel::oneshot::channel();
        runtime.spawn(self.tracked(async move {
            let Some(write) = pending.turn().await else { return };
            if let Err(err) = write.commit().await {
                tracing::error!("settings: the clear could not be written: {err}");
                return;
            }
            // Read before the next write commits, so what is shown is the cleared state.
            let settings = StartupSettings::load(&pool).await;
            drop(write);
            let _ = reloaded.send(settings);
        }));
        spawn_detached(async move {
            let Ok(settings) = read.await else { return };
            // `try_`, as the shell may be gone by the time the clear lands.
            self.state.try_set(SettingsState::Loaded(Box::new(settings)));
        });
    }

    /// Commits `pending` once its turn comes, unless a newer save has overtaken it, logs a failure
    /// as `what`, and tells `report` how the write ended while it is still the newest save.
    ///
    /// On the tokio runtime rather than the UI executor, and tracked: a save must land even when
    /// the view that made it goes away first, and when the app quits straight after it
    /// ([`finish_pending_writes`]).
    fn commit_detached(
        self,
        what: &'static str,
        pending: PendingWrite,
        report: impl FnOnce(Result<(), SaveError>) + 'static,
    ) {
        let Some(runtime) = self.runtime.get_value() else {
            return report(Err(logged(what, SaveError::Incomplete(TokioError::NoRuntime))));
        };
        let stamp = pending.stamp();
        let written = runtime.spawn(self.tracked(async move {
            let write = pending.turn().await?;
            // Logged here: after the window loop returns, the UI task below never runs again.
            Some(write.commit().await.map_err(|err| logged(what, SaveError::Write(err))))
        }));
        spawn_detached(async move {
            let result = match written.await {
                Ok(None) => return,
                Ok(Some(committed)) => committed,
                Err(err) => Err(logged(what, SaveError::Incomplete(TokioError::Join(err)))),
            };
            // The write order is released on the tokio runtime, so a newer save of the key can
            // finish and report before this task runs again.
            if stamp.is_newest() {
                report(result);
            }
        });
    }

    /// `write` counted among this window's writes and the process's, so both the services going
    /// and the app quitting wait for it.
    fn tracked<F: Future>(self, write: F) -> impl Future<Output = F::Output> {
        WRITES_IN_FLIGHT.track_future(self.writes.get_value().track_future(write))
    }

    /// The open config pool, for the readers that query it directly. `None` before the startup
    /// read has opened it, and once the services are gone: a secondary window can outlive the main
    /// window that owns them.
    pub fn pool(self) -> Option<SqlitePool> {
        self.writer.try_get_value().flatten().map(|writer| writer.pool().clone())
    }
}

/// How long the app's quit, or the services going, waits for the writes still running. The egui
/// app's final save waits as long.
pub const WRITE_GRACE: Duration = Duration::from_secs(5);

/// The settings writes not yet committed. Process-wide: the services that start them are gone by
/// the time the window loop returns, and the writes have to outlive them.
static WRITES_IN_FLIGHT: std::sync::LazyLock<TaskTracker> = std::sync::LazyLock::new(TaskTracker::new);

/// Waits up to `timeout` for every settings write already started to commit. Called once the
/// window loop has returned, before the runtime is dropped and aborts them.
pub fn finish_pending_writes(runtime: &Runtime, timeout: Duration) {
    wait_for_writes(runtime, &WRITES_IN_FLIGHT, timeout);
}

/// Waits up to `timeout` for the writes `writes` tracks, taking no more.
fn wait_for_writes(runtime: &Runtime, writes: &TaskTracker, timeout: Duration) {
    writes.close();
    if writes.is_empty() {
        return;
    }
    // A thread already inside a runtime cannot block on another; the quit still waits.
    if tokio::runtime::Handle::try_current().is_ok() {
        tracing::warn!("settings: {} writes were not waited for inside a tokio runtime", writes.len());
        return;
    }
    // Built inside the runtime: a tokio timer made outside one panics.
    if runtime.block_on(async { tokio::time::timeout(timeout, writes.wait()).await }).is_err() {
        tracing::warn!("settings: {} writes were still pending and are lost", writes.len());
    }
}

/// Why a settings write did not land.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("the value could not be serialized: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("the config database is not open")]
    DatabaseClosed,
    #[error("{0}")]
    Write(#[source] sqlx::Error),
    #[error("the write did not complete: {0}")]
    Incomplete(#[source] TokioError),
}

fn logged(what: &str, err: SaveError) -> SaveError {
    tracing::warn!("settings: {what} was not saved: {err}");
    err
}

/// What [`saved_for_test`] names a write of the armor viewer defaults row.
#[cfg(test)]
pub const ARMOR_DEFAULTS: &str = "armor_viewer_defaults";

/// What [`saved_for_test`] names a clear of every setting.
#[cfg(test)]
pub const CLEARED: &str = "cleared";

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use wows_toolkit_services::settings::StartupSettings;
    use zgui::reactive::Mounted;
    use zgui::reactive::prelude::GetValue;

    use super::finish_pending_writes;
    use crate::services::Services;

    /// A setting changed just before the app quits is in the database once the exit has waited
    /// for it, though the window and its executor are already gone.
    #[test]
    fn a_write_made_just_before_exit_lands() {
        const KEY: &str = "zgui_test_written_at_exit";

        zgui::reactive::install().expect("the reactive runtime installs");
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        let main = Mounted::new();
        let services = main.with(|| Services::provide(Some(Arc::clone(&runtime))));
        let pool = runtime.block_on(wows_toolkit_config::test_pool());
        let settings = runtime.block_on(StartupSettings::load(&pool));
        services.settings.adopt(settings, pool.clone());

        services.settings.save(KEY, &"quit");
        main.unmount();
        finish_pending_writes(&runtime, Duration::from_secs(5));

        let stored = runtime.block_on(wows_toolkit_config::queries::get_setting::<String>(&pool, KEY));
        assert_eq!(stored.as_deref(), Some("quit"));
    }

    /// A window closed without the app quitting, as a test reopening the shell does, has finished
    /// its writes by the time it is gone, so the next window reads them. The write here is still
    /// queued behind an earlier one as the window goes, as on a busy machine.
    #[test]
    fn a_write_queued_as_the_services_go_lands() {
        const EARLIER: &str = "zgui_test_written_first";
        const KEY: &str = "zgui_test_written_as_the_services_go";

        zgui::reactive::install().expect("the reactive runtime installs");
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        let main = Mounted::new();
        let services = main.with(|| Services::provide(Some(Arc::clone(&runtime))));
        let pool = runtime.block_on(wows_toolkit_config::test_pool());
        let settings = runtime.block_on(StartupSettings::load(&pool));
        services.settings.adopt(settings, pool.clone());

        let writer = services.settings.writer.get_value().expect("the settings are adopted");
        let earlier = runtime
            .block_on(writer.begin(EARLIER, "\"first\"".to_owned()).turn())
            .expect("nothing is ahead of the earlier write");
        services.settings.save(KEY, &"closed");
        let releasing = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(earlier);
        });
        main.unmount();
        releasing.join().expect("the earlier write is released");

        let stored = runtime.block_on(wows_toolkit_config::queries::get_setting::<String>(&pool, KEY));
        assert_eq!(stored.as_deref(), Some("closed"));
    }

    /// A save made just before Clear Settings is cleared rather than landing after it, one made just
    /// after survives it, and the clear is waited for at quit.
    #[test]
    fn a_clear_lands_between_the_saves_around_it() {
        const BEFORE: &str = "zgui_test_saved_before_clear";
        const AFTER: &str = "zgui_test_saved_after_clear";

        zgui::reactive::install().expect("the reactive runtime installs");
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        let main = Mounted::new();
        let services = main.with(|| Services::provide(Some(Arc::clone(&runtime))));
        let pool = runtime.block_on(wows_toolkit_config::test_pool());
        let settings = runtime.block_on(StartupSettings::load(&pool));
        services.settings.adopt(settings, pool.clone());

        services.settings.save(BEFORE, &"before");
        services.settings.clear();
        services.settings.save(AFTER, &"after");
        main.unmount();
        finish_pending_writes(&runtime, Duration::from_secs(5));

        let read = |key| runtime.block_on(wows_toolkit_config::queries::get_setting::<String>(&pool, key));
        assert_eq!(read(BEFORE), None, "the earlier save is cleared");
        assert_eq!(read(AFTER).as_deref(), Some("after"), "the later save stays");
    }
}
