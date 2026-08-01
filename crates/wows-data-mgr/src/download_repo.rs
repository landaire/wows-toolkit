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
use crate::builds::CorruptObject;
use crate::cas;

/// Base URL of the published game data repository, served as raw files.
pub const DEFAULT_REPO_BASE_URL: &str = "https://raw.githubusercontent.com/landaire/wows-replay-data/main";

/// GitHub API endpoint for the tip commit of the repository's main branch.
const REPO_TIP_API_URL: &str = "https://api.github.com/repos/landaire/wows-replay-data/commits/main";

/// Maximum number of concurrent file downloads.
const MAX_CONCURRENT_DOWNLOADS: usize = 16;

/// Maximum number of attempts for a single GET before its error is surfaced.
const MAX_GET_ATTEMPTS: u32 = 4;

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
    /// The local metadata references different content than the remote copy.
    pub stale_metadata: bool,
}

impl BuildIssues {
    /// Whether the build matches the remote repo with all content intact.
    pub fn is_clean(&self) -> bool {
        self.missing_objects == 0 && self.corrupt_objects == 0 && !self.stale_metadata
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
                        let mut corrupt_here = BTreeSet::new();
                        let issues = validate_build(
                            &cas_root,
                            &output_base.join(&entry.dir),
                            &remote_md,
                            &mut verified,
                            &mut corrupt,
                            &mut corrupt_here,
                        );
                        log_corrupt_objects(entry, &corrupt_here);
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

/// Write the identity of the objects a build's validation found corrupt to the
/// log.
///
/// The caller of [`validate_cache`] only ever sees a count, and a count is not
/// something a user can report or a maintainer can act on. This line is the
/// only place the failing objects are named, which is the same standard the
/// download path is held to.
fn log_corrupt_objects(entry: &BuildEntry, corrupt: &BTreeSet<String>) {
    if corrupt.is_empty() {
        return;
    }
    tracing::error!(
        "build {} ({}) references {} corrupt content object(s): {}",
        entry.build,
        entry.version,
        corrupt.len(),
        corrupt.iter().cloned().collect::<Vec<_>>().join(", ")
    );
}

/// Check one build's local content against its remote metadata.
///
/// `corrupt` is the store-wide verdict cache, so an object already known bad is
/// not re-read; `corrupt_here` collects every bad object *this* build
/// references, including ones an earlier build already found.
fn validate_build(
    cas_root: &Path,
    output_dir: &Path,
    remote_md: &BuildMetadata,
    verified: &mut BTreeSet<String>,
    corrupt: &mut BTreeSet<String>,
    corrupt_here: &mut BTreeSet<String>,
) -> BuildIssues {
    let mut issues = BuildIssues {
        stale_metadata: match BuildMetadata::load(&output_dir.join("metadata.toml")) {
            Some(local) => local.files != remote_md.files || local.derived != remote_md.derived,
            None => true,
        },
        ..Default::default()
    };

    check_entries(cas_root, &remote_md.files, &mut issues, verified, corrupt, corrupt_here);
    check_entries(cas_root, &remote_md.derived, &mut issues, verified, corrupt, corrupt_here);

    issues
}

/// Check every content hash in `entries`, accumulating any missing or corrupt
/// objects in the shared CAS store.
fn check_entries(
    cas_root: &Path,
    entries: &BTreeMap<String, String>,
    issues: &mut BuildIssues,
    verified: &mut BTreeSet<String>,
    corrupt: &mut BTreeSet<String>,
    corrupt_here: &mut BTreeSet<String>,
) {
    for hash in entries.values() {
        match object_state(cas_root, hash, verified, corrupt) {
            ObjectState::Ok => {}
            ObjectState::Missing => issues.missing_objects += 1,
            ObjectState::Corrupt => {
                issues.corrupt_objects += 1;
                corrupt_here.insert(hash.clone());
            }
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

/// Whether the remote publishes data for a requested build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAvailability {
    /// The remote has this exact build.
    Exact,
    /// The remote has no exact match; this is the closest published build.
    /// Downloading it may still not satisfy the replay that asked for it.
    Nearest { version: String, build: u32 },
    /// The remote publishes nothing usable for this build.
    Unpublished,
    /// The remote could not be asked: the build resolved in the index, but
    /// fetching its metadata failed. Distinct from `Unpublished` because
    /// reporting a network failure as an absence tells the user something
    /// untrue about their data.
    Unreachable,
}

/// The outcome of resolving one requested build against the remote index.
#[derive(Debug, Clone)]
pub struct ResolvedBuild {
    pub requested_build: u32,
    /// The version hint the caller supplied for this request, if any.
    pub requested_version: Option<String>,
    pub availability: RemoteAvailability,
}

/// A deduplicated plan for downloading a selection of builds.
///
/// `unique_missing_objects` is the size of the union of every selected build's
/// referenced hashes, minus what the local CAS already has. It is not a sum
/// over `resolved`: the CAS is shared, so overlapping builds contribute fewer
/// new objects than their individual sizes would suggest, and a build that is
/// a strict subset of another already-selected build adds nothing at all.
#[derive(Debug, Clone)]
pub struct DownloadPlan {
    pub unique_missing_objects: usize,
    pub resolved: Vec<ResolvedBuild>,
}

/// Distinct CAS objects that must be fetched for a selection of builds.
///
/// The union is taken across the whole selection rather than per build: the
/// CAS is shared, so adjacent builds overlap heavily and a per-build sum would
/// overstate the real transfer.
pub fn plan_objects_to_fetch(per_build_hashes: &[BTreeSet<String>], is_local: impl Fn(&str) -> bool) -> usize {
    per_build_hashes.iter().flatten().collect::<BTreeSet<_>>().into_iter().filter(|hash| !is_local(hash)).count()
}

/// Resolve each requested build against the index. `None` means the remote has
/// no exact or nearest-version match; that request must not abort the plan.
fn resolve_requests(index: &BuildsIndex, builds: &[(u32, Option<String>)]) -> Vec<Option<(BuildEntry, bool)>> {
    builds
        .iter()
        .map(|(build, version)| index.resolve_build(*build, version.as_deref()).map(|(e, exact)| (e.clone(), exact)))
        .collect()
}

/// The distinct build entries resolved requests point at. Two requests can
/// resolve to the same entry through nearest-version fallback; deduplicating
/// here means that entry's `metadata.toml` is fetched only once.
fn distinct_entries(resolutions: &[Option<(BuildEntry, bool)>]) -> Vec<BuildEntry> {
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for (entry, _) in resolutions.iter().flatten() {
        if seen.insert(entry.dir.clone()) {
            entries.push(entry.clone());
        }
    }
    entries
}

/// Outcome of fetching and parsing one resolved build entry's `metadata.toml`.
///
/// `entries` passed to `fetch_entry_hashes` have already resolved in the
/// index, so every one of these outcomes is about a build the index knows
/// about; the question is only whether its metadata could be read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryHashesOutcome {
    /// The metadata was fetched and parsed; these are the hashes it references.
    Fetched(BTreeSet<String>),
    /// The remote gave a definitive answer: `metadata.toml` does not exist at
    /// this path (a 404), even though the index lists the build.
    NotFound,
    /// The remote never gave a definitive answer: the request failed, or the
    /// response it returned could not be parsed as metadata.
    Unreachable,
}

/// Classify one `metadata.toml` fetch outcome. Pure, so the 404-vs-network-
/// failure and valid-vs-unparseable distinctions can be unit tested without a
/// live network.
fn classify_metadata_response(dir: &str, fetch_result: Result<Option<String>, Report>) -> EntryHashesOutcome {
    match fetch_result {
        Ok(Some(text)) => match toml::from_str::<BuildMetadata>(&text) {
            Ok(metadata) => EntryHashesOutcome::Fetched(metadata.referenced_hashes().into_iter().collect()),
            Err(e) => {
                tracing::warn!("could not parse remote metadata.toml for {dir}: {e}");
                EntryHashesOutcome::Unreachable
            }
        },
        Ok(None) => {
            tracing::warn!("remote metadata.toml missing for {dir}");
            EntryHashesOutcome::NotFound
        }
        Err(e) => {
            tracing::warn!("failed to fetch remote metadata.toml for {dir}: {e}");
            EntryHashesOutcome::Unreachable
        }
    }
}

/// Fetch `metadata.toml` for each entry, keyed by directory. Neither a 404 nor
/// a fetch or parse failure aborts the loop, so one bad build's metadata does
/// not prevent planning the others.
async fn fetch_entry_hashes(
    client: &reqwest::Client,
    base_url: &str,
    entries: &[BuildEntry],
) -> BTreeMap<String, EntryHashesOutcome> {
    let mut result = BTreeMap::new();
    for entry in entries {
        let url = format!("{base_url}/{}/metadata.toml", entry.dir);
        let outcome = classify_metadata_response(&entry.dir, get_text(client, &url).await);
        result.insert(entry.dir.clone(), outcome);
    }
    result
}

/// Assemble the final plan from resolved requests and their fetched hash sets.
/// Pure: takes the already-fetched (or already-failed) hashes for each distinct
/// directory, so it can be exercised without touching the network.
fn build_plan(
    builds: &[(u32, Option<String>)],
    resolutions: &[Option<(BuildEntry, bool)>],
    entry_hashes: &BTreeMap<String, EntryHashesOutcome>,
    is_local: impl Fn(&str) -> bool,
) -> DownloadPlan {
    let per_build_hashes: Vec<BTreeSet<String>> = entry_hashes
        .values()
        .filter_map(|outcome| match outcome {
            EntryHashesOutcome::Fetched(hashes) => Some(hashes.clone()),
            EntryHashesOutcome::NotFound | EntryHashesOutcome::Unreachable => None,
        })
        .collect();
    let unique_missing_objects = plan_objects_to_fetch(&per_build_hashes, is_local);

    let resolved = builds
        .iter()
        .zip(resolutions.iter())
        .map(|((build, version), resolution)| {
            let availability = match resolution {
                None => RemoteAvailability::Unpublished,
                Some((entry, exact)) => match entry_hashes.get(&entry.dir) {
                    Some(EntryHashesOutcome::Fetched(_)) if *exact => RemoteAvailability::Exact,
                    Some(EntryHashesOutcome::Fetched(_)) => {
                        RemoteAvailability::Nearest { version: entry.version.clone(), build: entry.build }
                    }
                    Some(EntryHashesOutcome::Unreachable) => RemoteAvailability::Unreachable,
                    Some(EntryHashesOutcome::NotFound) | None => RemoteAvailability::Unpublished,
                },
            };
            ResolvedBuild { requested_build: *build, requested_version: version.clone(), availability }
        })
        .collect();

    DownloadPlan { unique_missing_objects, resolved }
}

/// Download a selection of builds one after another into the shared CAS.
///
/// Sequential rather than concurrent: each build's missing-object set is
/// computed against the CAS as it starts, so a build that runs after another
/// fetches only what that one did not. Adjacent builds overlap by well over 90
/// percent, so running them concurrently multiplies the transfer by the number
/// of builds.
///
/// One build's failure does not stop the rest; the caller gets a result per
/// request, in request order.
pub async fn download_builds(
    client: &reqwest::Client,
    base_url: &str,
    output_base: &Path,
    requests: &[(u32, Option<String>)],
    force: bool,
    on_progress: impl Fn(u64, u64),
) -> Vec<Result<u32, Report>> {
    // One plan up front gives the bar a real total: the union of every selected
    // build's missing objects, which is exactly what the sequential downloads
    // below fetch. `None` when the plan could not be reached, in which case the
    // running total stands in and the bar grows as each build is opened.
    let planned_total: Option<u64> = plan_download(client, base_url, &cas::cas_root(output_base), requests)
        .await
        .inspect_err(|e| tracing::warn!("could not plan the download total: {e}"))
        .ok()
        .map(|plan| plan.unique_missing_objects as u64);

    drive_downloads_with_total(
        requests,
        planned_total,
        on_progress,
        async |build: u32, version: Option<&str>, on_build_progress: &dyn Fn(u64, u64)| {
            download_build(client, base_url, output_base, build, version, force, on_build_progress).await
        },
    )
    .await
}

/// Run `download_one` over each request in turn, accumulating progress across
/// the whole selection.
///
/// `download_one` is a parameter so the sequencing, the accumulation and the
/// per-build failure isolation are exercisable without a network or a CAS.
async fn drive_downloads_with_total<F>(
    requests: &[(u32, Option<String>)],
    planned_total: Option<u64>,
    on_progress: impl Fn(u64, u64),
    download_one: F,
) -> Vec<Result<u32, Report>>
where
    F: AsyncFn(u32, Option<&str>, &dyn Fn(u64, u64)) -> Result<u32, Report>,
{
    let mut results = Vec::with_capacity(requests.len());
    // Objects fetched by the builds already done. `Cell` because the progress
    // closure borrows it while the loop advances it between builds.
    let completed_before = std::cell::Cell::new(0u64);

    for (build, version) in requests {
        let build_total = std::cell::Cell::new(0u64);
        let result = {
            let report = |done: u64, total: u64| {
                build_total.set(total);
                let running = completed_before.get() + total;
                on_progress(completed_before.get() + done, planned_total.unwrap_or(running).max(running));
            };
            download_one(*build, version.as_deref(), &report).await
        };

        match &result {
            Ok(downloaded) => tracing::info!("downloaded game data for build {downloaded}"),
            Err(e) => tracing::warn!("failed to download build {build}: {e}"),
        }
        completed_before.set(completed_before.get() + build_total.get());
        results.push(result);
    }

    results
}

#[cfg(test)]
/// `drive_downloads_with_total` with no planned total, which is what the tests
/// drive: they assert the accumulation the loop does, not a total fetched from
/// a network they do not have.
async fn drive_downloads<F>(
    requests: &[(u32, Option<String>)],
    download_one: F,
    on_progress: impl Fn(u64, u64),
) -> Vec<Result<u32, Report>>
where
    F: AsyncFn(u32, Option<&str>, &dyn Fn(u64, u64)) -> Result<u32, Report>,
{
    drive_downloads_with_total(requests, None, on_progress, download_one).await
}

/// Plan a deduplicated download of a selection of builds: resolve each one
/// against the remote index, fetch each distinct resolved build's metadata at
/// most once, and report the union of missing CAS objects across the whole
/// selection alongside each build's remote availability.
pub async fn plan_download(
    client: &reqwest::Client,
    base_url: &str,
    cas_root: &Path,
    builds: &[(u32, Option<String>)],
) -> Result<DownloadPlan, Report> {
    let index = fetch_builds_index(client, base_url).await?;
    let resolutions = resolve_requests(&index, builds);
    let entries = distinct_entries(&resolutions);
    let entry_hashes = fetch_entry_hashes(client, base_url, &entries).await;
    Ok(build_plan(builds, &resolutions, &entry_hashes, |h| cas::object_exists(cas_root, h)))
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
    on_progress: &dyn Fn(u64, u64),
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
        match joined.attach_with(|| "download task failed")? {
            Ok(()) => {}
            Err(ObjectFailure::HashMismatch { hash, actual }) => {
                return Err(report_corrupt_object(&entry, &metadata, &hash, &actual));
            }
            Err(ObjectFailure::Other(report)) => return Err(report),
        }
        completed += 1;
        on_progress(completed, total);
    }

    // Builds are read through a CAS-backed VFS, so no symlinked vfs/ tree or
    // derived links are materialized. The downloaded content objects plus
    // metadata.toml are sufficient.

    // Versioned constants, when published for this build.
    let constants_url = format!("{base_url}/{}/constants.json", entry.dir);
    if let Some(bytes) = get_bytes(client, &constants_url).await? {
        write_file(&output_dir.join("constants.json"), &bytes)?;
    }

    write_file(&output_dir.join("metadata.toml"), meta_text.as_bytes())?;
    register_build(output_base, &entry)?;

    Ok(entry.build)
}

/// Why fetching one content object failed.
enum ObjectFailure {
    /// Every attempt served bytes that do not hash to the object's name. Bad
    /// data at rest rather than a transient fault, and the only failure the
    /// caller can attribute to specific files.
    HashMismatch { hash: String, actual: String },
    /// The object was missing from the remote, or the transfer itself failed.
    Other(Report),
}

impl From<Report> for ObjectFailure {
    fn from(report: Report) -> Self {
        Self::Other(report)
    }
}

/// Turn a hash mismatch into a reportable failure, naming the build and the
/// files the object backs. The full file list goes to the log; the returned
/// message is bounded because it reaches a toast.
fn report_corrupt_object(entry: &BuildEntry, metadata: &BuildMetadata, hash: &str, actual: &str) -> Report {
    let corrupt = CorruptObject::attribute(entry, metadata, hash, actual);
    tracing::error!(
        "build {} ({}) references corrupt content object {} (hashed to {}) after {MAX_GET_ATTEMPTS} attempts; \
         it backs {} file(s): {}",
        corrupt.build,
        corrupt.version,
        corrupt.hash,
        corrupt.actual,
        corrupt.files.len(),
        corrupt.all_files()
    );
    report!("{corrupt}")
}

/// Download a single content object, verify it against its expected hash, and
/// store it in the CAS.
///
/// A hash mismatch is retried first: the object is re-fetched with exponential
/// backoff, so a one-off bad response does not abort a whole build's download.
/// A mismatch that persists across every attempt is reported as
/// [`ObjectFailure::HashMismatch`] so the caller can name the build and files it
/// breaks. Only verified bytes are ever stored, so the CAS never holds an object
/// that does not match its name.
async fn download_object(
    client: &reqwest::Client,
    base_url: &str,
    cas_root: &Path,
    hash: &str,
) -> Result<(), ObjectFailure> {
    let url = format!("{base_url}/{}/{}/{}", cas::CAS_DIR, &hash[..2], &hash[2..]);
    let mut attempt = 0;
    loop {
        attempt += 1;
        let bytes =
            get_bytes(client, &url).await?.ok_or_else(|| report!("content object {hash} missing from remote"))?;
        let actual = cas::hash_bytes(&bytes);
        if actual == hash {
            let cas_root = cas_root.to_path_buf();
            tokio::task::spawn_blocking(move || cas::store(&cas_root, &bytes))
                .await
                .map_err(|e| ObjectFailure::Other(report!("CAS store task failed: {e}")))??;
            return Ok(());
        }
        if attempt >= MAX_GET_ATTEMPTS {
            return Err(ObjectFailure::HashMismatch { hash: hash.to_string(), actual });
        }
        let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1));
        tracing::warn!(
            "hash mismatch for {hash} (got {actual}, attempt {attempt}/{MAX_GET_ATTEMPTS}), retrying in {delay:?}"
        );
        tokio::time::sleep(delay).await;
    }
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

/// Whether a request error is a transient failure worth retrying: connect and
/// timeout errors, interrupted body reads, and server-side or rate-limit
/// statuses. Client errors (e.g. a 404, handled separately) are not retried.
fn is_retryable(err: &reqwest::Error) -> bool {
    err.is_timeout()
        || err.is_connect()
        || err.is_request()
        || err.is_body()
        || err.is_decode()
        || matches!(err.status(), Some(status) if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS)
}

/// One GET attempt: `Ok(None)` for a 404, otherwise the full response body.
async fn get_bytes_once(client: &reqwest::Client, url: &str) -> reqwest::Result<Option<Vec<u8>>> {
    let response = client.get(url).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let bytes = response.error_for_status()?.bytes().await?;
    Ok(Some(bytes.to_vec()))
}

/// GET `url`, returning `None` for a 404. Transient failures are retried with
/// exponential backoff so a single network blip does not abort a multi-object
/// download; the final attempt's error is surfaced.
async fn get_bytes(client: &reqwest::Client, url: &str) -> Result<Option<Vec<u8>>, Report> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match get_bytes_once(client, url).await {
            Ok(result) => return Ok(result),
            Err(err) if attempt < MAX_GET_ATTEMPTS && is_retryable(&err) => {
                let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1));
                tracing::warn!("GET {url} failed (attempt {attempt}/{MAX_GET_ATTEMPTS}), retrying in {delay:?}: {err}");
                tokio::time::sleep(delay).await;
            }
            Err(err) => return Err(err).attach_with(|| format!("failed to GET {url}")).map_err(Into::into),
        }
    }
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
            .block_on(download_build(&client, DEFAULT_REPO_BASE_URL, base, 296659, Some("0.6.13"), false, &|_, _| {}))
            .unwrap();
        assert_eq!(build, 296659);

        let build_dir = base.join("0.6.13_296659");
        assert!(build_dir.join("metadata.toml").exists());
        // No materialized vfs/ tree is created; GameParams.data reads back
        // non-empty through the CAS-backed VFS.
        use std::io::Read;
        let cas = crate::cas_vfs::BuildCas::open(&build_dir).unwrap();
        let mut gp = Vec::new();
        cas.vfs().join("content/GameParams.data").unwrap().open_file().unwrap().read_to_end(&mut gp).unwrap();
        assert!(!gp.is_empty());
        assert!(!build_dir.join("vfs").exists());
        // The build is registered locally.
        let index = BuildsIndex::load(&base.join("builds.toml"));
        assert!(index.find_by_build(296659).is_some());

        // A second download is a cheap no-op that still reports the same build.
        let again = runtime
            .block_on(download_build(&client, DEFAULT_REPO_BASE_URL, base, 296659, Some("0.6.13"), false, &|_, _| {}))
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

    fn hashes(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plan_shared_objects_between_builds_are_counted_once() {
        let a = hashes(&["h1", "h2", "h3"]);
        let b = hashes(&["h2", "h3", "h4"]);
        // Union is h1..h4 = 4, not 3 + 3 = 6.
        assert_eq!(plan_objects_to_fetch(&[a, b], |_| false), 4);
    }

    #[test]
    fn plan_locally_present_objects_are_excluded() {
        let a = hashes(&["h1", "h2", "h3"]);
        assert_eq!(plan_objects_to_fetch(&[a], |h| h == "h2"), 2);
    }

    #[test]
    fn plan_a_build_whose_objects_are_all_local_adds_nothing() {
        let a = hashes(&["h1", "h2"]);
        let b = hashes(&["h3"]);
        // Only h3 is missing; h1 and h2 are already in the CAS.
        assert_eq!(plan_objects_to_fetch(&[a, b], |h| h != "h3"), 1);
    }

    #[test]
    fn plan_an_empty_selection_needs_nothing() {
        assert_eq!(plan_objects_to_fetch(&[], |_| false), 0);
    }

    #[test]
    fn plan_a_build_adding_only_shared_objects_adds_nothing_to_the_total() {
        let a = hashes(&["h1", "h2"]);
        let b = hashes(&["h1", "h2"]);
        assert_eq!(plan_objects_to_fetch(&[a.clone()], |_| false), 2);
        assert_eq!(plan_objects_to_fetch(&[a, b], |_| false), 2);
    }

    fn entry(version: &str, build: u32, dir: &str) -> BuildEntry {
        BuildEntry { version: version.into(), build, dir: dir.into(), dumped_at: String::new() }
    }

    // Two requested builds resolving to the same entry via nearest-version
    // fallback must yield one distinct entry, so plan_download fetches that
    // entry's metadata.toml exactly once instead of twice.
    #[test]
    fn plan_two_requests_resolving_to_the_same_entry_dedupe_to_one() {
        let shared = entry("15.2.0", 12100000, "15.2.0_12100000");
        let resolutions = vec![Some((shared.clone(), false)), Some((shared, true))];
        let distinct = distinct_entries(&resolutions);
        assert_eq!(distinct.len(), 1);
    }

    // Requests resolving to genuinely different entries must not collapse.
    #[test]
    fn plan_requests_resolving_to_different_entries_stay_distinct() {
        let a = entry("15.1.0", 11965230, "15.1.0_11965230");
        let b = entry("15.2.0", 12100000, "15.2.0_12100000");
        let resolutions = vec![Some((a, true)), Some((b, true))];
        assert_eq!(distinct_entries(&resolutions).len(), 2);
    }

    // A build the index cannot resolve at all must not be present in the
    // distinct-entries list; it contributes no hashes and no metadata fetch.
    #[test]
    fn plan_unresolved_requests_contribute_no_entries() {
        let a = entry("15.2.0", 12100000, "15.2.0_12100000");
        let resolutions = vec![Some((a, true)), None];
        assert_eq!(distinct_entries(&resolutions).len(), 1);
    }

    // build_plan is the pure tail of plan_download: everything after the
    // network fetches. Simulating a 404 for one build's metadata.toml lets
    // the "one bad build does not sink the plan" rule be checked without a
    // live network: the failed build's own hashes are excluded, but the
    // other, healthy build's hashes and resolution are unaffected.
    #[test]
    fn plan_a_metadata_404_yields_unpublished_without_sinking_other_builds() {
        let good = entry("15.2.0", 12100000, "15.2.0_12100000");
        let bad = entry("15.3.0", 12200000, "15.3.0_12200000");
        let builds = vec![(12100000, Some("15.2.0".to_string())), (12200000, Some("15.3.0".to_string()))];
        let resolutions = vec![Some((good.clone(), true)), Some((bad.clone(), true))];

        let mut entry_hashes = BTreeMap::new();
        entry_hashes.insert(good.dir.clone(), EntryHashesOutcome::Fetched(hashes(&["h1", "h2"])));
        entry_hashes.insert(bad.dir.clone(), EntryHashesOutcome::NotFound);

        let plan = build_plan(&builds, &resolutions, &entry_hashes, |_| false);

        assert_eq!(plan.unique_missing_objects, 2);
        assert_eq!(plan.resolved.len(), 2);
        assert_eq!(plan.resolved[0].availability, RemoteAvailability::Exact);
        assert_eq!(plan.resolved[1].availability, RemoteAvailability::Unpublished);
    }

    // An index that cannot resolve a build at all (no exact or nearest match)
    // must still let the rest of the plan proceed.
    #[test]
    fn plan_an_unresolvable_build_is_unpublished_and_contributes_no_hashes() {
        let good = entry("15.2.0", 12100000, "15.2.0_12100000");
        let builds = vec![(12100000, Some("15.2.0".to_string())), (99999999, None)];
        let resolutions = vec![Some((good.clone(), true)), None];

        let mut entry_hashes = BTreeMap::new();
        entry_hashes.insert(good.dir.clone(), EntryHashesOutcome::Fetched(hashes(&["h1"])));

        let plan = build_plan(&builds, &resolutions, &entry_hashes, |_| false);

        assert_eq!(plan.unique_missing_objects, 1);
        assert_eq!(plan.resolved[0].availability, RemoteAvailability::Exact);
        assert_eq!(plan.resolved[1].availability, RemoteAvailability::Unpublished);
    }

    // Two requests that resolve to the same entry must count that entry's
    // hashes once in the total, matching the deduplication contract of
    // plan_objects_to_fetch, and both requests report accurate availability.
    #[test]
    fn plan_shared_entry_across_two_requests_counts_hashes_once() {
        let shared = entry("15.2.0", 12100000, "15.2.0_12100000");
        let builds = vec![(12100500, Some("15.2.0".to_string())), (12100000, Some("15.2.0".to_string()))];
        let resolutions = vec![Some((shared.clone(), false)), Some((shared.clone(), true))];

        let mut entry_hashes = BTreeMap::new();
        entry_hashes.insert(shared.dir.clone(), EntryHashesOutcome::Fetched(hashes(&["h1", "h2", "h3"])));

        let plan = build_plan(&builds, &resolutions, &entry_hashes, |_| false);

        assert_eq!(plan.unique_missing_objects, 3);
        assert_eq!(
            plan.resolved[0].availability,
            RemoteAvailability::Nearest { version: "15.2.0".into(), build: 12100000 }
        );
        assert_eq!(plan.resolved[1].availability, RemoteAvailability::Exact);
    }

    // A build that resolves in the index but whose metadata.toml could not be
    // fetched must report `Unreachable`, not `Unpublished`: the index found it,
    // the network just could not confirm what is there. A build that never
    // resolved in the index at all (no exact or nearest match) stays
    // `Unpublished`. Both are asserted in one test so the two outcomes are
    // proven distinguishable, not just individually non-`Exact`.
    #[test]
    fn plan_unreachable_metadata_is_distinct_from_an_unresolvable_build() {
        let reachable_but_broken = entry("15.3.0", 12200000, "15.3.0_12200000");
        let builds = vec![(12200000, Some("15.3.0".to_string())), (99999999, None)];
        let resolutions = vec![Some((reachable_but_broken.clone(), true)), None];

        let mut entry_hashes = BTreeMap::new();
        entry_hashes.insert(reachable_but_broken.dir.clone(), EntryHashesOutcome::Unreachable);

        let plan = build_plan(&builds, &resolutions, &entry_hashes, |_| false);

        assert_eq!(plan.unique_missing_objects, 0);
        assert_eq!(plan.resolved[0].availability, RemoteAvailability::Unreachable);
        assert_eq!(plan.resolved[1].availability, RemoteAvailability::Unpublished);
    }

    // classify_metadata_response is the pure mapping fetch_entry_hashes applies
    // to each metadata.toml request outcome, so the 404-vs-network-failure and
    // valid-vs-unparseable distinctions can be checked without a live network.
    // A 404 (`Ok(None)`) is a definitive answer from the remote -- nothing is
    // there -- so it maps to `NotFound`, matching how `validate_cache` treats a
    // missing `metadata.toml` as `MissingFromRemote` elsewhere in this file. A
    // request error never got a definitive answer at all, so it maps to
    // `Unreachable`, and so does a response that came back but failed to parse.
    #[test]
    fn classify_metadata_response_distinguishes_404_from_network_failure() {
        let not_found = classify_metadata_response("15.2.0_12100000", Ok(None));
        let unreachable = classify_metadata_response("15.2.0_12100000", Err(report!("simulated network failure")));

        assert_eq!(not_found, EntryHashesOutcome::NotFound);
        assert_eq!(unreachable, EntryHashesOutcome::Unreachable);
    }

    #[test]
    fn classify_metadata_response_distinguishes_valid_from_unparseable_toml() {
        let valid_toml = "version = \"15.2.0\"\nbuild = 12100000\n\n[files]\na = \"h1\"\n".to_string();
        let fetched = classify_metadata_response("15.2.0_12100000", Ok(Some(valid_toml)));
        let unparseable = classify_metadata_response("15.2.0_12100000", Ok(Some("not valid toml {{{".to_string())));

        assert_eq!(fetched, EntryHashesOutcome::Fetched(hashes(&["h1"])));
        assert_eq!(unparseable, EntryHashesOutcome::Unreachable);
    }

    // A corrupt object is only reportable if the failure carries the build and
    // the files it breaks: a bare 20-character hash tells the user nothing and
    // gives a bug report nothing to act on.
    #[test]
    fn a_corrupt_object_failure_names_the_build_and_the_files_it_breaks() {
        let entry = entry("15.4.0", 12506899, "15.4.0_12506899");
        let mut metadata = BuildMetadata { version: "15.4.0".into(), build: 12506899, ..Default::default() };
        for i in 0..9 {
            metadata.files.insert(format!("res/spaces/s{i}/space.settings"), "a24a46f62dc08fd95fc7".into());
        }

        let rendered =
            report_corrupt_object(&entry, &metadata, "a24a46f62dc08fd95fc7", "674dcbf6a9204c9fe942").to_string();

        assert!(rendered.contains("15.4.0"), "{rendered}");
        assert!(rendered.contains("12506899"), "{rendered}");
        assert!(rendered.contains("a24a46f62dc08fd95fc7"), "{rendered}");
        assert!(rendered.contains("res/spaces/s0/space.settings"), "{rendered}");
        assert!(rendered.contains("and 6 more"), "{rendered}");
        assert!(!rendered.contains("res/spaces/s3/space.settings"), "capped at three: {rendered}");
        assert!(rendered.contains("re-publishing"), "the user needs to know a retry is pointless: {rendered}");
    }

    /// Collects everything logged on the calling thread.
    #[derive(Clone, Default)]
    struct CapturedLog(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CapturedLog {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Run `body` with a subscriber that captures the log, and return what it
    /// wrote. The default is thread-local, so parallel tests do not see each
    /// other's output.
    fn captured_log(body: impl FnOnce()) -> String {
        let captured = CapturedLog::default();
        let subscriber =
            tracing_subscriber::fmt().with_writer(captured.clone()).with_ansi(false).without_time().finish();
        tracing::subscriber::with_default(subscriber, body);
        let bytes = captured.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    // The returned message is capped because it reaches a toast; the log is
    // where the evidence lives. Asserting only on the message lets the log line
    // be deleted with every test still passing, which is how the requirement
    // that put it there silently regresses.
    #[test]
    fn a_corrupt_object_failure_writes_every_file_to_the_log() {
        let entry = entry("15.4.0", 12506899, "15.4.0_12506899");
        let mut metadata = BuildMetadata { version: "15.4.0".into(), build: 12506899, ..Default::default() };
        let files: Vec<String> = (0..9).map(|i| format!("res/spaces/s{i}/space.settings")).collect();
        for file in &files {
            metadata.files.insert(file.clone(), "a24a46f62dc08fd95fc7".into());
        }

        let log = captured_log(|| {
            let _ = report_corrupt_object(&entry, &metadata, "a24a46f62dc08fd95fc7", "674dcbf6a9204c9fe942");
        });

        assert!(log.contains("a24a46f62dc08fd95fc7"), "{log}");
        assert!(log.contains("674dcbf6a9204c9fe942"), "{log}");
        assert!(log.contains("12506899"), "{log}");
        for file in &files {
            assert!(log.contains(file.as_str()), "the log dropped {file}: {log}");
        }
    }

    // The in-app validation surface reports a bare count. Without this line
    // nothing anywhere records which objects failed, on the surface a user hits
    // before they ever try a download.
    #[test]
    fn validation_writes_the_corrupt_objects_it_found_to_the_log() {
        let entry = entry("15.4.0", 12506899, "15.4.0_12506899");
        let corrupt = BTreeSet::from(["a24a46f62dc08fd95fc7".to_string(), "674dcbf6a9204c9fe942".to_string()]);

        let log = captured_log(|| log_corrupt_objects(&entry, &corrupt));

        assert!(log.contains("a24a46f62dc08fd95fc7"), "{log}");
        assert!(log.contains("674dcbf6a9204c9fe942"), "{log}");
        assert!(log.contains("12506899"), "{log}");
        assert!(log.contains("15.4.0"), "{log}");
    }

    // A build with nothing wrong must not write an error line saying so.
    #[test]
    fn validation_logs_nothing_when_no_object_is_corrupt() {
        let entry = entry("15.4.0", 12506899, "15.4.0_12506899");

        let log = captured_log(|| log_corrupt_objects(&entry, &BTreeSet::new()));

        assert!(log.is_empty(), "{log}");
    }

    // The requested-version hint is `Option<String>` end to end: a supplied
    // hint round-trips as `Some`, and an absent one is `None`, never `""`.
    #[test]
    fn plan_resolved_build_carries_the_version_hint_or_none_exactly() {
        let good = entry("15.2.0", 12100000, "15.2.0_12100000");
        let builds = vec![(12100000, Some("15.2.0".to_string())), (12100000, None)];
        let resolutions = vec![Some((good.clone(), true)), Some((good.clone(), true))];

        let mut entry_hashes = BTreeMap::new();
        entry_hashes.insert(good.dir.clone(), EntryHashesOutcome::Fetched(hashes(&["h1"])));

        let plan = build_plan(&builds, &resolutions, &entry_hashes, |_| false);

        assert_eq!(plan.resolved[0].requested_version, Some("15.2.0".to_string()));
        assert_eq!(plan.resolved[1].requested_version, None);
    }

    /// The CAS is shared, so a build downloaded after another must only fetch what
    /// the first one did not. Computing every build's missing set before any object
    /// has landed makes adjacent builds each fetch the full set; they overlap by
    /// well over 90 percent in practice, so that multiplies the real transfer by
    /// the number of builds.
    #[test]
    fn a_later_build_only_fetches_what_the_earlier_one_did_not() {
        let objects = |build: u32| -> BTreeSet<String> {
            match build {
                1 => hashes(&["a", "b", "c"]),
                _ => hashes(&["b", "c", "d"]),
            }
        };
        let stored = std::cell::RefCell::new(BTreeSet::<String>::new());
        let fetched = std::cell::Cell::new(0usize);

        let results = futures::executor::block_on(drive_downloads(
            &[(1, None), (2, None)],
            |build: u32, _version: Option<&str>, _on_progress: &dyn Fn(u64, u64)| {
                let missing: Vec<String> =
                    objects(build).into_iter().filter(|h| !stored.borrow().contains(h)).collect();
                fetched.set(fetched.get() + missing.len());
                stored.borrow_mut().extend(missing);
                std::future::ready(Ok(build))
            },
            |_, _| {},
        ));

        assert_eq!(fetched.get(), 4, "b and c are fetched once across both builds, not twice");
        assert_eq!(results.len(), 2);
    }

    /// One build's failure is not the selection's. The rest still download, and the
    /// caller gets a result per request so it can say which build went missing.
    #[test]
    fn a_failed_build_does_not_stop_the_ones_after_it() {
        let results = futures::executor::block_on(drive_downloads(
            &[(1, None), (2, None), (3, None)],
            |build: u32, _version: Option<&str>, _on_progress: &dyn Fn(u64, u64)| {
                std::future::ready(if build == 2 { Err(report!("build 2 is unreachable")) } else { Ok(build) })
            },
            |_, _| {},
        ));

        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok(), "a failure must not sink the builds queued behind it");
    }

    /// The bar spans the selection. Resetting per build makes a three-build
    /// download look like it restarts twice.
    #[test]
    fn progress_accumulates_across_builds() {
        let seen = std::cell::RefCell::new(Vec::new());

        futures::executor::block_on(drive_downloads(
            &[(1, None), (2, None)],
            |build: u32, _version: Option<&str>, on_progress: &dyn Fn(u64, u64)| {
                on_progress(0, 2);
                on_progress(2, 2);
                std::future::ready(Ok(build))
            },
            |done, total| seen.borrow_mut().push((done, total)),
        ));

        let seen = seen.borrow();
        assert_eq!(seen.last(), Some(&(4u64, 4u64)), "the second build continues the first build's count");
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "the count never goes backwards");
    }
}
