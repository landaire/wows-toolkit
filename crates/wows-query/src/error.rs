use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
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
    #[error("no ship matches {query:?}; {}", did_you_mean(suggestions, "try find_ships with a shorter name"))]
    ShipNotFound { query: String, suggestions: Vec<String> },

    #[error("{query:?} matches several ships: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousShip { query: String, candidates: Vec<String> },

    #[error("{ship} has no armor zone or plate named {zone:?}; zones: {}", zones.join(", "))]
    ZoneNotFound { ship: String, zone: String, zones: Vec<&'static str> },

    #[error(
        "no captain skill matches {query:?}; {}",
        did_you_mean(suggestions, "use the skill's English name or a common acronym such as IFHE")
    )]
    SkillNotFound { query: String, suggestions: Vec<String> },

    #[error("captain skill {skill} is not in the {class} skill tree")]
    SkillNotForClass { skill: String, class: &'static str },

    #[error("{query:?} matches several captain skills: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousSkill { query: String, candidates: Vec<String> },

    #[error(
        "no upgrade matches {query:?}; {}",
        did_you_mean(suggestions, "use the upgrade's English name, e.g. \"Main Battery Modification 2\"")
    )]
    UpgradeNotFound { query: String, suggestions: Vec<String> },

    #[error("{query:?} matches several upgrades: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousUpgrade { query: String, candidates: Vec<String> },

    #[error("upgrade {upgrade} does not fit {ship}")]
    UpgradeNotApplicable { upgrade: String, ship: String },

    #[error("upgrades {first} and {second} both use slot {slot}; a ship mounts one upgrade per slot")]
    UpgradeSlotConflict { first: String, second: String, slot: u8 },

    #[error("{ship} has no module matching {query:?}; modules: {}", modules.join(", "))]
    ModuleNotFound { ship: String, query: String, modules: Vec<String> },

    #[error("{query:?} matches several modules: {}; ask again with the exact name", candidates.join(", "))]
    AmbiguousModule { query: String, candidates: Vec<String> },

    #[error("modules {first} and {second} are both {kind}; a ship mounts one module of each kind")]
    ModuleSlotConflict { first: String, second: String, kind: &'static str },

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

    #[error("no GameParams entry has the name, index or id {query:?}")]
    ParamNotFound { query: String },

    /// Shared because a failed build is reported to every call until it is
    /// retried.
    #[error("raw GameParams are not available: {source}")]
    RawParams {
        #[source]
        source: Arc<RawParamsError>,
    },

    #[error(transparent)]
    Jq(#[from] JqError),
}

/// Failure to build or read the raw GameParams store.
#[derive(Debug, thiserror::Error)]
pub enum RawParamsError {
    #[error("the game data was loaded without a cache directory to hold them")]
    NoCacheDir,

    #[error("failed to open GameParams.data in {dir}")]
    Source {
        dir: PathBuf,
        #[source]
        source: wowsunpack::vfs::VfsError,
    },

    #[error("failed to decode GameParams.data")]
    Decode(#[source] wowsunpack::error::GameDataError),

    #[error("failed to encode GameParams entry {name} as JSON")]
    Encode {
        name: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("GameParams entry {name} is {len} bytes, more than a record holds")]
    RecordTooLarge { name: String, len: usize },

    #[error("I/O error on {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read or write the index {path}")]
    Index {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("{path} is {found} bytes; its index expects {expected}")]
    Truncated { path: PathBuf, expected: u64, found: u64 },

    #[error("the stored record of {name} is damaged")]
    DamagedRecord { name: String },

    #[error("{path} belongs to build {found}, not {expected}")]
    WrongBuild { path: PathBuf, expected: u32, found: u32 },

    #[error("the store published at {dir} was removed before it could be opened")]
    Vanished { dir: PathBuf },
}

/// A jq filter that does not compile. Messages are written for the model
/// that wrote the filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum CompileError {
    #[error("jq syntax error: {}", errors.join("; "))]
    Syntax { errors: Vec<String> },

    #[error("jq filter uses undefined {}", names.join(", "))]
    Undefined { names: Vec<String> },

    #[error("jq modules (import, include) are not available")]
    Module,

    #[error("names starting with __ are reserved; the filter uses {name}")]
    Reserved { name: String },
}

/// Failure to evaluate a jq filter.
#[derive(Debug, thiserror::Error)]
pub enum JqError {
    #[error(transparent)]
    Compile(#[from] CompileError),

    #[error("jq evaluation is not set up in this program; call wows_query::jq_worker::init() first in main")]
    NoWorker,

    #[error("failed to start the jq worker {program}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to create the jq worker's working directory {path}")]
    Workdir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("all jq workers stayed busy for {waited:?}; retry later")]
    Busy { waited: std::time::Duration },

    #[error("the jq worker exited before it read the request ({}); check that main calls wows_query::jq_worker::init() first", exit_description(.status.as_ref()))]
    Exited { status: Option<std::process::ExitStatus> },

    #[error("a jq input of {bytes} bytes exceeds the 4 GiB record limit")]
    InputTooLarge { bytes: usize },

    #[error("the jq worker sent an unreadable reply")]
    Reply(#[source] serde_json::Error),
}

fn exit_description(status: Option<&std::process::ExitStatus>) -> String {
    match status {
        Some(status) => status.to_string(),
        None => "exit status unknown".to_owned(),
    }
}

/// "did you mean: A, B?" when there are suggestions, otherwise `hint`.
fn did_you_mean(suggestions: &[String], hint: &str) -> String {
    if suggestions.is_empty() {
        return hint.to_owned();
    }

    format!("did you mean: {}?", suggestions.join(", "))
}
