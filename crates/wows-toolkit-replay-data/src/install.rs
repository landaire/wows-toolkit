//! What a game install says about itself: the version it runs and where its
//! replays are written.

use std::path::Path;
use std::path::PathBuf;

use wowsunpack::data::Version;

use crate::replay_folder::ReplayFolder;

/// The replays directory of the install at `wows_dir`, as the reader names it:
/// `{wows_dir}/replays`, or the version folder the game writes to. See
/// [`ReplayFolder::from_app_install`].
pub fn resolve_replays_dir(wows_dir: &Path) -> PathBuf {
    ReplayFolder::from_app_install(wows_dir).path().to_path_buf()
}

/// Reads the `<last_server_version>...</last_server_version>` node out of a
/// WoWs `preferences.xml`'s raw contents. Ports
/// `task/replays.rs::current_build_from_preferences`.
pub fn last_server_version(data: &str) -> Option<String> {
    const OPEN: &str = "<last_server_version>";
    const CLOSE: &str = "</last_server_version>";
    let start = data.find(OPEN)?;
    let end_of_node = data[start..].find(CLOSE)?;
    let version_str = &data[start + OPEN.len()..start + end_of_node];
    Some(version_str.trim().to_string())
}

/// The version the install says it is, from its own `preferences.xml`.
///
/// This is what names a dump of it. `None` when the file is absent or names
/// something that will not parse, in which case nothing can be dumped under a
/// name that would be found again.
pub fn installed_version(wows_dir: &Path) -> Option<Version> {
    let preferences = std::fs::read_to_string(wows_dir.join("preferences.xml")).ok()?;
    let version = last_server_version(&preferences)?;
    Version::try_from_client_exe(&version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_server_version_reads_the_node_contents() {
        let xml = "<preferences><clientOptions><last_server_version>13, 11, 0, 12668706</last_server_version></clientOptions></preferences>";
        assert_eq!(last_server_version(xml).as_deref(), Some("13, 11, 0, 12668706"));
    }

    #[test]
    fn last_server_version_is_none_when_the_node_is_absent() {
        assert_eq!(last_server_version("<preferences></preferences>"), None);
    }

    #[test]
    fn resolve_replays_dir_falls_back_to_the_default_when_no_preferences_file_exists() {
        let dir = std::env::temp_dir().join("wtk-replay-data-install-test-no-prefs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(resolve_replays_dir(&dir), dir.join("replays"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_replays_dir_prefers_the_versioned_subdir_when_it_exists() {
        let dir = std::env::temp_dir().join("wtk-replay-data-install-test-versioned");
        let _ = std::fs::remove_dir_all(&dir);
        let versioned = dir.join("replays").join("13.11.0.0");
        std::fs::create_dir_all(&versioned).unwrap();
        std::fs::write(dir.join("preferences.xml"), "<last_server_version>13, 11, 0, 12668706</last_server_version>")
            .unwrap();

        assert_eq!(resolve_replays_dir(&dir), versioned);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
