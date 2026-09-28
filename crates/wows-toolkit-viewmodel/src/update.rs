//! Updating the app, and cleaning up after an update.
//!
//! Shared, because the finalize step is a delete driven by an argument one
//! process hands another: one implementation of what may be deleted, with its
//! tests, rather than one per front end.

use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

/// Why a replaced-binary path was rejected. This gates a delete driven by an
/// untrusted argument, so each rejection carries the paths that caused it.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FinalizeError {
    #[error("replaced binary {replaced:?} is not in the same directory as {current:?}")]
    DifferentDirectory { replaced: PathBuf, current: PathBuf },
    #[error("replaced binary {replaced:?} does not have a .old extension")]
    UnexpectedName { replaced: PathBuf },
    #[error("replaced binary {replaced:?} does not exist")]
    Missing { replaced: PathBuf },
}

/// Decide whether `replaced` may be deleted by this process.
///
/// The directory check keeps an argument from naming a file elsewhere on the
/// system. The extension check narrows it further: every version that emits
/// this argument produces `<exe>.old`.
///
/// Versions v0.1.10 through v0.1.40 spawned the replacement using `argv[0]`,
/// which is relative when the app was launched by name from a shell, while
/// `current_exe()` is always absolute. Normalizing both sides before the
/// directory comparison is what makes those old launches still match; without
/// it, `Some("")` (the relative parent) never equals an absolute directory
/// and every update from those versions silently fails to finalize.
///
/// Returns the normalized `replaced` path on success. The caller must delete
/// that returned path, not the argument it passed in: this makes the "nothing
/// outside the executable's directory can be deleted" property true by
/// construction, instead of resting on the raw argument happening to already
/// be lexically equivalent to what was validated.
pub fn validate_finalize_target(current_exe: &Path, replaced: &Path) -> Result<PathBuf, FinalizeError> {
    // std::path::absolute is purely lexical (no filesystem access), so falling
    // back to the un-normalized path on error only keeps the comparison
    // stricter and can never widen what this function agrees to delete.
    let current_exe = std::path::absolute(current_exe).unwrap_or_else(|_| current_exe.to_path_buf());
    let replaced = std::path::absolute(replaced).unwrap_or_else(|_| replaced.to_path_buf());

    if current_exe.parent() != replaced.parent() {
        return Err(FinalizeError::DifferentDirectory { replaced, current: current_exe });
    }

    if replaced.extension() != Some("old".as_ref()) {
        return Err(FinalizeError::UnexpectedName { replaced });
    }

    if !replaced.exists() {
        return Err(FinalizeError::Missing { replaced });
    }

    // remove_file (via CreateFileW with FILE_FLAG_OPEN_REPARSE_POINT on
    // Windows) unlinks a symlink or junction itself rather than following it,
    // so the directory check above still bounds what actually gets deleted
    // even if `replaced` is a reparse point.
    Ok(replaced)
}

/// Where the app's own releases are published.
pub const RELEASES_URL: &str = "https://api.github.com/repos/landaire/wows-toolkit/releases/latest";

/// A published release, as much of it as an update needs.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Release {
    /// The tag, which carries the version with a leading `v`.
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

impl Release {
    /// The version this release is, from its tag.
    ///
    /// `None` for a tag that is not a version, which is a release this app has no
    /// way to compare itself against rather than a newer one.
    pub fn version(&self) -> Option<semver::Version> {
        semver::Version::parse(self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name)).ok()
    }

    /// Whether this release is newer than `running`.
    pub fn is_newer_than(&self, running: &str) -> bool {
        match (self.version(), semver::Version::parse(running).ok()) {
            (Some(released), Some(running)) => released > running,
            // A version neither side can parse is not an update: an app that
            // cannot tell must not offer to replace itself.
            _ => false,
        }
    }

    /// The asset to download on this platform, by file extension.
    ///
    /// Windows only, as the egui app's updater is: nothing else has a single-file
    /// executable to swap.
    pub fn asset_for_windows(&self) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|asset| asset.name.ends_with(".exe"))
    }
}

#[cfg(test)]
mod release_tests {
    use super::*;

    fn release(tag: &str, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.to_owned(),
            body: None,
            assets: assets
                .iter()
                .map(|name| ReleaseAsset {
                    name: (*name).to_owned(),
                    browser_download_url: format!("https://example.invalid/{name}"),
                })
                .collect(),
        }
    }

    /// A newer tag is an update; the same one and an older one are not.
    #[test]
    fn only_a_higher_version_is_an_update() {
        assert!(release("v1.0.3", &[]).is_newer_than("1.0.2"));
        assert!(!release("v1.0.2", &[]).is_newer_than("1.0.2"));
        assert!(!release("v1.0.1", &[]).is_newer_than("1.0.2"));
    }

    /// A pre-release orders below its release, which is what semver says and what
    /// keeps a beta from offering to replace the release it precedes.
    #[test]
    fn a_pre_release_does_not_replace_a_release() {
        assert!(!release("v1.0.2-beta1", &[]).is_newer_than("1.0.2"));
        assert!(release("v1.0.2", &[]).is_newer_than("1.0.2-beta1"));
    }

    /// A tag that is not a version offers nothing: this app cannot tell whether
    /// it is newer, and guessing would replace a build with an older one.
    #[test]
    fn an_unparseable_tag_is_not_an_update() {
        assert!(!release("nightly", &[]).is_newer_than("1.0.2"));
        assert!(!release("v1.0.3", &[]).is_newer_than("not-a-version"));
    }

    /// The executable is what is downloaded, not the installer or the archive
    /// beside it.
    #[test]
    fn the_windows_asset_is_the_executable() {
        let picked = release("v1.0.3", &["wows_toolkit.msi", "wows_toolkit.exe", "notes.txt"])
            .asset_for_windows()
            .map(|asset| asset.name.clone());
        assert_eq!(picked.as_deref(), Some("wows_toolkit.exe"));
        assert!(release("v1.0.3", &["wows_toolkit.msi"]).asset_for_windows().is_none());
    }
}
