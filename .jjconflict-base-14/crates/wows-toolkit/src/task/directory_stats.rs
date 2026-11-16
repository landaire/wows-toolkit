use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use rootcause::Report;
use tracing::warn;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetProgress;
use wows_toolkit_viewmodel::stats::replay_set::ReplaySetSkips;

use crate::data::directory_stats::DirectoryStatsId;
use crate::data::wows_data::ReplayDependencies;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::util::error::ToolkitError;

/// What a running directory stats read sends the pane it is filling.
pub enum DirectoryStatsUpdate {
    /// One more header read while finding and grouping the replays.
    Scanning { pane: DirectoryStatsId, progress: ReplaySetProgress },
    /// One more replay visited by the read itself.
    Read {
        pane: DirectoryStatsId,
        /// The game the replay yielded, if it yielded one.
        game: Option<Box<PerGameStat>>,
        progress: ReplaySetProgress,
        /// Every replay skipped so far, not only this one.
        skips: ReplaySetSkips,
    },
}

impl DirectoryStatsUpdate {
    pub fn pane(&self) -> DirectoryStatsId {
        match self {
            Self::Scanning { pane, .. } | Self::Read { pane, .. } => *pane,
        }
    }
}

/// What reading one replay produced.
enum GameRead {
    Game(PerGameStat),
    NoResults,
    MissingBuild,
    Unreadable,
}

/// Read every replay under `root` into a game for the directory stats pane
/// `pane`, without touching the session stats or the replay index.
///
/// Replays are read one build at a time, in ascending build order, for the
/// same reason the directory ingest does: the data map holds only a couple of
/// non-main builds at once.
pub fn start_directory_stats(deps: ReplayDependencies, pane: DirectoryStatsId, root: PathBuf) -> BackgroundTask {
    let (tx, rx) = super::completion_channel();
    let (update_tx, update_rx) =
        crate::ui_channel::throttled_channel(deps.egui_ctx.clone(), std::time::Duration::from_millis(250));

    crate::util::thread::spawn_logged("directory-stats", move || {
        let _ = tx.send(run_directory_stats(&deps, pane, root, &update_tx));
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::ReadingDirectoryStats { pane, rx: update_rx } }
}

fn run_directory_stats(
    deps: &ReplayDependencies,
    pane: DirectoryStatsId,
    root: PathBuf,
    tx: &crate::ui_channel::ThrottledSender<DirectoryStatsUpdate>,
) -> Result<BackgroundTaskCompletion, Report> {
    let paths = crate::task::replays::walk_replay_files(&root);
    let threads = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let mut closed = false;
    let scan = crate::task::scan::scan_paths(
        root,
        paths,
        crate::task::scan::read_header_version,
        |request| deps.build_cache.has_data_for(request),
        |scanned| {
            let progress = ReplaySetProgress { done: scanned.done, total: scanned.total };
            // The pane closed or the app is shutting down; the rest of the
            // directory has no reader.
            closed = tx.send(DirectoryStatsUpdate::Scanning { pane, progress }).is_err();
            if closed { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
        },
        threads,
    );
    if closed {
        return Ok(BackgroundTaskCompletion::DirectoryStatsRead { pane });
    }

    let mut skips = ReplaySetSkips { unreadable: scan.unreadable.len(), ..ReplaySetSkips::default() };
    let mut progress = ReplaySetProgress { done: scan.unreadable.len(), total: scan.total };
    if tx.send(DirectoryStatsUpdate::Read { pane, game: None, progress, skips }).is_err() {
        return Ok(BackgroundTaskCompletion::DirectoryStatsRead { pane });
    }

    // A full parse holds a battle's packet stream and report, so the worker
    // count stays well under the core count.
    let workers = NonZeroUsize::new((threads.get() / 2).clamp(1, 4)).unwrap_or(NonZeroUsize::MIN);

    for (request, group) in scan.read_order() {
        // Nothing is downloaded here: the directory listing offers that, and
        // a stats pane only reports what it could not read.
        if !deps.build_cache.has_data_for(request) || deps.build_cache.resolve(&request.version()).is_none() {
            skips.missing_build += group.len();
            progress.done += group.len();
            if tx.send(DirectoryStatsUpdate::Read { pane, game: None, progress, skips }).is_err() {
                break;
            }
            continue;
        }

        let flow = read_group(deps, group, workers, |read| {
            progress.done += 1;
            let game = match read {
                GameRead::Game(game) => Some(Box::new(game)),
                GameRead::NoResults => {
                    skips.no_results += 1;
                    None
                }
                GameRead::MissingBuild => {
                    skips.missing_build += 1;
                    None
                }
                GameRead::Unreadable => {
                    skips.unreadable += 1;
                    None
                }
            };
            match tx.send(DirectoryStatsUpdate::Read { pane, game, progress, skips }) {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            }
        });
        if flow.is_break() {
            break;
        }
    }

    Ok(BackgroundTaskCompletion::DirectoryStatsRead { pane })
}

/// Read `paths` on `workers` threads, handing each result to `on_read` on the
/// calling thread as it lands. `on_read` breaks to abandon the group.
fn read_group(
    deps: &ReplayDependencies,
    paths: &[PathBuf],
    workers: NonZeroUsize,
    mut on_read: impl FnMut(GameRead) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let next = AtomicUsize::new(0);
    let abort = AtomicBool::new(false);

    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<GameRead>();

        for _ in 0..workers.get().min(paths.len()) {
            let tx = tx.clone();
            let deps = deps.clone();
            let (next, abort) = (&next, &abort);
            scope.spawn(move || {
                while !abort.load(Ordering::Relaxed) {
                    let Some(path) = paths.get(next.fetch_add(1, Ordering::Relaxed)) else { break };

                    // One malformed replay must cost only itself.
                    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_game(&deps, path)))
                        .unwrap_or_else(|payload| {
                            let msg = crate::util::thread::panic_payload_to_string(&payload);
                            warn!("panic while reading replay {} for directory stats: {msg}", path.display());
                            GameRead::Unreadable
                        });
                    if tx.send(read).is_err() {
                        break;
                    }
                }
            });
        }
        // The coordinator holding a sender would keep its own recv loop alive
        // after every worker exits.
        drop(tx);

        while let Ok(read) = rx.recv() {
            if on_read(read).is_break() {
                abort.store(true, Ordering::Relaxed);
                return ControlFlow::Break(());
            }
        }

        ControlFlow::Continue(())
    })
}

/// Parse one replay and read the recording player's game off it.
fn read_game(deps: &ReplayDependencies, path: &Path) -> GameRead {
    let (replay, wows_data) =
        match crate::data::wows_data::ReplayLoader::build_replay_from_existing_file(deps, path.to_path_buf()) {
            Ok(built) => built,
            Err(e) => {
                warn!("skipping replay {} for directory stats: {e}", path.display());
                return match e.downcast_current_context::<ToolkitError>() {
                    Some(ToolkitError::ReplayBuildUnavailable { .. }) => GameRead::MissingBuild,
                    _ => GameRead::Unreadable,
                };
            }
        };
    let expected_build = wows_data.read().patch_version.to_string();

    let mut replay = replay.write();
    match replay.parse(&expected_build) {
        Ok(parsed) => replay.store_parse(parsed),
        Err(e) => {
            warn!("skipping replay {} for directory stats: {e}", path.display());
            return GameRead::Unreadable;
        }
    }
    replay.build_ui_report(deps);

    match crate::data::session_stats::PerGameStat::from_replay(&replay, &replay.resource_loader) {
        Some(game) => GameRead::Game(game.to_shared()),
        None => GameRead::NoResults,
    }
}
