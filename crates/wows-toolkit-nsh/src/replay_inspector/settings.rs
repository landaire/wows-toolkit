//! The shared settings the inspector reads, and the rows only it writes: the
//! player table's column widths and the last replay opened by hand. The
//! shell's `Settings` service writes the rest. Every function here blocks on
//! the settings database; callers run them in `blocking`.

use std::path::Path;
use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::SqlitePool;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::keys;

use crate::replay_inspector::error::InspectorError;

/// Dragged player-table column widths, one entry per `ReplayColumn::ALL`
/// index; `None` keeps the content-fit width. Shared with the GPUI app.
pub const COLUMN_WIDTHS_KEY: &str = "replay_column_widths_v2";

/// The settings the inspector acts on.
#[derive(Clone)]
pub struct InspectorSettings {
    /// `None` while the World of Warships directory is unset.
    pub wows_dir: Option<PathBuf>,
    pub debug_mode: bool,
    pub auto_load_latest_replay: bool,
    pub data_sharing: DataSharingMode,
    pub proxy_url: String,
    pub replay: ReplaySettings,
}

/// Reads the inspector's settings.
pub fn load() -> Result<InspectorSettings, InspectorError> {
    with_pool(|pool| async move {
        let mode = optional::<DataSharingMode>(&pool, keys::DATA_SHARING_MODE).await?;
        let compatibility = optional::<bool>(&pool, keys::SEND_REPLAY_DATA).await?;
        let data_sharing = match (mode, compatibility) {
            (Some(mode), Some(shared)) => mode.reconcile_with_compat_bool(shared),
            (Some(mode), None) => mode,
            (None, Some(shared)) => DataSharingMode::from_send_replay_data_bool(shared),
            (None, None) => DataSharingMode::default(),
        };
        let wows_dir = optional::<String>(&pool, keys::WOWS_DIR)
            .await?
            .filter(|dir| !dir.trim().is_empty())
            .map(PathBuf::from);

        // An absent row is a fresh install; these are the same defaults the
        // Settings tab shows for it.
        Ok(InspectorSettings {
            wows_dir,
            debug_mode: optional(&pool, keys::DEBUG_MODE).await?.unwrap_or(false),
            auto_load_latest_replay: optional(&pool, keys::AUTO_LOAD_LATEST_REPLAY).await?.unwrap_or(true),
            data_sharing,
            proxy_url: optional::<Option<String>>(&pool, keys::PROXY_URL).await?.flatten().unwrap_or_default(),
            replay: optional(&pool, keys::REPLAY_SETTINGS).await?.unwrap_or_default(),
        })
    })
}

/// Remembers the replay the user opened by hand.
pub fn save_current_replay_path(path: &Path) -> Result<(), InspectorError> {
    save(keys::CURRENT_REPLAY_PATH, &path.display().to_string())
}

/// The dragged column widths, one per `ReplayColumn::ALL` index. Empty when
/// none were ever saved.
pub fn load_column_widths() -> Result<Vec<Option<f32>>, InspectorError> {
    with_pool(|pool| async move { Ok(optional(&pool, COLUMN_WIDTHS_KEY).await?.unwrap_or_default()) })
}

pub fn save_column_widths(widths: &[Option<f32>]) -> Result<(), InspectorError> {
    save(COLUMN_WIDTHS_KEY, &widths)
}

/// Runs `work` against the settings database on a runtime of its own: the
/// caller is on a `blocking` thread.
pub fn with_pool<T, F, Fut>(work: F) -> Result<T, InspectorError>
where
    F: FnOnce(SqlitePool) -> Fut,
    Fut: Future<Output = Result<T, InspectorError>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| InspectorError::Runtime { source })?;

    runtime.block_on(async move {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| InspectorError::Database { source })?;
        work(pool).await
    })
}

fn save<T: Serialize>(key: &'static str, value: &T) -> Result<(), InspectorError> {
    with_pool(|pool| async move {
        wows_toolkit_config::queries::set_setting(&pool, key, value)
            .await
            .map_err(|source| InspectorError::SettingWrite { key, source })
    })
}

async fn optional<T: DeserializeOwned>(pool: &SqlitePool, key: &'static str) -> Result<Option<T>, InspectorError> {
    wows_toolkit_config::queries::try_get_setting(pool, key)
        .await
        .map_err(|source| InspectorError::SettingRead { key, source })
}
