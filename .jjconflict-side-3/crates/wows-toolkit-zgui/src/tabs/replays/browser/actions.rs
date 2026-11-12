//! Work over the listed replays: each chosen directory's own index source and the pass that fills
//! it, forced re-indexing, contributing, and the palette commands that start them.
//!
//! One pass reads replays at a time: two would each load and save the unreadable-file ledger over
//! the other's. The directories' own passes run one after another, in tab order, and wait for a
//! forced re-index to end.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use futures::StreamExt as _;
use futures::channel::mpsc;
use landaire_ui::Notice;
use rust_i18n::t;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_toolkit_config::index::query::record_paths_in_source;
use wows_toolkit_config::index::query_text::print_query;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::IndexWriteMode;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_replay_data::force_reindex;
use wows_toolkit_replay_data::force_reindex::BuildNeed;
use wows_toolkit_replay_data::force_reindex::Confirm;
use wows_toolkit_replay_data::force_reindex::Fetchable;
use wows_toolkit_replay_data::force_reindex::IndexPurpose;
use wows_toolkit_replay_data::force_reindex::ReindexCandidate;
use wows_toolkit_replay_data::force_reindex::ReindexScope;
use wows_toolkit_replay_data::force_reindex::ReindexSummary;
use wows_toolkit_replay_data::force_reindex::Release;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::parse_replay;
use wows_toolkit_replay_data::paths::folder_name;
use wows_toolkit_replay_data::replay_index::IndexMode;
use wows_toolkit_replay_data::replay_index::IndexOneError;
use wows_toolkit_replay_data::replay_index::IndexProgress;
use wows_toolkit_replay_data::replay_index::ScopeSourceError;
use wows_toolkit_replay_data::replay_index::ScopedReplay;
use wows_toolkit_replay_data::replay_index::SourceScope;
use wows_toolkit_replay_data::replay_index::build_index;
use wows_toolkit_replay_data::replay_index::index_one;
use wows_toolkit_replay_data::replay_index::reindex_replays;
use wows_toolkit_services::game_data_cache;
use wows_toolkit_services::game_data_cache::CacheOutcome;
use wows_toolkit_services::upload;
use wows_toolkit_services::upload::Candidate;
use wows_toolkit_services::upload::LedgerUse;
use wows_toolkit_services::upload::Outcome;
use wows_toolkit_services::upload::Sender;
use wows_toolkit_viewmodel::palette::PaletteMode;
use wows_toolkit_viewmodel::query_bar::seed;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::Task;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Track;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::install_stylesheet;
use zgui_ui::AlertDialogActionProps;
use zgui_ui::AlertDialogCancelProps;
use zgui_ui::AlertDialogContentProps;
use zgui_ui::AlertDialogFooterProps;
use zgui_ui::AlertDialogHeaderProps;
use zgui_ui::AlertDialogProps;
use zgui_ui::AlertDialogTitleProps;

use crate::services::GameDataState;
use crate::services::Job;
use crate::services::Progress;
use crate::services::Services;
use crate::services::TokioError;
use crate::shell::palette::PaletteRun;
use crate::shell::palette::RootCommand;
use crate::shell::palette::TabRequest;
use crate::shell::palette::chosen;
use crate::shell::palette::modes;
use crate::shell::replay_index::owner_rating;
use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;
use crate::tabs::replays::browser::BrowserEvents;
use crate::tabs::replays::browser::ReplayBrowserEvent;
use crate::tabs::replays::browser::folders::BrowserControl;
use crate::tabs::replays::browser::folders::FolderId;
use crate::tabs::replays::browser::folders::FolderSource;
use crate::tabs::replays::browser::folders::FolderState;

const SHEET: &str = "wt-replay-reindex";

const CSS: &str = css!(
    ".wt-reindex-summary {
        display: flex;
        flex-direction: column;
        gap: 4px;
        font-size: 14px;
    }"
);

/// Whether a header action is offered, and why not when it is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offer {
    Enabled,
    /// Carries the reason, already translated.
    Disabled(String),
}

/// Why a chosen directory has no index source of its own.
#[derive(Debug, thiserror::Error)]
pub enum SourceUnavailable {
    #[error(transparent)]
    Scope(#[from] ScopeSourceError),
    #[error(transparent)]
    Runtime(#[from] TokioError),
}

/// Where a chosen directory's own index source stands. A directory with none is not looked up
/// yet.
#[derive(Clone, Debug)]
pub enum FolderSourceState {
    /// Being looked up or created for the directory at this path.
    Resolving(PathBuf),
    Resolved {
        root: PathBuf,
        source: SourceId,
    },
    /// It could not be created, so the directory is neither indexed nor searched.
    Unavailable(Rc<SourceUnavailable>),
}

/// Where a chosen directory's own pass stands, while it has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PassState {
    /// Waiting for another pass, or for the game data.
    Queued,
    Running,
}

/// The running directory pass. It holds the pass slot until its worker returns, stopped or not,
/// so two passes never write at once.
struct FolderPass {
    cancel: Arc<AtomicBool>,
    /// Kept so the pass ends with the replays tab.
    _task: Task,
}

impl FolderPass {
    /// Asks the pass to stop after the replay it is reading.
    fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for FolderPass {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The browser's background work, as a `Copy` handle the [`BrowserControl`] holds.
#[derive(Clone, Copy)]
pub struct BrowserWork {
    sources: RwSignal<HashMap<FolderId, FolderSourceState>, LocalStorage>,
    passes: RwSignal<HashMap<FolderId, PassState>, LocalStorage>,
    running: StoredValue<Option<FolderPass>, LocalStorage>,
    /// A forced re-index is reading replays, or fetching the game data it reads them through.
    reindexing: RwSignal<bool, LocalStorage>,
    contributing: RwSignal<bool, LocalStorage>,
    /// Where the browser's events go, once the panel is up.
    events: StoredValue<Option<BrowserEvents>, LocalStorage>,
    owner: StoredValue<Owner, LocalStorage>,
}

impl BrowserWork {
    /// Called from the replays tab's body, with the control.
    pub fn new() -> Self {
        Self {
            sources: RwSignal::new_local(HashMap::new()),
            passes: RwSignal::new_local(HashMap::new()),
            running: StoredValue::new_local(None),
            reindexing: RwSignal::new_local(false),
            contributing: RwSignal::new_local(false),
            events: StoredValue::new_local(None),
            owner: StoredValue::new_local(Owner::current().expect("the browser's work is made inside a component")),
        }
    }

    /// Runs `future` under the replays tab, so it stops with it even when started from a
    /// listener.
    fn spawn(self, future: impl Future<Output = ()> + 'static) -> Task {
        let owner = self.owner.get_value();
        owner.with(|| spawn_local(future))
    }

    /// Whether a pass is reading replays: a forced re-index or a directory's own.
    fn is_busy(self) -> bool {
        self.reindexing.get_untracked() || self.running.with_value(Option::is_some)
    }

    /// The source state of tab `id`, subscribing to it.
    pub fn source(self, id: FolderId) -> Option<FolderSourceState> {
        self.sources.with(|sources| sources.get(&id).cloned())
    }

    /// Whether tab `id`'s replays are still being added to its source, subscribing to it.
    pub fn is_indexing(self, id: FolderId) -> bool {
        self.passes.with(|passes| passes.contains_key(&id))
    }
}

impl Default for BrowserWork {
    fn default() -> Self {
        Self::new()
    }
}

/// Hands the work the panel's events, for what it reports once it is done.
pub fn adopt_events(control: BrowserControl, events: BrowserEvents) {
    control.work.events.set_value(Some(events));
}

/// Offers the listing's commands on the palette for as long as the browser is up, and indexes
/// the listed directories again whenever the game data they are read through changes.
pub fn offer(control: BrowserControl) {
    let shell = control.shell();
    let palette = shell.palette;

    let versions = Signal::derive_local(move || force_reindex::versions(&kept_candidates(control)));
    let reindex = move |scope: ReindexScope| PaletteRun::action(move || force_reindex_scope(control, scope.clone()));
    palette.offer_mode(PaletteMode::ReindexVersions, modes::reindex_versions(versions, reindex));
    palette.offer(RootCommand::ReindexAll, PaletteRun::action(move || force_reindex_scope(control, ReindexScope::All)));
    palette.offer(
        RootCommand::ImportConstants,
        PaletteRun::action(move || {
            shell.select(TabKey::App(AppTab::ReplayInspector));
            import_constants(control);
        }),
    );
    for (command, ledger) in
        [(RootCommand::SendAllReplays, LedgerUse::Consult), (RootCommand::SendAllReplaysAgain, LedgerUse::Ignore)]
    {
        palette.offer(
            command,
            PaletteRun::action(move || {
                shell.select(TabKey::App(AppTab::ReplayInspector));
                contribute_all(control, ledger);
            }),
        );
    }

    let services = shell.services();
    let work = control.work;
    let following = RenderEffect::new(move |_| {
        if services.game_data.cache().is_some() {
            // In a task, so the passes this starts subscribe nothing here. The settings that came
            // with the cache are adopted by then too.
            work.spawn(async move { index_listed_folders(control) });
        }
    });
    on_cleanup_local(move || {
        drop(following);
        if let Some(pass) = work.running.try_update_value(Option::take).flatten() {
            pass.stop();
        }
    });
}

/// Every replay a palette command re-indexes, each once: the install's directory and the pinned
/// ones, each replay under its own directory's source. A directory opened once is left out, as a
/// palette command covers the directories the reader keeps. Subscribes to the tabs and their
/// files.
pub fn kept_candidates(control: BrowserControl) -> Vec<ReindexCandidate> {
    let kept: Vec<FolderId> = control.folders.with(|folders| {
        folders.iter().filter(|tab| tab.source == FolderSource::Install || tab.pinned).map(|tab| tab.id).collect()
    });
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut candidates = Vec::new();
    for id in kept {
        let Some(listing) = control.listings.with(|listings| listings.get(&id).copied()) else { continue };
        listing.files.track();
        candidates
            .extend(listing.reindex_candidates().into_iter().filter(|candidate| seen.insert(candidate.path.clone())));
    }

    candidates
}

/// Forces a re-index of the kept replays `scope` names, after asking.
fn force_reindex_scope(control: BrowserControl, scope: ReindexScope) {
    let candidates = scope.select(kept_candidates(control));
    prepare(control, candidates, Confirm::Always, IndexPurpose::Refresh);
}

/// Reads `candidates` again and rewrites their index entries, once confirmed as `confirm` asks.
pub fn force_reindex(control: BrowserControl, candidates: Vec<ReindexCandidate>, confirm: Confirm) {
    prepare(control, candidates, confirm, IndexPurpose::Refresh);
}

/// Whether the selected tab's replays can be searched, subscribing to what decides it.
///
/// The install's directory always can: the Live source is looked up when asked, and the reader is
/// told if nothing has created it.
pub fn search_offer(control: BrowserControl) -> Offer {
    let selected = control.selected.get();
    let install = control.folders.with(|folders| {
        folders.iter().find(|tab| tab.id == selected).is_none_or(|tab| tab.source == FolderSource::Install)
    });
    if install {
        return Offer::Enabled;
    }

    match control.work.source(selected) {
        Some(FolderSourceState::Resolved { .. }) => Offer::Enabled,
        Some(FolderSourceState::Unavailable(err)) => {
            Offer::Disabled(t!("ui.replay_folders.source_failed", error = err.to_string()).into_owned())
        }
        Some(FolderSourceState::Resolving(_)) | None => {
            Offer::Disabled(t!("ui.tabs.search_these_replays_unavailable").into_owned())
        }
    }
}

/// Whether the selected tab's list can be indexed now, subscribing to what decides it.
pub fn index_offer(control: BrowserControl) -> Offer {
    let work = control.work;
    if work.reindexing.get() {
        return Offer::Disabled(t!("ui.reindex.busy_index").into_owned());
    }
    match control.shell().services().game_data.state() {
        GameDataState::Ready(_) => {}
        GameDataState::Loading { .. } => {
            return Offer::Disabled(t!("ui.stats.directory_stats_unavailable").into_owned());
        }
        GameDataState::Failed(reason) => {
            return Offer::Disabled(t!("ui.replay.game_data_failed", error = reason).into_owned());
        }
        GameDataState::NoInstall => {
            let reason = t!("ui.messages.wows_dir_not_set");
            return Offer::Disabled(t!("ui.replay.game_data_failed", error = reason).into_owned());
        }
    }

    let selected = control.selected.get();
    if let Some(FolderSourceState::Unavailable(err)) = work.source(selected) {
        return Offer::Disabled(t!("ui.replay_folders.source_failed", error = err.to_string()).into_owned());
    }
    if work.is_indexing(selected) {
        return Offer::Disabled(t!("ui.replay_folders.indexing_in_background").into_owned());
    }
    if work.passes.with(|passes| passes.values().any(|pass| *pass == PassState::Running)) {
        return Offer::Disabled(t!("ui.reindex.busy_index").into_owned());
    }

    Offer::Enabled
}

/// Opens the Search tab over the selected tab's own source.
///
/// A directory still being indexed is searched all the same: what its source holds is correct as
/// far as it goes. The reader is told the results are not complete yet.
pub fn search_selected_folder(control: BrowserControl) {
    let Some(listing) = control.selected_listing() else { return };
    let shell = control.shell();
    let services = shell.services();
    let Some(pool) = services.settings.pool() else { return };
    let indexing = control.work.passes.with_untracked(|passes| passes.contains_key(&control.selected.get_untracked()));
    let scope = listing.index_scope();

    let found = services.on_tokio(async move { scope.existing(&pool).await });
    control.work.spawn(async move {
        let toasts = shell.toasts();
        match found.await {
            Ok(Ok(Some(source))) => {
                let text = print_query(&seed::source_scoped(source));
                shell.palette.send(shell, TabRequest::Search(text));
                if indexing {
                    toasts.info(t!("ui.replay_folders.search_while_indexing").into_owned());
                }
            }
            Ok(Ok(None)) => {
                toasts.warn(t!("ui.tabs.search_these_replays_unavailable").into_owned());
            }
            Ok(Err(err)) => search_unreadable(shell.toasts(), &err),
            Err(err) => search_unreadable(shell.toasts(), &err),
        }
    });
}

fn search_unreadable(toasts: landaire_ui::Toasts, err: &dyn std::error::Error) {
    tracing::warn!("search: the folder's index source could not be read: {err}");
    toasts.failed(t!("ui.index_folder.unreadable_index").into_owned());
}

/// Adds the selected tab's listed replays that its source does not hold yet, after asking. A
/// source nothing has created yet holds none of them.
pub fn index_selected_folder(control: BrowserControl) {
    let shell = control.shell();
    let services = shell.services();
    let toasts = shell.toasts();
    if control.work.is_busy() {
        toasts.warn(t!("ui.reindex.busy_index").into_owned());
        return;
    }
    let Some(listing) = control.selected_listing() else { return };
    let candidates = listing.reindex_candidates();
    if candidates.is_empty() {
        toasts.info(t!("ui.index_folder.nothing_listed").into_owned());
        return;
    }
    let Some(pool) = services.settings.pool() else {
        toasts.warn(t!("ui.reindex.no_index").into_owned());
        return;
    };

    let scope = listing.index_scope();
    let reading = scope.clone();
    let held = services.on_tokio(async move {
        match reading.existing(&pool).await? {
            Some(source) => record_paths_in_source(&pool, source).await,
            None => Ok::<_, IndexError>(HashSet::new()),
        }
    });
    control.work.spawn(async move {
        let held = match held.await {
            Ok(Ok(held)) => held,
            Ok(Err(err)) => return index_unreadable(shell.toasts(), &err),
            Err(err) => return index_unreadable(shell.toasts(), &err),
        };
        let missing: Vec<ReindexCandidate> = candidates
            .into_iter()
            .filter(|candidate| !held.contains(candidate.path.to_string_lossy().as_ref()))
            .collect();
        if missing.is_empty() {
            shell.toasts().info(t!("ui.index_folder.nothing_missing").into_owned());
            return;
        }

        prepare(control, missing, Confirm::Always, IndexPurpose::Fill(scope));
    });
}

fn index_unreadable(toasts: landaire_ui::Toasts, err: &dyn std::error::Error) {
    tracing::warn!("folder index: the indexed replays could not be read: {err}");
    toasts.failed(t!("ui.index_folder.unreadable_index").into_owned());
}

/// The install's replays directory, which the Live source is rooted at when a pass is the first to
/// create it. `None` while no game directory is set.
fn install_replays_root(services: Services) -> Option<PathBuf> {
    let wows_dir = services.settings.read_untracked(|settings| settings.wows_dir.clone())?;
    if wows_dir.is_empty() {
        return None;
    }

    Some(Path::new(&wows_dir).join("replays"))
}

/// Where downloaded game data goes. `None` when there is no storage directory to fall back on.
pub(crate) fn cache_base(services: Services) -> Option<PathBuf> {
    let cache_dir = services.settings.read_untracked(|settings| settings.game_data_cache_dir.clone())?;
    game_data_cache::base(&cache_dir)
}

pub(crate) fn proxy_url(services: Services) -> String {
    // No proxy until the settings are read, which is the setting's own default.
    services.settings.read_untracked(|settings| settings.proxy_url.clone()).unwrap_or_default()
}

/// Works out what reading `candidates` for `purpose` takes, asks when `confirm` says to, and runs
/// it.
fn prepare(control: BrowserControl, candidates: Vec<ReindexCandidate>, confirm: Confirm, purpose: IndexPurpose) {
    let shell = control.shell();
    let services = shell.services();
    let toasts = shell.toasts();
    if control.work.is_busy() {
        toasts.warn(t!("ui.reindex.busy_index").into_owned());
        return;
    }
    if candidates.is_empty() {
        toasts.info(t!("ui.reindex.nothing_listed").into_owned());
        return;
    }
    let Some(game_data) = services.game_data.cache_untracked() else {
        toasts.warn(t!("ui.reindex.no_game_data").into_owned());
        return;
    };
    if install_replays_root(services).is_none() {
        toasts.warn(t!("ui.reindex.no_game_data").into_owned());
        return;
    }

    // A directory listing and an index file read per release, so off the UI thread. A release
    // whose build number is already loaded counts as readable whatever its version: the loaded
    // builds are kept by number.
    let builds = force_reindex::builds(&candidates);
    let reading = game_data.clone();
    control.work.spawn(async move {
        let local: BTreeSet<Release> = blocking(move || {
            builds
                .iter()
                .map(|need| need.release)
                .filter(|release| reading.can_read_build(release.build.0, Some(&release.version_said())))
                .collect()
        })
        .await;
        let missing: Vec<Release> = force_reindex::builds(&candidates)
            .into_iter()
            .map(|need| need.release)
            .filter(|release| !local.contains(release))
            .collect();

        // Nothing to fetch, or nowhere to fetch it to: what cannot be read here is left out, and
        // the summary says why.
        let fetchable = if missing.is_empty() {
            Fetchable::Releases(BTreeSet::new())
        } else {
            match cache_base(services) {
                None => Fetchable::NoCacheDirectory,
                Some(base) => match ask_repository(control, base, &missing).await {
                    Some(fetchable) => Fetchable::Releases(fetchable),
                    None => return,
                },
            }
        };
        let summary = force_reindex::summarize(&candidates, &local, &fetchable);
        confirm_run(control, summary, confirm, purpose, game_data);
    });
}

/// Asks the repository which of `missing` it publishes. `None` when it could not be asked about
/// all of them: what could not be asked about is not known to be absent, so nothing runs.
async fn ask_repository(control: BrowserControl, base: PathBuf, missing: &[Release]) -> Option<BTreeSet<Release>> {
    let shell = control.shell();
    let services = shell.services();
    let Some(runtime) = services.runtime() else {
        shell.toasts().warn(t!("ui.dialogs.download_plan_failed").into_owned());
        return None;
    };
    shell.toasts().info(t!("ui.reindex.planning").into_owned());
    let job = services.jobs.start(Job { name: t!("ui.app.status_cache_planning").into_owned(), progress: None });

    let requests: Vec<(u32, Option<String>)> =
        missing.iter().map(|release| (release.build.0, Some(release.version_said()))).collect();
    let proxy = proxy_url(services);
    // The repository's futures are not `Send`, so each is built and driven on one worker thread.
    let outcome = blocking(move || {
        match wows_toolkit_services::http::client(crate::http::USER_AGENT, &proxy, reqwest::redirect::Policy::default())
        {
            Ok(client) => runtime.block_on(game_data_cache::plan(&client, &base, &requests)),
            Err(err) => CacheOutcome::Failed(err.to_string()),
        }
    })
    .await;
    drop(job);

    let fetchable = match outcome {
        CacheOutcome::Planned { plan } => {
            force_reindex::fetchable_from_plan(missing, &plan.resolved).map_err(|unreachable| unreachable.to_string())
        }
        CacheOutcome::Failed(reason) => Err(reason),
        CacheOutcome::Checked { .. } | CacheOutcome::Validated { .. } | CacheOutcome::Downloaded { .. } => {
            Err("the repository answered a plan request with something else".to_owned())
        }
    };
    match fetchable {
        Ok(fetchable) => Some(fetchable),
        Err(reason) => {
            tracing::warn!("force re-index: the download could not be planned: {reason}");
            shell.toasts().warn(t!("ui.dialogs.download_plan_failed").into_owned());
            None
        }
    }
}

/// Puts the summary to the reader, or runs it straight away when `confirm` allows and there is
/// nothing to say.
fn confirm_run(
    control: BrowserControl,
    summary: ReindexSummary,
    confirm: Confirm,
    purpose: IndexPurpose,
    game_data: GameDataCache,
) {
    let shell = control.shell();
    if summary.replays.is_empty() {
        shell.toasts().warn(t!("ui.reindex.nothing_readable", count = summary.skipped()).into_owned());
        return;
    }
    if summary.runs_unasked(confirm) {
        run(control, summary, purpose, game_data);
        return;
    }

    let lines: Vec<(usize, String)> = summary.lines(&purpose).into_iter().enumerate().collect();
    let said = lines.iter().map(|(_, line)| line.as_str()).collect::<Vec<_>>().join("\n");
    let (title, ok) = (purpose.confirm_title(), purpose.confirm_ok());
    let then: Rc<dyn Fn()> = Rc::new(move || run(control, summary.clone(), purpose.clone(), game_data.clone()));
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let open = RwSignal::new_local(true);
        let lines = StoredValue::new_local(lines.clone());
        let (title, ok, said) =
            (StoredValue::new_local(title.clone()), StoredValue::new_local(ok.clone()), StoredValue::new_local(said.clone()));
        let then = StoredValue::new_local(Rc::clone(&then));
        AnyView::new(view! {
            AlertDialog(open = open, on_open_change = handle.on_open_change()) {
                AlertDialogContent(attr:data-testid = "force-reindex-confirm") {
                    AlertDialogHeader {
                        AlertDialogTitle {{title.get_value()}}
                    }
                    column(class = "wt-reindex-summary", attr:data-testid = "force-reindex-summary", a11y:label = said.get_value()) {
                        for line in move || lines.get_value(), key = |line: &(usize, String)| line.0 {
                            text {{line.1.clone()}}
                        }
                    }
                    AlertDialogFooter {
                        AlertDialogCancel {{t!("ui.buttons.cancel").into_owned()}}
                        AlertDialogAction(attr:data-testid = "force-reindex-ok", on:click = move |_| then.with_value(|then| then())) {
                            {ok.get_value()}
                        }
                    }
                }
            }
        })
    });
}

/// Fetches what the summary needs, then reads its replays again. Holds the pass from the start,
/// downloads included, so no other pass starts beside it.
fn run(control: BrowserControl, summary: ReindexSummary, purpose: IndexPurpose, game_data: GameDataCache) {
    let work = control.work;
    let services = control.shell().services();
    if work.is_busy() {
        control.shell().toasts().warn(t!("ui.reindex.busy_index").into_owned());
        return;
    }
    work.reindexing.set(true);

    let requested: BTreeSet<Release> = summary.downloads.iter().map(|need| need.release).collect();
    let base = if requested.is_empty() { None } else { cache_base(services) };
    work.spawn(async move {
        // Without a cache directory the replays waiting on a download are left out.
        let unavailable = match base {
            Some(base) => download(control, base, &summary.downloads).await,
            None => requested,
        };
        let (replays, dropped) = summary.readable_after(&unavailable);
        read_again(control, replays, summary.skipped() + dropped, purpose, game_data).await;
    });
}

/// Fetches each release `downloads` names, newest first, one at a time. Answers the releases that
/// could not be fetched.
async fn download(control: BrowserControl, base: PathBuf, downloads: &[BuildNeed]) -> BTreeSet<Release> {
    let shell = control.shell();
    let services = shell.services();
    let mut failed = BTreeSet::new();
    let Some(runtime) = services.runtime() else {
        shell.toasts().failed(t!("ui.messages.game_data_download_failed").into_owned());
        return downloads.iter().map(|need| need.release).collect();
    };
    let name = t!("ui.app.status_cache_downloading").into_owned();
    let job = services.jobs.start(Job { name: name.clone(), progress: None });
    let proxy = proxy_url(services);

    let mut fetched = 0;
    for need in downloads {
        let release = need.release;
        let build = BuildUpdateStatus { build: release.build.0, version: release.version_said() };
        let (base, proxy, runtime) = (base.clone(), proxy.clone(), Arc::clone(&runtime));
        let (reports, steps) = mpsc::unbounded::<game_data_cache::Progress>();
        // The repository's futures are not `Send`, so each is built and driven on one worker
        // thread.
        let fetching = blocking(move || {
            match wows_toolkit_services::http::client(
                crate::http::USER_AGENT,
                &proxy,
                reqwest::redirect::Policy::default(),
            ) {
                Ok(client) => runtime.block_on(game_data_cache::download(&client, &base, &[build], |step| {
                    let _ = reports.unbounded_send(step);
                })),
                Err(err) => CacheOutcome::Failed(err.to_string()),
            }
        });
        let outcome = follow(fetching, steps, |step| {
            job.report(Job { name: name.clone(), progress: Progress::new(step.done, step.total) });
        })
        .await;
        match outcome {
            CacheOutcome::Downloaded { fetched: count, failed: builds } => {
                fetched += count;
                if !builds.is_empty() {
                    failed.insert(release);
                }
            }
            CacheOutcome::Failed(reason) => {
                tracing::warn!(build = release.build.0, "force re-index: the game data was not fetched: {reason}");
                failed.insert(release);
            }
            CacheOutcome::Checked { .. } | CacheOutcome::Validated { .. } | CacheOutcome::Planned { .. } => {
                failed.insert(release);
            }
        }
    }
    drop(job);

    if failed.is_empty() {
        shell.toasts().ok(t!("ui.messages.game_data_builds_downloaded", count = fetched).into_owned());
    } else {
        shell.toasts().failed(t!("ui.messages.game_data_download_failed").into_owned());
    }
    // Which rows have a preview and which builds are missing are both decided as a directory is
    // walked.
    if fetched > 0 {
        relist_listed(control);
    }

    failed
}

/// Reads every listed directory again.
fn relist_listed(control: BrowserControl) {
    let listed: Vec<FolderId> = control.folders.with_untracked(|folders| {
        folders.iter().filter(|tab| tab.state == FolderState::Listed).map(|tab| tab.id).collect()
    });
    for id in listed {
        if let Some(listing) = control.listing(id) {
            listing.relist();
        }
    }
}

impl BrowserControl {
    /// Reads every listed directory again, for game data that has since arrived.
    pub fn relist(self) {
        relist_listed(self);
    }
}

/// Reads `replays` again through the status bar's progress line. `skipped` is what the run left
/// out before this, for what is said at the end.
async fn read_again(
    control: BrowserControl,
    replays: Vec<ScopedReplay>,
    skipped: usize,
    purpose: IndexPurpose,
    game_data: GameDataCache,
) {
    let work = control.work;
    let shell = control.shell();
    let services = shell.services();
    let (Some(pool), Some(runtime)) = (services.settings.pool(), services.runtime()) else {
        work.reindexing.set(false);
        shell.toasts().failed(t!("ui.reindex.no_index").into_owned());
        return;
    };
    // Checked as the run was prepared; the setting can be cleared while a download runs.
    let Some(live_root) = install_replays_root(services) else {
        work.reindexing.set(false);
        shell.toasts().failed(t!("ui.reindex.no_game_data").into_owned());
        return;
    };

    let name = t!("ui.app.status_indexing").into_owned();
    let job = services.jobs.start(Job { name: name.clone(), progress: Progress::new(0, replays.len() as u64) });
    let write_mode = purpose.write_mode();
    let (reports, steps) = mpsc::unbounded::<IndexProgress>();
    let proxy = proxy_url(services);
    let reading = blocking(move || {
        // Nothing here stops a forced pass part-way; this is the flag the pass checks.
        let cancel = AtomicBool::new(false);
        let rating = owner_rating(&runtime, &proxy);
        let report = |step| {
            let _ = reports.unbounded_send(step);
        };
        reindex_replays(&runtime, &pool, &live_root, &replays, write_mode, &game_data, rating.as_ref(), &cancel, report)
    });
    let ran = follow(reading, steps, |step| {
        job.report(Job { name: name.clone(), progress: Progress::new(step.done, step.total) });
    })
    .await;
    drop(job);
    work.reindexing.set(false);

    match ran {
        Ok(progress) => {
            shell.toasts().show(pass_notice(&purpose, progress, skipped));
        }
        Err(err) => {
            shell.toasts().failed(err.to_string());
        }
    }
    // The rows read what the index held before the pass.
    for listing in control.all_listings() {
        listing.load_summaries();
    }
    run_next_pass(control);
}

/// What a finished pass says. A pass that did not reach every replay counts the rest as skipped,
/// so the three figures add up to what was asked for.
fn pass_notice(purpose: &IndexPurpose, progress: IndexProgress, skipped: usize) -> Notice {
    let skipped = skipped as u64 + progress.total.saturating_sub(progress.done);
    let (indexed, failed) = (progress.indexed, progress.failed);
    let said = match purpose {
        IndexPurpose::Refresh => t!("ui.reindex.done", indexed = indexed, failed = failed, skipped = skipped),
        IndexPurpose::Fill(_) => t!("ui.index_folder.done", indexed = indexed, failed = failed, skipped = skipped),
    }
    .into_owned();

    if failed > 0 || skipped > 0 { Notice::warn(said) } else { Notice::ok(said) }
}

/// Awaits `work`, handing each step it reports to `on_step` as it arrives. The steps end when
/// `work` drops their sender.
async fn follow<T, P>(
    work: impl Future<Output = T>,
    mut steps: mpsc::UnboundedReceiver<P>,
    mut on_step: impl FnMut(P),
) -> T {
    let shown = async {
        while let Some(step) = steps.next().await {
            on_step(step);
        }
    };
    let (done, ()) = futures::future::join(work, shown).await;

    done
}

/// Looks up or creates a chosen directory's own source once its tab has found the directory,
/// then queues the pass that adds what the source does not hold yet.
pub fn index_folder(control: BrowserControl, id: FolderId) {
    stop_pass(control, id);
    let Some(tab) = control.tab(id) else { return };
    let FolderSource::Chosen(root) = tab.source else { return };
    if tab.state != FolderState::Listed {
        return;
    }
    let work = control.work;
    let held = work.sources.with_untracked(|sources| sources.get(&id).cloned());
    match held {
        Some(FolderSourceState::Resolved { root: resolved, .. }) if resolved == root => {
            queue_pass(control, id);
            return;
        }
        // The lookup under way queues the pass when it lands.
        Some(FolderSourceState::Resolving(resolving)) if resolving == root => return,
        Some(_) | None => {}
    }
    let services = control.shell().services();
    let Some(pool) = services.settings.pool() else { return };

    work.sources.update(|sources| {
        sources.insert(id, FolderSourceState::Resolving(root.clone()));
    });
    let ensuring = root.clone();
    let ensured = services.on_tokio(async move {
        SourceScope::Folder(ensuring.clone()).ensure(&pool, &ensuring, jiff::Timestamp::now()).await
    });
    work.spawn(async move {
        let ensured = match ensured.await {
            Ok(Ok(source)) => Ok(source),
            Ok(Err(err)) => Err(SourceUnavailable::from(err)),
            Err(err) => Err(SourceUnavailable::from(err)),
        };
        source_found(control, id, root, ensured);
    });
}

fn source_found(control: BrowserControl, id: FolderId, root: PathBuf, ensured: Result<SourceId, SourceUnavailable>) {
    let work = control.work;
    let waiting = work.sources.with_untracked(
        |sources| matches!(sources.get(&id), Some(FolderSourceState::Resolving(resolving)) if *resolving == root),
    );
    if !waiting {
        return;
    }

    match ensured {
        Ok(source) => {
            work.sources.update(|sources| {
                sources.insert(id, FolderSourceState::Resolved { root, source });
            });
            queue_pass(control, id);
        }
        Err(err) => {
            tracing::warn!("replay folders: the directory's index source could not be created: {err}");
            work.sources.update(|sources| {
                sources.insert(id, FolderSourceState::Unavailable(Rc::new(err)));
            });
        }
    }
}

/// Queues every listed chosen directory's pass again, for game data that has just arrived or
/// changed.
fn index_listed_folders(control: BrowserControl) {
    let listed: Vec<FolderId> = control.folders.with_untracked(|folders| {
        folders
            .iter()
            .filter(|tab| matches!(tab.source, FolderSource::Chosen(_)) && tab.state == FolderState::Listed)
            .map(|tab| tab.id)
            .collect()
    });
    for id in listed {
        index_folder(control, id);
    }
}

fn queue_pass(control: BrowserControl, id: FolderId) {
    control.work.passes.update(|passes| {
        passes.insert(id, PassState::Queued);
    });
    run_next_pass(control);
}

/// Stops a directory's pass, queued or running. A running one keeps the slot until its worker
/// has returned.
fn stop_pass(control: BrowserControl, id: FolderId) {
    let work = control.work;
    let Some(stopped) = work.passes.with_untracked(|passes| passes.get(&id).copied()) else { return };
    work.passes.update(|passes| {
        passes.remove(&id);
    });
    if stopped == PassState::Running {
        work.running.with_value(|running| {
            if let Some(pass) = running {
                pass.stop();
            }
        });
    }
}

/// Starts the first queued directory's pass, in tab order, unless a pass is reading replays. A
/// queued directory waits for the game data its replays are read through.
fn run_next_pass(control: BrowserControl) {
    let work = control.work;
    if work.is_busy() {
        return;
    }
    let services = control.shell().services();
    let Some(game_data) = services.game_data.cache_untracked() else { return };
    let (Some(pool), Some(runtime)) = (services.settings.pool(), services.runtime()) else { return };
    let next = control.folders.with_untracked(|folders| {
        folders.iter().find_map(|tab| {
            let queued = work.passes.with_untracked(|passes| passes.get(&tab.id) == Some(&PassState::Queued));
            let FolderSource::Chosen(root) = &tab.source else { return None };
            if !queued {
                return None;
            }
            let source = work.sources.with_untracked(|sources| match sources.get(&tab.id) {
                Some(FolderSourceState::Resolved { root: resolved, source }) if resolved == root => Some(*source),
                Some(_) | None => None,
            })?;
            Some((tab.id, source, root.clone()))
        })
    });
    let Some((id, source, root)) = next else { return };

    let name = t!("ui.replay_folders.indexing", dir = folder_name(&root)).into_owned();
    let job = services.jobs.start(Job { name: name.clone(), progress: None });
    let cancel = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&cancel);
    let (reports, steps) = mpsc::unbounded::<IndexProgress>();
    let proxy = proxy_url(services);
    let task = work.spawn(async move {
        let walking = blocking(move || {
            let rating = owner_rating(&runtime, &proxy);
            let report = |step| {
                let _ = reports.unbounded_send(step);
            };
            build_index(&runtime, &pool, source, &root, &game_data, rating.as_ref(), &stop, IndexMode::FillGaps, report)
        });
        let built = follow(walking, steps, |step| {
            job.report(Job { name: name.clone(), progress: Progress::new(step.done, step.total) });
        })
        .await;
        drop(job);
        pass_finished(control, id, built);
    });
    work.passes.update(|passes| {
        passes.insert(id, PassState::Running);
    });
    work.running.set_value(Some(FolderPass { cancel, _task: task }));
}

fn pass_finished(control: BrowserControl, id: FolderId, built: Result<IndexProgress, IndexError>) {
    match built {
        Ok(progress) => tracing::info!(
            indexed = progress.indexed,
            skipped = progress.skipped,
            failed = progress.failed,
            "replay folders: the directory was indexed"
        ),
        Err(err) => tracing::warn!("replay folders: the directory was not indexed: {err}"),
    }
    let work = control.work;
    // A pass stopped and queued again while it ran keeps its place in the queue.
    if work.passes.with_untracked(|passes| passes.get(&id) == Some(&PassState::Running)) {
        work.passes.update(|passes| {
            passes.remove(&id);
        });
    }
    // Its task is the one finishing, so it is dropped rather than stopped.
    drop(work.running.try_update_value(Option::take).flatten());

    // The rows read what the source held before the pass.
    if let Some(listing) = control.listing(id) {
        listing.load_summaries();
    }
    run_next_pass(control);
}

/// Stops the tab's pass and forgets its source: the tab closed, its directory is gone, or it was
/// folded into another tab.
pub fn folder_closed(control: BrowserControl, id: FolderId) {
    stop_pass(control, id);
    control.work.sources.update(|sources| {
        sources.remove(&id);
    });
}

/// Puts a replay that landed in a watched directory into the index, under that directory's
/// source, so the index fills as the reader plays. Its own parse: it runs whether or not the
/// replay is opened.
pub fn replay_landed(control: BrowserControl, id: FolderId, path: &Path) {
    let services = control.shell().services();
    let Some(game_data) = services.game_data.cache_untracked() else { return };
    let (Some(pool), Some(runtime)) = (services.settings.pool(), services.runtime()) else { return };
    let Some(listing) = control.listing(id) else { return };
    // Roots the Live source when this battle is the first thing it holds.
    let Some(live_root) = path.parent().map(Path::to_path_buf) else { return };
    let scope = listing.index_scope();
    let path = path.to_path_buf();
    let proxy = proxy_url(services);

    control.work.spawn(async move {
        let indexed = blocking(move || -> Result<(), IndexOneError> {
            let now = jiff::Timestamp::now();
            let source = runtime.block_on(scope.ensure(&pool, &live_root, now))?;
            let rating = owner_rating(&runtime, &proxy);
            index_one(&runtime, &pool, &path, &game_data, rating.as_ref(), source, now, IndexWriteMode::Incremental)
        })
        .await;
        if let Err(err) = indexed {
            tracing::warn!("replay index: the battle that just landed was not indexed: {err}");
        }
    });
}

/// Takes a result mapping the reader points at as the loaded build's own.
fn import_constants(control: BrowserControl) {
    let shell = control.shell();
    let build = match shell.services().game_data.state_untracked() {
        GameDataState::Ready(loaded) => loaded.build(),
        GameDataState::NoInstall | GameDataState::Loading { .. } | GameDataState::Failed(_) => {
            shell.toasts().warn(t!("ui.replay.constants_no_build").into_owned());
            return;
        }
    };
    let asked = wows_toolkit_services::dialog::pick_file(
        Some(&t!("ui.replay.constants_import_title")),
        Some(wows_toolkit_services::dialog::Filter { label: "JSON", extensions: &["json"] }),
    );

    control.work.spawn(async move {
        let Some(path) = chosen(shell, asked).await else { return };
        let imported = blocking(move || wows_toolkit_services::constants::import(&path, build)).await;
        match imported {
            Ok(()) => {
                shell.toasts().ok(t!("ui.replay.constants_imported", build = build).into_owned());
                if let Some(events) = control.work.events.get_value() {
                    events.send(ReplayBrowserEvent::ConstantsImported { build });
                }
            }
            Err(reason) => {
                shell.toasts().failed(t!("ui.replay.constants_import_failed", reason = reason).into_owned());
            }
        }
    });
}

/// Contributes every battle the selected tab lists, as the data-sharing setting asks.
///
/// Under `LedgerUse::Consult` a replay already in the ledger is skipped before it is read, so a
/// second pass costs one ledger read per replay; under `LedgerUse::Ignore` every one is read and
/// sent.
fn contribute_all(control: BrowserControl, ledger: LedgerUse) {
    let shell = control.shell();
    let services = shell.services();
    let toasts = shell.toasts();
    let work = control.work;
    let Some(mode) = services.settings.read_untracked(|settings| settings.data_sharing) else { return };
    if mode == DataSharingMode::Off {
        toasts.warn(t!("ui.replay.contribute_nothing_shared").into_owned());
        return;
    }
    let Some(game_data) = services.game_data.cache_untracked() else { return };
    let Some(pool) = services.settings.pool() else { return };
    let Some(listing) = control.selected_listing() else { return };
    let paths = listing.listed_paths();
    if paths.is_empty() {
        toasts.warn(t!("ui.replay.contribute_none_listed").into_owned());
        return;
    }
    // One pass at a time: two would read the same directory twice and, with the ledger ignored,
    // send every battle twice.
    if work.contributing.get_untracked() {
        toasts.warn(t!("ui.replay.contribute_already_running").into_owned());
        return;
    }
    let proxy = proxy_url(services);
    let total = paths.len();
    work.contributing.set(true);

    work.spawn(async move {
        let job = services
            .jobs
            .start(Job { name: t!("ui.replay.contributing", done = 1, total = total).into_owned(), progress: None });
        let mut sent = 0usize;
        for (index, path) in paths.into_iter().enumerate() {
            job.report(Job {
                name: t!("ui.replay.contributing", done = index + 1, total = total).into_owned(),
                progress: Progress::new(index as u64, total as u64),
            });

            // The ledger before the read: a replay already contributed is seconds of parsing for a
            // request that would be refused.
            if ledger == LedgerUse::Consult {
                let (asked, named) = (pool.clone(), path.clone());
                let already = services.on_tokio(async move { upload::already_contributed(&asked, &named).await }).await;
                if matches!(already, Ok(true)) {
                    continue;
                }
            }

            let (reading, cache) = (path.clone(), game_data.clone());
            let parsed = blocking(move || parse_replay(&reading, &cache, None)).await;
            let parsed = match parsed {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::warn!(path = %path.display(), %err, "sharing: the replay did not parse");
                    continue;
                }
            };

            // The anchor both apps keep, recorded on the first sight of this replay: a battle
            // still being played is held for its results rather than sent half-finished.
            let (asked, named) = (pool.clone(), path.clone());
            let first_seen = services.on_tokio(async move { upload::first_seen(&asked, &named).await }).await;
            // Without a runtime the window starts now, which holds the upload.
            let first_seen = first_seen.unwrap_or_else(|_| jiff::Timestamp::now());
            let candidate = Candidate { path, shareable: parsed.shareable, first_seen };
            let (sending, proxy) = (pool.clone(), proxy.clone());
            let outcome = services
                .on_tokio(async move {
                    let sender = Sender { pool: &sending, agent: crate::http::USER_AGENT, proxy: &proxy };
                    upload::send(candidate, mode, ledger, sender).await
                })
                .await;
            if matches!(outcome, Ok(Outcome::Sent)) {
                sent += 1;
            }
        }
        drop(job);

        work.contributing.set(false);
        shell.toasts().ok(t!("ui.replay.contributed", sent = sent, total = total).into_owned());
    });
}
