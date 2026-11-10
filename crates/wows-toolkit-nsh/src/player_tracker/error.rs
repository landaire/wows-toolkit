use std::error::Error as _;

use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::queries::SettingError;

/// Why a tracker request did not complete.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("the database runtime could not start")]
    Runtime(#[source] std::io::Error),
    #[error("the shared database could not be opened")]
    Open(#[source] sqlx::Error),
    #[error("the replay index could not be read")]
    Index(#[source] IndexError),
    #[error("the tracked players could not be read")]
    LoadTracked(#[source] sqlx::Error),
    #[error("the players could not be written")]
    SaveTracked(#[source] sqlx::Error),
    #[error("the tracked players could not be cleared")]
    Clear(#[source] sqlx::Error),
    #[error("setting {key:?} could not be read")]
    ReadSetting {
        key: &'static str,
        #[source]
        source: SettingError,
    },
    #[error("setting {key:?} could not be written")]
    WriteSetting {
        key: &'static str,
        #[source]
        source: sqlx::Error,
    },
    #[error("the Current Match view settings could not be written")]
    WriteViewModes(#[source] sqlx::Error),
    #[error("{value:?} is not an account id")]
    AccountId { value: String },
    #[error("the game data settings could not be read")]
    GameData(#[source] crate::game_data::GameDataError),
    #[error("the tracker task did not complete")]
    Task(#[source] tokio::task::JoinError),
}

impl TrackerError {
    /// The error and every cause under it, as one line for the interface.
    pub fn report(&self) -> String {
        let mut text = self.to_string();
        let mut cause = self.source();
        while let Some(error) = cause {
            text.push_str(": ");
            text.push_str(&error.to_string());
            cause = error.source();
        }

        text
    }
}
