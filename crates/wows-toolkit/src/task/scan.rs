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

/// What one worker read out of one file's header.
enum HeaderRead {
    Version(Option<Version>),
    Panicked { message: String },
}

/// Group `paths` by the build their replays report, and record which of those
/// builds have no data on this machine.
///
/// `read_version` and `has_data` are parameters so the grouping, the counting
/// and the missing-build decision are exercisable without a filesystem or a
/// game install.
///
/// Headers are read by `threads` workers pulling from a shared queue. Results
/// land in one slot per path and are folded in path order after the workers
/// finish, so the grouping is identical whatever order the reads complete in.
///
/// `on_progress` runs on the calling thread, once per completed file. It
/// returns [`ControlFlow::Break`] to abandon the scan: a workspace closed
/// mid-scan has nothing left to fill, and a large directory would otherwise
/// keep reading headers for a listing nobody is watching. Files read while
/// the abandonment propagates to the workers still count.
pub fn scan_paths(
    root: PathBuf,
    paths: Vec<PathBuf>,
    read_version: impl Fn(&Path) -> Option<Version> + Sync,
    has_data: impl Fn(&BuildRequest) -> bool,
    mut on_progress: impl FnMut(IngestProgress) -> ControlFlow<()>,
    threads: std::num::NonZeroUsize,
) -> DirectoryScan {
    let total = paths.len();

    let slots: Vec<std::sync::OnceLock<HeaderRead>> = (0..total).map(|_| std::sync::OnceLock::new()).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let abort = std::sync::atomic::AtomicBool::new(false);

    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        for _ in 0..threads.get().min(total) {
            let tx = tx.clone();
            let (slots, paths, next, abort, read_version) = (&slots, &paths, &next, &abort, &read_version);
            scope.spawn(move || {
                loop {
                    if abort.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { break };

                    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_version(path)));
                    let read = match read {
                        Ok(version) => HeaderRead::Version(version),
                        Err(payload) => {
                            HeaderRead::Panicked { message: crate::util::thread::panic_payload_to_string(&payload) }
                        }
                    };
                    // Cannot already be set: each index is handed to one worker.
                    let _ = slots[index].set(read);

                    // The coordinator gone means the scan was abandoned;
                    // finishing the queue would be reads nobody folds.
                    if tx.send(()).is_err() {
                        break;
                    }
                }
            });
        }
        // The coordinator holding a sender would keep its own recv loop alive
        // after every worker exits.
        drop(tx);

        let mut done = 0;
        while rx.recv().is_ok() {
            done += 1;
            if on_progress(IngestProgress { done, total }).is_break() {
                abort.store(true, std::sync::atomic::Ordering::Relaxed);
                break;
            }
        }
    });

    let mut by_build: BTreeMap<NonZeroU32, ScanBuild> = BTreeMap::new();
    let mut unreadable = Vec::new();

    for (path, slot) in paths.into_iter().zip(slots) {
        // An empty slot is a file the abandoned scan never got to.
        let Some(read) = slot.into_inner() else { continue };

        match read {
            HeaderRead::Version(Some(version)) => match BuildRequest::new(version) {
                Some(request) => {
                    by_build
                        .entry(request.build())
                        .or_insert_with(|| ScanBuild { request, paths: Vec::new() })
                        .paths
                        .push(path);
                }
                None => unreadable.push((path, UnreadableReason::NoBuild)),
            },
            HeaderRead::Version(None) => unreadable.push((path, UnreadableReason::Header)),
            HeaderRead::Panicked { message } => {
                warn!("panic reading the header of {}, skipping it: {message}", path.display());
                unreadable.push((path, UnreadableReason::Panicked));
            }
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
    let (tx, rx) = crate::task::completion_channel();
    // Throttled: the scan reports per header read, and headers read fast
    // enough to wake the UI thousands of times a second.
    let (update_tx, update_rx) =
        crate::ui_channel::throttled_channel(deps.egui_ctx.clone(), std::time::Duration::from_millis(250));

    crate::util::thread::spawn_logged("scan-directory", move || {
        let paths = crate::task::replays::walk_replay_files(&root);
        let found: std::collections::HashSet<PathBuf> = paths.iter().cloned().collect();
        let _ = update_tx.send(crate::task::replays::IngestUpdate::Walked { workspace, paths: found });

        // Header reads are independent smallish file reads, so one worker per
        // core keeps the disk queue full. A failed parallelism query falls
        // back to a single worker, which is correct just slower.
        let threads = std::thread::available_parallelism().unwrap_or(std::num::NonZeroUsize::MIN);

        let scan = scan_paths(
            root,
            paths,
            |path| {
                // Only the plaintext header is read: no decrypt, no inflate, no
                // packet stream. A header that will not parse costs this file.
                // The borrowed parse keeps the bulk of the metadata strings
                // unowned when all the scan wants is the client version.
                let blob = wows_replays::ReplayFile::read_meta_blob(path).ok()?;
                let meta = wows_replays::ReplayMetaRef::from_slice(&blob).ok()?;
                Version::try_from_client_exe(&meta.clientVersionFromExe)
            },
            |request| deps.build_cache.has_data_for(request),
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
            threads,
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

    const ONE_THREAD: std::num::NonZeroUsize = std::num::NonZeroUsize::MIN;

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
            ONE_THREAD,
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
            ONE_THREAD,
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
            ONE_THREAD,
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
            ONE_THREAD,
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
            ONE_THREAD,
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
            ONE_THREAD,
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
            ONE_THREAD,
        );

        let build = std::num::NonZeroU32::new(100).expect("nonzero");
        assert_eq!(scan.by_build[&build].paths.len(), 2);
        assert!(scan.missing_builds.contains(&build));
    }

    /// Reads race across workers, but the fold runs in path order, so the scan
    /// must come out identical to a single-threaded one however the reads
    /// interleave. Progress still counts every file exactly once.
    #[test]
    fn a_parallel_scan_matches_a_single_threaded_one() {
        let files: Vec<(String, Option<Version>)> = (0..64)
            .map(|i| {
                let version = match i % 4 {
                    0 => Some(version(15, 0, 0, 100)),
                    1 => Some(version(14, 11, 0, 90)),
                    2 => Some(version(0, 6, 13, 0)),
                    _ => None,
                };
                (format!("file-{i:02}"), version)
            })
            .collect();

        let run = |threads: std::num::NonZeroUsize| {
            let mut reports = 0usize;
            let scan = scan_paths(
                path("root"),
                files.iter().map(|(name, _)| path(name)).collect(),
                |p| files.iter().find(|(name, _)| Path::new(name) == p).and_then(|(_, v)| *v),
                |request| request.build_u32() == 100,
                |progress| {
                    reports += 1;
                    assert_eq!(progress.done, reports, "done counts each completion exactly once");
                    ControlFlow::Continue(())
                },
                threads,
            );
            assert_eq!(reports, files.len());
            scan
        };

        let sequential = run(ONE_THREAD);
        let parallel = run(std::num::NonZeroUsize::new(8).expect("nonzero"));

        assert_eq!(parallel.total, sequential.total);
        assert_eq!(parallel.unreadable, sequential.unreadable);
        assert_eq!(parallel.missing_builds, sequential.missing_builds);
        let groups = |scan: &DirectoryScan| {
            scan.by_build.iter().map(|(build, group)| (*build, group.paths.clone())).collect::<Vec<_>>()
        };
        assert_eq!(groups(&parallel), groups(&sequential));
    }

    /// Break abandons the queue: the workers stop pulling files well short of
    /// the whole directory, everything reported before the break is kept, and
    /// the scan still returns.
    #[test]
    fn an_abandoned_parallel_scan_returns_without_reading_everything() {
        let names: Vec<String> = (0..64).map(|i| format!("file-{i:02}")).collect();
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let scan = scan_paths(
            path("root"),
            names.iter().map(|name| path(name)).collect(),
            |_| {
                reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Slow enough that the break lands while most of the queue is
                // still unread, whatever the scheduler does: reading all 64
                // takes at least 16ms across 8 workers, and the coordinator
                // breaks microseconds after the fourth completion.
                std::thread::sleep(std::time::Duration::from_millis(2));
                Some(version(15, 0, 0, 100))
            },
            |_| true,
            |progress| if progress.done >= 4 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) },
            std::num::NonZeroUsize::new(8).expect("nonzero"),
        );

        assert!(reads.load(std::sync::atomic::Ordering::Relaxed) < 64, "the tail of the queue stays unread");
        let build = std::num::NonZeroU32::new(100).expect("nonzero");
        let classified = scan.by_build[&build].paths.len() + scan.unreadable.len();
        assert!(classified >= 4, "everything reported before the break is kept");
        assert_eq!(scan.total, 64, "total still counts the files the walk found");
    }

    /// A read_version panic costs only its file, across workers: it lands in
    /// unreadable as Panicked, every other file classifies normally, and
    /// progress still counts the whole directory.
    #[test]
    fn a_panicking_header_read_costs_only_its_file() {
        let names: Vec<String> = (0..16).map(|i| format!("file-{i:02}")).collect();
        let mut reports = 0usize;
        let scan = scan_paths(
            path("root"),
            names.iter().map(|name| path(name)).collect(),
            |p| {
                if p == Path::new("file-07") {
                    panic!("header read gone wrong");
                }
                Some(version(15, 0, 0, 100))
            },
            |_| true,
            |progress| {
                reports = progress.done;
                ControlFlow::Continue(())
            },
            std::num::NonZeroUsize::new(8).expect("nonzero"),
        );

        assert_eq!(reports, 16, "the panicking file still advances progress");
        assert_eq!(scan.unreadable, vec![(path("file-07"), UnreadableReason::Panicked)]);
        let build = std::num::NonZeroU32::new(100).expect("nonzero");
        assert_eq!(scan.by_build[&build].paths.len(), 15);
    }
}
