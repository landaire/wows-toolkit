//! Top-level Replay Inspector tab: the file browser (`browser_view.rs`) in a
//! resizable sidebar next to a `DockArea` holding one `ReplayPanel` tab per
//! open replay. Double-clicking a replay in the browser
//! (`ReplayBrowserEvent::OpenReplay`) starts a background parse and adds a
//! tab; the tab itself shows "Loading..." until the parse completes (see
//! `panel.rs`). A repeat double-click on an already-open replay brings its
//! tab forward rather than adding a second one (see `open_replay`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::DockArea;
use gpui_kit::component::dock::DockPlacement;
use gpui_kit::component::dock::DockSkin;
use gpui_kit::component::dock::PaneRef;
use gpui_kit::component::dock::PanelId;
use gpui_kit::component::dock::panel_handle;
use gpui_kit::component::h_flex;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::resizable::h_resizable;
use gpui_kit::component::resizable::resizable_panel;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::Select;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::ReplaySettings;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

use super::browser_view::MissingBuild;
use super::browser_view::ReplayBrowser;
use super::browser_view::ReplayBrowserEvent;
use super::columns::default_columns;
use super::load::GameDataCache;
use super::load::GameDataStatus;
use super::load::spawn_parse;
use super::load::spawn_startup_preload;
use super::panel::AutoExport;
use super::panel::PanelSetup;
use super::panel::RenderRequested;
use super::panel::ReplayPanel;
use crate::replay_renderer::RendererEvent;
use crate::replay_renderer::ReplayRendererPanel;
use gpui_kit::component::Disableable;
use gpui_kit::component::input::InputState;

/// Identifies the message a batch render keeps on screen while it runs, so each
/// step replaces the last rather than stacking.
const BATCH_PROGRESS: &str = "replay-batch-render";

/// The same, for the pass that contributes every listed battle.
const CONTRIBUTE_PROGRESS: &str = "replay-contribute-all";

/// Identifies the message an auto-export failure keeps up, so a directory of
/// battles that all fail says so once.
const AUTO_EXPORT_FAILED: &str = "replay-auto-export-failed";

/// The same, for the notice that a battle was read through a mapping that does
/// not fit, which stands until one that does is on disk.
const CONSTANTS_UNFIT: &str = "replay-constants-unfit";

/// Sidebar width for the file browser, matching the egui app's left panel.
const BROWSER_WIDTH: Pixels = px(280.);
const BROWSER_MIN_WIDTH: Pixels = px(180.);
const BROWSER_MAX_WIDTH: Pixels = px(520.);

/// One entry in the grouping combo box. A local newtype because both
/// `ReplayGrouping` and `SearchableListItem` are foreign, and because
/// carrying the enum keeps the confirm handler off a string round trip.
#[derive(Clone)]
struct GroupingItem(ReplayGrouping);

impl SearchableListItem for GroupingItem {
    type Value = ReplayGrouping;

    fn title(&self) -> SharedString {
        SharedString::from(t!(grouping_label_key(self.0)).into_owned())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }
}

/// The catalogue key for a grouping's name, so the combo reads in the reader's
/// language as the egui app's own menu does (`ui/replay_parser/mod.rs:4301`).
/// `ReplayGrouping::label` is English, which is what the egui *selected* text
/// still shows.
/// Where a batch render's files go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BatchTarget {
    /// A directory the reader is asked for when the batch starts.
    Directory,
    /// A temporary directory, whose files go on the clipboard afterwards.
    Clipboard,
}

/// Puts the rendered videos on the clipboard, or says why they are not there.
fn copy_rendered_files(written: &[PathBuf]) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set().file_list(written))
        .map_err(|err| err.to_string())
}

const fn grouping_label_key(grouping: ReplayGrouping) -> &'static str {
    match grouping {
        ReplayGrouping::Date => "ui.replay.group.date",
        ReplayGrouping::Ship => "ui.replay.group.ship",
        ReplayGrouping::None => "ui.replay.group.none",
    }
}

/// Combo-box order, matching the egui app's `selectable_value` order.
const GROUPINGS: [ReplayGrouping; 3] = [ReplayGrouping::Date, ReplayGrouping::Ship, ReplayGrouping::None];

fn grouping_index(grouping: ReplayGrouping) -> usize {
    GROUPINGS.iter().position(|g| *g == grouping).expect("GROUPINGS lists every ReplayGrouping variant")
}

pub struct ReplayInspectorView {
    browser: Entity<ReplayBrowser>,
    /// The collaborative session, hosted or joined from this tab's header.
    collab: crate::collab::CollabState,
    /// The two fields the session popover writes into, held so a name typed
    /// once survives the popover closing.
    collab_name: Entity<InputState>,
    collab_token: Entity<InputState>,
    dock_area: Entity<DockArea>,
    /// `None` until `apply_settings` learns the WoWs directory; opening a
    /// replay before then is a no-op (the browser has nothing to
    /// double-click yet either, since its scan needs the same directory).
    game_data: Option<GameDataCache>,
    /// Startup preload of the current installed build's game data (see
    /// `load::spawn_startup_preload`), kicked off from `apply_settings` once
    /// the WoWs directory is known. `Loading` before that (including before
    /// settings arrive at all); a later replay open still works while this
    /// is `Loading` or `Failed` -- `spawn_parse` loads its own build on
    /// demand either way -- this only lets an already-warm build skip the
    /// wait.
    game_data_status: GameDataStatus,
    /// Set once the first replay is opened. Approximates "the dock has at
    /// least one tab" for the empty-state message without reaching into
    /// `DockArea`'s private layout fields; it does not clear if every tab is
    /// later closed, so the empty-state message can under-fire in that edge
    /// case. Acceptable for this milestone: closing tabs back to zero and
    /// re-showing the placeholder is not part of the brief.
    has_opened_replay: bool,
    /// Live replay panels keyed by the path they were opened from, so a
    /// repeat open on an already-open replay can be deduped instead of
    /// adding a second tab for it. Entries survive their panel's tab being
    /// closed until the next open for that same path notices the weak
    /// handle no longer upgrades and replaces the entry.
    open_panels: HashMap<PathBuf, WeakEntity<ReplayPanel>>,
    /// The replay this view last opened or brought forward, which decides
    /// which tab a plain open replaces when more than one is showing.
    current_replay: Option<PathBuf>,
    /// The playback viewports open, one per replay, for the same reason
    /// `open_panels` exists: a second ask brings the tab forward.
    open_renderers: HashMap<PathBuf, WeakEntity<ReplayRendererPanel>>,
    /// The directories open as tabs of their own, so one is brought forward
    /// rather than listed twice.
    open_workspaces: HashMap<PathBuf, WeakEntity<super::workspace::ReplayWorkspace>>,
    /// The install, kept because a workspace's listing needs it: "Open in Game"
    /// launches the executable beside it whatever directory is listed.
    install_dir: String,
    workspace_events: Vec<Subscription>,
    /// Held so a viewport's request for its own window still reaches this
    /// view; a dropped subscription is a silent button.
    renderer_events: Vec<Subscription>,
    /// Held for each open replay panel, so its Actions menu can reach this
    /// view. Swept with the panels themselves.
    panel_events: Vec<Subscription>,
    /// Whether the viewports have been told there is a session. Only a change
    /// is worth pushing, and this is read every draw.
    session_shared: bool,
    /// The expected-values table every replay tab rates its players against,
    /// loaded once per session beside the Stats tab's copy (`App::
    /// apply_session_stats`). Held here rather than fetched per tab so a
    /// replay opened before it arrives can still be filled in afterward by
    /// `set_personal_rating`.
    personal_rating: Option<Arc<PersonalRatingData>>,
    /// Session debug-mode flag: seeded from `AppPreferences.debug_mode` (the
    /// shared config DB) in `apply_settings`, then flippable at runtime via
    /// `App`'s global Ctrl+Shift+D shortcut (`set_debug_mode`, called from
    /// `app.rs`), which also pushes the new value into every currently open
    /// `ReplayPanel` -- not just panels opened afterward. This crate never
    /// writes settings back to the DB (see `settings.rs`'s module doc), so
    /// the toggle only overrides the setting for the running session.
    debug_mode: bool,
    /// Session-local copy of the persisted `ReplaySettings`, seeded from the
    /// shared config DB in `apply_settings`. The header toolbar's
    /// column-filter checkboxes read/write this and drive `default_columns`
    /// off it (`set_column_filter`). The live grouping selection lives on
    /// `browser`; `set_grouping` keeps this copy and the stored row in step
    /// with it.
    replay_settings: ReplaySettings,
    /// What the reader agreed to share, which decides what a finished battle
    /// contributes. Seeded from the row the Settings tab writes.
    data_sharing: wows_toolkit_viewmodel::settings::DataSharingMode,
    /// `AppPreferences.auto_load_latest_replay` in the egui app: seeded from
    /// the shared config DB in `apply_settings`, then flippable at runtime via
    /// the header checkbox, and read by the directory watcher when a replay
    /// lands. The flip is not written back to the DB, so it lasts the session
    /// (`docs/gpui-port-gaps.md`).
    auto_load_latest_replay: bool,
    /// Backing state for the header's grouping combo box. The live grouping
    /// lives on `browser`; this mirrors it so the closed combo shows the
    /// current value.
    grouping_select: Entity<SelectState<SearchableVec<GroupingItem>>>,
    /// The builds whose result mapping has already been asked about this
    /// session, so a second scan does not ask again.
    constants_asked: std::collections::BTreeSet<u32>,
    /// The builds already reported as read through the wrong mapping, so a second
    /// replay on the same build does not say it again.
    constants_mismatched: std::collections::BTreeSet<u32>,
    /// The bulk contribution now running, if one is: a second pass over the same
    /// directory would send everything twice.
    contributing: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// What the replay inspector takes from the app's settings.
pub struct InspectorSettings {
    pub wows_dir: String,
    /// Where the dumped builds are kept, so a replay from a build that is no
    /// longer installed can still be read. Empty is the default location.
    pub game_data_cache_dir: String,
    /// What the reader agreed to share: nothing, their builds, or the replays.
    pub data_sharing: wows_toolkit_viewmodel::settings::DataSharingMode,
    /// Whether a build loaded out of the install is written to that cache.
    pub auto_dump_game_data: bool,
    pub debug_mode: bool,
    pub replay_settings: ReplaySettings,
    pub auto_load_latest_replay: bool,
    /// The locale the listing's figures are grouped in.
    pub locale: Option<String>,
    /// The name a session appears under, as the Settings tab stores it. The
    /// popover writes the same row back, so a name set in either holds in
    /// both.
    pub collab_display_name: String,
}

/// A replay setting this tab owns was changed here, so the app writes the
/// row back. Carries the whole blob because that is how it is stored.
pub struct ReplaySettingsChanged(pub ReplaySettings);

/// The session started or stopped, for whoever owns a window this view does
/// not: the tactics board is the app's, and it draws in the session too.
pub struct SessionShared(pub crate::collab::CollabLink);

/// The collab popover's Tactics Board button. The board is the app's window, so
/// the ask goes up rather than opening anything here.
pub struct TacticsBoardRequested;

/// A workspace asked for its own directory to be searched.
///
/// Raised to the app, which owns the Search tab the query is run in.
pub struct SearchDirectory(pub PathBuf);

/// The listing holds replays from builds nothing on this machine can read.
///
/// Raised to the app, which owns the game-data cache and the jobs that fetch
/// into it.
pub struct GameDataMissing(pub Vec<MissingBuild>);

/// A battle was read through a mapping that does not fit its build.
///
/// Raised to the app, which holds the commit the published mappings were last
/// checked at: forgetting it is what makes the next check fetch again rather
/// than report the repository unchanged.
pub struct ConstantsUnfit;

/// A replay tab that is the one showing in its dock group.
#[derive(Clone)]
struct ShowingReplay {
    path: PathBuf,
    panel: PanelId,
}

/// Where an open puts the replay it was asked for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenTarget {
    /// In place of the replay tab already showing.
    ShowingTab,
    NewTab,
}

impl EventEmitter<ReplaySettingsChanged> for ReplayInspectorView {}
impl EventEmitter<GameDataMissing> for ReplayInspectorView {}
impl EventEmitter<SearchDirectory> for ReplayInspectorView {}
impl EventEmitter<SessionShared> for ReplayInspectorView {}
impl EventEmitter<TacticsBoardRequested> for ReplayInspectorView {}
impl EventEmitter<ConstantsUnfit> for ReplayInspectorView {}

/// A viewport asked for an armor viewer on one of the battle's ships, with
/// what that ship had taken by where playback is.
pub struct ShowArmorRequested {
    pub param_index: String,
    pub display_name: String,
    pub hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
    /// Who was firing at that ship, for the incoming-fire log.
    pub incoming: Box<crate::replay_renderer::IncomingContext>,
}

impl EventEmitter<ShowArmorRequested> for ReplayInspectorView {}

/// Playback moved and the followed ship has taken different hits by now.
pub struct ArmorFollowed {
    /// Where playback has reached, which the health strip marks.
    pub at: wows_replays::types::GameClock,
    pub hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
    pub health: Option<f32>,
}

impl EventEmitter<ArmorFollowed> for ReplayInspectorView {}

impl ReplayInspectorView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let browser = cx.new(ReplayBrowser::new);
        // The skinned area is what draws a tab bar over a group holding more
        // than one panel; a bare one renders only the displayed panel.
        let (dock_area, _) = DockSkin::dock_area("replay-inspector-dock", None, window, cx);
        let subscription = cx.subscribe_in(&browser, window, Self::on_browser_event);

        let items = SearchableVec::new(GROUPINGS.map(GroupingItem).to_vec());
        let grouping_select = cx.new(|cx| {
            SelectState::new(items, Some(IndexPath::new(grouping_index(ReplayGrouping::default()))), window, cx)
                .searchable(false)
        });
        // `Confirm(None)` is the cleared-selection case, which this combo
        // cannot produce: it is not `.cleanable()` and always holds a value.
        let grouping_subscription = cx.subscribe_in(&grouping_select, window, |this, _state, event, window, cx| {
            let SelectEvent::Confirm(Some(grouping)) = event else {
                return;
            };
            this.set_grouping(*grouping, window, cx);
        });

        Self {
            data_sharing: wows_toolkit_viewmodel::settings::DataSharingMode::default(),
            browser,
            collab: crate::collab::CollabState::default(),
            collab_name: cx
                .new(|cx| InputState::new(window, cx).placeholder(t!("ui.collab.display_name_hint").to_string())),
            collab_token: cx.new(|cx| InputState::new(window, cx).placeholder("toolkit-...".to_string())),
            dock_area,
            game_data: None,
            game_data_status: GameDataStatus::Loading { version: None },
            has_opened_replay: false,
            open_panels: HashMap::new(),
            current_replay: None,
            open_renderers: HashMap::new(),
            open_workspaces: HashMap::new(),
            install_dir: String::new(),
            workspace_events: Vec::new(),
            renderer_events: Vec::new(),
            panel_events: Vec::new(),
            session_shared: false,
            personal_rating: None,
            debug_mode: false,
            replay_settings: ReplaySettings::default(),
            auto_load_latest_replay: true,
            grouping_select,
            constants_asked: std::collections::BTreeSet::new(),
            constants_mismatched: std::collections::BTreeSet::new(),
            contributing: None,
            _subscriptions: vec![subscription, grouping_subscription],
        }
    }

    /// Starts the browser's directory scan, (re)builds the game-data cache
    /// for `wows_dir`, seeds the session debug-mode flag from
    /// `debug_mode` (`AppPreferences.debug_mode`), seeds the session
    /// `replay_settings`/`auto_load_latest_replay` flags and the browser's
    /// initial grouping (`replay_settings.grouping`), and kicks off the
    /// startup preload of the current installed build through that same cache
    /// -- so a later `spawn_parse` for a replay on that build (see
    /// `panel.rs`) finds the slot already warm instead of reloading it.
    /// Adopts a language without touching anything else.
    ///
    /// Separate from `apply_settings`: that one rebuilds the game-data cache
    /// and rescans the directory, which a language change has no reason to
    /// do, and which would strand every open panel on the discarded cache.
    pub fn set_locale(&mut self, locale: Option<String>, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.set_locale(locale, cx));
        cx.notify();
    }

    /// Called from `App::apply_settings`, which `main.rs` runs inside a
    /// `window.update`, so a `Window` is available for the grouping combo.
    pub fn apply_settings(&mut self, settings: InspectorSettings, window: &mut Window, cx: &mut Context<Self>) {
        let InspectorSettings {
            wows_dir,
            game_data_cache_dir,
            auto_dump_game_data,
            data_sharing,
            debug_mode,
            replay_settings,
            auto_load_latest_replay,
            locale,
            collab_display_name,
        } = settings;
        // Seeded rather than overwritten: a name typed into the popover and
        // not yet saved is the reader's most recent word on it.
        if self.collab.display_name.is_empty() {
            self.collab.display_name = collab_display_name.clone();
            self.collab_name.update(cx, |state, cx| state.set_value(collab_display_name, window, cx));
        }
        self.browser.update(cx, |browser, cx| {
            browser.set_locale(locale, cx);
            // The listing's second line is what the index knows about each
            // file, so it is read once the directory is known.
            browser.load_summaries(cx);
        });
        self.debug_mode = debug_mode;
        self.data_sharing = data_sharing;
        self.auto_load_latest_replay = auto_load_latest_replay;
        let grouping = replay_settings.grouping;
        self.replay_settings = replay_settings;
        self.adopt_grouping(grouping, window, cx);

        self.install_dir = wows_dir.clone();
        if wows_dir.is_empty() {
            self.game_data = None;
            self.game_data_status = GameDataStatus::Failed(t!("ui.messages.wows_dir_not_set").into_owned());
            let status = self.game_data_status.clone();
            self.browser.update(cx, |browser, cx| {
                browser.start_scan(wows_dir, cx);
                browser.set_game_data(&status, cx);
                browser.set_build_cache(None);
            });
            return;
        }

        // The kept preview renderers hold art read out of the previous
        // install's VFS, which this directory is replacing.
        crate::minimap_preview::forget_renderers();
        let game_data = GameDataCache::new(PathBuf::from(&wows_dir))
            .with_cache_dir(&game_data_cache_dir)
            .with_auto_dump(auto_dump_game_data);
        // The per-build caches for builds nothing can open any more, which is
        // gigabytes after a year of game updates.
        {
            let wows_dir = PathBuf::from(&wows_dir);
            let dump_base = wows_toolkit_config::game_data_dump_base_with_override(&game_data_cache_dir);
            cx.background_spawn(async move {
                super::load::prune_stale_caches(&wows_dir, dump_base.as_deref());
            })
            .detach();
        }
        self.game_data = Some(game_data.clone());
        // An open tab reads its own copy, which carries the directory and the
        // dump preference by value and would otherwise stay on the old install.
        for panel in self.open_panels.values() {
            let _ = panel.update(cx, |panel, _cx| panel.set_game_data(game_data.clone()));
        }
        // Named from the install's own `preferences.xml`, which is a file read
        // rather than a build load, so the listing can say which version it is
        // waiting for while the load runs.
        self.game_data_status = GameDataStatus::Loading {
            version: super::load::installed_version(std::path::Path::new(&wows_dir))
                .map(|version| format!("{}.{}.{}", version.major, version.minor, version.patch)),
        };
        let status = self.game_data_status.clone();
        self.browser.update(cx, |browser, cx| {
            browser.start_scan(wows_dir.clone(), cx);
            browser.set_game_data(&status, cx);
            // The listing's hover previews bake against whichever build a
            // replay was recorded on, which is what this cache loads.
            browser.set_build_cache(Some(game_data.clone()));
        });

        let preload = spawn_startup_preload(PathBuf::from(&wows_dir), game_data, cx);
        cx.spawn_in(window, async move |this, cx| {
            let status = preload.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.game_data_status = status.clone();
                let browser = this.browser.clone();
                browser.update(cx, |browser, cx| browser.set_game_data(&status, cx));
                // What the egui app reports when the same load finishes
                // (`app.rs`'s `GameDataLoaded`): the install is readable, and
                // separately whether it has any replays to show.
                match &status {
                    GameDataStatus::Ready(_) => {
                        crate::toast::ok(t!("ui.messages.game_data_loaded").to_string(), window, cx);
                        if this.browser.read(cx).is_empty() {
                            crate::toast::warn(t!("ui.messages.no_replays_detected").to_string(), window, cx);
                        }
                    }
                    GameDataStatus::Failed(reason) => {
                        crate::toast::failed(reason.clone(), window, cx);
                    }
                    GameDataStatus::Loading { .. } => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The current game-data preload status. Read by `App` so the Armor
    /// Viewer tab can adopt the SAME `Arc<LoadedGameData>` once it reaches
    /// `Ready` -- see `armor_viewer::pane::ArmorViewerPane::load_game_data` --
    /// rather than running a second `GameDataCache`/VFS/`GameParams` load for
    /// the same build.
    pub fn game_data_status(&self) -> GameDataStatus {
        self.game_data_status.clone()
    }

    fn on_browser_event(
        &mut self,
        _browser: &Entity<ReplayBrowser>,
        event: &ReplayBrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ReplayBrowserEvent::OpenReplay(path) => self.open_replay(path.clone(), window, cx),
            ReplayBrowserEvent::OpenReplayInNewTab(path) => self.open_replay_in_new_tab(path.clone(), window, cx),
            ReplayBrowserEvent::SessionStats { paths, replace } => {
                self.record_session_stats(paths.clone(), *replace, cx)
            }
            ReplayBrowserEvent::RenderReplay(path) => self.render_replay(path.clone(), window, cx),
            ReplayBrowserEvent::BuildsListed(builds) => {
                self.fetch_missing_constants(builds.clone(), crate::constants::Cached::Keep, window, cx)
            }
            ReplayBrowserEvent::RenderManyToVideo(paths) => self.render_many_to_video(paths.clone(), window, cx),
            ReplayBrowserEvent::RenderManyToClipboard(paths) => {
                self.render_many_to_clipboard(paths.clone(), window, cx)
            }
            // The game has just finished a match. The egui app opens it
            // straight away when this is on, which is what the checkbox
            // promises.
            ReplayBrowserEvent::ReplayAppeared(path) => {
                // Written whether or not it is opened: that is what "export
                // every battle" means, and the egui app writes it from the
                // background parser for the same reason.
                self.auto_export_landed(path, window, cx);
                self.index_landed(path, cx);
                self.contribute_landed(path, cx);
                if self.auto_load_latest_replay {
                    self.open_replay(path.clone(), window, cx);
                }
            }
            // The file under an open tab has changed, so that tab is reading
            // something that is no longer on disk.
            ReplayBrowserEvent::ReplayChanged(path) => self.reparse_open_replay(path, window, cx),
            ReplayBrowserEvent::WatchFailed(failure) => crate::toast::warn(failure.said(), window, cx),
            // The app owns the cache and the jobs that fill it, so the offer to
            // fetch these builds is raised to it.
            ReplayBrowserEvent::BuildsMissing(missing) => cx.emit(GameDataMissing(missing.clone())),
        }
    }

    /// Adopts what the reader agreed to share, so a battle landing after the
    /// change is contributed under the new setting rather than the old one.
    pub(crate) fn set_data_sharing(&mut self, mode: wows_toolkit_viewmodel::settings::DataSharingMode) {
        self.data_sharing = mode;
    }

    /// Fetches the result mapping for any listed build that has none.
    ///
    /// Battle results are read through a per-build mapping; without it the figures
    /// a replay reports are read through whatever mapping was last written, which
    /// is what the egui app fetches per build to avoid
    /// (`task/networking.rs`'s `FetchVersionedConstants`). Small files, so they are
    /// fetched without asking, and each build is asked about once a session.
    fn fetch_missing_constants(
        &mut self,
        builds: Vec<(u32, Option<String>)>,
        cached: crate::constants::Cached,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // What is already cached, and what this session has already had an answer
        // about, is not asked again. A build a sweep never reached stays unasked, so
        // the next scan picks it up.
        let wanted: Vec<(u32, Option<String>)> = builds
            .into_iter()
            .filter(|(build, _)| !self.constants_asked.contains(build))
            .filter(|(build, _)| cached == crate::constants::Cached::Replace || !crate::constants::is_cached(*build))
            .collect();
        if wanted.is_empty() {
            return;
        }

        cx.spawn_in(window, async move |this, cx| {
            let Ok(sweep) = cx.update(|_window, cx| crate::constants::fetch_for_builds(wanted, cached, cx)) else {
                return;
            };
            let outcomes = sweep.await;

            let mut written = 0usize;
            let mut answered = Vec::new();
            for (build, outcome) in outcomes {
                match outcome {
                    crate::constants::Fetched::Written { build, actual } => {
                        tracing::info!(build, actual, "constants: the result mapping was fetched");
                        written += 1;
                        answered.push(build);
                    }
                    crate::constants::Fetched::AlreadyOnDisk => answered.push(build),
                    // Left unanswered deliberately: a repository that refused once
                    // is worth asking again on a later scan, where marking it here
                    // would need a restart to retry.
                    crate::constants::Fetched::Failed(reason) => {
                        tracing::warn!(build, %reason, "constants: the result mapping was not fetched");
                    }
                }
            }

            let _ = this.update(cx, |this, _cx| this.constants_asked.extend(answered));
            if written == 0 {
                return;
            }
            // Withdrawn now a mapping that fits is on disk: the battles read
            // through the old one are read again below.
            let _ = this.update_in(cx, |this, window, cx| {
                this.constants_mismatched.clear();
                crate::toast::resolved(CONSTANTS_UNFIT, window, cx);
            });
            // What is open was read through the mapping that was there before, so
            // it is read again now there is a better one.
            let _ = this.update_in(cx, |this, window, cx| {
                crate::toast::info(t!("ui.replay.constants_written", count = written).into_owned(), window, cx);
                this.reparse_open_replays(window, cx);
            });
        })
        .detach();
    }

    /// The build the game data now loaded is for.
    ///
    /// `None` until a build is loaded, which is what refuses an import: a mapping
    /// is cached under a build, and there is no build to cache it under.
    fn loaded_build(&self) -> Option<u32> {
        let GameDataStatus::Ready(loaded) = &self.game_data_status else { return None };
        Some(loaded.build())
    }

    /// Says that a battle was read through the wrong mapping, and fetches the
    /// right one.
    ///
    /// What is on disk is fetched over rather than dropped first: a mapping the
    /// reader imported by hand is the only one some builds will ever have, and a
    /// fetch that fails would leave nothing in its place. The message stays up
    /// until one arrives, as the egui app keeps its own up
    /// (`app.rs`'s `check_constants_version_mismatch`).
    fn recover_constants(&mut self, build: u32, version: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.constants_mismatched.insert(build) {
            return;
        }
        crate::toast::stuck(CONSTANTS_UNFIT, t!("ui.messages.constants_version_mismatch").into_owned(), window, cx);
        cx.emit(ConstantsUnfit);
        // Asked about again, whatever this session already asked: the answer it
        // got is the mapping that does not fit.
        self.constants_asked.remove(&build);
        self.fetch_missing_constants(vec![(build, version)], crate::constants::Cached::Replace, window, cx);
    }

    /// Moves the playback that is feeding an armor viewer to `clock`.
    ///
    /// Only the viewport that is feeding one: another playback open beside it is
    /// showing a different battle, or the same one somewhere else on purpose.
    pub(crate) fn seek_following_playback(
        &mut self,
        clock: wows_replays::types::GameClock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for panel in self.open_renderers.values() {
            let Some(panel) = panel.upgrade() else { continue };
            if !panel.read(cx).is_following_armor() {
                continue;
            }
            panel.update(cx, |panel, cx| panel.go_to_clock(clock, window, cx));
            return;
        }
    }

    /// Reads every open replay again, for a mapping or a dump that has since
    /// arrived.
    pub(crate) fn reparse_open_replays(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open: Vec<PathBuf> = self.open_panels.keys().cloned().collect();
        for path in open {
            self.reparse_open_replay(&path, window, cx);
        }
    }

    /// Takes a mapping the reader points at as the loaded build's own.
    pub(crate) fn import_constants(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(build) = self.loaded_build() else {
            crate::toast::warn(t!("ui.replay.constants_no_build").into_owned(), window, cx);
            return;
        };
        let asked = crate::dialog::pick_file(
            Some(&t!("ui.replay.constants_import_title")),
            Some(crate::dialog::Filter { label: "JSON", extensions: &["json"] }),
        );

        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = asked.await else { return };
            let imported = crate::constants::import(&path, build);
            let _ = this.update_in(cx, |this, window, cx| match imported {
                Ok(()) => {
                    crate::toast::ok(t!("ui.replay.constants_imported", build = build).into_owned(), window, cx);
                    this.reparse_open_replays(window, cx);
                }
                Err(reason) => crate::toast::failed(
                    t!("ui.replay.constants_import_failed", reason = reason).into_owned(),
                    window,
                    cx,
                ),
            });
        })
        .detach();
    }

    /// Reads the listed directory again, for game data that has since arrived.
    pub(crate) fn relist(&mut self, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.relist(cx));
    }

    /// Lists a directory the reader picks, instead of the install's own.
    ///
    /// Opened as a workspace of its own, as the egui app opens one: a tab per
    /// directory, titled by its root and closeable, so an archive sits beside
    /// the install's own listing rather than replacing it. Everything else --
    /// previews, opening, indexing -- works on it as it does on the install's.
    pub(crate) fn open_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_folder(&t!("ui.replay.open_directory"));
        cx.spawn_in(window, async move |this, cx| {
            let Some(directory) = asked.await else { return };
            let _ = this.update_in(cx, |this, window, cx| this.open_workspace(directory, window, cx));
        })
        .detach();
    }

    /// Lists `root` in a tab of its own.
    ///
    /// A directory already open is brought forward rather than listed twice:
    /// two tabs on one directory would each watch it and each report the same
    /// replay appearing.
    pub(crate) fn open_workspace(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.open_workspaces.get(&root).and_then(|panel| panel.upgrade()) {
            let id = PanelId::from(open.entity_id());
            self.dock_area.update(cx, |dock_area, cx| dock_area.select_panel(id, window, cx));
            cx.notify();
            return;
        }

        let wows_dir = self.install_dir.clone();
        let workspace = cx.new(|cx| super::workspace::ReplayWorkspace::new(root.clone(), wows_dir, cx));
        self.workspace_events.push(cx.subscribe_in(&workspace, window, Self::on_workspace_event));
        self.open_workspaces.insert(root.clone(), workspace.downgrade());
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.add_panel_view(panel_handle(workspace), DockPlacement::Center, None, window, cx);
        });
        crate::toast::info(
            t!("ui.replay.reading_directory", dir = root.display().to_string()).into_owned(),
            window,
            cx,
        );
        cx.notify();
    }

    /// A workspace tab asked for something.
    ///
    /// Its listing's own events are handled the way the sidebar's are, so a
    /// replay opens from an archive exactly as it does from the install.
    fn on_workspace_event(
        &mut self,
        _workspace: &Entity<super::workspace::ReplayWorkspace>,
        event: &super::workspace::WorkspaceEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            super::workspace::WorkspaceEvent::Listing(listing) => {
                let listing = listing.clone();
                // Through the same handler the sidebar's listing goes through,
                // which is what keeps one behaviour for both.
                self.on_browser_event(&self.browser.clone(), &listing, window, cx);
            }
            super::workspace::WorkspaceEvent::SearchThese(root) => cx.emit(SearchDirectory(root.clone())),
        }
        // A tab the reader closed leaves a dead handle behind, which would
        // otherwise refuse to open that directory again.
        self.open_workspaces.retain(|_, panel| panel.upgrade().is_some());
    }

    /// Writes a video of each marked battle into a directory the reader picks.
    ///
    /// One background batch rather than one viewport per replay: the menu item
    /// says "Render N Replays to Video", and N open tabs baking tracks nobody
    /// watches is not that. Mirrors the egui app's own batch
    /// (`replay/renderer/video_export.rs:458`), which also writes into a chosen
    /// folder.
    fn render_many_to_video(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.render_many(paths, BatchTarget::Directory, window, cx);
    }

    /// Renders each marked battle and puts the files on the clipboard.
    ///
    /// The egui app's own batch to clipboard (`video_export.rs:488`): the videos
    /// go to a temporary directory this process does not clean up, because the
    /// clipboard holds paths into it until the reader pastes them.
    fn render_many_to_clipboard(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.render_many(paths, BatchTarget::Clipboard, window, cx);
    }

    /// Runs one batch render, saying which replay it has reached and what it
    /// managed in the end.
    fn render_many(&mut self, paths: Vec<PathBuf>, target: BatchTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else { return };
        let to_clipboard = matches!(target, BatchTarget::Clipboard);

        cx.spawn_in(window, async move |_this, cx| {
            let out_dir = match target {
                // Cancelled at the dialog.
                BatchTarget::Directory => {
                    match crate::dialog::pick_folder(&t!("ui.replay.renderer.export_video")).await {
                        Some(picked) => picked,
                        None => return,
                    }
                }
                BatchTarget::Clipboard => match crate::replay_renderer::temporary_batch_dir() {
                    Some(temporary) => temporary,
                    None => {
                        let _ = cx.update(|window, cx| {
                            crate::toast::failed(t!("ui.replay.renderer.export_path_unusable").into_owned(), window, cx)
                        });
                        return;
                    }
                },
            };
            let count = paths.len();
            let shown = out_dir.display().to_string();
            let _ = cx.update(|window, cx| {
                crate::toast::info(
                    t!("ui.replay.renderer.batch_started", count = count, dir = shown).into_owned(),
                    window,
                    cx,
                );
            });

            let (report, mut steps) = futures::channel::mpsc::unbounded();
            let batch = cx.update(|_window, cx| {
                // What a viewport would have shown, so a batch does not write a
                // different video than the reader has been watching.
                let defaults = crate::render_defaults::defaults(cx);
                crate::replay_renderer::batch_export(paths, game_data, out_dir, defaults, report, cx)
            });
            let Ok(batch) = batch else { return };

            // One message, rewritten as the batch walks the set: forty replays is
            // minutes of work, and a line that never moves reads as a hang.
            let watch = {
                let mut cx = cx.clone();
                async move {
                    while let Some(step) = futures::StreamExt::next(&mut steps).await {
                        let told = cx.update(|window, cx| {
                            crate::toast::progress(
                                BATCH_PROGRESS,
                                t!(
                                    "ui.replay.renderer.batch_progress",
                                    done = step.done + 1,
                                    total = step.total,
                                    replay = step.replay
                                )
                                .into_owned(),
                                window,
                                cx,
                            );
                        });
                        if told.is_err() {
                            break;
                        }
                    }
                }
            };

            let ((written, failed), ()) = futures::future::join(batch, watch).await;

            let _ = cx.update(|window, cx| {
                crate::toast::resolved(BATCH_PROGRESS, window, cx);
                let copied = if to_clipboard && !written.is_empty() { copy_rendered_files(&written) } else { Ok(()) };
                let said = match (&copied, failed.is_empty(), to_clipboard) {
                    // The files were written either way, so the message says where
                    // they are rather than only that the clipboard refused them.
                    (Err(reason), _, _) => {
                        t!("ui.replay.renderer.batch_not_copied", dir = shown.clone(), reason = reason.clone())
                            .into_owned()
                    }
                    (Ok(()), true, true) => t!("ui.replay.renderer.batch_copied", written = written.len()).into_owned(),
                    (Ok(()), true, false) => {
                        t!("ui.replay.renderer.batch_all_written", written = written.len()).into_owned()
                    }
                    (Ok(()), false, _) => {
                        t!("ui.replay.renderer.batch_finished", written = written.len(), failed = failed.len())
                            .into_owned()
                    }
                };
                if failed.is_empty() && copied.is_ok() {
                    crate::toast::ok(said, window, cx);
                } else {
                    crate::toast::warn(said, window, cx);
                }
            });
        })
        .detach();
    }

    /// Contributes every listed battle, as the data-sharing setting asks.
    ///
    /// The egui app's "Send all replays to ShipBuilds" (`app.rs:4712`): a pass
    /// over what is listed rather than only what lands while the app is open.
    /// Under `LedgerUse::Consult` a replay already in the ledger is skipped before
    /// it is read, so a second pass costs one ledger read per replay; the pass
    /// that ignores the ledger reads and sends every one of them.
    pub(crate) fn contribute_all(
        &mut self,
        ledger: crate::upload::LedgerUse,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use wows_toolkit_viewmodel::settings::DataSharingMode;

        if self.data_sharing == DataSharingMode::Off {
            crate::toast::warn(t!("ui.replay.contribute_nothing_shared").into_owned(), window, cx);
            return;
        }
        let Some(game_data) = self.game_data.clone() else { return };
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let paths = self.browser.read(cx).listed_paths();
        if paths.is_empty() {
            crate::toast::warn(t!("ui.replay.contribute_none_listed").into_owned(), window, cx);
            return;
        }
        if self.contributing.is_some() {
            // One pass at a time: two would read the same directory twice and, with
            // the ledger ignored, send every battle twice.
            crate::toast::warn(t!("ui.replay.contribute_already_running").into_owned(), window, cx);
            return;
        }
        let mode = self.data_sharing;
        let total = paths.len();

        self.contributing = Some(cx.spawn_in(window, async move |this, cx| {
            let mut sent = 0usize;
            for (index, path) in paths.into_iter().enumerate() {
                let told = cx.update(|window, cx| {
                    crate::toast::progress(
                        CONTRIBUTE_PROGRESS,
                        t!("ui.replay.contributing", done = index + 1, total = total).into_owned(),
                        window,
                        cx,
                    );
                });
                if told.is_err() {
                    return;
                }

                // The ledger before the read: a replay already contributed is
                // seconds of parsing for a request that would be refused.
                if ledger == crate::upload::LedgerUse::Consult {
                    let Ok(already) =
                        cx.update(|_window, cx| crate::upload::already_contributed(path.clone(), pool.clone(), cx))
                    else {
                        return;
                    };
                    if already.await {
                        continue;
                    }
                }

                let parse = match cx.update(|_window, cx| spawn_parse(path.clone(), game_data.clone(), None, cx)) {
                    Ok(parse) => parse,
                    Err(_) => return,
                };
                let Ok(parsed) = parse.await else {
                    tracing::warn!(path = %path.display(), "sharing: the replay did not parse");
                    continue;
                };

                // The anchor both apps keep, recorded on the first sight of this
                // replay: a battle still being played is held for its results
                // rather than sent half-finished.
                let Ok(first_seen) = cx.update(|_window, cx| crate::upload::first_seen(path.clone(), pool.clone(), cx))
                else {
                    return;
                };
                let candidate = crate::upload::Candidate {
                    path: path.clone(),
                    shareable: parsed.shareable,
                    first_seen: first_seen.await,
                };
                let Ok(contributed) = cx.update(|_window, cx| {
                    crate::upload::contribute(candidate, mode, ledger, pool.clone(), String::new(), cx)
                }) else {
                    return;
                };
                if matches!(contributed.await, crate::upload::Outcome::Sent) {
                    sent += 1;
                }
            }

            let _ = cx.update(|window, cx| {
                crate::toast::resolved(CONTRIBUTE_PROGRESS, window, cx);
                crate::toast::ok(t!("ui.replay.contributed", sent = sent, total = total).into_owned(), window, cx);
            });
            let _ = this.update(cx, |this, _cx| this.contributing = None);
        }));
    }

    /// Contributes a battle that has just landed, if the reader asked for that.
    ///
    /// This is what the data-sharing setting governs: off sends nothing, build
    /// data sends each player's setup, and replays sends the file itself. The
    /// rules are shared with the egui app (`wows_toolkit_viewmodel::upload`), and
    /// so is the ledger, so a battle is contributed once however it was read.
    fn contribute_landed(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        use wows_toolkit_viewmodel::settings::DataSharingMode;

        if self.data_sharing == DataSharingMode::Off {
            return;
        }
        let Some(game_data) = self.game_data.clone() else { return };
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let mode = self.data_sharing;
        // The service is reached through whatever proxy this machine is on,
        // which `http::client` resolves; the manual setting is the app's.
        let proxy = String::new();
        let parse = spawn_parse(path.to_path_buf(), game_data, None, cx);
        let named = path.to_path_buf();

        cx.spawn(async move |_this, cx| {
            let parsed = match parse.await {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::warn!(path = %named.display(), error = %err, "sharing: the replay did not parse");
                    return;
                }
            };
            let candidate = crate::upload::Candidate {
                path: named.clone(),
                shareable: parsed.shareable,
                // First seen now, which is what a replay the watcher just
                // reported is: the grace window for its results starts here.
                first_seen: jiff::Timestamp::now(),
            };
            let sent = cx.update(|cx| {
                crate::upload::contribute(candidate, mode, crate::upload::LedgerUse::Consult, pool, proxy, cx)
            });
            match sent.await {
                crate::upload::Outcome::Sent => {
                    tracing::info!(path = %named.display(), "sharing: the battle was contributed")
                }
                crate::upload::Outcome::Failed(reason) => {
                    tracing::warn!(path = %named.display(), %reason, "sharing: the battle was not contributed")
                }
                other => tracing::debug!(path = %named.display(), ?other, "sharing: nothing sent"),
            }
        })
        .detach();
    }

    /// Puts a battle that has just landed into the replay index.
    ///
    /// The egui app indexes each replay as its background parser reads it
    /// (`task/replays.rs:540`), so an index fills as you play. Without this the
    /// port's index only grows when the reader presses Build Index, and every row
    /// of the newest battles reads as "not indexed" until they do.
    ///
    /// Its own parse, deliberately: this runs whether or not the replay is opened,
    /// and a tab that opens it will not be waiting on this.
    fn index_landed(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else { return };
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let Some(runtime) = crate::runtime::runtime(cx) else { return };
        let Some(root) = path.parent().map(std::path::Path::to_path_buf) else { return };
        let path = path.to_path_buf();

        cx.spawn(async move |_this, cx| {
            let indexed = cx
                .background_spawn(async move {
                    let now = jiff::Timestamp::now();
                    let source =
                        runtime.block_on(wows_toolkit_config::index::query::ensure_default_source(&pool, &root, now));
                    let source = match source {
                        Ok(source) => source,
                        Err(err) => return Err(err.to_string()),
                    };
                    crate::replay_index::index_one(&runtime, &pool, &path, &game_data, source, now)
                        .map_err(|err| err.to_string())
                })
                .await;
            if let Err(err) = indexed {
                tracing::warn!("replay index: the battle that just landed was not indexed: {err}");
            }
        })
        .detach();
    }

    /// Writes a battle that has just landed out to the auto-export directory.
    ///
    /// The egui app exports from its background parser as each replay appears
    /// (`task/replays.rs:558`), so a match nobody opens is still written; the
    /// port used to write only what was opened, which made the setting say more
    /// than it did. Skipped for a replay carrying no battle results, as the egui
    /// gate skips it: the document would be a hollow one for a battle the client
    /// left before the server reported it.
    ///
    /// Named by ship, map, scenario, mode and time, which is the name the egui
    /// app writes (`wows_toolkit_viewmodel::replay_export::exported_file_stem`),
    /// so a directory both apps write into reads as one set.
    fn auto_export_landed(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        let AutoExport::To { directory, format } = AutoExport::from_settings(&self.replay_settings) else { return };
        let Some(game_data) = self.game_data.clone() else { return };

        let debug = self.debug_mode;
        let parse = spawn_parse(path.to_path_buf(), game_data, self.personal_rating.clone(), cx);
        let named = path.to_path_buf();
        cx.spawn_in(window, async move |_this, cx| {
            let parsed = match parse.await {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::warn!(path = %named.display(), error = %err, "auto-export: the replay did not parse");
                    return;
                }
            };
            if parsed.raw_results_json.is_none() {
                tracing::debug!(path = %named.display(), "auto-export: no battle results, so nothing is written");
                return;
            }
            let out = directory.join(&parsed.export_stem).with_extension(format.extension());

            let export = if debug { parsed.export } else { parsed.export.stripped() };
            let written = cx.background_spawn(async move { super::panel::write_export(&export, &out, format) }).await;
            match written {
                Ok(()) => {
                    tracing::info!(path = %named.display(), "auto-export: the battle was written");
                    // Whatever stopped the last one has passed, so the notice
                    // about it comes down.
                    let _ = cx.update(|window, cx| crate::toast::resolved(AUTO_EXPORT_FAILED, window, cx));
                }
                Err(err) => {
                    tracing::warn!(path = %named.display(), error = %err, "auto-export failed");
                    // Kept up rather than flashed past: the reader asked for a file
                    // per battle and is not getting one. Taken down again by the
                    // next battle that writes.
                    let _ = cx.update(|window, cx| {
                        crate::toast::stuck(
                            AUTO_EXPORT_FAILED,
                            t!("ui.replay.auto_export_failed", error = err.to_string()).into_owned(),
                            window,
                            cx,
                        );
                    });
                }
            }
        })
        .detach();
    }

    /// Re-reads a replay a tab is open on, leaving the tab where it is.
    fn reparse_open_replay(&mut self, path: &PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.open_panels.get(path).and_then(|panel| panel.upgrade()) else { return };
        let Some(game_data) = self.game_data.clone() else { return };
        panel.update(cx, |panel, cx| panel.reparse(game_data, window, cx));
    }

    /// Opens `path` in a dock tab. A repeat double-click on a replay that is
    /// already open is a no-op rather than adding a second tab for it:
    /// `open_panels` tracks the live panel entity per path, checked here
    /// before creating a new one.
    /// A repeat open brings that tab forward (`DockArea::select_panel`) rather
    /// than leaving the reader on whichever tab was in front.
    ///
    /// `pub(crate)` so another tab can send a replay here: the Search tab's
    /// results open in this inspector rather than in one of their own.
    pub(crate) fn open_replay(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_replay_into(path, OpenTarget::ShowingTab, window, cx);
    }

    /// Opens `path` beside whatever is already open, which is what the
    /// listing's "Open in New Tab" asks for.
    pub(crate) fn open_replay_in_new_tab(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_replay_into(path, OpenTarget::NewTab, window, cx);
    }

    fn open_replay_into(&mut self, path: PathBuf, target: OpenTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else {
            tracing::warn!(
                path = %path.display(),
                "replay inspector: open requested before the WoWs directory was known"
            );
            return;
        };

        // A replay that is already open is shown, not opened twice: the tab
        // it is in comes forward, which is what the egui listing does with a
        // replay it has already hydrated.
        if let Some(existing) = self.open_panels.get(&path).and_then(|panel| panel.upgrade()) {
            let id = PanelId::from(existing.entity_id());
            self.dock_area.update(cx, |dock_area, cx| dock_area.select_panel(id, window, cx));
            self.current_replay = Some(path);
            cx.notify();
            return;
        }

        // A plain open takes the place of the replay tab on screen, as the
        // egui listing does; the reader asks for a second tab by name.
        if target == OpenTarget::ShowingTab
            && let Some(showing) = self.replaceable_panel(cx)
        {
            self.open_panels.remove(&showing.path);
            self.dock_area.update(cx, |dock_area, cx| dock_area.remove_panel_id(showing.panel, window, cx));
        }

        let columns = default_columns(&self.replay_settings);
        let personal_rating = self.personal_rating.clone();
        // Read per open rather than held: a directory the reader has since
        // removed simply stops being written to.
        let auto_export = AutoExport::from_settings(&self.replay_settings);
        let setup =
            PanelSetup { path: path.clone(), game_data, debug: self.debug_mode, columns, personal_rating, auto_export };
        let panel = cx.new(|cx| ReplayPanel::new(setup, window, cx));
        // A panel's Actions menu can ask for its own replay to be rendered,
        // which is this view's to do: it owns the dock the viewport opens in.
        self.panel_events.push(cx.subscribe_in(&panel, window, |this, _panel, event: &RenderRequested, window, cx| {
            let RenderRequested { path, alts } = event;
            this.render_replay_with_alts(path.clone(), alts.clone(), window, cx);
        }));
        // A battle read through a mapping that is not its build's: said once, and
        // the right one fetched so the next read is decoded properly.
        self.panel_events.push(cx.subscribe_in(
            &panel,
            window,
            |this, _panel, event: &super::panel::ConstantsMismatched, window, cx| {
                this.recover_constants(event.build, event.version.clone(), window, cx);
            },
        ));
        self.open_panels.insert(path.clone(), panel.downgrade());
        self.current_replay = Some(path);
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
        });
        self.has_opened_replay = true;
        // A closed viewport leaves a dangling weak handle behind, and the
        // map is only walked when one is asked for, so it is swept here.
        self.open_renderers.retain(|_, panel| panel.upgrade().is_some());
        cx.notify();
    }

    /// The replay tab a plain open replaces: one that is the active tab of
    /// its own group.
    ///
    /// A dock can hold several groups, each with a replay showing, so the one
    /// this view last opened or brought forward wins; failing that, any of
    /// them does, in path order so the choice is at least repeatable.
    fn replaceable_panel(&self, cx: &App) -> Option<ShowingReplay> {
        let mut active: Vec<PanelId> = Vec::new();
        if let Some(tree) = self.dock_area.read(cx).layout(DockPlacement::Center) {
            tree.root().walk(&mut |node| {
                if let PaneRef::Tabs { panels, active_ix } = node.kind()
                    && let Some(id) = panels.get(active_ix)
                {
                    active.push(*id);
                }
            });
        }

        let mut showing: Vec<ShowingReplay> = self
            .open_panels
            .iter()
            .filter_map(|(path, panel)| {
                let panel = PanelId::from(panel.upgrade()?.entity_id());
                active.contains(&panel).then(|| ShowingReplay { path: path.clone(), panel })
            })
            .collect();
        showing.sort_by(|left, right| left.path.cmp(&right.path));

        let current = self.current_replay.as_ref();
        showing.iter().find(|shown| Some(&shown.path) == current).cloned().or_else(|| showing.into_iter().next())
    }

    /// Records `paths` in the Stats tab's session.
    ///
    /// Each replay is parsed on the background executor, as opening it would
    /// be: the figures a session row carries come from the battle results,
    /// which are only in the file. A replay that will not parse is logged and
    /// skipped rather than taking the rest of the set down with it.
    ///
    /// `replace` forgets what is recorded first, which is what the listing's
    /// "Set as Session Stats" means; "Add to" leaves it and appends.
    fn record_session_stats(&mut self, paths: Vec<PathBuf>, replace: bool, cx: &mut Context<Self>) {
        let Some(game_data) = self.game_data.clone() else {
            tracing::warn!("replay inspector: session stats asked for before the WoWs directory was known");
            return;
        };
        let Some(pool) = crate::settings_store::pool(cx) else { return };

        cx.spawn(async move |_this, cx| {
            if replace {
                let cleared = crate::runtime::spawn(cx, {
                    let pool = pool.clone();
                    async move { wows_toolkit_config::queries::clear_session_stats(&pool).await }
                })
                .await;
                if let Ok(Err(err)) = cleared {
                    tracing::warn!("session stats: the session could not be cleared: {err}");
                    return;
                }
            }

            for path in paths {
                let parsed = {
                    let game_data = game_data.clone();
                    let path = path.clone();
                    cx.background_spawn(async move { super::load::parse_replay(&path, &game_data, None) }).await
                };
                let stat = match parsed {
                    Ok(parsed) => parsed.session_stat,
                    Err(err) => {
                        tracing::warn!(path = %path.display(), "session stats: the replay would not parse: {err}");
                        continue;
                    }
                };
                let Some(stat) = stat else {
                    tracing::warn!(path = %path.display(), "session stats: the replay names no recording player");
                    continue;
                };

                let row = stat.to_row();
                let pool = pool.clone();
                let written = crate::runtime::spawn(cx, async move {
                    wows_toolkit_config::queries::add_session_stat(&pool, &row).await
                })
                .await;
                match written {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => tracing::warn!("session stats: the battle could not be recorded: {err}"),
                    Err(err) => tracing::warn!("session stats: the write did not complete: {err}"),
                }
            }
        })
        .detach();
    }

    /// Opens a playback viewport on `path`, in a dock tab of its own.
    ///
    /// A second ask for the same replay brings the tab it is in forward
    /// rather than baking the battle twice.
    pub(crate) fn render_replay(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.render_replay_with_alts(path, Vec::new(), window, cx);
    }

    /// The same, reading the battle through other recordings of it as well.
    pub(crate) fn render_replay_with_alts(
        &mut self,
        path: PathBuf,
        alts: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(game_data) = self.game_data.clone() else {
            tracing::warn!(
                path = %path.display(),
                "replay inspector: render requested before the WoWs directory was known"
            );
            return;
        };

        if let Some(existing) = self.open_renderers.get(&path).and_then(|panel| panel.upgrade()) {
            // A recording added since this viewport was baked is not on the map
            // it is showing, so it is baked again rather than brought forward
            // as it is.
            let stale = existing.read(cx).baked_alts() != alts.as_slice();
            if stale {
                existing.update(cx, |panel, cx| panel.rebake_with_alts(alts, cx));
            }
            let id = PanelId::from(existing.entity_id());
            self.dock_area.update(cx, |dock_area, cx| dock_area.select_panel(id, window, cx));
            cx.notify();
            return;
        }

        let title: SharedString = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| t!("ui.replay.context.render_replay").into_owned())
            .into();
        let link = self.collab.link();
        let panel = cx.new(|cx| {
            let mut panel = ReplayRendererPanel::new(path.clone(), alts, title, game_data, window, cx);
            panel.seed_collab(link, cx);
            panel
        });
        self.renderer_events.push(cx.subscribe_in(&panel, window, Self::on_renderer_event));
        self.open_renderers.insert(path, panel.downgrade());
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
        });
        cx.notify();
    }

    /// Hands every open viewport this app's end of the session, when whether
    /// there is one has changed.
    ///
    /// A session can start or stop while a viewport is open, and a viewport
    /// built before one started would otherwise never hear about it.
    fn share_session_with_renderers(&mut self, cx: &mut Context<Self>) {
        let active = self.collab.is_active();
        if active == self.session_shared {
            return;
        }
        self.session_shared = active;
        // One link each rather than one cloned to all: a link holds what its own
        // viewport has drawn while there is no session to hold it, and a shared
        // one would put every viewport's drawings on every other.
        for panel in self.open_renderers.values() {
            if let Some(panel) = panel.upgrade() {
                let link = self.collab.link();
                panel.update(cx, |panel, cx| panel.set_collab(link, cx));
            }
        }
        // The tactics board is a window this view does not own, and it draws in
        // the session as a viewport does.
        cx.emit(SessionShared(self.collab.link()));
    }

    /// Whether a session is running, for a window this view does not own.
    pub fn session_link(&self) -> crate::collab::CollabLink {
        self.collab.link()
    }

    /// Answers a viewport that asked for a window of its own, or for an
    /// armor viewer on one of the battle's ships.
    fn on_renderer_event(
        &mut self,
        panel: &Entity<ReplayRendererPanel>,
        event: &RendererEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RendererEvent::PopOut => self.pop_out_renderer(panel.clone(), window, cx),
            // Passed further up: the armor viewer is another tab, which this
            // view does not own either.
            RendererEvent::ShowArmor { param_index, display_name, hits, incoming } => cx.emit(ShowArmorRequested {
                param_index: param_index.clone(),
                display_name: display_name.clone(),
                hits: hits.clone(),
                incoming: incoming.clone(),
            }),
            RendererEvent::ArmorFollowed { hits, health, at } => {
                cx.emit(ArmorFollowed { at: *at, hits: hits.clone(), health: *health })
            }
        }
    }

    /// Moves a viewport out of the dock and into a window of its own.
    ///
    /// The same entity is re-hosted rather than a new one built: the baked
    /// track and the renderer it draws through come with it, so nothing is
    /// walked or loaded twice. GPUI entities are not owned by a window, which
    /// is what makes that possible.
    pub(crate) fn pop_out_renderer(
        &mut self,
        panel: Entity<ReplayRendererPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if panel.read(cx).is_popped_out() {
            return;
        }
        let title = panel.read(cx).path().file_stem().map(|stem| stem.to_string_lossy().into_owned());

        // Out of the dock first: the same entity drawn in two places would
        // fight over the one renderer it rasterises through.
        let id = PanelId::from(panel.entity_id());
        self.dock_area.update(cx, |dock_area, cx| dock_area.remove_panel_id(id, window, cx));

        let options = crate::window_shell::options(
            wows_toolkit_config::WindowKind::ReplayRenderer,
            title.clone().unwrap_or_else(|| t!("ui.replay.context.render_replay").into_owned()),
            cx,
        );
        let opened = cx.open_window(options, {
            let panel = panel.clone();
            move |window, cx| {
                crate::window_shell::remember(wows_toolkit_config::WindowKind::ReplayRenderer, window, cx);
                // Through a `Shell` so the panel's toasts and dialogs are drawn
                // in this window too, not only in the one the dock lives in.
                let view: AnyView = cx.new(|_cx| crate::window_shell::Shell::new(panel)).into();
                cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
            }
        });

        match opened {
            Ok(_) => panel.update(cx, |panel, cx| panel.mark_popped_out(cx)),
            Err(err) => {
                // The window would not open, so the viewport goes back where
                // it was rather than vanishing.
                tracing::warn!("replay renderer: the window could not be opened: {err}");
                self.dock_area.update(cx, |dock_area, cx| {
                    dock_area.add_panel_view(panel_handle(panel), DockPlacement::Center, None, window, cx);
                });
            }
        }
        cx.notify();
    }

    /// The expected-values table this tab rates replays against. Test-only:
    /// production code reaches the field directly.
    #[cfg(test)]
    pub(crate) fn personal_rating(&self) -> Option<&Arc<PersonalRatingData>> {
        self.personal_rating.as_ref()
    }

    /// The per-build game-data cache this tab loads replays through, once
    /// the WoWs directory is known. Shared rather than cloned fresh so a
    /// build another tab needs is loaded once for the whole app.
    /// Hands the view a game-data cache directly, which is all
    /// [`Self::open_replay`] waits on. Skips the directory scan and the
    /// startup build preload `apply_settings` starts, neither of which a test
    /// about the dock has anything to say about.
    #[cfg(test)]
    pub(crate) fn seed_game_data(&mut self, wows_dir: &str) {
        self.game_data = Some(GameDataCache::new(PathBuf::from(wows_dir)));
    }

    pub(crate) fn game_data(&self) -> Option<GameDataCache> {
        self.game_data.clone()
    }

    /// Adopts the session's expected-values table and pushes it into every
    /// open replay tab, so a replay opened before the table loaded gets its
    /// Personal Rating column filled in rather than staying empty until it is
    /// reopened. Called from `App::apply_session_stats`.
    ///
    /// Takes the table itself, not an option: a caller that has none must not
    /// be able to clear one already in hand, which would leave later tabs
    /// unrated while the tabs open at the time kept their ratings.
    pub(crate) fn set_personal_rating(&mut self, table: Arc<PersonalRatingData>, cx: &mut Context<Self>) {
        self.personal_rating = Some(table.clone());
        for panel in self.open_panels.values() {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |panel, cx| panel.set_personal_rating(table.clone(), cx));
            }
        }
        cx.notify();
    }

    /// Flips the session debug-mode flag and pushes the new value into every
    /// currently open replay tab (`open_panels`'s live entries; closed tabs'
    /// stale weak handles just fail to upgrade and are skipped), so toggling
    /// debug mode takes effect immediately rather than only on the next
    /// replay opened. Called from `App::toggle_debug_mode` (the app-wide
    /// Ctrl+Shift+D shortcut, `app.rs`) -- this crate has no enable UI of its
    /// own for debug mode.
    pub(crate) fn set_debug_mode(&mut self, debug_mode: bool, cx: &mut Context<Self>) {
        self.debug_mode = debug_mode;
        for panel in self.open_panels.values() {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |panel, cx| panel.set_debug(debug_mode, cx));
            }
        }
        cx.notify();
    }

    /// "Open manually": the header toolbar's file-picker button. Mirrors the
    /// egui app's `build_replay_header` open-manually handler
    /// (`ui/replay_parser/mod.rs:3659`) exactly -- same file filter -- except
    /// the picked path opens through this port's own dock flow
    /// (`open_replay`) rather than the egui app's `parse_replay_from_path`
    /// background task. A cancelled dialog is a no-op.
    pub(crate) fn open_manually(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let asked = crate::dialog::pick_file(None, Some(crate::dialog::REPLAYS));
        cx.spawn(async move |this, cx| {
            let Some(file) = asked.await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                // What the Settings tab reports as the current replay: the one
                // the reader named, which is what the egui app writes here too.
                crate::settings_store::save(wows_toolkit_viewmodel::settings::keys::CURRENT_REPLAY_PATH, &file, cx);
                this.open_replay(file, window, cx);
            });
        })
        .detach();
    }

    /// Flips "Autoload Latest Replay", which the directory watcher reads when a
    /// replay lands, and writes it to the row the egui app reads.
    fn set_auto_load_latest_replay(&mut self, value: bool, cx: &mut Context<Self>) {
        self.auto_load_latest_replay = value;
        crate::settings_store::save(wows_toolkit_viewmodel::settings::keys::AUTO_LOAD_LATEST_REPLAY, &value, cx);
        cx.notify();
    }

    /// The browser's current grouping strategy. The browser is the single
    /// source of truth for it; the header combo only mirrors it.
    pub(crate) fn grouping(&self, cx: &App) -> ReplayGrouping {
        self.browser.read(cx).grouping()
    }

    /// Switches the browser's grouping strategy. The header toolbar owns this
    /// control (matching the egui app's header placement); `browser` itself
    /// just applies it and rebuilds its tree (`ReplayBrowser::set_grouping`).
    ///
    /// The only writer, so the combo mirror cannot drift from the browser: a
    /// caller that reaches the browser directly would desync the two.
    /// Shows or hides the replay listing beside the open replays.
    fn set_listing_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if self.replay_settings.listing_collapsed == collapsed {
            return;
        }
        self.set_column_filter(|settings| settings.listing_collapsed = collapsed, cx);
    }

    fn set_grouping(&mut self, grouping: ReplayGrouping, window: &mut Window, cx: &mut Context<Self>) {
        self.adopt_grouping(grouping, window, cx);
        // Written back as the egui header writes it, so the listing is grouped
        // the same way on the next launch.
        self.set_column_filter(|settings| settings.grouping = grouping, cx);
    }

    /// Shows `grouping` without writing it back, for the seed from the stored
    /// row: announcing that would ask the app to save what it just read.
    fn adopt_grouping(&mut self, grouping: ReplayGrouping, window: &mut Window, cx: &mut Context<Self>) {
        self.browser.update(cx, |browser, cx| browser.set_grouping(grouping, cx));
        self.grouping_select.update(cx, |state, cx| state.set_selected_value(&grouping, window, cx));
        cx.notify();
    }

    /// Applies one column-filter checkbox's change: mutates `replay_settings`
    /// via `apply`, recomputes the visible-column set
    /// (`columns::default_columns`), and pushes it into every currently open
    /// replay tab (mirroring `set_debug_mode`'s live-propagation pattern) so
    /// the table(s) update immediately rather than only on the next replay
    /// opened.
    /// Replaces the whole settings blob, for an edit made on the Settings tab
    /// rather than through this header's own checkboxes.
    pub(crate) fn set_replay_settings(&mut self, settings: ReplaySettings, cx: &mut Context<Self>) {
        // Adopted, not announced: this came from the row, so saying it
        // changed would ask the app to write back what it just read.
        self.adopt_replay_settings(|current| *current = settings, cx);
    }

    /// An edit made here, which the app writes back to the shared row.
    fn set_column_filter(&mut self, apply: impl FnOnce(&mut ReplaySettings), cx: &mut Context<Self>) {
        self.adopt_replay_settings(apply, cx);
        cx.emit(ReplaySettingsChanged(self.replay_settings.clone()));
    }

    /// Applies `apply` and pushes the resulting columns into every open tab.
    fn adopt_replay_settings(&mut self, apply: impl FnOnce(&mut ReplaySettings), cx: &mut Context<Self>) {
        apply(&mut self.replay_settings);
        let columns = default_columns(&self.replay_settings);
        for panel in self.open_panels.values() {
            if let Some(panel) = panel.upgrade() {
                panel.update(cx, |panel, cx| panel.set_columns(columns.clone(), cx));
            }
        }
        cx.notify();
    }
}

/// The narrow rail the listing is collapsed and expanded from.
fn listing_rail(entity: Entity<ReplayInspectorView>, collapsed: bool) -> impl IntoElement {
    let (glyph, tooltip) = if collapsed {
        (crate::icons::CARET_RIGHT, "ui.replay.expand_listing")
    } else {
        (crate::icons::CARET_LEFT, "ui.replay.collapse_listing")
    };

    div()
        .id("replay-listing-rail")
        .test_support()
        .flex_none()
        .w(LISTING_RAIL_WIDTH)
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(crate::theme::text_dim())
        .tooltip(move |window, cx| Tooltip::new(t!(tooltip).into_owned()).build(window, cx))
        .child(crate::icons::icon(glyph))
        .on_click(move |_event, _window, cx| {
            entity.update(cx, |view, cx| view.set_listing_collapsed(!collapsed, cx));
        })
}

/// Wide enough for a caret and its click target, and no wider: the rail is
/// chrome beside the listing, not a column of its own.
const LISTING_RAIL_WIDTH: Pixels = px(18.);

/// One column-filter checkbox in the header toolbar: `checked` reflects
/// `replay_settings`, clicking applies `apply` to it via `set_column_filter`
/// and recomputes the visible columns. A free function for the same reason as
/// `column_filters_popover` -- built three times, once per optional column the egui
/// app's `build_replay_header` (`ui/replay_parser/mod.rs:3697-3711`) exposes a
/// toggle for and that has a live column in this port: Raw XP, Observed
/// Damage, Heals. Received Damage and Distance Traveled are not exposed here
/// either -- the egui app never lets the user toggle those, so they stay at
/// `ReplaySettings`'s config defaults.
fn column_filter_checkbox(
    entity: Entity<ReplayInspectorView>,
    id: &'static str,
    label: String,
    checked: bool,
    apply: impl Fn(&mut ReplaySettings, bool) + Copy + 'static,
) -> impl IntoElement {
    Checkbox::new(id).label(label).checked(checked).on_click(move |checked: &bool, _window, cx: &mut App| {
        let checked = *checked;
        entity.update(cx, |view, cx| view.set_column_filter(move |settings| apply(settings, checked), cx));
    })
}

/// The column-filter dropdown, matching the egui app's "Column Filters"
/// `ComboBox` (`ui/replay_parser/mod.rs:4609`) rather than showing the
/// checkboxes inline. `settings` is a snapshot: the `.content()` closure
/// re-runs on every open-render, so it always reflects the latest state.
///
/// The egui popup carries a fourth toggle, Entity ID, which neither app has a
/// column for; it is left out here rather than drawn as a control that does
/// nothing.
fn column_filters_popover(entity: Entity<ReplayInspectorView>, settings: ReplaySettings) -> impl IntoElement {
    Popover::new("replay-header-column-filters")
        .trigger(
            Button::new("replay-header-column-filters-trigger")
                .label(t!("ui.replay.column_filters").to_string())
                .compact(),
        )
        .content(move |_state, _window, _cx| {
            v_flex()
                .gap_1()
                .p_1()
                .child(column_filter_checkbox(
                    entity.clone(),
                    "replay-header-filter-raw-xp",
                    t!("stat.raw_xp").into_owned(),
                    settings.show_raw_xp,
                    |settings, value| settings.show_raw_xp = value,
                ))
                .child(column_filter_checkbox(
                    entity.clone(),
                    "replay-header-filter-observed-damage",
                    t!("ui.replay.column.observed_damage").into_owned(),
                    settings.show_observed_damage,
                    |settings, value| settings.show_observed_damage = value,
                ))
                .child(column_filter_checkbox(
                    entity.clone(),
                    "replay-header-filter-heals",
                    t!("ui.replay.column.heals").into_owned(),
                    settings.show_heals,
                    |settings, value| settings.show_heals = value,
                ))
        })
}

impl Render for ReplayInspectorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dock_content: AnyElement = if self.has_opened_replay {
            self.dock_area.clone().into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(crate::theme::text_dim())
                        .child(t!("ui.replay.select_replay").to_string()),
                )
                .into_any_element()
        };

        // The listing says so itself while the game data loads (it is what
        // the rows are named from), so the banner carries only the failure,
        // which nothing else reports.
        let status_banner = match &self.game_data_status {
            GameDataStatus::Failed(reason) => Some(
                div()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(t!("ui.replay.game_data_failed", error = reason).to_string())
                    .into_any_element(),
            ),
            GameDataStatus::Loading { .. } | GameDataStatus::Ready(_) => None,
        };

        let entity = cx.entity();

        // Header toolbar: mirrors the egui app's `build_replay_header`
        // (`ui/replay_parser/mod.rs:3657`) -- manual file open, autoload
        // checkbox, grouping selector, column-filter checkboxes, the session
        // popover -- in the same left-to-right order. Rendering a replay is
        // not here: it belongs to a replay, and an opened one carries it in
        // its own Actions menu.
        self.collab.display_name = self.collab_name.read(cx).value().trim().to_string();
        self.collab.bind(&entity, cx);
        // The session's event inbox is unbounded, so it is drained here every
        // draw rather than left to grow for as long as the session runs. What it
        // raised is said after the draw: a message queued during one is not on
        // the frame being drawn.
        let notices = self.collab.poll();
        if !notices.is_empty() {
            cx.defer_in(window, move |_this, window, cx| {
                for notice in notices {
                    notice.say(window, cx);
                }
            });
        }
        self.share_session_with_renderers(cx);
        let session = crate::collab_popover::render(self, &entity, cx);
        let replay_header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(session)
            .child(
                Button::new("replay-header-open-manually")
                    .icon(IconName::FolderOpen)
                    .label(t!("ui.replay.open_manually").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event: &ClickEvent, window, cx| this.open_manually(window, cx))),
            )
            // A folder of replays that is not the install's: an archive, or
            // someone else's. The egui app opens each as a workspace of its own.
            .child(
                Button::new("replay-header-open-directory")
                    .icon(IconName::Folder)
                    // The glyph alone: the header is already the width of the
                    // window at its narrowest, and the name is in the tooltip.
                    .tooltip(t!("ui.replay.open_directory").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event: &ClickEvent, window, cx| this.open_directory(window, cx))),
            )
            // Which directory is being listed, when it is not the install's: the
            // rows otherwise look like the game's own and are not.
            .children(self.browser.read(cx).chosen_directory().map(|directory| {
                let shown = directory.display().to_string();
                h_flex()
                    .id("replay-header-directory")
                    .test_support()
                    .aria_label(shown.clone())
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(crate::icons::icon(crate::icons::FOLDER))
                    .child(shown)
                    .into_any_element()
            }))
            .child(
                Checkbox::new("replay-header-auto-load-latest")
                    .label(t!("ui.replay.autoload_latest").to_string())
                    .checked(self.auto_load_latest_replay)
                    .tooltip(t!("ui.replay.autoload_latest_tooltip").to_string())
                    .on_click(
                        cx.listener(|this, checked: &bool, _window, cx| this.set_auto_load_latest_replay(*checked, cx)),
                    ),
            )
            .child(crate::ui::rule_v(cx))
            .child(
                crate::ui::boxed(px(160.), crate::ui::SELECT_SMALL_HEIGHT).child(
                    Select::new(&self.grouping_select)
                        .id("replay-header-grouping")
                        .title_prefix(t!("ui.replay.group_prefix").to_string())
                        .accessibility_label(t!("ui.replay.group_label").to_string())
                        .small(),
                ),
            )
            .child(column_filters_popover(entity.clone(), self.replay_settings.clone()));

        v_flex()
            .size_full()
            .child(replay_header)
            .when_some(status_banner, |this, banner| this.child(h_flex().flex_none().px_2().py_1().child(banner)))
            .child(
                // A plain flex row rather than `h_flex`, which centres its
                // children: this is the body, and the listing beside the
                // replays is a full-height column rather than a toolbar item.
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.))
                    // A rail beside the listing, as in the egui tab: one
                    // caret, pointing the way the click will move it.
                    .child(listing_rail(entity.clone(), self.replay_settings.listing_collapsed))
                    .child(if self.replay_settings.listing_collapsed {
                        div().flex_1().min_w(px(0.)).child(dock_content).into_any_element()
                    } else {
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                h_resizable("replay-inspector-split")
                                    .child(
                                        resizable_panel()
                                            .size(BROWSER_WIDTH)
                                            .size_range(BROWSER_MIN_WIDTH..BROWSER_MAX_WIDTH)
                                            .flex_none()
                                            .child(self.browser.clone()),
                                    )
                                    .child(resizable_panel().child(dock_content)),
                            )
                            .into_any_element()
                    }),
            )
    }
}

impl crate::collab_popover::SessionHost for ReplayInspectorView {
    fn collab(&self) -> &crate::collab::CollabState {
        &self.collab
    }

    fn collab_mut(&mut self) -> &mut crate::collab::CollabState {
        &mut self.collab
    }

    fn name_input(&self) -> &Entity<InputState> {
        &self.collab_name
    }

    fn token_input(&self) -> &Entity<InputState> {
        &self.collab_token
    }

    fn request_tactics_board(&mut self, cx: &mut Context<Self>) {
        cx.emit(TacticsBoardRequested);
    }
}

#[cfg(test)]
mod tests {
    use super::InspectorSettings;
    use super::ReplayInspectorView;
    use gpui_kit::AppContext as _;
    use gpui_kit::Entity;
    use gpui_kit::TestAppContext;
    use gpui_kit::WindowHandle;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestAppContextExt;
    use gpui_kit::test::TestWindowExt as _;
    use std::path::PathBuf;

    /// The test window's height, which the listing is measured against.
    const WINDOW_HEIGHT: gpui_kit::Pixels = px(800.);

    /// Every grouping names a catalogue entry, since the combo draws whatever
    /// the key resolves to and an absent one draws the key itself.
    #[test]
    fn every_grouping_has_a_name_in_the_catalog() {
        for grouping in super::GROUPINGS {
            let key = super::grouping_label_key(grouping);
            let rendered = rust_i18n::t!(key).into_owned();
            assert_ne!(rendered, key, "no catalog entry for {key}");
        }
    }

    /// Settings with a directory named, which is all `open_replay` waits on
    /// before it builds a panel. The directory does not have to exist: the
    /// panel reports the failed parse itself.
    fn settings() -> InspectorSettings {
        InspectorSettings {
            wows_dir: "G:/does-not-exist".to_string(),
            game_data_cache_dir: String::new(),
            auto_dump_game_data: false,
            data_sharing: Default::default(),
            debug_mode: false,
            replay_settings: Default::default(),
            auto_load_latest_replay: false,
            locale: None,
            collab_display_name: String::new(),
        }
    }

    /// The view inside a `Root`, which the dock's own overlays need under
    /// them.
    fn open_view(cx: &mut TestAppContext) -> (WindowHandle<gpui_kit::component::Root>, Entity<ReplayInspectorView>) {
        cx.update(gpui_kit::init);
        let view = std::cell::RefCell::new(None);
        let window = cx.open_window(size(px(1200.), WINDOW_HEIGHT), |window, cx| {
            let inspector = cx.new(|cx| ReplayInspectorView::new(window, cx));
            *view.borrow_mut() = Some(inspector.clone());
            gpui_kit::component::Root::new(inspector, window, cx)
        });
        let view = view.borrow_mut().take().expect("the view was built inside the window");
        (window, view)
    }

    /// The listing is a column down the side of the tab, and the rail takes
    /// it away and brings it back.
    ///
    /// The session popover is built from inside this tab's own render, where
    /// the tab's entity is leased. Reading that entity there panics with
    /// "cannot read ... while it is already being updated", which takes the
    /// whole tab down rather than just the popover -- so the header is drawn
    /// here and every control on it checked.
    #[gpui_kit::test]
    fn the_header_draws_with_the_session_control_on_it(cx: &mut TestAppContext) {
        let (window, _view) = open_view(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);

            assert!(window.try_find("collab-session-toggle").is_some(), "the session control is on the header");
            // Rendering belongs to a replay, so an opened one carries it in
            // its own Actions menu rather than the header carrying it for
            // whichever replay happens to be showing.
            assert!(window.try_find("replay-header-render").is_none(), "rendering is not a header control");
            // The rest of the header still draws beside it.
            assert!(window.try_find("replay-header-open-manually").is_some());
            assert!(window.try_find("replay-header-auto-load-latest").is_some());
        })
        .expect("the window is open");
    }

    /// Opening the popover with no session offers a name and the two ways to
    /// start one, and starts neither until a name is given: every peer reads
    /// that name, so an unnamed session makes a roster nobody can follow.
    #[gpui_kit::test]
    fn the_session_popover_starts_nothing_without_a_display_name(cx: &mut TestAppContext) {
        let (window, view) = open_view(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("collab-session-toggle", cx);
            window.render_frame(cx);

            assert!(window.try_find("collab-display-name").is_some(), "a name is asked for first");
            assert!(window.try_find("collab-host").is_some());
            assert!(window.try_find("collab-token").is_some());

            window.click("collab-host", cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        view.update(cx, |view, _cx| {
            assert!(!view.collab.is_active(), "nothing started without a name to start it under");
        });
    }

    /// The height is the assertion that matters: the listing rendered at its
    /// content height, centred in an otherwise empty tab, for as long as its
    /// row was an `h_flex` (which installs `items_center`).
    #[gpui_kit::test]
    fn the_rail_collapses_a_full_height_listing_and_brings_it_back(cx: &mut TestAppContext) {
        let (window, _view) = open_view(cx);

        let listing = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find("replay-browser-rows").bounds()
            })
            .expect("the window is open");

        assert!(
            listing.size.height > WINDOW_HEIGHT / 2.,
            "the listing runs down the tab rather than sitting at its content height, got {:?}",
            listing.size.height
        );

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);

            window.click("replay-listing-rail", cx);
            window.render_frame(cx);
            assert!(window.try_find("replay-browser-rows").is_none(), "the rail took the listing off screen");

            window.click("replay-listing-rail", cx);
            window.render_frame(cx);
            assert!(window.try_find("replay-browser-rows").is_some(), "and brought it back");
        })
        .expect("the window is open");
    }

    /// The boundary between the listing and the replays beside it drags.
    #[gpui_kit::test]
    fn the_listing_is_adjustable_in_width(cx: &mut TestAppContext) {
        let (window, _view) = open_view(cx);

        let before = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find("replay-browser-rows").bounds()
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            // The boundary sits on the listing's trailing edge.
            let from = gpui_kit::point(before.right(), before.center().y);
            window.drag(from, gpui_kit::point(from.x + px(80.), from.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        let after = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find("replay-browser-rows").bounds()
            })
            .expect("the window is open");

        assert!(
            after.size.width > before.size.width,
            "the listing widened with the drag, from {:?} to {:?}",
            before.size.width,
            after.size.width
        );
    }

    /// The listing keeps its place, and its drag, once a replay is open
    /// beside it: the dock that replaces the placeholder is the thing most
    /// likely to take the space over.
    #[gpui_kit::test]
    fn the_listing_survives_a_replay_being_opened_beside_it(cx: &mut TestAppContext) {
        let (window, view) = open_view(cx);

        let before = cx
            .update_window(window.into(), |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.seed_game_data("G:/does-not-exist");
                    view.open_replay(PathBuf::from("first.wowsreplay"), window, cx);
                });
                window.render_frame(cx);
                window.find("replay-browser-rows").bounds()
            })
            .expect("the window is open");

        assert!(before.size.width > px(0.), "the listing still has width with a replay open");

        cx.update_window(window.into(), |_, window, cx| {
            let from = gpui_kit::point(before.right(), before.center().y);
            window.drag(from, gpui_kit::point(from.x + px(80.), from.y), cx);
            window.render_frame(cx);
        })
        .expect("the window is open");

        let after = cx
            .update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.find("replay-browser-rows").bounds()
            })
            .expect("the window is open");

        assert!(
            after.size.width > before.size.width,
            "the boundary still drags with a replay open, from {:?} to {:?}",
            before.size.width,
            after.size.width
        );
    }

    /// A plain open takes the place of the replay on screen, and "Open in New
    /// Tab" is how a second one is asked for.
    #[gpui_kit::test]
    fn a_plain_open_replaces_the_replay_on_screen(cx: &mut TestAppContext) {
        let (window, view) = open_view(cx);

        cx.update_window(window.into(), |_, window, cx| {
            view.update(cx, |view, cx| {
                view.apply_settings(settings(), window, cx);
                view.open_replay(PathBuf::from("first.wowsreplay"), window, cx);
                assert_eq!(view.open_panels.len(), 1, "the first replay opens a tab");

                view.open_replay(PathBuf::from("second.wowsreplay"), window, cx);
                assert_eq!(view.open_panels.len(), 1, "the second takes its place");
                assert!(view.open_panels.contains_key(&PathBuf::from("second.wowsreplay")));

                view.open_replay_in_new_tab(PathBuf::from("third.wowsreplay"), window, cx);
                assert_eq!(view.open_panels.len(), 2, "a new tab is asked for by name");
            });
        })
        .expect("the window is open");
    }

    /// Re-opening a replay already open brings its tab forward rather than
    /// replacing it with a second copy of itself.
    #[gpui_kit::test]
    fn re_opening_the_showing_replay_leaves_it_alone(cx: &mut TestAppContext) {
        let (window, view) = open_view(cx);

        cx.update_window(window.into(), |_, window, cx| {
            view.update(cx, |view, cx| {
                view.apply_settings(settings(), window, cx);
                view.open_replay(PathBuf::from("first.wowsreplay"), window, cx);
                view.open_replay(PathBuf::from("first.wowsreplay"), window, cx);
                assert_eq!(view.open_panels.len(), 1);
                assert!(view.open_panels.contains_key(&PathBuf::from("first.wowsreplay")));
            });
        })
        .expect("the window is open");
    }
}
