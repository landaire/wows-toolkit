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
use crate::error::ModFolderError;
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

/// Where a file that replaces a packaged one goes for `build`: the first search path in the
/// build's `paths.xml`, which the game reads before its packages. The client keeps that file
/// beside its executable: `bin/<build>/bin64` or `bin32`, or the install's root in clients
/// older than the per-build layout. An install without one gets `bin/<build>/res_mods`, the
/// folder Wargaming's mod documentation names.
pub fn mod_folder(game_dir: &Path, build: u32) -> Result<PathBuf, ModFolderError> {
    let build_dir = game_dir.join("bin").join(build.to_string());
    let Some(paths_xml) = [build_dir.join("bin64"), build_dir.join("bin32"), game_dir.to_path_buf()]
        .into_iter()
        .map(|dir| dir.join("paths.xml"))
        .find(|path| path.is_file())
    else {
        return Ok(build_dir.join("res_mods"));
    };

    let text = std::fs::read_to_string(&paths_xml)
        .map_err(|source| ModFolderError::Read { path: paths_xml.clone(), source })?;

    first_search_path(&paths_xml, &text)
}

/// The first `<Path>` of `paths_xml`, resolved against the folder holding it unless it is
/// absolute. Separators may be either slash: the client writes backslashes.
fn first_search_path(paths_xml: &Path, text: &str) -> Result<PathBuf, ModFolderError> {
    let document =
        roxmltree::Document::parse(text).map_err(|_| ModFolderError::Parse { path: paths_xml.to_path_buf() })?;
    let listed = document
        .descendants()
        .filter(|node| node.has_tag_name("Path"))
        .find_map(|node| node.text().map(str::trim).filter(|text| !text.is_empty()))
        .ok_or_else(|| ModFolderError::NoSearchPath { path: paths_xml.to_path_buf() })?;

    if Path::new(listed).is_absolute() {
        return Ok(PathBuf::from(listed));
    }
    let mut resolved = paths_xml.to_path_buf();
    resolved.pop();
    for part in listed.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                resolved.pop();
            }
            part => resolved.push(part),
        }
    }

    Ok(resolved)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn search_path(text: &str) -> Result<PathBuf, ModFolderError> {
        first_search_path(Path::new("/wows/bin/13533378/bin64/paths.xml"), text)
    }

    #[test]
    fn the_first_search_path_is_the_mod_folder() {
        let current = r#"<root><Paths>
            <Path>..\res_mods</Path>
            <Path key="15b8d" packages="..\..\..\res_packages" type="PFS">..\idx</Path>
            <Path key="35d52">..\res</Path>
        </Paths></root>"#;
        let versioned = "<root><Paths><Path>../res_mods/0.7.2.0</Path><Path>../res</Path></Paths></root>";

        assert_eq!(search_path(current).unwrap(), PathBuf::from("/wows/bin/13533378/res_mods"));
        assert_eq!(search_path(versioned).unwrap(), PathBuf::from("/wows/bin/13533378/res_mods/0.7.2.0"));
    }

    #[test]
    fn a_paths_file_without_a_search_path_is_an_error() {
        assert!(matches!(search_path("<root><Paths/></root>"), Err(ModFolderError::NoSearchPath { .. })));
        assert!(matches!(search_path("<root><Paths>"), Err(ModFolderError::Parse { .. })));
    }

    #[test]
    fn a_client_older_than_the_per_build_layout_keeps_paths_xml_at_the_root() {
        let game_dir = std::env::temp_dir().join(format!("wowsunpack-mod-folder-{}", std::process::id()));
        std::fs::create_dir_all(&game_dir).unwrap();
        let paths = "<root><Paths><Path>res_mods/0.6.13.0</Path><Path>res</Path></Paths></root>";
        std::fs::write(game_dir.join("paths.xml"), paths).unwrap();

        let folder = mod_folder(&game_dir, 296659);
        std::fs::remove_dir_all(&game_dir).unwrap();

        assert_eq!(folder.unwrap(), game_dir.join("res_mods").join("0.6.13.0"));
    }
}
