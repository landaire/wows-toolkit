//! Utilities for loading game resources from a World of Warships installation directory.
//!
//! Provides version-matched resource loading: given a replay's version, finds and loads
//! the corresponding game build rather than blindly using the latest installed build.

use std::borrow::Cow;
use std::fs::read_dir;
use std::io::Read;
use std::path::{Path, PathBuf};

use rootcause::prelude::*;
use vfs::VfsPath;
use vfs::impls::overlay::OverlayFS;

use crate::data::assets_bin_vfs::AssetsBinVfs;
use crate::data::idx;
use crate::data::idx_vfs::IdxVfs;
use crate::data::wrappers::mmap::MmapPkgSource;
use crate::data::{DataFileWithCallback, Version};
use crate::error::GameDataError;
use crate::rpc::entitydefs::{EntitySpec, parse_scripts};

/// List all available build numbers in the game directory's `bin/` folder, sorted ascending.
pub fn list_available_builds(game_dir: &Path) -> Result<Vec<u32>, GameDataError> {
    let bin_dir = game_dir.join("bin");
    let mut builds: Vec<u32> = Vec::new();
    for entry in read_dir(&bin_dir)? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
            && let Some(build_num) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok())
        {
            builds.push(build_num);
        }
    }
    builds.sort();
    Ok(builds)
}

/// Find the build directory in the game directory that matches the replay's version.
pub fn find_matching_build(game_dir: &Path, replay_version: &Version) -> Result<u32, GameDataError> {
    let available_builds = list_available_builds(game_dir)?;

    if available_builds.contains(&replay_version.build) {
        Ok(replay_version.build)
    } else {
        Err(GameDataError::BuildNotFound { build: replay_version.build })
    }
}

/// Loaded game resources from a WoWS installation.
pub struct GameResources {
    pub specs: Vec<EntitySpec>,
    pub vfs: VfsPath,
}

/// Load game resources (entity specs, VFS) from a game directory,
/// using the build number that matches the replay version.
pub fn load_game_resources(game_dir: &Path, replay_version: &Version) -> Result<GameResources, GameDataError> {
    let build = find_matching_build(game_dir, replay_version)?;

    let idx_dir = game_dir.join("bin").join(build.to_string()).join("idx");
    let mut idx_files = Vec::new();

    for entry in read_dir(&idx_dir)? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let file_data = std::fs::read(entry.path())?;
            idx_files.push(idx::parse(&file_data)?);
        }
    }

    let pkgs_path = game_dir.join("res_packages");
    if !pkgs_path.exists() {
        return Err(GameDataError::ResPackagesNotFound);
    }

    let pkg_source = MmapPkgSource::new(&pkgs_path);
    let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
    let vfs = VfsPath::new(idx_vfs);

    let specs = {
        let vfs_ref = &vfs;
        let loader = DataFileWithCallback::new(move |path: &str| {
            let file_path = vfs_ref.join(path)?;
            let mut data = Vec::new();
            file_path.open_file()?.read_to_end(&mut data)?;
            Ok(Cow::Owned(data))
        });
        parse_scripts(&loader)?
    };

    Ok(GameResources { specs, vfs })
}

/// Returns the path to the English translations file for the given build.
pub fn translations_path(game_dir: &Path, build: u32) -> PathBuf {
    game_dir.join("bin").join(build.to_string()).join("res/texts/en/LC_MESSAGES/global.mo")
}

/// Build a VFS from a World of Warships installation directory.
///
/// Uses the latest build in `bin/`, loads all idx files, and overlays
/// `assets.bin` on top of the package VFS so that asset paths resolve
/// correctly.
///
/// This is the same VFS setup used by the CLI. If you already have a
/// [`VfsPath`], pass it directly to [`crate::export::ship::ShipAssets::load`]
/// instead.
pub fn build_game_vfs(game_dir: &Path) -> Result<VfsPath, Report> {
    let builds = list_available_builds(game_dir).attach_with(|| format!("game_dir: {}", game_dir.display()))?;
    let latest_build =
        builds.last().ok_or_else(|| rootcause::report!("No builds found in {}/bin", game_dir.display()))?;

    let idx_dir = game_dir.join("bin").join(latest_build.to_string()).join("idx");
    if !idx_dir.exists() {
        bail!("idx directory not found: {}", idx_dir.display());
    }

    let mut idx_files = Vec::new();
    for entry in read_dir(&idx_dir).context_with(|| format!("Failed to read idx dir: {}", idx_dir.display()))? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let path = entry.path();
            let data = std::fs::read(&path).attach_with(|| format!("path: {}", path.display()))?;
            let parsed = idx::parse(&data).attach_with(|| format!("path: {}", path.display()))?;
            idx_files.push(parsed);
        }
    }

    let pkgs_dir = game_dir.join("res_packages");
    if !pkgs_dir.exists() {
        bail!("res_packages not found: {}", pkgs_dir.display());
    }

    let pkg_source = MmapPkgSource::new(&pkgs_dir);
    let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
    let pkg_vfs = VfsPath::new(idx_vfs);

    // Overlay assets.bin on top of the package VFS.
    let mut assets_bin_data = Vec::new();
    let assets_loaded = pkg_vfs
        .join("content/assets.bin")
        .and_then(|p| p.open_file())
        .and_then(|mut f| {
            f.read_to_end(&mut assets_bin_data)?;
            Ok(())
        })
        .is_ok();

    if assets_loaded && let Ok(assets_vfs) = AssetsBinVfs::new(assets_bin_data) {
        let assets_layer = VfsPath::new(assets_vfs);
        let overlay = OverlayFS::new(&[assets_layer, pkg_vfs]);
        return Ok(VfsPath::new(overlay));
    }

    Ok(pkg_vfs)
}
