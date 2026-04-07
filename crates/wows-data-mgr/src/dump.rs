use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rootcause::prelude::*;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::vfs::VfsFileType;
use wowsunpack::vfs::VfsPath;

/// VFS directories dumped in their entirety. These are small, targeted
/// directories containing only the icons/fonts/data the renderer needs.
const REQUIRED_VFS_DIRS: &[&str] = &[
    "gui/fla/minimap",
    "gui/battle_hud/markers_minimap",
    "gui/battle_hud/icon_frag",
    "gui/battle_hud/markers/capture_point",
    "gui/battle_hud/markers/building_icons",
    "gui/consumables",
    "gui/powerups/drops",
    "gui/fonts",
    "gui/data/constants",
    "gui/ships_silhouettes",
    "scripts/entity_defs",
];

/// Individual VFS files required beyond the directory dumps.
const REQUIRED_VFS_FILES: &[&str] = &["content/GameParams.data", "scripts/entities.xml"];

/// Files to extract per map from `spaces/<map>/`.
/// The renderer only needs minimap images and map metadata, not geometry/models.
const MAP_FILES_SPACES: &[&str] = &["minimap.png", "minimap_water.png", "space.settings"];

/// Files to extract per map from `content/gameplay/<map>/`.
const MAP_FILES_GAMEPLAY: &[&str] = &["space.settings"];

/// Returns the dump directory path for a given version and build.
pub fn dump_dir(output_base: &Path, version_str: &str, build: u32) -> std::path::PathBuf {
    output_base.join(format!("{version_str}_{build}"))
}

/// Check if a valid dump exists for the given version and build.
/// A dump is considered valid if its `metadata.toml` marker file exists.
pub fn dump_exists(output_base: &Path, version_str: &str, build: u32) -> bool {
    dump_dir(output_base, version_str, build).join("metadata.toml").exists()
}

/// Dump game data required for replay loading and rendering.
///
/// When `progress` is `Some`, a CLI progress bar is updated during extraction.
/// When `None`, the function runs quietly (suitable for background use in a GUI).
///
/// If `allow_existing` is true and a complete dump already exists (metadata.toml present),
/// returns `Ok(())` immediately. Partial dumps are cleaned up and re-created.
pub fn dump_renderer_data(
    game_dir: &Path,
    build: u32,
    version_str: &str,
    output_base: &Path,
    progress: Option<&ProgressBar>,
    allow_existing: bool,
) -> Result<(), Report> {
    let output_dir = dump_dir(output_base, version_str, build);
    let vfs_dir = output_dir.join("vfs");

    if output_dir.join("metadata.toml").exists() {
        if allow_existing {
            return Ok(());
        }
        bail!("Output directory already exists: {}", output_dir.display());
    }

    // Clean up partial dumps
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)?;
    }

    let vfs = game_data::build_game_vfs(game_dir).attach_with(|| "Failed to build game VFS")?;

    // Extract VFS directories
    for dir in REQUIRED_VFS_DIRS {
        extract_vfs_dir(&vfs, dir, &vfs_dir, progress)?;
    }
    for file in REQUIRED_VFS_FILES {
        extract_vfs_file(&vfs, file, &vfs_dir)?;
        if let Some(pb) = progress {
            pb.inc(1);
        }
    }
    extract_map_files(&vfs, "spaces", MAP_FILES_SPACES, &vfs_dir, progress)?;
    extract_map_files(&vfs, "content/gameplay", MAP_FILES_GAMEPLAY, &vfs_dir, progress)?;

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    // Serialize GameParams via rkyv
    let game_params = GameMetadataProvider::from_vfs(&vfs).map_err(|e| report!("Failed to load GameParams: {e:?}"))?;
    let params: Vec<Param> = game_params.params().iter().map(|p| Arc::unwrap_or_clone(Arc::clone(p))).collect();
    let bytes =
        rkyv::to_bytes::<rkyv::rancor::Error>(&params).map_err(|e| report!("Failed to serialize GameParams: {e}"))?;
    std::fs::write(output_dir.join("game_params.rkyv"), &bytes).attach_with(|| "Failed to write game_params.rkyv")?;

    // Copy all language translations
    dump_all_translations(game_dir, build, &output_dir)?;

    // Write metadata last (acts as completion marker)
    let metadata = format!("version = \"{version_str}\"\nbuild = {build}\n");
    std::fs::write(output_dir.join("metadata.toml"), metadata)?;

    Ok(())
}

/// Create a configured progress bar for CLI use. Call before `dump_renderer_data`.
pub fn create_progress_bar(game_dir: &Path) -> Option<ProgressBar> {
    let vfs = game_data::build_game_vfs(game_dir).ok()?;
    let mut total_files = 0u64;
    for dir in REQUIRED_VFS_DIRS {
        total_files += count_vfs_dir_files(&vfs, dir);
    }
    total_files += REQUIRED_VFS_FILES.len() as u64;
    total_files += count_map_files(&vfs, "spaces", MAP_FILES_SPACES);
    total_files += count_map_files(&vfs, "content/gameplay", MAP_FILES_GAMEPLAY);

    let pb = ProgressBar::new(total_files);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40}] {pos}/{len}")
            .expect("valid template")
            .progress_chars("=> "),
    );
    pb.set_message("Extracting VFS");
    Some(pb)
}

/// Copy all language translation files from the game install to the dump directory.
fn dump_all_translations(game_dir: &Path, build: u32, output_dir: &Path) -> Result<(), Report> {
    let texts_dir = game_dir.join("bin").join(build.to_string()).join("res/texts");
    if !texts_dir.exists() {
        tracing::warn!("Translations directory not found: {}", texts_dir.display());
        return Ok(());
    }
    for entry in std::fs::read_dir(&texts_dir)?.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let lang = entry.file_name();
        let mo_src = entry.path().join("LC_MESSAGES/global.mo");
        if mo_src.exists() {
            let mo_dest = output_dir.join("translations").join(&lang).join("LC_MESSAGES/global.mo");
            std::fs::create_dir_all(mo_dest.parent().unwrap())?;
            std::fs::copy(&mo_src, &mo_dest)?;
        }
    }
    Ok(())
}

// -- Internal helpers --

fn count_vfs_dir_files(vfs: &VfsPath, dir: &str) -> u64 {
    let mut count = 0;
    if let Ok(vfs_dir_path) = vfs.join(dir)
        && let Ok(walker) = vfs_dir_path.walk_dir()
    {
        for entry in walker.flatten() {
            if entry.metadata().map(|m| m.file_type == VfsFileType::File).unwrap_or(false) {
                count += 1;
            }
        }
    }
    count
}

fn count_map_files(vfs: &VfsPath, parent_dir: &str, filenames: &[&str]) -> u64 {
    let mut count = 0;
    if let Ok(parent) = vfs.join(parent_dir)
        && let Ok(entries) = parent.read_dir()
    {
        for entry in entries {
            if entry.metadata().map(|m| m.file_type == VfsFileType::Directory).unwrap_or(false) {
                for filename in filenames {
                    if entry.join(filename).is_ok_and(|f: VfsPath| f.exists().unwrap_or(false)) {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

fn extract_map_files(
    vfs: &VfsPath,
    parent_dir: &str,
    filenames: &[&str],
    output_root: &Path,
    progress: Option<&ProgressBar>,
) -> Result<(), Report> {
    let parent = match vfs.join(parent_dir) {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };
    let entries = match parent.read_dir() {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries {
        if !entry.metadata().map(|m| m.file_type == VfsFileType::Directory).unwrap_or(false) {
            continue;
        }
        for filename in filenames {
            let file_path = match entry.join(filename) {
                Ok(f) => f,
                Err(_) => continue,
            };
            if !file_path.exists().unwrap_or(false) {
                continue;
            }
            let rel = file_path.as_str();
            let dest = output_root.join(rel.trim_start_matches('/'));
            if let Some(parent_path) = dest.parent() {
                std::fs::create_dir_all(parent_path)?;
            }
            let mut src = file_path.open_file().attach_with(|| format!("Failed to open VFS file: {rel}"))?;
            let mut buf = Vec::new();
            src.read_to_end(&mut buf)?;
            std::fs::write(&dest, &buf)?;
            if let Some(pb) = progress {
                pb.inc(1);
            }
        }
    }
    Ok(())
}

fn extract_vfs_dir(
    vfs: &VfsPath,
    vfs_path: &str,
    output_root: &Path,
    progress: Option<&ProgressBar>,
) -> Result<(), Report> {
    let dir = match vfs.join(vfs_path) {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };
    let walker = match dir.walk_dir() {
        Ok(w) => w,
        Err(_) => return Ok(()),
    };

    for entry in walker.flatten() {
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.file_type != VfsFileType::File {
            continue;
        }
        let rel = entry.as_str();
        let dest = output_root.join(rel.trim_start_matches('/'));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut src = entry.open_file().attach_with(|| format!("Failed to open VFS file: {rel}"))?;
        let mut buf = Vec::new();
        src.read_to_end(&mut buf)?;
        std::fs::write(&dest, &buf)?;
        if let Some(pb) = progress {
            pb.inc(1);
        }
    }
    Ok(())
}

fn extract_vfs_file(vfs: &VfsPath, vfs_path: &str, output_root: &Path) -> Result<(), Report> {
    let file = vfs.join(vfs_path).attach_with(|| format!("VFS path not found: {vfs_path}"))?;
    let dest = output_root.join(vfs_path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut src = file.open_file().attach_with(|| format!("Failed to open VFS file: {vfs_path}"))?;
    let mut buf = Vec::new();
    src.read_to_end(&mut buf)?;
    std::fs::write(&dest, &buf)?;
    Ok(())
}
