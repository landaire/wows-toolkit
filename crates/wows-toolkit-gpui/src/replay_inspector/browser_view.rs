//! Left-panel file browser: background replay discovery, a grouped tree
//! view, and the single-click-select / double-click-open interaction.
//!
//! **Discovery.** The egui app's replay directory is `{wows_dir}/replays`,
//! optionally overridden by a build-specific subdirectory read from
//! `preferences.xml`'s `<last_server_version>` node when that subdirectory
//! exists on disk (`task/replays.rs::load_wows_files`, the
//! `available_builds`/`latest_build` selection dropped -- this module only
//! needs the replays directory, not a loaded game build). `resolve_replays_dir`
//! ports that lookup; `scan_replay_files` then lists `*.wowsreplay` files
//! (skipping `temp.wowsreplay`, matching `task/replays.rs::replay_filepaths`)
//! and reads each one's header via `ReplayFile::meta_from_file` -- plaintext
//! metadata only, no packet decryption/decompression, so this is safe to run
//! for every file in the directory on every scan.
//!
//! **What a row says.** The scan reads only a replay's plaintext header, into
//! the shared [`ListedReplay`]; `rebuild_tree` then assembles both of a row's
//! lines from it exactly as the egui listing does
//! (`wows_toolkit_viewmodel::listing_row`): the identity line against the
//! loaded `GameMetadataProvider`, the stats line against the replay index's
//! summary for that file. A file the index has not seen says so rather than
//! showing blank figures, and the panel waits for the game data rather than
//! listing raw ship and map ids (see `render`).

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::tree::TreeItem;
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::tree::tree;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::GameParamId;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_viewmodel::listing_row::HoverFact;
use wows_toolkit_viewmodel::listing_row::LinePart;
use wows_toolkit_viewmodel::listing_row::ListedReplay;
use wows_toolkit_viewmodel::listing_row::hover_facts;
use wows_toolkit_viewmodel::listing_row::listed_row_identity;
use wows_toolkit_viewmodel::listing_row::resolve_row_stats;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::translations::translate_map_name;

use super::browser::BrowserNode;
use super::browser::ReplayLite;
use super::browser::build_browser_tree;
use super::columns::BattleOutcome;
use super::columns::ColorRole;
use crate::preview_hover::PreviewHover;

use super::load::GameDataCache;
use super::load::GameDataStatus;
use super::table::resolve_color;

/// The egui app's `t!("ui.replay.spectator")` value
/// (`crates/wt-translations/translations/en.toml`'s `[ui.replay] spectator`
/// key), hardcoded per this crate's no-i18n-wired convention (see the module
/// doc and `panel.rs`'s equivalent chat-tooltip literal). Shown for a replay
/// whose header names no relation-0 vehicle, or whose vehicle's ship id has
/// no resolvable translation, once game data is loaded -- mirroring
/// `Replay::vehicle_name`'s own fallback (`mod.rs:2580`).
const SPECTATOR_LABEL: &str = "Spectator";

/// One replay's header-only scan result, before ship/map translation (see the
/// module doc). `ship_id` is the relation-0 vehicle's `shipId`, mirroring
/// `Replay::player_vehicle` (`mod.rs:2576`); `raw_ship`/`raw_map` are the
/// untranslated fallbacks `translate_replay` uses while `game_data` is not
/// yet loaded.
struct RawReplay {
    path: PathBuf,
    listed: ListedReplay,
}

/// A leaf's path and (usually absent, see the module doc) battle result,
/// looked up by tree-item id during rendering. Groups need no side table:
/// their label already encodes everything they display.
#[derive(Clone)]
struct LeafInfo {
    path: PathBuf,
    /// The map as the replay names it, so the hover can draw the map before
    /// the replay has been read.
    map_name: String,
    /// The row's second line: damage, kills and the timestamp, in the pieces
    /// it is drawn from (the glyphs go in the icon font).
    stats: Rc<Vec<LinePart>>,
    outcome: MatchOutcome,
    in_division: bool,
    /// What the hover popup says under the minimap: one labelled fact per
    /// line, laid out as a grid rather than run together.
    hover: Rc<Vec<HoverFact>>,
}

/// Writes `paths` to the clipboard, one per line.
fn copy_paths(paths: &[PathBuf], cx: &mut App) {
    let text = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

/// Puts the replay files themselves on the clipboard, so they paste into a
/// file manager or an upload dialog rather than as their paths.
///
/// GPUI's clipboard has no file-list format, so this goes through `arboard`
/// directly, which is what the egui app's `copy_files_to_clipboard` does.
fn copy_replay_files(paths: &[PathBuf], window: &mut Window, cx: &mut App) {
    let outcome = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set().file_list(paths));
    match outcome {
        Ok(()) => {
            let message = if paths.len() == 1 {
                t!("ui.replay.context.copy_replay").into_owned()
            } else {
                t!("ui.replay.context.copy_replays", count = paths.len()).into_owned()
            };
            crate::toast::ok(message, window, cx);
        }
        Err(err) => {
            tracing::warn!("replay browser: the replay files could not be copied: {err}");
            crate::toast::failed(err.to_string(), window, cx);
        }
    }
}

/// Asks the game to play `replay`.
///
/// The client takes a replay path on its command line, which is what the
/// egui app's confirmed `OpenInGame` action does. A launch that fails says
/// so rather than leaving the reader waiting for a window.
fn open_replay_in_game(wows_dir: &str, replay: &Path, window: &mut Window, cx: &mut App) {
    let exe = Path::new(wows_dir).join("WorldOfWarships.exe");
    match std::process::Command::new(&exe).arg(replay).spawn() {
        Ok(_) => {}
        Err(err) => {
            tracing::warn!("replay browser: {} could not be launched: {err}", exe.display());
            crate::toast::failed(err.to_string(), window, cx);
        }
    }
}

/// Opens the system file manager with `path` selected.
///
/// Best effort: a file manager that is not there, or refuses, leaves a log
/// line rather than an error the listing would have to carry.
fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let command = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(target_os = "macos")]
    let command = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let command = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();

    if let Err(err) = command {
        tracing::warn!("replay listing: {} could not be shown in the file manager: {err}", path.display());
    }
}

/// A row's height, in multiples of the theme's font size: the identity line,
/// the smaller stats line under it, and a little space around both.
const ROW_LINE_HEIGHTS: f32 = 2.8;

/// What one level of the tree indents by, and the width the guide for it is
/// drawn in.
const INDENT: Pixels = px(16.);

/// The hover grid's label column, wide enough for the longest of them.
const HOVER_LABEL_WIDTH: Pixels = px(58.);

/// How far to the right of the pointer the hover preview sits, so it never
/// covers the row it belongs to.
const PREVIEW_CURSOR_OFFSET: Pixels = px(24.);

/// The preview's edge length. Square, as the minimap is; the egui app's own
/// popup uses 384 (`preview_popup::PREVIEW_SIZE`).
const PREVIEW_SIZE: f32 = 384.;

/// Closes the identity line of a battle played in a division, the way the
/// egui row does (`listing_row::row_layout_job`).
const DIVISION_GLYPH: &str = wows_toolkit_viewmodel::glyphs::USERS_THREE;

/// The egui semantic palette's `division` (`ui/theme/semantic.rs`), which is
/// what tints that glyph there.
const DIVISION_COLOR: u32 = 0xE5C158;

/// Background scan progress, driving the panel's content below the header.
enum ScanStatus {
    Loading,
    Loaded,
    Empty,
    Failed(ScanError),
}

/// How far the directory scan has got.
///
/// The walk that finds the candidate files is a single cheap pass, so `total`
/// is known before the first meta is read; `done` then climbs through it.
/// A total of zero means the walk itself is still running, which reads as
/// "scanning" with no denominator to state.
#[derive(Clone, Copy, Default)]
struct ScanProgress {
    done: usize,
    total: usize,
}

/// Files read between one progress report and the next.
///
/// Reporting every file would post a message and a repaint per meta read,
/// which is more work than the read itself on a warm directory.
const SCAN_REPORT_EVERY: usize = 16;

/// Reasons `start_scan` can fail before it ever reaches the background
/// thread. Only the case `start_scan` actually produces is represented; the
/// per-file read failures inside `scan_replay_files` are logged and skipped
/// rather than aborting the scan (see that function's doc comment), so they
/// never reach `ScanStatus::Failed`.
#[derive(Debug, thiserror::Error)]
enum ScanError {
    #[error("World of Warships directory is not set")]
    WowsDirMissing,
}

/// Event emitted when the user double-clicks a replay leaf. Milestone 5's
/// dock wiring subscribes to this to parse and open the replay; until then
/// nothing consumes it beyond `ReplayBrowser` logging and remembering the
/// path itself (see `open_requested`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayBrowserEvent {
    OpenReplay(PathBuf),
    /// Open this replay beside whatever is already open rather than in place
    /// of it.
    OpenReplayInNewTab(PathBuf),
    /// Play this replay's battle back on its minimap.
    RenderReplay(PathBuf),
    /// The game has just written a replay into the watched directory, and the
    /// listing now holds it.
    ReplayAppeared(PathBuf),
    /// A replay the listing already held has been written to again, and the
    /// listing now holds the new read. A tab open on it is reading the old one.
    ReplayChanged(PathBuf),
    /// The replays directory cannot be watched, so a finished match will not
    /// appear on its own. Said out loud, because the reader would otherwise wait
    /// for a listing that never grows.
    WatchFailed(WatchFailure),
    /// Record these battles in the Stats tab's session. `replace` forgets
    /// what is already recorded first, which is what "Set as" means.
    SessionStats {
        paths: Vec<PathBuf>,
        replace: bool,
    },
}

pub struct ReplayBrowser {
    files: Vec<RawReplay>,
    /// How far the running scan has got. `None` when none is running.
    scan_progress: Option<ScanProgress>,
    /// The replays the reader has ctrl-clicked, which every batch action
    /// works over. Empty means the one highlighted row, if any.
    marked: HashSet<PathBuf>,
    /// The install the listing is reading, empty until one is known. Only
    /// "Open in Game" needs it: the scan takes its own copy.
    wows_dir: String,
    grouping: ReplayGrouping,
    tree_state: Entity<TreeState>,
    status: ScanStatus,
    /// Rebuilt only by `rebuild_tree` (on scan/grouping/game-data changes),
    /// never per render: `render` clones this `Rc` (a pointer bump) instead
    /// of the map itself, which would otherwise deep-clone every leaf's
    /// owned `PathBuf` on every render of the browser.
    leaf_info: Rc<HashMap<SharedString, LeafInfo>>,
    /// Every replay under each group row, so a group's menu can act on all
    /// of them. Built with `leaf_info` and replaced with it.
    group_children: Rc<HashMap<SharedString, Vec<PathBuf>>>,
    /// The most recently single- or double-clicked leaf's path.
    selected_path: Option<PathBuf>,
    /// The most recently double-clicked leaf's path -- the "open" intent's
    /// minimal stand-in for Milestone 5's dock wiring (see the module doc).
    open_requested: Option<PathBuf>,
    /// The shared preloaded game data (see `set_game_data`). Rows are named
    /// against it, so the panel waits for it rather than listing raw ids.
    game_data: GameData,
    /// What the replay index knows about each listed file: outcome, damage,
    /// kills and division. Empty until the index answers (see
    /// `load_summaries`), which is what leaves a row reading "not indexed".
    summaries: HashMap<PathBuf, RowSummary>,
    /// The locale the stats line's figures are grouped in.
    locale: Option<String>,
    /// The build cache a hovered row's preview is baked against. Separate
    /// from `game_data`: that one is the currently installed build's metadata
    /// for naming rows, this one loads whichever build a replay was recorded
    /// on.
    build_cache: Option<GameDataCache>,
    /// Hover-to-preview: the same behaviour the Search tab has, over the
    /// listing's own rows (`preview_hover`).
    preview: PreviewHover,
    /// Where the pointer was when it entered the previewed row, so the popup
    /// is anchored beside it like the egui app's own hover popup.
    preview_anchor: Point<Pixels>,
    /// The watch on the replays directory. Held because dropping it stops the
    /// watch; replaced whenever the directory changes.
    watcher: Option<notify::RecommendedWatcher>,
    /// Bumped per scan, so a slower earlier scan (and the watch task it
    /// started) cannot write over a later one's listing.
    scan_generation: u64,
}

/// What the browser can translate its labels with.
///
/// `Loading` and `Unavailable` both mean "no provider", but they are not the
/// same to the reader: the first resolves on its own, so the panel waits
/// rather than showing a list of raw ship and map ids, while the second never
/// will, so the raw ids are all there is to show.
enum GameData {
    Loading,
    Unavailable,
    Ready(Arc<GameMetadataProvider>),
}

impl GameData {
    fn provider(&self) -> Option<&GameMetadataProvider> {
        match self {
            GameData::Ready(provider) => Some(provider),
            GameData::Loading | GameData::Unavailable => None,
        }
    }
}

impl EventEmitter<ReplayBrowserEvent> for ReplayBrowser {}

impl ReplayBrowser {
    /// Seeds a bake in flight with nothing to show yet. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_baking_preview_for_test(&mut self, path: std::path::PathBuf) {
        self.preview.seed_baking_for_test(path);
    }

    /// Seeds the listing with a replay at `path`, as a scan would. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_listing_for_test(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.files.push(RawReplay {
            path,
            listed: wows_toolkit_viewmodel::listing_row::ListedReplay {
                ship_id: None,
                map_name: "spaces/00_CO_ocean".to_owned(),
                game_type: String::new(),
                scenario: String::new(),
                date_time: "01.01.2026 00:00:00".to_owned(),
                build: None,
            },
        });
        self.status = ScanStatus::Loaded;
        self.rebuild_tree(cx);
    }

    /// What the listing holds, by path. Test-only.
    #[cfg(test)]
    pub(crate) fn listed_paths_for_test(&self) -> Vec<std::path::PathBuf> {
        self.files.iter().map(|file| file.path.clone()).collect()
    }

    /// Seeds a row being dwelled on, before any bake. Test-only.
    #[cfg(test)]
    pub(crate) fn seed_dwelling_preview_for_test(&mut self, path: std::path::PathBuf) {
        self.preview.seed_dwelling_for_test(path);
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        Self {
            files: Vec::new(),
            scan_progress: None,
            marked: HashSet::new(),
            wows_dir: String::new(),
            grouping: ReplayGrouping::default(),
            tree_state,
            status: ScanStatus::Loading,
            leaf_info: Rc::new(HashMap::new()),
            group_children: Rc::new(HashMap::new()),
            selected_path: None,
            open_requested: None,
            game_data: GameData::Loading,
            summaries: HashMap::new(),
            locale: None,
            build_cache: None,
            preview: PreviewHover::default(),
            preview_anchor: Point::default(),
            watcher: None,
            scan_generation: 0,
        }
    }

    /// The leaf most recently single- or double-clicked, if any.
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected_path.as_deref()
    }

    /// The path most recently opened via double-click, if any.
    pub fn open_requested(&self) -> Option<&Path> {
        self.open_requested.as_deref()
    }

    /// The currently active grouping strategy. The browser is the single
    /// source of truth for it; `view.rs`'s header combo mirrors this.
    pub fn grouping(&self) -> ReplayGrouping {
        self.grouping
    }

    /// Adopts `status`'s game data (or drops it, if `status` is no longer
    /// `Ready`) and rebuilds the tree so labels reflect it -- called whenever
    /// `load::GameDataStatus` changes, most importantly on its `Loading` ->
    /// `Ready` transition, so a browser built before game data preloaded gets
    /// its raw-fallback labels replaced with real ship/map names once it
    /// does. A no-op when the resolved provider is unchanged (comparing by
    /// `Arc` identity), so polling the same settled status repeatedly does
    /// not re-rebuild the tree for nothing.
    pub fn set_game_data(&mut self, status: &GameDataStatus, cx: &mut Context<Self>) {
        let new_state = match status {
            GameDataStatus::Ready(loaded) => GameData::Ready(Arc::clone(loaded.provider())),
            GameDataStatus::Loading => GameData::Loading,
            GameDataStatus::Failed(_) => GameData::Unavailable,
        };
        let unchanged = match (&self.game_data, &new_state) {
            (GameData::Ready(current), GameData::Ready(new)) => Arc::ptr_eq(current, new),
            (GameData::Loading, GameData::Loading) | (GameData::Unavailable, GameData::Unavailable) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        self.game_data = new_state;
        self.rebuild_tree(cx);
        cx.notify();
    }

    /// Adopts the build cache a hovered row's preview is baked against. The
    /// inspector owns it; the browser only reads it.
    pub fn set_build_cache(&mut self, cache: Option<GameDataCache>) {
        self.build_cache = cache;
    }

    /// The locale the rows' figures are grouped in.
    pub fn set_locale(&mut self, locale: Option<String>, cx: &mut Context<Self>) {
        if self.locale == locale {
            return;
        }
        self.locale = locale;
        self.rebuild_tree(cx);
        cx.notify();
    }

    /// Reads what the replay index knows about the listed files.
    ///
    /// One query over the live source rather than one per row: a listing can
    /// hold thousands of replays. A row with no summary reads as not indexed,
    /// which is what the egui listing shows for the same file.
    pub fn load_summaries(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        cx.spawn(async move |this, cx| {
            let loaded = crate::runtime::spawn(cx, async move {
                let Some(source) = query::live_source_id(&pool).await? else {
                    return Ok(HashMap::new());
                };
                query::row_summaries_for_source(&pool, source).await
            })
            .await;

            let summaries = match loaded {
                Ok(Ok(summaries)) => summaries,
                Ok(Err(err)) => {
                    tracing::warn!("replay browser: the index summaries could not be read: {err}");
                    return;
                }
                Err(err) => {
                    tracing::warn!("replay browser: the index summaries did not load: {err}");
                    return;
                }
            };

            let _ = this.update(cx, |this, cx| {
                this.summaries = summaries;
                this.rebuild_tree(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The pointer settled on a row: after the shared dwell, its battle plays
    /// back beside the listing.
    fn hover_leaf(&mut self, path: PathBuf, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.preview_anchor = position;
        let cache = self.build_cache.clone();
        let map_name = self.leaf_info.values().find(|leaf| leaf.path == path).map(|leaf| leaf.map_name.clone());
        self.preview.enter(path, map_name, cache, cx, |browser| &mut browser.preview);
    }

    /// Kicks off the background directory scan for `wows_dir`. Safe to call
    /// again later (e.g. if the user changes the WoWs directory); replaces
    /// whatever the previous scan found.
    pub fn start_scan(&mut self, wows_dir: String, cx: &mut Context<Self>) {
        // Kept because "Open in Game" launches the executable beside it.
        self.wows_dir = wows_dir.clone();
        // Every scan takes a number, and only the newest one's result is
        // applied: two scans in flight otherwise land in whichever order they
        // finish, leaving the listing (and the watch) on the older directory.
        self.scan_generation = self.scan_generation.wrapping_add(1);
        let generation = self.scan_generation;

        if wows_dir.is_empty() {
            // The directory the watch was on is no longer the one to watch,
            // and there is no new one; dropping it is what stops a match
            // finishing there from refilling a listing that says there is no
            // directory set.
            self.watcher = None;
            self.status = ScanStatus::Failed(ScanError::WowsDirMissing);
            cx.notify();
            return;
        }

        self.status = ScanStatus::Loading;
        self.scan_progress = Some(ScanProgress::default());
        cx.notify();

        // The scan reports itself as it goes rather than being polled: a
        // ticker would keep firing with nothing to say, and in the test
        // executor it would never stop.
        let (reports, mut progress_rx) = futures::channel::mpsc::unbounded();

        cx.spawn(async move |this, cx| {
            let replays_dir = resolve_replays_dir(Path::new(&wows_dir));
            let scanned = replays_dir.clone();
            let scan = cx.background_spawn(async move { scan_replay_files(&scanned, &reports) });

            let listen = {
                let this = this.clone();
                let mut cx = cx.clone();
                async move {
                    while let Some(progress) = progress_rx.next().await {
                        let kept = this.update(&mut cx, |this, cx| {
                            if this.scan_generation != generation {
                                return false;
                            }
                            this.scan_progress = Some(progress);
                            cx.notify();
                            true
                        });
                        if !matches!(kept, Ok(true)) {
                            break;
                        }
                    }
                }
            };

            let (files, ()) = futures::future::join(scan, listen).await;
            let _ = this.update(cx, |this, cx| {
                if this.scan_generation != generation {
                    return;
                }
                this.scan_progress = None;
                this.status = if files.is_empty() { ScanStatus::Empty } else { ScanStatus::Loaded };
                this.files = files;
                this.rebuild_tree(cx);
                this.watch_replays_dir(replays_dir, generation, cx);
                this.warm_listed_build(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Loads the build most of the listed replays were recorded on, ahead of
    /// anything asking for it.
    ///
    /// A preview cannot be baked without the build its replay was recorded
    /// on, and that is over a second of work. The startup preload warms the
    /// *installed* build, which is the right one only until the game
    /// updates; after that the first hover pays for the older build every
    /// session. Warming what the listing actually holds moves that cost off
    /// the first hover.
    /// Whether the scan found no replay files at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn warm_listed_build(&mut self, cx: &mut Context<Self>) {
        let Some(cache) = self.build_cache.clone() else { return };
        let Some(build) = most_common_build(&self.files) else { return };
        if cache.loaded_build(build).is_some() {
            return;
        }

        cx.spawn(async move |_this, cx| {
            let warmed = cx.background_spawn(async move { cache.get_or_load_build(build) }).await;
            match warmed {
                Ok(_) => tracing::debug!("replay browser: warmed build {build} for previews"),
                Err(err) => tracing::debug!("replay browser: build {build} did not warm: {err}"),
            }
        })
        .detach();
    }

    /// Watches the replays directory so a match the game has just written
    /// joins the listing without a rescan, which is what makes "Autoload
    /// Latest Replay" mean anything (the egui app's own watcher,
    /// `tab_state.rs`).
    fn watch_replays_dir(&mut self, replays_dir: PathBuf, generation: u64, cx: &mut Context<Self>) {
        use notify::Watcher as _;

        // Dropped before the new one is installed, so the old directory stops
        // reporting rather than both being watched.
        self.watcher = None;

        let (tx, mut rx) = futures::channel::mpsc::unbounded::<DirChange>();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            let make = match event.kind {
                notify::EventKind::Create(_)
                | notify::EventKind::Modify(notify::event::ModifyKind::Name(notify::event::RenameMode::To)) => {
                    DirChange::Appeared
                }
                // A replay written to again: a re-save, or a copy that landed
                // before the writer had finished with it.
                notify::EventKind::Modify(notify::event::ModifyKind::Data(_)) => DirChange::Changed,
                notify::EventKind::Remove(_) => DirChange::Removed,
                _ => return,
            };
            for path in event.paths {
                if is_finished_replay(&path) {
                    // The receiver is gone once the directory changes or the
                    // app closes; this runs on notify's own thread, so a send
                    // that fails is dropped rather than unwinding it.
                    let _ = tx.unbounded_send(make(path));
                }
            }
        });
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(err) => {
                tracing::warn!(error = ?err, "replay browser: no filesystem watcher available");
                cx.emit(ReplayBrowserEvent::WatchFailed(WatchFailure::NoWatcher(err.to_string())));
                return;
            }
        };
        if let Err(err) = watcher.watch(&replays_dir, notify::RecursiveMode::NonRecursive) {
            tracing::warn!(dir = %replays_dir.display(), error = ?err, "replay browser: the replays directory is not watchable");
            cx.emit(ReplayBrowserEvent::WatchFailed(WatchFailure::NotWatchable(err.to_string())));
            return;
        }
        self.watcher = Some(watcher);

        cx.spawn(async move |this, cx| {
            while let Some(change) = futures::StreamExt::next(&mut rx).await {
                // A removal is the one change with nothing to read: the file is
                // gone, which is what the listing is being told.
                if let DirChange::Removed(path) = &change {
                    let path = path.clone();
                    let updated = this.update(cx, |this, cx| {
                        if this.scan_generation != generation {
                            return;
                        }
                        this.forget(&path, cx);
                    });
                    if updated.is_err() {
                        break;
                    }
                    continue;
                }

                let Some(raw) = read_replay_when_complete(change.path().to_path_buf(), cx).await else { continue };
                let touched = raw.path.clone();
                let updated = this.update(cx, |this, cx| {
                    // A scan started while this read was waiting means the
                    // listing is no longer the one this replay belongs to.
                    if this.scan_generation != generation {
                        return false;
                    }
                    let held = this.files.iter().position(|existing| existing.path == raw.path);
                    match held {
                        // Already listed: the read replaces the row, so a
                        // re-saved replay reads as what is on disk now.
                        Some(at) => {
                            this.files[at] = raw;
                            this.rebuild_tree(cx);
                            cx.emit(ReplayBrowserEvent::ReplayChanged(touched));
                        }
                        None => {
                            this.files.push(raw);
                            this.status = ScanStatus::Loaded;
                            this.rebuild_tree(cx);
                            cx.emit(ReplayBrowserEvent::ReplayAppeared(touched));
                        }
                    }
                    cx.notify();
                    true
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Drops a replay the directory no longer holds, with whatever the listing
    /// remembered about it.
    fn forget(&mut self, path: &Path, cx: &mut Context<Self>) {
        let held = self.files.iter().position(|existing| existing.path == *path);
        let Some(at) = held else { return };
        self.files.remove(at);
        self.marked.remove(path);
        if self.selected_path.as_deref() == Some(path) {
            self.selected_path = None;
        }
        self.rebuild_tree(cx);
        cx.notify();
    }

    /// Switches the grouping strategy and rebuilds the tree. Called only from
    /// `ReplayInspectorView::set_grouping`, which owns the header control --
    /// matching the egui app's header placement -- and reaches in here to
    /// apply it.
    pub(crate) fn set_grouping(&mut self, grouping: ReplayGrouping, cx: &mut Context<Self>) {
        if self.grouping == grouping {
            return;
        }
        self.grouping = grouping;
        self.rebuild_tree(cx);
        cx.notify();
    }

    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.game_data.provider() else {
            // Every label would be a raw id; the panel waits instead (see
            // `render`), so there is nothing worth building yet.
            return;
        };
        let locale = self.locale.as_deref();
        let translated: Vec<ReplayLite> = self
            .files
            .iter()
            .map(|raw| ReplayLite {
                path: raw.path.clone(),
                map_name: raw.listed.map_name.clone(),
                identity: listed_row_identity(&raw.listed, provider),
                stats: resolve_row_stats(None, self.summaries.get(&raw.path)),
            })
            .collect();
        // The hover text is assembled per replay rather than per node: a leaf
        // knows its path, and the tree nodes carry only what they draw.
        let mut hover: HashMap<PathBuf, Rc<Vec<HoverFact>>> =
            translated.iter().map(|r| (r.path.clone(), Rc::new(hover_facts(&r.identity, &r.stats, locale)))).collect();
        let nodes = build_browser_tree(&translated, self.grouping, locale);
        let mut leaf_info = HashMap::new();
        let mut group_children = HashMap::new();
        let mut next_group_id = 0usize;
        let items: Vec<TreeItem> = nodes
            .into_iter()
            .map(|node| node_to_tree_item(node, &mut next_group_id, &mut leaf_info, &mut group_children, &mut hover).0)
            .collect();
        self.leaf_info = Rc::new(leaf_info);
        self.group_children = Rc::new(group_children);

        // A rebuild is a re-translation or a fresh scan of the same replays,
        // not a new listing: a group the user closed stays closed and the row
        // they were on stays selected. Groups are keyed by label, since their
        // ids are positional and a rebuild renumbers them.
        let (closed, selected) = self.tree_state.read_with(cx, |state, _cx| {
            let mut closed: HashSet<SharedString> = HashSet::new();
            let mut selected = None;
            for ix in 0.. {
                let Some(entry) = state.entry(ix) else { break };
                if entry.is_folder() && !entry.is_expanded() {
                    closed.insert(entry.item().label.clone());
                }
                if state.selected_index() == Some(ix) {
                    selected = Some(entry.item().id.clone());
                }
            }
            (closed, selected)
        });
        let items = items.into_iter().map(|item| restore_expansion(item, &closed)).collect::<Vec<_>>();

        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
            if let Some(selected) = selected {
                let ix = (0..)
                    .take_while(|ix| state.entry(*ix).is_some())
                    .find(|ix| state.entry(*ix).map(|entry| entry.item().id.clone()) == Some(selected.clone()));
                state.set_selected_index(ix, cx);
            }
        });
    }

    /// Handles a click on a leaf's rendered item: any click records the
    /// selection, a double-click additionally records/emits the open intent.
    fn handle_leaf_click(&mut self, path: PathBuf, click_count: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        // Ctrl adds to or removes from the set, the way a file manager does;
        // a plain click replaces it, so the common case stays one replay.
        if modifiers.secondary() {
            self.toggle_selected(path, cx);
            return;
        }

        self.marked.clear();
        self.selected_path = Some(path.clone());
        if click_count >= 2 {
            tracing::info!(path = %path.display(), "replay browser: open requested");
            self.open_requested = Some(path.clone());
            cx.emit(ReplayBrowserEvent::OpenReplay(path));
        }
        cx.notify();
    }

    /// Adds `path` to the marked set, or takes it out again.
    fn toggle_selected(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // The row a plain click left highlighted joins the set, so
        // ctrl-clicking a second row marks both rather than only the second.
        if self.marked.is_empty()
            && let Some(current) = self.selected_path.clone()
            && current != path
        {
            self.marked.insert(current);
        }
        if !self.marked.remove(&path) {
            self.marked.insert(path.clone());
            self.selected_path = Some(path);
        }
        cx.notify();
    }

    /// Every replay the reader has marked, in listing order.
    ///
    /// A single highlighted row counts as a selection of one, so an action
    /// offered on the set is offered on it too.
    fn selection(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            return self.selected_path.iter().cloned().collect();
        }
        let mut paths: Vec<PathBuf> = self.marked.iter().cloned().collect();
        paths.sort();
        paths
    }
}

/// The hover's facts as a two-column grid: labels down the left, values down
/// the right, so a reader finds the damage without reading the whole caption.
fn hover_grid(facts: &[HoverFact], label_color: Hsla) -> impl IntoElement {
    v_flex().gap_0().max_w(px(PREVIEW_SIZE)).children(facts.iter().map(move |fact| {
        h_flex()
            .gap_2()
            .items_start()
            .child(div().w(HOVER_LABEL_WIDTH).flex_none().text_xs().text_color(label_color).child(fact.label.clone()))
            .child(div().flex_1().min_w(px(0.)).text_xs().child(fact.value.clone()))
    }))
}

/// Re-closes the groups the user had closed, by label.
fn restore_expansion(item: TreeItem, closed: &HashSet<SharedString>) -> TreeItem {
    if !item.is_folder() {
        return item;
    }
    let expanded = !closed.contains(&item.label);
    let children: Vec<TreeItem> = item.children.iter().cloned().map(|child| restore_expansion(child, closed)).collect();
    TreeItem::new(item.id.clone(), item.label.clone()).children(children).expanded(expanded)
}

/// Converts one `BrowserNode` into a `TreeItem`, assigning group ids from
/// `next_group_id` (group labels are not always unique -- see
/// `browser.rs::build_date_groups`'s out-of-order-run doc -- so an
/// incrementing counter is the only reliable id source) and recording every
/// leaf's path/battle_result into `leaf_info`, keyed by the leaf's id (its
/// full path string, which is unique per file). Groups default to expanded
/// so the browser is immediately useful without an extra click per group.
///
/// Builds one tree row, and reports the replays under it so a group's menu
/// can act on all of them.
fn node_to_tree_item(
    node: BrowserNode,
    next_group_id: &mut usize,
    leaf_info: &mut HashMap<SharedString, LeafInfo>,
    group_children: &mut HashMap<SharedString, Vec<PathBuf>>,
    hover: &mut HashMap<PathBuf, Rc<Vec<HoverFact>>>,
) -> (TreeItem, Vec<PathBuf>) {
    match node {
        BrowserNode::Group { label, children } => {
            let id: SharedString = format!("replay-browser-group-{next_group_id}").into();
            *next_group_id += 1;
            let mut under = Vec::new();
            let children: Vec<TreeItem> = children
                .into_iter()
                .map(|child| {
                    let (item, paths) = node_to_tree_item(child, next_group_id, leaf_info, group_children, hover);
                    under.extend(paths);
                    item
                })
                .collect();
            group_children.insert(id.clone(), under.clone());
            (TreeItem::new(id, label).children(children).expanded(true), under)
        }
        BrowserNode::Leaf { label, stats, path, map_name, outcome, in_division } => {
            let id: SharedString = path.to_string_lossy().into_owned().into();
            let hover = hover.remove(&path).unwrap_or_default();
            let under = vec![path.clone()];
            leaf_info
                .insert(id.clone(), LeafInfo { path, map_name, stats: Rc::new(stats), outcome, in_division, hover });
            (TreeItem::new(id, label), under)
        }
    }
}

/// Maps a leaf's outcome to its label color: Win/Loss/Draw get the
/// win/loss/draw palette (`table.rs::resolve_color`, the same one the player
/// table uses), an outcome the index does not know is left uncolored --
/// matching the egui row, which draws an `Unknown` outcome in the plain text
/// colour (`listing_row::row_layout_job`).
fn leaf_label_color(outcome: MatchOutcome) -> Option<Hsla> {
    let outcome = match outcome {
        MatchOutcome::Win => BattleOutcome::Win,
        MatchOutcome::Loss => BattleOutcome::Loss,
        MatchOutcome::Draw => BattleOutcome::Draw,
        MatchOutcome::Unknown => return None,
    };
    Some(resolve_color(ColorRole::WinLoss(outcome)))
}

#[allow(clippy::too_many_arguments)]
fn render_browser_item(
    browser: Entity<ReplayBrowser>,
    ix: usize,
    entry: &TreeEntry,
    selected: bool,
    leaf_info: &HashMap<SharedString, LeafInfo>,
    marked: &HashSet<PathBuf>,
    row_height: Pixels,
    cx: &App,
) -> ListItem {
    let item = entry.item();
    let is_folder = entry.is_folder();
    let leaf = (!is_folder).then(|| leaf_info.get(&item.id)).flatten();

    // Line one names the battle and is tinted by its outcome; line two, in
    // de-emphasised text, reports it. The same two lines the egui row draws
    // (`listing_row::row_layout_job`), as two elements rather than one
    // layout job.
    let mut identity_el = div().overflow_hidden().text_ellipsis().whitespace_nowrap();
    if let Some(leaf) = leaf {
        identity_el = identity_el.when_some(leaf_label_color(leaf.outcome), |el, color| el.text_color(color));
    }
    let identity_el = h_flex().gap_1().items_center().child(identity_el.child(item.label.clone())).when_some(
        leaf.filter(|leaf| leaf.in_division),
        |row, _| {
            row.child(crate::icons::icon(DIVISION_GLYPH).text_color(resolve_color(ColorRole::Fixed(DIVISION_COLOR))))
        },
    );

    let mut lines = v_flex().flex_1().justify_center().overflow_hidden().child(identity_el);
    if let Some(leaf) = leaf {
        // The glyphs live in the icon font, the figures in the UI font, so
        // the line is drawn from its pieces rather than as one string.
        let stats = h_flex().text_xs().text_color(crate::theme::text_dim()).overflow_hidden().children(
            leaf.stats.iter().map(|part| match part {
                LinePart::Text(text) => div().whitespace_nowrap().child(text.clone()).into_any_element(),
                LinePart::Glyph(glyph) => crate::icons::icon(glyph).into_any_element(),
            }),
        );
        lines = lines.child(stats);
    }

    // The tree is a uniform list: it measures one row and lays every row out
    // at that height. A row taller than the first one would spill over its
    // neighbour and leave the hover and selection highlights -- which are
    // drawn at the measured height -- behind its text, so every row, group
    // header included, declares the same height.
    // The indent is drawn rather than merely left blank: a guide per level
    // is what tells a deep row which group it belongs to, the way the egui
    // listing's tree lines do.
    let mut row =
        h_flex().h(row_height).gap_1().items_center().child(crate::ui::indent_guides(entry.depth(), INDENT, cx));
    if is_folder {
        let chevron = if entry.is_expanded() { IconName::ChevronDown } else { IconName::ChevronRight };
        row = row.child(Icon::new(chevron));
    } else {
        row = row.child(div().w(INDENT));
    }
    row = row.child(lines);

    // Groups keep the panel's own background so they read as headers; the
    // replays under them alternate, which is what makes a long date group
    // scannable (`ui::stripe`).
    let striped = (!is_folder).then(|| crate::ui::stripe(ix, cx)).flatten();
    let selected = selected || leaf.is_some_and(|leaf| marked.contains(&leaf.path));
    let mut list_item =
        ListItem::new(ix).selected(selected).when_some(striped, |item, color| item.bg(color)).child(row);

    if let Some(leaf) = leaf {
        // No tooltip here: the hover popup carries the same words under the
        // map, and the egui row shows one or the other, never both
        // (`replay_parser/mod.rs`'s `on_hover_ui` / `on_hover_text` arms).
        let path = leaf.path.clone();
        let hover_browser = browser.clone();
        let hover_path = path.clone();
        list_item = list_item.on_mouse_enter(move |event: &MouseMoveEvent, _window, cx: &mut App| {
            let path = hover_path.clone();
            let position = event.position;
            hover_browser.update(cx, |browser, cx| browser.hover_leaf(path, position, cx));
        });
        list_item = list_item.on_click(move |event: &ClickEvent, window, cx: &mut App| {
            let modifiers = window.modifiers();
            browser
                .update(cx, |browser, cx| browser.handle_leaf_click(path.clone(), event.click_count(), modifiers, cx));
        });
    }

    crate::ui::tree_row(list_item, cx)
}

impl Render for ReplayBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.preview.release_dropped(window);
        let border = cx.theme().border;
        let entity = cx.entity();

        let header = h_flex().flex_none().gap_1().items_center().px_2().py_1().border_b_1().border_color(border).child(
            div().flex_1().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.replay.listing_caption").to_string()),
        );

        let body = match &self.status {
            // A spinner beside the line, as the egui listing shows while it
            // is reading the directory.
            ScanStatus::Loading => {
                let ScanProgress { done, total } = self.scan_progress.unwrap_or_default();
                // Zero files counted so far means the directory walk itself
                // is still running, and there is no denominator to report.
                let caption = if total == 0 {
                    t!("ui.replay.scanning").into_owned()
                } else {
                    t!("ui.replay.scanning_count", done = done, total = total).into_owned()
                };
                v_flex()
                    .p_2()
                    .gap_1()
                    .text_sm()
                    .text_color(crate::theme::text_dim())
                    .child(h_flex().gap_2().items_center().child(Spinner::new()).child(caption.clone()))
                    .when(total > 0, |this| {
                        this.child(
                            Progress::new("replay-scan-progress")
                                .accessibility_label(caption)
                                .value(done as f32 / total as f32 * 100.),
                        )
                    })
                    .into_any_element()
            }
            ScanStatus::Failed(reason) => {
                div().p_2().text_sm().text_color(crate::theme::text_dim()).child(reason.to_string()).into_any_element()
            }
            ScanStatus::Empty => div()
                .p_2()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.replay.no_replays").to_string())
                .into_any_element(),
            // Every row's ship and map name comes from the game data, so a
            // list built before it loads is a list of raw ids. The egui app
            // never shows that state: it builds its listing as part of the
            // same load (`task/replays.rs::load_wows_files`).
            ScanStatus::Loaded if matches!(self.game_data, GameData::Loading) => div()
                .p_2()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .child(t!("ui.replay.loading_game_data").to_string())
                .into_any_element(),
            ScanStatus::Loaded => {
                let entity = entity.clone();
                let leaf_info = self.leaf_info.clone();
                let context_menu_leaf_info = self.leaf_info.clone();
                let context_menu_children = self.group_children.clone();
                let context_menu_entity = entity.clone();
                let context_menu_wows_dir = self.wows_dir.clone();
                let context_menu_build_cache = self.build_cache.clone();
                // Two lines of text plus the space around them, against the
                // font the theme is currently drawing at.
                let row_height = cx.theme().font_size * ROW_LINE_HEIGHTS;
                let marked = self.marked.clone();
                let listing = tree(&self.tree_state, move |ix, entry, selected, _window, cx| {
                    render_browser_item(entity.clone(), ix, entry, selected, &leaf_info, &marked, row_height, cx)
                })
                .context_menu(move |_ix, entry, menu, _window, cx| {
                    let wows_dir = context_menu_wows_dir.clone();
                    let build_cache = context_menu_build_cache.clone();
                    let Some(leaf) = context_menu_leaf_info.get(&entry.item().id) else {
                        // A group: its own menu copies the paths of every
                        // replay under it, which is what a batch action on a
                        // date or a ship starts from.
                        let paths: Vec<PathBuf> =
                            context_menu_children.get(&entry.item().id).map(|paths| paths.to_vec()).unwrap_or_default();
                        if paths.is_empty() {
                            return menu;
                        }
                        let copy_paths_label = t!("ui.replay.context.copy_paths", count = paths.len()).into_owned();
                        let copy_files_label = t!("ui.replay.context.copy_replays", count = paths.len()).into_owned();
                        let path_list = paths.clone();
                        return menu
                            .item(PopupMenuItem::new(copy_files_label).on_click(move |_event, window, cx| {
                                copy_replay_files(&paths, window, cx);
                            }))
                            .item(PopupMenuItem::new(copy_paths_label).on_click(move |_event, _window, cx| {
                                copy_paths(&path_list, cx);
                            }));
                    };

                    // Right-clicking a row that is not in the set acts on
                    // that row, not on a selection the reader has moved on
                    // from; right-clicking one that is acts on all of them.
                    let selection = context_menu_entity.read(cx).selection();
                    let batch: Vec<PathBuf> =
                        if selection.len() > 1 && selection.contains(&leaf.path) { selection } else { Vec::new() };

                    let open_path = leaf.path.clone();
                    let new_tab_path = leaf.path.clone();
                    let new_tab_entity = context_menu_entity.clone();
                    let render_path = leaf.path.clone();
                    let render_entity = context_menu_entity.clone();
                    let session_paths = if batch.is_empty() { vec![leaf.path.clone()] } else { batch.clone() };
                    let set_paths = session_paths.clone();
                    let set_entity = context_menu_entity.clone();
                    let add_paths = session_paths.clone();
                    let add_entity = context_menu_entity.clone();
                    let session_count = session_paths.len();
                    let batch_files = batch.clone();
                    let batch_paths = batch.clone();
                    let batch_render = batch.clone();
                    let batch_entity = context_menu_entity.clone();
                    let copy_path = leaf.path.clone();
                    let copy_file = leaf.path.clone();
                    let in_game_path = leaf.path.clone();
                    let reveal_path = leaf.path.clone();
                    let open_entity = context_menu_entity.clone();
                    menu.when(!batch.is_empty(), move |menu| {
                        // The whole marked set, named by its size so the
                        // reader can see what they are about to act on.
                        let count = batch_files.len();
                        menu.item(
                            PopupMenuItem::new(t!("ui.replay.context.copy_replays", count = count).into_owned())
                                .on_click(move |_event, window, cx| {
                                    copy_replay_files(&batch_files, window, cx);
                                }),
                        )
                        .item(
                            PopupMenuItem::new(t!("ui.replay.context.copy_paths", count = count).into_owned())
                                .on_click(move |_event, _window, cx| copy_paths(&batch_paths, cx)),
                        )
                        .item(
                            PopupMenuItem::new(
                                t!("ui.replay.context.render_to_video_many", count = count).into_owned(),
                            )
                            .on_click(move |_event, _window, cx| {
                                let paths = batch_render.clone();
                                batch_entity.update(cx, |_browser, cx| {
                                    for path in paths {
                                        cx.emit(ReplayBrowserEvent::RenderReplay(path));
                                    }
                                });
                            }),
                        )
                        .separator()
                    })
                    .item(PopupMenuItem::new(t!("ui.collab.open").into_owned()).on_click(move |_event, _window, cx| {
                        let path = open_path.clone();
                        open_entity.update(cx, |_browser, cx| cx.emit(ReplayBrowserEvent::OpenReplay(path)));
                    }))
                    .item(PopupMenuItem::new(t!("ui.replay.context.open_in_new_tab").into_owned()).on_click(
                        move |_event, _window, cx| {
                            let path = new_tab_path.clone();
                            new_tab_entity
                                .update(cx, |_browser, cx| cx.emit(ReplayBrowserEvent::OpenReplayInNewTab(path)));
                        },
                    ))
                    .item(PopupMenuItem::new(t!("ui.replay.context.render_replay").into_owned()).on_click(
                        move |_event, _window, cx| {
                            let path = render_path.clone();
                            render_entity.update(cx, |_browser, cx| cx.emit(ReplayBrowserEvent::RenderReplay(path)));
                        },
                    ))
                    .separator()
                    .item(
                        PopupMenuItem::new(
                            if session_count == 1 {
                                t!("ui.replay.context.set_session_stats_one")
                            } else {
                                t!("ui.replay.context.set_session_stats_many", count = session_count)
                            }
                            .into_owned(),
                        )
                        .on_click(move |_event, _window, cx| {
                            let paths = set_paths.clone();
                            set_entity.update(cx, |_browser, cx| {
                                cx.emit(ReplayBrowserEvent::SessionStats { paths, replace: true });
                            });
                        }),
                    )
                    .item(
                        PopupMenuItem::new(
                            if session_count == 1 {
                                t!("ui.replay.context.add_session_stats_one")
                            } else {
                                t!("ui.replay.context.add_session_stats_many", count = session_count)
                            }
                            .into_owned(),
                        )
                        .on_click(move |_event, _window, cx| {
                            let paths = add_paths.clone();
                            add_entity.update(cx, |_browser, cx| {
                                cx.emit(ReplayBrowserEvent::SessionStats { paths, replace: false });
                            });
                        }),
                    )
                    .separator()
                    .item(PopupMenuItem::new(t!("ui.replay.context.copy_replay").into_owned()).on_click(
                        move |_event, window, cx| {
                            copy_replay_files(std::slice::from_ref(&copy_file), window, cx);
                        },
                    ))
                    .item(PopupMenuItem::new(t!("ui.replay.context.copy_path").into_owned()).on_click(
                        move |_event, _window, cx| {
                            copy_paths(std::slice::from_ref(&copy_path), cx);
                        },
                    ))
                    .item(PopupMenuItem::new(t!("ui.replay.context.show_in_explorer").into_owned()).on_click(
                        move |_event, _window, _cx| {
                            reveal_in_file_manager(&reveal_path);
                        },
                    ))
                    // The game's own keyboard reference, read from whichever
                    // build is loaded.
                    .when_some(build_cache, |menu, cache| {
                        menu.item(
                            PopupMenuItem::new(t!("ui.replay.context.show_replay_controls").into_owned()).on_click(
                                move |_event, window, cx| {
                                    let Some(build) = cache.newest_loaded() else { return };
                                    match super::controls::read_scheme(build.vfs()) {
                                        Some(groups) => super::controls::open(groups, window, cx),
                                        None => crate::toast::warn(
                                            t!("ui.replay.controls.unavailable").into_owned(),
                                            window,
                                            cx,
                                        ),
                                    }
                                },
                            ),
                        )
                    })
                    // Launching the game is not something to do by a
                    // mis-click, so it is confirmed first, as in the egui
                    // app (`ConfirmableAction::OpenInGame`).
                    .when(!wows_dir.is_empty(), move |menu| {
                        menu.item(PopupMenuItem::new(t!("ui.replay.context.open_in_game").into_owned()).on_click(
                            move |_event, window, cx| {
                                let wows_dir = wows_dir.clone();
                                let replay = in_game_path.clone();
                                window.open_alert_dialog(cx, move |alert, _window, _cx| {
                                    let wows_dir = wows_dir.clone();
                                    let replay = replay.clone();
                                    alert
                                        .title(t!("ui.replay.context.open_in_game").into_owned())
                                        .description(t!("confirm.open_in_game").into_owned())
                                        .show_cancel(true)
                                        .on_ok(move |_event, window, cx| {
                                            open_replay_in_game(&wows_dir, &replay, window, cx);
                                            true
                                        })
                                });
                            },
                        ))
                    })
                });

                // Enter on a highlighted replay opens it, which is what the
                // egui listing does. The tree's own Confirm only expands a
                // folder, so a leaf would otherwise answer to nothing.
                v_flex()
                    .id("replay-listing-keys")
                    .size_full()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                        if event.is_held || event.keystroke.key != "enter" {
                            return;
                        }
                        let Some(id) = this.tree_state.read(cx).selected_item().map(|item| item.id.clone()) else {
                            return;
                        };
                        let Some(leaf) = this.leaf_info.get(&id) else { return };
                        cx.emit(ReplayBrowserEvent::OpenReplay(leaf.path.clone()));
                    }))
                    .child(listing.flex_1())
                    .into_any_element()
            }
        };

        // The hovered row's battle, played back beside the listing. Anchored
        // at the pointer like the egui app's own hover popup
        // (`ui/replay_parser/preview_popup.rs`), and deferred so it paints
        // over the panel rather than inside its scroll area.
        // A bake reads the replay and loads the build it was recorded on,
        // which takes seconds the first time. The map goes up first, from a
        // build already open, with the spinner over it until the battle is
        // ready to play -- the same two stages the egui popup shows
        // (`preview_popup.rs`).
        // Shown only while the bake has nothing to play, or has stopped
        // feeding one: a spinner held over a preview that is already
        // running says nothing, and the egui popup drops it the moment the
        // first frame lands.
        let stalled = self.preview.is_stalled();
        let preview_map: Option<AnyElement> = match (self.preview.frame(), self.preview.awaits_preview()) {
            (Some(frame), _) => Some(
                div()
                    .relative()
                    .w(px(PREVIEW_SIZE))
                    .h(px(PREVIEW_SIZE))
                    .child(img(frame).w(px(PREVIEW_SIZE)).h(px(PREVIEW_SIZE)))
                    .when(stalled, |this| {
                        this.child(
                            h_flex().absolute().inset_0().items_center().justify_center().child(Spinner::new().large()),
                        )
                    })
                    .into_any_element(),
            ),
            // Open water for as long as a preview is coming: the map then
            // arrives in place, rather than the popup growing around it. The
            // spinner waits for a stall, so the dwell itself draws none.
            (None, true) => Some(
                h_flex()
                    .id("replay-preview-placeholder")
                    .test_support()
                    .w(px(PREVIEW_SIZE))
                    .h(px(PREVIEW_SIZE))
                    .bg(crate::preview_hover::MAP_PLACEHOLDER)
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().large())
                    .into_any_element(),
            ),
            (None, false) => None,
        };
        // Under the map: the detail the two drawn lines drop, one labelled
        // fact per line.
        let hover_facts = self
            .preview
            .watched_path()
            .and_then(|path| self.leaf_info.values().find(|leaf| leaf.path == path))
            .map(|leaf| leaf.hover.clone());
        // The popup is what a hovered row says, so it goes up as soon as the
        // row is known -- with the map once there is one.
        let preview_popup = (preview_map.is_some() || hover_facts.is_some()).then(|| {
            let theme = cx.theme();
            let anchor = point(self.preview_anchor.x + PREVIEW_CURSOR_OFFSET, self.preview_anchor.y);
            deferred(
                anchored().position(anchor).snap_to_window_with_margin(px(8.)).child(
                    v_flex()
                        .gap_1()
                        .p_1()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .children(preview_map)
                        .when_some(hover_facts, |this, facts| this.child(hover_grid(&facts, crate::theme::text_dim()))),
                ),
            )
            .with_priority(1)
        });

        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("replay-browser-rows")
                    .test_support()
                    .flex_1()
                    .min_h(px(0.))
                    // The rows themselves start a preview; leaving them all
                    // ends it, which is what the pointer crossing out of the
                    // listing means.
                    .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                        if !*hovered {
                            this.preview.leave(cx);
                        }
                    }))
                    .child(body),
            )
            .when_some(preview_popup, |this, popup| this.child(popup))
    }
}

/// Resolves the replays directory for `wows_dir`: `{wows_dir}/replays`,
/// overridden by a build-specific subdirectory when `preferences.xml` names
/// one and it exists on disk. Ports the relevant slice of
/// `task/replays.rs::load_wows_files` (`mod.rs:342-375`); the
/// `available_builds` gate and the (functionally redundant -- both loop
/// entries were the identical path) double directory-existence check in the
/// original are both dropped as out of scope for a directory-only lookup.
fn resolve_replays_dir(wows_dir: &Path) -> PathBuf {
    let default_dir = wows_dir.join("replays");

    let Some(version_str) =
        std::fs::read_to_string(wows_dir.join("preferences.xml")).ok().and_then(|data| last_server_version(&data))
    else {
        return default_dir;
    };
    let Some(version) = Version::try_from_client_exe(&version_str) else {
        return default_dir;
    };

    let versioned_dir = default_dir.join(format!("{}.{}.{}.0", version.major, version.minor, version.patch));
    if versioned_dir.exists() { versioned_dir } else { default_dir }
}

/// Reads the `<last_server_version>...</last_server_version>` node out of a
/// WoWs `preferences.xml`'s raw contents. Ports
/// `task/replays.rs::current_build_from_preferences`.
fn last_server_version(data: &str) -> Option<String> {
    const OPEN: &str = "<last_server_version>";
    const CLOSE: &str = "</last_server_version>";
    let start = data.find(OPEN)?;
    let end_of_node = data[start..].find(CLOSE)?;
    let version_str = &data[start + OPEN.len()..start + end_of_node];
    Some(version_str.trim().to_string())
}

/// Lists every non-temp `*.wowsreplay` file directly inside `replays_dir` and
/// reads each one's header-only metadata. Ports
/// `task/replays.rs::replay_filepaths`'s filter (minus the creation-time
/// sort, which `build_browser_tree` redoes by path anyway) plus a per-file
/// `ReplayFile::meta_from_file` read. A file that fails to parse (corrupt,
/// mid-write, or from a format this parser does not understand) is logged
/// and skipped rather than aborting the whole scan.
fn scan_replay_files(
    replays_dir: &Path,
    reports: &futures::channel::mpsc::UnboundedSender<ScanProgress>,
) -> Vec<RawReplay> {
    let Ok(entries) = std::fs::read_dir(replays_dir) else {
        return Vec::new();
    };

    // The walk is separated from the meta reads so the listing can say how
    // many files there are to read before it starts reading them.
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("wowsreplay") {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "temp.wowsreplay") {
            continue;
        }
        candidates.push(path);
    }
    let total = candidates.len();
    let _ = reports.unbounded_send(ScanProgress { done: 0, total });

    let mut out = Vec::with_capacity(total);
    for (read, path) in candidates.into_iter().enumerate() {
        match ReplayFile::meta_from_file(&path) {
            Ok(meta) => out.push(RawReplay { listed: ListedReplay::from_meta(&meta), path }),
            Err(err) => tracing::warn!(path = %path.display(), error = ?err, "failed to read replay meta"),
        }
        let done = read + 1;
        if done % SCAN_REPORT_EVERY == 0 || done == total {
            let _ = reports.unbounded_send(ScanProgress { done, total });
        }
    }
    out
}

/// The build the most listed replays were recorded on.
///
/// The most common one rather than the newest: a directory holding one
/// replay from a build nobody plays any more should not have the session
/// spend a second loading it.
fn most_common_build(files: &[RawReplay]) -> Option<u32> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for file in files {
        if let Some(build) = file.listed.build {
            *counts.entry(build).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(build, count)| (*count, *build)).map(|(build, _)| build)
}

/// Whether `path` names a replay the listing shows. `temp.wowsreplay` is the
/// match in progress: it has no container or metadata until the battle ends,
/// and the game renames it into place then.
/// Why the replays directory is not being watched.
///
/// The two are separate messages in the catalogue, and separate causes: no
/// watcher at all is a platform or resource failure, while an unwatchable
/// directory is usually a network share or one that has since gone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchFailure {
    NoWatcher(String),
    NotWatchable(String),
}

impl WatchFailure {
    /// The message the reader is shown, in the words the egui app uses.
    pub fn said(&self) -> String {
        match self {
            Self::NoWatcher(error) => t!("error.file_watcher_creation", error = error).into_owned(),
            Self::NotWatchable(error) => t!("error.replay_dir_watch", error = error).into_owned(),
        }
    }
}

/// What the watcher saw happen to one replay.
///
/// Carried rather than three channels, so the reads stay in the order the
/// directory reported them: a replay created and then written to must not be
/// read in the other order.
enum DirChange {
    Appeared(PathBuf),
    Changed(PathBuf),
    Removed(PathBuf),
}

impl DirChange {
    fn path(&self) -> &Path {
        match self {
            Self::Appeared(path) | Self::Changed(path) | Self::Removed(path) => path,
        }
    }
}

fn is_finished_replay(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("wowsreplay")
        && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
}

/// How long to keep retrying a replay the watcher has just reported, and how
/// long to wait between attempts. The file appears before the game has
/// finished writing its header, so the first read usually fails.
const APPEARED_REPLAY_ATTEMPTS: usize = 10;
const APPEARED_REPLAY_RETRY: std::time::Duration = std::time::Duration::from_millis(300);

/// Reads the header of a replay the watcher reported, retrying while the game
/// finishes writing it. `None` once the attempts run out, which is what a file
/// that is not a replay after all looks like.
///
/// The wait is a timer rather than a sleep: parking a thread of the shared
/// background executor for it would serialise every other background job
/// behind one replay, and would stall the single-threaded test executor
/// outright.
async fn read_replay_when_complete(path: PathBuf, cx: &AsyncApp) -> Option<RawReplay> {
    for attempt in 0..APPEARED_REPLAY_ATTEMPTS {
        if attempt > 0 {
            cx.background_executor().timer(APPEARED_REPLAY_RETRY).await;
        }
        let read = path.clone();
        let parsed = cx.background_spawn(async move { ReplayFile::meta_from_file(&read).ok() }).await;
        if let Some(meta) = parsed {
            return Some(RawReplay { listed: ListedReplay::from_meta(&meta), path });
        }
    }
    tracing::warn!(path = %path.display(), "replay browser: a new replay never became readable");
    None
}

#[cfg(test)]
mod tests {

    /// Ctrl-clicking builds a set: the row already highlighted joins it, so
    /// two ctrl-clicks mark two replays rather than only the second. Clicking
    /// again takes one out, and a plain click replaces the whole set.
    #[gpui_kit::test]
    fn ctrl_clicking_builds_a_selection_and_a_plain_click_replaces_it(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::Modifiers;
        use std::path::PathBuf;

        cx.update(gpui_kit::init);
        let browser = cx.update(|cx| cx.new(super::ReplayBrowser::new));

        let a = PathBuf::from("a.wowsreplay");
        let b = PathBuf::from("b.wowsreplay");
        let c = PathBuf::from("c.wowsreplay");
        let plain = Modifiers::default();
        let ctrl = Modifiers::secondary_key();

        cx.update(|cx| {
            browser.update(cx, |browser, cx| {
                browser.handle_leaf_click(a.clone(), 1, plain, cx);
                assert_eq!(browser.selection(), vec![a.clone()], "one click is a selection of one");

                browser.handle_leaf_click(b.clone(), 1, ctrl, cx);
                assert_eq!(browser.selection(), vec![a.clone(), b.clone()], "the highlighted row joins the set");

                browser.handle_leaf_click(a.clone(), 1, ctrl, cx);
                assert_eq!(browser.selection(), vec![b.clone()], "ctrl-clicking a marked row takes it out");

                browser.handle_leaf_click(c.clone(), 1, plain, cx);
                assert_eq!(browser.selection(), vec![c], "a plain click replaces the set");
            });
        });
    }

    /// A replay the directory no longer holds leaves the listing, and its
    /// marking and highlight go with it: the egui app drops the row on the same
    /// event (`tab_state.rs`'s `NotifyFileEvent::Removed` arm).
    #[gpui_kit::test]
    fn a_removed_replay_leaves_the_listing(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::Modifiers;
        use std::path::PathBuf;

        cx.update(gpui_kit::init);
        let browser = cx.update(|cx| cx.new(super::ReplayBrowser::new));

        let gone = PathBuf::from("gone.wowsreplay");
        let kept = PathBuf::from("kept.wowsreplay");

        cx.update(|cx| {
            browser.update(cx, |browser, cx| {
                browser.seed_listing_for_test(gone.clone(), cx);
                browser.seed_listing_for_test(kept.clone(), cx);
                browser.handle_leaf_click(gone.clone(), 1, Modifiers::default(), cx);
                assert_eq!(browser.selection(), vec![gone.clone()], "the row that is about to go is the selection");

                browser.forget(&gone, cx);

                assert_eq!(browser.listed_paths_for_test(), vec![kept], "only the replay still there is listed");
                assert!(browser.selection().is_empty(), "and nothing is selected, since the selection is gone");
            });
        });
    }

    /// A preview that has nothing to show yet holds open water, so the popup
    /// becomes the real map in place instead of growing around one.
    ///
    /// From the dwell, not from the bake: the bake draws its first map within
    /// milliseconds of starting, so the wait a reader sees is the one before it.
    #[gpui_kit::test]
    fn a_preview_with_nothing_to_show_yet_holds_open_water(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        use gpui_kit::px;
        use gpui_kit::size;
        use std::path::PathBuf;

        use gpui_kit::test::TestWindowExt as _;

        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1200.), px(800.)), |_window, cx| super::ReplayBrowser::new(cx));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find("replay-preview-placeholder").is_none(),
                "nothing is being baked, so there is no preview to hold space for"
            );
        })
        .expect("the test window stays open");

        // The pointer has settled on a row and the dwell is counting down.
        // Nothing is being baked yet, and the space is already held.
        window
            .update(cx, |browser, _window, cx| {
                browser.seed_dwelling_preview_for_test(PathBuf::from("a.wowsreplay"));
                cx.notify();
            })
            .expect("the test window stays open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let placeholder = window.find("replay-preview-placeholder");
            assert_eq!(
                placeholder.bounds().size.width,
                px(super::PREVIEW_SIZE),
                "the space is the size the map will be, so nothing moves when it lands"
            );
            assert_eq!(placeholder.bounds().size.height, px(super::PREVIEW_SIZE));
        })
        .expect("the test window stays open");

        // And it stays held once the bake starts.
        window
            .update(cx, |browser, _window, cx| {
                browser.seed_baking_preview_for_test(PathBuf::from("a.wowsreplay"));
                cx.notify();
            })
            .expect("the test window stays open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("replay-preview-placeholder").is_some(), "still held while the bake runs");
        })
        .expect("the test window stays open");
    }

    use super::is_finished_replay;
    use super::last_server_version;
    use super::resolve_replays_dir;
    use std::path::Path;

    #[test]
    fn the_watcher_ignores_everything_but_a_finished_replay() {
        assert!(is_finished_replay(Path::new("replays/20260818_132652_PWSD610-Smaland.wowsreplay")));
        // The match in progress, which has no metadata until it ends.
        assert!(!is_finished_replay(Path::new("replays/temp.wowsreplay")));
        assert!(!is_finished_replay(Path::new("replays/tempArenaInfo.json")));
        assert!(!is_finished_replay(Path::new("replays/20260818_132652_PWSD610-Smaland")));
    }

    #[test]
    fn last_server_version_reads_the_node_contents() {
        let xml = "<preferences><clientOptions><last_server_version>13, 11, 0, 12668706</last_server_version></clientOptions></preferences>";
        assert_eq!(last_server_version(xml).as_deref(), Some("13, 11, 0, 12668706"));
    }

    #[test]
    fn last_server_version_is_none_when_the_node_is_absent() {
        assert_eq!(last_server_version("<preferences></preferences>"), None);
    }

    #[test]
    fn resolve_replays_dir_falls_back_to_the_default_when_no_preferences_file_exists() {
        let dir = std::env::temp_dir().join("wtk-gpui-browser-view-test-no-prefs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(resolve_replays_dir(&dir), dir.join("replays"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_replays_dir_prefers_the_versioned_subdir_when_it_exists() {
        let dir = std::env::temp_dir().join("wtk-gpui-browser-view-test-versioned");
        let _ = std::fs::remove_dir_all(&dir);
        let versioned = dir.join("replays").join("13.11.0.0");
        std::fs::create_dir_all(&versioned).unwrap();
        std::fs::write(dir.join("preferences.xml"), "<last_server_version>13, 11, 0, 12668706</last_server_version>")
            .unwrap();

        assert_eq!(resolve_replays_dir(&dir), versioned);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
