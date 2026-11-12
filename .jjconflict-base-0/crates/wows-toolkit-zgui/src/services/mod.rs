//! What the whole process shares: the tokio runtime, the settings, the game data, the running jobs
//! and the interface language.
//!
//! [`Services`] is provided once, by `shell::AppRoot`, and is a `Copy` handle: read it with
//! [`Services::current`] in a component's body and move it into closures.

mod error;
mod game_data;
mod jobs;
pub mod progress;
mod settings;

use std::sync::Arc;

use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;

use crate::i18n::Locale;

pub use crate::services::error::TokioError;
pub use crate::services::game_data::GameData;
pub use crate::services::game_data::GameDataRequest;
pub use crate::services::game_data::GameDataState;
pub use crate::services::jobs::Job;
pub use crate::services::jobs::Jobs;
pub use crate::services::jobs::Progress;
pub use crate::services::settings::Settings;
pub use crate::services::settings::SettingsState;
#[cfg(test)]
pub use crate::services::settings::saved_for_test;

/// The process-wide services, as one handle.
#[derive(Clone, Copy)]
pub struct Services {
    /// `None` when the runtime could not be built, which every network and database read then
    /// reports as [`TokioError::NoRuntime`].
    runtime: StoredValue<Option<Arc<Runtime>>>,
    pub settings: Settings,
    pub game_data: GameData,
    pub jobs: Jobs,
    pub locale: Locale,
}

impl Services {
    /// Makes the services and publishes them to the scopes below the calling one.
    pub fn provide(runtime: Option<Arc<Runtime>>) -> Self {
        let runtime = StoredValue::new(runtime);
        let services = Self {
            runtime,
            settings: Settings::new(runtime),
            game_data: GameData::new(),
            jobs: Jobs::new(),
            locale: Locale::new(),
        };
        provide_local_context(services);

        services
    }

    /// The services of the window the caller is in. Called from a component's body.
    ///
    /// # Panics
    ///
    /// Outside `shell::AppRoot`, which is a component placed where nothing can reach the process's
    /// state.
    pub fn current() -> Self {
        use_local_context::<Self>().expect("a component reads the services from inside AppRoot")
    }

    /// The shared runtime itself, for work that owns a long-lived task on it rather than one
    /// future, such as a collab session.
    pub fn runtime(self) -> Option<Arc<Runtime>> {
        self.runtime.get_value()
    }

    /// Runs `future` on the tokio runtime and resolves with its output wherever it is awaited.
    ///
    /// Dropping the returned future aborts `future`, so a task cancelled with its view stops the
    /// work it started.
    pub fn on_tokio<F>(self, future: F) -> impl Future<Output = Result<F::Output, TokioError>> + 'static
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        on_tokio(self.runtime.get_value(), future)
    }
}

/// Aborts the tokio task when the waiting side is dropped. A bare `JoinHandle` detaches instead.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// [`Services::on_tokio`] for a caller that holds the runtime rather than the services.
pub async fn on_tokio<F>(runtime: Option<Arc<Runtime>>, future: F) -> Result<F::Output, TokioError>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let runtime = runtime.ok_or(TokioError::NoRuntime)?;
    let mut running = AbortOnDrop(runtime.spawn(future));

    (&mut running.0).await.map_err(TokioError::Join)
}
