use std::path::PathBuf;

use wows_toolkit_viewmodel::unpacker::extract::ExtractError;
use wows_toolkit_viewmodel::unpacker::game_params::GameParamsDumpError;
use wows_toolkit_viewmodel::unpacker::viewer::ViewerError;

/// Why an Unpacker action could not finish.
#[derive(Debug, thiserror::Error)]
pub enum UnpackerError {
    #[error("could not read the setting {key:?}")]
    ReadSetting {
        key: &'static str,
        #[source]
        source: wows_toolkit_config::StartupSettingError,
    },
    #[error("could not start the settings database runtime")]
    Runtime {
        #[source]
        source: std::io::Error,
    },
    #[error("could not open the settings database")]
    OpenDatabase {
        #[source]
        source: sqlx::Error,
    },
    #[error("could not save the setting {key:?}")]
    SaveSetting {
        key: &'static str,
        #[source]
        source: sqlx::Error,
    },
    #[error("could not list the builds in {}", dir.display())]
    ListBuilds {
        dir: PathBuf,
        #[source]
        source: wowsunpack::error::GameDataError,
    },
    #[error("could not load build {build}: {reason}")]
    LoadBuild { build: u32, reason: String },
    #[error("the background task stopped before it finished")]
    Task {
        #[source]
        source: tokio::task::JoinError,
    },
    #[error("could not show the file dialog: {reason}")]
    Dialog { reason: String },
    #[error(transparent)]
    Viewer(#[from] ViewerError),
    #[error(transparent)]
    Extract(#[from] ExtractError),
    #[error(transparent)]
    GameParams(#[from] GameParamsDumpError),
    #[error("could not decode the parameters of build {build}")]
    DecodeParams {
        build: u32,
        #[source]
        source: wows_toolkit_replay_data::error::ReplayLoadError,
    },
    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
