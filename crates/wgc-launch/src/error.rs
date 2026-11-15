use std::path::PathBuf;
use std::process::ExitStatus;

use thiserror::Error;

use crate::overlay::OverlayBackend;
use crate::session::ProcessId;

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("no game runner found; install Steam with Proton, or umu-launcher")]
    NoRunner,
    #[error("no Proton install at {dir}")]
    ProtonNotFound { dir: PathBuf },
    #[error("umu-run not found")]
    UmuNotFound { path: Option<PathBuf> },
    #[error("the Steam client was not found; Steam's Proton needs it")]
    SteamNotFound,
}

#[derive(Debug, Error)]
pub enum PrefixInitError {
    #[error("could not create {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not start {program}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("prefix initialization failed with {status}")]
    Failed { status: ExitStatus },
    #[error("the runner finished but created no prefix at {path}")]
    NotCreated { path: PathBuf },
}

#[derive(Debug, Error)]
pub enum ViewError {
    #[error("game install not found at {path}")]
    InstallMissing {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not test whether {path} is writable")]
    WriteProbe {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not prepare {path}")]
    StateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the install is read-only and neither a bwrap overlay nor fuse-overlayfs is available")]
    NoOverlayBackend,
    #[error(
        "the saved game data at {upper} was written by {recorded:?}, which is unavailable; delete it to start over"
    )]
    BackendUnavailable { upper: PathBuf, recorded: OverlayBackend },
    #[error("unrecognized overlay record {content:?} at {path}")]
    UnknownBackendRecord { path: PathBuf, content: String },
    #[error("read-only installs are only supported on Linux")]
    ReadOnlyUnsupported { path: PathBuf },
}

#[derive(Debug, Error)]
pub enum MountError {
    #[error("could not start {program}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} failed with {status}: {stderr}")]
    Failed { program: PathBuf, status: ExitStatus, stderr: String },
    #[error("could not read the mount table")]
    MountTable(#[source] std::io::Error),
    #[error("fuse-overlayfs cannot use {path} as its work directory because the path contains a backslash")]
    WorkDirBackslash { path: PathBuf },
}

#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("a game session is already running")]
    SessionRunning { pids: Vec<ProcessId> },
    #[error("could not list running processes")]
    Scan(#[source] std::io::Error),
    #[error(transparent)]
    Mount(#[from] MountError),
    #[error("could not create {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not start {program}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The session has ended when `wait` returns, whichever variant.
#[derive(Debug, Error)]
pub enum WaitError {
    #[error("could not wait for the runner")]
    Runner(#[source] std::io::Error),
    #[error("could not list running processes")]
    Scan(#[source] std::io::Error),
    #[error("could not unmount the game overlay")]
    Unmount(#[source] MountError),
}

#[derive(Debug, Error)]
pub enum KillError {
    #[error("could not list running processes")]
    Scan(#[source] std::io::Error),
    #[error("game processes survived the kill: {pids:?}")]
    Survivors { pids: Vec<ProcessId> },
}

#[derive(Debug, Error)]
#[error("{path:?} is not a plain relative path")]
pub struct RelativePathError {
    pub path: PathBuf,
}
