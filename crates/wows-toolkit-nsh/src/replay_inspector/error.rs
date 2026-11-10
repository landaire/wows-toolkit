use std::path::PathBuf;

use wows_toolkit_replay_data::error::ReplayLoadError;

use crate::replay_inspector::document::ReplayDocId;

/// Why an inspector operation failed.
#[derive(Debug, thiserror::Error)]
pub enum InspectorError {
    #[error("the settings database runtime could not start: {source}")]
    Runtime {
        #[source]
        source: std::io::Error,
    },
    #[error("the settings database could not be opened: {source}")]
    Database {
        #[source]
        source: sqlx::Error,
    },
    #[error("setting {key:?} could not be read: {source}")]
    SettingRead {
        key: &'static str,
        #[source]
        source: wows_toolkit_config::queries::SettingError,
    },
    #[error("setting {key:?} could not be written: {source}")]
    SettingWrite {
        key: &'static str,
        #[source]
        source: sqlx::Error,
    },
    #[error("a background task failed: {source}")]
    Task {
        #[source]
        source: tokio::task::JoinError,
    },
    #[error(transparent)]
    Load(#[from] ReplayLoadError),
    #[error(transparent)]
    GameData(#[from] crate::game_data::GameDataError),
    #[error("no open replay has id {id}")]
    UnknownDocument { id: ReplayDocId },
    #[error("{} could not be read: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{} could not be written: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Export(#[from] wows_toolkit_replay_data::export::ExportError),
    #[error("the file dialog could not be shown: {reason}")]
    Dialog { reason: String },
    #[error("{}", rust_i18n::t!("ui.replay.load_alt_perspective_limit", limit = limit))]
    AltLimit { limit: usize },
    #[error("{}", rust_i18n::t!("ui.replay.load_alt_perspective_failed", error = source))]
    AltRefused {
        #[source]
        source: ReplayLoadError,
    },
    #[error("{}", rust_i18n::t!("ui.replay.constants_no_build"))]
    NoBuildLoaded,
    #[error("{}", rust_i18n::t!("ui.replay.constants_import_failed", reason = reason))]
    ConstantsImport { reason: String },
    #[error("{}", rust_i18n::t!("ui.replay.constants_check_failed", reason = reason))]
    ConstantsCheck { reason: String },
    #[error(transparent)]
    Settings(#[from] crate::shell::SettingsError),
    #[error("the replay settings have not been read yet")]
    ListingNotRead,
    #[error("the replay index summaries could not be read: {source}")]
    IndexSummaries {
        #[source]
        source: wows_toolkit_config::index::rows::IndexError,
    },
    #[error("the session stats could not be written: {source}")]
    SessionStats {
        #[source]
        source: sqlx::Error,
    },
    #[error("World of Warships directory is not set")]
    WowsDirMissing,
    #[error("{source}")]
    Launch {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
