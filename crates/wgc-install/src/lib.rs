//! On-disk WoWs install discovery.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Clone)]
pub enum InstallSource {
    Wgc { working_dir: PathBuf },
    Steam { app_id: steamroom::depot::AppId, library: PathBuf },
    Manual,
}

impl InstallSource {
    pub fn label(&self) -> &'static str {
        match self {
            InstallSource::Wgc { .. } => "WGC",
            InstallSource::Steam { .. } => "Steam",
            InstallSource::Manual => "Manual",
        }
    }

    /// Steam installs are managed by Steam and must never be patched by us.
    pub fn is_read_only_content(&self) -> bool {
        matches!(self, InstallSource::Steam { .. })
    }
}

#[derive(Debug, Clone)]
pub struct GameInstall {
    pub path: PathBuf,
    pub version: Option<wowsunpack::data::Version>,
    pub build: Option<u32>,
    pub source: InstallSource,
    pub product_id: Option<wgc_core::ProductId>,
    pub auto_update: bool,
}

impl GameInstall {
    pub fn manual(path: PathBuf) -> GameInstall {
        GameInstall {
            path,
            version: None,
            build: None,
            source: InstallSource::Manual,
            product_id: None,
            auto_update: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("WGC data directory not found")]
    WgcDataDirMissing,
    #[error("failed to parse WGC xml: {detail}")]
    Xml { detail: String },
    #[error("io error: {detail}")]
    Io { detail: String },
}

pub mod steam;
pub mod wgc;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn manual_install_has_manual_source_and_no_version() {
        let install = GameInstall::manual(PathBuf::from("/games/wows"));
        assert_eq!(install.source.label(), "Manual");
        assert!(install.version.is_none());
        assert!(!install.auto_update);
    }
}
