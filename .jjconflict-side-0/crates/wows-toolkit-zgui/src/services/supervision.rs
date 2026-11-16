//! The watchdog over the process's long-lived worker threads, and the workers it watches: the
//! replay loader and the constants worker.

use wows_toolkit_replay_data::loader::ReplayLoader;
use wows_toolkit_services::supervision::watchdog::Watchdog;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;

use crate::services::constants::ConstantsWorker;

#[derive(Clone, Copy)]
pub struct Supervision {
    /// `None` when its thread could not be started, and then nothing is watched.
    watchdog: StoredValue<Option<Watchdog>>,
    /// `None` when its thread could not be started; a replay opened then fails to read.
    replay_loader: StoredValue<Option<ReplayLoader>>,
    /// `None` when its thread could not be started; constants are then only read from the cache.
    constants_worker: StoredValue<Option<ConstantsWorker>>,
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
        let constants_worker = watchdog.as_ref().and_then(|watchdog| {
            ConstantsWorker::start(watchdog)
                .inspect_err(|err| tracing::error!(%err, "the constants worker did not start"))
                .ok()
        });

        Self {
            watchdog: StoredValue::new(watchdog),
            replay_loader: StoredValue::new(replay_loader),
            constants_worker: StoredValue::new(constants_worker),
        }
    }

    pub fn watchdog(self) -> Option<Watchdog> {
        self.watchdog.get_value()
    }

    pub fn replay_loader(self) -> Option<ReplayLoader> {
        self.replay_loader.get_value()
    }

    pub fn constants_worker(self) -> Option<ConstantsWorker> {
        self.constants_worker.get_value()
    }
}

impl Default for Supervision {
    fn default() -> Self {
        Self::new()
    }
}
