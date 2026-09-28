//! Finding a newer release, installing it, and cleaning up after the one it
//! replaced.
//!
//! The version comparison, the release shape and the rule for what a finalize
//! step may delete are `wows_toolkit_viewmodel::update`, shared with the egui
//! app: both read the same releases and both delete only their own `.old`.
//!
//! Windows only, as the egui updater is: nothing else here ships a single-file
//! executable that can be swapped in place.

use std::path::Path;
use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;
use rust_i18n::t;
use wows_toolkit_viewmodel::update::RELEASES_URL;
use wows_toolkit_viewmodel::update::Release;

/// What a check found.
#[derive(Debug, Clone)]
pub enum Found {
    /// A release newer than this build, and the file to fetch for it.
    Newer { release: Release, asset_url: String },
    /// This build is the newest published one.
    UpToDate,
    /// The check could not be made.
    Failed(String),
}

/// Asks GitHub what the newest release is.
///
/// Plain HTTPS rather than a GitHub client library: one request, one JSON object,
/// and the app already has an HTTP client that honours the proxy this machine is
/// on.
pub fn check(proxy: String, cx: &App) -> Task<Found> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return Found::Failed("no runtime to check on".to_owned()) };
        runtime.block_on(async move {
            let client = match crate::http::client(&proxy, reqwest::redirect::Policy::default()) {
                Ok(client) => client,
                Err(err) => return Found::Failed(err.to_string()),
            };
            // GitHub refuses a request with no user agent; the client sets one.
            let response = match client.get(RELEASES_URL).header(reqwest::header::ACCEPT, GITHUB_JSON).send().await {
                Ok(response) => response,
                Err(err) => return Found::Failed(err.to_string()),
            };
            let release: Release = match response.json().await {
                Ok(release) => release,
                Err(err) => return Found::Failed(err.to_string()),
            };

            if !release.is_newer_than(env!("CARGO_PKG_VERSION")) {
                return Found::UpToDate;
            }
            match release.asset_for_windows() {
                Some(asset) => {
                    let asset_url = asset.browser_download_url.clone();
                    Found::Newer { release, asset_url }
                }
                // Newer, but with nothing this platform can install: saying it is
                // up to date would be wrong, and offering an install that cannot
                // run would be worse.
                None => Found::Failed(t!("ui.dialogs.update_windows_only").into_owned()),
            }
        })
    })
}

const GITHUB_JSON: &str = "application/vnd.github+json";

/// Downloads the new executable beside the running one and swaps it in.
///
/// The running executable cannot be overwritten while it runs, so it is renamed
/// to `.old`, the new one takes its place, and the new one is started with
/// `finalize-update --replaced <old>` to delete what it replaced. This is the
/// same sequence the egui app performs (`app.rs`'s `UpdateDownloaded` arm), so a
/// binary installed by either can be finalized by either.
pub fn install(asset_url: String, proxy: String, cx: &App) -> Task<Result<(), String>> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let runtime = runtime.ok_or_else(|| "no runtime to download on".to_owned())?;
        let current = std::env::current_exe().map_err(|err| err.to_string())?;
        let downloaded = current.with_extension("new");

        runtime.block_on(async {
            let client =
                crate::http::client(&proxy, reqwest::redirect::Policy::default()).map_err(|e| e.to_string())?;
            let bytes = client
                .get(&asset_url)
                .send()
                .await
                .map_err(|err| err.to_string())?
                .bytes()
                .await
                .map_err(|err| err.to_string())?;
            tokio::fs::write(&downloaded, &bytes).await.map_err(|err| err.to_string())
        })?;

        swap_in(&current, &downloaded)
    })
}

/// Puts `downloaded` where `current` is and starts it.
///
/// Never returns on success: the process that replaced itself exits so the new
/// one can delete it.
fn swap_in(current: &Path, downloaded: &Path) -> Result<(), String> {
    let replaced = PathBuf::from({
        let mut name = current.as_os_str().to_owned();
        name.push(".old");
        name
    });

    std::fs::rename(current, &replaced).map_err(|err| format!("the running executable could not be moved: {err}"))?;
    if let Err(err) = std::fs::rename(downloaded, current) {
        // Put it back: a failure here would otherwise leave no executable at the
        // path the shortcut points at.
        let _ = std::fs::rename(&replaced, current);
        return Err(format!("the new executable could not be moved into place: {err}"));
    }

    let started = std::process::Command::new(current)
        .arg("finalize-update")
        .arg("--replaced")
        .arg(&replaced)
        .spawn()
        .map_err(|err| format!("the updated executable would not start: {err}"));
    match started {
        Ok(_) => {
            std::process::exit(0);
        }
        Err(err) => {
            // The new binary is in place but did not start. Leave both where they
            // are: the reader can start it themselves, and the `.old` is what
            // they fall back to.
            Err(err)
        }
    }
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
