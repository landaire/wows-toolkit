//! Where a locally rebuilt derived artifact goes.
//!
//! A dump is content-addressed and may be a shared checkout of the published
//! archive, so an artifact that no longer loads (a game-params cache written
//! against an older `wowsunpack::game_params::cache` format) is rebuilt outside
//! the dump. Every tool that reads dumps resolves the same directory, so one
//! rebuild serves the GUI and the CLIs alike.

use std::path::PathBuf;

/// Application name the storage directory is derived from. The GUI resolves its
/// override root through this module, so the two cannot drift.
const APP_NAME: &str = "WoWs Toolkit";

/// App data directory:
///
/// - Windows: `%APPDATA%\WoWs Toolkit\data`
/// - macOS:   `~/Library/Application Support/WoWs-Toolkit`
/// - Linux:   `$XDG_DATA_HOME/wowstoolkit` or `~/.local/share/wowstoolkit`
pub fn storage_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from).map(|p| p.join(APP_NAME).join("data"))
    }
    #[cfg(target_os = "macos")]
    {
        home::home_dir().map(|p| {
            p.join("Library").join("Application Support").join(APP_NAME.replace(|c: char| c.is_ascii_whitespace(), "-"))
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home::home_dir().map(|p| p.join(".local").join("share")))
            .map(|p| p.join(APP_NAME.to_lowercase().replace(|c: char| c.is_ascii_whitespace(), "")))
    }
}

/// Directory whose contents shadow the derived artifacts of `build`.
///
/// Keyed by build, never by version: 18 of the 109 versions in the published
/// archive have more than one build, so a version-keyed cache would let one
/// server's params answer for another's.
pub fn build_override_root(build: u32) -> Option<PathBuf> {
    Some(storage_dir()?.join("build_overrides").join(build.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn override_root_is_keyed_by_build() {
        let Some(first) = super::build_override_root(13015811) else {
            eprintln!("skipping: no storage dir on this machine");
            return;
        };
        let second = super::build_override_root(12830008).unwrap();

        // Build numbers are per-server: two dumps of one version must not share
        // a rebuilt cache.
        assert_ne!(first, second);
        assert!(first.ends_with("build_overrides/13015811") || first.ends_with("build_overrides\\13015811"));
    }
}
