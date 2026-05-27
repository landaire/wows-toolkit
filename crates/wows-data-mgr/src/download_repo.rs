//! Download dumped game data from the wows-replay-data repository.
//!
//! Files are fetched as raw content from GitHub. Content-addressed objects that
//! already exist locally are skipped, so downloading a build only transfers the
//! assets it does not already share with builds already in the cache.

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

/// Maximum number of concurrent file downloads.
const MAX_CONCURRENT_DOWNLOADS: usize = 16;

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
/// `locales` lists the translation catalogs to fetch (e.g. the user's locale,
/// its primary language, and `en`). `on_progress(completed, total)` is invoked
/// as content objects are downloaded.
pub async fn download_build(
    client: &reqwest::Client,
    base_url: &str,
    output_base: &Path,
    target_build: u32,
    version_hint: Option<&str>,
    locales: &[String],
    on_progress: impl Fn(u64, u64),
) -> Result<u32, Report> {
    let index = fetch_builds_index(client, base_url).await?;
    let (entry, exact) =
        index.resolve_build(target_build, version_hint).ok_or_else(|| report!("no game data published for build {target_build}"))?;
    let entry = entry.clone();
    if !exact {
        tracing::info!(
            "no exact remote data for build {target_build}; downloading {} (build {})",
            entry.version,
            entry.build
        );
    }

    let cas_root = output_base.join("vfs_common");
    let output_dir = output_base.join(&entry.dir);

    // A complete download already on disk only needs to be registered.
    if output_dir.join("metadata.toml").exists() {
        register_build(output_base, &entry)?;
        return Ok(entry.build);
    }

    // Clear any partial directory left by an interrupted download.
    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir)
            .attach_with(|| format!("failed to clear partial download at {}", output_dir.display()))?;
    }

    // The build's metadata lists every content hash it references.
    let meta_url = format!("{base_url}/{}/metadata.toml", entry.dir);
    let meta_text =
        get_text(client, &meta_url).await?.ok_or_else(|| report!("remote metadata.toml not found for {}", entry.dir))?;
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

    // Recreate the extracted vfs tree and derived artifacts as symlinks into the CAS.
    let vfs_dir = output_dir.join("vfs");
    for (rel, hash) in &metadata.files {
        cas::link_file(&cas_root, hash, &vfs_dir.join(rel))?;
    }
    for (rel, hash) in &metadata.derived {
        cas::link_file(&cas_root, hash, &output_dir.join(rel))?;
    }

    // Translation catalogs are stored as plain files, not content-addressed.
    for locale in locales {
        let rel = format!("translations/{locale}/LC_MESSAGES/global.mo");
        let url = format!("{base_url}/{}/{rel}", entry.dir);
        if let Some(bytes) = get_bytes(client, &url).await? {
            write_file(&output_dir.join(&rel), &bytes)?;
        }
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
async fn download_object(
    client: &reqwest::Client,
    base_url: &str,
    cas_root: &Path,
    hash: &str,
) -> Result<(), Report> {
    let url = format!("{base_url}/vfs_common/{}/{}", &hash[..2], &hash[2..]);
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
        let locales = ["en".to_string()];

        let build = runtime
            .block_on(download_build(&client, DEFAULT_REPO_BASE_URL, base, 296659, Some("0.6.13"), &locales, |_, _| {}))
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
            .block_on(download_build(&client, DEFAULT_REPO_BASE_URL, base, 296659, Some("0.6.13"), &locales, |_, _| {}))
            .unwrap();
        assert_eq!(again, 296659);
    }
}
