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

pub mod registry;
pub mod steam;
pub mod version;
pub mod wgc;

use std::path::Path;

/// Deduplicate installs by canonicalized path, keeping the first occurrence
/// (callers order sources by trust: WGC, Steam, Registry, Manual).
pub fn dedupe_by_path(installs: Vec<GameInstall>) -> Vec<GameInstall> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for install in installs {
        let key = std::fs::canonicalize(&install.path).unwrap_or_else(|_| install.path.clone());
        if seen.insert(key) {
            out.push(install);
        }
    }
    out
}

/// Discover installs from every source, version-detect them, and dedupe.
pub fn discover_all(wgc_data_dir_override: Option<&Path>, manual: &[std::path::PathBuf]) -> Vec<GameInstall> {
    let mut all = Vec::new();
    match wgc::discover_wgc_installs(wgc_data_dir_override) {
        Ok(installs) => all.extend(installs),
        Err(e) => tracing::warn!("WGC discovery failed: {e}"),
    }
    all.extend(steam::discover_steam_installs());
    all.extend(registry::discover_registry_installs());
    all.extend(manual.iter().cloned().map(GameInstall::manual));

    let mut deduped = dedupe_by_path(all);
    for install in &mut deduped {
        version::detect(install);
    }
    deduped
}

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

    #[test]
    fn dedupe_keeps_first_source_for_same_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let wgc =
            GameInstall { source: InstallSource::Wgc { working_dir: p.clone() }, ..GameInstall::manual(p.clone()) };
        let manual = GameInstall::manual(p.clone());
        let deduped = super::dedupe_by_path(vec![wgc, manual]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].source.label(), "WGC");
    }
}
