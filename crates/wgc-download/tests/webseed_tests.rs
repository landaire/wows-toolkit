//! Webseed downloader tests against a small hand-rolled HTTP/1.1 server
//! (raw `std::net::TcpListener`, one thread per connection) - no external
//! HTTP server crate needed for a handful of fixed-body GET responses.

use std::collections::HashMap;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread;

use sha1::Digest;
use sha1::Sha1;
use wgc_download::error::DownloadError;
use wgc_download::torrent::TorrentFile;
use wgc_download::torrent::TorrentMeta;
use wgc_download::webseed::DownloadProgress;
use wgc_download::webseed::DownloadRequest;
use wgc_download::webseed::download;

struct TestServer {
    addr: SocketAddr,
    requests: Arc<AtomicUsize>,
}

impl TestServer {
    /// Serves `routes` (URL path -> response body) over HTTP/1.1 on a
    /// random loopback port, one thread per connection, until the process
    /// exits (test binaries are short-lived; nothing explicitly shuts this
    /// down).
    fn start(routes: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("test server local addr");
        let routes = Arc::new(routes);
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_thread = Arc::clone(&requests);

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let routes = Arc::clone(&routes);
                let requests = Arc::clone(&requests_for_thread);
                thread::spawn(move || handle_connection(stream, &routes, &requests));
            }
        });

        TestServer { addr, requests }
    }

    fn web_seed_base(&self) -> String {
        format!("http://{}/", self.addr)
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn handle_connection(mut stream: TcpStream, routes: &HashMap<String, Vec<u8>>, requests: &AtomicUsize) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone test server stream"));

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    loop {
        let mut header_line = String::new();
        match reader.read_line(&mut header_line) {
            Ok(0) => break,
            Ok(_) if header_line == "\r\n" || header_line == "\n" => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }

    let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
    requests.fetch_add(1, Ordering::SeqCst);

    let response = match routes.get(&path) {
        Some(body) => {
            let mut head =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
            head.extend_from_slice(body);
            head
        }
        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
}

fn sha1_hex(body: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(body);
    hex::encode(hasher.finalize())
}

fn single_file_torrent(name: &str, path: &str, body: &[u8]) -> TorrentMeta {
    TorrentMeta {
        name: name.to_string(),
        piece_length: 16_384,
        total_length: body.len() as u64,
        files: vec![TorrentFile {
            path: path.split('/').map(str::to_string).collect(),
            length: body.len() as u64,
            sha1: Some(sha1_hex(body)),
        }],
        unpacked_sizes: Default::default(),
    }
}

#[tokio::test]
async fn downloads_and_verifies_a_file() {
    let body = b"hello webseed world, this is a small fixture body".to_vec();
    let torrent = single_file_torrent("build1", "hotfix.dspkg", &body);
    let mut routes = HashMap::new();
    routes.insert("/build1/hotfix.dspkg".to_string(), body.clone());
    let server = TestServer::start(routes);

    let staging_dir = tempdir();
    let client = reqwest::Client::new();
    let req = DownloadRequest {
        staging_dir: staging_dir.clone(),
        torrent,
        web_seed_base: server.web_seed_base(),
        is_steam_install: false,
    };

    let progress_calls: Arc<std::sync::Mutex<Vec<DownloadProgress>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let progress_calls_cb = Arc::clone(&progress_calls);
    download(&client, &req, move |p| progress_calls_cb.lock().expect("progress lock").push(p))
        .await
        .expect("download should succeed");

    let written = std::fs::read(staging_dir.join("hotfix.dspkg")).expect("downloaded file should exist");
    assert_eq!(written, body);

    let calls = progress_calls.lock().expect("progress lock");
    let last = calls.last().expect("at least one progress call");
    assert_eq!(last.files_done, 1);
    assert_eq!(last.files_total, 1);
    assert_eq!(last.downloaded_bytes, body.len() as u64);

    cleanup(&staging_dir);
}

#[tokio::test]
async fn wrong_expected_sha1_yields_hash_mismatch() {
    let body = b"some content that will not match the wrong hash".to_vec();
    let mut torrent = single_file_torrent("build2", "hotfix.dspkg", &body);
    torrent.files[0].sha1 = Some("0".repeat(40));
    let mut routes = HashMap::new();
    routes.insert("/build2/hotfix.dspkg".to_string(), body);
    let server = TestServer::start(routes);

    let staging_dir = tempdir();
    let client = reqwest::Client::new();
    let req = DownloadRequest {
        staging_dir: staging_dir.clone(),
        torrent,
        web_seed_base: server.web_seed_base(),
        is_steam_install: false,
    };

    let err = download(&client, &req, |_| {}).await.expect_err("mismatched sha1 should error");
    match err {
        DownloadError::HashMismatch { expected, .. } => assert_eq!(expected, "0".repeat(40)),
        other => panic!("expected HashMismatch, got {other:?}"),
    }

    cleanup(&staging_dir);
}

#[tokio::test]
async fn steam_install_is_refused_before_any_request() {
    let body = b"unreachable".to_vec();
    let torrent = single_file_torrent("build3", "hotfix.dspkg", &body);
    // Deliberately point at a base with nothing listening: if `download`
    // ever tried to make a request here, it would fail with a transport
    // error rather than SteamInstallRefused, so this also proves the
    // refusal happens before any network I/O.
    let req = DownloadRequest {
        staging_dir: tempdir(),
        torrent,
        web_seed_base: "http://127.0.0.1:1/".to_string(),
        is_steam_install: true,
    };
    let client = reqwest::Client::new();

    let err = download(&client, &req, |_| {}).await.expect_err("steam install should be refused");
    assert!(matches!(err, DownloadError::SteamInstallRefused));

    cleanup(&req.staging_dir);
}

#[tokio::test]
async fn rerun_skips_an_already_verified_file() {
    let body = b"content downloaded once, then resumed/skipped".to_vec();
    let torrent = single_file_torrent("build4", "hotfix.dspkg", &body);
    let mut routes = HashMap::new();
    routes.insert("/build4/hotfix.dspkg".to_string(), body.clone());
    let server = TestServer::start(routes);

    let staging_dir = tempdir();
    let client = reqwest::Client::new();
    let req = DownloadRequest {
        staging_dir: staging_dir.clone(),
        torrent,
        web_seed_base: server.web_seed_base(),
        is_steam_install: false,
    };

    download(&client, &req, |_| {}).await.expect("first download should succeed");
    let count_after_first = server.request_count();
    assert_eq!(count_after_first, 1);

    download(&client, &req, |_| {}).await.expect("rerun should succeed by skipping");
    assert_eq!(server.request_count(), count_after_first, "rerun should not have re-requested the file");

    cleanup(&staging_dir);
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wgc-download-test-{}", uuid_ish()));
    std::fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Good-enough unique suffix for a test-scoped temp dir name: no external
/// uuid dependency needed for this.
fn uuid_ish() -> String {
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock").as_nanos();
    format!("{nanos}-{:?}", std::thread::current().id())
}
