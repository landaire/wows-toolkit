use std::path::PathBuf;

use wows_battle_world::process::ProcessError;
use wows_replays::ParseError;
use wows_replays::context::GameDataContextError;
use wowsunpack::data::Version;

use crate::battle_results::ResultsFormat;

#[derive(Debug, thiserror::Error)]
pub enum AnalysisError {
    #[error("replay carries an unparsable client version {version:?}")]
    UnparsableVersion { version: String },

    #[error(transparent)]
    GameData(#[from] GameDataContextError),

    #[error(transparent)]
    Process(#[from] ProcessError),

    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error("failed to encode packet {index} as JSON")]
    Encode {
        index: usize,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum BattleResultsError {
    #[error("replay carries an unparsable client version {version:?}")]
    UnparsableVersion { version: String },

    #[error("failed to load game data: {0}")]
    GameData(#[from] GameDataContextError),

    #[error("failed to process replay: {0}")]
    Process(#[from] ProcessError),

    #[error("replay has no battle results")]
    NoBattleResults,

    #[error("failed to parse raw battle results: {0}")]
    RawResults(#[source] serde_json::Error),

    #[error("{format:?} battle results need the build's constants")]
    MissingConstants { format: ResultsFormat },

    #[error("failed to serialize normalized report: {0}")]
    Serialize(#[source] serde_json::Error),
}

/// Why a replayshark command failed.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("failed to read replay {}", path.display())]
    ReadReplay { path: PathBuf },

    #[error("failed to analyze replay {}", path.display())]
    Analyze { path: PathBuf },

    #[error("failed to load game data for version {}", version.to_path())]
    GameData { version: Version },

    #[error("failed to write {}", path.display())]
    Write { path: PathBuf },

    #[error("failed to walk the replay directories")]
    Walk,

    #[error("unknown bench mode: {mode}")]
    UnknownBenchMode { mode: String },

    #[error("one of --out-dir or --out-file is required")]
    MissingOutput,

    #[error("one of -g/--game or -e/--extracted is required")]
    MissingGameData,

    #[error("failed to read constants file {}", path.display())]
    ReadConstants { path: PathBuf },

    #[error("failed to read PR expected values file {}", path.display())]
    ReadPersonalRating { path: PathBuf },

    #[error("failed to initialize constants resolver")]
    ConstantsResolver,

    #[error("{failed} of {total} replays failed")]
    ReplaysFailed { failed: usize, total: usize },
}
