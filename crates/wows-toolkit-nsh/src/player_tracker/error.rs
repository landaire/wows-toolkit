use std::error::Error as _;
use std::fmt;

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
    /// The error and every cause under it, displayed as one line.
    pub fn chain(&self) -> ErrorChain<'_> {
        ErrorChain(self)
    }
}

/// Displays a [`TrackerError`] followed by each of its causes, joined by
/// `": "`, as the interface shows it.
pub struct ErrorChain<'a>(&'a TrackerError);

impl fmt::Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut cause = self.0.source();
        while let Some(error) = cause {
            write!(f, ": {error}")?;
            cause = error.source();
        }

        Ok(())
    }
}

/// A tracker request's failure as the TSX view receives it: the error and
/// every cause under it.
#[derive(Debug)]
pub struct TrackerReport(TrackerError);

impl From<TrackerError> for TrackerReport {
    fn from(error: TrackerError) -> Self {
        Self(error)
    }
}

impl fmt::Display for TrackerReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.chain(), f)
    }
}
