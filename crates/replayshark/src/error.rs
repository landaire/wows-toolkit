use std::path::PathBuf;

use wows_battle_world::process::ProcessError;
use wows_replays::context::GameDataContextError;
use wowsunpack::data::Version;

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

/// Why a replayshark command failed.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("failed to read replay {}", path.display())]
    ReadReplay { path: PathBuf },

    #[error("failed to parse replay {}", path.display())]
    ParseReplay { path: PathBuf },

    #[error("failed to analyze replay {}", path.display())]
    Analyze { path: PathBuf },

    #[error("failed to load game data for version {}", version.to_path())]
    GameData { version: Version },

    #[error("failed to write {}", path.display())]
    Write { path: PathBuf },

    #[error("failed to walk {}", path.display())]
    Walk { path: PathBuf },

    #[error("unknown bench mode: {mode}")]
    UnknownBenchMode { mode: String },

    #[error("query failed")]
    Query,

    #[error("battle-results command failed")]
    BattleResults,
}
