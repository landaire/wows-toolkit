use std::path::PathBuf;
use std::process::ExitStatus;

use crate::scripts::Sha256Digest;

/// Failures loading `builds.toml`.
#[derive(Debug, thiserror::Error)]
pub enum BuildsIndexError {
    #[error("failed to read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a valid builds index")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// Failures loading the local registry (`versions.toml`).
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("failed to read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a valid registry")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// Failures storing, publishing or fetching client scripts.
#[derive(Debug, thiserror::Error)]
pub enum ScriptsError {
    #[error("failed to access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a zip archive")]
    NotZip { path: PathBuf },

    #[error(transparent)]
    BuildsIndex(#[from] BuildsIndexError),

    #[error("{location} is not a valid scripts index")]
    Index {
        location: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("{location} lists build key {key:?}, which is not a build number")]
    IndexKey { location: String, key: String },

    #[error("no scripts are stored locally for build {build}")]
    NotStored { build: u32 },

    #[error("the scripts.zip downloaded for build {build} has SHA-256 {actual}, but the index lists {expected}")]
    Checksum { build: u32, expected: Sha256Digest, actual: Sha256Digest },

    #[error("release {tag} has build zips but no index.toml; restore the index before publishing")]
    ReleaseWithoutIndex { tag: &'static str },

    #[error("failed to run gh")]
    GhSpawn {
        #[source]
        source: std::io::Error,
    },

    #[error("`gh {command}` exited with {status}: {stderr}")]
    Gh { command: String, status: ExitStatus, stderr: String },

    #[error("`gh {command}` wrote output that is not UTF-8")]
    GhUtf8 {
        command: String,
        #[source]
        source: std::string::FromUtf8Error,
    },

    #[error("gh wrote JSON this program does not understand")]
    GhJson {
        #[source]
        source: serde_json::Error,
    },

    #[cfg(feature = "download")]
    #[error("request to {url} failed")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}
