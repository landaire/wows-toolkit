//! The watchdog over the process's long-lived worker threads, and the replay loader it watches.

use wows_toolkit_replay_data::loader::ReplayLoader;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;

#[derive(Clone, Copy)]
pub struct Supervision {
    /// `None` when its thread could not be started, and then nothing is watched.
    watchdog: StoredValue<Option<Watchdog>>,
    /// `None` when its thread could not be started; a replay opened then fails to read.
    replay_loader: StoredValue<Option<ReplayLoader>>,
}

impl Supervision {
    pub fn new() -> Self {
        let watchdog = Watchdog::start()
            .inspect_err(|err| tracing::error!(%err, "the watchdog did not start; worker threads are not watched"))
            .ok();
        let replay_loader = watchdog.as_ref().and_then(|watchdog| {
            ReplayLoader::start(watchdog)
                .inspect_err(|err| tracing::error!(%err, "the replay loader did not start"))
                .ok()
        });

        Self { watchdog: StoredValue::new(watchdog), replay_loader: StoredValue::new(replay_loader) }
    }

    pub fn watchdog(self) -> Option<Watchdog> {
        self.watchdog.get_value()
    }

    pub fn replay_loader(self) -> Option<ReplayLoader> {
        self.replay_loader.get_value()
    }
}

impl Default for Supervision {
    fn default() -> Self {
        Self::new()
    }
}
