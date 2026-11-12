//! Each directory the reader opens is indexed under an index source of its
//! own, as the egui app indexes each directory workspace: its replays stay out
//! of the Live source the install's directory writes, and a search can be
//! limited to them. One directory is indexed at a time, in tab order, off the
//! UI thread.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use futures::StreamExt as _;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_replay_data::paths::folder_name;
use wows_toolkit_replay_data::replay_index::IndexMode;
use wows_toolkit_replay_data::replay_index::IndexProgress;
use wows_toolkit_replay_data::replay_index::ScopeSourceError;
use wows_toolkit_replay_data::replay_index::SourceScope;

use super::*;

/// Where a chosen directory's own source stands.
pub(crate) enum FolderSourceState {
    /// Not looked up: the directory has not been found, or the index is not
    /// open.
    Unresolved,
    Resolving(Task<()>),
    Resolved(SourceId),
    /// It could not be created, so the directory is neither indexed nor
    /// searched.
    Unavailable(Arc<ScopeSourceError>),
}

/// Where a chosen directory's background pass stands.
enum FolderPassState {
    Idle,
    /// Waiting for another directory's pass, or for the game data.
    Queued,
    Running(FolderPass),
}

/// A running pass. Dropping it stops the pass between two replays.
struct FolderPass {
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
}

impl Drop for FolderPass {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A directory tab's own source and the pass that fills it.
pub(crate) struct FolderIndex {
    source: FolderSourceState,
    pass: FolderPassState,
}

impl FolderIndex {
    pub(super) fn new() -> Self {
        Self { source: FolderSourceState::Unresolved, pass: FolderPassState::Idle }
    }

    #[cfg(test)]
    pub(crate) fn source(&self) -> &FolderSourceState {
        &self.source
    }

    /// Whether the directory's replays are still being added to its source.
    pub(crate) fn is_indexing(&self) -> bool {
        matches!(self.pass, FolderPassState::Queued | FolderPassState::Running(_))
    }
}

/// Whether a header action is offered, and why not when it is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Offer {
    Enabled,
    Disabled(String),
}

/// The source a tab's replays are written under.
fn folder_scope(folder: &ReplayFolder) -> SourceScope {
    match folder.source() {
        FolderSource::Install => SourceScope::Live,
        FolderSource::Chosen(root) => SourceScope::Folder(root.clone()),
    }
}

impl ReplayInspectorView {
    /// Looks up or creates a chosen directory's own source, then queues the
    /// pass that adds what the source does not hold yet.
    pub(super) fn index_folder(&mut self, id: FolderId, cx: &mut Context<Self>) {
        self.stop_folder_pass(id, cx);
        let Some(folder) = self.folder(id) else { return };
        let FolderSource::Chosen(root) = folder.source().clone() else { return };
        if folder.state() != FolderState::Listed {
            return;
        }
        if let FolderSourceState::Resolved(_) = folder.index().source {
            self.queue_folder_pass(id, cx);
            return;
        }
        let (Some(pool), Some(runtime)) = (crate::settings_store::pool(cx), crate::runtime::runtime(cx)) else {
            return;
        };

        let task = cx.spawn(async move |this, cx| {
            let ensured = cx
                .background_spawn(async move {
                    let scope = SourceScope::Folder(root.clone());
                    runtime.block_on(scope.ensure(&pool, &root, jiff::Timestamp::now()))
                })
                .await;
            let _ = this.update(cx, |this, cx| this.folder_source_found(id, ensured, cx));
        });
        if let Some(folder) = self.folder_mut(id) {
            folder.index_mut().source = FolderSourceState::Resolving(task);
        }
    }

    /// Queues every listed directory's pass again, for game data that has
    /// just arrived or changed.
    pub(super) fn index_listed_folders(&mut self, cx: &mut Context<Self>) {
        let listed: Vec<FolderId> = self
            .folders
            .iter()
            .filter(|folder| matches!(folder.source(), FolderSource::Chosen(_)))
            .filter(|folder| folder.state() == FolderState::Listed)
            .map(ReplayFolder::id)
            .collect();
        for id in listed {
            self.index_folder(id, cx);
        }
    }

    fn folder_source_found(
        &mut self,
        id: FolderId,
        ensured: Result<SourceId, ScopeSourceError>,
        cx: &mut Context<Self>,
    ) {
        let Some(folder) = self.folder_mut(id) else { return };

        match ensured {
            Ok(source) => {
                folder.index_mut().source = FolderSourceState::Resolved(source);
                self.queue_folder_pass(id, cx);
            }
            Err(err) => {
                tracing::warn!("replay folders: the directory's index source could not be created: {err}");
                folder.index_mut().source = FolderSourceState::Unavailable(Arc::new(err));
            }
        }
        cx.notify();
    }

    fn queue_folder_pass(&mut self, id: FolderId, cx: &mut Context<Self>) {
        let Some(folder) = self.folder_mut(id) else { return };

        folder.index_mut().pass = FolderPassState::Queued;
        self.run_next_folder_pass(cx);
        cx.notify();
    }

    /// Stops a directory's pass, queued or running, and takes it off the
    /// status bar.
    pub(super) fn stop_folder_pass(&mut self, id: FolderId, cx: &mut Context<Self>) {
        let Some(folder) = self.folder_mut(id) else { return };
        let stopped = std::mem::replace(&mut folder.index_mut().pass, FolderPassState::Idle);
        if !matches!(stopped, FolderPassState::Running(_)) {
            return;
        }

        crate::status_bar::finished(folder.browser().entity_id(), crate::status_bar::JobKind::FolderIndex, cx);
        self.run_next_folder_pass(cx);
    }

    /// Starts the first queued directory's pass, unless a pass is running
    /// here or in the app. A queued directory waits for the game data its
    /// replays are read through.
    pub(super) fn run_next_folder_pass(&mut self, cx: &mut Context<Self>) {
        if self.index_pass_running || self.is_indexing_folder() {
            return;
        }
        let Some(game_data) = self.game_data.clone() else { return };
        let (Some(pool), Some(runtime)) = (crate::settings_store::pool(cx), crate::runtime::runtime(cx)) else {
            return;
        };
        let next = self.folders.iter().find_map(|folder| {
            let FolderPassState::Queued = folder.index().pass else { return None };
            let FolderSourceState::Resolved(source) = folder.index().source else { return None };
            let FolderSource::Chosen(root) = folder.source() else { return None };
            Some((folder.id(), source, root.clone(), folder.browser().downgrade()))
        });
        let Some((id, source, root, browser)) = next else { return };

        let said = t!("ui.replay_folders.indexing", dir = folder_name(&root)).into_owned();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let personal_rating = self.personal_rating.clone();
        let unrated = personal_rating.is_none();
        let (ended, pass_ended) = futures::channel::oneshot::channel::<()>();
        let task = cx.spawn(async move |this, cx| {
            let (report, mut steps) = futures::channel::mpsc::unbounded::<IndexProgress>();
            let walking = cx.background_spawn(async move {
                let built = wows_toolkit_replay_data::replay_index::build_index(
                    &runtime,
                    &pool,
                    source,
                    &root,
                    &game_data,
                    personal_rating.as_deref(),
                    &stop,
                    IndexMode::FillGaps,
                    |step| {
                        let _ = report.unbounded_send(step);
                    },
                );
                let _ = ended.send(());

                built
            });
            let owner: AnyWeakEntity = browser.into();
            let follow = async {
                while let Some(step) = steps.next().await {
                    let progress = crate::status_bar::Progress::new(step.done, step.total);
                    let told = this.update(cx, |_this, cx| {
                        let job = crate::status_bar::JobKind::FolderIndex;
                        crate::status_bar::working(owner.clone(), job, said.clone(), progress, cx);
                    });
                    if told.is_err() {
                        break;
                    }
                }
            };
            let (built, ()) = futures::future::join(walking, follow).await;
            let _ = this.update(cx, |this, cx| this.folder_pass_finished(id, built, cx));
        });
        if unrated {
            // A stopped pass drops this task but still finishes the replay it is
            // reading, so its unrated rows are rated once the walk has ended.
            cx.spawn(async move |this, cx| {
                if pass_ended.await.is_ok() {
                    let _ = this.update(cx, |this, cx| this.repair_personal_ratings(cx));
                }
            })
            .detach();
        }
        if let Some(folder) = self.folder_mut(id) {
            folder.index_mut().pass = FolderPassState::Running(FolderPass { cancel, _task: task });
        }
    }

    fn folder_pass_finished(&mut self, id: FolderId, built: Result<IndexProgress, IndexError>, cx: &mut Context<Self>) {
        match built {
            Ok(progress) => tracing::info!(
                indexed = progress.indexed,
                skipped = progress.skipped,
                failed = progress.failed,
                "replay folders: the directory was indexed"
            ),
            Err(err) => tracing::warn!("replay folders: the directory was not indexed: {err}"),
        }
        let Some(folder) = self.folder_mut(id) else { return };
        folder.index_mut().pass = FolderPassState::Idle;
        let browser = folder.browser().clone();

        crate::status_bar::finished(browser.entity_id(), crate::status_bar::JobKind::FolderIndex, cx);
        // The rows read what the source held before the pass.
        browser.update(cx, |browser, cx| browser.load_summaries(cx));
        self.run_next_folder_pass(cx);
        cx.notify();
    }

    /// Whether a chosen directory's pass is reading replays.
    pub(crate) fn is_indexing_folder(&self) -> bool {
        self.folders.iter().any(|folder| matches!(folder.index().pass, FolderPassState::Running(_)))
    }

    /// Whether another index pass, which the app runs, is reading replays.
    /// The directories' own passes wait for it to end.
    pub(crate) fn set_index_pass_running(&mut self, running: bool, cx: &mut Context<Self>) {
        if self.index_pass_running == running {
            return;
        }

        self.index_pass_running = running;
        self.run_next_folder_pass(cx);
        cx.notify();
    }

    /// Whether the selected tab's replays can be searched.
    ///
    /// The install's directory always can: the Live source is looked up when
    /// asked, and the reader is told if nothing has created it.
    pub(super) fn search_offer(&self) -> Offer {
        let Some(folder) = self.folder(self.selected_folder) else { return Offer::Enabled };
        if folder.source() == &FolderSource::Install {
            return Offer::Enabled;
        }

        match &folder.index().source {
            FolderSourceState::Resolved(_) => Offer::Enabled,
            FolderSourceState::Unavailable(err) => {
                Offer::Disabled(t!("ui.replay_folders.source_failed", error = err.to_string()).into_owned())
            }
            FolderSourceState::Unresolved | FolderSourceState::Resolving(_) => {
                Offer::Disabled(t!("ui.tabs.search_these_replays_unavailable").into_owned())
            }
        }
    }

    /// Whether the selected tab's list can be indexed now.
    pub(super) fn index_offer(&self) -> Offer {
        if self.index_pass_running {
            return Offer::Disabled(t!("ui.reindex.busy_index").into_owned());
        }
        match &self.game_data_status {
            GameDataStatus::Ready(_) => {}
            GameDataStatus::Loading { .. } => {
                return Offer::Disabled(t!("ui.stats.directory_stats_unavailable").into_owned());
            }
            GameDataStatus::Failed(reason) => {
                return Offer::Disabled(t!("ui.replay.game_data_failed", error = reason).into_owned());
            }
        }
        let Some(folder) = self.folder(self.selected_folder) else { return Offer::Enabled };
        if let FolderSourceState::Unavailable(err) = &folder.index().source {
            return Offer::Disabled(t!("ui.replay_folders.source_failed", error = err.to_string()).into_owned());
        }
        if folder.index().is_indexing() {
            return Offer::Disabled(t!("ui.replay_folders.indexing_in_background").into_owned());
        }
        if self.is_indexing_folder() {
            return Offer::Disabled(t!("ui.reindex.busy_index").into_owned());
        }

        Offer::Enabled
    }

    /// Opens the Search tab over the selected tab's own source.
    ///
    /// A directory still being indexed is searched all the same: what its
    /// source holds is correct as far as it goes, and holding the search back
    /// would hide it. The reader is told the results are not complete yet.
    pub(super) fn search_selected_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.folder(self.selected_folder) else { return };
        let scope = folder_scope(folder);
        let indexing = folder.index().is_indexing();

        cx.emit(SearchFolder(scope));
        if indexing {
            crate::toast::info(t!("ui.replay_folders.search_while_indexing").into_owned(), window, cx);
        }
    }

    /// Asks for the selected tab's listed replays that its source does not
    /// hold yet to be indexed.
    pub(super) fn index_selected_folder(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.folder(self.selected_folder) else { return };
        let scope = folder_scope(folder);
        let candidates = folder.browser().read(cx).reindex_candidates();

        cx.emit(IndexFolderRequested { scope, candidates });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;

    use gpui_kit::AppContext as _;
    use gpui_kit::Context;
    use gpui_kit::Entity;
    use gpui_kit::TestAppContext;
    use gpui_kit::Window;
    use gpui_kit::WindowHandle;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt as _;
    use rust_i18n::t;
    use wows_replays::types::ArenaId;
    use wows_toolkit_config::index::query;
    use wows_toolkit_config::index::rows::MatchOutcome;
    use wows_toolkit_config::index::rows::ObjectiveMatch;
    use wows_toolkit_config::index::rows::ReplayRecord;
    use wows_toolkit_config::index::rows::SourceId;
    use wows_toolkit_config::index::rows::SourceKind;
    use wows_toolkit_config::index::unindexable::Unindexable;
    use wows_toolkit_replay_data::replay_index::ScopeSourceError;
    use wows_toolkit_replay_data::replay_index::SourceScope;
    use wows_toolkit_viewmodel::listing_row::ListedReplay;

    use super::super::SearchFolder;
    use super::FolderSourceState;
    use super::Offer;
    use crate::app::App;
    use crate::interaction_tests::open_app_in_root;
    use crate::interaction_tests::open_index;
    use crate::replay_inspector::ReplayInspectorView;
    use crate::replay_inspector::browser_view::DirChange;
    use crate::replay_inspector::simulated_watch;

    const SEARCH: &str = "replay-listing-search";
    const INDEX: &str = "replay-listing-index";

    /// A real replay, so a reported change is read as the game's would be.
    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../wows-replay-insights/tests/fixtures/replays/10000_1736020571_20250101_215420_PVSB018-Ipiranga_41_Conquest.wowsreplay",
        )
    }

    /// Marks the fixture's build as having its result mapping, so a listing
    /// that holds it asks no repository for one.
    fn hold_constants_for_fixture() {
        let meta = wows_replays::ReplayFile::meta_from_file(&fixture()).expect("the fixture is a replay");
        let build = ListedReplay::from_meta(&meta).build.expect("the fixture names its build");
        let path = crate::constants::cached_path(build).expect("tests have a storage directory");
        if !path.exists() {
            std::fs::write(path, "{}").expect("the mapping is written");
        }
    }

    fn canonical(dir: &Path) -> PathBuf {
        std::fs::canonicalize(dir).expect("the directory exists")
    }

    fn inspector(app: &Entity<App>, cx: &mut TestAppContext) -> Entity<ReplayInspectorView> {
        cx.update(|cx| app.read(cx).replay_inspector().clone())
    }

    fn runtime(cx: &mut TestAppContext) -> Arc<tokio::runtime::Runtime> {
        cx.update(|cx| crate::runtime::runtime(cx)).expect("the runtime is installed")
    }

    fn with_view(
        window: WindowHandle<Root>,
        view: &Entity<ReplayInspectorView>,
        cx: &mut TestAppContext,
        act: impl FnOnce(&mut ReplayInspectorView, &mut Window, &mut Context<ReplayInspectorView>),
    ) {
        cx.update_window(window.into(), |_, window, cx| view.update(cx, |view, cx| act(view, window, cx)))
            .expect("the test window stays open");
        cx.run_until_parked();
    }

    fn settle(window: WindowHandle<Root>, cx: &mut TestAppContext) {
        for _ in 0..3 {
            cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
                .expect("the test window stays open");
            cx.run_until_parked();
        }
    }

    fn click(window: WindowHandle<Root>, id: &'static str, cx: &mut TestAppContext) {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .expect("the test window stays open");
        settle(window, cx);
    }

    /// The source rooted at `root`, with its kind.
    fn source_at(pool: &sqlx::SqlitePool, root: &Path, cx: &mut TestAppContext) -> Option<(SourceId, SourceKind)> {
        let sources = runtime(cx).block_on(query::list_sources(pool)).expect("the sources read");
        sources
            .into_iter()
            .find(|source| source.root_path.as_deref() == Some(root))
            .map(|source| (source.id, source.kind))
    }

    fn live_source(pool: &sqlx::SqlitePool, cx: &mut TestAppContext) -> Option<SourceId> {
        runtime(cx).block_on(query::live_source_id(pool)).expect("the live source reads")
    }

    /// The replay inspector alone in a window. The app's Search tab is left
    /// out: running a search saves it through a tokio task, which the test
    /// scheduler refuses.
    fn open_inspector(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<ReplayInspectorView>) {
        cx.update(gpui_kit::init);
        let view = std::cell::RefCell::new(None);
        let window = cx.open_window(gpui_kit::size(gpui_kit::px(1200.), gpui_kit::px(800.)), |window, cx| {
            let inspector = cx.new(|cx| ReplayInspectorView::new(window, cx));
            *view.borrow_mut() = Some(inspector.clone());
            Root::new(inspector, window, cx)
        });
        let view = view.borrow_mut().take().expect("the view was built inside the window");

        (window, view)
    }

    /// Records the scope of every search the view asks for.
    fn record_searches(
        view: &Entity<ReplayInspectorView>,
        cx: &mut TestAppContext,
    ) -> (Rc<RefCell<Vec<SourceScope>>>, gpui_kit::Subscription) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorder = Rc::clone(&seen);
        let subscription = cx.update(|cx| {
            cx.subscribe(view, move |_view, event: &SearchFolder, _cx| recorder.borrow_mut().push(event.0.clone()))
        });

        (seen, subscription)
    }

    /// The source a search over `scope` runs over, as the app resolves it.
    fn resolved(scope: &SourceScope, pool: &sqlx::SqlitePool, cx: &mut TestAppContext) -> Option<SourceId> {
        runtime(cx).block_on(scope.existing(pool)).expect("the sources read")
    }

    /// Opens a directory as the reader does, and waits for its own source.
    fn open_folder(
        window: WindowHandle<Root>,
        view: &Entity<ReplayInspectorView>,
        root: &Path,
        cx: &mut TestAppContext,
    ) {
        let root = root.to_path_buf();
        with_view(window, view, cx, move |view, window, cx| view.open_folder(root, window, cx));
        settle(window, cx);
    }

    /// Search on the install's tab asks for the Live source and no other,
    /// though another source is indexed too.
    #[gpui_kit::test]
    fn search_on_the_install_tab_scopes_to_the_live_source(cx: &mut TestAppContext) {
        let (window, view) = open_inspector(cx);
        let pool = open_index(cx);
        let now = jiff::Timestamp::now();
        let live = runtime(cx)
            .block_on(query::ensure_default_source(&pool, Path::new("G:/wows/replays"), now))
            .expect("the live source is made");
        runtime(cx)
            .block_on(query::ensure_source(&pool, "Other", SourceKind::ImportedDir, Path::new("D:/other"), now))
            .expect("another source is made");
        let (seen, _subscription) = record_searches(&view, cx);

        click(window, SEARCH, cx);

        let asked = seen.borrow().clone();
        assert_eq!(asked, [SourceScope::Live]);
        assert_eq!(resolved(&asked[0], &pool, cx), Some(live));
    }

    /// Opening a directory gives it a source of its own, and Search on its
    /// tab asks for that source rather than the Live one.
    #[gpui_kit::test]
    fn search_on_a_chosen_folder_scopes_to_its_own_source(cx: &mut TestAppContext) {
        let (window, view) = open_inspector(cx);
        let pool = open_index(cx);
        let live = runtime(cx)
            .block_on(query::ensure_default_source(&pool, Path::new("G:/wows/replays"), jiff::Timestamp::now()))
            .expect("the live source is made");
        let dir = tempfile::tempdir().expect("a temp directory");
        let root = canonical(dir.path());
        let (seen, _subscription) = record_searches(&view, cx);

        open_folder(window, &view, &root, cx);

        let (own, kind) = source_at(&pool, &root, cx).expect("the directory has a source");
        assert_eq!(kind, SourceKind::ImportedDir);
        assert_ne!(own, live, "the directory's source is not the Live one");
        let held = cx.update(|cx| {
            let view = view.read(cx);
            let folder = view.folder(view.selected_folder).expect("the directory is selected");
            match folder.index().source() {
                FolderSourceState::Resolved(source) => Some(*source),
                _ => None,
            }
        });
        assert_eq!(held, Some(own), "the tab holds its source");

        click(window, SEARCH, cx);

        let asked = seen.borrow().clone();
        assert_eq!(asked, [SourceScope::Folder(root.clone())]);
        assert_eq!(resolved(&asked[0], &pool, cx), Some(own));
    }

    /// A directory whose source cannot be created offers no search, and says
    /// why on the button.
    #[gpui_kit::test]
    fn a_folder_without_a_source_offers_no_search(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        let pool = open_index(cx);
        let dir = tempfile::tempdir().expect("a temp directory");
        let root = canonical(dir.path());
        let view = inspector(&app, cx);
        // A closed pool refuses the insert the source is created by.
        runtime(cx).block_on(pool.close());

        open_folder(window, &view, &root, cx);

        let offer = cx.update(|cx| view.read(cx).search_offer());
        let reason = cx.update(|cx| {
            let view = view.read(cx);
            let folder = view.folder(view.selected_folder).expect("the directory is selected");
            match folder.index().source() {
                FolderSourceState::Unavailable(err) => {
                    t!("ui.replay_folders.source_failed", error = err.to_string()).into_owned()
                }
                _ => panic!("the source could not be created"),
            }
        });
        assert_eq!(offer, Offer::Disabled(reason));
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(SEARCH).disabled(), Some(true));
        })
        .expect("the test window stays open");
    }

    /// A directory the Live source is rooted at, such as an earlier
    /// install's replays directory, is not written into or searched as a
    /// chosen folder: the Live source would take its replays.
    #[gpui_kit::test]
    fn a_folder_at_the_live_root_is_kept_out_of_the_live_source(cx: &mut TestAppContext) {
        let (window, view) = open_inspector(cx);
        let pool = open_index(cx);
        let dir = tempfile::tempdir().expect("a temp directory");
        let root = canonical(dir.path());
        runtime(cx)
            .block_on(query::ensure_default_source(&pool, &root, jiff::Timestamp::now()))
            .expect("the live source is made");

        open_folder(window, &view, &root, cx);

        let refused = cx.update(|cx| {
            let view = view.read(cx);
            let folder = view.folder(view.selected_folder).expect("the directory is selected");
            matches!(
                folder.index().source(),
                FolderSourceState::Unavailable(err) if matches!(**err, ScopeSourceError::LiveRoot { .. })
            )
        });
        assert!(refused, "the directory has no source of its own");
        assert!(matches!(cx.update(|cx| view.read(cx).search_offer()), Offer::Disabled(_)));
        assert_eq!(resolved(&SourceScope::Folder(root), &pool, cx), None, "nor does a search find the Live one");
    }

    /// A battle landing in a pinned directory is written under that
    /// directory's source; one landing in the install's is what the Live
    /// source is made for.
    #[gpui_kit::test]
    fn a_replay_landing_in_a_pinned_folder_is_indexed_under_its_own_source(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        let pool = open_index(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let install = tempfile::tempdir().expect("a temp directory");
        let main_dir = install.path().join("replays");
        std::fs::create_dir(&main_dir).expect("the replays directory is made");
        let pinned = tempfile::tempdir().expect("a temp directory");
        let pinned = canonical(pinned.path());
        let view = inspector(&app, cx);
        let wows_dir = install.path().display().to_string();
        with_view(window, &view, cx, |view, _window, cx| {
            view.seed_game_data(&wows_dir);
            view.scan_install(wows_dir.clone(), cx);
        });
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(super::super::folders::PinnedFolders(vec![pinned.clone()]), window, cx);
        });
        settle(window, cx);

        let landed = pinned.join("landed.wowsreplay");
        std::fs::copy(fixture(), &landed).expect("the replay is copied");
        simulated_watch::report(&pinned, DirChange::Appeared(landed));
        settle(window, cx);

        let (_, kind) = source_at(&pool, &pinned, cx).expect("the pinned directory has its own source");
        assert_eq!(kind, SourceKind::ImportedDir);
        assert_eq!(live_source(&pool, cx), None, "the battle did not go to the Live source");

        let played = main_dir.join("played.wowsreplay");
        std::fs::copy(fixture(), &played).expect("the replay is copied");
        simulated_watch::report(&main_dir, DirChange::Appeared(played));
        settle(window, cx);

        assert!(live_source(&pool, cx).is_some(), "a battle in the install's directory goes to the Live source");
    }

    /// The index button says why it is not offered: game data still loading,
    /// or another pass running.
    #[gpui_kit::test]
    fn the_index_button_says_why_it_is_not_offered(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        let view = inspector(&app, cx);

        let loading = cx.update(|cx| view.read(cx).index_offer());
        assert_eq!(loading, Offer::Disabled(t!("ui.stats.directory_stats_unavailable").into_owned()));
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(INDEX).disabled(), Some(true));
        })
        .expect("the test window stays open");

        view.update(cx, |view, cx| view.set_index_pass_running(true, cx));
        let busy = cx.update(|cx| view.read(cx).index_offer());
        assert_eq!(busy, Offer::Disabled(t!("ui.reindex.busy_index").into_owned()));
    }

    /// Indexing a directory's list asks first, saying how many replays its
    /// source lacks and where they land, then reads exactly those into the
    /// directory's own source.
    ///
    /// The files are not real replays, so each read fails and lands in the
    /// unreadable-file ledger, which is what shows which files were read. They
    /// are not named as replays, so the directory's own background pass walks
    /// past them.
    #[gpui_kit::test]
    fn indexing_a_folder_asks_first_and_reads_only_what_its_source_lacks(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        let pool = open_index(cx);
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        let wows = dir.path().join("wows");
        std::fs::create_dir_all(wows.join("bin").join("100")).expect("the build directory can be made");
        let mut settings = crate::interaction_tests::test_settings();
        settings.wows_dir = wows.to_string_lossy().into_owned();
        app.update(cx, |app, _cx| app.seed_settings_for_test(settings));
        let archive = dir.path().join("archive");
        std::fs::create_dir_all(&archive).expect("the directory can be made");
        let root = canonical(&archive);
        let [a, b, c] = ["a", "b", "c"].map(|name| {
            let path = root.join(format!("{name}.bin"));
            std::fs::write(&path, b"not a replay").expect("the file can be written");
            path
        });
        let view = inspector(&app, cx);
        view.update(cx, |view, _cx| view.seed_game_data(&wows.to_string_lossy()));
        open_folder(window, &view, &root, cx);

        let (own, _) = source_at(&pool, &root, cx).expect("the directory has a source");
        runtime(cx).block_on(async {
            let indexed = ObjectiveMatch {
                arena_id: ArenaId::new(100),
                timestamp: jiff::Timestamp::now(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_mode_id: None,
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(100),
            };
            query::upsert_match(&pool, &indexed).await.expect("the match inserts");
            let record = ReplayRecord {
                arena_id: ArenaId::new(100),
                source_id: own,
                replay_path: a.clone(),
                file_mtime: None,
                outcome: MatchOutcome::Win,
                self_account_id: None,
                self_ship_id: None,
                self_survived: None,
                self_damage: None,
                self_kills: None,
                self_pr: None,
                results_available: true,
                indexed_at: jiff::Timestamp::now(),
            };
            query::upsert_record(&pool, &record).await.expect("the record inserts");
        });
        let browser =
            cx.update(|cx| view.read(cx).folder_browser_for_test(&root).expect("the directory has a tab").clone());
        browser.update(cx, |browser, cx| {
            for (path, played) in
                [(&a, "01.01.2026 10:00:00"), (&b, "02.01.2026 10:00:00"), (&c, "03.01.2026 10:00:00")]
            {
                browser.seed_release_for_test(path.clone(), "1.0.0", 100, played, cx);
            }
        });

        with_view(window, &view, cx, |view, _window, cx| view.index_selected_folder(cx));
        settle(window, cx);

        let expected = [
            t!("ui.index_folder.confirm_count", count = 2).into_owned(),
            t!("ui.reindex.confirm_range", oldest = "2026-01-02 10:00", newest = "2026-01-03 10:00").into_owned(),
            t!("ui.index_folder.note_folder", dir = wows_toolkit_replay_data::paths::shown_path(&root)).into_owned(),
        ]
        .join("\n");
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("force-reindex-summary").label(), Some(expected.as_str()));
            window.click("ok", cx);
        })
        .expect("the test window stays open");
        let before = crate::toast::said_for_test().len();
        settle(window, cx);

        let ledger = runtime(cx).block_on(Unindexable::load(&pool));
        assert!(ledger.contains(&b) && ledger.contains(&c), "both replays the source lacks were read");
        assert!(!ledger.contains(&a), "the one it holds was not");
        assert_eq!(live_source(&pool, cx), None, "nothing was written under the Live source");
        let said = crate::toast::said_for_test().split_off(before);
        assert!(
            said.contains(&t!("ui.index_folder.done", indexed = 0, failed = 2, skipped = 0).into_owned()),
            "the end of the pass is announced with its counts, said {said:?}"
        );
    }
}
