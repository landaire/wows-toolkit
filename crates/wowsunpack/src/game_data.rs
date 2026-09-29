//! Utilities for loading game resources from a World of Warships installation directory.
//!
//! Provides version-matched resource loading: given a replay's version, finds and loads
//! the corresponding game build rather than blindly using the latest installed build.

use std::borrow::Cow;
use std::fs::read_dir;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::*;
use vfs::VfsPath;
use vfs::impls::overlay::OverlayFS;

use crate::data::DataFileWithCallback;
use crate::data::Version;
use crate::data::assets_bin_vfs::AssetsBinVfs;
use crate::data::idx;
use crate::data::idx_vfs::IdxVfs;
use crate::data::wrappers::mmap::MmapPkgSource;
use crate::error::GameDataError;
use crate::rpc::entitydefs::EntitySpec;
use crate::rpc::entitydefs::parse_scripts;

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

    let build = replay_version.build_number().ok_or(GameDataError::BuildUnknown)?;
    if available_builds.contains(&build) { Ok(build) } else { Err(GameDataError::BuildNotFound { build }) }
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

/// Whether a built VFS carries the `content/assets.bin` overlay.
///
/// The overlay costs 213 MiB per build: the whole blob is read onto the heap
/// and kept there, plus an index of the paths inside it. Only a caller that
/// resolves those inner paths needs it, and a caller that reads
/// `content/assets.bin` as a file (which is what
/// [`crate::export::ship::ShipAssets`] does) reads it out of the package layer
/// either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetsBin {
    /// Paths inside `assets.bin` resolve against the returned VFS.
    Overlay,
    /// Only what the packages hold. `content/assets.bin` is still readable as
    /// a file; its contents are not addressable as paths.
    Omit,
}

/// Build a VFS from a World of Warships installation directory.
///
/// Uses the latest build in `bin/` and loads all idx files.
///
/// This is the same VFS setup used by the CLI. If you already have a
/// [`VfsPath`], pass it directly to [`crate::export::ship::ShipAssets::load`]
/// instead.
pub fn build_game_vfs(game_dir: &Path, assets: AssetsBin) -> Result<VfsPath, Report> {
    let builds = list_available_builds(game_dir).attach_with(|| format!("game_dir: {}", game_dir.display()))?;
    let latest_build =
        *builds.last().ok_or_else(|| rootcause::report!("No builds found in {}/bin", game_dir.display()))?;
    build_game_vfs_for_build(game_dir, latest_build, assets)
}

/// Build a VFS from a specific build number's idx files.
pub fn build_game_vfs_for_build(game_dir: &Path, build: u32, assets: AssetsBin) -> Result<VfsPath, Report> {
    let idx_dir = game_dir.join("bin").join(build.to_string()).join("idx");
    if !idx_dir.exists() {
        bail!("idx directory not found: {}", idx_dir.display());
    }

    let mut idx_files = Vec::new();
    let mut idx_errors = Vec::new();
    for entry in read_dir(&idx_dir).context_with(|| format!("Failed to read idx dir: {}", idx_dir.display()))? {
        let entry = entry?;
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let path = entry.path();
            let data = std::fs::read(&path).attach_with(|| format!("path: {}", path.display()))?;
            match idx::parse(&data) {
                Ok(parsed) => idx_files.push(parsed),
                Err(e) => {
                    let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    // Log the first 16 bytes as hex for debugging unknown formats
                    let header_hex: String =
                        data.iter().take(16).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                    eprintln!("WARN: Failed to parse idx file {filename}: {e} (header: {header_hex})");
                    idx_errors.push((filename, e));
                }
            }
        }
    }
    if idx_files.is_empty() && !idx_errors.is_empty() {
        let names: Vec<_> = idx_errors.iter().map(|(n, e)| format!("{n}: {e}")).collect();
        bail!("All idx files failed to parse for build {build}:\n  {}", names.join("\n  "));
    }

    let pkgs_dir = game_dir.join("res_packages");
    if !pkgs_dir.exists() {
        bail!("res_packages not found: {}", pkgs_dir.display());
    }

    let pkg_source = MmapPkgSource::new(&pkgs_dir);
    let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
    let pkg_vfs = VfsPath::new(idx_vfs);

    // Report VFS stats for debugging empty dumps
    if let Ok(root) = pkg_vfs.read_dir() {
        let count = root.count();
        if count == 0 && !idx_files.is_empty() {
            eprintln!(
                "WARN: VFS is empty despite {} idx files parsing successfully for build {build}",
                idx_files.len()
            );
            // List available pkg files on disk
            let mut on_disk = Vec::new();
            if let Ok(entries) = read_dir(&pkgs_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str()
                        && name.ends_with(".pkg")
                    {
                        on_disk.push(name.to_string());
                    }
                }
            }
            on_disk.sort();
            eprintln!("  pkg files on disk ({}):", on_disk.len());
            for pkg in &on_disk {
                eprintln!("    {pkg}");
            }
            for (i, idx) in idx_files.iter().enumerate() {
                let _fname = idx_errors.iter().find(|(_, _)| false).map(|(n, _)| n.as_str()).unwrap_or("?");
                eprintln!(
                    "  idx[{i}]: {} resources, {} file_infos, {} volumes",
                    idx.resources.len(),
                    idx.file_infos.len(),
                    idx.volumes.len()
                );
                for vol in &idx.volumes {
                    let exists = on_disk.iter().any(|p| p == &vol.filename);
                    eprintln!("    volume {}: {} (exists: {})", vol.volume_id, vol.filename, exists);
                }
            }
        }
    }
    if !idx_errors.is_empty() {
        eprintln!(
            "WARN: {}/{} idx files failed to parse for build {build}:",
            idx_errors.len(),
            idx_errors.len() + idx_files.len()
        );
        for (name, e) in &idx_errors {
            eprintln!("  {name}: {e}");
        }
    }

    if assets == AssetsBin::Omit {
        return Ok(pkg_vfs);
    }

    Ok(overlay_assets_bin(&pkg_vfs).map(|overlay| overlay.vfs).unwrap_or(pkg_vfs))
}

/// A package VFS with `assets.bin` over it, and the blob both layers read.
pub struct AssetsOverlay {
    /// The overlaid VFS: paths inside `assets.bin` resolve against it.
    pub vfs: VfsPath,
    /// The raw blob, shared with the overlay rather than read a second time.
    pub bytes: std::sync::Arc<Vec<u8>>,
}

/// `pkg_vfs` with the paths inside its `content/assets.bin` resolvable.
///
/// Separate from [`build_game_vfs_for_build`] so a caller holding a package
/// VFS can add the overlay later without parsing that build's idx files a
/// second time. `None` when the build ships no readable `assets.bin`, which
/// leaves the caller with the package VFS it already had.
pub fn overlay_assets_bin(pkg_vfs: &VfsPath) -> Option<AssetsOverlay> {
    let mut assets_bin_data = Vec::new();
    pkg_vfs
        .join("content/assets.bin")
        .and_then(|p| p.open_file())
        .and_then(|mut f| {
            f.read_to_end(&mut assets_bin_data)?;
            Ok(())
        })
        .ok()?;

    let bytes = std::sync::Arc::new(assets_bin_data);
    let assets_vfs = AssetsBinVfs::from_shared(std::sync::Arc::clone(&bytes)).ok()?;
    let overlay = OverlayFS::new(&[VfsPath::new(assets_vfs), pkg_vfs.clone()]);
    Some(AssetsOverlay { vfs: VfsPath::new(overlay), bytes })
}
