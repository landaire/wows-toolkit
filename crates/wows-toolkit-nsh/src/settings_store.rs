//! Reading and writing single rows of the shared settings database from the
//! background runtime. Writes go through one task, so two saves of the same
//! row land in the order they were made.

use std::sync::LazyLock;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::runtime;

static WRITER: LazyLock<Option<tokio::sync::mpsc::UnboundedSender<Write>>> = LazyLock::new(start_writer);

/// Why a setting could not be read.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Runtime(#[from] runtime::RuntimeError),
    #[error("could not open the settings database")]
    Open(#[source] sqlx::Error),
    #[error("could not read setting {key:?}")]
    Read {
        key: &'static str,
        #[source]
        source: wows_toolkit_config::queries::SettingError,
    },
    #[error("the read of setting {key:?} did not complete")]
    Task {
        key: &'static str,
        #[source]
        source: tokio::task::JoinError,
    },
}

struct Write {
    key: &'static str,
    json: String,
}

/// Reads `key`. `Ok(None)` is a row never written.
pub async fn load<T>(key: &'static str) -> Result<Option<T>, SettingsError>
where
    T: DeserializeOwned + Send + 'static,
{
    let runtime = runtime::shared()?;
    let task = runtime.spawn(async move {
        let pool = wows_toolkit_config::open_db().await.map_err(SettingsError::Open)?;
        wows_toolkit_config::queries::try_get_setting::<T>(&pool, key)
            .await
            .map_err(|source| SettingsError::Read { key, source })
    });

    match task.await {
        Ok(read) => read,
        Err(source) => Err(SettingsError::Task { key, source }),
    }
}

/// Queues `value` to be written under `key`. A failed write is logged: the
/// value is still in use for this run.
pub fn save<T: Serialize>(key: &'static str, value: &T) {
    let json = match serde_json::to_string(value) {
        Ok(json) => json,
        Err(error) => {
            log::error!("settings: {key} could not be serialized: {error}");
            return;
        }
    };
    let Some(writer) = WRITER.as_ref() else {
        log::error!("settings: {key} was not saved: the background runtime is not available");
        return;
    };
    if writer.send(Write { key, json }).is_err() {
        log::error!("settings: {key} was not saved: the settings writer has stopped");
    }
}

fn start_writer() -> Option<tokio::sync::mpsc::UnboundedSender<Write>> {
    let runtime = match runtime::shared() {
        Ok(runtime) => runtime,
        Err(error) => {
            log::error!("settings: {error}");
            return None;
        }
    };
    let (sender, mut writes) = tokio::sync::mpsc::unbounded_channel::<Write>();
    runtime.spawn(async move {
        let mut pool: Option<sqlx::SqlitePool> = None;
        while let Some(write) = writes.recv().await {
            if pool.is_none() {
                match wows_toolkit_config::open_db().await {
                    Ok(opened) => pool = Some(opened),
                    Err(error) => {
                        log::error!("settings: {} was not saved: {error}", write.key);
                        continue;
                    }
                }
            }
            let Some(pool) = pool.as_ref() else {
                continue;
            };
            if let Err(error) = wows_toolkit_config::queries::set_setting_raw(pool, write.key, &write.json).await {
                log::error!("settings: {} was not saved: {error}", write.key);
            }
        }
    });

    Some(sender)
}
