use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rootcause::prelude::*;
use wowsunpack::game_data;
use wowsunpack::game_params::cache;
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
    "gui/crew_commander/skills",
    "gui/modernization_icons",
    "gui/signal_flags",
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

    dump_all_translations(game_dir, build, &output_dir)?;

    // Fetch and store versioned constants (non-fatal)
    #[cfg(feature = "constants")]
    match crate::constants::ConstantsFetcher::new() {
        Ok(fetcher) => {
            write_constants_for_build(&output_dir, build, &fetcher);
        }
        Err(e) => {
            tracing::warn!("Could not initialize constants fetcher for build {build}: {e:?}");
        }
    }

    // Write enhanced metadata with file hashes. The derived artifacts (rkyv
    // blob, compressed copies) are generated and content-addressed by the same
    // step the refresh-derived command uses, so dumps and refreshes agree.
    let mut metadata =
        BuildMetadata { version: version_str.to_string(), build, files: file_hashes, derived: BTreeMap::new() };
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

/// Convert `vfs_dir/content/GameParams.data` into a rkyv-encoded `Vec<Param>`
/// using the current `wowsunpack` schema. Returns `None` when the source file
/// is missing or the conversion fails (panic from a layout-incompatible older
/// pickle, or serialization error). Diagnostics are logged via stderr.
fn derive_game_params_rkyv(vfs_dir: &Path) -> Option<Vec<u8>> {
    if !vfs_dir.join("content/GameParams.data").exists() {
        return None;
    }
    let vfs = VfsPath::new(wowsunpack::vfs::PhysicalFS::new(vfs_dir));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let gmp = GameMetadataProvider::from_vfs(&vfs)?;
        let params: Vec<Param> = gmp.params().iter().map(|p| Arc::unwrap_or_clone(Arc::clone(p))).collect();
        cache::encode(&params).map_err(|e| report!("Failed to serialize: {e}"))
    }));
    match result {
        Ok(Ok(bytes)) => Some(bytes),
        Ok(Err(e)) => {
            eprintln!("WARN: GameParams re-derivation failed for {}: {e:?}", vfs_dir.display());
            None
        }
        Err(_) => {
            eprintln!("WARN: GameParams re-derivation panicked for {} (incompatible pickle format)", vfs_dir.display(),);
            None
        }
    }
}

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
/// The rkyv blob is derived from `vfs/content/GameParams.data` against the
/// current `wowsunpack::game_params::types` schema; the on-disk rkyv is only
/// consulted as a fallback when the extracted vfs is missing or conversion
/// fails. Each artifact is stored in the CAS, linked back into `build_dir`,
/// and recorded in `metadata.derived`. Idempotent.
pub fn refresh_build_derived(build_dir: &Path, cas_root: &Path, metadata: &mut BuildMetadata) -> Result<(), Report> {
    metadata.derived.clear();

    let rkyv_path = build_dir.join("game_params.rkyv");
    let rkyv_bytes = derive_game_params_rkyv(&build_dir.join("vfs"));
    let rkyv_bytes = match rkyv_bytes {
        Some(b) => Some(b),
        None if rkyv_path.exists() => {
            Some(std::fs::read(&rkyv_path).attach_with(|| format!("Failed to read {}", rkyv_path.display()))?)
        }
        None => None,
    };
    if let Some(rkyv_bytes) = rkyv_bytes {
        let hash = store_and_relink(&rkyv_bytes, &rkyv_path, cas_root)?;
        metadata.derived.insert("game_params.rkyv".to_string(), hash);

        let compressed =
            ruzstd::encoding::compress_to_vec(rkyv_bytes.as_slice(), ruzstd::encoding::CompressionLevel::Fastest);
        let zst_path = build_dir.join("game_params.rkyv.zst");
        let hash = store_and_relink(&compressed, &zst_path, cas_root)?;
        metadata.derived.insert("game_params.rkyv.zst".to_string(), hash);
    }

    // The web client fetches only the English catalog, zstd-compressed.
    let mo_rel = "translations/en/LC_MESSAGES/global.mo";
    let mo_path = build_dir.join(mo_rel);
    if mo_path.exists() {
        let mo_bytes = std::fs::read(&mo_path).attach_with(|| format!("Failed to read {}", mo_path.display()))?;
        let compressed =
            ruzstd::encoding::compress_to_vec(mo_bytes.as_slice(), ruzstd::encoding::CompressionLevel::Fastest);
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

    // Build a single fetcher up front so the GitHub listing only runs once
    // even when backfilling constants for many builds. `None` here just skips
    // the constants step rather than failing the whole refresh.
    #[cfg(feature = "constants")]
    let constants_fetcher = match crate::constants::ConstantsFetcher::new() {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("WARN: Could not initialize constants fetcher: {e:?}");
            None
        }
    };

    for entry in &targets {
        let build_dir = output_base.join(&entry.dir);
        let meta_path = build_dir.join("metadata.toml");
        let mut metadata = BuildMetadata::load(&meta_path).unwrap_or(BuildMetadata {
            version: entry.version.clone(),
            build: entry.build,
            ..Default::default()
        });

        #[cfg(feature = "constants")]
        let constants_added = if let Some(fetcher) = constants_fetcher.as_ref() {
            update_constants_if_missing(&build_dir, entry.build, fetcher)
        } else {
            false
        };

        match refresh_build_derived(&build_dir, &cas_root, &mut metadata) {
            Ok(()) => {
                metadata.save(&meta_path)?;
                #[cfg(feature = "constants")]
                let constants_note = if constants_added { " + constants" } else { "" };
                #[cfg(not(feature = "constants"))]
                let constants_note = "";
                println!("  {} - {} derived artifact(s){}", entry.dir, metadata.derived.len(), constants_note);
            }
            Err(e) => eprintln!("WARN: {} - failed to refresh derived data: {e:?}", entry.dir),
        }
    }

    println!("Refreshed {} build(s).", targets.len());
    Ok(())
}

/// Write `constants.json` into `build_dir` if upstream has constants for this
/// build. Logs a warning and leaves the build alone when nothing is published
/// (e.g. very old builds the wows-constants repo doesn't cover).
#[cfg(feature = "constants")]
fn write_constants_for_build(build_dir: &Path, build: u32, fetcher: &crate::constants::ConstantsFetcher) -> bool {
    let Some((data, actual_build)) = fetcher.fetch(build) else {
        tracing::warn!("No upstream constants available for build {build}");
        return false;
    };
    let bytes = match serde_json::to_vec_pretty(&data) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Failed to serialize constants for build {build}: {e}");
            return false;
        }
    };
    if let Err(e) = std::fs::write(build_dir.join("constants.json"), &bytes) {
        tracing::warn!("Failed to write constants.json for build {build}: {e}");
        return false;
    }
    if actual_build != build {
        tracing::info!("Stored constants from build {actual_build} (fallback for {build})");
    }
    true
}

/// Fetch and write `constants.json` only when the build doesn't already have
/// one. Returns `true` if a new file was written. Constants for already-shipped
/// builds don't change upstream, so leaving existing files alone keeps repeat
/// refreshes idempotent and fast.
#[cfg(feature = "constants")]
fn update_constants_if_missing(build_dir: &Path, build: u32, fetcher: &crate::constants::ConstantsFetcher) -> bool {
    if build_dir.join("constants.json").exists() {
        return false;
    }
    write_constants_for_build(build_dir, build, fetcher)
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

/// Remove content-addressed objects no longer referenced by any build present
/// on disk. Scans every directory under `output_base` that contains a
/// `metadata.toml`, so it stays correct even when `builds.toml` is out of sync
/// (e.g. a build directory was deleted manually without GC). Aborts without
/// removing anything if any metadata file cannot be read, so in-use objects are
/// never deleted. Returns the number of objects removed.
pub fn gc_unreferenced(output_base: &Path) -> Result<usize, Report> {
    let cas_root = output_base.join("vfs_common");
    if !cas_root.exists() {
        return Ok(0);
    }

    let mut live = std::collections::HashSet::new();
    for entry in std::fs::read_dir(output_base)
        .attach_with(|| format!("Failed to read {}", output_base.display()))?
        .flatten()
    {
        let meta_path = entry.path().join("metadata.toml");
        if !meta_path.exists() {
            continue;
        }
        match BuildMetadata::load(&meta_path) {
            Some(meta) => live.extend(meta.referenced_hashes()),
            None => bail!("unreadable metadata at {}; aborting GC", meta_path.display()),
        }
    }

    cas::gc(&cas_root, &live)
}

/// Migrate any pre-CAS dumps in `output_base` into content-addressed storage.
///
/// Older dumps stored the extracted `vfs/` tree as plain files with no entries
/// in `metadata.files`. This rehashes those files into `vfs_common/`, replaces
/// them with symlinks, records the hashes, and regenerates derived artifacts so
/// the dump deduplicates against every other build. Returns the number of
/// builds migrated. Builds already in CAS format are left untouched.
pub fn migrate_to_cas(output_base: &Path) -> Result<usize, Report> {
    let cas_root = output_base.join("vfs_common");
    let mut migrated = 0;
    for entry in std::fs::read_dir(output_base)
        .attach_with(|| format!("Failed to read {}", output_base.display()))?
        .flatten()
    {
        let build_dir = entry.path();
        let meta_path = build_dir.join("metadata.toml");
        let Some(mut metadata) = BuildMetadata::load(&meta_path) else {
            continue;
        };
        if metadata.has_file_hashes() || !build_dir.join("vfs").exists() {
            continue;
        }
        match migrate_build_to_cas(&build_dir, &cas_root, &mut metadata) {
            Ok(()) => {
                metadata.save(&meta_path)?;
                migrated += 1;
            }
            Err(e) => tracing::warn!("failed to migrate {} to CAS: {e}", build_dir.display()),
        }
    }
    Ok(migrated)
}

/// Rehash a single pre-CAS build's `vfs/` tree into the CAS, replacing each
/// plain file with a symlink and recording its hash in `metadata.files`.
fn migrate_build_to_cas(build_dir: &Path, cas_root: &Path, metadata: &mut BuildMetadata) -> Result<(), Report> {
    let vfs_dir = build_dir.join("vfs");
    let mut stack = vec![vfs_dir.clone()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).attach_with(|| format!("Failed to read {}", dir.display()))?.flatten()
        {
            let path = entry.path();
            let file_type = entry.file_type().attach_with(|| format!("Failed to stat {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            // Symlinks are already-migrated CAS references; leave them alone.
            if file_type.is_symlink() {
                continue;
            }
            let rel = path.strip_prefix(&vfs_dir).expect("walked path is under vfs_dir").to_string_lossy().replace('\\', "/");
            let data = std::fs::read(&path).attach_with(|| format!("Failed to read {}", path.display()))?;
            let hash = cas::store(cas_root, &data)?;
            std::fs::remove_file(&path).attach_with(|| format!("Failed to remove {}", path.display()))?;
            cas::link_file(cas_root, &hash, &path)?;
            metadata.files.insert(rel, hash);
        }
    }
    refresh_build_derived(build_dir, cas_root, metadata)
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

#[cfg(test)]
mod maintenance_tests {
    use super::*;

    fn write_build_metadata(build_dir: &Path, version: &str, build: u32, files: &[(&str, &str)]) {
        std::fs::create_dir_all(build_dir).unwrap();
        let mut meta = BuildMetadata { version: version.to_string(), build, ..Default::default() };
        for (rel, hash) in files {
            meta.files.insert((*rel).to_string(), (*hash).to_string());
        }
        meta.save(&build_dir.join("metadata.toml")).unwrap();
    }

    #[test]
    fn gc_unreferenced_removes_orphans_and_keeps_live() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join("vfs_common");

        let live_hash = cas::store(&cas_root, b"live object").unwrap();
        let orphan_hash = cas::store(&cas_root, b"orphan object").unwrap();
        write_build_metadata(&base.join("1.0.0_100"), "1.0.0", 100, &[("gui/a.png", &live_hash)]);

        let removed = gc_unreferenced(base).unwrap();
        assert_eq!(removed, 1);
        assert!(cas::object_exists(&cas_root, &live_hash));
        assert!(!cas::object_exists(&cas_root, &orphan_hash));
    }

    #[test]
    fn gc_unreferenced_aborts_on_unreadable_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join("vfs_common");
        let orphan_hash = cas::store(&cas_root, b"orphan object").unwrap();

        let build_dir = base.join("1.0.0_100");
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(build_dir.join("metadata.toml"), b"this is not valid toml = =").unwrap();

        assert!(gc_unreferenced(base).is_err());
        // Nothing was removed because GC aborted.
        assert!(cas::object_exists(&cas_root, &orphan_hash));
    }

    #[test]
    fn migrate_to_cas_dedups_plain_files() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let build_dir = base.join("1.0.0_100");

        // Old-format dump: plain files in vfs/, no file hashes in metadata.
        write_build_metadata(&build_dir, "1.0.0", 100, &[]);
        let file_path = build_dir.join("vfs/gui/a.png");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, b"some asset bytes").unwrap();

        let migrated = migrate_to_cas(base).unwrap();
        assert_eq!(migrated, 1);

        // The plain file is now a symlink whose content still reads back.
        assert!(std::fs::symlink_metadata(&file_path).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&file_path).unwrap(), b"some asset bytes");

        // Metadata now records the hash, and a second pass is a no-op.
        let meta = BuildMetadata::load(&build_dir.join("metadata.toml")).unwrap();
        assert!(meta.has_file_hashes());
        assert!(meta.files.contains_key("gui/a.png"));
        assert_eq!(migrate_to_cas(base).unwrap(), 0);
    }
}
