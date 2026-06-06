//! Download dumped game data from the wows-replay-data repository.
//!
//! Files are fetched as raw content from GitHub. Content-addressed objects that
//! already exist locally are skipped, so downloading a build only transfers the
//! assets it does not already share with builds already in the cache.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use rootcause::prelude::*;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::builds::BuildEntry;
use crate::builds::BuildMetadata;
use crate::builds::BuildsIndex;
use crate::cas;

/// Base URL of the published game data repository, served as raw files.
pub const DEFAULT_REPO_BASE_URL: &str = "https://raw.githubusercontent.com/landaire/wows-replay-data/main";

/// GitHub API endpoint for the tip commit of the repository's main branch.
const REPO_TIP_API_URL: &str = "https://api.github.com/repos/landaire/wows-replay-data/commits/main";

/// Maximum number of concurrent file downloads.
const MAX_CONCURRENT_DOWNLOADS: usize = 16;

/// A locally-cached build whose remote data differs from the copy on disk.
#[derive(Debug, Clone)]
pub struct BuildUpdateStatus {
    pub build: u32,
    pub version: String,
}

/// Result of checking the repository for updates to locally-cached builds.
#[derive(Debug, Clone)]
pub struct UpdateCheck {
    /// The repository's current tip commit, to persist for the next check.
    pub tip: String,
    /// Builds present locally whose remote data has changed.
    pub updates: Vec<BuildUpdateStatus>,
}

/// Counts of the ways a locally-cached build can diverge from the remote repo.
#[derive(Debug, Clone, Default)]
pub struct BuildIssues {
    /// Content objects the remote metadata references but the local CAS lacks.
    pub missing_objects: usize,
    /// Local content objects whose bytes no longer hash to their name.
    pub corrupt_objects: usize,
    /// Extracted files the metadata lists but that are absent from the build tree.
    pub missing_files: usize,
    /// The local metadata references different content than the remote copy.
    pub stale_metadata: bool,
}

impl BuildIssues {
    /// Whether the build matches the remote repo with all content intact.
    pub fn is_clean(&self) -> bool {
        self.missing_objects == 0 && self.corrupt_objects == 0 && self.missing_files == 0 && !self.stale_metadata
    }
}

/// Outcome of validating one locally-cached build against the remote repo.
#[derive(Debug, Clone)]
pub enum ValidationOutcome {
    /// Local content matches the remote repo exactly.
    Clean,
    /// The build is cached locally but no longer published upstream, so there
    /// is no source of truth to validate it against.
    MissingFromRemote,
    /// Local content diverges from the remote repo and should be re-downloaded.
    NeedsRepair(BuildIssues),
}

/// Validation result for a single locally-cached build.
#[derive(Debug, Clone)]
pub struct BuildValidation {
    pub build: u32,
    pub version: String,
    pub outcome: ValidationOutcome,
}

/// Result of validating every locally-cached build against the remote repo.
#[derive(Debug, Clone)]
pub struct CacheValidation {
    /// The repository tip at validation time, to persist when the cache is clean.
    pub tip: String,
    pub builds: Vec<BuildValidation>,
}

impl CacheValidation {
    /// Builds that diverge from the remote repo and need re-downloading.
    pub fn needs_repair(&self) -> impl Iterator<Item = &BuildValidation> {
        self.builds.iter().filter(|b| matches!(b.outcome, ValidationOutcome::NeedsRepair(_)))
    }
}

/// Fetch the current tip commit SHA of the repository's main branch.
pub async fn fetch_repo_tip(client: &reqwest::Client) -> Result<String, Report> {
    let response = client
        .get(REPO_TIP_API_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .attach_with(|| "failed to request repository tip")?
        .error_for_status()
        .attach_with(|| "error status fetching repository tip")?;
    let bytes = response.bytes().await.attach_with(|| "failed to read repository tip response")?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).attach_with(|| "failed to parse repository tip response")?;
    json.get("sha")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| report!("repository tip response missing 'sha'"))
}

/// Determine which locally-cached builds have newer data published upstream.
///
/// Fetches the repository tip first; when it matches `known_tip` nothing has
/// changed and no per-build requests are made. Otherwise each cached build's
/// `metadata.toml` (the manifest of every content hash it references) is
/// compared against the remote copy, so the returned list names exactly the
/// builds whose content differs.
pub async fn check_for_updates(
    client: &reqwest::Client,
    base_url: &str,
    output_base: &Path,
    known_tip: Option<&str>,
) -> Result<UpdateCheck, Report> {
    let tip = fetch_repo_tip(client).await?;
    if known_tip == Some(tip.as_str()) {
        return Ok(UpdateCheck { tip, updates: Vec::new() });
    }

    let local = BuildsIndex::load(&output_base.join("builds.toml"));
    let remote = fetch_builds_index(client, base_url).await?;

    let mut updates = Vec::new();
    for entry in &local.builds {
        let meta_path = output_base.join(&entry.dir).join("metadata.toml");
        if !meta_path.exists() {
            continue;
        }
        let Some(remote_entry) = remote.find_by_build(entry.build) else {
            continue;
        };
        let url = format!("{base_url}/{}/metadata.toml", remote_entry.dir);
        let Some(remote_text) = get_text(client, &url).await? else {
            continue;
        };
        let remote_md: BuildMetadata = match toml::from_str(&remote_text) {
            Ok(md) => md,
            Err(e) => {
                tracing::warn!("could not parse remote metadata for build {}: {e}", entry.build);
                continue;
            }
        };
        let differs = match BuildMetadata::load(&meta_path) {
            Some(local_md) => local_md.files != remote_md.files || local_md.derived != remote_md.derived,
            None => true,
        };
        if differs {
            updates.push(BuildUpdateStatus { build: entry.build, version: entry.version.clone() });
        }
    }

    Ok(UpdateCheck { tip, updates })
}

/// Validate every locally-cached build against the remote repository, which is
/// the source of truth.
///
/// For each cached build the remote `metadata.toml` is fetched and every content
/// object it references is checked for presence and integrity in the local CAS,
/// the extracted build tree is checked for the same files, and the local
/// metadata is compared against the remote copy to catch stale data. Shared
/// content objects are read and hashed at most once across all builds.
/// `on_progress(completed, total)` is invoked as each build is validated.
pub async fn validate_cache(
    client: &reqwest::Client,
    base_url: &str,
    output_base: &Path,
    on_progress: impl Fn(u64, u64),
) -> Result<CacheValidation, Report> {
    let tip = fetch_repo_tip(client).await?;
    let local = BuildsIndex::load(&output_base.join("builds.toml"));
    let remote = fetch_builds_index(client, base_url).await?;
    let cas_root = cas::cas_root(output_base);

    let present: Vec<&BuildEntry> =
        local.builds.iter().filter(|e| output_base.join(&e.dir).join("metadata.toml").exists()).collect();
    let total = present.len() as u64;
    on_progress(0, total);

    // Verdicts for content objects, so a hash shared by many builds is read and
    // hashed once rather than once per referencing build.
    let mut verified: BTreeSet<String> = BTreeSet::new();
    let mut corrupt: BTreeSet<String> = BTreeSet::new();

    let mut builds = Vec::with_capacity(present.len());
    for (i, entry) in present.iter().enumerate() {
        let outcome = match remote.find_by_build(entry.build) {
            None => ValidationOutcome::MissingFromRemote,
            Some(remote_entry) => {
                let url = format!("{base_url}/{}/metadata.toml", remote_entry.dir);
                match get_text(client, &url).await? {
                    None => ValidationOutcome::MissingFromRemote,
                    Some(text) => {
                        let remote_md: BuildMetadata =
                            toml::from_str(&text).attach_with(|| "failed to parse remote metadata.toml")?;
                        let issues = validate_build(
                            &cas_root,
                            &output_base.join(&entry.dir),
                            &remote_md,
                            &mut verified,
                            &mut corrupt,
                        );
                        if issues.is_clean() {
                            ValidationOutcome::Clean
                        } else {
                            ValidationOutcome::NeedsRepair(issues)
                        }
                    }
                }
            }
        };
        builds.push(BuildValidation { build: entry.build, version: entry.version.clone(), outcome });
        on_progress(i as u64 + 1, total);
    }

    Ok(CacheValidation { tip, builds })
}

/// Verdict for one content object during validation.
enum ObjectState {
    Ok,
    Missing,
    Corrupt,
}

/// Check one build's local content against its remote metadata.
fn validate_build(
    cas_root: &Path,
    output_dir: &Path,
    remote_md: &BuildMetadata,
    verified: &mut BTreeSet<String>,
    corrupt: &mut BTreeSet<String>,
) -> BuildIssues {
    let mut issues = BuildIssues::default();

    issues.stale_metadata = match BuildMetadata::load(&output_dir.join("metadata.toml")) {
        Some(local) => local.files != remote_md.files || local.derived != remote_md.derived,
        None => true,
    };

    check_entries(cas_root, &output_dir.join("vfs"), &remote_md.files, &mut issues, verified, corrupt);
    check_entries(cas_root, output_dir, &remote_md.derived, &mut issues, verified, corrupt);

    issues
}

/// Check every `(relative path, hash)` pair in `entries`, accumulating any
/// missing or corrupt objects and any absent files under `tree_root`.
fn check_entries(
    cas_root: &Path,
    tree_root: &Path,
    entries: &BTreeMap<String, String>,
    issues: &mut BuildIssues,
    verified: &mut BTreeSet<String>,
    corrupt: &mut BTreeSet<String>,
) {
    for (rel, hash) in entries {
        match object_state(cas_root, hash, verified, corrupt) {
            ObjectState::Ok => {}
            ObjectState::Missing => issues.missing_objects += 1,
            ObjectState::Corrupt => issues.corrupt_objects += 1,
        }
        if !tree_root.join(rel).exists() {
            issues.missing_files += 1;
        }
    }
}

/// Resolve a content object's state, reusing prior verdicts. Present objects are
/// read and re-hashed once; the result is cached so later builds reuse it.
fn object_state(
    cas_root: &Path,
    hash: &str,
    verified: &mut BTreeSet<String>,
    corrupt: &mut BTreeSet<String>,
) -> ObjectState {
    if verified.contains(hash) {
        return ObjectState::Ok;
    }
    if corrupt.contains(hash) {
        return ObjectState::Corrupt;
    }
    match std::fs::read(cas::cas_path(cas_root, hash)) {
        Err(_) => ObjectState::Missing,
        Ok(bytes) => {
            if cas::hash_bytes(&bytes) == hash {
                verified.insert(hash.to_string());
                ObjectState::Ok
            } else {
                corrupt.insert(hash.to_string());
                ObjectState::Corrupt
            }
        }
    }
}

/// Fetch and parse the remote `builds.toml` index.
pub async fn fetch_builds_index(client: &reqwest::Client, base_url: &str) -> Result<BuildsIndex, Report> {
    let url = format!("{base_url}/builds.toml");
    let body = get_text(client, &url).await?.ok_or_else(|| report!("remote builds.toml not found"))?;
    Ok(toml::from_str(&body).attach_with(|| "failed to parse remote builds.toml")?)
}

/// Download a build's data into `output_base`, deduplicating against content
/// already present in the local CAS. Returns the build number actually
/// downloaded, which differs from `target_build` when a version fallback is used.
///
/// `version_hint` is the replay's `major.minor.patch` string, used to fall back
/// to a different build of the same version when no exact match is published.
/// All referenced content (including the content-addressed per-locale
/// translation catalogs) is fetched from the CAS. When `force` is true an
/// existing copy is rebuilt rather than skipped, picking up newer remote data.
/// `on_progress(completed, total)` is invoked as content objects are downloaded.
pub async fn download_build(
    client: &reqwest::Client,
    base_url: &str,
    output_base: &Path,
    target_build: u32,
    version_hint: Option<&str>,
    force: bool,
    on_progress: impl Fn(u64, u64),
) -> Result<u32, Report> {
    let index = fetch_builds_index(client, base_url).await?;
    let (entry, exact) = index
        .resolve_build(target_build, version_hint)
        .ok_or_else(|| report!("no game data published for build {target_build}"))?;
    let entry = entry.clone();
    if !exact {
        tracing::info!(
            "no exact remote data for build {target_build}; downloading {} (build {})",
            entry.version,
            entry.build
        );
    }

    let cas_root = cas::cas_root(output_base);
    let output_dir = output_base.join(&entry.dir);

    // A complete download already on disk only needs to be registered, unless a
    // forced refresh is rebuilding it to pick up newer remote data.
    if !force && output_dir.join("metadata.toml").exists() {
        register_build(output_base, &entry)?;
        return Ok(entry.build);
    }

    // Clear any partial or stale directory before rebuilding. Content objects in
    // the shared common/ store are left in place so the rebuild only fetches what changed.
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)
            .attach_with(|| format!("failed to clear download directory at {}", output_dir.display()))?;
    }

    // The build's metadata lists every content hash it references.
    let meta_url = format!("{base_url}/{}/metadata.toml", entry.dir);
    let meta_text = get_text(client, &meta_url)
        .await?
        .ok_or_else(|| report!("remote metadata.toml not found for {}", entry.dir))?;
    let metadata: BuildMetadata = toml::from_str(&meta_text).attach_with(|| "failed to parse remote metadata.toml")?;

    // Download every referenced object not already in the local CAS.
    let missing: BTreeSet<String> =
        metadata.referenced_hashes().into_iter().filter(|h| !cas::object_exists(&cas_root, h)).collect();
    let total = missing.len() as u64;
    on_progress(0, total);

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
    let mut set = JoinSet::new();
    for hash in missing {
        let client = client.clone();
        let cas_root = cas_root.clone();
        let base_url = base_url.to_string();
        let semaphore = Arc::clone(&semaphore);
        set.spawn(async move {
            let _permit = semaphore.acquire().await.expect("semaphore closed");
            download_object(&client, &base_url, &cas_root, &hash).await
        });
    }
    let mut completed = 0u64;
    while let Some(joined) = set.join_next().await {
        joined.attach_with(|| "download task failed")??;
        completed += 1;
        on_progress(completed, total);
    }

    // Recreate the extracted vfs tree and derived artifacts (rkyv blobs and the
    // content-addressed per-locale translation catalogs) as symlinks into the CAS.
    let vfs_dir = output_dir.join("vfs");
    for (rel, hash) in &metadata.files {
        cas::link_file(&cas_root, hash, &vfs_dir.join(rel))?;
    }
    for (rel, hash) in &metadata.derived {
        cas::link_file(&cas_root, hash, &output_dir.join(rel))?;
    }

    // Versioned constants, when published for this build.
    let constants_url = format!("{base_url}/{}/constants.json", entry.dir);
    if let Some(bytes) = get_bytes(client, &constants_url).await? {
        write_file(&output_dir.join("constants.json"), &bytes)?;
    }

    write_file(&output_dir.join("metadata.toml"), meta_text.as_bytes())?;
    register_build(output_base, &entry)?;

    Ok(entry.build)
}

/// Download a single content object and store it in the CAS, verifying its hash.
async fn download_object(client: &reqwest::Client, base_url: &str, cas_root: &Path, hash: &str) -> Result<(), Report> {
    let url = format!("{base_url}/{}/{}/{}", cas::CAS_DIR, &hash[..2], &hash[2..]);
    let bytes = get_bytes(client, &url).await?.ok_or_else(|| report!("content object {hash} missing from remote"))?;
    let actual = cas::hash_bytes(&bytes);
    if actual != hash {
        bail!("hash mismatch for {hash}: remote object hashed to {actual}");
    }
    cas::store(cas_root, &bytes)?;
    Ok(())
}

/// Add or update the build's entry in the local `builds.toml`.
fn register_build(output_base: &Path, entry: &BuildEntry) -> Result<(), Report> {
    let builds_path = output_base.join("builds.toml");
    let mut index = BuildsIndex::load(&builds_path);
    index.upsert(entry.clone());
    index.save(&builds_path)
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), Report> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).attach_with(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, data).attach_with(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<Option<Vec<u8>>, Report> {
    let response = client.get(url).send().await.attach_with(|| format!("failed to request {url}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status().attach_with(|| format!("error status for {url}"))?;
    let bytes = response.bytes().await.attach_with(|| format!("failed to read body of {url}"))?;
    Ok(Some(bytes.to_vec()))
}

async fn get_text(client: &reqwest::Client, url: &str) -> Result<Option<String>, Report> {
    match get_bytes(client, url).await? {
        Some(bytes) => Ok(Some(String::from_utf8(bytes).attach_with(|| format!("{url} is not valid UTF-8"))?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Network-dependent end-to-end download against the real repository.
    // Run with: cargo test -p wows-data-mgr --features download -- --ignored
    #[ignore]
    #[test]
    fn download_real_build_reconstructs_dump() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let client = reqwest::Client::builder().user_agent("wows-data-mgr-test").build().unwrap();

        let build = runtime
            .block_on(download_build(&client, DEFAULT_REPO_BASE_URL, base, 296659, Some("0.6.13"), false, |_, _| {}))
            .unwrap();
        assert_eq!(build, 296659);

        let build_dir = base.join("0.6.13_296659");
        assert!(build_dir.join("metadata.toml").exists());
        // GameParams.data is symlinked into the CAS and reads back non-empty.
        let gp = std::fs::read(build_dir.join("vfs/content/GameParams.data")).unwrap();
        assert!(!gp.is_empty());
        // The build is registered locally.
        let index = BuildsIndex::load(&base.join("builds.toml"));
        assert!(index.find_by_build(296659).is_some());

        // A second download is a cheap no-op that still reports the same build.
        let again = runtime
            .block_on(download_build(&client, DEFAULT_REPO_BASE_URL, base, 296659, Some("0.6.13"), false, |_, _| {}))
            .unwrap();
        assert_eq!(again, 296659);

        // The freshly-downloaded build matches upstream, so no updates are found.
        let check = runtime.block_on(check_for_updates(&client, DEFAULT_REPO_BASE_URL, base, None)).unwrap();
        assert!(!check.tip.is_empty());
        assert!(check.updates.is_empty(), "expected no updates, got {:?}", check.updates);

        // Passing the known tip short-circuits without per-build requests.
        let cached =
            runtime.block_on(check_for_updates(&client, DEFAULT_REPO_BASE_URL, base, Some(&check.tip))).unwrap();
        assert_eq!(cached.tip, check.tip);
        assert!(cached.updates.is_empty());
    }
}
