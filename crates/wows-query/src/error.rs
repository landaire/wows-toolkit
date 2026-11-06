use std::path::PathBuf;

use wowsunpack::game_params::ttx::modifiers::ModifierError;

/// Failure to load a game data build.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{dir} is not a game data dump: it has no readable metadata.toml")]
    NotADump { dir: PathBuf },

    #[error("{root} lists no game data builds in builds.toml")]
    NoBuilds { root: PathBuf },

    #[error("game data dump {dir} has no {file}")]
    MissingFile { dir: PathBuf, file: &'static str },

    #[error("game version {version:?} in {dir} is not major.minor.patch")]
    Version { dir: PathBuf, version: String },

    #[error("failed to read {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse GameParams in {dir}")]
    GameParams {
        dir: PathBuf,
        #[source]
        source: wowsunpack::error::GameDataError,
    },

    #[error("failed to parse translations {path}")]
    Translations {
        path: PathBuf,
        #[source]
        source: gettext::Error,
    },

    #[error("the game data build index lists no builds")]
    NoRemoteBuilds,

    #[error("failed to download game data")]
    Download,
}

/// A query that cannot be answered from the loaded data. Messages are written
/// for the person or model that made the query.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("no ship matches {query:?}; try find_ships with a shorter name")]
    ShipNotFound { query: String },

    #[error("{query:?} matches several ships: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousShip { query: String, candidates: Vec<String> },

    #[error("{ship} has no armor zone or plate named {zone:?}; zones: {}", zones.join(", "))]
    ZoneNotFound { ship: String, zone: String, zones: Vec<&'static str> },

    #[error("no captain skill matches {query:?}; use the skill's English name or a common acronym such as IFHE")]
    SkillNotFound { query: String },

    #[error("captain skill {skill} is not in the {class} skill tree")]
    SkillNotForClass { skill: String, class: &'static str },

    #[error("{query:?} matches several captain skills: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousSkill { query: String, candidates: Vec<String> },

    #[error("no upgrade matches {query:?}; use the upgrade's English name, e.g. \"Main Battery Modification 2\"")]
    UpgradeNotFound { query: String },

    #[error("{query:?} matches several upgrades: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousUpgrade { query: String, candidates: Vec<String> },

    #[error("upgrade {upgrade} does not fit {ship}")]
    UpgradeNotApplicable { upgrade: String, ship: String },

    #[error("upgrades {first} and {second} both use slot {slot}; a ship mounts one upgrade per slot")]
    UpgradeSlotConflict { first: String, second: String, slot: u8 },

    #[error("{reason}")]
    InvalidArguments { reason: &'static str },

    #[error("failed to apply modifiers to {ship}")]
    Modifiers {
        ship: String,
        #[source]
        source: ModifierError,
    },

    #[error("not a readable World of Warships replay")]
    Replay(#[source] wows_replays::ParseError),
}
