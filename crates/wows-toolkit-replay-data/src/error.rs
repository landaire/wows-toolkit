use std::path::PathBuf;

/// Reasons a replay could not become a [`ReplayReportModel`](crate::model::ReplayReportModel).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReplayLoadError {
    /// Reading the replay file off disk failed (not found, permissions, truncated).
    #[error("failed to read replay file: {0}")]
    Io(String),
    /// The replay's `clientVersionFromExe` did not parse into a build number.
    #[error("could not parse the replay's client version")]
    VersionParse,
    /// Neither the install nor the game-data cache has the replay's build.
    #[error("replay build {build} is not installed (no bin/{build} directory found)")]
    UnsupportedVersion { build: u32 },
    /// The game VFS, `GameParams`, or entity scripts could not be loaded.
    #[error("failed to load game data: {0}")]
    GameData(String),
    /// The replay's header/metadata block itself was corrupt or malformed.
    #[error("failed to parse replay: {0}")]
    Parse(String),
    /// Another recording was offered as this battle and is not it.
    #[error("{0}")]
    AltRefused(#[from] AltRefusal),
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
    #[error("the builds under {} could not be listed: {reason}", wows_dir.join("bin").display())]
    ListBuilds { wows_dir: PathBuf, reason: String },
    #[error("no installed builds found under {}", wows_dir.join("bin").display())]
    NoInstalledBuilds { wows_dir: PathBuf },
    #[error(transparent)]
    Load(#[from] ReplayLoadError),
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
