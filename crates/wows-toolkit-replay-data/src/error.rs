use std::path::PathBuf;
use std::sync::Arc;

use rootcause::Report;
use wows_battle_world::merged::MergeError;
use wows_replays::ParseError;

/// Reasons a replay could not become a [`ReplayReportModel`](crate::model::ReplayReportModel).
///
/// `Clone` because one build's load outcome is shared by every waiter on it,
/// so the sources are held behind `Arc`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReplayLoadError {
    /// Reading the replay file off disk failed (not found, permissions, truncated).
    #[error("failed to read replay file: {0:?}")]
    Io(Arc<Report<ParseError>>),
    /// The replay's `clientVersionFromExe` did not parse into a build number.
    #[error("could not parse the replay's client version")]
    VersionParse,
    /// Neither the install nor the game-data cache has the replay's build.
    #[error("replay build {build} is not installed (no bin/{build} directory found)")]
    UnsupportedVersion { build: u32 },
    /// The game VFS, `GameParams`, or entity scripts could not be loaded.
    #[error("failed to load game data: {0}")]
    GameData(#[from] GameDataLoadError),
    /// The replay file did not parse.
    #[error("failed to parse replay: {0:?}")]
    Parse(Arc<Report<ParseError>>),
    /// A recording read only to check that it belongs to a battle did not
    /// parse. Reported by its error alone: the full report is for the log.
    #[error("failed to parse replay: {}", .0.current_context())]
    CheckUnreadable(Arc<Report<ParseError>>),
    /// The replay's header block did not parse.
    #[error("failed to parse replay: {0}")]
    Header(Arc<Report<ParseError>>),
    /// The recordings of one battle could not be walked together.
    #[error("failed to parse replay: {0}")]
    Merge(Arc<MergeError>),
    /// Another recording was offered as this battle and is not it.
    #[error("{0}")]
    AltRefused(#[from] AltRefusal),
}

/// Why a build's game data could not be loaded.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GameDataLoadError {
    #[error("{0}")]
    Vfs(Arc<Report>),
    #[error("{0}")]
    Params(Arc<wowsunpack::error::GameDataError>),
    #[error("no metadata.toml in the dump at {}", dump_dir.display())]
    NoDumpMetadata { dump_dir: PathBuf },
    #[error("game data load panicked: {message}")]
    Panicked { message: String },
}

/// Why another recording was refused.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AltRefusal {
    #[error("it was recorded on {alt} and this battle on {primary}")]
    Version { primary: String, alt: String },
    #[error("the battle it belongs to could not be read from it")]
    NoArenaId,
    #[error("it is a recording of battle {alt}, not {primary}")]
    OtherBattle { primary: wows_replays::types::ArenaId, alt: wows_replays::types::ArenaId },
}

/// Why the installed build's game data could not be preloaded.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PreloadError {
    #[error("the builds under {} could not be listed: {source}", wows_dir.join("bin").display())]
    ListBuilds { wows_dir: PathBuf, source: Arc<wowsunpack::error::GameDataError> },
    #[error("no installed builds found under {}", wows_dir.join("bin").display())]
    NoInstalledBuilds { wows_dir: PathBuf },
    #[error(transparent)]
    Load(#[from] ReplayLoadError),
}

/// Why the battle in progress has no figures from the stats service.
#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("the roster could not be read from the live replay")]
    NoRoster,
    #[error(transparent)]
    Refused(#[from] wows_toolkit_viewmodel::match_stats::MatchStatsError),
    #[error(transparent)]
    Client(#[from] wows_toolkit_viewmodel::http::HttpError),
    #[error("could not reach the stats service")]
    Transport(#[source] reqwest::Error),
}

/// Why the personal-rating expected values could not be refreshed.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error(transparent)]
    Client(#[from] wows_toolkit_viewmodel::http::HttpError),
    #[error("the expected values could not be fetched")]
    Request(#[source] reqwest::Error),
    #[error("the expected values were rejected")]
    Rejected(#[source] wows_toolkit_viewmodel::personal_rating::ExpectedValuesError),
}

/// Why a replay contributed nothing to the index.
#[derive(Debug, thiserror::Error)]
pub enum IndexOneError {
    #[error("the replay could not be read")]
    Parse(#[source] ReplayLoadError),
    #[error("the rows could not be stored")]
    Store(#[from] wows_toolkit_config::index::rows::IndexError),
}

/// Why a game-data cache job did not finish.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("{0}")]
    Check(Report),
    #[error("{0}")]
    Validate(Report),
    #[error("{0}")]
    Plan(Report),
}

/// Why a build's result mapping could not be fetched or stored.
#[derive(Debug, thiserror::Error)]
pub enum ConstantsError {
    #[error("there is no storage directory to cache constants in")]
    NoStorageDir,
    #[error("no runtime to fetch on")]
    NoRuntime,
    #[error("{0}")]
    Check(#[from] wows_data_mgr::constants::ConstantsFetchError),
    #[error("{0}")]
    Fetcher(Report),
    #[error("the repository published nothing for it")]
    NothingPublished,
    #[error("{0}")]
    Serialize(#[source] serde_json::Error),
    #[error("{0}")]
    Read(#[source] std::io::Error),
    #[error("{0}")]
    Parse(#[source] serde_json::Error),
    #[error("that file is not a constants mapping")]
    NotAMapping,
    #[error("{}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
