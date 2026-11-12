//! One directory's listing: its scan, its watch, the index's facts about its replays, and the
//! rows they make.
//!
//! The install's replays directory and every directory the reader opened each have one. The scan
//! reads every replay's header off the UI thread, then the watch, when the listing is watched,
//! reports what changes after it. Every result is checked against the scan and watch it belongs
//! to, since the directory may have been listed again while it was on its way.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::channel::oneshot;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_replay_data::force_reindex::ReindexCandidate;
use wows_toolkit_replay_data::install::resolve_replays_dir;
use wows_toolkit_replay_data::replay_dir::APPEARED_REPLAY_ATTEMPTS;
use wows_toolkit_replay_data::replay_dir::APPEARED_REPLAY_RETRY;
use wows_toolkit_replay_data::replay_dir::DirChange;
use wows_toolkit_replay_data::replay_dir::DirWatch;
use wows_toolkit_replay_data::replay_dir::MissingBuild;
use wows_toolkit_replay_data::replay_dir::ScanProgress;
use wows_toolkit_replay_data::replay_dir::ScannedReplay;
use wows_toolkit_replay_data::replay_dir::WatchMessage;
use wows_toolkit_replay_data::replay_dir::WatchSink;
use wows_toolkit_replay_data::replay_dir::WatchState;
use wows_toolkit_replay_data::replay_dir::WatchStopCause;
use wows_toolkit_replay_data::replay_dir::missing_builds;
use wows_toolkit_replay_data::replay_dir::read_replay;
use wows_toolkit_replay_data::replay_dir::report_missed;
use wows_toolkit_replay_data::replay_dir::scan_replay_files;
use wows_toolkit_replay_data::replay_dir::start_watch;
use wows_toolkit_replay_data::replay_index::SourceScope;
use wows_toolkit_services::supervision::NextStep;
use wows_toolkit_services::supervision::RestartPolicy;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::Task;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;

use crate::services::GameDataState;
use crate::services::Services;
use crate::tabs::replays::browser::tree::BrowserTree;
use crate::tabs::replays::browser::tree::NodeKey;
use crate::tabs::replays::browser::tree::TreeInputs;

/// Which directory a listing reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListingSource {
    /// The install's replays directory, found from the game directory in the settings.
    Install,
    /// A directory the reader opened.
    Chosen(PathBuf),
}

/// Whether a listing follows its directory after the scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListingWatch {
    Watched,
    Unwatched,
}

/// What the listing has to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanStatus {
    /// No game directory is set, so the install's directory is unknown.
    NoInstall,
    Loading,
    Loaded,
    /// The directory holds no replay.
    Empty(PathBuf),
}

/// What a listing tells its owner as it happens.
#[derive(Clone, Debug)]
pub enum ListingEvent {
    /// A replay new to the directory was read.
    Appeared(PathBuf),
    /// A listed replay was written again and read again.
    Changed(PathBuf),
    Removed(PathBuf),
    WatchStopped {
        cause: WatchStopCause,
        next: NextStep,
    },
    /// A watch that had stopped is running again.
    WatchResumed,
    /// The listing stopped watching because it was told to, so whatever was said about its
    /// watch no longer holds.
    WatchWithdrawn,
    /// The listed replays need these builds, which nothing on this machine can read.
    BuildsMissing(Vec<MissingBuild>),
}

/// One directory's listing, as a `Copy` handle. Made in a component's body; everything it
/// started stops with that component.
#[derive(Clone, Copy)]
pub struct Listing {
    pub source: RwSignal<ListingSource, LocalStorage>,
    pub files: RwSignal<Vec<ScannedReplay>, LocalStorage>,
    pub status: RwSignal<ScanStatus, LocalStorage>,
    pub progress: RwSignal<Option<ScanProgress>, LocalStorage>,
    pub summaries: RwSignal<HashMap<PathBuf, RowSummary>, LocalStorage>,
    /// The directory the last scan read.
    pub directory: RwSignal<Option<PathBuf>, LocalStorage>,
    pub grouping: RwSignal<ReplayGrouping, LocalStorage>,
    pub watch_state: RwSignal<WatchState, LocalStorage>,

    pub tree: RwSignal<Rc<BrowserTree>, LocalStorage>,
    pub selection: RwSignal<HashSet<NodeKey>, LocalStorage>,
    pub expanded: RwSignal<HashSet<NodeKey>, LocalStorage>,
    /// Replays the watch added since the reader last looked: they carry the New tag and glow
    /// once as they arrive.
    pub arrivals: RwSignal<HashSet<PathBuf>, LocalStorage>,

    work: StoredValue<Work, LocalStorage>,
}

/// What a listing runs, none of which a view reads.
struct Work {
    services: Services,
    owner: Owner,
    timers: Option<Timers>,
    report: Rc<dyn Fn(ListingEvent)>,
    watch: ListingWatch,
    watcher: Option<DirWatch>,
    restarts: RestartPolicy,
    scan_generation: u64,
    watch_generation: u64,
    summaries_generation: u64,
    scan: Option<Task>,
    watching: Option<Task>,
    summaries: Option<Task>,
    restart: Option<TimeoutHandle>,
    /// Groups seen so far, so a new one opens expanded and one the reader closed stays closed.
    known_groups: HashSet<NodeKey>,
}

impl Listing {
    /// A listing of `source`. The install's is scanned at once and whenever the game directory
    /// changes; a chosen directory's when [`Listing::relist`] is called, once the directory has
    /// been found. Called from a component's body.
    pub fn new(
        source: ListingSource,
        watch: ListingWatch,
        grouping: ReplayGrouping,
        report: Rc<dyn Fn(ListingEvent)>,
    ) -> Self {
        let services = Services::current();
        let listing = Self {
            source: RwSignal::new_local(source),
            files: RwSignal::new_local(Vec::new()),
            status: RwSignal::new_local(ScanStatus::Loading),
            progress: RwSignal::new_local(None),
            summaries: RwSignal::new_local(HashMap::new()),
            directory: RwSignal::new_local(None),
            grouping: RwSignal::new_local(grouping),
            watch_state: RwSignal::new_local(WatchState::Starting),
            tree: RwSignal::new_local(Rc::new(BrowserTree::default())),
            selection: RwSignal::new_local(HashSet::new()),
            expanded: RwSignal::new_local(HashSet::new()),
            arrivals: RwSignal::new_local(HashSet::new()),
            work: StoredValue::new_local(Work {
                services,
                owner: Owner::current().expect("a listing is made inside a component"),
                timers: Timers::current(),
                report,
                watch,
                watcher: None,
                restarts: RestartPolicy::new(),
                scan_generation: 0,
                watch_generation: 0,
                summaries_generation: 0,
                scan: None,
                watching: None,
                summaries: None,
                restart: None,
                known_groups: HashSet::new(),
            }),
        };

        listing.follow_inputs(services);
        on_cleanup_local(move || {
            listing.work.try_update_value(|work| {
                work.watcher = None;
                work.restart = None;
            });
        });
        // The install's directory follows the game directory, which the settings name once they
        // are read and the reader may change.
        let relisting = RenderEffect::new(move |previous: Option<Option<String>>| {
            let wows_dir = services.settings.read(|settings| settings.wows_dir.clone());
            let follows = listing.source.with_untracked(|source| *source == ListingSource::Install);
            if follows && previous.as_ref() != Some(&wows_dir) {
                // The last install's directory is no longer this listing's, even while the new
                // one is being resolved.
                if listing.directory.get_untracked().is_some() {
                    listing.directory.set(None);
                }
                listing.relist();
            }
            wows_dir
        });
        on_cleanup_local(move || drop(relisting));

        listing
    }

    /// Builds the rows again whenever what they are made of changes.
    fn follow_inputs(self, services: Services) {
        let effect = RenderEffect::new(move |_| {
            let state = services.game_data.state();
            let cache = services.game_data.cache();
            let locale = services.settings.read(|settings| settings.locale.clone()).flatten();
            let grouping = self.grouping.get();
            let fallback = match &state {
                GameDataState::Ready(loaded) => Some(std::sync::Arc::clone(loaded.provider())),
                _ => None,
            };
            let tree = self.files.with(|files| {
                self.summaries.with(|summaries| {
                    BrowserTree::build(&TreeInputs {
                        files,
                        summaries,
                        cache: cache.as_ref(),
                        fallback: fallback.as_deref(),
                        grouping,
                        locale: locale.as_deref(),
                    })
                })
            });

            let fresh: Vec<NodeKey> = self.work.with_value(|work| {
                tree.groups()
                    .map(|id| NodeKey::Group(id.clone()))
                    .filter(|key| !work.known_groups.contains(key))
                    .collect()
            });
            if !fresh.is_empty() {
                self.work.update_value(|work| work.known_groups.extend(fresh.iter().cloned()));
                self.expanded.update(|expanded| expanded.extend(fresh));
            }
            self.tree.set(Rc::new(tree));
        });
        on_cleanup_local(move || drop(effect));
    }

    /// The index scope the listing's replays are written under.
    pub fn index_scope(self) -> SourceScope {
        match self.source.get_untracked() {
            ListingSource::Install => SourceScope::Live,
            ListingSource::Chosen(root) => SourceScope::Folder(root),
        }
    }

    /// Whether the listing holds `path`.
    pub fn holds(self, path: &Path) -> bool {
        self.files.with_untracked(|files| files.iter().any(|file| file.path == path))
    }

    /// The listed replays, in listing order.
    pub fn listed_paths(self) -> Vec<PathBuf> {
        self.tree.with_untracked(|tree| tree.order().to_vec())
    }

    /// The listed replays as a forced re-index sees them.
    pub fn reindex_candidates(self) -> Vec<ReindexCandidate> {
        let scope = self.index_scope();
        self.files.with_untracked(|files| files.iter().map(|file| file.reindex_candidate(&scope)).collect())
    }

    /// The selected replays, sorted. Groups in the selection add nothing.
    pub fn selected_paths(self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.selection.with_untracked(|selection| {
            selection.iter().filter_map(|key| key.leaf_path().map(Path::to_path_buf)).collect()
        });
        paths.sort();

        paths
    }

    /// "Ship - Map" for `path` while it is listed.
    pub fn battle_label(self, path: &Path) -> Option<String> {
        self.tree.with_untracked(|tree| tree.leaf(path).map(|leaf| leaf.battle.clone()))
    }

    /// The row was opened, so it is no longer new.
    pub fn mark_opened(self, path: &Path) {
        if self.arrivals.with_untracked(|arrivals| arrivals.contains(path)) {
            self.arrivals.update(|arrivals| {
                arrivals.remove(path);
            });
        }
    }

    /// The reader left the listing, so nothing in it is new any more.
    pub fn clear_arrivals(self) {
        if self.arrivals.with_untracked(|arrivals| !arrivals.is_empty()) {
            self.arrivals.set(HashSet::new());
        }
    }

    pub fn set_grouping(self, grouping: ReplayGrouping) {
        if self.grouping.get_untracked() != grouping {
            self.grouping.set(grouping);
        }
    }

    /// Names another directory, as when a chosen directory turns out to be another path. The
    /// caller relists once the directory is known to be there.
    pub fn set_source(self, source: ListingSource) {
        if self.source.get_untracked() != source {
            self.source.set(source);
        }
    }

    pub fn set_watch(self, watch: ListingWatch) {
        let changed = self.work.try_update_value(|work| std::mem::replace(&mut work.watch, watch) != watch);
        if changed != Some(true) {
            return;
        }

        match watch {
            ListingWatch::Unwatched => self.end_watch(),
            // A scan under way starts the watch when it finishes.
            ListingWatch::Watched if self.status.get_untracked() == ScanStatus::Loading => {}
            ListingWatch::Watched => self.relist(),
        }
    }

    pub fn watch(self) -> ListingWatch {
        self.work.with_value(|work| work.watch)
    }

    /// Starts a stopped watch again, with its restarts counted afresh.
    pub fn restart_watch(self) {
        if !matches!(self.watch_state.get_untracked(), WatchState::Stopped { .. }) {
            return;
        }
        self.work.update_value(|work| {
            work.restarts.reset();
            work.restart = None;
        });
        self.relist();
    }

    /// Reads the directory again from the start.
    pub fn relist(self) {
        let generation = self.work.try_update_value(|work| {
            work.scan_generation += 1;
            if let Some(scan) = work.scan.take() {
                scan.cancel();
            }
            work.scan_generation
        });
        let Some(generation) = generation else { return };

        let source = self.source.get_untracked();
        let wows_dir =
            self.work.with_value(|work| work.services.settings.read_untracked(|settings| settings.wows_dir.clone()));
        if source == ListingSource::Install && wows_dir.as_deref().is_none_or(str::is_empty) {
            self.end_watch();
            self.status.set(ScanStatus::NoInstall);
            return;
        }

        self.status.set(ScanStatus::Loading);
        self.progress.set(Some(ScanProgress::default()));
        let task = self.spawn(async move {
            let dir = match source {
                ListingSource::Install => {
                    let wows = PathBuf::from(wows_dir.unwrap_or_default());
                    blocking(move || resolve_replays_dir(&wows)).await
                }
                ListingSource::Chosen(root) => root,
            };
            let directory_changed = self.directory.get_untracked().as_ref() != Some(&dir);
            if directory_changed {
                self.directory.set(Some(dir.clone()));
                self.summaries.set(HashMap::new());
                self.load_summaries();
            }

            let (reports, mut progress) = mpsc::unbounded();
            let scanned_dir = dir.clone();
            let scan = blocking(move || scan_replay_files(&scanned_dir, &reports));
            let shown = self.spawn(async move {
                while let Some(report) = progress.next().await {
                    self.show_scan_progress(generation, report);
                }
            });
            let scanned = scan.await;
            shown.cancel();
            if !self.is_current_scan(generation) {
                return;
            }

            self.progress.set(None);
            let status = if scanned.files.is_empty() { ScanStatus::Empty(dir.clone()) } else { ScanStatus::Loaded };
            self.files.set(scanned.files);
            self.status.set(status);
            self.watch_dir(dir, generation, scanned.walked);
            self.report_missing_builds();
        });
        self.work.update_value(|work| work.scan = Some(task));
    }

    /// Reads the index's facts about the listed replays, which the rows report.
    pub fn load_summaries(self) {
        let Some(dir) = self.directory.get_untracked() else { return };
        let scope = match self.source.get_untracked() {
            ListingSource::Install => SourceScope::Live,
            ListingSource::Chosen(_) => SourceScope::Folder(dir),
        };
        let services = self.work.with_value(|work| work.services);
        let Some(pool) = services.settings.pool() else { return };
        let generation = self.work.try_update_value(|work| {
            work.summaries_generation += 1;
            work.summaries_generation
        });
        let Some(generation) = generation else { return };

        let task = self.spawn(async move {
            let read = services
                .on_tokio(async move {
                    match scope.existing(&pool).await? {
                        Some(source) => {
                            wows_toolkit_config::index::query::row_summaries_for_source(&pool, source).await
                        }
                        None => Ok(HashMap::new()),
                    }
                })
                .await;
            let current = self.work.try_with_value(|work| work.summaries_generation == generation);
            if current != Some(true) {
                return;
            }
            match read {
                Ok(Ok(summaries)) => self.summaries.set(summaries),
                Ok(Err(err)) => tracing::warn!(error = %err, "the listing's index facts could not be read"),
                Err(err) => tracing::warn!(error = %err, "the listing's index facts could not be read"),
            }
        });
        self.work.update_value(|work| work.summaries = Some(task));
    }

    /// Drops `path`, which left the directory.
    pub fn forget(self, path: &Path) {
        self.files.update(|files| files.retain(|file| file.path != path));
        let key = NodeKey::Leaf(path.to_path_buf());
        if self.selection.with_untracked(|selection| selection.contains(&key)) {
            self.selection.update(|selection| {
                selection.remove(&key);
            });
        }
        self.mark_opened(path);
    }

    /// Shows how far scan `generation` has got, unless a later scan has started since.
    pub(super) fn show_scan_progress(self, generation: u64, report: ScanProgress) {
        if self.is_current_scan(generation) {
            self.progress.set(Some(report));
        }
    }

    #[cfg(test)]
    pub(super) fn scan_generation(self) -> u64 {
        self.work.with_value(|work| work.scan_generation)
    }

    fn is_current_scan(self, generation: u64) -> bool {
        self.work.try_with_value(|work| work.scan_generation == generation) == Some(true)
    }

    fn is_current_watch(self, scan: u64, watch: u64) -> bool {
        self.work.try_with_value(|work| work.scan_generation == scan && work.watch_generation == watch) == Some(true)
    }

    /// Runs `future` under the listing's component, so it stops with it even when started
    /// from a listener.
    fn spawn(self, future: impl Future<Output = ()> + 'static) -> Task {
        let owner = self.work.with_value(|work| work.owner.clone());
        owner.with(|| spawn_local(future))
    }

    fn say(self, event: ListingEvent) {
        if let Some(report) = self.work.try_with_value(|work| Rc::clone(&work.report)) {
            report(event);
        }
    }

    /// Reports the builds the listed replays need that nothing here can read, if any.
    fn report_missing_builds(self) {
        let missing = self.missing_builds();
        if !missing.is_empty() {
            self.say(ListingEvent::BuildsMissing(missing));
        }
    }

    /// The builds the listed replays need that nothing here can read. With no cache yet, every
    /// listed build is.
    pub fn missing_builds(self) -> Vec<MissingBuild> {
        let cache = self.work.with_value(|work| work.services.game_data.cache_untracked());
        self.files.with_untracked(|files| match &cache {
            Some(cache) => missing_builds(files, cache),
            None => {
                let mut counted: std::collections::BTreeMap<u32, MissingBuild> = std::collections::BTreeMap::new();
                for file in files {
                    let Some(build) = file.listed.build else { continue };
                    let entry = counted.entry(build).or_insert_with(|| MissingBuild {
                        build,
                        version: file.version.as_ref().map(wows_toolkit_replay_data::force_reindex::version_said),
                        replays: 0,
                    });
                    entry.replays += 1;
                }
                counted.into_values().collect()
            }
        })
    }

    /// Drops the watch, as the listing no longer follows its directory.
    fn end_watch(self) {
        let was_stopped = matches!(self.watch_state.get_untracked(), WatchState::Stopped { .. });
        self.stop_watching();
        self.work.update_value(|work| {
            work.restart = None;
            work.restarts.reset();
        });
        self.watch_state.set(WatchState::Starting);
        if was_stopped {
            self.say(ListingEvent::WatchWithdrawn);
        }
    }

    fn stop_watching(self) {
        self.work.update_value(|work| {
            work.watcher = None;
            work.watch_generation += 1;
            if let Some(task) = work.watching.take() {
                task.cancel();
            }
        });
    }

    /// Follows `dir` from the end of scan `scan`, which read the files in `walked`.
    fn watch_dir(self, dir: PathBuf, scan: u64, walked: HashSet<PathBuf>) {
        self.stop_watching();
        if self.watch() == ListingWatch::Unwatched {
            return;
        }

        let (messages, mut received) = mpsc::unbounded();
        let started = start_watch(&dir, WatchSink::new(dir.clone(), messages.clone()));
        let watcher = match started {
            Ok(watcher) => watcher,
            Err(cause) => {
                self.watch_stopped(cause);
                return;
            }
        };
        let watch = self.work.with_value(|work| work.watch_generation);
        self.work.update_value(|work| {
            work.watcher = Some(watcher);
            work.restarts.started(Instant::now());
        });
        let was_stopped = matches!(self.watch_state.get_untracked(), WatchState::Stopped { .. });
        self.watch_state.set(WatchState::Running);
        if was_stopped {
            self.say(ListingEvent::WatchResumed);
        }

        let missed_dir = dir.clone();
        let missed = blocking(move || report_missed(&missed_dir, &walked, &messages));
        let task = self.spawn(async move {
            missed.await;
            let cause = loop {
                let Some(message) = received.next().await else { break WatchStopCause::Exited };
                if !self.is_current_watch(scan, watch) {
                    return;
                }
                match message {
                    WatchMessage::Stopped(cause) => break cause,
                    WatchMessage::Rescan => {
                        self.relist();
                        return;
                    }
                    WatchMessage::Missed(path) if self.holds(&path) => {}
                    WatchMessage::Missed(path) | WatchMessage::Change(DirChange::Appeared(path)) => {
                        self.read_changed(path, scan, watch).await;
                    }
                    WatchMessage::Change(DirChange::Changed(path)) => self.read_changed(path, scan, watch).await,
                    WatchMessage::Change(DirChange::Removed(path)) => {
                        self.forget(&path);
                        self.say(ListingEvent::Removed(path));
                    }
                }
            };
            if self.is_current_watch(scan, watch) {
                self.watch_stopped(cause);
            }
        });
        self.work.update_value(|work| work.watching = Some(task));
    }

    /// Reads a replay the watch reported, waiting for the game to finish writing its header.
    async fn read_changed(self, path: PathBuf, scan: u64, watch: u64) {
        let mut read = None;
        for attempt in 0..APPEARED_REPLAY_ATTEMPTS {
            if attempt > 0 {
                self.sleep(APPEARED_REPLAY_RETRY).await;
            }
            let reading = path.clone();
            read = blocking(move || read_replay(reading)).await;
            if read.is_some() {
                break;
            }
        }
        let Some(replay) = read else {
            tracing::warn!(path = %path.display(), "a replay the watch reported could not be read");
            return;
        };
        if !self.is_current_watch(scan, watch) {
            return;
        }

        let listed = self.holds(&path);
        self.files.update(|files| match files.iter_mut().find(|file| file.path == path) {
            Some(file) => *file = replay,
            None => files.push(replay),
        });
        if listed {
            self.say(ListingEvent::Changed(path));
        } else {
            self.arrivals.update(|arrivals| {
                arrivals.insert(path.clone());
            });
            self.status.set(ScanStatus::Loaded);
            self.say(ListingEvent::Appeared(path));
        }
        self.report_missing_builds();
    }

    async fn sleep(self, after: Duration) {
        let Some(timers) = self.work.with_value(|work| work.timers.clone()) else { return };
        let (done, waited) = oneshot::channel();
        let _timer = timers.set_timeout(after, move || {
            let _ = done.send(());
        });
        let _ = waited.await;
    }

    /// The watch stopped: schedules a restart while the policy allows one.
    fn watch_stopped(self, cause: WatchStopCause) {
        self.stop_watching();
        let Some(next) = self.work.try_update_value(|work| work.restarts.stopped(Instant::now())) else { return };
        if let NextStep::Retry { delay, .. } = next {
            let timer = self
                .work
                .with_value(|work| work.timers.clone())
                .map(|timers| timers.set_timeout(delay, move || self.relist()));
            self.work.update_value(|work| work.restart = timer);
        }
        self.watch_state.set(WatchState::Stopped { cause: cause.clone(), next });
        self.say(ListingEvent::WatchStopped { cause, next });
    }
}
