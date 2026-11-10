//! The replay listing's header-only scan: which files in a directory are
//! finished replays, what each one's plaintext header says, and which of
//! the builds they were recorded on this machine can read.
//!
//! Only the metadata block is read (`ReplayFile::meta_from_file`): no packet
//! decryption or decompression, so a directory of thousands of replays is
//! walked in seconds.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use wows_replays::ReplayFile;
use wows_toolkit_viewmodel::listing_row::ListedReplay;
use wowsunpack::data::Version;

use crate::error::ReplayLoadError;
use crate::load::GameDataCache;

/// One replay's header, before ship and map names are translated.
#[derive(Debug, Clone)]
pub struct ScannedReplay {
    pub path: PathBuf,
    pub listed: ListedReplay,
    /// The `major.minor.patch` the replay names. Build numbers are per
    /// server, so a download falls back on the version to find the nearest
    /// published build.
    pub version: Option<String>,
}

/// How far a directory scan has got. A `total` of zero means the walk
/// itself is still running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub done: usize,
    pub total: usize,
}

/// A build the listing holds, how many of its replays were recorded on it,
/// and the version they name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedBuild {
    pub build: u32,
    pub version: Option<String>,
    pub replays: usize,
}

/// A build the listing needs and nothing on this machine has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingBuild {
    pub build: u32,
    /// The version its replays name, for the cross-server fallback when the
    /// repository has no exact build.
    pub version: Option<String>,
    /// How many listed replays were recorded on it, so the reader can judge
    /// whether the download is worth it.
    pub replays: usize,
}

/// Whether `path` names a replay the listing shows. `temp.wowsreplay` is the
/// match in progress: it has no metadata until the battle ends, and the game
/// renames it into place then.
pub fn is_finished_replay(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("wowsreplay")
        && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
}

/// Reads the header of the replay at `path`.
pub fn read_scanned(path: &Path) -> Result<ScannedReplay, ReplayLoadError> {
    let meta = ReplayFile::meta_from_file(path).map_err(|err| ReplayLoadError::Parse(err.to_string()))?;

    Ok(ScannedReplay {
        path: path.to_path_buf(),
        listed: ListedReplay::from_meta(&meta),
        version: Version::try_from_client_exe(&meta.clientVersionFromExe)
            .map(|version| format!("{}.{}.{}", version.major, version.minor, version.patch)),
    })
}

/// Lists every finished replay directly inside `dir` and reads each one's
/// header, reporting progress through `report`: the total once the walk is
/// done, then every few files and at the last one. A file that does not read
/// (corrupt, mid-write, a format this parser does not understand) is logged
/// and skipped rather than ending the scan.
pub fn scan_replay_files(dir: &Path, mut report: impl FnMut(ScanProgress)) -> Vec<ScannedReplay> {
    // Reporting every file would cost more than the read itself on a warm
    // directory.
    const REPORT_EVERY: usize = 16;

    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if is_finished_replay(&path) {
            candidates.push(path);
        }
    }

    let total = candidates.len();
    report(ScanProgress { done: 0, total });

    let mut scanned = Vec::with_capacity(total);
    for (read, path) in candidates.into_iter().enumerate() {
        match read_scanned(&path) {
            Ok(replay) => scanned.push(replay),
            Err(err) => tracing::warn!(path = %path.display(), "failed to read replay meta: {err}"),
        }

        let done = read + 1;
        if done % REPORT_EVERY == 0 || done == total {
            report(ScanProgress { done, total });
        }
    }

    scanned
}

/// Every build the listing holds, with how many replays each accounts for
/// and the first version its replays name. A replay whose header names no
/// build is not counted: nothing could be fetched for it.
pub fn listed_builds(files: &[ScannedReplay]) -> Vec<ListedBuild> {
    let mut counted: BTreeMap<u32, ListedBuild> = BTreeMap::new();
    for file in files {
        let Some(build) = file.listed.build else { continue };
        let entry = counted.entry(build).or_insert_with(|| ListedBuild { build, version: None, replays: 0 });
        entry.replays += 1;
        if entry.version.is_none() {
            entry.version = file.version.clone();
        }
    }

    counted.into_values().collect()
}

/// The listed builds nothing on this machine can read.
pub fn missing_builds(files: &[ScannedReplay], cache: &GameDataCache) -> Vec<MissingBuild> {
    listed_builds(files)
        .into_iter()
        .filter(|listed| !cache.can_read_build(listed.build, listed.version.as_deref()))
        .map(|listed| MissingBuild { build: listed.build, version: listed.version, replays: listed.replays })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(build: Option<u32>, version: &str) -> ScannedReplay {
        ScannedReplay {
            path: PathBuf::from(format!("{}-{version}.wowsreplay", build.unwrap_or_default())),
            version: (!version.is_empty()).then(|| version.to_owned()),
            listed: ListedReplay {
                ship_id: None,
                map_name: String::new(),
                game_type: String::new(),
                scenario: String::new(),
                date_time: String::new(),
                build,
            },
        }
    }

    #[test]
    fn the_watcher_ignores_everything_but_a_finished_replay() {
        assert!(is_finished_replay(Path::new("replays/20260818_132652_PWSD610-Smaland.wowsreplay")));
        // The match in progress, which has no metadata until it ends.
        assert!(!is_finished_replay(Path::new("replays/temp.wowsreplay")));
        assert!(!is_finished_replay(Path::new("replays/tempArenaInfo.json")));
        assert!(!is_finished_replay(Path::new("replays/20260818_132652_PWSD610-Smaland")));
    }

    #[test]
    fn listed_builds_are_counted_per_build_and_skip_headers_without_one() {
        let files = vec![listed(Some(200), "0.11.0"), listed(Some(100), "0.10.5"), listed(Some(200), ""), listed(None, "")];

        assert_eq!(
            listed_builds(&files),
            vec![
                ListedBuild { build: 100, version: Some("0.10.5".to_owned()), replays: 1 },
                ListedBuild { build: 200, version: Some("0.11.0".to_owned()), replays: 2 },
            ]
        );
    }

    /// A build nothing on this machine can read is reported once, with how
    /// many of the listed replays are waiting on it. A build the cache holds
    /// is not reported at all.
    #[test]
    fn the_listing_reports_only_the_builds_nothing_can_read() {
        wows_toolkit_config::storage_override::set(std::env::temp_dir().join("wt-replay-data-tests"));
        let cache_dir = tempfile::tempdir().expect("a temp directory");
        std::fs::create_dir_all(cache_dir.path().join("0.10.5_100")).expect("the dump directory is created");
        std::fs::write(cache_dir.path().join("0.10.5_100").join("metadata.toml"), "").expect("the dump is marked");

        let files = vec![listed(Some(100), "0.10.5"), listed(Some(200), "0.11.0"), listed(Some(200), "0.11.0"), listed(None, "")];

        // A directory that is not an install, so only the cache can answer.
        let cache =
            GameDataCache::new(PathBuf::from("G:/does-not-exist")).with_cache_dir(&cache_dir.path().to_string_lossy());

        let missing = missing_builds(&files, &cache);
        assert_eq!(missing, vec![MissingBuild { build: 200, version: Some("0.11.0".to_owned()), replays: 2 }]);
    }

    #[test]
    fn a_directory_that_does_not_exist_scans_to_nothing() {
        let mut reports = Vec::new();

        let scanned = scan_replay_files(Path::new("G:/does-not-exist"), |progress| reports.push(progress));

        assert!(scanned.is_empty());
        assert!(reports.is_empty(), "nothing was walked, so there is nothing to report");
    }

    #[test]
    fn an_unreadable_replay_is_skipped_and_progress_still_reaches_the_total() {
        let dir = tempfile::tempdir().expect("a temp directory");
        std::fs::write(dir.path().join("a.wowsreplay"), b"not a replay").expect("the file is written");
        std::fs::write(dir.path().join("temp.wowsreplay"), b"in progress").expect("the file is written");
        std::fs::write(dir.path().join("notes.txt"), b"not a replay either").expect("the file is written");
        let mut reports = Vec::new();

        let scanned = scan_replay_files(dir.path(), |progress| reports.push(progress));

        assert!(scanned.is_empty());
        assert_eq!(reports, vec![ScanProgress { done: 0, total: 1 }, ScanProgress { done: 1, total: 1 }]);
    }
}
