//! The directories the listing shows, one tab each under the header: the install's replays
//! directory first, then the ones the reader opened.
//!
//! The install's directory and pinned directories are watched; any other is read once when it
//! is opened and again on Refresh. A watched directory that is not selected counts the replays
//! added or changed in it until the reader selects it.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::MotionPreference;
use landaire_ui::Notice;
use landaire_ui::ToastKey;
use landaire_ui::motion_preference;
use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::paths::folder_name;
use wows_toolkit_replay_data::paths::shown_path;
use wows_toolkit_replay_data::replay_dir::WatchState;
use wows_toolkit_replay_data::replay_dir::WatchStopCause;
use wows_toolkit_services::supervision::NextStep;
use wows_toolkit_services::supervision::RestartPolicy;
use wows_toolkit_viewmodel::palette::PaletteMode;
use wows_toolkit_viewmodel::replay_folders::FOLDER_HISTORY;
use wows_toolkit_viewmodel::replay_folders::FolderHistory;
use wows_toolkit_viewmodel::replay_folders::PINNED_FOLDERS;
use wows_toolkit_viewmodel::replay_folders::PinnedFolders;
use zgui::component;
use zgui::css;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
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
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::TimeoutHandle;
use zgui::view::Timers;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::SpinnerProps;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;

use crate::icons::AppIconProps;
use crate::services::Services;
use crate::shell::Shell;
use crate::shell::palette::modes;
use crate::tabs::replays::browser::BrowserEvents;
use crate::tabs::replays::browser::ReplayBrowserEvent;
use crate::tabs::replays::browser::actions;
use crate::tabs::replays::browser::actions::BrowserWork;
use crate::tabs::replays::browser::header::ListingHeaderProps;
use crate::tabs::replays::browser::listing::Listing;
use crate::tabs::replays::browser::listing::ListingEvent;
use crate::tabs::replays::browser::listing::ListingSource;
use crate::tabs::replays::browser::listing::ListingWatch;
use crate::tabs::replays::browser::view::ListingViewProps;

const SHEET: &str = "wt-replay-folders";

const CSS: &str = css!(
    ".wt-browser {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-height: 0;
    }
    .wt-folder-tabs {
        display: flex;
        flex: none;
        gap: 2px;
        min-width: 0;
        padding: 0 4px;
        overflow-x: auto;
        overflow-y: hidden;
        --zgui-scrollbar-width: none;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-folder-tab {
        position: relative;
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        font-size: 12px;
        color: var(--wt-color-text-dim);
        cursor: pointer;
    }
    .wt-folder-tab:hover { background-color: var(--lui-color-widget-hot); }
    .wt-folder-tab-selected { color: var(--lui-color-text); font-weight: 600; }
    .wt-folder-tab__underline {
        position: absolute;
        bottom: 0;
        left: 4px;
        right: 4px;
        height: 2px;
        background-color: var(--lui-color-accent);
    }
    .wt-folder-tab__label {
        max-width: 140px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .wt-folder-tab__warning { color: var(--wt-color-warn); }
    .wt-folder-tab__close {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 14px;
        height: 14px;
        border-radius: 3px;
        color: var(--wt-color-text-dim);
    }
    .wt-folder-tab__close:hover { background-color: var(--lui-color-widget-hot); color: var(--lui-color-text); }
    .wt-watch-dot {
        position: relative;
        flex: none;
        width: 6px;
        height: 6px;
        border-radius: 3px;
        background-color: var(--wt-color-ok);
    }
    .wt-watch-dot--stopped { background-color: var(--wt-color-error); cursor: pointer; }
    .wt-watch-ring {
        position: absolute;
        left: -6px;
        top: -6px;
        width: 18px;
        height: 18px;
        border-radius: 9px;
        border: 1px solid var(--wt-color-ok);
        opacity: 0;
        animation: wt-watch-ring 600ms cubic-bezier(0.22, 1, 0.36, 1);
    }
    [reduced-motion] .wt-watch-ring { animation: none; }
    @keyframes wt-watch-ring {
        from { transform: scale(0.3333); opacity: 1; }
        to { transform: scale(1); opacity: 0; }
    }
    .wt-unseen-badge {
        display: flex;
        align-items: center;
        justify-content: center;
        min-width: 14px;
        height: 14px;
        padding: 0 3px;
        border-radius: 7px;
        background-color: var(--lui-color-accent);
        color: var(--lui-color-panel);
        font-size: 10px;
        font-weight: 600;
        animation: lui-fade-in 150ms ease-out;
    }
    .wt-folder-notice {
        display: flex;
        flex-direction: column;
        flex: 1;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 16px;
        text-align: center;
        font-size: 14px;
    }
    .wt-folder-notice__icon { color: var(--wt-color-warn); }
    .wt-folder-notice__hint { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-folder-notice-host { display: flex; flex-direction: column; flex: none; }
    .wt-folder-notice-host .wt-folder-notice { min-height: 240px; }
    .wt-folder-pane { display: flex; flex-direction: column; flex: 1; min-height: 0; }
    .wt-folder-pane-hidden { display: none; }"
);

/// How long the ring a landing replay sends out from a watched tab's dot takes to spread and
/// fade. The tab's count changes once it has.
pub(super) const LANDING_PULSE: Duration = Duration::from_millis(600);

#[cfg(test)]
thread_local! {
    /// The control the replays tab made last on this thread, for tests driving the shell.
    pub(super) static MADE: std::cell::Cell<Option<BrowserControl>> = const { std::cell::Cell::new(None) };
}

/// One directory tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FolderId(u64);

impl FolderId {
    /// The install's tab, which is always there and always first.
    pub const INSTALL: Self = Self(0);

    /// The number the tab's elements are addressed by.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Which directory a tab lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderSource {
    /// The install's replays directory, which follows the game directory in the settings.
    Install,
    /// A directory the reader opened, by its canonical path when it exists.
    Chosen(PathBuf),
}

/// Whether a tab's directory was there when it was last looked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderState {
    /// Being looked for off the UI thread: a directory on a network share can take many seconds
    /// to answer.
    Checking,
    Listed,
    Missing,
}

/// Whether a saved list has been read, which decides whether it may be written: a list that was
/// never read must not be written over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedList {
    NotRead,
    Reading,
    Read,
    Unreadable,
}

/// One directory tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderTab {
    pub id: FolderId,
    pub source: FolderSource,
    pub pinned: bool,
    pub state: FolderState,
}

impl FolderTab {
    /// Whether the tab's listing follows its directory: the install's always, a chosen one while
    /// pinned and there.
    pub fn watch(&self) -> ListingWatch {
        match (&self.source, self.pinned, self.state) {
            (FolderSource::Install, _, _) | (FolderSource::Chosen(_), true, FolderState::Listed) => {
                ListingWatch::Watched
            }
            (FolderSource::Chosen(_), _, _) => ListingWatch::Unwatched,
        }
    }

    pub fn label(&self) -> String {
        match &self.source {
            FolderSource::Install => t!("ui.replay_folders.install").into_owned(),
            FolderSource::Chosen(root) => folder_name(root),
        }
    }

    fn root(&self) -> Option<&Path> {
        match &self.source {
            FolderSource::Install => None,
            FolderSource::Chosen(root) => Some(root),
        }
    }
}

/// The replays landed in a tab whose count waits for the ring the latest of them sent.
pub(super) struct PendingLanding {
    pub(super) paths: HashSet<PathBuf>,
    /// Dropped to cancel the wait, when another landing restarts it or the tab goes.
    _timer: TimeoutHandle,
}

/// One look for a chosen directory, so only a tab's latest is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CheckId(u64);

/// The directory tabs and their listings, as a `Copy` handle the replays tab makes in its body
/// and hands to [`super::ReplayBrowser`].
#[derive(Clone, Copy)]
pub struct BrowserControl {
    pub(super) folders: RwSignal<Vec<FolderTab>, LocalStorage>,
    pub(super) selected: RwSignal<FolderId, LocalStorage>,
    pub(super) listings: RwSignal<HashMap<FolderId, Listing>, LocalStorage>,
    /// Replays added or changed in a tab since the reader last had it selected.
    unseen: RwSignal<HashMap<FolderId, HashSet<PathBuf>>, LocalStorage>,
    /// Counts the replays landed in each tab, so each landing sends its own ring.
    landings: RwSignal<HashMap<FolderId, u64>, LocalStorage>,
    pub(super) history: RwSignal<FolderHistory, LocalStorage>,
    pins_saved: RwSignal<SavedList, LocalStorage>,
    history_saved: RwSignal<SavedList, LocalStorage>,
    /// Recent directories dropped before the stored list was read, kept out of it once it is.
    history_dropped: StoredValue<Vec<PathBuf>, LocalStorage>,
    /// The grouping every listing shows, as the settings keep it.
    pub(super) grouping: RwSignal<ReplayGrouping, LocalStorage>,
    next_id: StoredValue<u64, LocalStorage>,
    timers: StoredValue<Option<Timers>, LocalStorage>,
    /// The landed replays each tab counts once its ring has spread, one wait per tab.
    pub(super) pending_landings: StoredValue<HashMap<FolderId, PendingLanding>, LocalStorage>,
    /// The install's replays directory by its canonical path, once resolved off the UI thread.
    install_key: StoredValue<Option<PathBuf>, LocalStorage>,
    next_check: StoredValue<u64, LocalStorage>,
    /// The look under way for each chosen tab's directory.
    checks: StoredValue<HashMap<FolderId, CheckId>, LocalStorage>,
    owner: StoredValue<Owner, LocalStorage>,
    /// The index passes, forced re-indexing and contributing.
    pub(super) work: BrowserWork,
    shell: Shell,
}

impl BrowserControl {
    /// Called from the replays tab's body inside the shell.
    pub fn new() -> Self {
        let shell = Shell::current();
        let install = FolderId::INSTALL;
        let control = Self {
            folders: RwSignal::new_local(vec![FolderTab {
                id: install,
                source: FolderSource::Install,
                pinned: false,
                state: FolderState::Listed,
            }]),
            selected: RwSignal::new_local(install),
            listings: RwSignal::new_local(HashMap::new()),
            unseen: RwSignal::new_local(HashMap::new()),
            landings: RwSignal::new_local(HashMap::new()),
            history: RwSignal::new_local(FolderHistory::default()),
            pins_saved: RwSignal::new_local(SavedList::NotRead),
            history_saved: RwSignal::new_local(SavedList::NotRead),
            history_dropped: StoredValue::new_local(Vec::new()),
            grouping: RwSignal::new_local(ReplayGrouping::default()),
            next_id: StoredValue::new_local(1),
            timers: StoredValue::new_local(Timers::current()),
            pending_landings: StoredValue::new_local(HashMap::new()),
            install_key: StoredValue::new_local(None),
            next_check: StoredValue::new_local(0),
            checks: StoredValue::new_local(HashMap::new()),
            owner: StoredValue::new_local(Owner::current().expect("the control is made inside a component")),
            work: BrowserWork::new(),
            shell,
        };
        control.follow_settings();
        #[cfg(test)]
        MADE.with(|made| made.set(Some(control)));
        control.offer_folders();

        control
    }

    fn services(self) -> Services {
        self.shell.services()
    }

    pub(super) fn shell(self) -> Shell {
        self.shell
    }

    fn spawn(self, future: impl Future<Output = ()> + 'static) {
        let owner = self.owner.get_value();
        owner.with(|| {
            spawn_local(future);
        });
    }

    /// Reads the grouping and the kept directories once the settings are in.
    fn follow_settings(self) {
        let services = self.services();
        let effect = RenderEffect::new(move |_| {
            let Some(grouping) = services.settings.read(|settings| settings.replay.grouping) else { return };
            self.grouping.set(grouping);
            // Tests share one settings database across threads, so they hand the stored lists in
            // through `adopt_*` rather than reading each other's.
            let restores = cfg!(not(test));
            if restores && self.pins_saved.get_untracked() == SavedList::NotRead && services.settings.pool().is_some() {
                self.restore_folders();
            }
        });
        on_cleanup_local(move || drop(effect));
    }

    /// Offers the recent directories on the palette.
    fn offer_folders(self) {
        let history = Signal::derive_local(move || self.history.with(|history| history.folders().to_vec()));
        self.shell.palette.offer_mode(PaletteMode::ReplayFolders, modes::replay_folders(self.shell, history));
        actions::offer(self);
    }

    /// Reads the pinned and recent directories, then opens the pinned ones.
    fn restore_folders(self) {
        let services = self.services();
        let Some(pool) = services.settings.pool() else { return };
        self.pins_saved.set(SavedList::Reading);
        self.history_saved.set(SavedList::Reading);
        self.spawn(async move {
            let read = services
                .on_tokio(async move {
                    let pins =
                        wows_toolkit_config::queries::try_get_setting::<PinnedFolders>(&pool, PINNED_FOLDERS).await;
                    let history =
                        wows_toolkit_config::queries::try_get_setting::<FolderHistory>(&pool, FOLDER_HISTORY).await;
                    (pins.map_err(|err| err.to_string()), history.map_err(|err| err.to_string()))
                })
                .await;
            let (pins, history) = match read {
                Ok(read) => read,
                Err(err) => (Err(err.to_string()), Err(err.to_string())),
            };
            match history {
                Ok(stored) => self.adopt_folder_history(stored.unwrap_or_default()),
                Err(err) => {
                    tracing::warn!("the recent replay directories could not be read: {err}");
                    self.history_saved.set(SavedList::Unreadable);
                }
            }
            match pins {
                Ok(stored) => self.adopt_pinned_folders(stored.unwrap_or_default()),
                Err(err) => {
                    tracing::warn!("the pinned replay directories could not be read: {err}");
                    self.pins_saved.set(SavedList::Unreadable);
                    self.shell.toasts().failed(t!("ui.replay_folders.pins_unreadable").into_owned());
                }
            }
        });
    }

    /// Opens a tab for each stored pinned directory, in order.
    pub(super) fn adopt_pinned_folders(self, stored: PinnedFolders) {
        self.pins_saved.set(SavedList::Reading);
        for root in stored.0.iter() {
            self.add_folder(root.clone(), true);
        }
        self.pins_saved.set(SavedList::Read);
        if self.pinned_folders() != stored {
            self.save_pinned_folders();
        }
    }

    /// Puts the stored recent directories after those opened this session.
    pub(super) fn adopt_folder_history(self, mut stored: FolderHistory) {
        self.history_saved.set(SavedList::Read);
        let dropped = self.history_dropped.get_value();
        self.history_dropped.set_value(Vec::new());
        for root in &dropped {
            stored.forget(root);
        }
        let changed_before =
            self.history.with_untracked(|history| !history.folders().is_empty()) || !dropped.is_empty();
        self.history.update(|history| history.merge_older(stored));
        if changed_before {
            self.save_folder_history();
        }
    }

    /// The pinned directories, in tab order.
    pub fn pinned_folders(self) -> PinnedFolders {
        PinnedFolders(self.folders.with_untracked(|folders| {
            folders.iter().filter(|tab| tab.pinned).filter_map(|tab| tab.root().map(Path::to_path_buf)).collect()
        }))
    }

    fn save_pinned_folders(self) {
        if self.pins_saved.get_untracked() == SavedList::Read {
            self.services().settings.save(PINNED_FOLDERS, &self.pinned_folders());
        }
    }

    fn save_folder_history(self) {
        if self.history_saved.get_untracked() == SavedList::Read {
            self.services().settings.save(FOLDER_HISTORY, &self.history.get_untracked());
        }
    }

    pub fn tab(self, id: FolderId) -> Option<FolderTab> {
        self.folders.with_untracked(|folders| folders.iter().find(|tab| tab.id == id).cloned())
    }

    pub fn listing(self, id: FolderId) -> Option<Listing> {
        self.listings.with_untracked(|listings| listings.get(&id).copied())
    }

    /// The selected tab's listing.
    pub fn selected_listing(self) -> Option<Listing> {
        self.listing(self.selected.get_untracked())
    }

    /// The install's listing.
    pub fn install_listing(self) -> Option<Listing> {
        self.listing(FolderId::INSTALL)
    }

    /// Every tab's listing, the install's first.
    pub fn all_listings(self) -> Vec<Listing> {
        self.folders.with_untracked(|folders| folders.iter().filter_map(|tab| self.listing(tab.id)).collect())
    }

    /// The tab already showing `root`: the install's when it is the install's directory. Chosen
    /// tabs are keyed by canonical path once checked, so no path is resolved here.
    fn folder_for(self, root: &Path) -> Option<FolderId> {
        let install = self.install_listing().and_then(|listing| listing.directory.get_untracked());
        let install_key = self.install_key.get_value();
        if install.as_deref() == Some(root) || install_key.as_deref() == Some(root) {
            return Some(FolderId::INSTALL);
        }

        self.folders.with_untracked(|folders| folders.iter().find(|tab| tab.root() == Some(root)).map(|tab| tab.id))
    }

    /// Adds a tab for `root`, or finds the one showing it, pinning it if asked.
    pub(super) fn add_folder(self, root: PathBuf, pinned: bool) -> FolderId {
        if let Some(id) = self.folder_for(&root) {
            if pinned {
                self.set_pinned(id, true);
            }
            return id;
        }

        let id = FolderId(self.next_id.get_value());
        self.next_id.update_value(|next| *next += 1);
        self.folders.update(|folders| {
            folders.push(FolderTab {
                id,
                source: FolderSource::Chosen(root.clone()),
                pinned,
                state: FolderState::Checking,
            });
        });
        self.check_folder(id, root);

        id
    }

    /// Looks for the tab's directory off the UI thread, under its canonical path. A later check
    /// of the same tab supersedes this one.
    fn check_folder(self, id: FolderId, root: PathBuf) {
        let check = CheckId(self.next_check.get_value());
        self.next_check.update_value(|next| *next += 1);
        self.checks.update_value(|checks| {
            checks.insert(id, check);
        });
        self.update_tab(id, |tab| tab.state = FolderState::Checking);
        self.spawn(async move {
            let (key, present) = blocking(move || {
                // A path that does not resolve is kept as named; it is then reported missing.
                let key = std::fs::canonicalize(&root).unwrap_or(root);
                let present = key.is_dir();
                (key, present)
            })
            .await;
            let current = self.checks.try_update_value(|checks| {
                let current = checks.get(&id) == Some(&check);
                if current {
                    checks.remove(&id);
                }
                current
            });
            if current == Some(true) {
                self.folder_checked(id, key, present);
            }
        });
    }

    fn folder_checked(self, id: FolderId, key: PathBuf, present: bool) {
        let Some(tab) = self.tab(id) else { return };
        let old = tab.root().map(Path::to_path_buf);

        // The same directory under another path: the tab already showing it stays.
        if let Some(into) = self.folder_for(&key).filter(|into| *into != id) {
            if self.selected.get_untracked() == id {
                self.select_folder(into);
            }
            self.remove_tab(id);
            if into == FolderId::INSTALL {
                // The install's tab always shows its directory, so the alias is neither pinned
                // nor recent.
                if tab.pinned {
                    self.save_pinned_folders();
                }
                if let Some(old) = old {
                    self.forget_recent(&old);
                }
                return;
            }
            if tab.pinned {
                self.set_pinned(into, true);
            }
            if let Some(old) = old {
                self.rename_recent(&old, &key);
            }
            return;
        }

        let state = if present { FolderState::Listed } else { FolderState::Missing };
        self.update_tab(id, |tab| {
            tab.source = FolderSource::Chosen(key.clone());
            tab.state = state;
        });
        if !present {
            self.unseen.update(|unseen| {
                unseen.remove(&id);
            });
        }
        if let Some(listing) = self.listing(id) {
            listing.set_source(ListingSource::Chosen(key.clone()));
            if present {
                listing.relist();
            }
            listing.set_watch(self.tab(id).map_or(ListingWatch::Unwatched, |tab| tab.watch()));
        }
        if old.as_deref() != Some(key.as_path()) {
            if tab.pinned {
                self.save_pinned_folders();
            }
            if let Some(old) = old {
                self.rename_recent(&old, &key);
            }
        }
        if present {
            actions::index_folder(self, id);
        } else {
            actions::folder_closed(self, id);
        }
    }

    fn update_tab(self, id: FolderId, change: impl FnOnce(&mut FolderTab)) {
        self.folders.update(|folders| {
            if let Some(tab) = folders.iter_mut().find(|tab| tab.id == id) {
                change(tab);
            }
        });
    }

    /// Resolves the install's replays directory off the UI thread, then folds the chosen tabs on
    /// it into the install's.
    fn install_listed(self, directory: PathBuf) {
        self.install_key.set_value(None);
        self.spawn(async move {
            let resolving = directory.clone();
            // A directory that does not resolve is compared by the path it was named by.
            let key = blocking(move || std::fs::canonicalize(&resolving).unwrap_or(resolving)).await;
            let current = self.install_listing().and_then(|listing| listing.directory.get_untracked());
            if current.as_ref() != Some(&directory) {
                return;
            }
            self.install_key.set_value(Some(key.clone()));
            self.fold_install_into_tabs(&key);
        });
    }

    /// A chosen tab on the install's replays directory gives way to the install's own tab, and
    /// the directory leaves the recent list, since the install's tab always shows it.
    fn fold_install_into_tabs(self, install: &Path) {
        let folded: Vec<FolderTab> = self.folders.with_untracked(|folders| {
            folders.iter().filter(|tab| tab.root() == Some(install)).cloned().collect()
        });
        for tab in folded {
            if self.selected.get_untracked() == tab.id {
                self.select_folder(FolderId::INSTALL);
            }
            self.remove_tab(tab.id);
            if tab.pinned {
                self.save_pinned_folders();
            }
            if let Some(root) = tab.root() {
                self.forget_recent(root);
            }
        }
    }

    /// Takes a tab out with what it holds: its unseen count, its pending landings, its pending
    /// check, its watch message and its index work.
    fn remove_tab(self, id: FolderId) {
        self.folders.update(|folders| folders.retain(|tab| tab.id != id));
        self.unseen.update(|unseen| {
            unseen.remove(&id);
        });
        self.landings.update(|landings| {
            landings.remove(&id);
        });
        self.pending_landings.update_value(|pending| {
            pending.remove(&id);
        });
        self.checks.update_value(|checks| {
            checks.remove(&id);
        });
        self.shell.toasts().withdraw(&watch_toast_key(id));
        actions::folder_closed(self, id);
    }

    fn rename_recent(self, old: &Path, new: &Path) {
        let renamed = self.history.try_update(|history| history.replace(old, new)) == Some(true);
        if renamed {
            self.save_folder_history();
        }
    }

    /// Drops `root` from the recent list, or remembers to once the stored list is read.
    fn forget_recent(self, root: &Path) {
        if self.history_saved.get_untracked() != SavedList::Read {
            self.history_dropped.update_value(|dropped| dropped.push(root.to_path_buf()));
        }
        if self.history.try_update(|history| history.forget(root)) == Some(true) {
            self.save_folder_history();
        }
    }

    /// Lists `root` in a tab, adding one unless it is listed already, and selects it.
    pub fn open_folder(self, root: PathBuf) {
        let id = self.add_folder(root.clone(), false);
        if id != FolderId::INSTALL {
            self.history.update(|history| history.record(root));
            self.save_folder_history();
        }
        self.select_folder(id);
    }

    /// Opens a recent directory, or says it is gone and drops it from the list.
    pub fn open_recent_folder(self, root: PathBuf) {
        self.spawn(async move {
            let checked = root.clone();
            let present = blocking(move || checked.is_dir()).await;
            if present {
                self.open_folder(root);
                return;
            }
            self.shell.toasts().warn(t!("ui.replay_folders.recent_missing", dir = shown_path(&root)).into_owned());
            self.forget_recent(&root);
        });
    }

    /// Asks for a directory and lists it.
    pub fn browse_folder(self) {
        let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.replay.open_directory"));
        self.spawn(async move {
            if let Ok(Some(directory)) = asked.await {
                self.open_folder(directory);
            }
        });
    }

    pub fn select_folder(self, id: FolderId) {
        let leaving = self.selected.get_untracked();
        if leaving == id {
            return;
        }
        if let Some(listing) = self.listing(leaving) {
            listing.clear_arrivals();
        }
        self.selected.set(id);
        self.unseen.update(|unseen| {
            unseen.remove(&id);
        });
        self.pending_landings.update_value(|pending| {
            pending.remove(&id);
        });
    }

    /// Pins or unpins a chosen directory's tab. Unpinning a tab whose directory is gone closes it.
    pub fn pin_folder(self, id: FolderId, pinned: bool) {
        if self.pins_saved.get_untracked() != SavedList::Read {
            return;
        }
        let Some(tab) = self.tab(id) else { return };
        if tab.source == FolderSource::Install || tab.pinned == pinned {
            return;
        }
        if tab.state == FolderState::Missing {
            if !pinned {
                self.close_folder(id);
            }
            return;
        }
        self.set_pinned(id, pinned);
    }

    fn set_pinned(self, id: FolderId, pinned: bool) {
        self.update_tab(id, |tab| tab.pinned = pinned);
        if !pinned {
            self.unseen.update(|unseen| {
                unseen.remove(&id);
            });
        }
        if let (Some(listing), Some(tab)) = (self.listing(id), self.tab(id)) {
            listing.set_watch(tab.watch());
        }
        self.save_pinned_folders();
    }

    /// Closes a chosen directory's tab; the install's tab stays.
    pub fn close_folder(self, id: FolderId) {
        let Some(tab) = self.tab(id) else { return };
        if tab.source == FolderSource::Install {
            return;
        }
        if self.selected.get_untracked() == id {
            self.select_folder(FolderId::INSTALL);
        }
        self.remove_tab(id);
        if tab.pinned {
            self.save_pinned_folders();
        }
    }

    /// Reads the tab's directory again: a chosen one is looked for first.
    pub fn refresh_folder(self, id: FolderId) {
        let Some(tab) = self.tab(id) else { return };
        match tab.source {
            FolderSource::Chosen(root) => self.check_folder(id, root),
            FolderSource::Install => {
                if let Some(listing) = self.listing(id) {
                    listing.relist();
                }
            }
        }
    }

    pub fn restart_folder_watch(self, id: FolderId) {
        if let Some(listing) = self.listing(id) {
            listing.restart_watch();
        }
    }

    /// How many replays the tab's badge counts: those noted that its listing still holds.
    pub fn unseen(self, id: FolderId) -> usize {
        let Some(listing) = self.listings.with(|listings| listings.get(&id).copied()) else { return 0 };
        self.unseen.with(|unseen| {
            unseen.get(&id).map_or(0, |paths| {
                listing.files.with(|files| files.iter().filter(|file| paths.contains(&file.path)).count())
            })
        })
    }

    /// Notes a replay that landed or changed in a tab the reader is not looking at. A landed one
    /// sends a ring from the tab's dot and counts once the ring has spread; a later landing in
    /// the same tab restarts the wait, so each tab holds one.
    fn note_unseen(self, id: FolderId, path: PathBuf, landed: bool, motion: MotionPreference) {
        if self.selected.get_untracked() == id {
            return;
        }
        let noted = self.unseen.with_untracked(|unseen| unseen.get(&id).is_some_and(|paths| paths.contains(&path)))
            || self.pending_landings.with_value(|pending| pending.get(&id).is_some_and(|landing| landing.paths.contains(&path)));
        if noted {
            return;
        }

        let timers = self.timers.get_value().filter(|_| landed && motion == MotionPreference::Full);
        let Some(timers) = timers else {
            self.count_unseen(id, std::iter::once(path));
            return;
        };
        self.landings.update(|landings| *landings.entry(id).or_insert(0) += 1);
        let timer = timers.set_timeout(LANDING_PULSE, move || self.count_landed(id));
        self.pending_landings.update_value(|pending| {
            let mut paths = pending.remove(&id).map(|landing| landing.paths).unwrap_or_default();
            paths.insert(path);
            pending.insert(id, PendingLanding { paths, _timer: timer });
        });
    }

    /// The tab's ring has spread: what landed in it counts.
    fn count_landed(self, id: FolderId) {
        let landing = self.pending_landings.try_update_value(|pending| pending.remove(&id)).flatten();
        if let Some(landing) = landing {
            self.count_unseen(id, landing.paths);
        }
    }

    fn count_unseen(self, id: FolderId, paths: impl IntoIterator<Item = PathBuf>) {
        if self.selected.get_untracked() != id {
            self.unseen.update(|unseen| unseen.entry(id).or_default().extend(paths));
        }
    }

    fn forget_unseen(self, id: FolderId, path: &Path) {
        self.unseen.update(|unseen| {
            if let Some(paths) = unseen.get_mut(&id) {
                paths.remove(path);
            }
        });
    }

    /// Carries out what folder `id`'s listing reports.
    fn on_listing_event(self, id: FolderId, event: ListingEvent, events: &BrowserEvents, motion: MotionPreference) {
        let install = id == FolderId::INSTALL;
        match event {
            ListingEvent::Appeared(path) => {
                let battle = self
                    .listing(id)
                    .and_then(|listing| listing.battle_label(&path))
                    .unwrap_or_else(|| path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned());
                self.note_unseen(id, path.clone(), true, motion);
                actions::replay_landed(self, id, &path);
                events.send(ReplayBrowserEvent::ReplayAppeared { path, battle, install });
            }
            ListingEvent::Changed(path) => {
                self.note_unseen(id, path.clone(), false, motion);
                events.send(ReplayBrowserEvent::ReplayChanged(path));
            }
            ListingEvent::Removed(path) => {
                self.forget_unseen(id, &path);
                events.send(ReplayBrowserEvent::ReplayRemoved(path));
            }
            ListingEvent::WatchStopped { cause, next } => self.say_watch_stopped(id, &cause, next),
            ListingEvent::WatchResumed => {
                let folder = self.tab(id).map(|tab| tab.label()).unwrap_or_default();
                self.shell.toasts().show(
                    Notice::ok(t!("ui.replay_folders.watch_resumed", folder = folder).into_owned())
                        .keyed(watch_toast_key(id)),
                );
            }
            ListingEvent::WatchWithdrawn => self.shell.toasts().withdraw(&watch_toast_key(id)),
            ListingEvent::BuildsMissing(missing) => events.send(ReplayBrowserEvent::BuildsMissing(missing)),
        }
    }

    fn say_watch_stopped(self, id: FolderId, cause: &WatchStopCause, next: NextStep) {
        let toasts = self.shell.toasts();
        if !next.is_announced() {
            toasts.withdraw(&watch_toast_key(id));
            return;
        }
        let folder = self.tab(id).map(|tab| tab.label()).unwrap_or_default();
        let notice = Notice::warn(watch_stopped_said(&folder, cause, next).join(" "))
            .keyed(watch_toast_key(id))
            .action(t!("ui.replay_folders.restart_watch").into_owned(), move || self.restart_folder_watch(id));
        let notice = match next {
            NextStep::GaveUp => notice.sticky(),
            NextStep::Retry { .. } => notice,
        };
        toasts.show(notice);
    }

    fn watch_stopped(self, id: FolderId) -> bool {
        let watched = self.tab(id).is_some_and(|tab| tab.watch() == ListingWatch::Watched);
        watched
            && self.listing(id).is_some_and(|listing| matches!(listing.watch_state.get(), WatchState::Stopped { .. }))
    }
}

fn watch_toast_key(id: FolderId) -> ToastKey {
    ToastKey::new(format!("replay-folder-watch-{}", id.0))
}

/// What is said when a tab's watch stops: why, then what happens next.
fn watch_stopped_said(folder: &str, cause: &WatchStopCause, next: NextStep) -> [String; 2] {
    let stopped = t!("ui.replay_folders.watch_stopped", folder = folder, cause = cause.said()).into_owned();
    let then = match next {
        NextStep::Retry { attempt, delay } => t!(
            "ui.replay_folders.watch_restarting",
            seconds = delay.as_secs(),
            attempt = attempt.get(),
            attempts = RestartPolicy::ATTEMPTS.get()
        )
        .into_owned(),
        NextStep::GaveUp => {
            t!("ui.replay_folders.watch_gave_up", attempts = RestartPolicy::ATTEMPTS.get()).into_owned()
        }
    };

    [stopped, then]
}

/// The header, the tab strip, and every tab's listing, the selected one shown.
#[allow(non_snake_case, reason = "built like a component, from the panel's own body")]
pub(super) fn FolderPanels(control: BrowserControl, events: BrowserEvents) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let motion = motion_preference();
    let notice = move || folder_notice(control);
    actions::adopt_events(control, events.clone());
    let pane_events = StoredValue::new_local(events.clone());

    view! {
        column(class = "wt-browser", attr:data-testid = "replay-browser") {
            ListingHeader(control = control, events = events)
            FolderStrip(control = control, motion = motion)
            column(class = "wt-folder-notice-host") {{notice}}
            for tab in move || control.folders.get(), key = |tab: &FolderTab| tab.id {
                FolderPane(control = control, id = tab.id, events = pane_events.get_value(), motion = motion)
            }
        }
    }
}

/// One tab's listing, built when the tab is added and kept while it is not selected so its
/// watch keeps running.
#[component]
fn FolderPane(
    control: BrowserControl,
    id: FolderId,
    events: BrowserEvents,
    motion: Signal<MotionPreference, LocalStorage>,
) -> impl IntoView {
    let tab = control.tab(id).expect("a pane is built for a tab that exists");
    let source = match &tab.source {
        FolderSource::Install => ListingSource::Install,
        FolderSource::Chosen(root) => ListingSource::Chosen(root.clone()),
    };
    // A chosen directory is scanned once it has been found: here when the check already landed,
    // otherwise when it lands.
    let watch = tab.watch();
    let reported = events.clone();
    let report: Rc<dyn Fn(ListingEvent)> =
        Rc::new(move |event| control.on_listing_event(id, event, &reported, motion.get_untracked()));
    let listing = Listing::new(source, watch, control.grouping.get_untracked(), report);
    control.listings.update(|listings| {
        listings.insert(id, listing);
    });
    on_cleanup_local(move || {
        control.listings.try_update(|listings| listings.remove(&id));
    });
    if matches!(tab.source, FolderSource::Chosen(_)) && tab.state == FolderState::Listed {
        listing.relist();
    }
    let grouping = RenderEffect::new(move |_| listing.set_grouping(control.grouping.get()));
    on_cleanup_local(move || drop(grouping));
    if id == FolderId::INSTALL {
        let folding = RenderEffect::new(move |_| match listing.directory.get() {
            Some(install) => control.install_listed(install),
            None => control.install_key.set_value(None),
        });
        on_cleanup_local(move || drop(folding));
    }

    let shown = move || {
        control.selected.get() == id
            && control
                .folders
                .with(|folders| folders.iter().any(|tab| tab.id == id && tab.state == FolderState::Listed))
    };

    view! {
        column(class = "wt-folder-pane", class:wt-folder-pane-hidden = move || !shown()) {
            ListingView(listing = listing, events = events.clone())
        }
    }
}

/// What a selected chosen directory shows instead of its listing while it is being looked for or
/// is gone.
fn folder_notice(control: BrowserControl) -> Option<AnyView> {
    let selected = control.selected.get();
    let tab = control.folders.with(|folders| folders.iter().find(|tab| tab.id == selected).cloned())?;
    let root = tab.root()?.to_path_buf();
    match tab.state {
        FolderState::Listed => None,
        FolderState::Checking => {
            let said = t!("ui.replay_folders.checking", dir = shown_path(&root)).into_owned();
            Some(AnyView::new(view! {
                column(class = "wt-folder-notice", attr:data-testid = "replay-folder-checking") {
                    Spinner(label = said.clone())
                    text {{said}}
                }
            }))
        }
        FolderState::Missing => {
            let hint = tab.pinned.then(|| {
                view! {
                    text(class = "wt-folder-notice__hint") {{t!("ui.replay_folders.missing_hint").into_owned()}}
                }
            });
            let (label, act) = if tab.pinned {
                (
                    t!("ui.replay_folders.unpin").into_owned(),
                    UnsyncCallback::new(move |()| control.pin_folder(selected, false)),
                )
            } else {
                (
                    t!("ui.replay_folders.close").into_owned(),
                    UnsyncCallback::new(move |()| control.close_folder(selected)),
                )
            };
            Some(AnyView::new(view! {
                column(class = "wt-folder-notice", attr:data-testid = "replay-folder-missing") {
                    AppIcon(glyph = Glyph::TriangleAlert, class = "wt-folder-notice__icon")
                    text {{t!("ui.replay_folders.missing", dir = shown_path(&root)).into_owned()}}
                    {hint.map(IntoView::into_view)}
                    Button(
                        size = ButtonSize::Sm,
                        variant = ButtonVariant::Outline,
                        attr:data-testid = "replay-folder-missing-action",
                        on:click = move |_| act.run(())
                    ) {{label}}
                }
            }))
        }
    }
}

/// The directory tabs.
#[component]
fn FolderStrip(control: BrowserControl, motion: Signal<MotionPreference, LocalStorage>) -> impl IntoView {
    view! {
        row(class = "wt-folder-tabs", attr:data-testid = "replay-folder-tabs") {
            for tab in move || control.folders.get(), key = |tab: &FolderTab| tab.id {
                FolderTabView(control = control, id = tab.id, motion = motion)
            }
        }
    }
}

#[component]
fn FolderTabView(
    control: BrowserControl,
    id: FolderId,
    motion: Signal<MotionPreference, LocalStorage>,
) -> impl IntoView {
    let tab =
        Signal::derive_local(move || control.folders.with(|folders| folders.iter().find(|tab| tab.id == id).cloned()));
    let selected = move || control.selected.get() == id;
    let lifted =
        move || selected() && tab.with(|tab| tab.as_ref().is_some_and(|tab| tab.state != FolderState::Checking));
    let label = move || tab.with(|tab| tab.as_ref().map(FolderTab::label).unwrap_or_default());
    let glyph = move || match tab.with(|tab| tab.as_ref().map(|tab| tab.source == FolderSource::Install)) {
        Some(true) => Glyph::Monitor,
        _ => Glyph::Folder,
    };
    let missing = move || tab.with(|tab| tab.as_ref().is_some_and(|tab| tab.state == FolderState::Missing));
    let pinned = move || tab.with(|tab| tab.as_ref().is_some_and(|tab| tab.pinned));
    let watched = move || tab.with(|tab| tab.as_ref().is_some_and(|tab| tab.watch() == ListingWatch::Watched));
    let stopped = move || control.watch_stopped(id);
    let unseen = move || if selected() { 0 } else { control.unseen(id) };
    let closable = move || tab.with(|tab| tab.as_ref().is_some_and(|tab| tab.source != FolderSource::Install));
    let landing = move || control.landings.with(|landings| landings.get(&id).copied());
    let ring = move || {
        let number = landing()?;
        (motion.get() == MotionPreference::Full).then_some(number)
    };
    let tooltip = move || folder_tooltip(control, id);

    view! {
        ContextMenu {
            ContextMenuTrigger {
                Tooltip {
                    TooltipTrigger {
                        control(
                            class = "wt-folder-tab",
                            class:wt-folder-tab-selected = lifted,
                            attr:data-testid = format!("replay-folder-{}", id.0),
                            a11y:label = label,
                            on:click = move |_| control.select_folder(id),
                            on:pointer_up = move |ev| {
                                if ev.button == Some(PointerButton::Middle) && closable() {
                                    control.close_folder(id);
                                }
                            }
                        ) {
                            Show(when = lifted) {
                                box(class = "wt-folder-tab__underline")
                            }
                            AppIcon(glyph = Signal::derive_local(glyph))
                            text(class = "wt-folder-tab__label") {{label}}
                            Show(when = missing) {
                                AppIcon(glyph = Glyph::TriangleAlert, class = "wt-folder-tab__warning")
                            }
                            Show(when = pinned) {
                                AppIcon(glyph = Glyph::Pin, attr:data-testid = format!("replay-folder-pin-{}", id.0))
                            }
                            Show(when = move || watched() && !stopped()) {
                                box(class = "wt-watch-dot", attr:data-testid = format!("replay-folder-watched-{}", id.0)) {
                                    for number in move || ring().into_iter().collect::<Vec<u64>>(), key = |number: &u64| *number {
                                        box(class = "wt-watch-ring", attr:data-testid = format!("replay-folder-ring-{number}"))
                                    }
                                }
                            }
                            Show(when = move || watched() && stopped()) {
                                control(
                                    class = "wt-watch-dot wt-watch-dot--stopped",
                                    attr:data-testid = format!("replay-folder-watch-stopped-{}", id.0),
                                    a11y:label = t!("ui.replay_folders.restart_watch").into_owned(),
                                    on:click = move |ev| {
                                        ev.stop_propagation();
                                        control.restart_folder_watch(id);
                                    }
                                )
                            }
                            Show(when = move || unseen() > 0) {
                                text(
                                    class = "wt-unseen-badge",
                                    attr:data-testid = format!("replay-folder-badge-{}", id.0),
                                    a11y:label = move || unseen().to_string()
                                ) {{move || unseen().to_string()}}
                            }
                            Show(when = closable) {
                                control(
                                    class = "wt-folder-tab__close",
                                    attr:data-testid = format!("replay-folder-close-{}", id.0),
                                    a11y:label = t!("ui.replay_folders.close").into_owned(),
                                    on:click = move |ev| {
                                        ev.stop_propagation();
                                        control.close_folder(id);
                                    }
                                ) {
                                    AppIcon(glyph = Glyph::X)
                                }
                            }
                        }
                    }
                    TooltipContent {{tooltip}}
                }
            }
            ContextMenuContent {
                {move || tab_menu(control, id)}
            }
        }
    }
}

/// The tab's menu: pin or unpin, restart a stopped watch, refresh, close.
fn tab_menu(control: BrowserControl, id: FolderId) -> Option<AnyView> {
    let tab = control.tab(id)?;
    let chosen = tab.source != FolderSource::Install;
    let pins_ready = control.pins_saved.get() == SavedList::Read;
    let pin = (chosen && !(!tab.pinned && tab.state == FolderState::Missing)).then(|| {
        let (label, pinned) = if tab.pinned {
            (t!("ui.replay_folders.unpin").into_owned(), false)
        } else {
            (t!("ui.replay_folders.pin").into_owned(), true)
        };
        view! {
            MenuItem(disabled = !pins_ready, on_select = UnsyncCallback::new(move |()| control.pin_folder(id, pinned))) {{label}}
        }
    });
    let restart = control.watch_stopped(id).then(|| {
        view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| control.restart_folder_watch(id))) {
                {t!("ui.replay_folders.restart_watch").into_owned()}
            }
        }
    });
    let close = chosen.then(|| {
        view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| control.close_folder(id))) {
                {t!("ui.replay_folders.close").into_owned()}
            }
        }
    });

    Some(AnyView::new(view! {
        {pin.map(IntoView::into_view)}
        {restart.map(IntoView::into_view)}
        MenuItem(on_select = UnsyncCallback::new(move |()| control.refresh_folder(id))) {
            {t!("ui.replay_folders.refresh").into_owned()}
        }
        {close.map(IntoView::into_view)}
    }))
}

/// What hovering a tab says: its directory, whether it is followed, and what it holds unseen.
fn folder_tooltip(control: BrowserControl, id: FolderId) -> String {
    let Some(tab) = control.folders.with(|folders| folders.iter().find(|tab| tab.id == id).cloned()) else {
        return String::new();
    };
    let directory = match tab.root() {
        Some(root) => Some(root.to_path_buf()),
        None => control.listings.with(|listings| listings.get(&id).and_then(|listing| listing.directory.get())),
    };
    let dir = directory.as_deref().map(shown_path).unwrap_or_default();
    match tab.state {
        FolderState::Checking => return t!("ui.replay_folders.checking", dir = dir).into_owned(),
        FolderState::Missing => return t!("ui.replay_folders.missing", dir = dir).into_owned(),
        FolderState::Listed => {}
    }

    let mut lines = vec![dir];
    match tab.watch() {
        ListingWatch::Watched => {
            let state = control.listings.with(|listings| listings.get(&id).map(|listing| listing.watch_state.get()));
            match state {
                Some(WatchState::Stopped { cause, next }) => {
                    lines.extend(watch_stopped_said(&tab.label(), &cause, next));
                    lines.push(t!("ui.replay_folders.watch_restart_hint").into_owned());
                }
                _ => lines.push(t!("ui.replay_folders.watched").into_owned()),
            }
        }
        ListingWatch::Unwatched => lines.push(t!("ui.replay_folders.unwatched").into_owned()),
    }
    let unseen = control.unseen(id);
    if control.selected.get() != id && unseen > 0 {
        lines.push(t!("ui.replay_folders.unseen", count = unseen).into_owned());
    }

    lines.join("\n")
}
