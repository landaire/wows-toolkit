use wows_battle_world::process::ProcessError;
use wows_replays::context::GameDataContextError;

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("replay carries an unparsable client version {version:?}")]
    UnparsableVersion { version: String },

    #[error(transparent)]
    GameData(#[from] GameDataContextError),

    #[error(transparent)]
    Process(#[from] ProcessError),

    #[error("failed to encode packet {index} as JSON")]
    Encode {
        index: usize,
        #[source]
        source: serde_json::Error,
    },
}
