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

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
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
pub(crate) const PADDING_FILE_PREFIX: &str = ".____padding_file";

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

/// Progress reported repeatedly while streaming one file's bytes and once
/// more on that file's completion (including a resumed/already-verified
/// skip). `downloaded_bytes`/`total_bytes` are cumulative across the whole
/// torrent, not per-file: `downloaded_bytes` only ever increases across one
/// `download()` call, and `total_bytes` is that torrent's fixed total (sum
/// of its real files' `length`). `file`/`files_done`/`files_total` still
/// describe the in-flight file, for a "file X/Y" label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub file: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// Files fully verified so far, not counting the in-flight `file`.
    pub files_done: usize,
    pub files_total: usize,
}

/// Cooperative stop signals checked while streaming a file's bytes, so a
/// multi-GB in-flight download responds to Cancel/Pause promptly rather
/// than only between files or between torrents. Cancel discards the run;
/// Pause also stops it, but staged (sha1-verified) files stay on disk so a
/// resumed run skips them via [`already_verified`].
#[derive(Clone)]
pub struct DownloadControl {
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
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
    control: &DownloadControl,
    progress: impl Fn(DownloadProgress),
) -> Result<(), DownloadError> {
    if req.is_steam_install {
        return Err(DownloadError::SteamInstallRefused);
    }

    let real_files: Vec<&TorrentFile> =
        req.torrent.files.iter().filter(|f| !f.relative_path().starts_with(PADDING_FILE_PREFIX)).collect();
    let torrent_total: u64 = real_files.iter().map(|f| f.length).sum();
    let ctx = DownloadContext {
        web_seed_base: &req.web_seed_base,
        torrent_name: &req.torrent.name,
        staging_dir: &req.staging_dir,
        files_total: real_files.len(),
        files_done: AtomicUsize::new(0),
        downloaded_total: AtomicU64::new(0),
        torrent_total,
        control,
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
    /// Bytes streamed in so far across every file of this torrent (real
    /// files only). Shared by every concurrent `download_one` call so
    /// `DownloadProgress::downloaded_bytes` is cumulative, not per-file.
    downloaded_total: AtomicU64,
    /// Sum of every real file's `length`; the torrent-wide denominator
    /// reported alongside `downloaded_total`.
    torrent_total: u64,
    control: &'a DownloadControl,
    progress: &'a P,
}

/// Download and verify one file, or skip it if already present and valid.
async fn download_one<P: Fn(DownloadProgress)>(
    client: &reqwest::Client,
    ctx: &DownloadContext<'_, P>,
    file: &TorrentFile,
) -> Result<(), DownloadError> {
    if ctx.control.cancel.load(Ordering::SeqCst) {
        return Err(DownloadError::Cancelled);
    }
    if ctx.control.pause.load(Ordering::SeqCst) {
        return Err(DownloadError::Paused);
    }

    let relative_path = file.relative_path();
    let expected_sha1 =
        file.sha1.as_deref().ok_or_else(|| DownloadError::MissingSha1 { file: relative_path.clone() })?;

    let dest = safe_join(ctx.staging_dir, &file.path)?;
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| DownloadError::Io { detail: format!("creating directory {}: {e}", parent.display()) })?;
    }

    if already_verified(&dest, expected_sha1).await? {
        let done = ctx.files_done.fetch_add(1, Ordering::SeqCst) + 1;
        let cumulative = ctx.downloaded_total.fetch_add(file.length, Ordering::SeqCst) + file.length;
        (ctx.progress)(DownloadProgress {
            file: relative_path,
            downloaded_bytes: cumulative,
            total_bytes: ctx.torrent_total,
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
        if ctx.control.cancel.load(Ordering::SeqCst) {
            return Err(DownloadError::Cancelled);
        }
        if ctx.control.pause.load(Ordering::SeqCst) {
            return Err(DownloadError::Paused);
        }
        let chunk = chunk.map_err(|e| DownloadError::Request { url: url.clone(), detail: e.to_string() })?;
        hasher.update(&chunk);
        out.write_all(&chunk)
            .await
            .map_err(|e| DownloadError::Io { detail: format!("writing {}: {e}", dest.display()) })?;
        downloaded += chunk.len() as u64;
        let cumulative = ctx.downloaded_total.fetch_add(chunk.len() as u64, Ordering::SeqCst) + chunk.len() as u64;
        (ctx.progress)(DownloadProgress {
            file: relative_path.clone(),
            downloaded_bytes: cumulative,
            total_bytes: ctx.torrent_total,
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
    // Chunks already accumulated this file's bytes into `downloaded_total`
    // above; load rather than add again here.
    (ctx.progress)(DownloadProgress {
        file: relative_path,
        downloaded_bytes: ctx.downloaded_total.load(Ordering::SeqCst),
        total_bytes: ctx.torrent_total,
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

/// Join `base` with each of `segments`, rejecting any segment that could
/// escape `base` (path traversal). A torrent's file path segments come
/// from a `.torrent` manifest, which may be malicious or corrupted, so
/// each segment must be exactly one `Component::Normal` - not empty, not
/// `.` or `..`, not an absolute-path root, and not a Windows drive/UNC
/// prefix - before it is trusted as a path component to join.
///
/// Reused by [`crate::unpack`] to guard 7z archive entry names, which are
/// just as untrusted as a torrent manifest's file paths.
pub(crate) fn safe_join(base: &Path, segments: &[String]) -> Result<PathBuf, DownloadError> {
    let mut dest = base.to_path_buf();
    for segment in segments {
        let mut components = Path::new(segment).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(name)), None) => dest.push(name),
            _ => return Err(DownloadError::UnsafePath { path: segments.join("/") }),
        }
    }
    Ok(dest)
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
    use std::collections::BTreeMap;

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

    #[test]
    fn safe_join_accepts_nested_normal_segments() {
        let base = Path::new("/staging");
        let segments = vec!["sub".to_string(), "dir".to_string(), "file.dspkg".to_string()];
        assert_eq!(safe_join(base, &segments).unwrap(), Path::new("/staging/sub/dir/file.dspkg"));
    }

    #[test]
    fn safe_join_rejects_parent_dir_segment() {
        let base = Path::new("/staging");
        let segments = vec!["..".to_string(), "evil.txt".to_string()];
        assert!(matches!(safe_join(base, &segments), Err(DownloadError::UnsafePath { .. })));
    }

    #[test]
    fn safe_join_rejects_parent_dir_in_middle_segment() {
        let base = Path::new("/staging");
        let segments = vec!["a".to_string(), "..".to_string(), "b".to_string()];
        assert!(matches!(safe_join(base, &segments), Err(DownloadError::UnsafePath { .. })));
    }

    #[test]
    fn safe_join_rejects_absolute_segment() {
        let base = Path::new("/staging");
        let segments = vec!["/abs".to_string()];
        assert!(matches!(safe_join(base, &segments), Err(DownloadError::UnsafePath { .. })));
    }

    #[test]
    fn safe_join_rejects_empty_and_current_dir_segments() {
        let base = Path::new("/staging");
        assert!(matches!(safe_join(base, &["".to_string()]), Err(DownloadError::UnsafePath { .. })));
        assert!(matches!(safe_join(base, &[".".to_string()]), Err(DownloadError::UnsafePath { .. })));
    }

    /// Unique per-test directory under the OS temp dir; not removed on
    /// success since these are single-use scratch dirs, and leaving them
    /// makes a failed assertion's on-disk state easy to inspect.
    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("wgc-download-test-{name}-{}-{nanos}", std::process::id()))
    }

    /// A control with both flags clear, for tests that don't exercise
    /// cancel/pause.
    fn no_control() -> DownloadControl {
        DownloadControl { cancel: Arc::new(AtomicBool::new(false)), pause: Arc::new(AtomicBool::new(false)) }
    }

    fn traversal_torrent(file_path: Vec<String>) -> TorrentMeta {
        TorrentMeta {
            name: "wows_test".to_string(),
            piece_length: 1 << 20,
            total_length: 5,
            files: vec![TorrentFile { path: file_path, length: 5, sha1: Some("0".repeat(40)) }],
            unpacked_sizes: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn download_rejects_parent_dir_traversal_and_writes_nothing_outside_staging() {
        let staging_dir = unique_temp_dir("traversal-parent");
        tokio::fs::create_dir_all(&staging_dir).await.unwrap();

        let req = DownloadRequest {
            staging_dir: staging_dir.clone(),
            torrent: traversal_torrent(vec!["..".to_string(), "evil.txt".to_string()]),
            web_seed_base: "http://127.0.0.1:1/unused/".to_string(),
            is_steam_install: false,
        };
        let client = reqwest::Client::new();
        let result = download(&client, &req, &no_control(), |_| {}).await;
        assert!(matches!(result, Err(DownloadError::UnsafePath { .. })), "expected UnsafePath, got {result:?}");

        // The traversal target would land next to staging_dir itself, one
        // level up from where the (rejected) file path points.
        let traversal_target = staging_dir.parent().unwrap().join("evil.txt");
        assert!(!traversal_target.exists(), "traversal target must not be written: {}", traversal_target.display());

        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
    }

    #[tokio::test]
    async fn download_rejects_absolute_path_traversal_and_writes_nothing_outside_staging() {
        let staging_dir = unique_temp_dir("traversal-abs");
        tokio::fs::create_dir_all(&staging_dir).await.unwrap();

        let req = DownloadRequest {
            staging_dir: staging_dir.clone(),
            torrent: traversal_torrent(vec!["/abs-evil.txt".to_string()]),
            web_seed_base: "http://127.0.0.1:1/unused/".to_string(),
            is_steam_install: false,
        };
        let client = reqwest::Client::new();
        let result = download(&client, &req, &no_control(), |_| {}).await;
        assert!(matches!(result, Err(DownloadError::UnsafePath { .. })), "expected UnsafePath, got {result:?}");

        let traversal_target = Path::new("/abs-evil.txt");
        assert!(!traversal_target.exists(), "traversal target must not be written: {}", traversal_target.display());

        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
    }

    /// A pre-set cancel flag must abort before any request is made -- the
    /// same "checked at the very top of `download_one`" guarantee the
    /// `SteamInstallRefused` test above exercises for that refusal.
    #[tokio::test]
    async fn cancel_flag_set_before_start_aborts_without_any_request() {
        let staging_dir = unique_temp_dir("cancel-preset");
        tokio::fs::create_dir_all(&staging_dir).await.unwrap();

        let req = DownloadRequest {
            staging_dir: staging_dir.clone(),
            torrent: traversal_torrent(vec!["file.dspkg".to_string()]),
            web_seed_base: "http://127.0.0.1:1/unused/".to_string(),
            is_steam_install: false,
        };
        let control =
            DownloadControl { cancel: Arc::new(AtomicBool::new(true)), pause: Arc::new(AtomicBool::new(false)) };
        let client = reqwest::Client::new();

        let result = download(&client, &req, &control, |_| {}).await;
        assert!(matches!(result, Err(DownloadError::Cancelled)), "expected Cancelled, got {result:?}");

        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
    }

    /// Mirrors `cancel_flag_set_before_start_aborts_without_any_request`,
    /// but for the pause flag returning `Paused` instead of `Cancelled`.
    #[tokio::test]
    async fn pause_flag_set_before_start_aborts_without_any_request() {
        let staging_dir = unique_temp_dir("pause-preset");
        tokio::fs::create_dir_all(&staging_dir).await.unwrap();

        let req = DownloadRequest {
            staging_dir: staging_dir.clone(),
            torrent: traversal_torrent(vec!["file.dspkg".to_string()]),
            web_seed_base: "http://127.0.0.1:1/unused/".to_string(),
            is_steam_install: false,
        };
        let control =
            DownloadControl { cancel: Arc::new(AtomicBool::new(false)), pause: Arc::new(AtomicBool::new(true)) };
        let client = reqwest::Client::new();

        let result = download(&client, &req, &control, |_| {}).await;
        assert!(matches!(result, Err(DownloadError::Paused)), "expected Paused, got {result:?}");

        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
    }

    /// `downloaded_bytes` reported for a multi-file torrent must accumulate
    /// across files (the bug this fix corrects: it used to reset to 0 at the
    /// start of each new file), and must never decrease between successive
    /// progress calls.
    #[tokio::test]
    async fn downloaded_bytes_accumulate_across_files_and_never_decrease() {
        use std::io::BufRead;
        use std::io::BufReader;
        use std::io::Write;
        use std::net::TcpListener;

        fn sha1_hex(body: &[u8]) -> String {
            let mut hasher = Sha1::new();
            hasher.update(body);
            hex::encode(hasher.finalize())
        }

        // Answers one request with the body whose file name ends the request
        // path. Downloads run concurrently, so accept order is not file order.
        fn serve_once(listener: &TcpListener, bodies: &[(&str, Vec<u8>)]) {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let path = line.split_whitespace().nth(1).expect("request line has a path");
            let body = &bodies.iter().find(|(name, _)| path.ends_with(&format!("/{name}"))).expect("known file").1;
            loop {
                let mut header = String::new();
                match reader.read_line(&mut header) {
                    Ok(0) => break,
                    Ok(_) if header == "\r\n" || header == "\n" => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
            let mut head =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
            head.extend_from_slice(body);
            let _ = stream.write_all(&head);
            let _ = stream.flush();
        }

        let body_a = b"first file body, some bytes".to_vec();
        let body_b = b"second file body, a few more bytes than the first".to_vec();

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bodies = vec![("a.dspkg", body_a.clone()), ("b.dspkg", body_b.clone())];
        let handle = std::thread::spawn(move || {
            for _ in 0..bodies.len() {
                serve_once(&listener, &bodies);
            }
        });

        let staging_dir = unique_temp_dir("accumulate");
        tokio::fs::create_dir_all(&staging_dir).await.unwrap();
        let torrent = TorrentMeta {
            name: "acc_test".to_string(),
            piece_length: 1 << 20,
            total_length: (body_a.len() + body_b.len()) as u64,
            files: vec![
                TorrentFile {
                    path: vec!["a.dspkg".to_string()],
                    length: body_a.len() as u64,
                    sha1: Some(sha1_hex(&body_a)),
                },
                TorrentFile {
                    path: vec!["b.dspkg".to_string()],
                    length: body_b.len() as u64,
                    sha1: Some(sha1_hex(&body_b)),
                },
            ],
            unpacked_sizes: BTreeMap::new(),
        };
        let req = DownloadRequest {
            staging_dir: staging_dir.clone(),
            torrent,
            web_seed_base: format!("http://{addr}/"),
            is_steam_install: false,
        };

        let progress: Arc<std::sync::Mutex<Vec<DownloadProgress>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let progress_cb = Arc::clone(&progress);
        let client = reqwest::Client::new();
        // buffer_unordered(4) runs both files at once; the server answers
        // each by path, so the assertion does not depend on scheduling order.
        download(&client, &req, &no_control(), move |p| progress_cb.lock().expect("progress lock").push(p))
            .await
            .expect("download should succeed");
        handle.join().expect("server thread should not panic");

        let calls = progress.lock().expect("progress lock").clone();
        assert!(!calls.is_empty(), "expected at least one progress call");
        let expected_total = (body_a.len() + body_b.len()) as u64;
        let mut last = 0u64;
        for call in &calls {
            assert!(call.downloaded_bytes >= last, "downloaded_bytes must never decrease: {calls:?}");
            assert_eq!(call.total_bytes, expected_total, "total_bytes must stay the torrent's fixed total");
            last = call.downloaded_bytes;
        }
        assert_eq!(last, expected_total, "final progress call should reach the torrent's full size");

        let _ = tokio::fs::remove_dir_all(&staging_dir).await;
    }
}
