//! Parses WG `.torrent` (bencode) manifests for WGC-managed game builds,
//! downloads them over HTTP webseeds (BEP-19, no tracker/DHT/peers), and
//! 7z-extracts each downloaded FULL `.dspkg` into an install directory.
//!
//! `torrent` turns a `.torrent` byte slice into a typed [`TorrentMeta`].
//! `webseed` fetches each real file it describes from a webseed base URL,
//! verifying sha1 as it streams. `unpack` 7z-extracts a downloaded FULL
//! `.dspkg` (DELTA packages are rsync diffs, not yet supported). `install`
//! drives both phases together for one torrent's build.

pub mod error;
pub mod install;
pub mod torrent;
pub mod unpack;
pub mod webseed;

pub use error::DownloadError;
pub use install::InstallOutcome;
pub use install::InstallProgress;
pub use install::InstallRequest;
pub use install::download_and_install;
pub use torrent::TorrentFile;
pub use torrent::TorrentMeta;
pub use torrent::parse_torrent;
pub use unpack::is_delta_package;
pub use unpack::unpack_dspkg;
pub use webseed::DownloadProgress;
pub use webseed::DownloadRequest;
pub use webseed::download;

use std::path::PathBuf;

/// Fetch a `.torrent` manifest's bytes from `url` (a catalog `torrent_urls`
/// entry) and parse it. Separate from [`parse_torrent`] so callers that
/// already have the bytes (tests, cached fixtures) skip the network call.
pub async fn fetch_torrent(client: &reqwest::Client, url: &str) -> Result<TorrentMeta, DownloadError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| DownloadError::Request { url: url.to_string(), detail: e.to_string() })?;
    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Http { url: url.to_string(), status });
    }
    let bytes =
        response.bytes().await.map_err(|e| DownloadError::Request { url: url.to_string(), detail: e.to_string() })?;
    parse_torrent(&bytes)
}

/// The game folder name under whichever platform-standard parent directory
/// `default_install_dir` picks. Always a suggestion: callers (the
/// directory picker) can override it.
const GAME_DIR_NAME: &str = "World_of_Warships";

/// A platform-standard default install directory. Not guaranteed to exist
/// or be writable; it is only a starting point for a directory picker, so
/// this never errors - every branch has a documented, always-available
/// fallback.
///
/// Uses `std::env` rather than the `dirs-next` crate to match how the rest
/// of this workspace resolves platform directories (see
/// `wgc-install::wgc::default_data_dir`, `wgc-launch::proton::find_steam_root`).
pub fn default_install_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        windows_default_install_dir()
    }
    #[cfg(target_os = "macos")]
    {
        macos_default_install_dir()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_default_install_dir()
    }
}

/// WG's own installer defaults to `<system drive>\Games\World_of_Warships`
/// (not under Program Files), so that convention is matched here rather
/// than `%ProgramFiles%`. `SystemDrive` is set by Windows itself for every
/// process; the literal `C:` fallback only matters in the unlikely case a
/// launcher stripped it from the environment.
#[cfg(target_os = "windows")]
fn windows_default_install_dir() -> PathBuf {
    let drive = std::env::var_os("SystemDrive").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:"));
    drive.join("Games").join(GAME_DIR_NAME)
}

/// `~/Applications/World of Warships`, matching macOS's convention of
/// space-separated app names under `~/Applications`. `$HOME` is unset only
/// in unusual environments (e.g. a stripped-down sandbox); the literal
/// `/Users/Shared` fallback keeps this infallible without inventing a
/// per-user path we cannot resolve.
#[cfg(target_os = "macos")]
fn macos_default_install_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/Users/Shared"));
    home.join("Applications").join("World of Warships")
}

/// XDG Base Directory: `$XDG_DATA_HOME/wows-toolkit/World_of_Warships`,
/// else `~/.local/share/wows-toolkit/World_of_Warships` per the XDG
/// fallback spec. `$HOME` unset falls back to `/tmp`, matching this
/// function's "always return something usable" contract; real Linux
/// sessions always have `$HOME` set.
#[cfg(all(unix, not(target_os = "macos")))]
fn linux_default_install_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("wows-toolkit").join(GAME_DIR_NAME);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".local/share/wows-toolkit").join(GAME_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_install_dir_is_nonempty_and_names_the_game() {
        let dir = default_install_dir();
        assert!(!dir.as_os_str().is_empty());
        let name = dir.file_name().and_then(|n| n.to_str()).expect("dir should have a final component");
        assert!(
            name == "World_of_Warships" || name == "World of Warships",
            "expected the final path component to name the game, got {name:?}"
        );
    }
}
