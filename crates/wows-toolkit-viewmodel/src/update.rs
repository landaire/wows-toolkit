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
pub const RELEASES_URL: &str = "https://api.github.com/repos/landaire/wows-toolkit/releases?per_page=100";

/// A published release, as much of it as an update needs.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Release {
    /// The tag, which carries the version with a leading `v`.
    pub tag_name: String,
    pub html_url: String,
    pub draft: bool,
    pub prerelease: bool,
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

    /// The application archive, excluding the installer and command-line tools.
    pub fn asset_for_windows(&self) -> Option<&ReleaseAsset> {
        let name = format!("wows_toolkit_{}_windows.zip", self.tag_name);
        self.assets.iter().find(|asset| asset.name == name)
    }
}

/// Select the newest published version in the running build's update channel.
pub fn select_release(releases: Vec<Release>, running: &str) -> Result<Option<Release>, semver::Error> {
    let running = semver::Version::parse(running)?;
    Ok(releases
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| release.version().map(|version| (version, release)))
        .filter(|(version, release)| {
            version > &running && (!running.pre.is_empty() || (version.pre.is_empty() && !release.prerelease))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, release)| release))
}

#[cfg(test)]
mod release_tests {
    use super::*;

    fn release(tag: &str, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.to_owned(),
            draft: false,
            prerelease: tag.contains('-'),
            html_url: format!("https://github.com/landaire/wows-toolkit/releases/tag/{tag}"),
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

    #[test]
    fn update_channels_select_the_newest_published_version() {
        let mut draft = release("v2.0.0", &[]);
        draft.draft = true;
        let mut marked_prerelease = release("v1.3.0", &[]);
        marked_prerelease.prerelease = true;
        let releases = vec![
            release("v1.1.0-beta2", &[]),
            release("v1.0.2", &[]),
            draft,
            release("nightly", &[]),
            marked_prerelease,
            release("v1.2.0-beta1", &[]),
        ];
        assert_eq!(select_release(releases.clone(), "1.0.0").unwrap().unwrap().tag_name, "v1.0.2");
        assert_eq!(select_release(releases, "1.1.0-beta1").unwrap().unwrap().tag_name, "v1.3.0");
        assert_eq!(select_release(vec![release("v1.1.0", &[])], "1.1.0-beta1").unwrap().unwrap().tag_name, "v1.1.0");
        assert!(select_release(vec![release("v1.1.0-beta1", &[])], "1.1.0-beta1").unwrap().is_none());
        assert!(select_release(vec![], "invalid").is_err());
    }

    #[test]
    fn the_windows_asset_is_the_application_archive() {
        let picked = release(
            "v1.0.2",
            &[
                "wows-toolkit-v1.0.2-windows-x86_64.msi",
                "wows_toolkit_tools_v1.0.2_windows.zip",
                "wows_toolkit_tools_v1.0.2_win64.zip",
                "wows_toolkit_v1.0.2_windows.zip",
                "wows_toolkit.pdb",
            ],
        )
        .asset_for_windows()
        .map(|asset| asset.name.clone());
        assert_eq!(picked.as_deref(), Some("wows_toolkit_v1.0.2_windows.zip"));
        assert!(release("v1.0.3", &["wows_toolkit.msi"]).asset_for_windows().is_none());
    }
}
