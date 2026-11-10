//! The replay listing: the replays of a directory, grouped by date or ship,
//! read in the background and kept current by a watch on the directory.
//!
//! The scan reads only each replay's plaintext header. A row names its ship
//! and map against the game data and reports damage, kills and the outcome
//! from the replay index, so the listing of the install's directory waits
//! for the game data rather than listing raw ids.

mod actions;
mod glyph;
mod listing;
mod watch;

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use nsh_base::alert_dialog;
use nsh_base::context_menu;
use nsh_base::dialog;
use nsh_base::menu_item;
use nsh_base::menu_separator;
use nsh_base::progress;
use nsh_core::Color;
use nsh_core::Hsla;
use nsh_core::element::ImageSrc;
use nsh_core::keymap::When;
use nsh_core::prelude::*;
use notify::RecommendedWatcher;
use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::replay_scan::ScanProgress;
use wows_toolkit_replay_data::replay_scan::ScannedReplay;
use wows_toolkit_replay_data::replay_scan::listed_builds;
use wows_toolkit_replay_data::replay_scan::read_scanned;
use wows_toolkit_replay_data::replay_scan::scan_replay_files;
use wows_toolkit_viewmodel::controls::CommandGroup;
use wows_toolkit_viewmodel::listing_row::LinePart;
use wows_toolkit_viewmodel::theme::DARK_SEMANTIC;
use wows_toolkit_viewmodel::theme::LIGHT_SEMANTIC;

use crate::game_data::GameDataState;
use crate::replay_inspector::constants::BuildNumber;
use crate::replay_inspector::error::InspectorError;
use crate::replay_inspector::settings;

use self::actions::Controls;
use self::listing::Leaf;
use self::listing::Listing;
use self::listing::Naming;
use self::listing::Node;
use self::listing::Pressed;
use self::listing::ShownIndex;
use self::listing::ShownRow;
use self::listing::Step;
use self::listing::Upserted;
use self::watch::DirChange;

/// A row, in multiples of the 16-point base font: the identity line, the
/// smaller stats line under it, and a little space around both.
const ROW_LINE_HEIGHTS: f32 = 2.8;

/// What one level of the tree indents by.
const INDENT: f32 = 16.0;

/// How the listing groups its replays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum ListingGrouping {
    #[default]
    Date,
    Ship,
    None,
}

impl From<ListingGrouping> for ReplayGrouping {
    fn from(grouping: ListingGrouping) -> Self {
        match grouping {
            ListingGrouping::Date => ReplayGrouping::Date,
            ListingGrouping::Ship => ReplayGrouping::Ship,
            ListingGrouping::None => ReplayGrouping::None,
        }
    }
}

/// Where a batch render goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum RenderTarget {
    /// Video files in a directory the reader picks.
    #[default]
    Video,
    /// Video files put on the clipboard.
    Clipboard,
}

/// How a notice is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum NoticeLevel {
    #[default]
    Ok,
    Info,
    Warn,
    Failed,
}

/// One labelled fact of a hovered replay.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct HoverFactEntry {
    pub label: SharedString,
    pub value: SharedString,
}

/// The replay the pointer is on, for the hover preview.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub struct ReplayHover {
    pub path: SharedString,
    /// The map as the replay names it (`spaces/00_CO_ocean`).
    pub map_name: SharedString,
    /// The pointer, in the root view's points.
    pub x: f32,
    pub y: f32,
    pub facts: Vec<HoverFactEntry>,
}

/// A build the listing holds.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct ListedBuildEntry {
    pub build: BuildNumber,
    /// The `major.minor.patch` its replays name.
    pub version: Option<SharedString>,
    pub replays: u32,
    /// The install or the game data cache can read it.
    pub readable: bool,
}

#[derive(Debug, Clone, PartialEq, Props)]
pub struct ReplayBrowserProps {
    /// A directory the reader chose; without one, the install's replays
    /// directory from the settings.
    pub directory: Prop<Option<SharedString>>,
    #[prop(default)]
    pub grouping: Prop<ListingGrouping>,
    /// A change reads the settings again and rescans.
    #[prop(default)]
    pub rescan: Prop<u32>,
    /// Offer the render items.
    #[prop(default)]
    pub can_render: Prop<bool>,
}

#[derive(Debug, Clone, PartialEq, Event)]
pub enum ReplayBrowserEvent {
    #[args(path)]
    Open(SharedString),
    #[args(path)]
    OpenInNewTab(SharedString),
    #[args(path)]
    Render(SharedString),
    #[args(paths, target)]
    RenderMany(Vec<SharedString>, RenderTarget),
    #[args(hover)]
    PreviewHover(ReplayHover),
    /// The pointer left the rows.
    PreviewLeave,
    /// The game wrote a replay into the watched directory, and the listing
    /// holds it now.
    #[args(path)]
    ReplayAppeared(SharedString),
    /// A listed replay was written to again, and the listing holds the new
    /// read.
    #[args(path)]
    ReplayChanged(SharedString),
    /// The builds the listing holds, after every scan and watched change.
    #[args(builds)]
    BuildsListed(Vec<ListedBuildEntry>),
    #[args(text, level)]
    Notice(SharedString, NoticeLevel),
}

/// Where the scan stands.
#[derive(Clone)]
enum Status {
    Scanning,
    Loaded,
    Empty,
    Failed(Rc<InspectorError>),
}

impl PartialEq for Status {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Failed(a), Self::Failed(b)) => Rc::ptr_eq(a, b),
            (Self::Scanning, Self::Scanning) | (Self::Loaded, Self::Loaded) | (Self::Empty, Self::Empty) => true,
            _ => false,
        }
    }
}

/// What the panel shows below the header.
#[derive(Debug, Clone, PartialEq)]
enum Body {
    Scanning,
    Note(String),
    Tree,
}

/// The game data the rows are named against, as last seen.
#[derive(Clone, Default)]
struct GameView {
    current: Option<Arc<LoadedGameData>>,
    has_cache: bool,
    loading: Option<LoadingBuild>,
}

/// The installed build while it loads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadingBuild {
    /// The version it names, when it names one.
    version: Option<String>,
}

/// Counts the scans, so an earlier scan (and its watch) cannot write over a
/// later one's listing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ScanGeneration(u64);

impl ScanGeneration {
    fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

impl GameView {
    fn same_naming(&self, other: &Self) -> bool {
        let same_current = match (&self.current, &other.current) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };

        same_current && self.has_cache == other.has_cache
    }
}

/// The listing's state and what acts on it. `Copy`: handles only, so every
/// closure in the view can hold it.
#[derive(Clone, Copy)]
struct Browser {
    cx: Scope<ReplayBrowserEvent>,
    /// The component's own owner, which tasks started from menus run under:
    /// a menu's items go when it closes.
    owner: Owner,
    can_render: Prop<bool>,

    listing: Signal<Listing>,
    status: Signal<Status>,
    progress: Signal<ScanProgress>,
    /// Listing the install's directory rather than a chosen one.
    install_listing: Signal<bool>,
    game: Signal<GameView>,

    /// The install, for "Open in Game", whatever directory is listed.
    wows_dir: Stored<Option<PathBuf>>,
    generation: Stored<ScanGeneration>,
    /// Held because dropping it stops the watch.
    watcher: Stored<Option<RecommendedWatcher>>,
    hovered: Stored<Option<PathBuf>>,

    focused: Signal<bool>,
    /// A row was pressed since the listing last lost the keyboard focus.
    /// Linux gives views no keyboard focus from a press or `focus()`
    /// (nsh-platform-linux `text_input.rs:1388`), so a press is what makes
    /// the listing the one the keys act on.
    engaged: Signal<bool>,
    controls: Signal<Option<Rc<Vec<CommandGroup>>>>,
    keys: ElementRef,
    list: ElementRef,
}

#[component]
pub fn ReplayBrowser(props: ReplayBrowserProps, cx: Scope<ReplayBrowserEvent>) -> impl IntoElement {
    let ReplayBrowserProps { directory, grouping, rescan, can_render } = props;
    let browser = Browser {
        cx,
        owner: Owner::current(),
        can_render,
        listing: signal_with(Listing::default(), SignalEquals::Never),
        status: signal(Status::Scanning),
        progress: signal(ScanProgress::default()),
        install_listing: signal(true),
        game: signal_with(GameView::default(), SignalEquals::Never),
        wows_dir: stored(None),
        generation: stored(ScanGeneration::default()),
        watcher: stored(None),
        hovered: stored(None),
        focused: signal(false),
        engaged: signal(false),
        controls: signal_with(None, SignalEquals::Never),
        keys: element_ref(),
        list: element_ref(),
    };

    effect(move || {
        let directory = directory.get().map(|directory| PathBuf::from(directory.as_str()));
        rescan.get();
        untracked(move || browser.scan(directory));
    });
    effect(move || {
        let grouping = ReplayGrouping::from(grouping.get());
        untracked(move || browser.listing.update(|listing| listing.set_grouping(grouping)));
    });
    browser.follow_game_data();
    browser.bind_keys();

    view()
        .app_class(class("flex-1 min-h-0 min-w-0"))
        .child(body(browser))
        .child(controls_dialog(browser))
}

impl Browser {
    fn emit(self, event: ReplayBrowserEvent) {
        self.cx.emit(event);
    }

    fn notice(self, text: impl Into<SharedString>, level: NoticeLevel) {
        self.emit(ReplayBrowserEvent::Notice(text.into(), level));
    }

    fn is_stale(self, generation: ScanGeneration) -> bool {
        self.generation.get() != generation
    }

    /// Reads the settings and walks `directory`, or the install's replays
    /// directory without one, then watches it.
    fn scan(self, directory: Option<PathBuf>) {
        let generation = self.generation.with(|generation| {
            *generation = generation.next();
            *generation
        });
        // The directory the watch was on is no longer the one to watch.
        self.watcher.replace(None);
        self.install_listing.set(directory.is_none());
        self.progress.set(ScanProgress::default());
        self.status.set(Status::Scanning);

        spawn(async move {
            let located = blocking(move || locate(directory)).await;
            if self.is_stale(generation) {
                return;
            }
            let located = match located {
                Ok(Ok(located)) => located,
                Ok(Err(err)) => return self.fail(err),
                Err(source) => return self.fail(InspectorError::Task { source }),
            };
            self.wows_dir.replace(located.wows_dir);

            let dir = located.dir;
            let (reports, mut progress) = tokio::sync::mpsc::unbounded_channel();
            let scanning = blocking({
                let dir = dir.clone();
                move || {
                    scan_replay_files(&dir, |report| {
                        let _ = reports.send(report);
                    })
                }
            });
            while let Some(report) = progress.recv().await {
                if self.is_stale(generation) {
                    return;
                }
                self.progress.set(report);
            }
            let files = match scanning.await {
                Ok(files) => files,
                Err(source) => return self.fail(InspectorError::Task { source }),
            };
            if self.is_stale(generation) {
                return;
            }

            let status = if files.is_empty() { Status::Empty } else { Status::Loaded };
            self.listing.update(|listing| listing.set_files(files));
            self.status.set(status);
            self.watch(&dir, generation);
            self.load_summaries();
            self.report_builds();
        });
    }

    fn fail(self, err: InspectorError) {
        tracing::warn!("replay listing: the scan failed: {err}");
        self.status.set(Status::Failed(Rc::new(err)));
    }

    fn watch(self, dir: &Path, generation: ScanGeneration) {
        let (watcher, mut changes) = match watch::watch(dir) {
            Ok(watching) => watching,
            Err(failure) => {
                tracing::warn!(dir = %dir.display(), "replay listing: {failure}");
                self.notice(failure.to_string(), NoticeLevel::Warn);
                return;
            }
        };
        self.watcher.replace(Some(watcher));

        spawn(async move {
            while let Some(change) = changes.recv().await {
                if self.is_stale(generation) {
                    return;
                }
                let path = match change {
                    DirChange::Removed(path) => {
                        self.listing.update(|listing| {
                            listing.forget(&path);
                        });
                        continue;
                    }
                    DirChange::Appeared(path) | DirChange::Changed(path) => path,
                };

                let Some(scanned) = read_when_complete(path).await else { continue };
                if self.is_stale(generation) {
                    return;
                }
                let touched = shared_path(&scanned.path);
                let mut upserted = Upserted::Added;
                self.listing.update(|listing| upserted = listing.upsert(scanned));
                self.status.set(Status::Loaded);
                match upserted {
                    Upserted::Added => self.emit(ReplayBrowserEvent::ReplayAppeared(touched)),
                    Upserted::Replaced => self.emit(ReplayBrowserEvent::ReplayChanged(touched)),
                }
                self.load_summaries();
                self.report_builds();
            }
        });
    }

    /// Reads what the replay index knows about the listed files: one query
    /// over the live source rather than one per row.
    fn load_summaries(self) {
        self.owner.run(|| {
            spawn(async move {
                let loaded = blocking(|| {
                    settings::with_pool(|pool| async move {
                        let summaries_failed = |source| InspectorError::IndexSummaries { source };
                        let Some(source) = query::live_source_id(&pool).await.map_err(summaries_failed)? else {
                            return Ok(HashMap::new());
                        };
                        query::row_summaries_for_source(&pool, source).await.map_err(summaries_failed)
                    })
                })
                .await;

                match loaded {
                    Ok(Ok(summaries)) => self.listing.update(|listing| listing.set_summaries(summaries)),
                    Ok(Err(err)) => tracing::warn!("replay listing: the index summaries could not be read: {err}"),
                    Err(err) => tracing::warn!("replay listing: the index summaries did not load: {err}"),
                }
            })
        });
    }

    /// Says which builds the listing holds and which of them nothing on this
    /// machine can read.
    fn report_builds(self) {
        let builds = self.listing.with_untracked(|listing| listed_builds(listing.files()));
        self.owner.run(|| {
            spawn(async move {
                let entries = blocking(move || {
                    let cache = crate::game_data::cache();
                    builds
                        .into_iter()
                        .map(|listed| ListedBuildEntry {
                            readable: cache
                                .as_ref()
                                .is_some_and(|cache| cache.can_read_build(listed.build, listed.version.as_deref())),
                            build: BuildNumber(listed.build),
                            version: listed.version.map(SharedString::from),
                            // Saturates: no directory holds four billion replays.
                            replays: u32::try_from(listed.replays).unwrap_or(u32::MAX),
                        })
                        .collect::<Vec<_>>()
                })
                .await;

                match entries {
                    Ok(entries) => self.emit(ReplayBrowserEvent::BuildsListed(entries)),
                    Err(err) => tracing::warn!("replay listing: the listed builds were not checked: {err}"),
                }
            })
        });
    }

    /// Names the rows against the game data as it loads and changes.
    fn follow_game_data(self) {
        spawn(async move {
            let mut changes = crate::game_data::changes();
            loop {
                let seen = blocking(|| {
                    let status = crate::game_data::status();
                    let has_cache = crate::game_data::cache().is_some();
                    (status, has_cache)
                })
                .await;
                let Ok((status, has_cache)) = seen else { return };

                let next = match status {
                    GameDataState::Ready(loaded) => GameView { current: Some(loaded), has_cache, loading: None },
                    GameDataState::Loading { version } => {
                        GameView { current: None, has_cache, loading: Some(LoadingBuild { version }) }
                    }
                    GameDataState::NotConfigured | GameDataState::Failed { .. } => {
                        GameView { current: None, has_cache, loading: None }
                    }
                };
                let renamed = !self.game.with_untracked(|game| game.same_naming(&next));
                if renamed {
                    let naming = Naming {
                        current: next.current.clone(),
                        cache: crate::game_data::cache(),
                        locale: Some(rust_i18n::locale().to_string()),
                    };
                    self.listing.update(|listing| listing.set_naming(naming));
                }
                self.game.set(next);

                if changes.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    /// The listing's keys, bound while it has the keyboard focus or a row
    /// was last pressed.
    fn bind_keys(self) {
        const KEYS: &[(&str, bool, Key)] = &[
            ("up", true, Key::Step(Step::Previous)),
            ("down", true, Key::Step(Step::Next)),
            ("home", false, Key::Step(Step::First)),
            ("end", false, Key::Step(Step::Last)),
            ("left", true, Key::Step(Step::Out)),
            ("right", true, Key::Step(Step::In)),
            ("enter", false, Key::Activate),
        ];

        effect(move || {
            if !self.focused.get() && !self.engaged.get() {
                return;
            }
            for &(keystroke, repeat, key) in KEYS {
                on_key_with(keystroke, KeyOptions { when: When::NotEditing, repeat }, move |_| self.key(key));
            }
        });
    }

    fn key(self, key: Key) {
        match key {
            Key::Step(step) => {
                let mut moved = None;
                self.listing.update(|listing| moved = listing.step(step));
                if let Some(at) = moved {
                    self.list.scroll_to_index(
                        // Saturates: the list cannot hold more rows than a u32 counts.
                        u32::try_from(at.0).unwrap_or(u32::MAX),
                        Percentage(0.5),
                        Points(0.0),
                        false,
                    );
                }
            }
            Key::Activate => {
                let mut pressed = Pressed::Nothing;
                self.listing.update(|listing| pressed = listing.activate());
                self.open_pressed(pressed);
            }
        }
    }

    fn press(self, at: ShownIndex, press: &PressEvent) {
        self.keys.focus();
        self.engaged.set(true);

        let secondary = if apple_keys() { press.modifiers.cmd } else { press.modifiers.ctrl };
        let mut pressed = Pressed::Nothing;
        self.listing.update(|listing| {
            pressed = match press.clicks {
                // A keyboard or assistive technology activation: what Return does.
                0 => {
                    listing.press(at, false, 1);
                    listing.activate()
                }
                clicks => listing.press(at, secondary, clicks),
            };
        });
        self.open_pressed(pressed);
    }

    fn open_pressed(self, pressed: Pressed) {
        match pressed {
            Pressed::Open(path) => self.emit(ReplayBrowserEvent::Open(shared_path(&path))),
            Pressed::Nothing => {}
        }
    }

    /// The pointer came over the shown row `at`: a replay's preview follows
    /// it, once per replay.
    fn hover(self, at: ShownIndex, mouse: &MouseEvent) {
        let Some(Node::Leaf(leaf)) = self.listing.with_untracked(|listing| listing.shown_row(at)).map(|row| row.node)
        else {
            return;
        };
        if self.hovered.with(|hovered| hovered.as_deref() == Some(leaf.path.as_path())) {
            return;
        }

        self.hovered.replace(Some(leaf.path.clone()));
        self.emit(ReplayBrowserEvent::PreviewHover(ReplayHover {
            path: shared_path(&leaf.path),
            map_name: leaf.map_name.clone().into(),
            x: mouse.page_position.x.0,
            y: mouse.page_position.y.0,
            facts: leaf
                .hover
                .iter()
                .map(|fact| HoverFactEntry { label: fact.label.clone().into(), value: fact.value.clone().into() })
                .collect(),
        }));
    }

    fn leave(self) {
        if self.hovered.replace(None).is_some() {
            self.emit(ReplayBrowserEvent::PreviewLeave);
        }
    }

    /// Puts the replay files on the clipboard, so they paste into a file
    /// manager or an upload dialog rather than as their paths.
    fn copy_replays(self, paths: &[PathBuf]) {
        set_clipboard(actions::uri_list(paths));

        let said = match paths.len() {
            1 => t!("ui.replay.context.copy_replay"),
            count => t!("ui.replay.context.copy_replays", count = count),
        };
        self.notice(said.into_owned(), NoticeLevel::Ok);
    }

    fn copy_paths(paths: &[PathBuf]) {
        set_clipboard(actions::path_list(paths));
    }

    /// Records the replays in the session statistics. Replacing the session
    /// throws away everything recorded in it, so it is confirmed first.
    fn session_stats(self, paths: Vec<PathBuf>, replace: bool) {
        self.owner.run(|| {
            spawn(async move {
                if replace {
                    let confirmed = confirm(
                        t!("ui.replay.context.set_session_stats_one").into_owned(),
                        t!("confirm.set_as_session_stats").into_owned(),
                    )
                    .await;
                    if !confirmed {
                        return;
                    }
                }

                let recorded = blocking(move || actions::record_session_stats(&paths, replace))
                    .await
                    .map_err(|source| InspectorError::Task { source })
                    .flatten();
                let Err(failure) = recorded else {
                    return;
                };
                tracing::warn!("replay listing: {failure}");
                self.notice(failure.to_string(), NoticeLevel::Failed);
            })
        });
    }

    fn show_controls(self) {
        self.owner.run(|| {
            spawn(async move {
                let Ok(controls) = blocking(actions::read_controls).await else { return };
                match controls {
                    Controls::Groups(groups) => self.controls.set(Some(Rc::new(groups))),
                    Controls::Unavailable => self.notice(t!("ui.replay.controls.unavailable").into_owned(), NoticeLevel::Warn),
                    // Nothing is loaded to read the reference from.
                    Controls::NoBuild => {}
                }
            })
        });
    }

    /// Asks the game to play `replay`. Launching the game is not something to
    /// do by a mis-click, so it is confirmed first.
    fn open_in_game(self, wows_dir: PathBuf, replay: PathBuf) {
        self.owner.run(|| {
            spawn(async move {
                let confirmed = confirm(
                    t!("ui.replay.context.open_in_game").into_owned(),
                    t!("confirm.open_in_game").into_owned(),
                )
                .await;
                if !confirmed {
                    return;
                }

                let launched = blocking(move || {
                    wows_toolkit_launcher::open_replay_in_game(&wows_dir, replay, |_command| {})
                        .map_err(|source| InspectorError::Launch { source: Box::new(source) })
                })
                .await
                .map_err(|source| InspectorError::Task { source })
                .flatten();
                let Err(failure) = launched else {
                    return;
                };
                tracing::warn!("replay listing: the game could not be launched: {failure}");
                self.notice(failure.to_string(), NoticeLevel::Failed);
            })
        });
    }
}

/// A key the listing answers while it is the one in use.
#[derive(Debug, Clone, Copy)]
enum Key {
    Step(Step),
    Activate,
}

/// The directory to list, and the install beside it.
struct Located {
    wows_dir: Option<PathBuf>,
    dir: PathBuf,
}

/// Reads the settings for the install, and resolves the directory to list:
/// the chosen one, else the install's replays directory. Blocking.
fn locate(directory: Option<PathBuf>) -> Result<Located, InspectorError> {
    let settings = settings::load();
    let Some(dir) = directory else {
        let wows_dir = settings?.wows_dir.ok_or(InspectorError::WowsDirMissing)?;
        let dir = wows_toolkit_replay_data::load::resolve_replays_dir(&wows_dir);
        return Ok(Located { wows_dir: Some(wows_dir), dir });
    };

    // A chosen directory lists without the settings; only "Open in Game"
    // needs the install.
    let wows_dir = match settings {
        Ok(settings) => settings.wows_dir,
        Err(err) => {
            tracing::warn!("replay listing: the settings could not be read: {err}");
            None
        }
    };

    Ok(Located { wows_dir, dir })
}

/// Reads the header of a replay the watcher reported, retrying while the
/// game finishes writing it: the file appears before its header is
/// complete. `None` once the attempts run out, which is what a file that is
/// not a replay after all looks like.
async fn read_when_complete(path: PathBuf) -> Option<ScannedReplay> {
    const ATTEMPTS: usize = 10;
    const RETRY: Duration = Duration::from_millis(300);

    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            timer(RETRY).await;
        }
        let read = path.clone();
        if let Ok(Ok(scanned)) = blocking(move || read_scanned(&read)).await {
            return Some(scanned);
        }
    }

    tracing::warn!(path = %path.display(), "replay listing: a new replay never became readable");
    None
}

/// Shows a confirmation and resolves to whether the reader went ahead.
async fn confirm(title: String, description: String) -> bool {
    const PROCEED: &str = "proceed";

    let answer = alert_dialog()
        .title(title)
        .description(description)
        .cancel("cancel", t!("ui.buttons.cancel").into_owned())
        .action(PROCEED, "OK")
        .show()
        .await;

    answer.is_ok_and(|answer| answer.action.as_deref() == Some(PROCEED))
}

fn class(value: &'static str) -> Option<SharedString> {
    Some(SharedString::from(value))
}

fn shared_path(path: &Path) -> SharedString {
    SharedString::from(path.to_string_lossy().into_owned())
}

fn hsla(rgb: u32) -> Hsla {
    Hsla::from(Color(rgb << 8 | 0xff))
}

fn body(browser: Browser) -> AnyElement {
    let shown = computed(move || match browser.status.get() {
        Status::Scanning => Body::Scanning,
        Status::Failed(reason) => Body::Note(reason.to_string()),
        Status::Empty => Body::Note(t!("ui.replay.no_replays").into_owned()),
        // A list built before the game data loads is a list of raw ids; a
        // chosen directory lists at once.
        Status::Loaded => match browser.game.with(|game| game.loading.clone()) {
            Some(LoadingBuild { version }) if browser.install_listing.get() => Body::Note(match version {
                Some(version) => t!("ui.replay.loading_game_data_version", version = version).into_owned(),
                None => t!("ui.replay.loading_game_data").into_owned(),
            }),
            _ => Body::Tree,
        },
    });

    dynamic(move || match shown.get() {
        Body::Scanning => scanning(browser),
        Body::Note(said) => text(said).app_class(class("p-2 text-sm text-muted-foreground")).into_element(),
        Body::Tree => tree(browser),
    })
}

fn scanning(browser: Browser) -> AnyElement {
    let caption = move || {
        let ScanProgress { done, total } = browser.progress.get();
        // No total yet: the walk itself is still running.
        let said = match total {
            0 => t!("ui.replay.scanning"),
            total => t!("ui.replay.scanning_count", done = done, total = total),
        };
        SharedString::from(said.into_owned())
    };
    let percent = move || {
        let ScanProgress { done, total } = browser.progress.get();
        if total == 0 { 0.0 } else { done as f32 / total as f32 * 100.0 }
    };

    // The walk has no total to measure against, so it shows activity alone.
    let walking = move || browser.progress.with(|progress| progress.total == 0);

    view()
        .app_class(class("p-2 gap-1"))
        .child(
            view()
                .app_class(class("flex-row items-center gap-2"))
                .child(when(walking, move || {
                    progress()
                        .indeterminate(true)
                        .accessibility_label(move || Some(caption()))
                        .track_class("w-[60px] h-1.5 rounded-full bg-muted overflow-hidden")
                        .indicator_class("h-full rounded-full bg-primary")
                }))
                .child(text(caption).app_class(class("text-sm text-muted-foreground"))),
        )
        .child(when(
            move || browser.progress.with(|progress| progress.total > 0),
            move || {
                progress()
                    .value(percent)
                    .accessibility_label(move || Some(caption()))
                    .track_class("h-1.5 rounded-full bg-muted overflow-hidden")
                    .indicator_class("h-full rounded-full bg-primary")
            },
        ))
        .into_element()
}

fn tree(browser: Browser) -> AnyElement {
    let rows = virtual_list(
        move || browser.listing.with(Listing::shown_len),
        move |at: Position| row(browser, at),
    )
    .estimated_item_size(move || row_height())
    .element_ref(&browser.list)
    .app_class(class("flex-1 min-h-0"));

    view()
        .role(AccessibilityRole::Tree)
        .focusable(true)
        .element_ref(&browser.keys)
        .on_focus(move || browser.focused.set(true))
        .on_blur(move || {
            browser.focused.set(false);
            browser.engaged.set(false);
        })
        .on_mouse_leave(move |_: &MouseEvent| browser.leave())
        .app_class(class("flex-1 min-h-0"))
        .child(rows)
        .into_element()
}

fn row(browser: Browser, at: Position) -> AnyElement {
    let at = ShownIndex(at.index());
    let shown = computed(move || browser.listing.with(|listing| listing.shown_row(at)));

    let content = view()
        .accessible(true)
        .role(AccessibilityRole::TreeItem)
        .focusable(false)
        // A row past the end, briefly drawn while the list shrinks, has no label.
        .label(move || shown.with(|row| row.as_ref().map(row_label).unwrap_or_default()))
        .accessibility_state(move || {
            shown.with(|row| AccessibilityState {
                selected: row.as_ref().is_some_and(|row| row.selected),
                expanded: row.as_ref().and_then(|row| matches!(row.node, Node::Group(_)).then_some(row.expanded)),
                ..AccessibilityState::default()
            })
        })
        .accessibility_level(move || shown.with(|row| row.as_ref().map_or(0, |row| row.depth + 1)))
        .app_class(move || shown.with(|row| class(row_class(row.as_ref()))))
        .on_press(move |press: &PressEvent| browser.press(at, press))
        .on_mouse_enter(move |mouse: &MouseEvent| browser.hover(at, mouse))
        .child(dynamic(move || match shown.get() {
            Some(row) => row_content(&row),
            None => Vec::new(),
        }));

    let menu = context_menu()
        .trigger(content)
        .popup_class("min-w-56 p-1 rounded-(--radius) border border-border bg-popover")
        .child(dynamic(move || match shown.get() {
            Some(row) => menu_items(browser, &row),
            None => Vec::new(),
        }));

    // A fixed height: the list places rows by their measured size, and the
    // menu's closed popup must not add to it.
    view()
        .h(move || row_height())
        .app_class(class("w-full min-w-0 overflow-hidden"))
        .child(menu)
        .into_element()
}

/// Both lines at the text size the reader chose.
fn row_height() -> Points {
    px(16.0 * ROW_LINE_HEIGHTS * font_scale())
}

fn row_label(row: &ShownRow) -> SharedString {
    match &row.node {
        Node::Group(group) => SharedString::from(group.label.to_string()),
        Node::Leaf(leaf) => SharedString::from(leaf.file_name.clone()),
    }
}

fn row_class(row: Option<&ShownRow>) -> &'static str {
    let Some(row) = row else {
        return "flex-row items-center gap-1 h-full w-full min-w-0 pr-2";
    };

    match (row.selected, row.stripe) {
        (true, _) => "flex-row items-center gap-1 h-full w-full min-w-0 pr-2 bg-accent",
        (false, true) => "flex-row items-center gap-1 h-full w-full min-w-0 pr-2 bg-muted/40 hover:bg-accent/60",
        (false, false) => "flex-row items-center gap-1 h-full w-full min-w-0 pr-2 hover:bg-accent/60",
    }
}

/// A row's indent, caret and two lines: the identity tinted by the outcome
/// and closed by the division glyph, then the stats line.
fn row_content(row: &ShownRow) -> Vec<AnyElement> {
    let indent = view().w(px(INDENT * row.depth as f32)).into_element();

    match &row.node {
        Node::Group(group) => {
            let caret = if row.expanded { glyph::CARET_DOWN } else { glyph::CARET_RIGHT };
            vec![
                indent,
                glyph_view(caret, "text-muted-foreground"),
                text(group.label.to_string())
                    .app_class(class("flex-1 min-w-0 text-sm font-semibold text-foreground truncate"))
                    .into_element(),
            ]
        }
        Node::Leaf(leaf) => vec![indent, view().w(px(INDENT)).into_element(), leaf_lines(leaf)],
    }
}

fn leaf_lines(leaf: &Leaf) -> AnyElement {
    let tint = match leaf.outcome {
        MatchOutcome::Win => "shrink min-w-0 text-sm text-win truncate",
        MatchOutcome::Loss => "shrink min-w-0 text-sm text-loss truncate",
        MatchOutcome::Draw => "shrink min-w-0 text-sm text-draw truncate",
        MatchOutcome::Unknown => "shrink min-w-0 text-sm text-foreground truncate",
    };
    let mut identity =
        view().app_class(class("flex-row items-center gap-1 min-w-0 w-full")).child(text(leaf.label.clone()).app_class(class(tint)));
    if leaf.in_division {
        identity = identity.child(
            svg(ImageSrc::svg_markup(glyph::USERS_THREE))
                .w(px(12.0))
                .h(px(12.0))
                .text_color(AdaptiveColor { light: hsla(LIGHT_SEMANTIC.division), dark: hsla(DARK_SEMANTIC.division) }),
        );
    }

    let stats = view().app_class(class("flex-row items-center min-w-0 overflow-hidden")).children(
        leaf.stats.iter().map(|part| match part {
            LinePart::Text(said) => text(said.clone())
                .app_class(class("text-xs text-muted-foreground whitespace-nowrap"))
                .into_element(),
            LinePart::Glyph(glyph) => match glyph::markup(glyph) {
                Some(markup) => glyph_view(markup, "text-muted-foreground"),
                None => text(*glyph).app_class(class("text-xs text-muted-foreground")).into_element(),
            },
        }),
    );

    view()
        .app_class(class("flex-1 min-w-0 justify-center overflow-hidden"))
        .child(identity)
        .child(stats)
        .into_element()
}

fn glyph_view(markup: &'static str, color: &'static str) -> AnyElement {
    svg(ImageSrc::svg_markup(markup)).w(px(12.0)).h(px(12.0)).app_class(class(color)).into_element()
}

fn menu_item_view(label: String, on_select: impl Fn() + 'static) -> AnyElement {
    menu_item()
        .accessibility_label(label.clone())
        .app_class(class("min-h-7 px-2 rounded-(--radius) justify-center hover:bg-accent"))
        .on_select(on_select)
        .child(text(label).app_class(class("text-xs text-popover-foreground")))
        .into_element()
}

fn separator() -> AnyElement {
    menu_separator().app_class(class("h-px my-1 bg-border")).into_element()
}

fn menu_items(browser: Browser, row: &ShownRow) -> Vec<AnyElement> {
    let leaf = match &row.node {
        Node::Group(group) => return group_menu(browser, group.paths.clone()),
        Node::Leaf(leaf) => Rc::clone(leaf),
    };
    let path = leaf.path.clone();
    let can_render = browser.can_render.get_untracked();

    // Right-clicking a replay outside the marked set acts on that replay,
    // not on a set the reader has moved on from.
    let selection = browser.listing.with_untracked(Listing::selection);
    let batch = if selection.len() > 1 && selection.contains(&path) { selection } else { Vec::new() };

    let mut items = Vec::new();
    if !batch.is_empty() {
        let count = batch.len();
        let batch = Rc::new(batch.clone());
        items.push(menu_item_view(t!("ui.replay.context.copy_replays", count = count).into_owned(), {
            let batch = Rc::clone(&batch);
            move || browser.copy_replays(&batch)
        }));
        items.push(menu_item_view(t!("ui.replay.context.copy_paths", count = count).into_owned(), {
            let batch = Rc::clone(&batch);
            move || Browser::copy_paths(&batch)
        }));
        if can_render {
            items.push(menu_item_view(t!("ui.replay.context.render_to_video_many", count = count).into_owned(), {
                let batch = Rc::clone(&batch);
                move || browser.emit(ReplayBrowserEvent::RenderMany(shared_paths(&batch), RenderTarget::Video))
            }));
            items.push(menu_item_view(t!("ui.replay.context.render_to_clipboard_many", count = count).into_owned(), {
                let batch = Rc::clone(&batch);
                move || browser.emit(ReplayBrowserEvent::RenderMany(shared_paths(&batch), RenderTarget::Clipboard))
            }));
        }
        items.push(separator());
    }

    let shared = shared_path(&path);
    items.push(menu_item_view(t!("ui.collab.open").into_owned(), {
        let shared = shared.clone();
        move || browser.emit(ReplayBrowserEvent::Open(shared.clone()))
    }));
    items.push(menu_item_view(t!("ui.replay.context.open_in_new_tab").into_owned(), {
        let shared = shared.clone();
        move || browser.emit(ReplayBrowserEvent::OpenInNewTab(shared.clone()))
    }));
    if can_render {
        items.push(menu_item_view(t!("ui.replay.context.render_replay").into_owned(), {
            let shared = shared.clone();
            move || browser.emit(ReplayBrowserEvent::Render(shared.clone()))
        }));
    }
    items.push(separator());

    let session = Rc::new(if batch.is_empty() { vec![path.clone()] } else { batch });
    let (set_label, add_label) = match session.len() {
        1 => (t!("ui.replay.context.set_session_stats_one"), t!("ui.replay.context.add_session_stats_one")),
        count => (
            t!("ui.replay.context.set_session_stats_many", count = count),
            t!("ui.replay.context.add_session_stats_many", count = count),
        ),
    };
    items.push(menu_item_view(set_label.into_owned(), {
        let session = Rc::clone(&session);
        move || browser.session_stats(session.to_vec(), true)
    }));
    items.push(menu_item_view(add_label.into_owned(), {
        let session = Rc::clone(&session);
        move || browser.session_stats(session.to_vec(), false)
    }));
    items.push(separator());

    let single = Rc::new(vec![path.clone()]);
    items.push(menu_item_view(t!("ui.replay.context.copy_replay").into_owned(), {
        let single = Rc::clone(&single);
        move || browser.copy_replays(&single)
    }));
    items.push(menu_item_view(t!("ui.replay.context.copy_path").into_owned(), {
        let single = Rc::clone(&single);
        move || Browser::copy_paths(&single)
    }));
    items.push(menu_item_view(t!("ui.replay.context.show_in_explorer").into_owned(), {
        let path = path.clone();
        move || actions::reveal(&path)
    }));
    if browser.game.with_untracked(|game| game.has_cache) {
        items.push(menu_item_view(t!("ui.replay.context.show_replay_controls").into_owned(), move || {
            browser.show_controls()
        }));
    }
    if let Some(wows_dir) = browser.wows_dir.get() {
        items.push(menu_item_view(t!("ui.replay.context.open_in_game").into_owned(), move || {
            browser.open_in_game(wows_dir.clone(), path.clone())
        }));
    }

    items
}

/// A group's menu copies every replay under it, which is what a batch
/// action on a date or a ship starts from.
fn group_menu(browser: Browser, paths: Vec<PathBuf>) -> Vec<AnyElement> {
    if paths.is_empty() {
        return Vec::new();
    }

    let count = paths.len();
    let paths = Rc::new(paths);
    vec![
        menu_item_view(t!("ui.replay.context.copy_replays", count = count).into_owned(), {
            let paths = Rc::clone(&paths);
            move || browser.copy_replays(&paths)
        }),
        menu_item_view(t!("ui.replay.context.copy_paths", count = count).into_owned(), move || {
            Browser::copy_paths(&paths)
        }),
    ]
}

fn shared_paths(paths: &[PathBuf]) -> Vec<SharedString> {
    paths.iter().map(|path| shared_path(path)).collect()
}

/// The game's keyboard reference while a replay plays, over the window.
/// Escape or a press beside it closes it.
fn controls_dialog(browser: Browser) -> AnyElement {
    when(
        move || browser.controls.with(Option::is_some),
        move || {
            let groups = browser.controls.get_untracked().unwrap_or_default();
            let close = move || browser.controls.set(None);

            let panel = view()
                .w(px(560.0))
                .app_class(class("gap-3 p-4 rounded-(--radius) border border-border bg-popover"))
                .on_press(|| {})
                .focusable(false)
                .child(
                    text(t!("ui.replay.controls.window_title").into_owned())
                        .app_class(class("text-base font-semibold text-popover-foreground")),
                )
                .child(
                    scroll()
                        .h(px(520.0))
                        .child(view().app_class(class("gap-4 p-2")).children(groups.iter().map(command_group))),
                );

            dialog().animation(ModalAnimation::Fade).on_cancel(close).child(
                view()
                    .app_class(class("flex-1 items-center justify-center bg-black/50"))
                    .focusable(false)
                    .on_press(close)
                    .child(panel),
            )
        },
    )
    .into_element()
}

fn command_group(group: &CommandGroup) -> AnyElement {
    let rows = group.commands.iter().map(|command| {
        let keys = match &command.key2 {
            Some(second) => format!("{}  /  {}", command.key1, second),
            None => command.key1.clone(),
        };
        view()
            .app_class(class("flex-row items-center gap-2"))
            .child(text(command.label.clone()).app_class(class("flex-1 min-w-0 text-sm text-popover-foreground")))
            .child(text(keys).w(px(180.0)).font_family("monospace").app_class(class("text-sm text-muted-foreground")))
            .into_element()
    });

    view()
        .app_class(class("gap-1"))
        .child(text(group.title).app_class(class("text-sm font-bold text-popover-foreground")))
        .children(rows)
        .into_element()
}
