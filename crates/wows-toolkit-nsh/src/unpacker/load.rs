//! Reading what the Unpacker browses: the shared settings it starts from,
//! the builds an install carries, and one build's package VFS.
//!
//! Every function here blocks; the component runs them off the engine
//! thread.

use std::path::Path;
use std::path::PathBuf;

use wows_toolkit_viewmodel::settings::keys;
use wowsunpack::vfs::VfsPath;

use super::error::UnpackerError;

/// A game build number. A newtype so a build cannot be mixed up with the
/// other counts the tab shows (queue lengths, progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildNumber(pub u32);

impl std::fmt::Display for BuildNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The settings the Unpacker reads from the shared database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpackerSettings {
    /// The World of Warships install, absent until the user sets one.
    pub wows_dir: Option<PathBuf>,
    /// Where extractions write; empty until the user picks one, which is
    /// what keeps Extract disabled.
    pub output_dir: String,
}

/// Reads the install and extraction directories. An unset or empty value
/// is absent: the egui and GPUI apps store an empty string for "not set".
pub fn read_settings() -> Result<UnpackerSettings, UnpackerError> {
    let wows_dir = wows_toolkit_config::load_startup_setting::<String>(keys::WOWS_DIR)
        .map_err(|source| UnpackerError::ReadSetting { key: keys::WOWS_DIR, source })?
        .filter(|dir| !dir.trim().is_empty())
        .map(PathBuf::from);
    let output_dir = wows_toolkit_config::load_startup_setting::<String>(keys::OUTPUT_DIR)
        .map_err(|source| UnpackerError::ReadSetting { key: keys::OUTPUT_DIR, source })?
        .unwrap_or_default();

    Ok(UnpackerSettings { wows_dir, output_dir })
}

/// Saves the extraction directory, shared with the other front ends.
pub fn save_output_dir(output_dir: &str) -> Result<(), UnpackerError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| UnpackerError::Runtime { source })?;

    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.map_err(|source| UnpackerError::OpenDatabase { source })?;
        wows_toolkit_config::queries::set_setting(&pool, keys::OUTPUT_DIR, &output_dir)
            .await
            .map_err(|source| UnpackerError::SaveSetting { key: keys::OUTPUT_DIR, source })
    })
}

/// The builds `wows_dir` carries, newest first: the order the build
/// selector lists them in.
pub fn list_builds(wows_dir: &Path) -> Result<Vec<BuildNumber>, UnpackerError> {
    let mut builds: Vec<BuildNumber> = wowsunpack::game_data::list_available_builds(wows_dir)
        .map_err(|source| UnpackerError::ListBuilds { dir: wows_dir.to_path_buf(), source })?
        .into_iter()
        .map(BuildNumber)
        .collect();
    builds.sort_unstable_by(|left, right| right.cmp(left));

    Ok(builds)
}

/// `build`'s package VFS, with assets.bin's paths overlaid as the GPUI
/// app's package pane shows them.
pub fn load_package_vfs(wows_dir: &Path, build: BuildNumber) -> Result<VfsPath, UnpackerError> {
    wowsunpack::game_data::build_game_vfs_for_build(wows_dir, build.0, wowsunpack::game_data::AssetsBin::Overlay)
        .map_err(|report| UnpackerError::LoadBuild { build: build.0, reason: report.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_are_listed_newest_first() {
        let install = tempfile::tempdir().expect("a temporary install is created");
        for build in ["100", "300", "200", "not-a-build"] {
            std::fs::create_dir_all(install.path().join("bin").join(build)).expect("a build directory is created");
        }

        let builds = list_builds(install.path()).expect("the builds list");

        assert_eq!(builds, vec![BuildNumber(300), BuildNumber(200), BuildNumber(100)]);
    }

    #[test]
    fn an_install_without_a_bin_directory_reports_it() {
        let install = tempfile::tempdir().expect("a temporary install is created");

        assert!(matches!(list_builds(install.path()), Err(UnpackerError::ListBuilds { .. })));
    }
}
