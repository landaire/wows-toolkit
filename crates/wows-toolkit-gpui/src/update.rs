//! Release discovery, archive download, executable replacement, and cleanup.

use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;
use rootcause::prelude::*;
use wows_toolkit_viewmodel::update::RELEASES_URL;
use wows_toolkit_viewmodel::update::Release;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("The update runtime is not available")]
    NoRuntime,
    #[error("The running executable has no parent directory: {path:?}")]
    MissingDirectory { path: PathBuf },
    #[error("The update archive does not contain wows_toolkit.exe")]
    MissingExecutable,
    #[error("The update archive does not contain a Windows executable")]
    InvalidExecutable,
    #[error("In-place updates are only supported on Windows")]
    UnsupportedPlatform,
    #[error("Could not restore {current:?} from {backup:?}: {source}")]
    Restore {
        current: PathBuf,
        backup: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn check(proxy: String, cx: &App) -> Task<Result<Option<Release>, rootcause::Report>> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let runtime = runtime.ok_or(UpdateError::NoRuntime)?;
        runtime.block_on(async move {
            let client = crate::http::client(&proxy, reqwest::redirect::Policy::default())
                .context("Could not create the update HTTP client")?;
            check_release(&client, RELEASES_URL, env!("CARGO_PKG_VERSION")).await
        })
    })
}

async fn check_release(
    client: &reqwest::Client,
    url: &str,
    running: &str,
) -> Result<Option<Release>, rootcause::Report> {
    let releases: Vec<Release> = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .context("Could not request the latest release")?
        .error_for_status()
        .context("The release server rejected the update check")?
        .json()
        .await
        .context("Could not read the release information")?;
    Ok(wows_toolkit_viewmodel::update::select_release(releases, running)
        .context("The running app version is invalid")?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Downloaded {
    pub read: u64,
    pub total: Option<u64>,
}

pub fn install(
    asset_url: String,
    proxy: String,
    progress: futures::channel::mpsc::UnboundedSender<Downloaded>,
    cx: &App,
) -> Task<Result<(), rootcause::Report>> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        if !cfg!(target_os = "windows") {
            return Err(UpdateError::UnsupportedPlatform.into());
        }
        let runtime = runtime.ok_or(UpdateError::NoRuntime)?;
        let current = std::env::current_exe().context("Could not locate the running executable")?;
        let client = crate::http::client(&proxy, reqwest::redirect::Policy::default())
            .context("Could not create the update HTTP client")?;
        let archive = runtime.block_on(download_archive(&client, &asset_url, &progress))?;
        // Stage on the same filesystem so replacement uses a rename.
        let directory = current.parent().ok_or_else(|| UpdateError::MissingDirectory { path: current.clone() })?;
        let mut staged = tempfile::Builder::new()
            .prefix(".wows-toolkit-update-")
            .suffix(".exe")
            .tempfile_in(directory)
            .context("Could not stage the update beside the running executable")?;
        extract_executable(archive.reopen().context("Could not read the downloaded archive")?, staged.as_file_mut())?;
        archive.close().context("Could not remove the downloaded update archive")?;
        staged.as_file().sync_all().context("Could not save the extracted update")?;
        // Close the file before renaming or launching it on Windows.
        let staged = staged.into_temp_path();
        swap_in(&current, &staged, |current, replaced| {
            let mut command = std::process::Command::new(current);
            command.arg("finalize-update").arg("--replaced").arg(replaced);
            crate::child_process::prepare(&mut command).spawn().map(|_| ())
        })?;
        std::process::exit(0);
    })
}

async fn download_archive(
    client: &reqwest::Client,
    url: &str,
    progress: &futures::channel::mpsc::UnboundedSender<Downloaded>,
) -> Result<tempfile::NamedTempFile, rootcause::Report> {
    use tokio::io::AsyncWriteExt as _;

    let mut response = client
        .get(url)
        .send()
        .await
        .context("Could not download the update")?
        .error_for_status()
        .context("The release server rejected the update download")?;
    let total = response.content_length();
    let archive = tempfile::Builder::new()
        .prefix("wows-toolkit-update-")
        .suffix(".zip")
        .tempfile()
        .context("Could not create the update archive file")?;
    let mut file = tokio::fs::File::from_std(archive.reopen().context("Could not open the update archive file")?);
    let mut read = 0;
    while let Some(chunk) = response.chunk().await.context("The update download was interrupted")? {
        file.write_all(&chunk).await.context("Could not write the update archive")?;
        read += chunk.len() as u64;
        let _ = progress.unbounded_send(Downloaded { read, total });
    }
    file.flush().await.context("Could not save the update archive")?;
    Ok(archive)
}

fn extract_executable(archive: impl Read + Seek, output: &mut impl Write) -> Result<(), rootcause::Report> {
    let mut archive = zip::ZipArchive::new(archive).context("Could not read the update ZIP archive")?;
    let mut executable = match archive.by_name("wows_toolkit.exe") {
        Ok(executable) => executable,
        Err(zip::result::ZipError::FileNotFound) => return Err(UpdateError::MissingExecutable.into()),
        Err(error) => return Err(error).context("Could not open wows_toolkit.exe in the update archive")?,
    };
    let mut header = [0; 2];
    executable.read_exact(&mut header).context("The update executable is truncated")?;
    if header != *b"MZ" {
        return Err(UpdateError::InvalidExecutable.into());
    }
    output.write_all(&header).context("Could not extract the update executable")?;
    std::io::copy(&mut executable, output).context("Could not extract the update executable")?;
    Ok(())
}

fn swap_in(
    current: &Path,
    downloaded: &Path,
    launch: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), rootcause::Report> {
    let mut name = current.as_os_str().to_owned();
    name.push(".old");
    let replaced = PathBuf::from(name);
    std::fs::rename(current, &replaced).context("Could not move the running executable to its backup")?;
    if let Err(error) = std::fs::rename(downloaded, current) {
        restore(&replaced, current)?;
        return Err(error).context("Could not move the update into place; the original executable was restored")?;
    }
    if let Err(error) = launch(current, &replaced) {
        // Move the failed replacement aside before restoring the running version.
        std::fs::rename(current, downloaded)
            .context("Could not move the failed update aside; the original is in .old")?;
        restore(&replaced, current)?;
        return Err(error).context("Could not start the update; the original executable was restored")?;
    }
    Ok(())
}

fn restore(backup: &Path, current: &Path) -> Result<(), UpdateError> {
    std::fs::rename(backup, current).map_err(|source| UpdateError::Restore {
        current: current.to_path_buf(),
        backup: backup.to_path_buf(),
        source,
    })
}

/// Deletes the executable an update replaced, if that is what the path is.
///
/// Runs before any window: the process that spawned this one is exiting, and the
/// file is only deletable once it has. Refused for anything but this app's own
/// `.old` beside this executable (`validate_finalize_target`), because the path
/// comes from an argument.
pub fn finalize(replaced: &Path) {
    let Ok(current) = std::env::current_exe() else {
        eprintln!("finalize-update: this executable has no path, so nothing is deleted");
        return;
    };
    match wows_toolkit_viewmodel::update::validate_finalize_target(&current, replaced) {
        Ok(target) => {
            // The old process may still be exiting, so a first delete can fail on
            // a file still mapped. A handful of tries over a second is what the
            // egui app's own finalize does.
            for attempt in 0..RETRIES {
                match std::fs::remove_file(&target) {
                    Ok(()) => return,
                    Err(err) if attempt + 1 == RETRIES => {
                        eprintln!("finalize-update: {} could not be deleted: {err}", target.display());
                    }
                    Err(_) => std::thread::sleep(RETRY_WAIT),
                }
            }
        }
        Err(err) => eprintln!("finalize-update: refusing to delete {}: {err}", replaced.display()),
    }
}

const RETRIES: usize = 10;
const RETRY_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            archive.start_file(*name, zip::write::SimpleFileOptions::default()).expect("ZIP entry");
            archive.write_all(bytes).expect("ZIP contents");
        }
        archive.finish().expect("ZIP directory").into_inner()
    }

    fn response(status: &str, body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("test HTTP listener");
        let url = format!("http://{}", listener.local_addr().expect("listener address"));
        let header = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("HTTP connection");
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).expect("request timeout");
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("HTTP request");
                request.push(byte[0]);
            }
            stream.write_all(header.as_bytes()).expect("HTTP response headers");
            stream.write_all(&body).expect("HTTP response body");
        });
        (url, server)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("test runtime")
    }

    const RELEASE: &str = r###"[{
        "tag_name": "v1.0.2",
        "draft": false,
        "prerelease": false,
        "html_url": "https://github.com/landaire/wows-toolkit/releases/tag/v1.0.2",
        "body": "## Changes\nUpdate notes",
        "assets": [
            {"name": "wows_toolkit_tools_v1.0.2_win64.zip", "browser_download_url": "https://example.invalid/tools.zip"},
            {"name": "wows_toolkit_v1.0.2_windows.zip", "browser_download_url": "https://example.invalid/app.zip"}
        ]
    }]"###;

    #[test]
    fn checking_an_older_build_finds_the_published_archive() {
        let (url, server) = response("200 OK", RELEASE.as_bytes().to_vec());
        let client = reqwest::Client::builder().no_proxy().build().expect("test HTTP client");
        let release =
            runtime().block_on(check_release(&client, &url, "1.0.0")).expect("release check").expect("newer release");
        server.join().expect("HTTP server");
        assert_eq!(release.tag_name, "v1.0.2");
        assert_eq!(
            release.asset_for_windows().expect("application asset").browser_download_url,
            "https://example.invalid/app.zip"
        );
    }

    #[test]
    fn an_http_failure_is_not_reported_as_a_release() {
        let (url, server) = response("403 Forbidden", RELEASE.as_bytes().to_vec());
        let client = reqwest::Client::builder().no_proxy().build().expect("test HTTP client");
        assert!(runtime().block_on(check_release(&client, &url, "1.0.0")).is_err());
        server.join().expect("HTTP server");
    }

    #[test]
    fn downloads_are_extracted_and_report_the_archive_size() {
        let bytes = archive(&[
            ("replayshark.exe", b"MZtool"),
            ("wows_toolkit.exe", b"MZapplication"),
            ("wows_toolkit.pdb", b"symbols"),
        ]);
        let size = bytes.len() as u64;
        let (url, server) = response("200 OK", bytes);
        let client = reqwest::Client::builder().no_proxy().build().expect("test HTTP client");
        let (progress, mut received) = futures::channel::mpsc::unbounded();
        let downloaded = runtime().block_on(download_archive(&client, &url, &progress)).expect("downloaded archive");
        server.join().expect("HTTP server");
        drop(progress);
        let mut output = Vec::new();
        extract_executable(downloaded.reopen().expect("archive file"), &mut output).expect("application extraction");
        assert_eq!(output, b"MZapplication");
        let mut last = None;
        while let Ok(step) = received.try_recv() {
            last = Some(step);
        }
        assert_eq!(last, Some(Downloaded { read: size, total: Some(size) }));
    }

    #[test]
    fn an_http_failure_cannot_be_installed_even_if_its_body_is_a_zip() {
        let (url, server) = response("503 Service Unavailable", archive(&[("wows_toolkit.exe", b"MZapplication")]));
        let client = reqwest::Client::builder().no_proxy().build().expect("test HTTP client");
        let (progress, _) = futures::channel::mpsc::unbounded();
        assert!(runtime().block_on(download_archive(&client, &url, &progress)).is_err());
        server.join().expect("HTTP server");
    }

    #[test]
    fn invalid_archives_do_not_produce_an_executable() {
        for bytes in [
            b"not a ZIP archive".to_vec(),
            archive(&[("replayshark.exe", b"MZtool")]),
            archive(&[("../wows_toolkit.exe", b"MZoutside")]),
            archive(&[("wows_toolkit.exe", b"")]),
            archive(&[("wows_toolkit.exe", b"not an executable")]),
        ] {
            let mut output = Vec::new();
            assert!(extract_executable(Cursor::new(bytes), &mut output).is_err());
            assert!(output.is_empty());
        }
    }

    #[test]
    fn failed_replacement_and_launch_restore_the_running_executable() {
        let dir = tempfile::tempdir().expect("update directory");
        let current = dir.path().join("wows_toolkit.exe");
        let staged = dir.path().join("update.exe");
        let backup = dir.path().join("wows_toolkit.exe.old");
        std::fs::write(&current, b"original").expect("original executable");
        assert!(swap_in(&current, &staged, |_, _| panic!("a missing update cannot launch")).is_err());
        assert_eq!(std::fs::read(&current).expect("restored executable"), b"original");
        assert!(!backup.exists());
        std::fs::write(&staged, b"replacement").expect("staged executable");
        assert!(
            swap_in(&current, &staged, |current, backup| {
                assert_eq!(std::fs::read(current).expect("replacement executable"), b"replacement");
                assert_eq!(std::fs::read(backup).expect("backup executable"), b"original");
                Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "test launch refusal"))
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&current).expect("restored executable"), b"original");
        assert_eq!(std::fs::read(&staged).expect("failed replacement"), b"replacement");
        assert!(!backup.exists());
    }

    #[test]
    fn successful_replacement_leaves_the_backup_for_the_child() {
        let dir = tempfile::tempdir().expect("update directory");
        let current = dir.path().join("wows_toolkit.exe");
        let staged = dir.path().join("update.exe");
        std::fs::write(&current, b"original").expect("original executable");
        std::fs::write(&staged, b"replacement").expect("staged executable");
        swap_in(&current, &staged, |current, backup| {
            assert_eq!(std::fs::read(current).expect("replacement executable"), b"replacement");
            assert_eq!(std::fs::read(backup).expect("backup executable"), b"original");
            assert_eq!(
                wows_toolkit_viewmodel::update::validate_finalize_target(current, backup).expect("cleanup target"),
                backup
            );
            Ok(())
        })
        .expect("replacement and launch");
        assert!(!staged.exists());
        assert_eq!(std::fs::read(&current).expect("replacement executable"), b"replacement");
    }

    #[test]
    #[ignore = "Downloads and extracts a published Windows release"]
    fn published_release_updates_version_1_0_0() {
        let runtime = runtime();
        let client = crate::http::client("", reqwest::redirect::Policy::default()).expect("release HTTP client");
        let release = runtime
            .block_on(check_release(&client, RELEASES_URL, "1.0.0"))
            .expect("published release check")
            .expect("release newer than 1.0.0");
        let asset = release.asset_for_windows().expect("published Windows application archive");
        let (progress, _) = futures::channel::mpsc::unbounded();
        let downloaded = runtime
            .block_on(download_archive(&client, &asset.browser_download_url, &progress))
            .expect("published archive download");
        let mut extracted = tempfile::tempfile().expect("executable staging file");
        extract_executable(downloaded.reopen().expect("archive file"), &mut extracted)
            .expect("published executable extraction");
        assert!(extracted.metadata().expect("executable metadata").len() > 1024);
    }
}
