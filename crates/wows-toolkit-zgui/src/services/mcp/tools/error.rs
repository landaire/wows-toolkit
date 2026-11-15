use std::path::PathBuf;

use wows_mcp::PathError;
use wows_toolkit_config::index::query_text::ParseErrorKind;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_replay_data::error::ReplayLoadError;

/// Why a tool could not answer. `Display` is written for the model that made the call.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// An argument the schema allows but the tool does not, such as a page too long.
    #[error("{message}")]
    InvalidArgument { message: String },

    #[error("the replay index is not available yet")]
    IndexUnavailable,

    #[error("the replay index could not be read: {source}")]
    Index {
        #[source]
        source: IndexError,
    },

    #[error("the query does not parse at characters {start}..{end}: {kind}")]
    Query { start: usize, end: usize, kind: ParseErrorKind },

    #[error("no indexed replay has arena id {arena_id}")]
    UnknownArena { arena_id: i64 },

    #[error("cannot read the replay: {source}")]
    ReplayPath {
        #[source]
        source: PathError,
    },

    #[error("{path:?} is not a .wowsreplay file")]
    NotAReplay { path: PathBuf },

    #[error("{path:?} is {size} bytes, larger than any replay")]
    ReplayTooLarge { path: PathBuf, size: u64 },

    #[error("cannot read the replay {path:?}: {reason}")]
    ReplayRead { path: PathBuf, reason: String },

    #[error("no game data is available; set the game directory in Settings")]
    NoGameData,

    #[error("cannot load the replay: {source}")]
    ReplayLoad {
        #[source]
        source: ReplayLoadError,
    },

    #[error("the replay's client version {version:?} names no build")]
    UnknownVersion { version: String },

    #[error("the answer could not be encoded: {source}")]
    Encode {
        #[source]
        source: serde_json::Error,
    },

    #[error("the answer would be {size} bytes, more than the {limit} allowed; {narrow}")]
    TooLarge { size: usize, limit: usize, narrow: &'static str },

    #[error("the game data cache's list of builds cannot be read: {source}")]
    CacheIndex {
        #[source]
        source: wows_data_mgr::error::BuildsIndexError,
    },

    #[error("build {build} is neither installed nor in the game data cache")]
    UnknownBuild { build: u32 },

    #[error("cannot open the files of build {build}: {reason}")]
    BuildFiles { build: u32, reason: String },

    #[error("{path:?} contains a parent directory component")]
    ParentComponent { path: String },

    #[error("{path:?} contains a drive separator or a NUL")]
    ForbiddenCharacter { path: String },

    #[error("{path:?} does not exist in build {build}")]
    NotFound { build: u32, path: String },

    #[error("{path:?} is a directory; list it with game_files_list")]
    IsDirectory { path: String },

    #[error("{path:?} is a file; read it with game_file_read")]
    NotADirectory { path: String },

    #[error("reading {path:?} failed: {reason}")]
    FileRead { path: String, reason: String },

    #[error("{path:?} is not valid UTF-8 at byte {at}; read it as base64")]
    NotText { path: String, at: u64 },

    #[error("the tool's task failed: {reason}")]
    Task { reason: String },
}
