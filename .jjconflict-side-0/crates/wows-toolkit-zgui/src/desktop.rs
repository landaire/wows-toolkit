//! Handing a directory or a link to the desktop: its file manager, its browser.
//!
//! zgui has no opener of its own, so this runs the platform's: `explorer.exe`, `open` or
//! `xdg-open`, each of which takes a directory or a URL.

use std::ffi::OsStr;
use std::path::Path;

/// Shows `dir` in the desktop's file manager.
pub fn open_directory(dir: &Path) {
    run_opener(dir.as_os_str());
}

/// Opens `url` in the desktop's browser.
pub fn open_url(url: &str) {
    run_opener(OsStr::new(url));
}

/// Shows the toolkit's storage directory in the file manager.
///
/// The directory is created first: it does not exist until something has been written there, and
/// opening a path that is not there yet reads as a broken button rather than an empty folder.
pub fn open_data_directory() {
    let Some(dir) = wows_toolkit_config::storage_dir() else {
        tracing::warn!("there is no storage directory to open");
        return;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!("the storage directory could not be created: {err}");
        return;
    }

    open_directory(&dir);
}

fn run_opener(target: &OsStr) {
    #[cfg(target_os = "windows")]
    let opener = "explorer.exe";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let opener = "xdg-open";

    let mut command = std::process::Command::new(opener);
    command.arg(target);
    if let Err(err) = wows_toolkit_services::child_process::prepare(&mut command).spawn() {
        tracing::warn!("{} could not be opened: {err}", target.to_string_lossy());
    }
}
