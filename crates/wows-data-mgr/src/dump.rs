use std::collections::BTreeMap;
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

use crate::builds::BuildEntry;
use crate::builds::BuildMetadata;
use crate::builds::BuildsIndex;
use crate::cas;

/// VFS directories dumped in their entirety.
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
const MAP_FILES_SPACES: &[&str] = &["minimap.png", "minimap_water.png", "space.settings"];

/// Files to extract per map from `content/gameplay/<map>/`.
const MAP_FILES_GAMEPLAY: &[&str] = &["space.settings"];

/// Returns the dump directory path for a given version and build.
pub fn dump_dir(output_base: &Path, version_str: &str, build: u32) -> std::path::PathBuf {
    output_base.join(format!("{version_str}_{build}"))
}

/// Check if a valid dump exists for the given version and build.
pub fn dump_exists(output_base: &Path, version_str: &str, build: u32) -> bool {
    dump_dir(output_base, version_str, build).join("metadata.toml").exists()
}

/// Dump game data with content-addressed deduplication.
///
/// VFS files are stored in `{output_base}/vfs_common/` by hash, with symlinks
/// in the build's `vfs/` directory. Non-VFS files (game_params.rkyv, translations)
/// are stored directly in the build directory.
///
/// When `progress` is `Some`, a CLI progress bar is updated during extraction.
/// When `allow_existing` is true and a complete dump already exists, returns immediately.
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
    let cas_root = output_base.join("vfs_common");

    if output_dir.join("metadata.toml").exists() {
        if allow_existing {
            return Ok(());
        }
        bail!("Output directory already exists: {}", output_dir.display());
    }

    // Clean up partial dumps
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)
            .attach_with(|| format!("Failed to clean up partial dump at {}", output_dir.display()))?;
    }

    let vfs = game_data::build_game_vfs_for_build(game_dir, build).attach_with(|| "Failed to build game VFS")?;

    // Extract VFS files through CAS
    let mut file_hashes: BTreeMap<String, String> = BTreeMap::new();

    for dir in REQUIRED_VFS_DIRS {
        extract_vfs_dir_cas(&vfs, dir, &vfs_dir, &cas_root, &mut file_hashes, progress)?;
    }
    for file in REQUIRED_VFS_FILES {
        extract_vfs_file_cas(&vfs, file, &vfs_dir, &cas_root, &mut file_hashes)?;
        if let Some(pb) = progress {
            pb.inc(1);
        }
    }
    extract_map_files_cas(&vfs, "spaces", MAP_FILES_SPACES, &vfs_dir, &cas_root, &mut file_hashes, progress)?;
    extract_map_files_cas(
        &vfs,
        "content/gameplay",
        MAP_FILES_GAMEPLAY,
        &vfs_dir,
        &cas_root,
        &mut file_hashes,
        progress,
    )?;

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    std::fs::create_dir_all(&output_dir)
        .attach_with(|| format!("Failed to create output directory {}", output_dir.display()))?;

    // Serialize GameParams via rkyv (stored directly, not in CAS).
    // Wrap in catch_unwind because old game data may have missing fields that cause panics.
    let vfs_clone = vfs.clone();
    let game_params_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let gmp = GameMetadataProvider::from_vfs(&vfs_clone)?;
        let params: Vec<Param> = gmp.params().iter().map(|p| Arc::unwrap_or_clone(Arc::clone(p))).collect();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&params).map_err(|e| report!("Failed to serialize: {e}"))?;
        Ok::<_, rootcause::Report>(bytes)
    }));
    match game_params_result {
        Ok(Ok(bytes)) => {
            // Written as a plain file; refresh_build_derived (below) moves it
            // into the CAS and produces the compressed copy.
            std::fs::write(output_dir.join("game_params.rkyv"), &bytes)
                .attach_with(|| "Failed to write game_params.rkyv")?;
        }
        Ok(Err(e)) => {
            eprintln!("WARN: GameParams conversion failed for build {build}: {e:?}");
        }
        Err(_) => {
            eprintln!("WARN: GameParams conversion panicked for build {build} (incompatible format)");
        }
    }

    // Copy all language translations (stored directly)
    dump_all_translations(game_dir, build, &output_dir)?;

    // Fetch and store versioned constants (non-fatal)
    #[cfg(feature = "constants")]
    {
        match crate::constants::fetch_versioned_constants_blocking(build) {
            Ok((data, actual_build)) => {
                if let Ok(bytes) = serde_json::to_vec_pretty(&data) {
                    let _ = std::fs::write(output_dir.join("constants.json"), &bytes);
                    if actual_build != build {
                        tracing::info!("Stored constants from build {actual_build} (fallback for {build})");
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Could not fetch constants for build {build}: {e}");
            }
        }
    }

    // Write enhanced metadata with file hashes. The derived artifacts (rkyv
    // blob, compressed copies) are generated and content-addressed by the same
    // step the refresh-derived command uses, so dumps and refreshes agree.
    let mut metadata = BuildMetadata {
        version: version_str.to_string(),
        build,
        files: file_hashes,
        derived: BTreeMap::new(),
    };
    refresh_build_derived(&output_dir, &cas_root, &mut metadata)?;
    metadata.save(&output_dir.join("metadata.toml"))?;

    // Update master builds index
    let builds_path = output_base.join("builds.toml");
    let mut index = BuildsIndex::load(&builds_path);
    index.upsert(BuildEntry {
        version: version_str.to_string(),
        build,
        dir: format!("{version_str}_{build}"),
        dumped_at: jiff::Zoned::now().to_string(),
    });
    index.save(&builds_path)?;

    Ok(())
}

/// Create a configured progress bar for CLI use.
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

/// Remove a dumped build, cleaning up orphaned CAS objects.
pub fn remove_build(output_base: &Path, target_build: u32) -> Result<(), Report> {
    let builds_path = output_base.join("builds.toml");
    let mut index = BuildsIndex::load(&builds_path);
    let entry = index
        .find_by_build(target_build)
        .ok_or_else(|| report!("Build {target_build} not found in builds.toml"))?
        .clone();

    let target_dir = output_base.join(&entry.dir);
    let target_meta = BuildMetadata::load(&target_dir.join("metadata.toml"));

    // Collect hashes still in use by other builds (vfs tree + derived data)
    let mut live_hashes = std::collections::HashSet::new();
    for other in &index.builds {
        if other.build == target_build {
            continue;
        }
        if let Some(meta) = BuildMetadata::load(&output_base.join(&other.dir).join("metadata.toml")) {
            live_hashes.extend(meta.referenced_hashes());
        }
    }

    // Delete orphaned CAS objects
    if let Some(meta) = target_meta {
        let cas_root = output_base.join("vfs_common");
        for hash in meta.referenced_hashes() {
            if !live_hashes.contains(&hash) {
                let path = cas::cas_path(&cas_root, &hash);
                let _ = std::fs::remove_file(&path);
            }
        }
        // Clean up empty fanout directories
        let _ = cas::gc(&cas_root, &live_hashes);
    }

    // Remove build directory
    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir)
            .attach_with(|| format!("Failed to remove build directory {}", target_dir.display()))?;
    }

    // Update builds index
    index.remove_build(target_build);
    index.save(&builds_path)?;

    Ok(())
}

// -- Translation dumping --

fn dump_all_translations(game_dir: &Path, build: u32, output_dir: &Path) -> Result<(), Report> {
    let texts_dir = game_dir.join("bin").join(build.to_string()).join("res/texts");
    if !texts_dir.exists() {
        tracing::warn!("Translations directory not found: {}", texts_dir.display());
        return Ok(());
    }
    for entry in std::fs::read_dir(&texts_dir)
        .attach_with(|| format!("Failed to read translations directory {}", texts_dir.display()))?
        .flatten()
    {
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

// -- CAS-aware extraction helpers --

/// Read a VFS file into a buffer, store in CAS, and create a link in the build's vfs dir.
fn store_and_link(
    data: &[u8],
    rel_path: &str,
    vfs_dir: &Path,
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
) -> Result<(), Report> {
    let hash = cas::store(cas_root, data)?;
    let link_path = vfs_dir.join(rel_path.trim_start_matches('/'));
    cas::link_file(cas_root, &hash, &link_path)?;
    file_hashes.insert(rel_path.trim_start_matches('/').to_string(), hash);
    Ok(())
}

// -- Derived artifact generation (shared by dump and refresh-derived) --

/// Store `data` in the CAS and point `link_path` at it, replacing any file or
/// symlink already there. Returns the content hash.
fn store_and_relink(data: &[u8], link_path: &Path, cas_root: &Path) -> Result<String, Report> {
    let hash = cas::store(cas_root, data)?;
    let _ = std::fs::remove_file(link_path);
    cas::link_file(cas_root, &hash, link_path)?;
    Ok(hash)
}

/// Generate and content-address a build's derived artifacts: the rkyv game
/// params blob, its zstd copy, and the English translation catalog's zstd copy.
/// Uncompressed inputs are read from `build_dir`; each artifact is stored in
/// the CAS, linked back into `build_dir`, and recorded in `metadata.derived`.
///
/// Shared by the initial dump and the `refresh-derived` command so both
/// produce identical, deduplicated output. Idempotent and safe to re-run.
pub fn refresh_build_derived(
    build_dir: &Path,
    cas_root: &Path,
    metadata: &mut BuildMetadata,
) -> Result<(), Report> {
    metadata.derived.clear();

    let rkyv_path = build_dir.join("game_params.rkyv");
    if rkyv_path.exists() {
        let rkyv_bytes =
            std::fs::read(&rkyv_path).attach_with(|| format!("Failed to read {}", rkyv_path.display()))?;
        let hash = store_and_relink(&rkyv_bytes, &rkyv_path, cas_root)?;
        metadata.derived.insert("game_params.rkyv".to_string(), hash);

        let compressed = ruzstd::encoding::compress_to_vec(
            rkyv_bytes.as_slice(),
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let zst_path = build_dir.join("game_params.rkyv.zst");
        let hash = store_and_relink(&compressed, &zst_path, cas_root)?;
        metadata.derived.insert("game_params.rkyv.zst".to_string(), hash);
    }

    // The web client fetches only the English catalog, zstd-compressed.
    let mo_rel = "translations/en/LC_MESSAGES/global.mo";
    let mo_path = build_dir.join(mo_rel);
    if mo_path.exists() {
        let mo_bytes =
            std::fs::read(&mo_path).attach_with(|| format!("Failed to read {}", mo_path.display()))?;
        let compressed = ruzstd::encoding::compress_to_vec(
            mo_bytes.as_slice(),
            ruzstd::encoding::CompressionLevel::Fastest,
        );
        let zst_path = build_dir.join(format!("{mo_rel}.zst"));
        let hash = store_and_relink(&compressed, &zst_path, cas_root)?;
        metadata.derived.insert(format!("{mo_rel}.zst"), hash);
    }

    Ok(())
}

/// Regenerate derived artifacts for every dumped build (or one build when
/// `only_build` is given), then garbage-collect CAS objects no longer
/// referenced by any build.
pub fn refresh_derived(output_base: &Path, only_build: Option<u32>) -> Result<(), Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let cas_root = output_base.join("vfs_common");

    let targets: Vec<&BuildEntry> = match only_build {
        Some(b) => index.builds.iter().filter(|e| e.build == b).collect(),
        None => index.builds.iter().collect(),
    };
    if targets.is_empty() {
        bail!("No matching builds found in {}", output_base.join("builds.toml").display());
    }

    for entry in &targets {
        let build_dir = output_base.join(&entry.dir);
        let meta_path = build_dir.join("metadata.toml");
        let mut metadata = BuildMetadata::load(&meta_path).unwrap_or(BuildMetadata {
            version: entry.version.clone(),
            build: entry.build,
            ..Default::default()
        });
        match refresh_build_derived(&build_dir, &cas_root, &mut metadata) {
            Ok(()) => {
                metadata.save(&meta_path)?;
                println!("  {} - {} derived artifact(s)", entry.dir, metadata.derived.len());
            }
            Err(e) => eprintln!("WARN: {} - failed to refresh derived data: {e:?}", entry.dir),
        }
    }

    println!(
        "Refreshed {} build(s). Replacing artifacts can leave orphaned CAS objects; \
         run `wows-data-mgr gc` to reclaim them.",
        targets.len()
    );
    Ok(())
}

/// Remove content-addressed objects no longer referenced by any build. An
/// object is live if it appears in some build's metadata (the extracted vfs
/// tree or the derived artifacts). Aborts without deleting anything if any
/// build's metadata cannot be read, so in-use objects are never removed.
pub fn gc_cas(output_base: &Path) -> Result<(), Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let cas_root = output_base.join("vfs_common");

    let mut live = std::collections::HashSet::new();
    for entry in &index.builds {
        let meta_path = output_base.join(&entry.dir).join("metadata.toml");
        let meta = BuildMetadata::load(&meta_path)
            .ok_or_else(|| report!("{} has no readable metadata.toml; aborting GC", entry.dir))?;
        live.extend(meta.referenced_hashes());
    }

    let removed = cas::gc(&cas_root, &live)?;
    println!("GC removed {removed} orphaned CAS object(s); {} still referenced.", live.len());
    Ok(())
}

fn extract_vfs_dir_cas(
    vfs: &VfsPath,
    vfs_path: &str,
    vfs_dir: &Path,
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
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
        let mut buf = Vec::new();
        match entry.open_file() {
            Ok(mut f) => f.read_to_end(&mut buf)?,
            Err(e) => {
                tracing::warn!("Failed to open VFS file {rel}: {e}");
                continue;
            }
        };
        store_and_link(&buf, rel, vfs_dir, cas_root, file_hashes)?;
        if let Some(pb) = progress {
            pb.inc(1);
        }
    }
    Ok(())
}

fn extract_vfs_file_cas(
    vfs: &VfsPath,
    vfs_path: &str,
    vfs_dir: &Path,
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
) -> Result<(), Report> {
    let file = match vfs.join(vfs_path) {
        Ok(f) => f,
        Err(_) => {
            tracing::warn!("VFS path not found (skipping): {vfs_path}");
            return Ok(());
        }
    };
    let mut buf = Vec::new();
    match file.open_file() {
        Ok(mut f) => f.read_to_end(&mut buf)?,
        Err(_) => {
            tracing::warn!("Could not open VFS file (skipping): {vfs_path}");
            return Ok(());
        }
    };
    store_and_link(&buf, vfs_path, vfs_dir, cas_root, file_hashes)?;
    Ok(())
}

fn extract_map_files_cas(
    vfs: &VfsPath,
    parent_dir: &str,
    filenames: &[&str],
    vfs_dir: &Path,
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
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
            let mut buf = Vec::new();
            match file_path.open_file() {
                Ok(mut f) => f.read_to_end(&mut buf)?,
                Err(e) => {
                    tracing::warn!("Failed to open VFS file {rel}: {e}");
                    continue;
                }
            };
            store_and_link(&buf, rel, vfs_dir, cas_root, file_hashes)?;
            if let Some(pb) = progress {
                pb.inc(1);
            }
        }
    }
    Ok(())
}

// -- Counting helpers (for progress bar) --

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
