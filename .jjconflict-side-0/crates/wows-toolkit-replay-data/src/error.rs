//! Errors of the replay and game-data loads.

/// Reasons a replay could not become a [`ReplayReportModel`](crate::model::ReplayReportModel).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReplayLoadError {
    /// Reading the replay file off disk failed (not found, permissions, truncated).
    #[error("failed to read replay file: {0}")]
    Io(String),
    /// The replay's `clientVersionFromExe` did not parse into a build number.
    #[error("could not parse the replay's client version")]
    VersionParse,
    /// The replay's build has no matching `bin/<build>` directory in the
    /// install. Loading an uninstalled build's data from a dump is not
    /// implemented yet.
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
