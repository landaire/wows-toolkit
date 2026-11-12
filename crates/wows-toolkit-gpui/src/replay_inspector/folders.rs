//! The directories the replay listing shows, one tab each under the listing's
//! header: the install's replays directory first, then the ones the reader
//! opened.
//!
//! The install's directory and pinned directories are watched; any other is
//! read once when it is opened and again on Refresh. A watched directory that
//! is not selected counts the replays added or changed in it until the reader
//! selects it.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::menu::ContextMenuExt as _;
use serde::Deserialize;
use serde::Serialize;
use wows_toolkit_replay_data::install::resolve_replays_dir;
use wows_toolkit_replay_data::paths::folder_name;
use wows_toolkit_services::supervision::NextStep;
use wows_toolkit_services::supervision::RestartPolicy;

use super::*;
use crate::toast::Level;
use crate::toast::Stay;
use wows_toolkit_replay_data::replay_dir::WatchState;
use wows_toolkit_replay_data::replay_dir::WatchStopCause;

/// The settings row the pinned directories are kept in, in tab order.
pub(crate) const PINNED_FOLDERS: &str = "gpui.pinned_replay_directories";

/// The settings row the recently opened directories are kept in, most recent
/// first.
pub(crate) const FOLDER_HISTORY: &str = "gpui.replay_folder_history";

/// One directory tab, named by the listing that reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FolderId(EntityId);

impl FolderId {
    /// The number the tab's elements are addressed by.
    fn element_index(self) -> usize {
        self.0.as_u64() as usize
    }
}

/// Which directory a tab lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FolderSource {
    /// The install's replays directory, which follows the game directory set
    /// in the settings.
    Install,
    /// A directory the reader opened, by its canonical path when it exists.
    Chosen(PathBuf),
}

/// Whether a tab's directory was there when it was last looked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FolderState {
    /// Being looked for off the UI thread: a directory on a network share can
    /// take many seconds to answer.
    Checking,
    Listed,
    Missing,
}

/// How long the ring a landing replay sends out from a watched tab's dot
/// takes to spread and fade. The tab's count changes once it has.
const LANDING_PULSE: Duration = Duration::from_millis(600);

/// Numbers the replays noted in one tab, so each landing draws its own ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LandingNumber(u64);

/// The latest replay noted in a tab.
#[derive(Clone, Copy, Debug)]
struct Landing {
    number: LandingNumber,
    at: Instant,
}

/// What brought a replay to a tab's notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Noted {
    /// A replay new to the directory, which sends out a ring.
    Landed,
    /// A replay already there written again.
    Rewritten,
}

/// Whether a saved list has been read, which decides whether it may be
/// written: a list that was never read must not be written over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SavedList {
    NotRead,
    Reading,
    Read,
    Unreadable,
}

pub(crate) struct ReplayFolder {
    source: FolderSource,
    browser: Entity<ReplayBrowser>,
    pinned: bool,
    state: FolderState,
    /// A chosen directory's own index source. The install's directory writes
    /// the Live source and leaves this unresolved.
    index: FolderIndex,

    /// Replays added or changed since the reader last had this tab selected,
    /// with when the badge starts counting each.
    unseen: HashMap<PathBuf, Instant>,
    landing: Option<Landing>,
    /// Renders the tab again when the latest landing's ring has spread.
    _count_wake: Option<Task<()>>,
    _events: Subscription,
}

impl ReplayFolder {
    pub(crate) fn id(&self) -> FolderId {
        FolderId(self.browser.entity_id())
    }

    pub(crate) fn source(&self) -> &FolderSource {
        &self.source
    }

    pub(crate) fn state(&self) -> FolderState {
        self.state
    }

    pub(crate) fn is_pinned(&self) -> bool {
        self.pinned
    }

    pub(crate) fn browser(&self) -> &Entity<ReplayBrowser> {
        &self.browser
    }

    pub(crate) fn index(&self) -> &FolderIndex {
        &self.index
    }

    pub(super) fn index_mut(&mut self) -> &mut FolderIndex {
        &mut self.index
    }

    /// How many replays the tab's badge counts: those noted that the listing
    /// still holds.
    pub(crate) fn unseen(&self, cx: &App) -> usize {
        let browser = self.browser.read(cx);
        self.unseen.keys().filter(|path| browser.holds(path)).count()
    }

    /// What the badge counts at `now`: a landed replay once its ring has
    /// spread, so the count changes after the ring rather than with it.
    fn counted(&self, now: Instant, cx: &App) -> usize {
        let reduce_motion = cx.reduce_motion();
        let browser = self.browser.read(cx);
        self.unseen
            .iter()
            .filter(|(path, counts_from)| (reduce_motion || now >= **counts_from) && browser.holds(path))
            .count()
    }

    fn watch(&self) -> ListingWatch {
        match (&self.source, self.pinned, self.state) {
            (FolderSource::Install, _, _) | (FolderSource::Chosen(_), true, FolderState::Listed) => {
                ListingWatch::Watched
            }
            (FolderSource::Chosen(_), _, _) => ListingWatch::Unwatched,
        }
    }

    /// What the tab is called: the game's own directory, or the opened
    /// directory's name.
    fn label(&self) -> String {
        match &self.source {
            FolderSource::Install => t!("ui.replay_folders.install").into_owned(),
            FolderSource::Chosen(root) => folder_name(root),
        }
    }

    /// Whether the tab's directory is watched but its watch has stopped.
    fn watch_stopped(&self, cx: &App) -> bool {
        self.watch() == ListingWatch::Watched
            && matches!(self.browser.read(cx).watch_state(), WatchState::Stopped { .. })
    }
}

/// The pinned directories in tab order, as kept in the settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct PinnedFolders(pub Vec<PathBuf>);

/// The directories most recently opened, most recent first, as kept in the
/// settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct FolderHistory(Vec<PathBuf>);

impl FolderHistory {
    const LIMIT: usize = 10;

    pub(crate) fn folders(&self) -> &[PathBuf] {
        &self.0
    }

    /// Puts `root` first, once.
    pub(crate) fn record(&mut self, root: PathBuf) {
        self.0.retain(|held| *held != root);
        self.0.insert(0, root);
        self.0.truncate(Self::LIMIT);
    }

    /// Drops `root`, saying whether it was there.
    pub(crate) fn forget(&mut self, root: &Path) -> bool {
        let before = self.0.len();
        self.0.retain(|held| held != root);

        self.0.len() != before
    }

    /// Puts `new` where `old` was, once. Says whether `old` was there.
    fn replace(&mut self, old: &Path, new: &Path) -> bool {
        let Some(at) = self.0.iter().position(|held| held == old) else { return false };

        self.0[at] = new.to_path_buf();
        let mut kept = false;
        self.0.retain(|held| held != new || !std::mem::replace(&mut kept, true));

        true
    }

    /// Puts `older` after what was recorded here, as a stored list read after
    /// some directories were already opened this session.
    fn merge_older(&mut self, older: FolderHistory) {
        for root in older.0 {
            if !self.0.contains(&root) {
                self.0.push(root);
            }
        }
        self.0.truncate(Self::LIMIT);
    }
}

/// How a directory is told apart from another: its canonical path when it
/// exists, else the path as given, so a missing pinned directory keeps its
/// place in the list.
fn folder_key(root: PathBuf) -> PathBuf {
    std::fs::canonicalize(&root).unwrap_or(root)
}

impl ReplayInspectorView {
    /// A tab for `source`, its listing reporting to this view.
    pub(super) fn new_folder(
        source: FolderSource,
        pinned: bool,
        state: FolderState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ReplayFolder {
        let browser = cx.new(ReplayBrowser::new);
        let events = cx.subscribe_in(&browser, window, Self::on_browser_event);

        ReplayFolder {
            source,
            browser,
            pinned,
            state,
            index: FolderIndex::new(),
            unseen: HashMap::new(),
            landing: None,
            _count_wake: None,
            _events: events,
        }
    }

    #[cfg(test)]
    pub(crate) fn folders(&self) -> &[ReplayFolder] {
        &self.folders
    }

    #[cfg(test)]
    pub(crate) fn selected_folder(&self) -> FolderId {
        self.selected_folder
    }

    /// The listing on screen.
    pub(crate) fn browser(&self) -> &Entity<ReplayBrowser> {
        self.folder(self.selected_folder).map_or_else(|| self.install_browser(), ReplayFolder::browser)
    }

    /// The listing of the install's replays directory, which is always the
    /// first tab.
    pub(crate) fn install_browser(&self) -> &Entity<ReplayBrowser> {
        &self.folders[0].browser
    }

    /// Every tab's listing, in tab order.
    pub(super) fn browsers(&self) -> Vec<Entity<ReplayBrowser>> {
        self.folders.iter().map(|folder| folder.browser.clone()).collect()
    }

    pub(super) fn folder(&self, id: FolderId) -> Option<&ReplayFolder> {
        self.folders.iter().find(|folder| folder.id() == id)
    }

    pub(super) fn folder_mut(&mut self, id: FolderId) -> Option<&mut ReplayFolder> {
        self.folders.iter_mut().find(|folder| folder.id() == id)
    }

    /// The selected tab's directory, once it is known.
    pub(super) fn selected_folder_path(&self) -> Option<PathBuf> {
        match self.folder(self.selected_folder)?.source() {
            FolderSource::Install => self.install_root.clone(),
            FolderSource::Chosen(root) => Some(root.clone()),
        }
    }

    /// The tab a listing belongs to.
    pub(super) fn folder_of(&self, browser: &Entity<ReplayBrowser>) -> Option<FolderId> {
        let id = FolderId(browser.entity_id());
        self.folder(id).map(ReplayFolder::id)
    }

    /// Lists the install's replays directory in the first tab, and learns
    /// where that is off the UI thread: finding it reads the install's
    /// preferences, and comparing it with the other tabs resolves its path.
    pub(crate) fn scan_install(&mut self, wows_dir: String, cx: &mut Context<Self>) {
        self.install_root = None;
        self.install_browser().clone().update(cx, |browser, cx| browser.start_scan(wows_dir.clone(), cx));
        if wows_dir.is_empty() {
            self._install_root_check = None;
            return;
        }

        // Replaced by the next scan, which drops a check still running for this one.
        self._install_root_check = Some(cx.spawn(async move |this, cx| {
            let root = cx.background_spawn(async move { folder_key(resolve_replays_dir(Path::new(&wows_dir))) }).await;
            let _ = this.update(cx, |this, cx| {
                this.install_root = Some(root);
                this.fold_install_into_tabs(cx);
            });
        }));
    }

    /// The tab that lists `root` as written, if one does.
    fn folder_for(&self, root: &Path) -> Option<FolderId> {
        if self.install_root.as_deref() == Some(root) {
            return Some(self.folders[0].id());
        }

        self.folders
            .iter()
            .find(|folder| folder.source == FolderSource::Chosen(root.to_path_buf()))
            .map(ReplayFolder::id)
    }

    /// Notes a change in a tab's directory that the reader has not seen, when
    /// the tab is not the one selected. Says whether it was noted.
    pub(super) fn note_unseen(
        &mut self,
        browser: &Entity<ReplayBrowser>,
        path: &Path,
        noted: Noted,
        cx: &mut Context<Self>,
    ) -> bool {
        let selected = self.selected_folder;
        let Some(folder) = self.folders.iter_mut().find(|folder| folder.browser == *browser) else { return false };
        if folder.id() == selected {
            return false;
        }

        cx.notify();
        if folder.unseen.contains_key(path) {
            return true;
        }

        let now = cx.background_executor().now();
        if noted == Noted::Rewritten {
            folder.unseen.insert(path.to_path_buf(), now);
            return true;
        }

        folder.unseen.insert(path.to_path_buf(), now + LANDING_PULSE);
        let number = folder.landing.map_or(LandingNumber(0), |landing| LandingNumber(landing.number.0 + 1));
        folder.landing = Some(Landing { number, at: now });
        // Replaces the wake for an earlier landing, whose count waits for this ring.
        folder._count_wake = (!cx.reduce_motion()).then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(LANDING_PULSE).await;
                let _ = this.update(cx, |_this, cx| cx.notify());
            })
        });

        true
    }

    /// Drops a replay that left a tab's directory from what its badge counts.
    pub(super) fn forget_unseen(&mut self, browser: &Entity<ReplayBrowser>, path: &Path, cx: &mut Context<Self>) {
        let Some(folder) = self.folders.iter_mut().find(|folder| folder.browser == *browser) else { return };
        if folder.unseen.remove(path).is_some() {
            cx.notify();
        }
    }

    pub(crate) fn select_folder(&mut self, id: FolderId, cx: &mut Context<Self>) {
        let Some(at) = self.folders.iter().position(|folder| folder.id() == id) else { return };

        // The listing being left drops its New tags, so they are gone when the
        // reader comes back to it.
        if self.selected_folder != id
            && let Some(left) = self.folder(self.selected_folder)
        {
            left.browser().update(cx, |browser, cx| browser.clear_new_arrivals(cx));
        }

        self.selected_folder = id;
        self.folders[at].unseen.clear();
        self.folder_scroll.scroll_to_item(at);
        cx.notify();
    }

    /// Adds a tab for `root`, or finds the one it already has. A directory has
    /// one tab however its path is written, and the install's replays
    /// directory is always its own first tab.
    ///
    /// A new tab is listed once its directory has been looked for, which
    /// happens off the UI thread; a second spelling of a directory that has a
    /// tab is merged into that tab then.
    pub(crate) fn add_folder(
        &mut self,
        root: PathBuf,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FolderId {
        if let Some(id) = self.folder_for(&root) {
            if pinned {
                self.set_pinned(id, true, cx);
            }
            return id;
        }

        let folder = Self::new_folder(FolderSource::Chosen(root.clone()), pinned, FolderState::Checking, window, cx);
        let watch = folder.watch();
        let id = folder.id();
        let (install_dir, locale, grouping) =
            (self.install_dir.clone(), self.locale.clone(), self.replay_settings.grouping);
        let (cache, status) = (self.game_data.clone(), self.game_data_status.clone());
        folder.browser.update(cx, |browser, cx| {
            browser.set_wows_dir(install_dir);
            browser.set_locale(locale, cx);
            browser.set_build_cache(cache);
            browser.set_game_data(&status, cx);
            browser.set_grouping(grouping, cx);
            browser.set_watch(watch, cx);
        });
        self.folders.push(folder);
        self.check_folder(id, root, cx);
        cx.notify();

        id
    }

    /// Looks for a tab's directory off the UI thread: one on a network share
    /// that is offline can take many seconds to answer.
    fn check_folder(&mut self, id: FolderId, root: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (key, present) = cx
                .background_spawn(async move {
                    let key = folder_key(root);
                    let present = key.is_dir();
                    (key, present)
                })
                .await;
            let _ = this.update(cx, |this, cx| this.folder_checked(id, key, present, cx));
        })
        .detach();
    }

    /// Lists a tab whose directory has been looked for, or says it is gone,
    /// or merges it into the tab that already lists the same directory.
    fn folder_checked(&mut self, id: FolderId, key: PathBuf, present: bool, cx: &mut Context<Self>) {
        // Closed while it was being looked for.
        let Some(at) = self.folders.iter().position(|folder| folder.id() == id) else { return };
        let FolderSource::Chosen(old) = self.folders[at].source.clone() else { return };

        let existing = if self.install_root.as_ref() == Some(&key) {
            Some(self.folders[0].id())
        } else {
            self.folders
                .iter()
                .find(|folder| folder.id() != id && folder.source == FolderSource::Chosen(key.clone()))
                .map(ReplayFolder::id)
        };
        if let Some(existing) = existing {
            self.merge_folder(id, existing, cx);
            self.rename_recent(&old, &key, cx);
            return;
        }

        let folder = &mut self.folders[at];
        folder.source = FolderSource::Chosen(key.clone());
        folder.state = if present { FolderState::Listed } else { FolderState::Missing };
        if !present {
            folder.unseen.clear();
        }
        let watch = folder.watch();
        let pinned = folder.pinned;
        folder.browser.update(cx, |browser, cx| {
            // A directory that is gone is not walked: the listing would read
            // it as one without replays.
            if present {
                browser.scan_directory(key.clone(), cx);
            }
            // After the scan has started, so the listing follows the directory
            // from when the scan finishes rather than walking it twice.
            browser.set_watch(watch, cx);
        });
        if old != key {
            if pinned {
                self.save_pinned_folders(cx);
            }
            self.rename_recent(&old, &key, cx);
        }
        if present {
            self.index_folder(id, cx);
        } else {
            self.stop_folder_pass(id, cx);
        }
        cx.notify();
    }

    /// Folds the tab `from` into `into`, which lists the same directory: its
    /// pin and the selection move with it.
    fn merge_folder(&mut self, from: FolderId, into: FolderId, cx: &mut Context<Self>) {
        let Some(at) = self.folders.iter().position(|folder| folder.id() == from) else { return };

        let merged = self.folders.remove(at);
        crate::toast::withdraw_everywhere(watch_toast_key(&merged.browser), cx);
        if merged.pinned {
            if let Some(folder) = self.folder_mut(into).filter(|folder| folder.source != FolderSource::Install) {
                folder.pinned = true;
                let watch = folder.watch();
                folder.browser.update(cx, |browser, cx| browser.set_watch(watch, cx));
            }
            self.save_pinned_folders(cx);
        }
        if self.selected_folder == from {
            self.select_folder(into, cx);
        }
        // A pass the merged tab was running stopped with it.
        self.run_next_folder_pass(cx);
        cx.notify();
    }

    /// Follows a recent directory to the path its tab compares by, or drops
    /// it when that is the install's own.
    fn rename_recent(&mut self, old: &Path, new: &Path, cx: &mut Context<Self>) {
        let changed = if self.install_root.as_deref() == Some(new) {
            self.forget_recent(old)
        } else {
            old != new && self.folder_history.replace(old, new)
        };
        if changed {
            self.save_folder_history(cx);
        }
    }

    /// Drops a recent directory, here and from the stored list when that has
    /// not been read yet. Says whether it was held here.
    fn forget_recent(&mut self, root: &Path) -> bool {
        if self.history_saved != SavedList::Read {
            self.history_dropped.push(root.to_path_buf());
        }

        self.folder_history.forget(root)
    }

    /// Opens `root` as the reader asked for it: its tab is selected, and the
    /// directory goes to the front of the recently opened ones.
    pub(crate) fn open_folder(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.add_folder(root.clone(), false, window, cx);
        if id != self.folders[0].id() {
            // As given; `folder_checked` puts the path its tab compares by in its place.
            self.folder_history.record(root);
            self.save_folder_history(cx);
        }
        self.select_folder(id, cx);
    }

    /// Opens a directory from the recently opened ones, or says it is gone and
    /// drops it from them. Looked for off the UI thread, as a tab's is.
    pub(crate) fn open_recent_folder(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let looked_for = root.clone();
        cx.spawn_in(window, async move |this, cx| {
            let present = cx.background_spawn(async move { looked_for.is_dir() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if present {
                    this.open_folder(root, window, cx);
                    return;
                }

                let said = t!("ui.replay_folders.recent_missing", dir = shown_path(&root)).into_owned();
                crate::toast::warn(said, window, cx);
                if this.forget_recent(&root) {
                    this.save_folder_history(cx);
                }
            });
        })
        .detach();
    }

    /// Asks for a directory and opens it.
    pub(crate) fn browse_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.replay.open_directory"));
        cx.spawn_in(window, async move |this, cx| {
            let Some(directory) = crate::dialog::chosen(asked, &this, cx).await else { return };
            let _ = this.update_in(cx, |this, window, cx| this.open_folder(directory, window, cx));
        })
        .detach();
    }

    pub(crate) fn folder_history(&self) -> &FolderHistory {
        &self.folder_history
    }

    pub(crate) fn pin_folder(&mut self, id: FolderId, pinned: bool, cx: &mut Context<Self>) {
        if self.pins_saved != SavedList::Read {
            return;
        }
        let Some(folder) = self.folder(id) else { return };
        if folder.source == FolderSource::Install || folder.pinned == pinned {
            return;
        }

        // Unpinned, a directory that is not there has nothing left to show.
        if !pinned && folder.state == FolderState::Missing {
            self.close_folder(id, cx);
            return;
        }
        if pinned && folder.state == FolderState::Missing {
            return;
        }

        self.set_pinned(id, pinned, cx);
    }

    fn set_pinned(&mut self, id: FolderId, pinned: bool, cx: &mut Context<Self>) {
        let Some(folder) = self.folder_mut(id) else { return };
        if folder.pinned == pinned {
            return;
        }

        folder.pinned = pinned;
        // An unwatched directory's count would no longer follow it.
        if !pinned {
            folder.unseen.clear();
        }
        let watch = folder.watch();
        folder.browser.update(cx, |browser, cx| browser.set_watch(watch, cx));
        self.save_pinned_folders(cx);
        cx.notify();
    }

    /// Closes a tab, unpinning it without asking: the directory is untouched,
    /// and pinning it again is two clicks.
    pub(crate) fn close_folder(&mut self, id: FolderId, cx: &mut Context<Self>) {
        let Some(at) = self.folders.iter().position(|folder| folder.id() == id) else { return };
        if self.folders[at].source == FolderSource::Install {
            return;
        }

        let closed = self.folders.remove(at);
        crate::toast::withdraw_everywhere(watch_toast_key(&closed.browser), cx);
        if closed.pinned {
            self.save_pinned_folders(cx);
        }
        if self.selected_folder == id {
            self.selected_folder = self.folders[0].id();
            self.folders[0].unseen.clear();
        }
        self.run_next_folder_pass(cx);
        cx.notify();
    }

    /// Reads a tab's directory again: lists one that has come back, and says
    /// so of one that has gone.
    pub(crate) fn refresh_folder(&mut self, id: FolderId, cx: &mut Context<Self>) {
        let Some(folder) = self.folder(id) else { return };
        let (source, browser) = (folder.source.clone(), folder.browser.clone());

        match source {
            FolderSource::Chosen(root) => self.check_folder(id, root, cx),
            FolderSource::Install => browser.update(cx, |browser, cx| browser.relist(cx)),
        }
    }

    /// Drops the tab of a directory that has become the install's own, which
    /// the first tab lists and watches from then on.
    fn fold_install_into_tabs(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.install_root.clone() else { return };

        let source = FolderSource::Chosen(root.clone());
        if let Some(folder) = self.folders.iter().find(|folder| folder.source == source) {
            let id = folder.id();
            let install = self.folders[0].id();
            self.merge_folder(id, install, cx);
        }
        if self.forget_recent(&root) {
            self.save_folder_history(cx);
        }
    }

    /// The pinned directories, in tab order.
    pub(crate) fn pinned_folders(&self) -> PinnedFolders {
        PinnedFolders(
            self.folders
                .iter()
                .filter(|folder| folder.pinned)
                .filter_map(|folder| match &folder.source {
                    FolderSource::Chosen(root) => Some(root.clone()),
                    FolderSource::Install => None,
                })
                .collect(),
        )
    }

    fn save_pinned_folders(&self, cx: &mut Context<Self>) {
        if self.pins_saved != SavedList::Read {
            return;
        }
        crate::settings_store::save(PINNED_FOLDERS, &self.pinned_folders(), cx);
    }

    fn save_folder_history(&self, cx: &mut Context<Self>) {
        if self.history_saved != SavedList::Read {
            return;
        }
        crate::settings_store::save(FOLDER_HISTORY, &self.folder_history, cx);
    }

    /// Reads the pinned and recently opened directories once the settings
    /// database is open, and opens the pinned ones as tabs.
    pub(crate) fn restore_folders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pins_saved != SavedList::NotRead {
            return;
        }
        let Some(pool) = crate::settings_store::pool(cx) else { return };

        self.pins_saved = SavedList::Reading;
        self.history_saved = SavedList::Reading;
        cx.spawn_in(window, async move |this, cx| {
            let stored = crate::runtime::block_on(cx, move || async move {
                let pinned =
                    wows_toolkit_config::queries::try_get_setting::<PinnedFolders>(&pool, PINNED_FOLDERS).await;
                let history =
                    wows_toolkit_config::queries::try_get_setting::<FolderHistory>(&pool, FOLDER_HISTORY).await;
                (pinned, history)
            })
            .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let (pinned, history) = stored;
                match history {
                    // No row is a list nothing has been opened into yet.
                    Ok(history) => this.adopt_folder_history(history.unwrap_or_default(), cx),
                    Err(err) => {
                        tracing::warn!("replay folders: the recently opened directories could not be read: {err}");
                        this.history_saved = SavedList::Unreadable;
                    }
                }
                match pinned {
                    // No row is a list nothing has been pinned to yet.
                    Ok(pinned) => this.adopt_pinned_folders(pinned.unwrap_or_default(), window, cx),
                    Err(err) => {
                        tracing::warn!("replay folders: the pinned directories could not be read: {err}");
                        // Pinning stays off, so the stored list is not written over.
                        this.pins_saved = SavedList::Unreadable;
                        crate::toast::failed(t!("ui.replay_folders.pins_unreadable").into_owned(), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the pinned directories read at startup, in their saved order, and
    /// lets the list be changed from then on.
    pub(crate) fn adopt_pinned_folders(&mut self, pinned: PinnedFolders, window: &mut Window, cx: &mut Context<Self>) {
        // Not written while the tabs are being added, which would store the
        // list part-way through.
        self.pins_saved = SavedList::Reading;
        for root in pinned.0.iter().cloned() {
            self.add_folder(root, true, window, cx);
        }
        self.pins_saved = SavedList::Read;

        // A stored path naming the install, or a tab pinned while the list was
        // being read, leaves the tabs differing from what was stored.
        if self.pinned_folders() != pinned {
            self.save_pinned_folders(cx);
        }
        cx.notify();
    }

    /// Takes the recently opened directories read at startup, behind any
    /// opened before they were read and without any dropped before then.
    pub(crate) fn adopt_folder_history(&mut self, mut stored: FolderHistory, cx: &mut Context<Self>) {
        self.history_saved = SavedList::Read;
        let dropped = std::mem::take(&mut self.history_dropped);
        for root in &dropped {
            stored.forget(root);
        }
        let changed_before = !self.folder_history.0.is_empty() || !dropped.is_empty();
        self.folder_history.merge_older(stored);
        if changed_before {
            self.save_folder_history(cx);
        }
    }

    /// The tab strip under the listing's header. Scrolls sideways once the
    /// tabs are wider than the listing.
    pub(super) fn render_folder_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let tabs: Vec<AnyElement> = self.folders.iter().map(|folder| self.render_folder_tab(folder, cx)).collect();

        h_flex()
            .id("replay-folder-tabs")
            .test_support()
            .flex_none()
            .w_full()
            .overflow_x_scroll()
            .track_scroll(&self.folder_scroll)
            .gap_0p5()
            .px_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(tabs)
            .into_any_element()
    }

    fn render_folder_tab(&self, folder: &ReplayFolder, cx: &mut Context<Self>) -> AnyElement {
        let id = folder.id();
        let index = id.element_index();
        let selected = id == self.selected_folder;
        let now = cx.background_executor().now();
        // The selected tab is what the reader is looking at, so it counts nothing.
        let unseen = if selected { 0 } else { folder.counted(now, cx) };
        let watched = folder.watch() == ListingWatch::Watched;
        let stopped = folder.watch_stopped(cx);
        let glyph = match &folder.source {
            FolderSource::Install => crate::icons::Glyph::Monitor,
            FolderSource::Chosen(_) => crate::icons::Glyph::Folder,
        };
        let label = folder.label();
        let indicator = watched.then(|| {
            if !stopped {
                return watch_dot(index, folder.landing, now, cx);
            }
            stopped_dot(id, cx)
        });
        let view = cx.entity();

        let tab = h_flex()
            .id(("replay-folder", index))
            .test_support()
            .aria_label(label.clone())
            .relative()
            .flex_none()
            .gap_1()
            .px_2()
            .py_1()
            .items_center()
            .text_xs()
            .cursor_pointer()
            .hover(|style| style.bg(cx.theme().secondary_hover))
            .when(selected, |tab| {
                tab.text_color(cx.theme().foreground)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().absolute().bottom_0().left_1().right_1().h(px(2.)).bg(crate::theme::accent()))
            })
            .when(!selected || folder.state == FolderState::Checking, |tab| tab.text_color(crate::theme::text_dim()))
            .child(crate::icons::icon(glyph))
            .child(div().max_w(px(140.)).truncate().child(label))
            .when(folder.state == FolderState::Missing, |tab| {
                tab.child(crate::icons::icon(crate::icons::Glyph::TriangleAlert).text_color(cx.theme().warning))
            })
            .when(folder.pinned, |tab| {
                tab.child(crate::icons::icon(crate::icons::Glyph::Pin).id(("replay-folder-pin", index)).test_support())
            })
            .children(indicator)
            .when(unseen > 0, |tab| tab.child(unseen_badge(index, unseen, cx)))
            .tooltip({
                let view = view.clone();
                // A tab closed while hovered says nothing.
                move |window, cx| {
                    let said = view.read(cx).folder_tooltip(id, cx).unwrap_or_default();
                    Tooltip::new(said).build(window, cx)
                }
            })
            .on_click(cx.listener(move |this, _event, _window, cx| this.select_folder(id, cx)));

        let source = folder.source.clone();
        let pinned = folder.pinned;
        let state = folder.state;
        let pins_ready = self.pins_saved == SavedList::Read;
        tab.context_menu(move |menu, _window, _cx| {
            let pin = view.clone();
            let restart = view.clone();
            let refresh = view.clone();
            let close = view.clone();
            let menu = match (&source, pinned, state) {
                (FolderSource::Install, _, _) | (FolderSource::Chosen(_), false, FolderState::Missing) => menu,
                (FolderSource::Chosen(_), _, _) => {
                    let label = if pinned { t!("ui.replay_folders.unpin") } else { t!("ui.replay_folders.pin") };
                    menu.item(PopupMenuItem::new(label.into_owned()).disabled(!pins_ready).on_click(
                        move |_event, _window, cx| {
                            pin.update(cx, |view, cx| view.pin_folder(id, !pinned, cx));
                        },
                    ))
                }
            };
            let menu = if stopped {
                menu.item(PopupMenuItem::new(t!("ui.replay_folders.restart_watch").into_owned()).on_click(
                    move |_event, _window, cx| {
                        restart.update(cx, |view, cx| view.restart_folder_watch(id, cx));
                    },
                ))
            } else {
                menu
            };
            let menu = menu.item(PopupMenuItem::new(t!("ui.replay_folders.refresh").into_owned()).on_click(
                move |_event, _window, cx| {
                    refresh.update(cx, |view, cx| view.refresh_folder(id, cx));
                },
            ));
            match source {
                FolderSource::Install => menu,
                FolderSource::Chosen(_) => {
                    menu.item(PopupMenuItem::new(t!("ui.replay_folders.close").into_owned()).on_click(
                        move |_event, _window, cx| {
                            close.update(cx, |view, cx| view.close_folder(id, cx));
                        },
                    ))
                }
            }
        })
        .into_any_element()
    }

    /// Starts a tab's stopped watch again.
    pub(crate) fn restart_folder_watch(&mut self, id: FolderId, cx: &mut Context<Self>) {
        let Some(folder) = self.folder(id) else { return };
        folder.browser.clone().update(cx, |browser, cx| browser.restart_watch(cx));
    }

    /// Tells the reader a tab's watch has stopped, with a button to restart
    /// it.
    ///
    /// Only the first stop of a run of failures is said, and the one that
    /// ends the automatic restarts, which stays until dismissed; the restarts
    /// between are only logged. Each replaces what was last said of the tab.
    pub(super) fn say_watch_stopped(
        &self,
        browser: &Entity<ReplayBrowser>,
        cause: &WatchStopCause,
        next: NextStep,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A restart that ran only briefly leaves its resumed message up,
        // which is no longer so.
        if !next.is_announced() {
            crate::toast::withdraw(watch_toast_key(browser), window, cx);
            return;
        }
        let Some(id) = self.folder_of(browser) else { return };
        let Some(folder) = self.folder(id) else { return };

        let said = watch_stopped_said(&folder.label(), cause, next).join(" ");
        let view = cx.entity().downgrade();
        let restart =
            crate::toast::Action::new(t!("ui.replay_folders.restart_watch").into_owned(), move |_window, cx| {
                let _ = view.update(cx, |view, cx| view.restart_folder_watch(id, cx));
            });
        let stay = match next {
            NextStep::Retry { .. } => Stay::Briefly,
            NextStep::GaveUp => Stay::UntilDismissed,
        };
        crate::toast::about(watch_toast_key(browser), Level::Warning, said, Some(restart), stay, window, cx);
    }

    /// Tells the reader a tab's stopped watch is running again, in place of
    /// what was said of it stopping.
    pub(super) fn say_watch_resumed(&self, browser: &Entity<ReplayBrowser>, window: &mut Window, cx: &mut App) {
        let Some(folder) = self.folder_of(browser).and_then(|id| self.folder(id)) else { return };

        let said = t!("ui.replay_folders.watch_resumed", folder = folder.label()).into_owned();
        crate::toast::about(watch_toast_key(browser), Level::Ok, said, None, Stay::Briefly, window, cx);
    }

    /// What hovering a tab says: the whole path, whether it is watched, and
    /// what its badge counts.
    fn folder_tooltip(&self, id: FolderId, cx: &App) -> Option<String> {
        let folder = self.folder(id)?;
        let root = match &folder.source {
            FolderSource::Install => self.install_root.clone(),
            FolderSource::Chosen(root) => Some(root.clone()),
        };
        let shown = root.as_deref().map(shown_path);
        // Only an opened directory is looked for or found gone, and it always has a path.
        let dir = || shown.clone().unwrap_or_default();
        match folder.state {
            FolderState::Checking => return Some(t!("ui.replay_folders.checking", dir = dir()).into_owned()),
            FolderState::Missing => return Some(t!("ui.replay_folders.missing", dir = dir()).into_owned()),
            FolderState::Listed => {}
        }

        let mut lines: Vec<String> = shown.into_iter().collect();
        match (folder.watch(), folder.browser.read(cx).watch_state()) {
            (ListingWatch::Watched, WatchState::Stopped { cause, next }) => {
                lines.extend(watch_stopped_said(&folder.label(), cause, *next));
                lines.push(t!("ui.replay_folders.watch_restart_hint").into_owned());
            }
            (ListingWatch::Watched, WatchState::Starting | WatchState::Running) => {
                lines.push(t!("ui.replay_folders.watched").into_owned());
            }
            (ListingWatch::Unwatched, _) => lines.push(t!("ui.replay_folders.unwatched").into_owned()),
        }
        let unseen = folder.unseen(cx);
        if id != self.selected_folder && unseen > 0 {
            lines.push(t!("ui.replay_folders.unseen", count = unseen).into_owned());
        }

        Some(lines.join("\n"))
    }

    /// What the listing shows for a tab whose directory is being looked for
    /// or is gone.
    pub(super) fn render_folder_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let folder = self.folder(self.selected_folder)?;
        let FolderSource::Chosen(root) = &folder.source else { return None };
        match folder.state {
            FolderState::Listed => return None,
            FolderState::Checking => {
                let said = t!("ui.replay_folders.checking", dir = shown_path(root)).into_owned();
                return Some(crate::loading::Loading::new("replay-folder-checking", said).top().into_any_element());
            }
            FolderState::Missing => {}
        }

        let id = folder.id();
        let pinned = folder.pinned;
        let action = if pinned { t!("ui.replay_folders.unpin") } else { t!("ui.replay_folders.close") };
        let element = v_flex()
            .id("replay-folder-missing")
            .test_support()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .child(crate::icons::icon(crate::icons::Glyph::TriangleAlert).text_color(cx.theme().warning))
            .child(
                div()
                    .text_sm()
                    .text_center()
                    .child(t!("ui.replay_folders.missing", dir = shown_path(root)).into_owned()),
            )
            .when(pinned, |panel| {
                panel.child(
                    div()
                        .text_xs()
                        .text_center()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.replay_folders.missing_hint").into_owned()),
                )
            })
            .child(Button::new("replay-folder-missing-action").label(action.into_owned()).small().on_click(
                cx.listener(move |this, _event, _window, cx| {
                    if pinned {
                        this.pin_folder(id, false, cx);
                    } else {
                        this.close_folder(id, cx);
                    }
                }),
            ))
            .into_any_element();

        Some(element)
    }
}

/// The count of replays a tab holds that the reader has not seen.
fn unseen_badge(index: usize, unseen: usize, cx: &App) -> AnyElement {
    let said = unseen.to_string();
    let badge = div()
        .id(("replay-folder-badge", index))
        .test_support()
        .aria_label(said.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(px(14.))
        .h(px(14.))
        .px(px(3.))
        .rounded_full()
        .bg(crate::theme::accent())
        .text_color(cx.theme().background)
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(said);

    crate::motion::fade_in(("replay-folder-badge-fade", index), badge)
}

/// How wide the dot on a watched tab is.
const WATCH_DOT: Pixels = px(6.);

/// The dot on a watched tab, which sends out one ring as a replay lands in
/// its directory.
fn watch_dot(index: usize, landing: Option<Landing>, now: Instant, cx: &App) -> AnyElement {
    /// How many dot widths across the ring is when it has faded.
    const REACH: f32 = 3.;

    let color = cx.theme().success;
    let spreading =
        landing.filter(|landing| !cx.reduce_motion() && now.saturating_duration_since(landing.at) < LANDING_PULSE);
    let ring = spreading.map(|landing| {
        div().absolute().rounded_full().border_1().border_color(color).with_animation(
            ElementId::NamedInteger("replay-folder-ring".into(), landing.number.0),
            Animation::new(LANDING_PULSE).with_easing(ease_out_quint()),
            |ring, delta| {
                let across = WATCH_DOT * (1. + delta * (REACH - 1.));
                let offset = (WATCH_DOT - across) / 2.;
                ring.size(across).top(offset).left(offset).opacity(1. - delta)
            },
        )
    });

    div()
        .id(("replay-folder-watched", index))
        .test_support()
        .relative()
        .flex_none()
        .size(WATCH_DOT)
        .rounded_full()
        .bg(color)
        .children(ring)
        .into_any_element()
}

/// The dot on a watched tab whose watch has stopped, which restarts it when
/// clicked.
fn stopped_dot(id: FolderId, cx: &Context<ReplayInspectorView>) -> AnyElement {
    div()
        .id(("replay-folder-watch-stopped", id.element_index()))
        .on_click(cx.listener(move |this, _event, _window, cx| {
            // Restarting is all the dot does; the tab stays as it is.
            cx.stop_propagation();
            this.restart_folder_watch(id, cx);
        }))
        .test_support()
        .flex_none()
        .size(WATCH_DOT)
        .rounded_full()
        .bg(cx.theme().danger)
        .cursor_pointer()
        .into_any_element()
}

/// What is said of a stopped watch: what stopped and why, then whether it is
/// started again.
fn watch_stopped_said(folder: &str, cause: &WatchStopCause, next: NextStep) -> [String; 2] {
    let attempts = RestartPolicy::ATTEMPTS.get();
    let stopped = t!("ui.replay_folders.watch_stopped", folder = folder, cause = cause.said()).into_owned();
    let next = match next {
        NextStep::Retry { attempt, delay } => t!(
            "ui.replay_folders.watch_restarting",
            seconds = delay.as_secs(),
            attempt = attempt.get(),
            attempts = attempts
        ),
        NextStep::GaveUp => t!("ui.replay_folders.watch_gave_up", attempts = attempts),
    };

    [stopped, next.into_owned()]
}

/// The identity of what is said about a listing's watch, so each message
/// replaces the last.
pub(super) fn watch_toast_key(browser: &Entity<ReplayBrowser>) -> ElementId {
    ElementId::NamedInteger("replay-folder-watch".into(), browser.entity_id().as_u64())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use gpui_kit::AppContext as _;
    use gpui_kit::Context;
    use gpui_kit::ElementId;
    use gpui_kit::Entity;
    use gpui_kit::TestAppContext;
    use gpui_kit::Window;
    use gpui_kit::WindowHandle;
    use gpui_kit::component::IndexPath;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt as _;
    use rust_i18n::t;
    use wows_toolkit_services::supervision::NextStep;
    use wows_toolkit_viewmodel::listing_row::ListedReplay;

    use super::FOLDER_HISTORY;
    use super::FolderHistory;
    use super::FolderId;
    use super::FolderSource;
    use super::PINNED_FOLDERS;
    use super::PinnedFolders;
    use crate::app::App;
    use crate::interaction_tests::open_app_in_root;
    use crate::replay_inspector::ReplayBrowser;
    use crate::replay_inspector::ReplayInspectorView;
    use wows_toolkit_replay_data::replay_dir::DirChange;
    use wows_toolkit_replay_data::replay_dir::WatchState;
    use wows_toolkit_replay_data::replay_dir::WatchStopCause;
    use wows_toolkit_replay_data::simulated_watch;

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
        let path = wows_toolkit_services::constants::cached_path(build).expect("tests have a storage directory");
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
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    /// The opened directories, in tab order.
    fn roots(view: &Entity<ReplayInspectorView>, cx: &mut TestAppContext) -> Vec<PathBuf> {
        cx.update(|cx| {
            view.read(cx)
                .folders()
                .iter()
                .filter_map(|folder| match folder.source() {
                    FolderSource::Chosen(root) => Some(root.clone()),
                    FolderSource::Install => None,
                })
                .collect()
        })
    }

    fn folder_id(view: &Entity<ReplayInspectorView>, root: &Path, cx: &mut TestAppContext) -> FolderId {
        cx.update(|cx| {
            let folders = view.read(cx).folders();
            folders
                .iter()
                .find(|folder| *folder.source() == FolderSource::Chosen(root.to_path_buf()))
                .map(super::ReplayFolder::id)
                .expect("the directory has a tab")
        })
    }

    fn install_id(view: &Entity<ReplayInspectorView>, cx: &mut TestAppContext) -> FolderId {
        cx.update(|cx| view.read(cx).folders()[0].id())
    }

    fn listing(view: &Entity<ReplayInspectorView>, root: &Path, cx: &mut TestAppContext) -> Entity<ReplayBrowser> {
        cx.update(|cx| view.read(cx).folder_browser_for_test(root).expect("the directory has a tab").clone())
    }

    fn listed(browser: &Entity<ReplayBrowser>, cx: &mut TestAppContext) -> Vec<PathBuf> {
        cx.update(|cx| browser.read(cx).listed_paths_for_test())
    }

    fn tab(id: FolderId) -> ElementId {
        ("replay-folder", id.element_index()).into()
    }

    /// Opens a tab's menu and says what it offers, each with the row it
    /// answers to.
    fn open_tab_menu(window: WindowHandle<Root>, id: FolderId, cx: &mut TestAppContext) -> Vec<(u64, String)> {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click(tab(id), cx);
        })
        .expect("the test window stays open");
        // The menu opens through a deferred effect.
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            crate::interaction_tests::menu_items(window)
                .into_iter()
                .filter_map(|(row, item)| Some((row, item.label()?.to_owned())))
                .collect()
        })
        .expect("the test window stays open")
    }

    fn choose_from_tab_menu(window: WindowHandle<Root>, id: FolderId, label: &str, cx: &mut TestAppContext) {
        let offered = open_tab_menu(window, id, cx);
        let (row, _) = offered
            .iter()
            .find(|(_, offer)| offer == label)
            .unwrap_or_else(|| panic!("the tab menu offers {label:?}, offered {offered:?}"));
        cx.update_window(window.into(), |_, window, cx| {
            window.within("popup-menu").click(ElementId::Integer(*row), cx);
        })
        .expect("the test window stays open");
        cx.run_until_parked();
    }

    fn shows_pin(window: WindowHandle<Root>, id: FolderId, cx: &mut TestAppContext) -> bool {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.try_find(("replay-folder-pin", id.element_index())).is_some()
        })
        .expect("the test window stays open")
    }

    /// The count a tab's badge shows, while it shows one, once the ring of
    /// the latest landing has spread.
    fn badge(window: WindowHandle<Root>, id: FolderId, cx: &mut TestAppContext) -> Option<String> {
        cx.executor().advance_clock(super::LANDING_PULSE);
        cx.run_until_parked();
        badge_now(window, id, cx)
    }

    /// The count a tab's badge shows at once.
    fn badge_now(window: WindowHandle<Root>, id: FolderId, cx: &mut TestAppContext) -> Option<String> {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window
                .try_find(("replay-folder-badge", id.element_index()))
                .and_then(|badge| badge.label().map(str::to_owned))
        })
        .expect("the test window stays open")
    }

    fn click_tab(window: WindowHandle<Root>, id: FolderId, cx: &mut TestAppContext) {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(tab(id), cx);
        })
        .expect("the test window stays open");
        cx.run_until_parked();
    }

    /// Copies the fixture into `dir` as `name` and reports it to the watches
    /// on `dir`.
    fn land(dir: &Path, name: &str, cx: &mut TestAppContext) -> PathBuf {
        let landed = dir.join(name);
        std::fs::copy(fixture(), &landed).expect("the replay is copied");
        simulated_watch::report(dir, DirChange::Appeared(landed.clone()));
        cx.run_until_parked();
        landed
    }

    /// Writes `value` under `key` in a settings table and reads it back, as
    /// `settings_store::save` writes it and `restore_folders` reads it.
    fn through_settings_table<T>(key: &str, value: &T) -> T
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime");
        runtime
            .block_on(async {
                let pool = wows_toolkit_config::test_pool().await;
                let json = serde_json::to_string(value).expect("the list serializes");
                wows_toolkit_config::queries::set_setting_raw(&pool, key, &json).await.expect("the list is written");
                wows_toolkit_config::queries::try_get_setting::<T>(&pool, key).await.expect("the list is read")
            })
            .expect("the list was stored")
    }

    /// Pinning from a tab's menu marks the tab, watches the directory and
    /// keeps the list in tab order; the stored list reopens the same tabs,
    /// pinned and watched, at the next startup.
    #[gpui_kit::test]
    fn a_pinned_folder_is_kept_and_reopens_at_startup(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let first = tempfile::tempdir().expect("a temp directory");
        let second = tempfile::tempdir().expect("a temp directory");
        let (first, second) = (canonical(first.path()), canonical(second.path()));
        let view = inspector(&app, cx);

        // Nothing was stored, as on a first run.
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders::default(), window, cx);
            view.add_folder(first.clone(), false, window, cx);
            view.add_folder(second.clone(), false, window, cx);
        });
        let (first_id, second_id) = (folder_id(&view, &first, cx), folder_id(&view, &second, cx));
        assert_eq!(simulated_watch::watching(&first), 0, "an unpinned directory is not watched");
        assert!(!shows_pin(window, first_id, cx));

        let pin = t!("ui.replay_folders.pin").into_owned();
        choose_from_tab_menu(window, second_id, &pin, cx);
        choose_from_tab_menu(window, first_id, &pin, cx);

        assert!(shows_pin(window, first_id, cx), "a pinned tab shows its pin");
        assert!(shows_pin(window, second_id, cx));
        assert_eq!(simulated_watch::watching(&first), 1, "a pinned directory is watched");
        assert_eq!(simulated_watch::watching(&second), 1);
        let saved = cx.update(|cx| view.read(cx).pinned_folders());
        assert_eq!(saved, PinnedFolders(vec![first.clone(), second.clone()]), "kept in tab order");

        let restored = through_settings_table(PINNED_FOLDERS, &saved);
        let (restarted_window, restarted) = open_app_in_root(cx);
        let restarted = inspector(&restarted, cx);
        with_view(restarted_window, &restarted, cx, |view, window, cx| {
            view.adopt_pinned_folders(restored, window, cx);
        });

        assert_eq!(roots(&restarted, cx), vec![first.clone(), second.clone()], "reopened in the saved order");
        assert_eq!(cx.update(|cx| restarted.read(cx).pinned_folders()), saved, "and still pinned");
        assert_eq!(simulated_watch::watching(&first), 2, "and watched again");
        assert_eq!(simulated_watch::watching(&second), 2);
        let restored_id = folder_id(&restarted, &first, cx);
        assert!(shows_pin(restarted_window, restored_id, cx));
    }

    /// A change in a pinned directory reaches that directory's listing and no
    /// other, and a change in the install's replays directory never reaches
    /// the pinned one.
    #[gpui_kit::test]
    fn a_watched_change_reaches_only_its_own_folder(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let install = tempfile::tempdir().expect("a temp directory");
        let main_dir = install.path().join("replays");
        std::fs::create_dir(&main_dir).expect("the replays directory is made");
        let pinned = tempfile::tempdir().expect("a temp directory");
        let unpinned = tempfile::tempdir().expect("a temp directory");
        let (pinned, unpinned) = (canonical(pinned.path()), canonical(unpinned.path()));
        let view = inspector(&app, cx);

        let main = cx.update(|cx| view.read(cx).install_browser().clone());
        let wows_dir = install.path().display().to_string();
        with_view(window, &view, cx, |view, _window, cx| view.scan_install(wows_dir, cx));
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![pinned.clone()]), window, cx);
            view.add_folder(unpinned.clone(), false, window, cx);
        });
        let (pinned_listing, unpinned_listing) = (listing(&view, &pinned, cx), listing(&view, &unpinned, cx));
        assert_eq!(simulated_watch::watching(&main_dir), 1, "the install's listing watches its own directory");
        assert_eq!(simulated_watch::watching(&pinned), 1, "only the pinned tab watches the pinned directory");
        assert_eq!(simulated_watch::watching(&unpinned), 0, "an unpinned directory is read once, not watched");

        let landed = land(&pinned, "landed.wowsreplay", cx);
        assert_eq!(listed(&pinned_listing, cx), vec![landed.clone()], "the replay is read into the pinned listing");
        assert!(listed(&main, cx).is_empty(), "the install's listing does not hold it");
        assert!(listed(&unpinned_listing, cx).is_empty(), "nor does another directory's listing");

        let played = land(&main_dir, "played.wowsreplay", cx);
        assert_eq!(listed(&main, cx), vec![played], "the install's listing holds its own battle");
        assert_eq!(listed(&pinned_listing, cx), vec![landed.clone()], "and the pinned listing does not");

        assert_eq!(simulated_watch::report(&pinned, DirChange::Removed(landed)), 1);
        cx.run_until_parked();
        assert!(listed(&pinned_listing, cx).is_empty(), "a removed replay leaves the pinned listing");
    }

    /// Unpinning ends the watch, so nothing that lands afterwards is listed;
    /// pinning again reads what landed in between.
    #[gpui_kit::test]
    fn unpinning_stops_the_updates(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let dir = tempfile::tempdir().expect("a temp directory");
        let dir = canonical(dir.path());
        let view = inspector(&app, cx);

        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![dir.clone()]), window, cx);
        });
        let (id, folder) = (folder_id(&view, &dir, cx), listing(&view, &dir, cx));
        assert_eq!(simulated_watch::watching(&dir), 1);

        choose_from_tab_menu(window, id, &t!("ui.replay_folders.unpin"), cx);
        assert_eq!(simulated_watch::watching(&dir), 0, "the watch ends with the pin");
        assert!(!shows_pin(window, id, cx));
        assert_eq!(cx.update(|cx| view.read(cx).pinned_folders()), PinnedFolders::default());

        let landed = dir.join("landed.wowsreplay");
        std::fs::copy(fixture(), &landed).expect("the replay is copied");
        assert_eq!(simulated_watch::report(&dir, DirChange::Appeared(landed.clone())), 0, "nothing hears it");
        cx.run_until_parked();
        assert!(listed(&folder, cx).is_empty(), "an unpinned directory is not updated");
        assert_eq!(badge(window, id, cx), None, "nor counted");

        choose_from_tab_menu(window, id, &t!("ui.replay_folders.pin"), cx);
        assert_eq!(simulated_watch::watching(&dir), 1);
        assert_eq!(listed(&folder, cx), vec![landed], "pinning again reads what landed while unwatched");
    }

    /// A pinned directory that is gone at startup still gets its tab, says
    /// so, and can be unpinned from where it says so.
    #[gpui_kit::test]
    fn a_missing_pinned_folder_keeps_its_tab_and_can_be_unpinned(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let kept = tempfile::tempdir().expect("a temp directory");
        let kept = canonical(kept.path());
        let gone = tempfile::tempdir().expect("a temp directory");
        let missing = canonical(gone.path());
        drop(gone);
        let view = inspector(&app, cx);

        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![missing.clone(), kept.clone()]), window, cx);
        });
        assert_eq!(roots(&view, cx), vec![missing.clone(), kept.clone()], "the missing directory keeps its tab");
        assert_eq!(simulated_watch::watching(&missing), 0, "there is nothing to watch");
        let id = folder_id(&view, &missing, cx);
        assert!(shows_pin(window, id, cx));

        click_tab(window, id, cx);
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-folder-missing").is_some(), "the listing says the directory is gone");
            window.click("replay-folder-missing-action", cx);
        })
        .expect("the test window stays open");
        cx.run_until_parked();

        assert_eq!(roots(&view, cx), vec![kept.clone()], "unpinning closes the tab");
        assert_eq!(cx.update(|cx| view.read(cx).pinned_folders()), PinnedFolders(vec![kept]));
        assert_eq!(cx.update(|cx| view.read(cx).selected_folder()), install_id(&view, cx), "the install's is shown");
    }

    /// Opening a directory adds its tab and selects it; opening it again,
    /// however its path is written, selects the same tab. The install's
    /// replays directory is its own first tab, which is never added twice,
    /// closed or pinned.
    #[gpui_kit::test]
    fn opening_a_folder_adds_or_selects_one_tab_and_the_install_stays_first(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let dir = tempfile::tempdir().expect("a temp directory");
        let dir = canonical(dir.path());
        let install = tempfile::tempdir().expect("a temp directory");
        let main_dir = install.path().join("replays");
        std::fs::create_dir(&main_dir).expect("the replays directory is made");
        let view = inspector(&app, cx);
        let wows_dir = install.path().display().to_string();
        with_view(window, &view, cx, |view, _window, cx| view.scan_install(wows_dir, cx));
        let install_tab = install_id(&view, cx);

        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders::default(), window, cx);
            view.adopt_folder_history(FolderHistory::default(), cx);
            view.open_folder(dir.clone(), window, cx);
        });
        let id = folder_id(&view, &dir, cx);
        assert_eq!(roots(&view, cx), vec![dir.clone()]);
        assert_eq!(cx.update(|cx| view.read(cx).selected_folder()), id, "the opened directory is shown");

        with_view(window, &view, cx, |view, window, cx| {
            view.select_folder(install_tab, cx);
            view.open_folder(dir.join("."), window, cx);
        });
        assert_eq!(roots(&view, cx), vec![dir.clone()], "one tab per directory");
        assert_eq!(cx.update(|cx| view.read(cx).selected_folder()), id, "and opening it again selects it");

        with_view(window, &view, cx, |view, window, cx| {
            view.open_folder(main_dir.clone(), window, cx);
            view.add_folder(main_dir.clone(), true, window, cx);
        });
        assert_eq!(roots(&view, cx), vec![dir.clone()], "the install's directory gets no second tab");
        assert_eq!(cx.update(|cx| view.read(cx).selected_folder()), install_tab, "opening it selects its own");
        assert_eq!(cx.update(|cx| view.read(cx).pinned_folders()), PinnedFolders::default(), "and it is not pinned");
        let recent = cx.update(|cx| view.read(cx).folder_history().folders().to_vec());
        assert_eq!(recent, vec![dir.clone()], "nor is it a recent directory");

        let offered: Vec<String> = open_tab_menu(window, install_tab, cx).into_iter().map(|(_, label)| label).collect();
        assert_eq!(offered, vec![t!("ui.replay_folders.refresh").into_owned()], "the install's tab only refreshes");
        with_view(window, &view, cx, |view, _window, cx| {
            view.close_folder(install_tab, cx);
            view.pin_folder(install_tab, true, cx);
        });
        assert_eq!(install_id(&view, cx), install_tab, "nor can it be closed");
        assert_eq!(cx.update(|cx| view.read(cx).pinned_folders()), PinnedFolders::default());
    }

    /// A watched directory that is not shown counts the replays added or
    /// changed in it, each once; selecting it clears the count, and the
    /// directory on screen never counts.
    #[gpui_kit::test]
    fn a_badge_counts_unseen_replays_until_the_folder_is_selected(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let busy = tempfile::tempdir().expect("a temp directory");
        let quiet = tempfile::tempdir().expect("a temp directory");
        let (busy, quiet) = (canonical(busy.path()), canonical(quiet.path()));
        let view = inspector(&app, cx);

        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![busy.clone(), quiet.clone()]), window, cx);
        });
        let (busy_id, quiet_id) = (folder_id(&view, &busy, cx), folder_id(&view, &quiet, cx));
        assert_eq!(badge(window, busy_id, cx), None, "nothing has happened yet");

        let first = land(&busy, "first.wowsreplay", cx);
        land(&busy, "second.wowsreplay", cx);
        assert_eq!(simulated_watch::report(&busy, DirChange::Changed(first)), 1);
        cx.run_until_parked();
        assert_eq!(badge(window, busy_id, cx).as_deref(), Some("2"), "two replays, one of them written twice");
        assert_eq!(badge(window, quiet_id, cx), None, "a directory where nothing happened shows none");
        assert_eq!(badge(window, install_id(&view, cx), cx), None);

        click_tab(window, busy_id, cx);
        assert_eq!(cx.update(|cx| view.read(cx).selected_folder()), busy_id);
        assert_eq!(badge(window, busy_id, cx), None, "selecting the directory clears its count");

        land(&busy, "third.wowsreplay", cx);
        assert_eq!(badge(window, busy_id, cx), None, "the directory on screen never counts");
        click_tab(window, quiet_id, cx);
        assert_eq!(badge(window, busy_id, cx), None, "nor counts later what landed while it was shown");
    }

    /// A watched tab carries a dot and an unwatched one does not; a landing
    /// replay changes the count only once its ring has spread.
    #[gpui_kit::test]
    fn the_count_changes_after_the_landing_ring(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let watched = tempfile::tempdir().expect("a temp directory");
        let unwatched = tempfile::tempdir().expect("a temp directory");
        let (watched, unwatched) = (canonical(watched.path()), canonical(unwatched.path()));
        let view = inspector(&app, cx);

        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![watched.clone()]), window, cx);
            view.add_folder(unwatched.clone(), false, window, cx);
        });
        cx.run_until_parked();
        let (watched_id, unwatched_id) = (folder_id(&view, &watched, cx), folder_id(&view, &unwatched, cx));
        let shows_dot = |id: FolderId, cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.try_find(("replay-folder-watched", id.element_index())).is_some()
            })
            .expect("the test window stays open")
        };
        assert!(shows_dot(watched_id, cx), "a pinned directory is watched");
        assert!(shows_dot(install_id(&view, cx), cx), "so is the install's");
        assert!(!shows_dot(unwatched_id, cx), "a directory read once is not");

        land(&watched, "first.wowsreplay", cx);
        assert_eq!(badge(window, watched_id, cx).as_deref(), Some("1"));

        land(&watched, "second.wowsreplay", cx);
        assert_eq!(badge_now(window, watched_id, cx).as_deref(), Some("1"), "the count holds while the ring spreads");
        assert_eq!(badge(window, watched_id, cx).as_deref(), Some("2"), "and changes once it has");
    }

    /// The recently opened directories survive a restart through the
    /// settings, the palette lists them after "Browse...", and choosing one
    /// opens it in the listing; one that is gone is said to be and dropped.
    #[gpui_kit::test]
    fn the_palette_offers_browse_then_recent_folders(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let kept = tempfile::tempdir().expect("a temp directory");
        let kept = canonical(kept.path());
        let gone = tempfile::tempdir().expect("a temp directory");
        let missing = canonical(gone.path());
        drop(gone);
        let view = inspector(&app, cx);

        let mut recent = FolderHistory::default();
        recent.record(missing.clone());
        recent.record(kept.clone());
        let stored = through_settings_table(FOLDER_HISTORY, &recent);
        assert_eq!(stored, recent, "the list reads back as it was written");
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders::default(), window, cx);
            view.adopt_folder_history(stored, cx);
        });

        let open_layer = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                app.update(cx, |app, cx| {
                    app.open_palette_mode(wows_toolkit_viewmodel::palette::PaletteMode::ReplayFolders, window, cx)
                });
            })
            .expect("the test window stays open");
            settle(window, cx);
        };
        let name = |path: &Path| path.file_name().expect("a named directory").to_string_lossy().into_owned();

        open_layer(cx);
        cx.update_window(window.into(), |_, window, cx| {
            let row = |ix: usize| window.find(IndexPath::new(ix)).label().map(str::to_owned);
            assert_eq!(row(0), Some(t!("ui.replay_folders.browse").into_owned()), "Browse comes first");
            assert_eq!(row(1), Some(name(&kept)), "then the most recent");
            assert_eq!(row(2), Some(name(&missing)));
            assert!(window.try_find(IndexPath::new(3)).is_none());
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("the test window stays open");
        settle(window, cx);
        assert_eq!(roots(&view, cx), vec![kept.clone()], "choosing a directory opens it");
        assert_eq!(cx.update(|cx| view.read(cx).selected_folder()), folder_id(&view, &kept, cx));

        let before = crate::toast::said_for_test().len();
        open_layer(cx);
        cx.update_window(window.into(), |_, window, cx| {
            window.press("down", cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .expect("the test window stays open");
        settle(window, cx);
        let said = crate::toast::said_for_test().split_off(before);
        let expected = t!("ui.replay_folders.recent_missing", dir = missing.display().to_string()).into_owned();
        assert!(said.contains(&expected), "a directory that is gone is said to be, said {said:?}");
        assert_eq!(roots(&view, cx), vec![kept.clone()], "and gets no tab");
        let recent = cx.update(|cx| view.read(cx).folder_history().folders().to_vec());
        assert_eq!(recent, vec![kept], "and leaves the recent directories");
    }

    /// A palette re-index covers the install's directory and the pinned ones,
    /// each replay once however many listings hold it. A directory opened once
    /// is left out: its replays may be anyone's.
    #[gpui_kit::test]
    fn a_forced_reindex_covers_the_install_and_pinned_folders_once(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let pinned_dir = tempfile::tempdir().expect("a temp directory");
        let pinned_dir = canonical(pinned_dir.path());
        let opened_dir = tempfile::tempdir().expect("a temp directory");
        let opened_dir = canonical(opened_dir.path());
        let view = inspector(&app, cx);
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![pinned_dir.clone()]), window, cx);
            view.add_folder(opened_dir.clone(), false, window, cx);
        });

        let [install_only, shared, pinned_only] =
            ["install-only", "shared", "pinned-only"].map(|name| pinned_dir.join(format!("{name}.wowsreplay")));
        let opened_only = opened_dir.join("opened-only.wowsreplay");
        let played = "01.01.2026 10:00:00";
        let main = cx.update(|cx| view.read(cx).install_browser().clone());
        main.update(cx, |browser, cx| {
            browser.seed_release_for_test(install_only.clone(), "1.0.0", 100, played, cx);
            browser.seed_release_for_test(shared.clone(), "1.0.0", 100, played, cx);
        });
        listing(&view, &pinned_dir, cx).update(cx, |browser, cx| {
            browser.seed_release_for_test(shared.clone(), "1.0.0", 100, played, cx);
            browser.seed_release_for_test(pinned_only.clone(), "1.0.0", 100, played, cx);
        });
        listing(&view, &opened_dir, cx).update(cx, |browser, cx| {
            browser.seed_release_for_test(opened_only.clone(), "1.0.0", 100, played, cx);
        });

        let mut covered: Vec<PathBuf> =
            cx.update(|cx| view.read(cx).reindex_candidates(cx).into_iter().map(|candidate| candidate.path).collect());
        covered.sort();
        let mut expected = vec![install_only, shared, pinned_only];
        expected.sort();
        assert_eq!(covered, expected);
    }

    /// A pinned directory deleted while open says so on Refresh, and stops
    /// being watched.
    #[gpui_kit::test]
    fn refreshing_a_folder_that_is_gone_says_so(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let gone = tempfile::tempdir().expect("a temp directory");
        let dir = canonical(gone.path());
        let view = inspector(&app, cx);
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![dir.clone()]), window, cx);
        });
        let id = folder_id(&view, &dir, cx);
        assert_eq!(simulated_watch::watching(&dir), 1);

        drop(gone);
        click_tab(window, id, cx);
        choose_from_tab_menu(window, id, &t!("ui.replay_folders.refresh"), cx);

        assert_eq!(simulated_watch::watching(&dir), 0, "a directory that is gone is not watched");
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-folder-missing").is_some(), "the listing says the directory is gone");
        })
        .expect("the test window stays open");
        assert_eq!(cx.update(|cx| view.read(cx).pinned_folders()), PinnedFolders(vec![dir]), "and it stays pinned");
    }

    /// A game directory changed to one whose replays directory already has a
    /// tab takes that tab over: one tab, one watch, and it is no longer a
    /// recent or pinned directory of its own.
    #[gpui_kit::test]
    fn a_new_install_takes_over_the_tab_of_its_replays_directory(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let install = tempfile::tempdir().expect("a temp directory");
        let main_dir = install.path().join("replays");
        std::fs::create_dir(&main_dir).expect("the replays directory is made");
        let main_dir = canonical(&main_dir);
        let view = inspector(&app, cx);

        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders::default(), window, cx);
            view.adopt_folder_history(FolderHistory::default(), cx);
            view.open_folder(main_dir.clone(), window, cx);
            let id = view.folders()[1].id();
            view.pin_folder(id, true, cx);
        });
        assert_eq!(roots(&view, cx), vec![main_dir.clone()]);
        assert_eq!(simulated_watch::watching(&main_dir), 1);

        let wows_dir = install.path().display().to_string();
        with_view(window, &view, cx, |view, _window, cx| view.scan_install(wows_dir, cx));

        assert!(roots(&view, cx).is_empty(), "the install's tab lists it");
        assert_eq!(cx.update(|cx| view.read(cx).selected_folder()), install_id(&view, cx));
        assert_eq!(simulated_watch::watching(&main_dir), 1, "watched once");
        assert_eq!(cx.update(|cx| view.read(cx).pinned_folders()), PinnedFolders::default());
        assert!(cx.update(|cx| view.read(cx).folder_history().folders().is_empty()));
    }

    /// A recent directory found gone before the stored list was read stays
    /// out of it once it is.
    #[gpui_kit::test]
    fn a_recent_folder_dropped_before_the_list_is_read_stays_dropped(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        let gone = tempfile::tempdir().expect("a temp directory");
        let missing = canonical(gone.path());
        drop(gone);
        let view = inspector(&app, cx);

        with_view(window, &view, cx, |view, window, cx| {
            view.open_recent_folder(missing.clone(), window, cx);
            let mut stored = FolderHistory::default();
            stored.record(missing.clone());
            view.adopt_folder_history(stored, cx);
        });

        assert!(cx.update(|cx| view.read(cx).folder_history().folders().is_empty()));
    }

    /// A tab is added before its directory has been looked for, which happens
    /// off the UI thread, and is listed and watched once the check returns.
    /// The palette lists a recent directory as present until its own check
    /// finds it gone.
    #[gpui_kit::test]
    fn directories_are_looked_for_off_the_ui_thread(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let dir = tempfile::tempdir().expect("a temp directory");
        let dir = canonical(dir.path());
        let gone = tempfile::tempdir().expect("a temp directory");
        let missing = canonical(gone.path());
        drop(gone);
        let view = inspector(&app, cx);

        let state = |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).folders()[1].state());
        cx.update_window(window.into(), |_, window, cx| {
            view.update(cx, |view, cx| view.adopt_pinned_folders(PinnedFolders(vec![dir.clone()]), window, cx));
        })
        .expect("the test window stays open");
        assert_eq!(state(cx), super::FolderState::Checking, "the tab is there before its directory is looked for");
        assert_eq!(simulated_watch::watching(&dir), 0, "and nothing is watched yet");

        cx.run_until_parked();
        assert_eq!(state(cx), super::FolderState::Listed);
        assert_eq!(simulated_watch::watching(&dir), 1, "the check's answer starts the watch");

        let mut recent = FolderHistory::default();
        recent.record(missing);
        recent.record(dir);
        with_view(window, &view, cx, |view, _window, cx| view.adopt_folder_history(recent, cx));
        let marked = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                assert!(window.try_find(IndexPath::new(2)).is_some(), "the gone directory is listed");
                window.try_find(("palette-folder-missing", 2_usize)).is_some()
            })
            .expect("the test window stays open")
        };
        cx.update_window(window.into(), |_, window, cx| {
            app.update(cx, |app, cx| {
                app.open_palette_mode(wows_toolkit_viewmodel::palette::PaletteMode::ReplayFolders, window, cx)
            });
        })
        .expect("the test window stays open");
        assert!(!marked(cx), "listed as present until it has been looked for");
        settle(window, cx);
        assert!(marked(cx), "and marked gone once the check returns");
    }

    /// A battle the game finishes in the install's directory opens and is
    /// announced whichever tab is shown, and counts on the install's tab when
    /// another is shown. A replay landing in a pinned directory that is not
    /// shown is only counted.
    #[gpui_kit::test]
    fn the_install_announces_a_new_battle_whichever_tab_is_shown(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let install = tempfile::tempdir().expect("a temp directory");
        let main_dir = install.path().join("replays");
        std::fs::create_dir(&main_dir).expect("the replays directory is made");
        let shown = tempfile::tempdir().expect("a temp directory");
        let other = tempfile::tempdir().expect("a temp directory");
        let (shown, other) = (canonical(shown.path()), canonical(other.path()));
        let view = inspector(&app, cx);
        let wows_dir = install.path().display().to_string();
        with_view(window, &view, cx, |view, window, cx| {
            view.scan_install(wows_dir, cx);
            view.adopt_pinned_folders(PinnedFolders(vec![shown.clone(), other.clone()]), window, cx);
        });
        click_tab(window, folder_id(&view, &shown, cx), cx);

        let before = crate::toast::said_for_test().len();
        let played = land(&main_dir, "played.wowsreplay", cx);
        let main = cx.update(|cx| view.read(cx).install_browser().clone());
        let battle = cx.update(|cx| main.read(cx).battle_label(&played)).expect("the battle is listed").to_string();
        let said = crate::toast::said_for_test().split_off(before);
        let announced = [
            t!("ui.replay.opened_new_battle", battle = battle.as_str()).into_owned(),
            t!("ui.replay.new_battle_listed", battle = battle.as_str()).into_owned(),
        ];
        assert!(said.iter().any(|line| announced.contains(line)), "the battle is announced, said {said:?}");
        assert_eq!(badge(window, install_id(&view, cx), cx).as_deref(), Some("1"), "and counted on its tab");

        let before = crate::toast::said_for_test().len();
        let copied = land(&other, "copied.wowsreplay", cx);
        let said = crate::toast::said_for_test().split_off(before);
        let other_listing = listing(&view, &other, cx);
        let battle =
            cx.update(|cx| other_listing.read(cx).battle_label(&copied)).expect("the battle is listed").to_string();
        let announced = [
            t!("ui.replay.opened_new_battle", battle = battle.as_str()).into_owned(),
            t!("ui.replay.new_battle_listed", battle = battle.as_str()).into_owned(),
        ];
        assert!(!said.iter().any(|line| announced.contains(line)), "a pinned directory's arrival is not announced");
        assert_eq!(badge(window, folder_id(&view, &other, cx), cx).as_deref(), Some("1"), "only counted");
    }

    fn history(names: &[&str]) -> FolderHistory {
        let mut history = FolderHistory::default();
        for name in names.iter().rev() {
            history.record(PathBuf::from(name));
        }
        history
    }

    fn names(history: &FolderHistory) -> Vec<String> {
        history.folders().iter().map(|path| path.display().to_string()).collect()
    }

    /// Opening a directory puts it first, once, and the list keeps the ten
    /// most recent.
    #[test]
    fn the_history_is_most_recent_first_once_each_and_capped() {
        let mut history = FolderHistory::default();
        for name in ["a", "b", "c"] {
            history.record(PathBuf::from(name));
        }
        assert_eq!(names(&history), ["c", "b", "a"]);

        history.record(PathBuf::from("a"));
        assert_eq!(names(&history), ["a", "c", "b"], "a directory opened again moves to the front");

        for index in 0..12 {
            history.record(PathBuf::from(format!("d{index}")));
        }
        assert_eq!(history.folders().len(), 10);
        assert_eq!(names(&history)[0], "d11");
        assert_eq!(names(&history)[9], "d2", "the oldest fall off the end");

        assert!(history.forget(std::path::Path::new("d5")));
        assert!(!history.forget(std::path::Path::new("d5")));
        assert!(!names(&history).contains(&"d5".to_owned()));
    }

    /// A stored list read after some directories were opened goes behind
    /// them, without repeats.
    #[test]
    fn a_stored_history_goes_behind_what_was_opened_first() {
        let mut opened = history(&["new", "shared"]);
        opened.merge_older(history(&["shared", "old"]));

        assert_eq!(names(&opened), ["new", "shared", "old"]);
    }

    /// The stored form is a plain list of paths, as the pinned list is.
    #[test]
    fn the_history_is_stored_as_a_list_of_paths() {
        let stored = history(&["/replays/b", "/replays/a"]);
        let json = serde_json::to_string(&stored).expect("the list serializes");

        assert_eq!(json, r#"["/replays/b","/replays/a"]"#);
        assert_eq!(serde_json::from_str::<FolderHistory>(&json).expect("the list reads back"), stored);
    }

    fn watch_state(browser: &Entity<ReplayBrowser>, cx: &mut TestAppContext) -> WatchState {
        cx.update(|cx| browser.read(cx).watch_state().clone())
    }

    /// Which dot a tab shows: `Some(true)` for a running watch, `Some(false)`
    /// for a stopped one.
    fn dot(window: WindowHandle<Root>, id: FolderId, cx: &mut TestAppContext) -> Option<bool> {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let running = window.try_find(("replay-folder-watched", id.element_index())).is_some();
            let stopped = window.try_find(("replay-folder-watch-stopped", id.element_index())).is_some();
            match (running, stopped) {
                (false, false) => None,
                (true, false) => Some(true),
                (false, true) => Some(false),
                (true, true) => panic!("a tab shows one dot"),
            }
        })
        .expect("the test window stays open")
    }

    /// The messages put up since `before` were.
    fn said_since(before: usize) -> Vec<String> {
        crate::toast::said_for_test().split_off(before)
    }

    /// An error from the install's watcher turns the Game tab's dot red and
    /// says so; the restart a second later reads the directory again, which
    /// lists what landed while the watch was down, and the dot is green again.
    /// A second stop in the same run of failures is not said again.
    #[gpui_kit::test]
    fn a_failed_watch_turns_red_and_restarts_with_a_rescan(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let install = tempfile::tempdir().expect("a temp directory");
        let main_dir = install.path().join("replays");
        std::fs::create_dir(&main_dir).expect("the replays directory is made");
        let view = inspector(&app, cx);
        let main = cx.update(|cx| view.read(cx).install_browser().clone());
        let wows_dir = install.path().display().to_string();
        with_view(window, &view, cx, |view, _window, cx| view.scan_install(wows_dir, cx));
        let id = install_id(&view, cx);
        assert_eq!(dot(window, id, cx), Some(true), "a running watch shows green");
        let before = crate::toast::said_for_test().len();

        simulated_watch::fail(&main_dir, "the event queue overflowed");
        cx.run_until_parked();

        assert_eq!(dot(window, id, cx), Some(false), "a stopped watch shows red");
        assert_eq!(simulated_watch::watching(&main_dir), 0, "the failed watch is let go");
        let WatchState::Stopped { cause: WatchStopCause::Backend { error }, next } = watch_state(&main, cx) else {
            panic!("the watch stopped on the watcher's error")
        };
        assert!(error.contains("the event queue overflowed"));
        assert!(matches!(next, NextStep::Retry { delay, .. } if delay == std::time::Duration::from_secs(1)));
        let said = said_since(before);
        let game = t!("ui.replay_folders.install").into_owned();
        assert_eq!(said.len(), 1, "the stop is said once: {said:?}");
        assert!(said[0].contains(&game) && said[0].contains("the event queue overflowed"), "{said:?}");
        let tooltip = cx.update(|cx| view.read(cx).folder_tooltip(id, cx)).expect("the tab is there");
        assert!(tooltip.contains(&t!("ui.replay_folders.watch_restart_hint").into_owned()), "{tooltip}");

        let landed = main_dir.join("landed.wowsreplay");
        std::fs::copy(fixture(), &landed).expect("the replay is copied");
        cx.executor().advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();

        assert_eq!(watch_state(&main, cx), WatchState::Running);
        assert_eq!(dot(window, id, cx), Some(true), "a restarted watch shows green");
        assert_eq!(simulated_watch::watching(&main_dir), 1);
        assert_eq!(listed(&main, cx), vec![landed], "the restart reads what landed while it was down");
        let resumed = t!("ui.replay_folders.watch_resumed", folder = game).into_owned();
        let said = said_since(before);
        assert!(said.contains(&resumed), "the restart is said: {said:?}");

        simulated_watch::exit(&main_dir);
        cx.run_until_parked();

        let WatchState::Stopped { cause, next } = watch_state(&main, cx) else { panic!("the watch stopped") };
        assert_eq!(cause, WatchStopCause::Exited, "a watcher's thread that ends is a stop");
        assert!(!next.is_announced(), "a stop soon after a restart is the same run of failures");
        assert_eq!(said_since(before).len(), said.len(), "and is not said again");
    }

    /// A watcher that dropped events has its directory read again, so what it
    /// missed is listed.
    #[gpui_kit::test]
    fn a_watcher_that_lost_events_reads_the_directory_again(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        hold_constants_for_fixture();
        let dir = tempfile::tempdir().expect("a temp directory");
        let dir = canonical(dir.path());
        let view = inspector(&app, cx);
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![dir.clone()]), window, cx);
        });
        let folder = listing(&view, &dir, cx);
        let missed = dir.join("missed.wowsreplay");
        std::fs::copy(fixture(), &missed).expect("the replay is copied");

        simulated_watch::overflow(&dir);
        cx.run_until_parked();

        assert_eq!(listed(&folder, cx), vec![missed]);
        assert_eq!(watch_state(&folder, cx), WatchState::Running, "a lost event is not a stop");
        assert_eq!(simulated_watch::watching(&dir), 1);
    }

    /// A watch whose thread panics, and whose restarts fail, is restarted
    /// three times and then left for the reader, who restarts it from the
    /// tab's dot. Only the first stop and the last are said.
    #[gpui_kit::test]
    fn a_panicked_watch_backs_off_then_waits_for_the_reader(cx: &mut TestAppContext) {
        let (window, app) = open_app_in_root(cx);
        simulated_watch::install();
        let dir = tempfile::tempdir().expect("a temp directory");
        let dir = canonical(dir.path());
        let view = inspector(&app, cx);
        with_view(window, &view, cx, |view, window, cx| {
            view.adopt_pinned_folders(PinnedFolders(vec![dir.clone()]), window, cx);
        });
        let (id, folder) = (folder_id(&view, &dir, cx), listing(&view, &dir, cx));
        assert_eq!(simulated_watch::asked(&dir), 1);
        let before = crate::toast::said_for_test().len();

        simulated_watch::refuse(&dir, true);
        simulated_watch::crash(&dir, "poll failed");
        cx.run_until_parked();
        assert!(matches!(
            watch_state(&folder, cx),
            WatchState::Stopped { cause: WatchStopCause::Panicked { .. }, next: NextStep::Retry { .. } }
        ));
        assert_eq!(dot(window, id, cx), Some(false));

        for (seconds, asked) in [(1, 2), (5, 3), (30, 4)] {
            cx.executor().advance_clock(std::time::Duration::from_secs(seconds));
            cx.run_until_parked();
            assert_eq!(simulated_watch::asked(&dir), asked, "restarted after {seconds} s");
        }
        assert!(matches!(watch_state(&folder, cx), WatchState::Stopped { next: NextStep::GaveUp, .. }));
        cx.executor().advance_clock(std::time::Duration::from_secs(60 * 60));
        cx.run_until_parked();
        assert_eq!(simulated_watch::asked(&dir), 4, "no restart follows the last");
        let said = said_since(before);
        assert_eq!(said.len(), 2, "the first stop and the last are said: {said:?}");
        let gave_up = t!("ui.replay_folders.watch_gave_up", attempts = 3).into_owned();
        assert!(said[1].ends_with(&gave_up), "{said:?}");

        choose_from_tab_menu(window, id, &t!("ui.replay_folders.restart_watch"), cx);
        assert_eq!(simulated_watch::asked(&dir), 5, "the menu restarts it");
        assert!(
            matches!(watch_state(&folder, cx), WatchState::Stopped { next, .. } if next.is_announced() && next != NextStep::GaveUp),
            "with the full set of restarts behind it again"
        );
        assert_eq!(said_since(before).len(), 3, "and its failure is said");

        simulated_watch::refuse(&dir, false);
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("replay-folder-watch-stopped", id.element_index()), cx);
        })
        .expect("the test window stays open");
        cx.run_until_parked();

        assert_eq!(watch_state(&folder, cx), WatchState::Running, "the dot restarts it");
        assert_eq!(dot(window, id, cx), Some(true));
        assert_eq!(simulated_watch::watching(&dir), 1);
        let resumed =
            t!("ui.replay_folders.watch_resumed", folder = wows_toolkit_replay_data::paths::folder_name(&dir))
                .into_owned();
        assert_eq!(said_since(before).len(), 4);
        assert_eq!(said_since(before).last(), Some(&resumed));
    }
}
