//! Reading every replay in a directory into the games a Stats view over that
//! directory shows.
//!
//! The games are folded through `wows_toolkit_viewmodel::stats::replay_set`;
//! each front end runs [`read_directory`] on a thread of its own and applies
//! the updates it reports.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetProgress;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;
use wowsunpack::data::Version;

use crate::error::ReplayLoadError;
use crate::load::GameDataCache;

/// What reading a directory reports as it goes.
#[derive(Debug)]
pub enum DirectoryStatsUpdate {
    /// One more header read while finding and grouping the replays.
    Scanning(ReplaySetProgress),
    /// One more replay visited by the read itself.
    Read {
        game: Option<Box<PerGameStat>>,
        progress: ReplaySetProgress,
        /// Every replay skipped so far, not only this one.
        skips: ReplaySetSkips,
    },
}

/// What reading one replay produced.
enum GameRead {
    Game(PerGameStat),
    NoResults,
    MissingBuild,
    Unreadable,
}

/// The replays of one build, and the version their headers name.
struct BuildGroup {
    version: String,
    paths: Vec<PathBuf>,
}

/// Reads every replay at or below `root` into a game, reporting each result
/// to `report`. Stops once `report` breaks.
///
/// Replays are read one build at a time, in ascending build order, so a
/// directory spanning many builds loads their game data one after another
/// rather than all at once. Nothing is downloaded: a build this machine has
/// no game data for is counted as skipped.
pub fn read_directory(
    root: &Path,
    game_data: &GameDataCache,
    report: &mut dyn FnMut(DirectoryStatsUpdate) -> ControlFlow<()>,
) {
    let paths = replays_beneath(root);
    let total = paths.len();
    let workers = worker_count();

    let mut versions: Vec<Option<Version>> = vec![None; total];
    let mut scanned = 0;
    let scan = for_each_path(&paths, workers, read_header_version, |index, version| {
        scanned += 1;
        // A header read that panicked leaves the slot empty, which counts the
        // file unreadable.
        versions[index] = version.flatten();
        report(DirectoryStatsUpdate::Scanning(ReplaySetProgress { done: scanned, total }))
    });
    if scan.is_break() {
        return;
    }

    let mut groups: BTreeMap<u32, BuildGroup> = BTreeMap::new();
    let mut skips = ReplaySetSkips::default();
    for (path, version) in paths.into_iter().zip(versions) {
        let Some((build, version)) = version.and_then(|version| Some((version.build_number()?, version))) else {
            skips.unreadable += 1;
            continue;
        };
        groups
            .entry(build)
            .or_insert_with(|| BuildGroup {
                version: format!("{}.{}.{}", version.major, version.minor, version.patch),
                paths: Vec::new(),
            })
            .paths
            .push(path);
    }

    let mut progress = ReplaySetProgress { done: skips.unreadable, total };
    if report(DirectoryStatsUpdate::Read { game: None, progress, skips }).is_break() {
        return;
    }

    for (build, group) in groups {
        if !game_data.can_read_build(build, Some(&group.version)) {
            skips.missing_build += group.paths.len();
            progress.done += group.paths.len();
            if report(DirectoryStatsUpdate::Read { game: None, progress, skips }).is_break() {
                return;
            }
            continue;
        }

        let flow = for_each_path(
            &group.paths,
            workers,
            |path| read_game(path, game_data),
            |_index, read| {
                progress.done += 1;
                let game = match read {
                    Some(GameRead::Game(game)) => Some(Box::new(game)),
                    Some(GameRead::NoResults) => {
                        skips.no_results += 1;
                        None
                    }
                    Some(GameRead::MissingBuild) => {
                        skips.missing_build += 1;
                        None
                    }
                    Some(GameRead::Unreadable) | None => {
                        skips.unreadable += 1;
                        None
                    }
                };
                report(DirectoryStatsUpdate::Read { game, progress, skips })
            },
        );
        if flow.is_break() {
            return;
        }
    }
}

/// Every replay at or below `root`, matching the egui app's directory stats:
/// symlinks are not followed, and the live recording is left out because it
/// has no results yet.
pub fn replays_beneath(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "wowsreplay")
                && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
        })
        .collect();
    paths.sort();
    paths
}

/// Half the cores, at most four: a full parse holds a battle's packet stream
/// and report.
fn worker_count() -> NonZeroUsize {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    NonZeroUsize::new((cores / 2).clamp(1, 4)).unwrap_or(NonZeroUsize::MIN)
}

/// Runs `work` over `paths` on `workers` threads, handing each result to
/// `on_result` on the calling thread as it lands, with the path's index. A
/// result is `None` when `work` panicked on that path. `on_result` breaks to
/// abandon the rest.
fn for_each_path<T: Send>(
    paths: &[PathBuf],
    workers: NonZeroUsize,
    work: impl Fn(&Path) -> T + Sync,
    mut on_result: impl FnMut(usize, Option<T>) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let next = AtomicUsize::new(0);
    let abort = AtomicBool::new(false);

    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<(usize, Option<T>)>();

        for _ in 0..workers.get().min(paths.len()) {
            let tx = tx.clone();
            let (next, abort, work) = (&next, &abort, &work);
            scope.spawn(move || {
                while !abort.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { break };

                    // One malformed replay must cost only itself.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(path)));
                    if result.is_err() {
                        tracing::warn!("directory stats: reading {} panicked", path.display());
                    }
                    if tx.send((index, result.ok())).is_err() {
                        break;
                    }
                }
            });
        }
        // The coordinator holding a sender would keep its own recv loop alive
        // after every worker exits.
        drop(tx);

        while let Ok((index, result)) = rx.recv() {
            if on_result(index, result).is_break() {
                abort.store(true, Ordering::Relaxed);
                return ControlFlow::Break(());
            }
        }

        ControlFlow::Continue(())
    })
}

/// The client version a replay's header names. Only the plaintext header is
/// read.
fn read_header_version(path: &Path) -> Option<Version> {
    let blob = wows_replays::ReplayFile::read_meta_blob(path).ok()?;
    let meta = wows_replays::ReplayMetaRef::from_slice(&blob).ok()?;
    Version::try_from_client_exe(&meta.clientVersionFromExe)
}

/// Parses one replay and reads the recording player's game off it.
fn read_game(path: &Path, game_data: &GameDataCache) -> GameRead {
    let parsed = match crate::load::parse_replay(path, game_data, None) {
        Ok(parsed) => parsed,
        Err(ReplayLoadError::UnsupportedVersion { .. }) => return GameRead::MissingBuild,
        Err(err) => {
            tracing::warn!("directory stats: skipping {}: {err}", path.display());
            return GameRead::Unreadable;
        }
    };
    // A battle whose results never arrived is one the session does not record
    // either.
    if parsed.indexable.results_pending {
        return GameRead::NoResults;
    }

    match parsed.session_stat {
        Some(game) => GameRead::Game(game),
        None => GameRead::NoResults,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game-data cache that reads and writes only a temporary storage
    /// directory, never the user's.
    fn test_cache() -> GameDataCache {
        wows_toolkit_config::storage_override::set(
            std::env::temp_dir().join(format!("wows-toolkit-replay-data-tests-{}", std::process::id())),
        );
        GameDataCache::without_install()
    }

    fn read(root: &Path) -> Vec<DirectoryStatsUpdate> {
        let mut updates = Vec::new();
        read_directory(root, &test_cache(), &mut |update| {
            updates.push(update);
            ControlFlow::Continue(())
        });
        updates
    }

    /// A file that is not a replay costs only itself and is counted as
    /// unreadable; the live recording is not part of the set at all.
    #[test]
    fn a_broken_replay_is_counted_unreadable_and_the_live_recording_is_left_out() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        std::fs::write(dir.path().join("broken.wowsreplay"), b"not a replay").expect("the file can be written");
        std::fs::write(dir.path().join("temp.wowsreplay"), b"in progress").expect("the file can be written");
        std::fs::write(dir.path().join("notes.txt"), b"unrelated").expect("the file can be written");

        let updates = read(dir.path());

        let Some(DirectoryStatsUpdate::Read { game, progress, skips }) = updates.last() else {
            panic!("the read ends on a read update");
        };
        assert!(game.is_none());
        assert_eq!(*progress, ReplaySetProgress { done: 1, total: 1 }, "only the broken replay is in the set");
        assert_eq!(*skips, ReplaySetSkips { unreadable: 1, ..ReplaySetSkips::default() });
    }

    /// Replays in subdirectories are part of the set; a live recording there
    /// is still left out.
    #[test]
    fn replays_in_subdirectories_are_read() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        let nested = dir.path().join("2026").join("archive");
        std::fs::create_dir_all(&nested).expect("the directory can be made");
        std::fs::write(dir.path().join("top.wowsreplay"), b"not a replay").expect("the file can be written");
        std::fs::write(nested.join("nested.wowsreplay"), b"not a replay").expect("the file can be written");
        std::fs::write(nested.join("temp.wowsreplay"), b"in progress").expect("the file can be written");

        let updates = read(dir.path());

        let Some(DirectoryStatsUpdate::Read { progress, skips, .. }) = updates.last() else {
            panic!("the read ends on a read update");
        };
        assert_eq!(*progress, ReplaySetProgress { done: 2, total: 2 });
        assert_eq!(*skips, ReplaySetSkips { unreadable: 2, ..ReplaySetSkips::default() });
    }

    /// Breaking out of the report stops the read.
    #[test]
    fn a_broken_report_stops_the_read() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        for index in 0..4 {
            std::fs::write(dir.path().join(format!("{index}.wowsreplay")), b"not a replay").expect("written");
        }

        let mut calls = 0;
        read_directory(dir.path(), &test_cache(), &mut |_| {
            calls += 1;
            ControlFlow::Break(())
        });

        assert_eq!(calls, 1);
    }
}
