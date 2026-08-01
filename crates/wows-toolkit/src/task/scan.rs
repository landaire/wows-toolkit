use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::ops::ControlFlow;
use std::path::Path;
use std::path::PathBuf;

use tracing::warn;
use wowsunpack::data::Version;

use crate::task::BuildRequest;
use crate::task::replays::IngestProgress;

/// The replays in a directory that share one build, and the request that build
/// is downloaded and resolved under.
pub struct ScanBuild {
    pub request: BuildRequest,
    pub paths: Vec<PathBuf>,
}

/// Why a file the walk found will never be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnreadableReason {
    /// The header would not parse at all.
    Header,
    /// The header parsed but its client version carries no build, so no game
    /// data can be resolved and none can be offered for download.
    NoBuild,
    /// Reading the header panicked. One malformed file must cost only itself.
    Panicked,
}

/// What one walk of a directory found, before any replay has been read in full.
pub struct DirectoryScan {
    pub root: PathBuf,
    /// Keyed on the build, which is what identifies a set of replays that can
    /// share one load of game data.
    pub by_build: BTreeMap<NonZeroU32, ScanBuild>,
    /// Files that will never be read, with what stopped each one.
    pub unreadable: Vec<(PathBuf, UnreadableReason)>,
    /// The subset of `by_build` this machine has no data for.
    pub missing_builds: BTreeSet<NonZeroU32>,
    /// Every `.wowsreplay` the walk found, readable or not.
    pub total: usize,
}

impl DirectoryScan {
    /// The paths to read, build by build, in the order the read stage visits
    /// them. Grouping is what keeps one build's data resident across all of its
    /// replays instead of being evicted and reloaded.
    ///
    /// Ascending build order is load-bearing, not incidental: resolving a build
    /// bridges constants forward from an already-loaded older build and must
    /// never apply a newer build's constants to an older replay.
    pub fn read_order(&self) -> impl Iterator<Item = (&BuildRequest, &[PathBuf])> {
        self.by_build.values().map(|group| (&group.request, group.paths.as_slice()))
    }

    /// How many files the read stage will actually visit.
    ///
    /// Not `total`: the files in `unreadable` were resolved by the scan and are
    /// never opened again, so a read-stage bar over `total` stops short of full.
    pub fn to_read(&self) -> usize {
        self.by_build.values().map(|group| group.paths.len()).sum()
    }
}

/// Group `paths` by the build their replays report, and record which of those
/// builds have no data on this machine.
///
/// `read_version` and `has_data` are parameters so the grouping, the counting
/// and the missing-build decision are exercisable without a filesystem or a
/// game install.
///
/// `on_progress` returns [`ControlFlow::Break`] to abandon the scan: a
/// workspace closed mid-scan has nothing left to fill, and a large directory
/// would otherwise keep reading headers for a listing nobody is watching.
pub fn scan_paths(
    root: PathBuf,
    paths: Vec<PathBuf>,
    read_version: impl Fn(&Path) -> Option<Version>,
    has_data: impl Fn(&BuildRequest) -> bool,
    mut on_progress: impl FnMut(IngestProgress) -> ControlFlow<()>,
) -> DirectoryScan {
    let total = paths.len();
    let mut by_build: BTreeMap<NonZeroU32, ScanBuild> = BTreeMap::new();
    let mut unreadable = Vec::new();

    for (visited, path) in paths.into_iter().enumerate() {
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_version(&path)));

        match read {
            Ok(Some(version)) => match BuildRequest::new(version) {
                Some(request) => {
                    by_build
                        .entry(request.build())
                        .or_insert_with(|| ScanBuild { request, paths: Vec::new() })
                        .paths
                        .push(path);
                }
                None => unreadable.push((path, UnreadableReason::NoBuild)),
            },
            Ok(None) => unreadable.push((path, UnreadableReason::Header)),
            Err(payload) => {
                let message = crate::util::thread::panic_payload_to_string(&payload);
                warn!("panic reading the header of {}, skipping it: {message}", path.display());
                unreadable.push((path, UnreadableReason::Panicked));
            }
        }

        if on_progress(IngestProgress { done: visited + 1, total }).is_break() {
            break;
        }
    }

    let missing_builds =
        by_build.iter().filter(|(_, group)| !has_data(&group.request)).map(|(build, _)| *build).collect();

    DirectoryScan { root, by_build, unreadable, missing_builds, total }
}

/// Walk `root` and read each replay's header, without reading any packet
/// stream. The result is retained by the caller so the read stage does not walk
/// the directory a second time.
pub fn start_scan_directory(
    deps: crate::data::wows_data::ReplayDependencies,
    workspace: crate::db::index::rows::WorkspaceId,
    root: PathBuf,
) -> crate::task::BackgroundTask {
    let (tx, rx) = std::sync::mpsc::channel();
    let (update_tx, update_rx) = std::sync::mpsc::channel();

    crate::util::thread::spawn_logged("scan-directory", move || {
        let paths = crate::task::replays::walk_replay_files(&root);
        let found: std::collections::HashSet<PathBuf> = paths.iter().cloned().collect();
        let _ = update_tx.send(crate::task::replays::IngestUpdate::Walked { workspace, paths: found });

        let scan = scan_paths(
            root,
            paths,
            |path| {
                // Only the plaintext header is read: no decrypt, no inflate, no
                // packet stream. A header that will not parse costs this file.
                wows_replays::ReplayFile::meta_from_file(path)
                    .ok()
                    .and_then(|meta| Version::try_from_client_exe(&meta.clientVersionFromExe))
            },
            |request| deps.wows_data_map.has_data_for(request),
            |progress| {
                let sent = update_tx.send(crate::task::replays::IngestUpdate::Stage {
                    workspace,
                    stage: crate::task::replays::IngestStage::Scanning(progress),
                });
                // Nothing is listening any more: the run was dropped or the app
                // is shutting down, and the rest of a large directory's headers
                // would be read for a listing nobody is watching.
                if sent.is_ok() { ControlFlow::Continue(()) } else { ControlFlow::Break(()) }
            },
        );

        let _ =
            tx.send(Ok(crate::task::BackgroundTaskCompletion::DirectoryScanned { workspace, scan: Box::new(scan) }));
    });

    crate::task::BackgroundTask {
        receiver: Some(rx),
        kind: crate::task::BackgroundTaskKind::ScanningDirectory { workspace, rx: update_rx },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(major: u32, minor: u32, patch: u32, build: u32) -> Version {
        Version { major, minor, patch, build: std::num::NonZeroU32::new(build) }
    }

    fn path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    /// Replays of one build group under it whatever order the walk found them
    /// in, which is what lets the read stage load that build's data once.
    #[test]
    fn replays_group_under_their_build() {
        let files = [("a", version(15, 0, 0, 100)), ("b", version(14, 11, 0, 90)), ("c", version(15, 0, 0, 100))];
        let scan = scan_paths(
            path("root"),
            files.iter().map(|(name, _)| path(name)).collect(),
            |p| files.iter().find(|(name, _)| Path::new(name) == p).map(|(_, v)| *v),
            |_| true,
            |_| ControlFlow::Continue(()),
        );

        assert_eq!(scan.total, 3);
        assert_eq!(scan.by_build.len(), 2);
        let build_100 = std::num::NonZeroU32::new(100).expect("nonzero");
        assert_eq!(scan.by_build[&build_100].paths, vec![path("a"), path("c")]);
    }

    /// A 0.6.x replay reports build 0, which is no build at all. It must not
    /// become a group of its own or be requested for download.
    #[test]
    fn a_replay_with_no_build_is_unreadable_not_a_group() {
        let scan = scan_paths(
            path("root"),
            vec![path("old")],
            |_| Some(version(0, 6, 13, 0)),
            |_| true,
            |_| ControlFlow::Continue(()),
        );

        assert!(scan.by_build.is_empty(), "no build key may be invented for it");
        assert_eq!(scan.unreadable, vec![(path("old"), UnreadableReason::NoBuild)]);
        assert_eq!(scan.total, 1, "it is still one of the files the directory holds");
    }

    /// A header that will not parse costs that file and nothing else.
    #[test]
    fn an_unparseable_header_does_not_sink_the_scan() {
        let scan = scan_paths(
            path("root"),
            vec![path("bad"), path("good")],
            |p| (p == Path::new("good")).then(|| version(15, 0, 0, 100)),
            |_| true,
            |_| ControlFlow::Continue(()),
        );

        assert_eq!(scan.unreadable, vec![(path("bad"), UnreadableReason::Header)]);
        assert_eq!(scan.by_build.len(), 1);
    }

    /// The missing set is what the prompt is built from, so it names only the
    /// builds that are actually absent.
    #[test]
    fn only_absent_builds_are_reported_missing() {
        let files = [("a", version(15, 0, 0, 100)), ("b", version(14, 11, 0, 90))];
        let scan = scan_paths(
            path("root"),
            files.iter().map(|(name, _)| path(name)).collect(),
            |p| files.iter().find(|(name, _)| Path::new(name) == p).map(|(_, v)| *v),
            |request| request.build_u32() == 100,
            |_| ControlFlow::Continue(()),
        );

        assert_eq!(scan.missing_builds, BTreeSet::from([std::num::NonZeroU32::new(90).expect("nonzero")]));
    }

    /// Progress is reported per file, including the ones that do not load, so
    /// a run of unreadable files does not look like a stall.
    #[test]
    fn every_file_advances_progress() {
        let mut seen = Vec::new();
        scan_paths(
            path("root"),
            vec![path("bad"), path("good")],
            |p| (p == Path::new("good")).then(|| version(15, 0, 0, 100)),
            |_| true,
            |progress| {
                seen.push(progress.done);
                ControlFlow::Continue(())
            },
        );

        assert_eq!(seen, vec![1, 2]);
    }

    /// The scan is what a directory open now starts with, and the read stage
    /// consumes its grouping. Reading the groups back in order is what keeps one
    /// build's data resident across all of its replays.
    #[test]
    fn read_order_visits_one_build_at_a_time() {
        let files = [("a", version(15, 0, 0, 100)), ("b", version(14, 11, 0, 90)), ("c", version(15, 0, 0, 100))];
        let scan = scan_paths(
            path("root"),
            files.iter().map(|(name, _)| path(name)).collect(),
            |p| files.iter().find(|(name, _)| Path::new(name) == p).map(|(_, v)| *v),
            |_| true,
            |_| ControlFlow::Continue(()),
        );

        let visited: Vec<u32> = scan.read_order().map(|(request, _)| request.build_u32()).collect();
        assert_eq!(visited, vec![90, 100], "each build appears exactly once, in build order");
    }

    /// The prompt names how many replays each absent build is holding back.
    #[test]
    fn a_missing_build_reports_the_replays_waiting_on_it() {
        let files = [("a", version(15, 0, 0, 100)), ("b", version(15, 0, 0, 100))];
        let scan = scan_paths(
            path("root"),
            files.iter().map(|(name, _)| path(name)).collect(),
            |p| files.iter().find(|(name, _)| Path::new(name) == p).map(|(_, v)| *v),
            |_| false,
            |_| ControlFlow::Continue(()),
        );

        let build = std::num::NonZeroU32::new(100).expect("nonzero");
        assert_eq!(scan.by_build[&build].paths.len(), 2);
        assert!(scan.missing_builds.contains(&build));
    }
}
