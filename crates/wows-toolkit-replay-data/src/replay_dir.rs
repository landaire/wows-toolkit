//! Listing and watching a replays directory.
//!
//! The scan reads every finished replay's header; the watch, on notify's own
//! thread, reports what changes after it. Both report over `futures`
//! channels, and every function here blocks, so a front end runs them on its
//! own workers and reads the channels on its own executor.

use std::collections::HashSet;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use rust_i18n::t;
use wows_replays::ReplayFile;
use wows_toolkit_services::supervision::NextStep;
use wows_toolkit_services::supervision::PanicReport;
use wows_toolkit_viewmodel::listing_row::ListedReplay;
use wowsunpack::data::Version;

use crate::force_reindex::ReindexCandidate;
use crate::load::GameDataCache;
use crate::replay_folder::MergedView;
use crate::replay_folder::ReplayFolder;
use crate::replay_folder::Seen;
use crate::replay_folder::Subfolders;
use crate::replay_folder::WatchPlan;
use crate::replay_folder::WatchTargets;
use crate::replay_index::SourceScope;

/// One replay's header-only read, before ship and map names are translated.
#[derive(Clone, Debug)]
pub struct ScannedReplay {
    pub path: PathBuf,
    pub listed: ListedReplay,
    /// The version and build the replay names, kept because a build number
    /// alone does not identify data to fetch: build numbers are per server, so a
    /// download falls back on the version to find the nearest published build.
    /// `None` when the header's version does not parse.
    pub version: Option<Version>,
}

impl ScannedReplay {
    /// This replay as a forced re-index sees it.
    pub fn reindex_candidate(&self, scope: &SourceScope) -> ReindexCandidate {
        ReindexCandidate::from_listing(self.path.clone(), scope.clone(), self.version, &self.listed.date_time)
    }
}

/// How far the directory scan has got.
///
/// The walk that finds the candidate files is a single cheap pass, so `total`
/// is known before the first meta is read; `done` then climbs through it.
/// A total of zero means the walk itself is still running, which reads as
/// "scanning" with no denominator to state.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScanProgress {
    pub done: usize,
    pub total: usize,
}

/// Files read between one progress report and the next.
///
/// Reporting every file would post a message and a repaint per meta read,
/// which is more work than the read itself on a warm directory.
const SCAN_REPORT_EVERY: usize = 16;

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

/// What a scan read of a directory.
pub struct ScannedDir {
    pub files: Vec<ScannedReplay>,
    /// The replay files the scan settled: those it read, and those it could
    /// not read that were last written before it began. The watch started
    /// after it reports the rest, which arrived or were still being written.
    pub walked: HashSet<PathBuf>,
}

/// Lists every non-temp `*.wowsreplay` file in `replays_dir` and its
/// subdirectories and reads each one's header-only metadata. See [`scan_folder`].
pub fn scan_replay_files(
    replays_dir: &Path,
    reports: &futures::channel::mpsc::UnboundedSender<ScanProgress>,
) -> ScannedDir {
    scan_folder(&ReplayFolder::from_dir(replays_dir.to_path_buf()), reports)
}

/// Lists every non-temp `*.wowsreplay` file in `folder` and reads each one's
/// header-only metadata. Ports `task/replays.rs::replay_filepaths`'s filter
/// (minus the creation-time sort, which `build_browser_tree` redoes by battle
/// time) plus a per-file `ReplayFile::meta_from_file` read. A file that
/// fails to parse (corrupt, mid-write, or from a format this parser does not
/// understand) is logged and skipped rather than aborting the whole scan.
pub fn scan_folder(
    folder: &ReplayFolder,
    reports: &futures::channel::mpsc::UnboundedSender<ScanProgress>,
) -> ScannedDir {
    let began = std::time::SystemTime::now();

    // The walk is separated from the meta reads so the listing can say how
    // many files there are to read before it starts reading them.
    let candidates = folder.replay_files();
    let total = candidates.len();
    let mut walked = HashSet::with_capacity(total);
    let _ = reports.unbounded_send(ScanProgress { done: 0, total });

    let mut out = Vec::with_capacity(total);
    for (read, path) in candidates.into_iter().enumerate() {
        match ReplayFile::meta_from_file(&path) {
            Ok(meta) => {
                walked.insert(path.clone());
                out.push(ScannedReplay {
                    version: Version::try_from_client_exe(&meta.clientVersionFromExe),
                    listed: ListedReplay::from_meta(&meta),
                    path,
                });
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), error = ?err, "failed to read replay meta");
                // A time that cannot be read leaves the file to be read again.
                let written = std::fs::metadata(&path).and_then(|metadata| metadata.modified());
                if written.is_ok_and(|written| written < began) {
                    walked.insert(path);
                }
            }
        }
        let done = read + 1;
        if done % SCAN_REPORT_EVERY == 0 || done == total {
            let _ = reports.unbounded_send(ScanProgress { done, total });
        }
    }

    ScannedDir { files: out, walked }
}

/// The listed builds nothing on this machine can read, with how many replays
/// each one accounts for.
///
/// Ports the set the egui app offers to download after a directory scan
/// (`app.rs`'s `DirectoryScanned` arm). A replay whose header names no build is
/// not counted: nothing could be fetched for it.
pub fn missing_builds(files: &[ScannedReplay], cache: &GameDataCache) -> Vec<MissingBuild> {
    let mut counted: std::collections::BTreeMap<u32, MissingBuild> = std::collections::BTreeMap::new();
    for file in files {
        let Some(build) = file.listed.build else { continue };
        let entry = counted.entry(build).or_insert_with(|| MissingBuild { build, version: None, replays: 0 });
        entry.replays += 1;
        if entry.version.is_none() {
            entry.version = file.version.as_ref().map(crate::force_reindex::version_said);
        }
    }

    counted.into_values().filter(|missing| !cache.can_read_build(missing.build, missing.version.as_deref())).collect()
}

/// Whether a listing's directory watch is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchState {
    /// Not started: the listing starts it when its scan finishes.
    Starting,
    Running,
    /// Stopped; `next` says whether a restart is waiting.
    Stopped {
        cause: WatchStopCause,
        next: NextStep,
    },
}

/// Why a directory watch stopped, or never started.
///
/// No watcher at all is a platform or resource failure, while an unwatchable
/// directory is usually a network share or one that has since gone.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WatchStopCause {
    #[error("no filesystem watcher could be created: {error}")]
    NoWatcher { error: String },
    #[error("the directory could not be watched: {error}")]
    NotWatchable { error: String },
    #[error("the watcher reported an error: {error}")]
    Backend { error: String },
    #[error("the watched directory was moved or removed")]
    DirectoryGone,
    #[error("the watcher panicked: {report}")]
    Panicked { report: PanicReport },
    #[error("the watcher's thread ended")]
    Exited,
}

impl WatchStopCause {
    /// What the reader is shown. The two start failures read as the egui app
    /// words them.
    pub fn said(&self) -> String {
        match self {
            Self::NoWatcher { error } => t!("error.file_watcher_creation", error = error).into_owned(),
            Self::NotWatchable { error } => t!("error.replay_dir_watch", error = error).into_owned(),
            Self::Backend { error } => t!("ui.replay_folders.watch_cause_backend", error = error).into_owned(),
            Self::DirectoryGone => t!("ui.replay_folders.watch_cause_gone").into_owned(),
            Self::Panicked { report } => t!("ui.replay_folders.watch_cause_panicked", error = report).into_owned(),
            Self::Exited => t!("ui.replay_folders.watch_cause_exited").into_owned(),
        }
    }
}

/// What the watcher saw happen to one replay.
///
/// Carried rather than three channels, so the reads stay in the order the
/// directory reported them: a replay created and then written to must not be
/// read in the other order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirChange {
    Appeared(PathBuf),
    Changed(PathBuf),
    Removed(PathBuf),
}

impl DirChange {
    pub fn path(&self) -> &Path {
        match self {
            Self::Appeared(path) | Self::Changed(path) | Self::Removed(path) => path,
        }
    }
}

/// What a directory watch sends its listing.
pub enum WatchMessage {
    Change(DirChange),
    /// The watcher dropped events, so the directory is read again.
    Rescan,
    /// A replay the scan did not walk, found once the watch had started.
    Missed(PathBuf),
    Stopped(WatchStopCause),
}

/// The end of a directory watch that runs on the watcher's own thread.
///
/// Says when the watch stops: on an error from the watcher, on its directory
/// being removed, and on a panic, whether in this code or in the watcher's,
/// which drops this as it unwinds the thread.
pub struct WatchSink {
    targets: WatchTargets,
    /// The watch covers one directory, so every entry it reports is the folder's.
    single_dir: bool,
    /// `Some` for a folder spread over overlay layers.
    merged: Option<MergedView>,
    /// `Some` for a folder listed with its subdirectories.
    subfolders: Option<Subfolders>,
    messages: futures::channel::mpsc::UnboundedSender<WatchMessage>,
    stopped: bool,
    /// Cleared when this is dropped, which the watcher's thread does as it
    /// ends.
    thread_alive: Arc<AtomicBool>,
}

impl WatchSink {
    pub fn new(dir: PathBuf, messages: futures::channel::mpsc::UnboundedSender<WatchMessage>) -> Self {
        let targets = WatchTargets { contents: vec![dir], ..WatchTargets::default() };

        Self::with_targets(targets, None, None, messages)
    }

    fn with_targets(
        targets: WatchTargets,
        merged: Option<MergedView>,
        subfolders: Option<Subfolders>,
        messages: futures::channel::mpsc::UnboundedSender<WatchMessage>,
    ) -> Self {
        let single_dir = targets.watched().len() == 1;

        Self {
            targets,
            single_dir,
            merged,
            subfolders,
            messages,
            stopped: false,
            thread_alive: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn handle(&mut self, event: notify::Result<notify::Event>) {
        if self.stopped {
            return;
        }

        let forwarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.forward(event)));
        let cause = match forwarded {
            Ok(None) => return,
            Ok(Some(cause)) => cause,
            Err(payload) => WatchStopCause::Panicked { report: PanicReport::caught(payload.as_ref()) },
        };
        self.stop(cause);
    }

    /// Passes on what happened to a finished replay, or says why the watch
    /// has stopped.
    fn forward(&mut self, event: notify::Result<notify::Event>) -> Option<WatchStopCause> {
        let event = match event {
            Ok(event) => event,
            Err(err) => return Some(WatchStopCause::Backend { error: err.to_string() }),
        };
        // The watch goes with its directory. A move reads as a rename of the
        // directory itself, after which inotify follows it to its new path.
        let gone = matches!(
            event.kind,
            notify::EventKind::Remove(_) | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
        );
        if gone && event.paths.iter().any(|path| self.targets.contents.contains(path)) {
            return Some(WatchStopCause::DirectoryGone);
        }
        if event.need_rescan() {
            let _ = self.messages.unbounded_send(WatchMessage::Rescan);
            return None;
        }
        // A subdirectory removed or moved away takes its replays with it, though no event names
        // them.
        let lost = gone
            && self.subfolders.as_ref().is_some_and(|subfolders| event.paths.iter().any(|path| subfolders.lost(path)));
        if lost {
            let _ = self.messages.unbounded_send(WatchMessage::Rescan);
            return None;
        }

        let seen = match event.kind {
            notify::EventKind::Create(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Name(notify::event::RenameMode::To)) => {
                Seen::Created
            }
            // A replay written to again: a re-save, a copy that landed
            // before the writer had finished with it, or battle results
            // added to a battle the client left early. Windows reports a
            // content change as `Any`.
            notify::EventKind::Modify(notify::event::ModifyKind::Data(_) | notify::event::ModifyKind::Any) => {
                Seen::Written
            }
            notify::EventKind::Remove(_) => Seen::Removed,
            _ => return None,
        };

        // The folder moved, so the listing resolves and reads it again.
        if seen == Seen::Created && event.paths.iter().any(|path| self.targets.moved_by(path)) {
            let _ = self.messages.unbounded_send(WatchMessage::Rescan);
            return None;
        }

        // A watch of one directory reports only its entries, under whatever
        // spelling of the path the platform uses.
        for path in event.paths {
            if !self.single_dir && !self.targets.holds(&path) {
                continue;
            }
            // A subdirectory created or moved in brings replays no event names.
            if seen == Seen::Created
                && let Some(subfolders) = &mut self.subfolders
            {
                for replay in subfolders.arrived(&path) {
                    let _ = self.messages.unbounded_send(WatchMessage::Missed(replay));
                }
            }
            let changes = match &mut self.merged {
                Some(merged) => merged.apply(&path, seen),
                None if is_finished_replay(&path) => vec![match seen {
                    Seen::Created => DirChange::Appeared(path),
                    Seen::Written => DirChange::Changed(path),
                    Seen::Removed => DirChange::Removed(path),
                }],
                None => Vec::new(),
            };
            for change in changes {
                if let (Some(subfolders), DirChange::Appeared(replay)) = (&mut self.subfolders, &change) {
                    subfolders.hold(replay);
                }
                self.change(change);
            }
        }

        None
    }

    /// Sends `change` to the listing, saying whether it is still there to
    /// read it. It is gone once the directory changes or the app closes.
    pub fn change(&self, change: DirChange) -> bool {
        self.messages.unbounded_send(WatchMessage::Change(change)).is_ok()
    }

    fn stop(&mut self, cause: WatchStopCause) {
        self.stopped = true;
        // A listing that has gone has nothing to restart.
        let _ = self.messages.unbounded_send(WatchMessage::Stopped(cause));
    }
}

impl Drop for WatchSink {
    fn drop(&mut self) {
        self.thread_alive.store(false, Ordering::Release);
        // Dropped by a panic unwinding the watcher's thread, which ends the
        // watch with it.
        if std::thread::panicking() && !self.stopped {
            self.stop(WatchStopCause::Panicked { report: PanicReport::unwinding() });
        }
    }
}

/// A watcher on notify's own thread; dropping it ends the watch.
struct FilesystemWatch {
    watcher: ManuallyDrop<notify::RecommendedWatcher>,
    thread_alive: Arc<AtomicBool>,
}

impl Drop for FilesystemWatch {
    fn drop(&mut self) {
        // notify's inotify watcher panics as it is dropped once its thread has
        // ended, so one whose thread has ended is leaked instead.
        if self.thread_alive.load(Ordering::Acquire) {
            // SAFETY: dropped once, here, and never used again.
            unsafe { ManuallyDrop::drop(&mut self.watcher) };
        }
    }
}

/// Holds a directory's watch open; dropping it ends the watch.
pub struct DirWatch {
    _handle: WatchHandle,
}

enum WatchHandle {
    Filesystem {
        _watch: FilesystemWatch,
    },
    #[cfg(any(test, feature = "test-support"))]
    Simulated {
        _registration: crate::simulated_watch::Registration,
    },
}

pub(crate) fn is_finished_replay(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("wowsreplay")
        && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
}

/// The replay a directory entry names, when the listing shows it. The scan
/// and the check for replays the watch missed both read entries through
/// this, so a file one of them lists is never news to the other.
///
/// A symlink counts when it leads to a file: a replays directory of links
/// into another install is a reader's own arrangement.
pub(crate) fn listed_replay(entry: &std::fs::DirEntry) -> Option<PathBuf> {
    let path = entry.path();
    if !is_finished_replay(&path) {
        return None;
    }
    let file_type = entry.file_type().ok()?;
    let is_file = file_type.is_file() || (file_type.is_symlink() && path.is_file());
    is_file.then_some(path)
}

/// Starts the watch `plan` prepared, sending what happens in its folder to
/// `messages`.
pub fn start_planned_watch(
    plan: WatchPlan,
    messages: futures::channel::mpsc::UnboundedSender<WatchMessage>,
) -> Result<DirWatch, WatchStopCause> {
    let WatchPlan { targets, merged, subfolders } = plan;
    let watched = targets.watched();

    start(&watched, WatchSink::with_targets(targets, merged, subfolders, messages))
}

/// Watches `watched`, the first of which names the watch to a test.
fn start(watched: &[PathBuf], sink: WatchSink) -> Result<DirWatch, WatchStopCause> {
    // The test scheduler refuses wakeups from notify's own thread, so a test
    // reports changes itself.
    #[cfg(any(test, feature = "test-support"))]
    if crate::simulated_watch::is_installed() {
        let Some(dir) = watched.first() else {
            return Err(WatchStopCause::NotWatchable { error: "nothing to watch".to_owned() });
        };
        return crate::simulated_watch::register(dir, sink)
            .map(|registration| DirWatch { _handle: WatchHandle::Simulated { _registration: registration } });
    }

    filesystem_watch(watched, sink).map(|watch| DirWatch { _handle: WatchHandle::Filesystem { _watch: watch } })
}

/// Watches each of `watched` on notify's own thread.
fn filesystem_watch(watched: &[PathBuf], mut sink: WatchSink) -> Result<FilesystemWatch, WatchStopCause> {
    use notify::Watcher as _;

    let thread_alive = Arc::clone(&sink.thread_alive);
    let modes: Vec<notify::RecursiveMode> = watched
        .iter()
        .map(|dir| {
            if sink.targets.is_recursive(dir) {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            }
        })
        .collect();
    let watcher = notify::recommended_watcher(move |event| sink.handle(event))
        .map_err(|err| WatchStopCause::NoWatcher { error: err.to_string() })?;
    let mut watch = FilesystemWatch { watcher: ManuallyDrop::new(watcher), thread_alive };
    for (dir, mode) in watched.iter().zip(modes) {
        watch.watcher.watch(dir, mode).map_err(|err| WatchStopCause::NotWatchable { error: err.to_string() })?;
    }

    Ok(watch)
}

/// How many times to read a replay the watcher has just reported, and how
/// long to wait between attempts. The file appears before the game has
/// finished writing its header, so the first read usually fails.
pub const APPEARED_REPLAY_ATTEMPTS: usize = 10;
pub const APPEARED_REPLAY_RETRY: std::time::Duration = std::time::Duration::from_millis(300);

/// Reads one replay's header. `None` while it is still being written, or when
/// it is not a replay after all.
pub fn read_replay(path: PathBuf) -> Option<ScannedReplay> {
    let meta = ReplayFile::meta_from_file(&path).ok()?;

    Some(ScannedReplay {
        version: Version::try_from_client_exe(&meta.clientVersionFromExe),
        listed: ListedReplay::from_meta(&meta),
        path,
    })
}

/// Why a named replay's header could not be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HeaderReadError {
    #[error("{reason}")]
    Failed { reason: String },
    #[error("the reader stopped on an internal error: {report}")]
    Panicked { report: PanicReport },
}

/// A named replay whose header could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnreadableReplay {
    pub path: PathBuf,
    pub error: HeaderReadError,
}

/// What a read of named replay files found.
pub struct ReadReplays {
    pub files: Vec<ScannedReplay>,
    pub unreadable: Vec<UnreadableReplay>,
}

/// Reads the header of each of `paths`, which the reader named rather than a
/// directory listing found, so a file that cannot be read is reported rather
/// than skipped. A panic in the reader fails only its own file.
pub fn read_replay_files(
    paths: &[PathBuf],
    reports: &futures::channel::mpsc::UnboundedSender<ScanProgress>,
) -> ReadReplays {
    let total = paths.len();
    let _ = reports.unbounded_send(ScanProgress { done: 0, total });
    let mut read = ReadReplays { files: Vec::with_capacity(total), unreadable: Vec::new() };
    for (at, path) in paths.iter().enumerate() {
        let meta = std::panic::catch_unwind(|| {
            #[cfg(any(test, feature = "test-support"))]
            crate::simulated_panic::panic_if_asked(path);
            ReplayFile::meta_from_file(path)
        });
        let error = match meta {
            Ok(Ok(meta)) => {
                read.files.push(ScannedReplay {
                    version: Version::try_from_client_exe(&meta.clientVersionFromExe),
                    listed: ListedReplay::from_meta(&meta),
                    path: path.clone(),
                });
                None
            }
            Ok(Err(report)) => Some(HeaderReadError::Failed { reason: report.current_context().to_string() }),
            Err(payload) => Some(HeaderReadError::Panicked { report: PanicReport::caught(payload.as_ref()) }),
        };
        if let Some(error) = error {
            tracing::warn!(path = %path.display(), %error, "a named replay could not be read");
            read.unreadable.push(UnreadableReplay { path: path.clone(), error });
        }
        let done = at + 1;
        if done % SCAN_REPORT_EVERY == 0 || done == total {
            let _ = reports.unbounded_send(ScanProgress { done, total });
        }
    }

    read
}

/// Sends the replays in `folder` the scan did not walk to `messages`, as
/// [`WatchMessage::Missed`]. They reached the folder before the watch
/// started and after the scan read it, so neither saw them. A folder that
/// cannot be read cannot be watched either, which the watch reports.
pub fn report_missed_in(
    folder: &ReplayFolder,
    walked: &HashSet<PathBuf>,
    messages: &futures::channel::mpsc::UnboundedSender<WatchMessage>,
) {
    for path in folder.replay_files() {
        if !walked.contains(&path) {
            let _ = messages.unbounded_send(WatchMessage::Missed(path));
        }
    }
}

#[cfg(test)]
mod tests {
    /// A build nothing on this machine can read is reported once, with how many
    /// of the listed replays are waiting on it, so the offer to fetch it can say
    /// what it is for. A build the cache holds is not reported at all.
    #[test]
    fn the_listing_reports_only_the_builds_nothing_can_read() {
        use std::path::PathBuf;

        let cache_dir = tempfile::tempdir().expect("a temp directory");
        std::fs::create_dir_all(cache_dir.path().join("0.10.5_100")).expect("the dump directory is created");
        std::fs::write(cache_dir.path().join("0.10.5_100").join("metadata.toml"), "").expect("the dump is marked");

        let listed = |build: Option<u32>, version: &str| super::ScannedReplay {
            path: PathBuf::from(format!("{}-{version}.wowsreplay", build.unwrap_or_default())),
            version: (!version.is_empty()).then(|| {
                let client_exe = format!("{},{}", version.replace('.', ","), build.unwrap_or_default());
                wowsunpack::data::Version::try_from_client_exe(&client_exe).expect("the test version parses")
            }),
            listed: wows_toolkit_viewmodel::listing_row::ListedReplay {
                ship_id: None,
                map_name: String::new(),
                game_type: String::new(),
                scenario: String::new(),
                date_time: String::new(),
                build,
                rating_scope: wows_core::rating_scope::RatingScope::Unrated,
            },
        };

        let files = vec![
            listed(Some(100), "0.10.5"),
            listed(Some(200), "0.11.0"),
            listed(Some(200), "0.11.0"),
            // A header naming no build: nothing could be fetched for it.
            listed(None, ""),
        ];

        // A directory that is not an install, so only the cache can answer.
        let cache = crate::load::GameDataCache::new(PathBuf::from("G:/does-not-exist"))
            .with_cache_dir(&cache_dir.path().to_string_lossy());

        let missing = super::missing_builds(&files, &cache);
        assert_eq!(missing.len(), 1, "the cached build is not offered, got {missing:?}");
        assert_eq!(missing[0].build, 200);
        assert_eq!(missing[0].version.as_deref(), Some("0.11.0"));
        assert_eq!(missing[0].replays, 2, "both replays of that build are counted");
    }

    /// A replay reached through a symlink is walked by the scan, so the watch
    /// never reports it as one that arrived after the listing was read.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_replay_is_walked_rather_than_reported_as_new() {
        let real = tempfile::tempdir().expect("a temporary directory");
        let listed = tempfile::tempdir().expect("a temporary directory");
        let target = real.path().join("20260818_132652_PWSD610-Smaland.wowsreplay");
        std::fs::write(&target, b"not a replay").expect("the file is written");
        let link = listed.path().join("20260818_132652_PWSD610-Smaland.wowsreplay");
        std::os::unix::fs::symlink(&target, &link).expect("the link is made");
        let dangling = listed.path().join("20260818_132653_PWSD610-Gone.wowsreplay");
        std::os::unix::fs::symlink(real.path().join("gone.wowsreplay"), &dangling).expect("the link is made");
        // The scan counts a file written before it began as walked even when
        // it cannot be read, so the written time must be in the past.
        std::thread::sleep(std::time::Duration::from_millis(20));

        let (reports, _progress) = futures::channel::mpsc::unbounded();
        let scanned = super::scan_replay_files(listed.path(), &reports);

        assert!(scanned.walked.contains(&link), "the link to a replay is walked");
        assert!(!scanned.walked.contains(&dangling), "a link to nothing is not a replay");
    }

    use super::is_finished_replay;
    use std::path::Path;

    #[test]
    fn the_watcher_ignores_everything_but_a_finished_replay() {
        assert!(is_finished_replay(Path::new("replays/20260818_132652_PWSD610-Smaland.wowsreplay")));
        // The match in progress, which has no metadata until it ends.
        assert!(!is_finished_replay(Path::new("replays/temp.wowsreplay")));
        assert!(!is_finished_replay(Path::new("replays/tempArenaInfo.json")));
        assert!(!is_finished_replay(Path::new("replays/20260818_132652_PWSD610-Smaland")));
    }

    /// The filesystem watch reports a replay written into its directory, and
    /// reports nothing more once it is dropped.
    #[test]
    fn the_filesystem_watch_reports_a_new_replay_until_it_is_dropped() {
        use std::time::Duration;
        use std::time::Instant;

        use futures::channel::mpsc::TryRecvError;

        use super::DirChange;
        use super::WatchMessage;
        use super::WatchSink;

        // Waits for the next report; `None` once the watch has closed the
        // channel or nothing arrives in time.
        fn next(
            messages: &mut futures::channel::mpsc::UnboundedReceiver<WatchMessage>,
            deadline: Instant,
        ) -> Option<WatchMessage> {
            loop {
                match messages.try_recv() {
                    Ok(message) => return Some(message),
                    Err(TryRecvError::Empty) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(TryRecvError::Empty | TryRecvError::Closed) => return None,
                }
            }
        }

        let dir = tempfile::tempdir().expect("a temp directory");
        let (sender, mut messages) = futures::channel::mpsc::unbounded();
        let sink = WatchSink::new(dir.path().to_path_buf(), sender);
        let watcher = super::filesystem_watch(&[dir.path().to_path_buf()], sink).expect("the directory is watchable");

        let landed = dir.path().join("landed.wowsreplay");
        std::fs::write(&landed, b"not yet a replay").expect("the file is written");
        let deadline = Instant::now() + Duration::from_secs(10);
        let first = next(&mut messages, deadline);
        assert!(
            matches!(&first, Some(WatchMessage::Change(DirChange::Appeared(path))) if *path == landed),
            "the new file is reported first"
        );

        // Dropping it closes the channel, which is what ends the listing's
        // task for the watch. Reports queued before the drop may come first.
        drop(watcher);
        let deadline = Instant::now() + Duration::from_secs(10);
        while next(&mut messages, deadline).is_some() {}
        assert!(matches!(messages.try_recv(), Err(TryRecvError::Closed)), "the dropped watch closes its channel");
    }

    /// Removing the watched directory ends the watch with it, which the watch
    /// says rather than falling silent.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_filesystem_watch_says_its_directory_was_removed() {
        use std::time::Duration;
        use std::time::Instant;

        use super::WatchMessage;
        use super::WatchSink;
        use super::WatchStopCause;

        let parent = tempfile::tempdir().expect("a temp directory");
        let dir = parent.path().join("replays");
        std::fs::create_dir(&dir).expect("the directory is made");
        let (sender, mut messages) = futures::channel::mpsc::unbounded();
        let _watcher = super::filesystem_watch(std::slice::from_ref(&dir), WatchSink::new(dir.clone(), sender))
            .expect("the directory is watchable");

        std::fs::remove_dir(&dir).expect("the directory is removed");

        let deadline = Instant::now() + Duration::from_secs(10);
        let stopped = loop {
            match messages.try_recv() {
                Ok(WatchMessage::Stopped(cause)) => break Some(cause),
                Ok(WatchMessage::Change(_) | WatchMessage::Rescan | WatchMessage::Missed(_)) => {}
                Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break None,
            }
        };
        assert_eq!(stopped, Some(WatchStopCause::DirectoryGone));
    }

    /// A chosen folder's watch reports a replay written into a subfolder and the replays of a
    /// subfolder moved in, and has the folder read again when a subfolder holding replays leaves.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_chosen_folder_is_watched_with_its_subfolders() {
        use std::time::Duration;
        use std::time::Instant;

        use super::DirChange;
        use super::ReplayFolder;
        use super::WatchMessage;

        // Waits for a report `wanted` accepts, passing over other changes. A rescan or a stop
        // that is not wanted fails the wait.
        fn wait_for(
            messages: &mut futures::channel::mpsc::UnboundedReceiver<WatchMessage>,
            wanted: impl Fn(&WatchMessage) -> bool,
        ) -> bool {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match messages.try_recv() {
                    Ok(message) if wanted(&message) => return true,
                    Ok(WatchMessage::Rescan | WatchMessage::Stopped(_)) => return false,
                    Ok(WatchMessage::Change(_) | WatchMessage::Missed(_)) => {}
                    Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                    Err(_) => return false,
                }
            }
        }

        let parent = tempfile::tempdir().expect("a temp directory");
        let root = parent.path().join("chosen");
        std::fs::create_dir_all(root.join("13.11.0.0")).expect("the directory is made");
        std::fs::write(root.join("13.11.0.0/old.wowsreplay"), b"listed").expect("the file is written");
        let (sender, mut messages) = futures::channel::mpsc::unbounded();
        let plan = ReplayFolder::from_dir(root.clone()).watch_plan();
        let _watch = super::start_planned_watch(plan, sender).expect("the directory is watchable");

        let written = root.join("13.11.0.0/new.wowsreplay");
        std::fs::write(&written, b"not yet a replay").expect("the file is written");
        assert!(
            wait_for(
                &mut messages,
                |message| matches!(message, WatchMessage::Change(DirChange::Appeared(path)) if *path == written)
            ),
            "a replay written into a subfolder is reported"
        );

        let staged = parent.path().join("staged");
        std::fs::create_dir_all(staged.join("deeper")).expect("the directory is made");
        std::fs::write(staged.join("deeper/moved.wowsreplay"), b"not a replay").expect("the file is written");
        std::fs::rename(&staged, root.join("13.12.0.0")).expect("the directory is moved in");
        let moved = root.join("13.12.0.0/deeper/moved.wowsreplay");
        assert!(
            wait_for(&mut messages, |message| matches!(message, WatchMessage::Missed(path) if *path == moved)),
            "the replays of a subfolder moved in are reported"
        );

        std::fs::rename(root.join("13.11.0.0"), parent.path().join("away")).expect("the directory is moved out");
        assert!(
            wait_for(&mut messages, |message| matches!(message, WatchMessage::Rescan)),
            "a subfolder moved out has the folder read again"
        );
    }
}
