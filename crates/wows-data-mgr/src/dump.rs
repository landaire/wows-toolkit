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
use crate::builds::CorruptObject;
use crate::cas;

/// VFS directories dumped in their entirety. `gui/battle_hud` is dumped whole
/// (rather than enumerating subdirectories) so newly-added HUD icons are always
/// captured without code changes.
const VFS_DIRS: &[&str] = &[
    "gui/fla/minimap",
    "gui/battle_hud",
    "gui/consumables",
    "gui/powerups/drops",
    "gui/fonts",
    "gui/data/constants",
    "gui/ships_silhouettes",
    "gui/ribbons",
    "gui/achievements",
    "gui/nation_flags",
    "gui/crew_commander/skills",
    "gui/modernization_icons",
    "gui/signal_flags",
    "scripts/entity_defs",
];

/// Directories whose absence (zero files extracted) makes the dump unusable.
/// Kept deliberately small: most icon directories are version-dependent (added
/// in later game versions), so their absence is tolerated with a warning.
const REQUIRED_NONEMPTY_DIRS: &[&str] = &["scripts/entity_defs"];

/// Individual VFS files required beyond the directory dumps. A dump missing any
/// of these can't parse replays, so their absence is fatal.
const REQUIRED_VFS_FILES: &[&str] = &["content/GameParams.data", "scripts/entities.xml"];

/// Files to extract per map from `spaces/<map>/`.
const MAP_FILES_SPACES: &[&str] = &["minimap.png", "minimap_water.png", "space.settings"];

/// Files to extract per map from `content/gameplay/<map>/`.
const MAP_FILES_GAMEPLAY: &[&str] = &["space.settings"];

/// Glob patterns covering every VFS path the dump extracts.
///
/// Feed these to `wowsunpack pkgs` to resolve the minimal set of `.pkg` files
/// to download for a build — letting callers fetch all idx (small) first, then
/// only the packages actually required, instead of the full multi-GiB depots.
pub fn required_path_globs() -> Vec<String> {
    let mut globs = Vec::new();
    for dir in VFS_DIRS {
        // `wowsunpack pkgs` matches with the glob crate's default options, where
        // `*` spans `/`, so `{dir}/*` matches every file under the tree.
        globs.push(format!("{dir}/*"));
    }
    for file in REQUIRED_VFS_FILES {
        globs.push((*file).to_string());
    }
    for name in MAP_FILES_SPACES {
        globs.push(format!("spaces/*/{name}"));
    }
    for name in MAP_FILES_GAMEPLAY {
        globs.push(format!("content/gameplay/*/{name}"));
    }
    globs
}

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
/// Every file (VFS content, translation catalogs, derived artifacts) is
/// stored in `{output_base}/common/` by hash and recorded in `metadata.toml`.
/// No tree is written into the build directory; materializing one is
/// `relink`'s job.
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
    let cas_root = cas::cas_root(output_base);

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

    let mut dir_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for dir in VFS_DIRS {
        let count = extract_vfs_dir_cas(&vfs, dir, &cas_root, &mut file_hashes, progress)?;
        dir_counts.insert(dir, count);
    }
    let mut missing_files = Vec::new();
    for file in REQUIRED_VFS_FILES {
        if !extract_vfs_file_cas(&vfs, file, &cas_root, &mut file_hashes)? {
            missing_files.push(*file);
        }
        if let Some(pb) = progress {
            pb.inc(1);
        }
    }
    let map_count = extract_map_files_cas(&vfs, "spaces", MAP_FILES_SPACES, &cas_root, &mut file_hashes, progress)?;
    extract_map_files_cas(&vfs, "content/gameplay", MAP_FILES_GAMEPLAY, &cas_root, &mut file_hashes, progress)?;

    if let Some(pb) = progress {
        pb.finish_and_clear();
    }

    // Fail loudly on an incomplete dump rather than silently shipping one that
    // renders blank maps or can't parse replays. CAS objects already written
    // are shared and harmless; only the (unregistered) build dir is discarded.
    let mut problems = Vec::new();
    if !missing_files.is_empty() {
        problems.push(format!("missing required file(s): {}", missing_files.join(", ")));
    }
    for dir in REQUIRED_NONEMPTY_DIRS {
        if dir_counts.get(dir).copied().unwrap_or(0) == 0 {
            problems.push(format!("required directory '{dir}' extracted no files"));
        }
    }
    if map_count == 0 {
        problems.push(
            "no map data extracted (spaces/*/minimap.png); the content depot's spaces packages are likely missing"
                .to_string(),
        );
    }
    if !problems.is_empty() {
        let _ = std::fs::remove_dir_all(&output_dir);
        bail!("Incomplete dump for build {build} ({version_str}): {}", problems.join("; "));
    }
    for dir in VFS_DIRS {
        if dir_counts.get(dir).copied().unwrap_or(0) == 0 {
            tracing::warn!("dump for build {build}: directory '{dir}' extracted no files");
        }
    }

    std::fs::create_dir_all(&output_dir)
        .attach_with(|| format!("Failed to create output directory {}", output_dir.display()))?;

    let mut metadata = BuildMetadata {
        version: version_str.to_string(),
        build,
        // Extraction stores into common/ and records here. Materializing a
        // tree is the link step's job.
        materialized: false,
        files: file_hashes,
        derived: BTreeMap::new(),
    };

    dump_all_translations(game_dir, build, &cas_root, &mut metadata.derived)?;

    // Fetch and store versioned constants (non-fatal)
    #[cfg(feature = "constants")]
    match crate::constants::ConstantsFetcher::new() {
        Ok(fetcher) => {
            write_constants_for_build(&output_dir, build, Some(version_str), &fetcher);
        }
        Err(e) => {
            tracing::warn!("Could not initialize constants fetcher for build {build}: {e:?}");
        }
    }

    // The derived artifacts (rkyv blob, compressed copies) are generated and
    // content-addressed by the same step the refresh-derived command uses, so
    // dumps and refreshes agree.
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

/// Add the assets an existing build is missing without re-extracting the data it
/// already has. Extracts maps (and, with `with_gui`, the `gui/` asset dirs) from
/// `game_dir` into the store and records them, then regenerates derived
/// artifacts (the rkyv game-params blob, with the current parser) from the
/// build's existing `GameParams.data`. The build's tree, if it has one, is
/// relinked to pick up the new entries; a build with no tree stays that way.
///
/// Unlike [`dump_renderer_data`], this never reads `content/GameParams.data`,
/// `scripts/`, or other already-present data from the game install, so the caller
/// only needs the `gui` and `spaces_*` packages on disk -- not the multi-gigabyte
/// `basecontent` package whose `GameParams.data` the build already holds.
///
/// The build must already exist in `builds.toml`. Returns the number of maps
/// extracted; errors if no maps were found.
pub fn complete_build(game_dir: &Path, build: u32, output_base: &Path, with_gui: bool) -> Result<usize, Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let entry = index
        .find_by_build(build)
        .ok_or_else(|| report!("build {build} is not in builds.toml; dump it normally first"))?
        .clone();
    let output_dir = output_base.join(&entry.dir);
    let cas_root = cas::cas_root(output_base);
    let meta_path = output_dir.join("metadata.toml");
    let mut metadata =
        BuildMetadata::load(&meta_path).ok_or_else(|| report!("{} has no readable metadata.toml", entry.dir))?;

    let vfs = game_data::build_game_vfs_for_build(game_dir, build).attach_with(|| "Failed to build game VFS")?;

    if with_gui {
        // Only the `gui/` dirs live in the gui package; re-extracting other
        // VFS_DIRS (e.g. scripts/entity_defs) would need packages we deliberately
        // skip, and that data is already present in the build.
        for dir in VFS_DIRS.iter().filter(|d| d.starts_with("gui")) {
            extract_vfs_dir_cas(&vfs, dir, &cas_root, &mut metadata.files, None)?;
        }
    }

    let map_count = extract_map_files_cas(&vfs, "spaces", MAP_FILES_SPACES, &cas_root, &mut metadata.files, None)?;
    extract_map_files_cas(&vfs, "content/gameplay", MAP_FILES_GAMEPLAY, &cas_root, &mut metadata.files, None)?;
    if map_count == 0 {
        bail!("no maps extracted for build {build}; refusing to record an incomplete build");
    }

    // Regenerate derived artifacts from the build's existing GameParams.data,
    // picking up the current parser. Needs no package downloads.
    refresh_build_derived(&output_dir, &cas_root, &mut metadata)?;
    if metadata.materialized {
        relink_build(&output_dir, &cas_root, &metadata)?;
    } else {
        tracing::warn!(
            "build {build} is not materialized, so the newly extracted files were not linked into its tree; \
             run `relink` to materialize it",
            build = metadata.build
        );
    }
    metadata.save(&meta_path)?;
    Ok(map_count)
}

/// Create a configured progress bar for CLI use.
pub fn create_progress_bar(game_dir: &Path) -> Option<ProgressBar> {
    let vfs = game_data::build_game_vfs(game_dir).ok()?;
    let mut total_files = 0u64;
    for dir in VFS_DIRS {
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
        let cas_root = cas::cas_root(output_base);
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

// -- Local sync (offline mirror of download_repo) --

/// Which builds to copy when syncing from a local source dump base.
pub enum SyncSelector {
    /// Every build listed in the source `builds.toml`.
    All,
    /// Only the highest build number in the source.
    Latest,
    /// A single exact build number.
    Build(u32),
    /// All builds matching a `major.minor.patch` version string.
    Version(String),
}

/// Outcome of copying one build during a sync.
pub struct SyncedBuild {
    pub build: u32,
    pub version: String,
    /// Whether content was copied (`false` means it was already present and skipped).
    pub copied: bool,
}

/// Copy builds from a local source dump base into `output_base`, deduplicating
/// against content already present in the destination CAS.
///
/// This is the offline analog of `download_repo::download_build`: it reconstructs
/// each selected build's directory (vfs/derived symlinks, constants, metadata)
/// from the source's content-addressed store with no network access, copying only
/// the content objects the destination is missing. Useful for promoting a
/// freshly-dumped build into the toolkit's data cache without publishing it.
pub fn sync_from_local(
    source_base: &Path,
    output_base: &Path,
    selector: &SyncSelector,
    force: bool,
) -> Result<Vec<SyncedBuild>, Report> {
    if source_base == output_base {
        bail!("source and destination are the same directory");
    }
    let source_index = BuildsIndex::load(&source_base.join("builds.toml"));
    if source_index.builds.is_empty() {
        bail!("no builds.toml entries found in source {}", source_base.display());
    }

    let entries: Vec<BuildEntry> = match selector {
        SyncSelector::All => source_index.builds.clone(),
        SyncSelector::Latest => {
            let latest =
                source_index.builds.iter().max_by_key(|e| e.build).ok_or_else(|| report!("source has no builds"))?;
            vec![latest.clone()]
        }
        SyncSelector::Build(b) => {
            let entry = source_index
                .find_by_build(*b)
                .ok_or_else(|| report!("build {b} not found in source {}", source_base.display()))?;
            vec![entry.clone()]
        }
        SyncSelector::Version(v) => {
            let matches: Vec<BuildEntry> = source_index.find_by_version(v).into_iter().cloned().collect();
            if matches.is_empty() {
                bail!("no builds matching version '{v}' in source {}", source_base.display());
            }
            matches
        }
    };

    let mut synced = Vec::new();
    for entry in &entries {
        let copied = copy_build_from_local(source_base, output_base, entry, force)?;
        synced.push(SyncedBuild { build: entry.build, version: entry.version.clone(), copied });
    }
    Ok(synced)
}

/// Copy one build's data from `source_base` into `output_base`. Returns `true`
/// when content was copied, `false` when an existing complete copy was reused.
fn copy_build_from_local(
    source_base: &Path,
    output_base: &Path,
    entry: &BuildEntry,
    force: bool,
) -> Result<bool, Report> {
    let src_cas = cas::cas_root(source_base);
    let dst_cas = cas::cas_root(output_base);
    let output_dir = output_base.join(&entry.dir);

    // A complete copy already on disk only needs registering, unless forced.
    if !force && output_dir.join("metadata.toml").exists() {
        register_build(output_base, entry)?;
        return Ok(false);
    }

    let src_dir = source_base.join(&entry.dir);
    let meta_path = src_dir.join("metadata.toml");
    let metadata = BuildMetadata::load(&meta_path)
        .ok_or_else(|| report!("source build {} has no readable metadata.toml", entry.dir))?;

    // Copy every referenced content object the destination doesn't already have,
    // verifying each against its hash. This happens before touching the existing
    // build directory so a failed copy (e.g. an inconsistent source) never
    // destroys a good local copy. Objects land in the shared store; nothing is
    // wired into the build until every object is present.
    for hash in metadata.referenced_hashes() {
        if cas::object_exists(&dst_cas, &hash) {
            continue;
        }
        let src_obj = cas::cas_path(&src_cas, &hash);
        let data =
            std::fs::read(&src_obj).attach_with(|| format!("source content object {} missing", src_obj.display()))?;
        let actual = cas::hash_bytes(&data);
        if actual != hash {
            bail!("source content object {hash} hashed to {actual}");
        }
        cas::store(&dst_cas, &data)?;
    }

    // All content is present; now clear any partial/stale directory and rebuild.
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)
            .attach_with(|| format!("failed to clear destination build dir {}", output_dir.display()))?;
    }
    std::fs::create_dir_all(&output_dir)
        .attach_with(|| format!("failed to create destination build dir {}", output_dir.display()))?;

    // A copy is as materialized as its source claimed to be; nothing else here
    // creates a tree.
    if metadata.materialized {
        let vfs_dir = output_dir.join("vfs");
        for (rel, hash) in &metadata.files {
            cas::link_file(&dst_cas, hash, &vfs_dir.join(rel))?;
        }
        for (rel, hash) in &metadata.derived {
            cas::link_file(&dst_cas, hash, &output_dir.join(rel))?;
        }
    } else {
        tracing::debug!("build {} is not materialized; copy stores objects only", entry.build);
    }

    // Versioned constants, when present alongside the source build.
    let src_constants = src_dir.join("constants.json");
    if src_constants.exists() {
        let bytes =
            std::fs::read(&src_constants).attach_with(|| format!("failed to read {}", src_constants.display()))?;
        let dest = output_dir.join("constants.json");
        std::fs::create_dir_all(dest.parent().unwrap())?;
        std::fs::write(&dest, &bytes).attach_with(|| format!("failed to write {}", dest.display()))?;
    }

    metadata.save(&output_dir.join("metadata.toml"))?;
    register_build(output_base, entry)?;
    Ok(true)
}

/// Add or update the build's entry in the destination `builds.toml`.
fn register_build(output_base: &Path, entry: &BuildEntry) -> Result<(), Report> {
    let builds_path = output_base.join("builds.toml");
    let mut index = BuildsIndex::load(&builds_path);
    index.upsert(entry.clone());
    index.save(&builds_path)
}

// -- Translation dumping --

/// Store every locale's message catalog from the install in the CAS and record
/// it in `derived`. The catalogs are copies of game files rather than
/// derivations, which is why nothing else can regenerate them.
fn dump_all_translations(
    game_dir: &Path,
    build: u32,
    cas_root: &Path,
    derived: &mut BTreeMap<String, String>,
) -> Result<(), Report> {
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
        let lang = entry.file_name().to_string_lossy().into_owned();
        let mo_src = entry.path().join("LC_MESSAGES/global.mo");
        if !mo_src.exists() {
            continue;
        }
        let bytes = std::fs::read(&mo_src).attach_with(|| format!("Failed to read {}", mo_src.display()))?;
        let hash = cas::store(cas_root, &bytes)?;
        derived.insert(format!("translations/{lang}/LC_MESSAGES/global.mo"), hash);
        if lang == "en" {
            // The web client fetches only the English catalog, zstd-compressed.
            let compressed =
                ruzstd::encoding::compress_to_vec(bytes.as_slice(), ruzstd::encoding::CompressionLevel::Fastest);
            let hash = cas::store(cas_root, &compressed)?;
            derived.insert("translations/en/LC_MESSAGES/global.mo.zst".to_string(), hash);
        }
    }
    Ok(())
}

// -- CAS-aware extraction helpers --

/// Read a VFS file into the CAS and record its hash in the manifest. The bytes
/// are the store's; `metadata.files` is the record that they belong to this
/// build.
fn store_and_record(
    data: &[u8],
    rel_path: &str,
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
) -> Result<(), Report> {
    let hash = cas::store(cas_root, data)?;
    file_hashes.insert(rel_path.trim_start_matches('/').to_string(), hash);
    Ok(())
}

// -- Derived artifact generation (shared by dump and refresh-derived) --

/// Convert `content/GameParams.data` read through `vfs` into a rkyv-encoded
/// `Vec<Param>` using the current `wowsunpack` schema. Returns `None` when the
/// source file is missing or the conversion fails (panic from a
/// layout-incompatible older pickle, or serialization error). Diagnostics are
/// logged via stderr.
fn derive_game_params_rkyv(vfs: &VfsPath) -> Option<Vec<u8>> {
    if !vfs.join("content/GameParams.data").ok()?.exists().unwrap_or(false) {
        return None;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let gmp = GameMetadataProvider::from_vfs(vfs)?;
        let params: Vec<Param> = gmp.params().iter().map(|p| Arc::unwrap_or_clone(Arc::clone(p))).collect();
        cache::encode(&params).map_err(|e| report!("Failed to serialize: {e}"))
    }));
    match result {
        Ok(Ok(bytes)) => Some(bytes),
        Ok(Err(e)) => {
            eprintln!("WARN: GameParams re-derivation failed: {e:?}");
            None
        }
        Err(_) => {
            eprintln!("WARN: GameParams re-derivation panicked (incompatible pickle format)");
            None
        }
    }
}

/// Store `data` in the CAS and return its hash. Nothing is written to the build
/// directory: `metadata` is the record of what a build holds.
fn store_only(data: &[u8], cas_root: &Path) -> Result<String, Report> {
    cas::store(cas_root, data)
}

/// Point `link_path` at the stored object, replacing whatever is there. Only
/// for a build whose metadata says it is materialized.
fn relink_artifact(hash: &str, link_path: &Path, cas_root: &Path) -> Result<(), Report> {
    let _ = std::fs::remove_file(link_path);
    cas::link_file(cas_root, hash, link_path)
}

/// Generate and content-address a build's derived artifacts: the rkyv game
/// params blob, its zstd copy, and the English translation catalog's zstd copy.
/// The rkyv blob is derived from `content/GameParams.data` read through
/// `resolve_vfs` (the manifest-backed store for a CAS-format build, the
/// physical `vfs/` tree for a legacy one) against the current
/// `wowsunpack::game_params::types` schema; the on-disk rkyv is only
/// consulted as a fallback when that derivation fails, whether because the
/// source file is absent or because conversion errored or panicked. Each
/// artifact is stored in the CAS and recorded in `metadata.derived`; it is also
/// linked back into `build_dir` when `metadata.materialized` is set. Idempotent.
///
/// Artifacts that cannot be regenerated keep the entry they already had:
/// `metadata.toml`, not the build directory, is the record of what a build
/// owns, and a build whose materialised tree has been pruned has every artifact
/// in the store and none on disk. Returns the recorded paths whose backing
/// object is missing from the store, which means the store is damaged.
pub fn refresh_build_derived(
    build_dir: &Path,
    cas_root: &Path,
    metadata: &mut BuildMetadata,
) -> Result<Vec<String>, Report> {
    let recorded = std::mem::take(&mut metadata.derived);
    let materialized = metadata.materialized;

    let rkyv_path = build_dir.join("game_params.rkyv");
    let rkyv_bytes = derive_game_params_rkyv(&crate::cas_vfs::resolve_vfs(build_dir, cas_root, metadata));
    let rkyv_bytes = match rkyv_bytes {
        Some(b) => Some(b),
        None if rkyv_path.exists() => {
            Some(std::fs::read(&rkyv_path).attach_with(|| format!("Failed to read {}", rkyv_path.display()))?)
        }
        None => None,
    };
    if let Some(rkyv_bytes) = rkyv_bytes {
        let hash = store_only(&rkyv_bytes, cas_root)?;
        if materialized {
            relink_artifact(&hash, &rkyv_path, cas_root)?;
        }
        metadata.derived.insert("game_params.rkyv".to_string(), hash);

        let compressed =
            ruzstd::encoding::compress_to_vec(rkyv_bytes.as_slice(), ruzstd::encoding::CompressionLevel::Fastest);
        let zst_path = build_dir.join("game_params.rkyv.zst");
        let hash = store_only(&compressed, cas_root)?;
        if materialized {
            relink_artifact(&hash, &zst_path, cas_root)?;
        }
        metadata.derived.insert("game_params.rkyv.zst".to_string(), hash);
    } else if recorded.contains_key("game_params.rkyv") {
        tracing::warn!(
            "build {}: kept the recorded game_params.rkyv; it could not be re-derived from this build's GameParams.data",
            metadata.build
        );
    }

    // Content-address the per-locale translation catalogs (raw .mo files copied
    // from the game install). They are identical across many builds, so this
    // deduplicates them into the shared store like every other asset.
    let translations_dir = build_dir.join("translations");
    if translations_dir.exists() {
        for lang_entry in std::fs::read_dir(&translations_dir)
            .attach_with(|| format!("Failed to read {}", translations_dir.display()))?
            .flatten()
        {
            if !lang_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let lang = lang_entry.file_name().to_string_lossy().into_owned();
            let mo_rel = format!("translations/{lang}/LC_MESSAGES/global.mo");
            let mo_path = build_dir.join(&mo_rel);
            if !mo_path.exists() {
                continue;
            }
            let bytes = std::fs::read(&mo_path).attach_with(|| format!("Failed to read {}", mo_path.display()))?;
            let hash = store_only(&bytes, cas_root)?;
            if materialized {
                relink_artifact(&hash, &mo_path, cas_root)?;
            }
            metadata.derived.insert(mo_rel, hash);
        }
    }

    // The web client fetches only the English catalog, zstd-compressed.
    let mo_rel = "translations/en/LC_MESSAGES/global.mo";
    let mo_path = build_dir.join(mo_rel);
    if mo_path.exists() {
        let mo_bytes = std::fs::read(&mo_path).attach_with(|| format!("Failed to read {}", mo_path.display()))?;
        let compressed =
            ruzstd::encoding::compress_to_vec(mo_bytes.as_slice(), ruzstd::encoding::CompressionLevel::Fastest);
        let zst_path = build_dir.join(format!("{mo_rel}.zst"));
        let hash = store_only(&compressed, cas_root)?;
        if materialized {
            relink_artifact(&hash, &zst_path, cas_root)?;
        }
        metadata.derived.insert(format!("{mo_rel}.zst"), hash);
    }

    let mut missing_objects = Vec::new();
    for (rel, hash) in recorded {
        if metadata.derived.contains_key(&rel) {
            continue;
        }
        if !cas::object_exists(cas_root, &hash) {
            missing_objects.push(rel.clone());
        }
        metadata.derived.insert(rel, hash);
    }
    missing_objects.sort();

    Ok(missing_objects)
}

/// What a refresh across a dump base found.
#[derive(Debug, Default)]
pub struct RefreshSummary {
    /// Builds the refresh ran over.
    pub builds: usize,
    /// Build directory -> recorded derived paths whose backing object is not in
    /// the store.
    pub damaged: BTreeMap<String, Vec<String>>,
    /// Build directories whose refresh failed outright.
    pub failed: Vec<String>,
}

impl RefreshSummary {
    /// Whether the metadata across the base is a trustworthy statement of what
    /// the store must keep. GC deletes every object no build references, so
    /// collecting against a record known to be damaged or incomplete turns a
    /// recoverable inconsistency into deleted content.
    pub fn safe_to_gc(&self) -> bool {
        self.damaged.is_empty() && self.failed.is_empty()
    }
}

/// Regenerate derived artifacts for every dumped build (or one build when
/// `only_build` is given). The caller decides whether to garbage-collect
/// afterwards; consult [`RefreshSummary::safe_to_gc`] first.
pub fn refresh_derived(output_base: &Path, only_build: Option<u32>) -> Result<RefreshSummary, Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let cas_root = cas::cas_root(output_base);

    let targets: Vec<&BuildEntry> = match only_build {
        Some(b) => index.builds.iter().filter(|e| e.build == b).collect(),
        None => index.builds.iter().collect(),
    };
    if targets.is_empty() {
        bail!("No matching builds found in {}", output_base.join("builds.toml").display());
    }

    // One fetcher serves every build so the GitHub listing runs at most once,
    // and it is only built once a build actually needs constants: a base whose
    // builds all have theirs stays offline. `None` skips the constants step
    // rather than failing the whole refresh.
    #[cfg(feature = "constants")]
    let mut constants_fetcher: Option<crate::constants::ConstantsFetcher> = None;
    #[cfg(feature = "constants")]
    let mut fetcher_failed = false;

    let mut summary = RefreshSummary { builds: targets.len(), ..Default::default() };

    for entry in &targets {
        let build_dir = output_base.join(&entry.dir);
        let meta_path = build_dir.join("metadata.toml");
        let mut metadata = BuildMetadata::load(&meta_path).unwrap_or(BuildMetadata {
            version: entry.version.clone(),
            build: entry.build,
            ..Default::default()
        });

        #[cfg(feature = "constants")]
        let constants_added = if build_dir.join("constants.json").exists() {
            false
        } else {
            if constants_fetcher.is_none() && !fetcher_failed {
                match crate::constants::ConstantsFetcher::new() {
                    Ok(f) => constants_fetcher = Some(f),
                    Err(e) => {
                        eprintln!("WARN: Could not initialize constants fetcher: {e:?}");
                        fetcher_failed = true;
                    }
                }
            }
            match constants_fetcher.as_ref() {
                Some(fetcher) => {
                    write_constants_for_build(&build_dir, entry.build, Some(entry.version.as_str()), fetcher)
                }
                None => false,
            }
        };

        match refresh_build_derived(&build_dir, &cas_root, &mut metadata) {
            Ok(missing_objects) => {
                metadata.save(&meta_path)?;
                #[cfg(feature = "constants")]
                let constants_note = if constants_added { " + constants" } else { "" };
                #[cfg(not(feature = "constants"))]
                let constants_note = "";
                println!("  {} - {} derived artifact(s){}", entry.dir, metadata.derived.len(), constants_note);
                if !missing_objects.is_empty() {
                    eprintln!(
                        "WARN: {} - {} recorded artifact(s) have no object in the store, starting with {}",
                        entry.dir,
                        missing_objects.len(),
                        missing_objects[0]
                    );
                    summary.damaged.insert(entry.dir.clone(), missing_objects);
                }
                if !metadata.materialized {
                    tracing::warn!(
                        "build {} is not materialized, so its tree was not updated; run `relink` to materialize it",
                        metadata.build
                    );
                }
            }
            Err(e) => {
                eprintln!("WARN: {} - failed to refresh derived data: {e:?}", entry.dir);
                summary.failed.push(entry.dir.clone());
            }
        }
    }

    println!("Refreshed {} build(s).", summary.builds);
    Ok(summary)
}

/// Write `constants.json` into `build_dir` if upstream has constants for this
/// build. Logs a warning and leaves the build alone when nothing is published
/// (e.g. very old builds the wows-constants repo doesn't cover).
#[cfg(feature = "constants")]
fn write_constants_for_build(
    build_dir: &Path,
    build: u32,
    version: Option<&str>,
    fetcher: &crate::constants::ConstantsFetcher,
) -> bool {
    let Some((data, actual_build)) = fetcher.fetch(build, version) else {
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

/// Sync a build tree to `metadata.toml`: every indexed path gets a link at the
/// object the entry names, and every link the manifest no longer names is
/// removed. Paths whose object is not in the store are left alone, since a
/// link to nothing is what `verify` already reports. Returns the paths that
/// had to be repointed.
pub fn relink_build(build_dir: &Path, cas_root: &Path, metadata: &BuildMetadata) -> Result<Vec<String>, Report> {
    let mut repointed = Vec::new();
    for (rel, hash) in metadata.files.iter().chain(metadata.derived.iter()) {
        if !cas::object_exists(cas_root, hash) {
            continue;
        }
        // `files` materialise under vfs/, `derived` at the build root.
        let link = if metadata.files.contains_key(rel) { build_dir.join("vfs").join(rel) } else { build_dir.join(rel) };
        if cas::link_target_hash(&link).as_ref() == Some(hash) && link.exists() {
            continue;
        }
        cas::link_file(cas_root, hash, &link)?;
        repointed.push(rel.clone());
    }
    repointed.sort();

    // A tree is a materialization of metadata.toml, so relinking is a sync in
    // both directions: a path the manifest dropped keeps no link. Only links
    // that resolve into THIS build's store are removed; real files, and
    // symlinks aimed elsewhere (another base, a backup), are the user's.
    //
    // Skipped for a legacy build: `metadata.files` is empty there, so every
    // store link in its tree would read as unindexed, and deleting it would
    // destroy the tree's only copy of the data. An unindexed store link in a
    // CAS-format build is recoverable content that `reindex` records; it is
    // safe to remove here because the manifest, not the tree, is authoritative.
    if metadata.has_file_hashes() {
        for rel in unindexed_paths(build_dir, metadata)? {
            let path = build_dir.join(&rel);
            let Ok(link) = path.symlink_metadata() else {
                continue;
            };
            if !link.file_type().is_symlink() || !crate::cas_vfs::symlink_points_into(&path, cas_root) {
                continue;
            }
            tracing::info!("removing stale link {} (not in metadata.toml)", path.display());
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("failed to remove stale link {} (left in place): {e}", path.display());
            }
        }
    }

    Ok(repointed)
}

/// Re-materialise every dumped build (or one build when `only_build` is given)
/// from its metadata. Returns the repointed paths per build that needed any.
pub fn relink_builds(output_base: &Path, only_build: Option<u32>) -> Result<BTreeMap<String, Vec<String>>, Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let cas_root = cas::cas_root(output_base);

    let targets: Vec<&BuildEntry> = match only_build {
        Some(b) => index.builds.iter().filter(|e| e.build == b).collect(),
        None => index.builds.iter().collect(),
    };
    if targets.is_empty() {
        bail!("No matching builds found in {}", output_base.join("builds.toml").display());
    }

    let mut repointed = BTreeMap::new();
    for entry in targets {
        let build_dir = output_base.join(&entry.dir);
        let Some(mut metadata) = BuildMetadata::load(&build_dir.join("metadata.toml")) else {
            bail!("{} has no readable metadata.toml", entry.dir);
        };
        let paths = relink_build(&build_dir, &cas_root, &metadata)?;
        // The tree exists now, so the record says so: every other command reads
        // this rather than probing the directory.
        if !metadata.materialized {
            metadata.materialized = true;
            metadata.save(&build_dir.join("metadata.toml"))?;
        }
        if !paths.is_empty() {
            repointed.insert(entry.dir.clone(), paths);
        }
    }
    Ok(repointed)
}

/// Delete the materialized symlink trees of every build in `output_base` (or
/// one build). Returns the links removed per build directory, omitting builds
/// that had nothing to remove.
///
/// The inverse of [`relink_builds`]. Readers resolve every file through
/// `metadata.toml` and `common/`, so a build with no tree is fully readable;
/// this reclaims the directory entries a materialized tree costs.
pub fn prune_materialized_trees(
    output_base: &Path,
    only_build: Option<u32>,
) -> Result<BTreeMap<String, usize>, Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let targets: Vec<&BuildEntry> = match only_build {
        Some(build) => index.builds.iter().filter(|entry| entry.build == build).collect(),
        None => index.builds.iter().collect(),
    };
    if targets.is_empty() {
        bail!("No matching builds found in {}", output_base.join("builds.toml").display());
    }

    let mut pruned = BTreeMap::new();
    for entry in targets {
        let build_dir = output_base.join(&entry.dir);
        let Some(cas) = crate::cas_vfs::BuildCas::open(&build_dir) else {
            bail!("{} has no readable metadata.toml", entry.dir);
        };
        let removed = cas.prune_materialized_tree();
        // metadata.toml is the record of whether a tree exists, so clearing the
        // tree clears the claim. Gated on the same condition prune itself uses:
        // a legacy build's tree is never actually pruned (it is the only copy
        // of its data), so its `materialized` flag must not be cleared either.
        if let Some(mut metadata) = BuildMetadata::load(&build_dir.join("metadata.toml"))
            && metadata.has_file_hashes()
            && metadata.materialized
        {
            metadata.materialized = false;
            metadata.save(&build_dir.join("metadata.toml"))?;
        }
        if removed > 0 {
            pruned.insert(entry.dir.clone(), removed);
        }
    }
    Ok(pruned)
}

/// Reindex every dumped build (or one build when `only_build` is given),
/// persisting the entries recovered. Returns a report per build that had
/// anything to report at all.
pub fn reindex_builds(output_base: &Path, only_build: Option<u32>) -> Result<BTreeMap<String, ReindexReport>, Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let cas_root = cas::cas_root(output_base);

    let targets: Vec<&BuildEntry> = match only_build {
        Some(b) => index.builds.iter().filter(|e| e.build == b).collect(),
        None => index.builds.iter().collect(),
    };
    if targets.is_empty() {
        bail!("No matching builds found in {}", output_base.join("builds.toml").display());
    }

    let mut reports = BTreeMap::new();
    for entry in targets {
        let build_dir = output_base.join(&entry.dir);
        let meta_path = build_dir.join("metadata.toml");
        let Some(mut metadata) = BuildMetadata::load(&meta_path) else {
            bail!("{} has no readable metadata.toml", entry.dir);
        };
        let report = reindex_build(&build_dir, &cas_root, &mut metadata)?;
        if !report.added.is_empty() {
            metadata.save(&meta_path)?;
        }
        if !report.added.is_empty() || !report.unbacked.is_empty() {
            reports.insert(entry.dir.clone(), report);
        }
    }
    Ok(reports)
}

/// Remove content-addressed objects no longer referenced by any build. An
/// object is live if it appears in some build's metadata (the extracted vfs
/// tree or the derived artifacts). Aborts without deleting anything if any
/// build's metadata cannot be read, so in-use objects are never removed.
pub fn gc_cas(output_base: &Path) -> Result<(), Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let cas_root = cas::cas_root(output_base);

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

/// Consistency report for one build in a dump base.
pub struct BuildVerification {
    pub dir: String,
    pub build: u32,
    pub version: String,
    /// Total unique content hashes the build references.
    pub referenced: usize,
    /// Referenced hashes with no object in the shared store.
    pub missing_objects: Vec<String>,
    /// Referenced objects present in the store whose bytes hash to something
    /// else. Only populated when hash checking is requested.
    pub corrupt_objects: Vec<CorruptObject>,
    /// VFS-relative paths whose symlink target does not resolve to a file.
    pub broken_links: Vec<String>,
    /// Build-relative paths present in the build tree that no metadata entry
    /// points at. Nothing keeps their objects alive and no other check reads
    /// them, so they are content the build has effectively lost.
    pub unindexed: Vec<String>,
    /// True when `metadata.toml` was missing or unparseable.
    pub metadata_unreadable: bool,
}

impl BuildVerification {
    pub fn is_ok(&self) -> bool {
        !self.metadata_unreadable
            && self.missing_objects.is_empty()
            && self.corrupt_objects.is_empty()
            && self.broken_links.is_empty()
            && self.unindexed.is_empty()
    }
}

/// Verdict for one content object in the shared store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectAudit {
    /// The object is present and its bytes hash to its name.
    Intact,
    /// Nothing is stored under this hash, or what is there cannot be read.
    Absent,
    /// The object is present but its bytes hash to something else.
    Corrupt { actual: String },
}

/// Audit every content hash referenced by any of `builds`, invoking `audit` at
/// most once per distinct hash.
///
/// The store is shared across builds, so one object is typically referenced by
/// dozens of them. Auditing per build would re-read the same gigabytes once per
/// referencing build.
fn audit_objects(
    builds: &[&BuildMetadata],
    mut audit: impl FnMut(&str) -> ObjectAudit,
) -> BTreeMap<String, ObjectAudit> {
    let mut verdicts: BTreeMap<String, ObjectAudit> = BTreeMap::new();
    for meta in builds {
        for hash in meta.referenced_hashes() {
            if verdicts.contains_key(&hash) {
                continue;
            }
            let verdict = audit(&hash);
            verdicts.insert(hash, verdict);
        }
    }
    verdicts
}

/// Read one content object and compare its bytes against the name it is stored
/// under.
fn audit_object_on_disk(cas_root: &Path, hash: &str) -> ObjectAudit {
    let path = cas::cas_path(cas_root, hash);
    match cas::hash_file(&path) {
        Ok(actual) if actual == hash => ObjectAudit::Intact,
        Ok(actual) => ObjectAudit::Corrupt { actual },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ObjectAudit::Absent,
        Err(e) => {
            tracing::warn!("could not read content object {}: {e}", path.display());
            ObjectAudit::Absent
        }
    }
}

/// Files in a build tree that no metadata entry points at.
///
/// A build tree is a materialisation of `metadata.toml`: `vfs/<rel>` for
/// `files`, build-relative paths for `derived`. Anything else is content the
/// index has lost track of, which no other check can see and no GC will keep
/// alive. Symlinks are leaves here, never directories to descend into.
fn unindexed_paths(build_dir: &Path, meta: &BuildMetadata) -> Result<Vec<String>, Report> {
    let mut unindexed = Vec::new();
    let mut stack = vec![build_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(report!("Failed to read {}: {e}", dir.display())),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = entry.file_type().attach_with(|| format!("Failed to stat {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(build_dir)
                .expect("walked path is under build_dir")
                .to_string_lossy()
                .replace('\\', "/");
            if NOT_BUILD_CONTENT.contains(&rel.as_str()) {
                continue;
            }
            let indexed = match rel.strip_prefix("vfs/") {
                Some(vfs_rel) => meta.files.contains_key(vfs_rel),
                None => meta.derived.contains_key(&rel),
            };
            if !indexed {
                unindexed.push(rel);
            }
        }
    }
    unindexed.sort();
    Ok(unindexed)
}

/// Build-root files that describe a build rather than being indexed content.
const NOT_BUILD_CONTENT: [&str; 2] = ["metadata.toml", "constants.json"];

/// What a reindex found in a build tree.
#[derive(Debug, Default)]
pub struct ReindexReport {
    /// VFS-relative paths added to `metadata.files`.
    pub added: Vec<String>,
    /// Build-relative paths the tree holds that could not be indexed: the store
    /// has no object behind them, so recording them would have metadata claim
    /// content nothing can read.
    pub unbacked: Vec<String>,
}

/// Record content a build tree holds that `metadata.toml` never indexed.
///
/// A store link whose path has no entry is unreachable through metadata, which
/// means no GC keeps its object alive and no consistency check reads it. Adding
/// the entry is only sound when the object is actually there; anything else is
/// reported for the caller to deal with.
pub fn reindex_build(build_dir: &Path, cas_root: &Path, metadata: &mut BuildMetadata) -> Result<ReindexReport, Report> {
    let mut report = ReindexReport::default();
    for rel in unindexed_paths(build_dir, metadata)? {
        let Some(vfs_rel) = rel.strip_prefix("vfs/") else {
            report.unbacked.push(rel);
            continue;
        };
        let hash = cas::link_target_hash(&build_dir.join(&rel));
        match hash {
            Some(hash) if cas::object_exists(cas_root, &hash) => {
                metadata.files.insert(vfs_rel.to_string(), hash);
                report.added.push(vfs_rel.to_string());
            }
            _ => report.unbacked.push(rel),
        }
    }
    report.added.sort();
    report.unbacked.sort();
    Ok(report)
}

/// Verify that every build in `builds.toml` is internally consistent: its
/// `metadata.toml` parses, every referenced content object exists in the shared
/// store, and (when `check_links` is set) every reconstructed symlink resolves
/// to a readable file and every file in the tree is one metadata indexes. Returns a report per build; the caller decides how to act.
///
/// With `check_hashes` set, every referenced object is read and re-hashed rather
/// than merely checked for presence, which is the only way to catch an object
/// whose name is right and whose bytes are not. Each distinct object is read
/// once across the whole dump base.
pub fn verify_builds(
    output_base: &Path,
    check_links: bool,
    check_hashes: bool,
) -> Result<Vec<BuildVerification>, Report> {
    let cas_root = cas::cas_root(output_base);
    verify_builds_audited(output_base, check_links, check_hashes, |hash| audit_object_on_disk(&cas_root, hash))
}

/// [`verify_builds`] with the object audit supplied, so a test can count how
/// many times each object is read. Production has exactly one auditor; the
/// parameter exists because "each object is read once" is otherwise
/// unobservable from outside, and a `verify_builds` that audited per build
/// would pass every other test in this module.
fn verify_builds_audited(
    output_base: &Path,
    check_links: bool,
    check_hashes: bool,
    audit: impl FnMut(&str) -> ObjectAudit,
) -> Result<Vec<BuildVerification>, Report> {
    let index = BuildsIndex::load(&output_base.join("builds.toml"));
    let cas_root = cas::cas_root(output_base);

    let loaded: Vec<(&BuildEntry, Option<BuildMetadata>)> = index
        .builds
        .iter()
        .map(|entry| (entry, BuildMetadata::load(&output_base.join(&entry.dir).join("metadata.toml"))))
        .collect();

    let audits = if check_hashes {
        let readable: Vec<&BuildMetadata> = loaded.iter().filter_map(|(_, meta)| meta.as_ref()).collect();
        audit_objects(&readable, audit)
    } else {
        BTreeMap::new()
    };

    let mut reports = Vec::new();
    for (entry, meta) in loaded {
        let build_dir = output_base.join(&entry.dir);
        let Some(meta) = meta else {
            reports.push(BuildVerification {
                dir: entry.dir.clone(),
                build: entry.build,
                version: entry.version.clone(),
                referenced: 0,
                missing_objects: Vec::new(),
                corrupt_objects: Vec::new(),
                broken_links: Vec::new(),
                unindexed: Vec::new(),
                metadata_unreadable: true,
            });
            continue;
        };

        let referenced = meta.referenced_hashes();
        let mut missing_objects = Vec::new();
        let mut corrupt_objects = Vec::new();
        for hash in &referenced {
            match audits.get(hash) {
                Some(ObjectAudit::Intact) => {}
                Some(ObjectAudit::Absent) => missing_objects.push(hash.clone()),
                Some(ObjectAudit::Corrupt { actual }) => {
                    corrupt_objects.push(CorruptObject::attribute(entry, &meta, hash, actual));
                }
                // Without hash checking, presence is all that can be known.
                None => {
                    if !cas::object_exists(&cas_root, hash) {
                        missing_objects.push(hash.clone());
                    }
                }
            }
        }
        missing_objects.sort();
        corrupt_objects.sort_by(|a, b| a.hash.cmp(&b.hash));

        let mut broken_links = Vec::new();
        let mut unindexed = Vec::new();
        if check_links {
            for (rel, _) in meta.files.iter().chain(meta.derived.iter()) {
                let path = build_dir.join("vfs").join(rel);
                // Derived artifacts live at the build root, not under vfs/.
                let candidate = if path.exists() { path } else { build_dir.join(rel) };
                if !candidate.exists() {
                    broken_links.push(rel.clone());
                }
            }
            broken_links.sort();
            unindexed = unindexed_paths(&build_dir, &meta)?;
        }

        reports.push(BuildVerification {
            dir: entry.dir.clone(),
            build: entry.build,
            version: entry.version.clone(),
            referenced: referenced.len(),
            missing_objects,
            corrupt_objects,
            broken_links,
            unindexed,
            metadata_unreadable: false,
        });
    }
    Ok(reports)
}

/// Remove content-addressed objects no longer referenced by any build present
/// on disk. Scans every directory under `output_base` that contains a
/// `metadata.toml`, so it stays correct even when `builds.toml` is out of sync
/// (e.g. a build directory was deleted manually without GC). Aborts without
/// removing anything if any metadata file cannot be read, so in-use objects are
/// never deleted. Returns the number of objects removed.
pub fn gc_unreferenced(output_base: &Path) -> Result<usize, Report> {
    let cas_root = cas::cas_root(output_base);
    if !cas_root.exists() {
        return Ok(0);
    }

    let mut live = std::collections::HashSet::new();
    for entry in
        std::fs::read_dir(output_base).attach_with(|| format!("Failed to read {}", output_base.display()))?.flatten()
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

/// Migrate a dump base from the legacy `vfs_common/` CAS directory to `common/`,
/// rewriting every build's symlinks to point at the new store. Handles both a
/// clean rename (when `common/` doesn't exist yet) and a merge (when a redump
/// has already created `common/` while old builds still reference `vfs_common/`).
/// No-op when the legacy directory is absent. Returns whether a migration ran.
pub fn migrate_cas_dir_name(output_base: &Path) -> Result<bool, Report> {
    let legacy = output_base.join(cas::LEGACY_CAS_DIR);
    let current = cas::cas_root(output_base);
    if !legacy.exists() {
        return Ok(false);
    }

    if !current.exists() {
        // Fast path: nothing in the new store yet, so move it wholesale.
        std::fs::rename(&legacy, &current)
            .attach_with(|| format!("failed to rename {} to {}", legacy.display(), current.display()))?;
    } else {
        // Both stores exist: fold legacy objects into `common/`. Objects are
        // content-addressed, so a name collision means identical bytes — keep
        // the existing copy and drop the duplicate.
        merge_cas_objects(&legacy, &current)?;
    }

    // Relative symlinks under each build may still name the old store, so
    // re-create every build's links against `common/`.
    relink_all_builds(output_base, &current)?;

    // Drop whatever remains of the legacy tree (emptied by the merge, or already
    // gone after the rename).
    if legacy.exists() {
        std::fs::remove_dir_all(&legacy)
            .attach_with(|| format!("failed to remove emptied legacy store {}", legacy.display()))?;
    }

    Ok(true)
}

/// Move every object from a legacy CAS tree into `dest`, deduplicating by hash.
/// A collision (same fanout/name) is identical content, so the source copy is
/// simply removed. Empties the source fanout directories as it goes.
fn merge_cas_objects(legacy: &Path, dest: &Path) -> Result<(), Report> {
    for fanout in std::fs::read_dir(legacy).attach_with(|| format!("Failed to read {}", legacy.display()))?.flatten() {
        if !fanout.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let dest_fanout = dest.join(fanout.file_name());
        for obj in std::fs::read_dir(fanout.path())?.flatten() {
            if !obj.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let dest_path = dest_fanout.join(obj.file_name());
            if dest_path.exists() {
                std::fs::remove_file(obj.path())?;
                continue;
            }
            std::fs::create_dir_all(&dest_fanout)?;
            // rename works within a volume; fall back to copy+delete across volumes.
            if std::fs::rename(obj.path(), &dest_path).is_err() {
                std::fs::copy(obj.path(), &dest_path)?;
                std::fs::remove_file(obj.path())?;
            }
        }
    }
    Ok(())
}

/// Re-create every build's `vfs/` and derived symlinks against `cas_root`,
/// using the hashes recorded in each `metadata.toml`. Idempotent.
fn relink_all_builds(output_base: &Path, cas_root: &Path) -> Result<(), Report> {
    for entry in
        std::fs::read_dir(output_base).attach_with(|| format!("Failed to read {}", output_base.display()))?.flatten()
    {
        let build_dir = entry.path();
        let Some(mut meta) = BuildMetadata::load(&build_dir.join("metadata.toml")) else {
            continue;
        };
        let vfs_dir = build_dir.join("vfs");

        // A rename migration reconciles the record with what is on disk, unlike
        // every other caller of this module, which trusts `materialized` and
        // never infers it from the directory. A legacy build's metadata
        // predates the field and reads false even though its tree is real;
        // probing for the tree here is how that tree's links get repointed at
        // the renamed store instead of dangling. A build that never had a tree
        // is left alone.
        if !meta.materialized && !vfs_dir.is_dir() {
            continue;
        }

        let relink = |rel: &str, hash: &str, base: &Path| {
            let link = base.join(rel);
            let _ = std::fs::remove_file(&link);
            if let Err(e) = cas::link_file(cas_root, hash, &link) {
                tracing::warn!("failed to relink {}: {e}", link.display());
            }
        };
        for (rel, hash) in &meta.files {
            relink(rel, hash, &vfs_dir);
        }
        for (rel, hash) in &meta.derived {
            relink(rel, hash, &build_dir);
        }
        // The tree exists now, so the record says so: every other command reads
        // this rather than probing the directory.
        if !meta.materialized {
            meta.materialized = true;
            meta.save(&build_dir.join("metadata.toml"))?;
        }
    }
    Ok(())
}

/// Migrate any pre-CAS dumps in `output_base` into content-addressed storage.
///
/// Older dumps stored the extracted `vfs/` tree as plain files with no entries
/// in `metadata.files`. This rehashes those files into `common/`, replaces
/// them with symlinks, records the hashes, and regenerates derived artifacts so
/// the dump deduplicates against every other build. Returns the number of
/// builds migrated. Builds already in CAS format are left untouched.
pub fn migrate_to_cas(output_base: &Path) -> Result<usize, Report> {
    let cas_root = cas::cas_root(output_base);
    let mut migrated = 0;
    for entry in
        std::fs::read_dir(output_base).attach_with(|| format!("Failed to read {}", output_base.display()))?.flatten()
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
        for entry in std::fs::read_dir(&dir).attach_with(|| format!("Failed to read {}", dir.display()))?.flatten() {
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
            let rel =
                path.strip_prefix(&vfs_dir).expect("walked path is under vfs_dir").to_string_lossy().replace('\\', "/");
            let data = std::fs::read(&path).attach_with(|| format!("Failed to read {}", path.display()))?;
            let hash = cas::store(cas_root, &data)?;
            std::fs::remove_file(&path).attach_with(|| format!("Failed to remove {}", path.display()))?;
            cas::link_file(cas_root, &hash, &path)?;
            metadata.files.insert(rel, hash);
        }
    }
    // This build had a tree, and the migration keeps it.
    metadata.materialized = true;
    refresh_build_derived(build_dir, cas_root, metadata)?;
    Ok(())
}

fn extract_vfs_dir_cas(
    vfs: &VfsPath,
    vfs_path: &str,
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
    progress: Option<&ProgressBar>,
) -> Result<usize, Report> {
    let dir = match vfs.join(vfs_path) {
        Ok(d) => d,
        Err(_) => return Ok(0),
    };
    let walker = match dir.walk_dir() {
        Ok(w) => w,
        Err(_) => return Ok(0),
    };

    let mut count = 0;
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
        store_and_record(&buf, rel, cas_root, file_hashes)?;
        count += 1;
        if let Some(pb) = progress {
            pb.inc(1);
        }
    }
    Ok(count)
}

/// Extract a single VFS file. Returns `true` if it was found and stored.
fn extract_vfs_file_cas(
    vfs: &VfsPath,
    vfs_path: &str,
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
) -> Result<bool, Report> {
    let file = match vfs.join(vfs_path) {
        Ok(f) => f,
        Err(_) => {
            tracing::warn!("VFS path not found (skipping): {vfs_path}");
            return Ok(false);
        }
    };
    let mut buf = Vec::new();
    match file.open_file() {
        Ok(mut f) => f.read_to_end(&mut buf)?,
        Err(_) => {
            tracing::warn!("Could not open VFS file (skipping): {vfs_path}");
            return Ok(false);
        }
    };
    store_and_record(&buf, vfs_path, cas_root, file_hashes)?;
    Ok(true)
}

/// Extract the named files from each subdirectory of `parent_dir`. Returns the
/// number of files extracted.
fn extract_map_files_cas(
    vfs: &VfsPath,
    parent_dir: &str,
    filenames: &[&str],
    cas_root: &Path,
    file_hashes: &mut BTreeMap<String, String>,
    progress: Option<&ProgressBar>,
) -> Result<usize, Report> {
    let parent = match vfs.join(parent_dir) {
        Ok(d) => d,
        Err(_) => return Ok(0),
    };
    let entries = match parent.read_dir() {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };

    let mut count = 0;
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
            store_and_record(&buf, rel, cas_root, file_hashes)?;
            count += 1;
            if let Some(pb) = progress {
                pb.inc(1);
            }
        }
    }
    Ok(count)
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

    fn register(base: &Path, version: &str, build: u32) {
        let path = base.join("builds.toml");
        let mut index = BuildsIndex::load(&path);
        index.upsert(BuildEntry {
            version: version.to_string(),
            build,
            dir: format!("{version}_{build}"),
            dumped_at: String::new(),
        });
        index.save(&path).unwrap();
    }

    #[test]
    fn extraction_stores_without_writing_to_the_build_dir() {
        let base = tempfile::tempdir().unwrap();
        let cas_root = cas::cas_root(base.path());
        let build_dir = base.path().join("1.2.3_100");
        let mut file_hashes = BTreeMap::new();

        store_and_record(b"content bytes", "/content/x.dat", &cas_root, &mut file_hashes).unwrap();

        // The manifest gains the entry and the store gains the bytes. The build
        // directory is not touched: a tree is what `relink` is for.
        let hash = file_hashes.get("content/x.dat").expect("recorded");
        assert!(cas::object_exists(&cas_root, hash));
        assert!(!build_dir.exists());
    }

    /// An object rewritten in place keeps its name, so existence checking sees
    /// nothing wrong. This is exactly what `core.autocrlf` did to the published
    /// archive: same path, different bytes, different length.
    #[test]
    fn hash_checking_finds_an_object_whose_bytes_were_changed() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join("common");

        let intact = cas::store(&cas_root, b"an object nobody touched").unwrap();
        let tampered = cas::store(&cas_root, b"<xml>\r\n  <node />\r\n</xml>\r\n").unwrap();
        write_build_metadata(
            &base.join("15.4.0_12506899"),
            "15.4.0",
            12506899,
            &[("res/a.xml", &intact), ("res/b.xml", &tampered), ("res/c.xml", &tampered)],
        );
        register(base, "15.4.0", 12506899);

        let rewritten: &[u8] = b"<xml>\n  <node />\n</xml>\n";
        std::fs::write(cas::cas_path(&cas_root, &tampered), rewritten).unwrap();

        let unchecked = verify_builds(base, false, false).unwrap();
        assert!(unchecked[0].is_ok(), "existence checking cannot see rewritten bytes");

        let checked = verify_builds(base, false, true).unwrap();
        let report = &checked[0];
        assert!(!report.is_ok(), "hash checking must reject the rewritten object");
        assert!(report.missing_objects.is_empty(), "the object is present, just wrong");
        assert_eq!(report.corrupt_objects.len(), 1, "only the rewritten object is corrupt");

        let corrupt = &report.corrupt_objects[0];
        assert_eq!(corrupt.hash, tampered);
        assert_eq!(corrupt.actual, cas::hash_bytes(rewritten));
        assert_eq!(corrupt.build, 12506899);
        assert_eq!(corrupt.version, "15.4.0");
        assert_eq!(corrupt.files, vec!["res/b.xml", "res/c.xml"]);
    }

    /// The single-pass design is the whole reason auditing 12 GB across 127
    /// builds is feasible: a per-build audit re-reads every shared object once
    /// per referencing build.
    #[test]
    fn an_object_is_hashed_once_even_when_several_builds_reference_it() {
        let mut first = BuildMetadata { version: "15.3.0".into(), build: 12400000, ..Default::default() };
        first.files.insert("res/a.xml".into(), "shared".into());
        first.files.insert("res/only_in_first.xml".into(), "unique".into());
        let mut second = BuildMetadata { version: "15.4.0".into(), build: 12506899, ..Default::default() };
        second.files.insert("res/b.xml".into(), "shared".into());
        second.derived.insert("GameParams.rkyv".into(), "shared".into());

        let mut reads: Vec<String> = Vec::new();
        let verdicts = audit_objects(&[&first, &second], |hash| {
            reads.push(hash.to_string());
            ObjectAudit::Intact
        });

        assert_eq!(reads.iter().filter(|h| h.as_str() == "shared").count(), 1, "read {reads:?}");
        assert_eq!(reads.len(), 2, "read {reads:?}");
        assert_eq!(verdicts.len(), 2);
    }

    /// The test above exercises `audit_objects` directly, which says nothing
    /// about how `verify_builds` calls it. A `verify_builds` that called it once
    /// per build with a one-element slice would re-read every shared object per
    /// referencing build -- 12 GB times 127 builds -- and pass every other test
    /// here. This one counts the reads the whole verification makes.
    #[test]
    fn verifying_a_whole_dump_base_hashes_each_shared_object_once() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        for (version, build, own) in [("15.3.0", 12400000u32, "only_in_first"), ("15.4.0", 12506899, "only_in_second")]
        {
            write_build_metadata(
                &base.join(format!("{version}_{build}")),
                version,
                build,
                &[("res/shared.xml", "shared"), ("res/own.xml", own)],
            );
            register(base, version, build);
        }

        let mut reads: Vec<String> = Vec::new();
        let reports = verify_builds_audited(base, false, true, |hash| {
            reads.push(hash.to_string());
            ObjectAudit::Intact
        })
        .unwrap();

        assert_eq!(reports.len(), 2);
        assert_eq!(reads.iter().filter(|h| h.as_str() == "shared").count(), 1, "read {reads:?}");
        assert_eq!(reads.len(), 3, "read {reads:?}");
    }

    /// Without `check_hashes` no object is read at all, which is what makes
    /// reporting a corrupt count on that path a claim about an audit that never
    /// ran.
    #[test]
    fn verifying_without_hash_checking_reads_no_object() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        write_build_metadata(&base.join("15.4.0_12506899"), "15.4.0", 12506899, &[("res/a.xml", "shared")]);
        register(base, "15.4.0", 12506899);

        let mut reads: Vec<String> = Vec::new();
        let reports = verify_builds_audited(base, false, false, |hash| {
            reads.push(hash.to_string());
            ObjectAudit::Intact
        })
        .unwrap();

        assert_eq!(reports.len(), 1);
        assert!(reads.is_empty(), "read {reads:?}");
        assert!(reports[0].corrupt_objects.is_empty(), "nothing was hashed, so nothing can be called corrupt");
    }

    /// Reading each object once must not cost per-build attribution: the user
    /// needs to know every build a corrupt object breaks.
    #[test]
    fn a_shared_corrupt_object_is_reported_against_every_build_that_references_it() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join("common");

        let shared = cas::store(&cas_root, b"shared between two builds\r\n").unwrap();
        write_build_metadata(&base.join("15.3.0_12400000"), "15.3.0", 12400000, &[("res/a.xml", &shared)]);
        write_build_metadata(&base.join("15.4.0_12506899"), "15.4.0", 12506899, &[("res/b.xml", &shared)]);
        register(base, "15.3.0", 12400000);
        register(base, "15.4.0", 12506899);

        std::fs::write(cas::cas_path(&cas_root, &shared), b"rewritten\n").unwrap();

        let reports = verify_builds(base, false, true).unwrap();
        assert_eq!(reports.len(), 2);
        for report in &reports {
            assert_eq!(report.corrupt_objects.len(), 1, "{} missed the corrupt object", report.dir);
            assert_eq!(report.corrupt_objects[0].hash, shared);
        }
        assert_eq!(reports[0].corrupt_objects[0].files, vec!["res/a.xml"]);
        assert_eq!(reports[1].corrupt_objects[0].files, vec!["res/b.xml"]);
    }

    /// A hash with no object at all is missing, not corrupt: re-downloading
    /// fixes one and cannot fix the other.
    #[test]
    fn a_missing_object_is_not_reported_as_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        write_build_metadata(
            &base.join("15.4.0_12506899"),
            "15.4.0",
            12506899,
            &[("res/a.xml", "0123456789abcdef0123")],
        );
        register(base, "15.4.0", 12506899);

        let reports = verify_builds(base, false, true).unwrap();
        assert_eq!(reports[0].missing_objects, vec!["0123456789abcdef0123"]);
        assert!(reports[0].corrupt_objects.is_empty());
    }

    #[test]
    fn gc_unreferenced_removes_orphans_and_keeps_live() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join("common");

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
        let cas_root = base.join("common");
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

    #[test]
    fn migrate_cas_dir_name_renames_and_relinks() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Legacy layout: vfs_common/ store + a build whose vfs file symlinks into it.
        let legacy = base.join(cas::LEGACY_CAS_DIR);
        let hash = cas::store(&legacy, b"icon bytes").unwrap();
        let build_dir = base.join("1.0.0_100");
        let link = build_dir.join("vfs/gui/x.png");
        cas::link_file(&legacy, &hash, &link).unwrap();
        write_build_metadata(&build_dir, "1.0.0", 100, &[("gui/x.png", &hash)]);

        assert!(migrate_cas_dir_name(base).unwrap());
        assert!(base.join(cas::CAS_DIR).exists());
        assert!(!base.join(cas::LEGACY_CAS_DIR).exists());
        // The symlink now resolves through common/ and still reads back.
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&link).unwrap(), b"icon bytes");
        // Idempotent: nothing to migrate the second time.
        assert!(!migrate_cas_dir_name(base).unwrap());
    }

    /// A rename migration probes for a real `vfs/` directory as a narrow,
    /// deliberate exception to trusting `materialized` -- everywhere else the
    /// record leads. A legacy build's metadata predates the field (reads
    /// false) but its tree is real, so it must be relinked and the flag
    /// caught up; a manifest-only build with no tree must be left untouched.
    #[test]
    fn relink_all_builds_only_touches_a_build_that_already_has_a_tree() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join(cas::CAS_DIR);

        // Legacy build: a real vfs/ tree on disk, metadata reads unmaterialized.
        let legacy_hash = cas::store(&cas_root, b"icon bytes").unwrap();
        let legacy_dir = base.join("1.0.0_100");
        let legacy_link = legacy_dir.join("vfs/gui/x.png");
        std::fs::create_dir_all(legacy_link.parent().unwrap()).unwrap();
        std::fs::write(&legacy_link, b"stale placeholder").unwrap();
        write_build_metadata(&legacy_dir, "1.0.0", 100, &[("gui/x.png", &legacy_hash)]);

        // Fresh CAS-format build: no tree, materialized explicitly false.
        let manifest_hash = cas::store(&cas_root, b"other bytes").unwrap();
        let manifest_dir = base.join("2.0.0_200");
        write_build_metadata(&manifest_dir, "2.0.0", 200, &[("content/x.dat", &manifest_hash)]);

        if relink_all_builds(base, &cas_root).is_err() {
            eprintln!("skipping: cannot create symlinks on this platform/account");
            return;
        }

        // The legacy build's tree was relinked, and the record now says so.
        match legacy_link.symlink_metadata() {
            Ok(m) if m.file_type().is_symlink() => {
                assert_eq!(std::fs::read(&legacy_link).unwrap(), b"icon bytes");
            }
            _ => eprintln!("skipping the link assertion: cannot create symlinks on this platform/account"),
        }
        assert!(BuildMetadata::load(&legacy_dir.join("metadata.toml")).unwrap().materialized);

        // The manifest-only build was left alone: no tree, flag unchanged.
        assert!(!manifest_dir.join("vfs").exists());
        assert!(!BuildMetadata::load(&manifest_dir.join("metadata.toml")).unwrap().materialized);
    }

    #[test]
    fn relink_records_that_the_build_is_materialized() {
        let base = tempfile::tempdir().unwrap();
        let cas_root = cas::cas_root(base.path());
        let hash = cas::store(&cas_root, b"hi").unwrap();
        let build_dir = base.path().join("1.2.3_100");
        std::fs::create_dir_all(&build_dir).unwrap();
        let mut meta = BuildMetadata { version: "1.2.3".into(), build: 100, ..Default::default() };
        meta.files.insert("content/x.dat".into(), hash);
        meta.save(&build_dir.join("metadata.toml")).unwrap();
        let mut index = BuildsIndex::load(&base.path().join("builds.toml"));
        index.upsert(BuildEntry {
            version: "1.2.3".into(),
            build: 100,
            dir: "1.2.3_100".into(),
            dumped_at: "now".into(),
        });
        index.save(&base.path().join("builds.toml")).unwrap();

        if relink_builds(base.path(), None).is_err() {
            eprintln!("skipping: cannot create symlinks on this platform/account");
            return;
        }

        assert!(BuildMetadata::load(&build_dir.join("metadata.toml")).unwrap().materialized);
    }

    #[test]
    fn prune_materialized_trees_reports_what_it_removed() {
        let base = tempfile::tempdir().unwrap();
        let cas_root = cas::cas_root(base.path());
        let hash = cas::store(&cas_root, b"rkyv").unwrap();
        let build_dir = base.path().join("1.2.3_100");
        std::fs::create_dir_all(&build_dir).unwrap();
        let mut meta = BuildMetadata { version: "1.2.3".into(), build: 100, ..Default::default() };
        // A build with links is a materialized build.
        meta.materialized = true;
        meta.files.insert("content/x.dat".into(), hash.clone());
        meta.derived.insert("game_params.rkyv".into(), hash.clone());
        meta.save(&build_dir.join("metadata.toml")).unwrap();
        let mut index = BuildsIndex::load(&base.path().join("builds.toml"));
        index.upsert(BuildEntry {
            version: "1.2.3".into(),
            build: 100,
            dir: "1.2.3_100".into(),
            dumped_at: "now".into(),
        });
        index.save(&base.path().join("builds.toml")).unwrap();
        if cas::link_file(&cas_root, &hash, &build_dir.join("game_params.rkyv")).is_err() {
            eprintln!("skipping: cannot create symlinks on this platform/account");
            return;
        }

        let removed = prune_materialized_trees(base.path(), None).unwrap();

        assert_eq!(removed.get("1.2.3_100"), Some(&1));
        assert!(build_dir.join("game_params.rkyv").symlink_metadata().is_err());
        // The store keeps the bytes; only the convenience link went away.
        assert!(cas::object_exists(&cas_root, &hash));
        // The tree is gone, so the record says so.
        assert!(!BuildMetadata::load(&build_dir.join("metadata.toml")).unwrap().materialized);
        assert!(prune_materialized_trees(base.path(), None).unwrap().is_empty());
    }

    /// A legacy build's tree is its only copy of the data: pruning must leave
    /// both the tree and the `materialized` flag alone, even though
    /// `relink_all_builds` can set that flag true for a hash-less build.
    #[test]
    fn prune_materialized_trees_leaves_a_legacy_build_untouched() {
        let base = tempfile::tempdir().unwrap();
        let build_dir = base.path().join("0.6.0_50");
        let tree_file = build_dir.join("vfs/content/GameParams.data");
        std::fs::create_dir_all(tree_file.parent().unwrap()).unwrap();
        std::fs::write(&tree_file, b"legacy params").unwrap();
        let mut meta = BuildMetadata { version: "0.6.0".into(), build: 50, ..Default::default() };
        meta.materialized = true;
        meta.save(&build_dir.join("metadata.toml")).unwrap();
        let mut index = BuildsIndex::load(&base.path().join("builds.toml"));
        index.upsert(BuildEntry {
            version: "0.6.0".into(),
            build: 50,
            dir: "0.6.0_50".into(),
            dumped_at: "now".into(),
        });
        index.save(&base.path().join("builds.toml")).unwrap();

        let pruned = prune_materialized_trees(base.path(), None).unwrap();

        assert!(pruned.is_empty(), "nothing was removed from a legacy build's tree");
        assert_eq!(std::fs::read(&tree_file).unwrap(), b"legacy params");
        assert!(BuildMetadata::load(&build_dir.join("metadata.toml")).unwrap().materialized);
    }

    #[test]
    fn refresh_derived_content_addresses_translations() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join(cas::CAS_DIR);
        let build_dir = base.join("1.0.0_100");
        // A raw per-locale catalog as a plain file (as copied from the game install).
        let mo = build_dir.join("translations/ru/LC_MESSAGES/global.mo");
        std::fs::create_dir_all(mo.parent().unwrap()).unwrap();
        std::fs::write(&mo, b"catalog bytes").unwrap();

        let mut meta = BuildMetadata { version: "1.0.0".into(), build: 100, ..Default::default() };
        meta.materialized = true;
        refresh_build_derived(&build_dir, &cas_root, &mut meta).unwrap();

        // The catalog is now a symlink into the shared store, recorded in derived.
        assert!(std::fs::symlink_metadata(&mo).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&mo).unwrap(), b"catalog bytes");
        assert!(meta.derived.contains_key("translations/ru/LC_MESSAGES/global.mo"));
    }

    #[test]
    fn refresh_derived_does_not_materialize_an_unmaterialized_build() {
        let base = tempfile::tempdir().unwrap();
        let cas_root = cas::cas_root(base.path());
        let build_dir = base.path().join("1.2.3_100");
        std::fs::create_dir_all(build_dir.join("translations/ru/LC_MESSAGES")).unwrap();
        std::fs::write(build_dir.join("translations/ru/LC_MESSAGES/global.mo"), b"catalog").unwrap();
        let mut meta = BuildMetadata { version: "1.2.3".into(), build: 100, ..Default::default() };

        refresh_build_derived(&build_dir, &cas_root, &mut meta).unwrap();

        // The bytes are recorded and stored; nothing was linked, because nothing
        // asked for this build to be materialized.
        let hash = meta.derived.get("translations/ru/LC_MESSAGES/global.mo").expect("recorded");
        assert!(cas::object_exists(&cas_root, hash));
        let mo = build_dir.join("translations/ru/LC_MESSAGES/global.mo");
        assert!(!mo.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&mo).unwrap(), b"catalog");
    }

    #[test]
    fn refresh_derived_relinks_a_materialized_build() {
        let base = tempfile::tempdir().unwrap();
        let cas_root = cas::cas_root(base.path());
        let build_dir = base.path().join("1.2.3_100");
        std::fs::create_dir_all(build_dir.join("translations/ru/LC_MESSAGES")).unwrap();
        std::fs::write(build_dir.join("translations/ru/LC_MESSAGES/global.mo"), b"catalog").unwrap();
        let mut meta = BuildMetadata { version: "1.2.3".into(), build: 100, ..Default::default() };
        meta.materialized = true;

        refresh_build_derived(&build_dir, &cas_root, &mut meta).unwrap();

        let mo = build_dir.join("translations/ru/LC_MESSAGES/global.mo");
        let link = mo.symlink_metadata().expect("the catalog should still be present");
        if !link.file_type().is_symlink() {
            eprintln!("skipping the link assertion: cannot create symlinks on this platform/account");
            return;
        }
        // The link resolves to the stored object.
        assert_eq!(std::fs::read(&mo).unwrap(), b"catalog");
    }

    /// A build whose materialised tree was pruned still owns its artifacts:
    /// they live in the store and `metadata.toml` is the only record of them.
    /// Dropping those entries orphans the objects, and the next GC deletes them.
    #[test]
    fn refresh_derived_keeps_artifacts_that_live_only_in_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join(cas::CAS_DIR);
        let build_dir = base.join("1.0.0_100");

        let mo_hash = cas::store(&cas_root, b"catalog bytes").unwrap();
        let rkyv_hash = cas::store(&cas_root, b"params blob").unwrap();
        // Pruning leaves the directory skeleton behind but removes every link.
        std::fs::create_dir_all(build_dir.join("translations/ru/LC_MESSAGES")).unwrap();

        let mut meta = BuildMetadata { version: "1.0.0".into(), build: 100, ..Default::default() };
        meta.derived.insert("translations/ru/LC_MESSAGES/global.mo".into(), mo_hash.clone());
        meta.derived.insert("game_params.rkyv".into(), rkyv_hash.clone());

        refresh_build_derived(&build_dir, &cas_root, &mut meta).unwrap();

        assert_eq!(meta.derived.get("translations/ru/LC_MESSAGES/global.mo"), Some(&mo_hash));
        assert_eq!(meta.derived.get("game_params.rkyv"), Some(&rkyv_hash));
    }

    /// A missing object is damage to the store, not evidence the build never
    /// owned the artifact. The entry stays so `verify` can flag it, and the
    /// caller hears about it so it does not GC against a damaged record.
    #[test]
    fn refresh_derived_reports_entries_whose_object_vanished() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = base.join(cas::CAS_DIR);
        let build_dir = base.join("1.0.0_100");
        std::fs::create_dir_all(&build_dir).unwrap();

        let mut meta = BuildMetadata { version: "1.0.0".into(), build: 100, ..Default::default() };
        meta.derived.insert("game_params.rkyv".into(), "0123456789abcdef0123".into());

        let missing = refresh_build_derived(&build_dir, &cas_root, &mut meta).unwrap();

        assert_eq!(missing, vec!["game_params.rkyv".to_string()]);
        assert_eq!(meta.derived.get("game_params.rkyv"), Some(&"0123456789abcdef0123".to_string()));
    }

    #[test]
    fn reindex_records_a_store_link_the_metadata_never_had() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = cas::cas_root(base);
        let hash = cas::store(&cas_root, b"icon bytes").unwrap();
        let build_dir = base.join("1.0.0_100");
        cas::link_file(&cas_root, &hash, &build_dir.join("vfs/gui/stray.png")).unwrap();
        let mut meta = BuildMetadata { version: "1.0.0".into(), build: 100, ..Default::default() };

        let report = reindex_build(&build_dir, &cas_root, &mut meta).unwrap();

        assert_eq!(report.added, vec!["gui/stray.png".to_string()]);
        assert_eq!(meta.files.get("gui/stray.png"), Some(&hash));
    }

    /// A link left pointing at superseded content contradicts the entry that
    /// names the path. Metadata is the authority, so the tree gets repointed at
    /// what it says, not the other way round.
    #[test]
    fn relink_repoints_a_link_that_disagrees_with_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = cas::cas_root(base);
        let current = cas::store(&cas_root, b"current bytes").unwrap();
        let build_dir = base.join("1.0.0_100");
        let link = build_dir.join("vfs/gui/icon.png");
        cas::link_file(&cas_root, "0123456789abcdef0123", &link).unwrap();
        let mut meta = BuildMetadata { version: "1.0.0".into(), build: 100, ..Default::default() };
        meta.files.insert("gui/icon.png".into(), current.clone());

        let repointed = relink_build(&build_dir, &cas_root, &meta).unwrap();

        assert_eq!(repointed, vec!["gui/icon.png".to_string()]);
        assert_eq!(std::fs::read(&link).unwrap(), b"current bytes");
    }

    #[test]
    fn relink_removes_links_the_manifest_no_longer_records() {
        let base = tempfile::tempdir().unwrap();
        let cas_root = cas::cas_root(base.path());
        let hash = cas::store(&cas_root, b"hi").unwrap();
        let build_dir = base.path().join("1.2.3_100");
        std::fs::create_dir_all(build_dir.join("vfs/content")).unwrap();
        let mut meta = BuildMetadata { version: "1.2.3".into(), build: 100, ..Default::default() };
        meta.materialized = true;
        meta.files.insert("content/x.dat".into(), hash.clone());

        // A link an earlier dump left behind, for a path no manifest entry names.
        let stale = build_dir.join("vfs/content/gone.dat");
        if cas::link_file(&cas_root, &hash, &stale).is_err() {
            eprintln!("skipping: cannot create symlinks on this platform/account");
            return;
        }
        // Content that is not a link into the store is the user's, not ours.
        let real = build_dir.join("vfs/content/notes.txt");
        std::fs::write(&real, b"keep me").unwrap();
        // A symlink into a DIFFERENT build's store (a sibling base) must survive:
        // the removal rule is "resolves into THIS build's store", not "any store
        // link". The target must be real and store-shaped, or a lexical rule that
        // only checks the path shape would wrongly spare it too.
        let other_base = base.path().join("other_base");
        let other_cas_root = cas::cas_root(&other_base);
        let other_hash = cas::store(&other_cas_root, b"someone else's bytes").unwrap();
        let other_target = cas::cas_path(&other_cas_root, &other_hash);
        let foreign = build_dir.join("vfs/content/foreign.dat");
        assert!(try_symlink(&other_target, &foreign));

        relink_build(&build_dir, &cas_root, &meta).unwrap();

        assert!(stale.symlink_metadata().is_err(), "a link the manifest does not record must go");
        assert!(build_dir.join("vfs/content/x.dat").exists(), "a recorded path must be materialized");
        assert_eq!(std::fs::read(&real).unwrap(), b"keep me");
        assert!(
            foreign.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()),
            "a link into a different build's store must survive"
        );
        assert_eq!(std::fs::read(&foreign).unwrap(), b"someone else's bytes");
        assert!(cas::object_exists(&cas_root, &hash), "the store keeps the bytes either way");
    }

    /// A legacy build's tree is its only copy of the data: `metadata.files` is
    /// empty there, so every store link would read as unindexed. Relink must
    /// not run its removal pass on such a build at all.
    #[test]
    fn relink_leaves_a_legacy_build_tree_alone() {
        let base = tempfile::tempdir().unwrap();
        let cas_root = cas::cas_root(base.path());
        let hash = cas::store(&cas_root, b"legacy params").unwrap();
        let build_dir = base.path().join("0.6.0_50");
        let link = build_dir.join("vfs/content/GameParams.data");
        if cas::link_file(&cas_root, &hash, &link).is_err() {
            eprintln!("skipping: cannot create symlinks on this platform/account");
            return;
        }
        let meta = BuildMetadata { version: "0.6.0".into(), build: 50, ..Default::default() };

        let repointed = relink_build(&build_dir, &cas_root, &meta).unwrap();

        assert!(repointed.is_empty());
        assert!(link.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()));
        assert_eq!(std::fs::read(&link).unwrap(), b"legacy params");
    }

    /// Create a file symlink, returning false when the platform/account forbids
    /// it. Tests that need symlinks skip gracefully when this returns false.
    fn try_symlink(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_file(target, link);
        #[cfg(not(windows))]
        let result = std::os::unix::fs::symlink(target, link);
        result.is_ok()
    }

    #[test]
    fn reindex_over_a_base_persists_the_new_entries() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = cas::cas_root(base);
        let hash = cas::store(&cas_root, b"icon bytes").unwrap();
        let build_dir = base.join("1.0.0_100");
        cas::link_file(&cas_root, &hash, &build_dir.join("vfs/gui/stray.png")).unwrap();
        write_build_metadata(&build_dir, "1.0.0", 100, &[]);
        register(base, "1.0.0", 100);

        let reports = reindex_builds(base, None).unwrap();

        assert_eq!(reports["1.0.0_100"].added, vec!["gui/stray.png".to_string()]);
        let saved = BuildMetadata::load(&build_dir.join("metadata.toml")).unwrap();
        assert_eq!(saved.files.get("gui/stray.png"), Some(&hash));
    }

    /// A link with no object behind it names content the store does not have.
    /// Indexing it would make `metadata.toml` claim content that cannot be
    /// read, so it is reported and left alone.
    #[test]
    fn reindex_will_not_index_a_link_whose_object_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = cas::cas_root(base);
        let build_dir = base.join("1.0.0_100");
        cas::link_file(&cas_root, "0123456789abcdef0123", &build_dir.join("vfs/gui/stray.png")).unwrap();
        let mut meta = BuildMetadata { version: "1.0.0".into(), build: 100, ..Default::default() };

        let report = reindex_build(&build_dir, &cas_root, &mut meta).unwrap();

        assert_eq!(report.unbacked, vec!["vfs/gui/stray.png".to_string()]);
        assert!(report.added.is_empty());
        assert!(meta.files.is_empty());
    }

    /// A file in the build tree that no entry points at is unreachable through
    /// metadata, so nothing keeps its object alive and no consistency check
    /// looks at it. It rots silently until something tries to read it.
    #[test]
    fn verify_reports_a_file_the_metadata_never_indexed() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let cas_root = cas::cas_root(base);
        let hash = cas::store(&cas_root, b"icon bytes").unwrap();
        let build_dir = base.join("1.0.0_100");
        cas::link_file(&cas_root, &hash, &build_dir.join("vfs/gui/known.png")).unwrap();
        cas::link_file(&cas_root, &hash, &build_dir.join("vfs/gui/stray.png")).unwrap();
        write_build_metadata(&build_dir, "1.0.0", 100, &[("gui/known.png", &hash)]);
        register(base, "1.0.0", 100);

        let reports = verify_builds(base, true, false).unwrap();

        assert_eq!(reports[0].unindexed, vec!["vfs/gui/stray.png".to_string()]);
        assert!(!reports[0].is_ok());
    }

    /// Set up a dump base holding one registered build with the given derived
    /// entries. The `constants.json` keeps the refresh offline; fetching
    /// constants is not what these tests are about.
    fn base_with_build(base: &Path, derived: &[(&str, &str)]) {
        let build_dir = base.join("1.0.0_100");
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(build_dir.join("constants.json"), b"{}").unwrap();
        let mut meta = BuildMetadata { version: "1.0.0".into(), build: 100, ..Default::default() };
        for (rel, hash) in derived {
            meta.derived.insert((*rel).to_string(), (*hash).to_string());
        }
        meta.save(&build_dir.join("metadata.toml")).unwrap();
        register(base, "1.0.0", 100);
    }

    #[test]
    fn refresh_derived_withholds_gc_when_an_object_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        base_with_build(base, &[("game_params.rkyv", "0123456789abcdef0123")]);

        let summary = refresh_derived(base, None).unwrap();

        assert!(!summary.safe_to_gc());
        assert_eq!(summary.damaged.get("1.0.0_100"), Some(&vec!["game_params.rkyv".to_string()]));
    }

    #[test]
    fn refresh_derived_vouches_for_a_base_whose_objects_are_all_present() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let hash = cas::store(&cas::cas_root(base), b"params blob").unwrap();
        base_with_build(base, &[("game_params.rkyv", &hash)]);

        let summary = refresh_derived(base, None).unwrap();

        assert!(summary.safe_to_gc());
        assert!(summary.damaged.is_empty());
    }
}
