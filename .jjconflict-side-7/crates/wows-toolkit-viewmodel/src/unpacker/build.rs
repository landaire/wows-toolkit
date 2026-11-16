//! Which game builds an install carries, and loading one of them for browsing.

use std::path::Path;
use std::path::PathBuf;

use wowsunpack::vfs::VfsPath;

use crate::unpacker::assets_bin;
use crate::unpacker::assets_bin::AssetsBinError;

/// A game build number. A newtype so a build cannot be mixed up with the other bare integers on
/// the tab (queue lengths, progress counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildNumber(pub u32);

impl std::fmt::Display for BuildNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// One build of one install. Two installs can carry the same build number, so a loaded build is
/// known by both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSource {
    pub dir: PathBuf,
    pub build: BuildNumber,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildLoadError {
    #[error("could not list the builds in {dir}")]
    List {
        dir: PathBuf,
        #[source]
        source: wowsunpack::error::GameDataError,
    },
    /// `reason` is the loader's report, which is not a `std::error::Error`.
    #[error("could not load build {build}: {reason}")]
    Load { build: BuildNumber, reason: String },
    #[error(transparent)]
    AssetsBin(#[from] AssetsBinError),
}

/// The builds in `dir`, newest first: the order the build selector lists them.
pub fn list_builds(dir: &Path) -> Result<Vec<BuildNumber>, BuildLoadError> {
    let mut builds: Vec<BuildNumber> = wowsunpack::game_data::list_available_builds(dir)
        .map_err(|source| BuildLoadError::List { dir: dir.to_path_buf(), source })?
        .into_iter()
        .map(BuildNumber)
        .collect();
    builds.sort_unstable_by(|a, b| b.cmp(a));
    Ok(builds)
}

/// The package VFS of `source`, with assets.bin paths overlaid. Slow: run off the UI thread.
pub fn load_package_vfs(source: &BuildSource) -> Result<VfsPath, BuildLoadError> {
    wowsunpack::game_data::build_game_vfs_for_build(
        &source.dir,
        source.build.0,
        wowsunpack::game_data::AssetsBin::Overlay,
    )
    .map_err(|report| BuildLoadError::Load { build: source.build, reason: report.to_string() })
}

/// The assets.bin prototypes of a loaded package VFS, as their own VFS. Read after the package
/// tree so the package listing is usable first.
pub fn load_assets_bin(package_vfs: &VfsPath) -> Result<VfsPath, BuildLoadError> {
    Ok(assets_bin::open(package_vfs)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_are_listed_newest_first() {
        let dir = std::env::temp_dir().join(format!("wt-vm-builds-{}", std::process::id()));
        for build in ["100", "300", "200", "not-a-build"] {
            std::fs::create_dir_all(dir.join("bin").join(build)).unwrap();
        }
        let builds = list_builds(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(builds, vec![BuildNumber(300), BuildNumber(200), BuildNumber(100)]);
    }

    #[test]
    fn a_missing_install_fails_to_list() {
        let dir = std::env::temp_dir().join("wt-vm-builds-missing");
        assert!(matches!(list_builds(&dir), Err(BuildLoadError::List { .. })));
    }
}
