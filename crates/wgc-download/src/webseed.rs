//! BEP-19 (GetRight-style) webseed HTTP downloader for a parsed
//! [`TorrentMeta`].
//!
//! This is deliberately HTTP-only: no tracker announce, no DHT, no peer
//! wire protocol. WG's own CDN serves each torrent's files individually
//! over plain HTTP at a webseed base URL (from the WGUS `web_seeds`
//! element), so a torrent client's swarm machinery is unnecessary -
//! fetching each file directly and verifying its sha1 against the
//! manifest gives the same integrity guarantee with far less surface
//! area and no peer-to-peer exposure.

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use futures::stream::StreamExt;
use futures::stream::{
    self,
};
use sha1::Digest;
use sha1::Sha1;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

use crate::error::DownloadError;
use crate::torrent::TorrentFile;
use crate::torrent::TorrentMeta;

/// How many files download concurrently.
const CONCURRENT_FILES: usize = 4;

/// Naming-convention padding entries some torrent tools emit (as opposed to
/// the BEP47 `attr=p` flag, which `torrent::parse_torrent` already filters
/// before a [`TorrentFile`] exists). Neither is real WG content, so both
/// are skipped defensively.
const PADDING_FILE_PREFIX: &str = ".____padding_file";

/// A request to fetch every real file of one torrent's build into
/// `staging_dir` over HTTP webseeds.
pub struct DownloadRequest {
    pub staging_dir: PathBuf,
    pub torrent: TorrentMeta,
    /// Webseed base URL from the WGUS `web_seeds` element. Ends with `/`.
    pub web_seed_base: String,
    /// Steam owns a Steam install's directory and patches it itself; set
    /// this from the install's `InstallSource` so `download` can refuse
    /// before making any request.
    pub is_steam_install: bool,
}

/// Progress for one file, reported as bytes stream in and once more on
/// completion (including a resumed/already-verified skip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub file: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// Files fully verified so far, not counting the in-flight `file`.
    pub files_done: usize,
    pub files_total: usize,
}

/// Download every real file of `req.torrent` from `req.web_seed_base` into
/// `req.staging_dir`, verifying each against its manifest sha1.
///
/// Up to [`CONCURRENT_FILES`] files download at once. A file already
/// present in `staging_dir` with a matching sha1 is skipped (resume).
/// Any error - wrong status, transport failure, short read, sha1
/// mismatch - aborts the whole download; partial results from files that
/// completed before the error stay on disk (they already passed
/// verification) and are picked up by resume on retry.
pub async fn download(
    client: &reqwest::Client,
    req: &DownloadRequest,
    progress: impl Fn(DownloadProgress),
) -> Result<(), DownloadError> {
    if req.is_steam_install {
        return Err(DownloadError::SteamInstallRefused);
    }

    let real_files: Vec<&TorrentFile> =
        req.torrent.files.iter().filter(|f| !f.relative_path().starts_with(PADDING_FILE_PREFIX)).collect();
    let ctx = DownloadContext {
        web_seed_base: &req.web_seed_base,
        torrent_name: &req.torrent.name,
        staging_dir: &req.staging_dir,
        files_total: real_files.len(),
        files_done: AtomicUsize::new(0),
        progress: &progress,
    };
    let ctx = &ctx;

    let mut results = stream::iter(real_files)
        .map(|file| {
            let client = client.clone();
            async move { download_one(&client, ctx, file).await }
        })
        .buffer_unordered(CONCURRENT_FILES);

    while let Some(result) = results.next().await {
        result?;
    }
    Ok(())
}

/// Shared, per-`download()`-call context passed to each concurrent file
/// download: the pieces of [`DownloadRequest`] and progress-tracking state
/// that every file needs, grouped so `download_one` takes a handful of
/// arguments instead of one per field.
struct DownloadContext<'a, P: Fn(DownloadProgress)> {
    web_seed_base: &'a str,
    torrent_name: &'a str,
    staging_dir: &'a Path,
    files_total: usize,
    files_done: AtomicUsize,
    progress: &'a P,
}

/// Download and verify one file, or skip it if already present and valid.
async fn download_one<P: Fn(DownloadProgress)>(
    client: &reqwest::Client,
    ctx: &DownloadContext<'_, P>,
    file: &TorrentFile,
) -> Result<(), DownloadError> {
    let relative_path = file.relative_path();
    let expected_sha1 =
        file.sha1.as_deref().ok_or_else(|| DownloadError::MissingSha1 { file: relative_path.clone() })?;

    let dest = file.path.iter().fold(ctx.staging_dir.to_path_buf(), |acc, segment| acc.join(segment));
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| DownloadError::Io { detail: format!("creating directory {}: {e}", parent.display()) })?;
    }

    if already_verified(&dest, expected_sha1).await? {
        let done = ctx.files_done.fetch_add(1, Ordering::SeqCst) + 1;
        (ctx.progress)(DownloadProgress {
            file: relative_path,
            downloaded_bytes: file.length,
            total_bytes: file.length,
            files_done: done,
            files_total: ctx.files_total,
        });
        return Ok(());
    }

    let url = build_url(ctx.web_seed_base, ctx.torrent_name, &relative_path);
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| DownloadError::Request { url: url.clone(), detail: e.to_string() })?;

    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Http { url, status });
    }

    let mut out = tokio::fs::File::create(&dest)
        .await
        .map_err(|e| DownloadError::Io { detail: format!("creating {}: {e}", dest.display()) })?;
    let mut hasher = Sha1::new();
    let mut downloaded: u64 = 0;
    let mut body = response.bytes_stream();

    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| DownloadError::Request { url: url.clone(), detail: e.to_string() })?;
        hasher.update(&chunk);
        out.write_all(&chunk)
            .await
            .map_err(|e| DownloadError::Io { detail: format!("writing {}: {e}", dest.display()) })?;
        downloaded += chunk.len() as u64;
        (ctx.progress)(DownloadProgress {
            file: relative_path.clone(),
            downloaded_bytes: downloaded,
            total_bytes: file.length,
            files_done: ctx.files_done.load(Ordering::SeqCst),
            files_total: ctx.files_total,
        });
    }
    out.flush().await.map_err(|e| DownloadError::Io { detail: format!("flushing {}: {e}", dest.display()) })?;
    drop(out);

    if downloaded != file.length {
        return Err(DownloadError::ShortRead { file: relative_path, expected: file.length, got: downloaded });
    }

    let got = hex::encode(hasher.finalize());
    if got != expected_sha1 {
        // Best-effort cleanup so a corrupt file isn't left looking
        // downloaded; the HashMismatch below is returned regardless of
        // whether this succeeds.
        let _ = tokio::fs::remove_file(&dest).await;
        return Err(DownloadError::HashMismatch { file: relative_path, expected: expected_sha1.to_string(), got });
    }

    let done = ctx.files_done.fetch_add(1, Ordering::SeqCst) + 1;
    (ctx.progress)(DownloadProgress {
        file: relative_path,
        downloaded_bytes: downloaded,
        total_bytes: file.length,
        files_done: done,
        files_total: ctx.files_total,
    });
    Ok(())
}

/// `true` if `dest` exists and its sha1 already matches `expected_sha1`
/// (BEP-19 resume: nothing to fetch). A missing file is the expected,
/// legitimate "not downloaded yet" case, not an error. Any other I/O
/// failure while checking an existing file is propagated, never silently
/// treated as "doesn't exist".
async fn already_verified(dest: &Path, expected_sha1: &str) -> Result<bool, DownloadError> {
    match tokio::fs::metadata(dest).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(DownloadError::Io { detail: format!("checking {}: {e}", dest.display()) }),
    }
    let actual = sha1_hex_of_file(dest).await?;
    Ok(actual == expected_sha1)
}

async fn sha1_hex_of_file(path: &Path) -> Result<String, DownloadError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| DownloadError::Io { detail: format!("opening {}: {e}", path.display()) })?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| DownloadError::Io { detail: format!("reading {}: {e}", path.display()) })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Join `web_seed_base` (expected to end with `/`), `torrent_name`, and
/// `relative_path` with exactly one `/` at each boundary, regardless of
/// stray slashes on either side of the join.
fn build_url(web_seed_base: &str, torrent_name: &str, relative_path: &str) -> String {
    let base = web_seed_base.trim_end_matches('/');
    let name = torrent_name.trim_matches('/');
    let rel = relative_path.trim_start_matches('/');
    format!("{base}/{name}/{rel}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_joins_with_single_slashes() {
        assert_eq!(
            build_url("http://example.com/patches/", "wows_1_na", "build/client.dspkg"),
            "http://example.com/patches/wows_1_na/build/client.dspkg"
        );
        // Defensive: still exactly one slash even with stray extras.
        assert_eq!(
            build_url("http://example.com/patches//", "/wows_1_na/", "/build/client.dspkg"),
            "http://example.com/patches/wows_1_na/build/client.dspkg"
        );
    }
}
