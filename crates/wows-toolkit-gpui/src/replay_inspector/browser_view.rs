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
//! **`ship`/`map` translation.** The scan itself only reads a replay's raw
//! header fields -- the relation-0 vehicle's `shipId` (mirroring
//! `Replay::player_vehicle`, `mod.rs:2576`) and the raw `mapName` key (e.g.
//! `"spaces/00_CO_ocean"`) -- into `RawReplay`, never the preloaded game data
//! (a header read must stay cheap; see `scan_replay_files`). `translate_replay`
//! then resolves each `RawReplay` into a `ReplayLite` against `ReplayBrowser`'s
//! `game_data` (the shared preloaded `GameMetadataProvider`, adopted via
//! `set_game_data` once `load::GameDataStatus` reaches `Ready`), mirroring the
//! egui app's `Replay::vehicle_name`/`map_name` (`mod.rs:2580`/`2592`)
//! exactly: `vehicle_name` via `param_localization_id` + `localized_name_from_id`
//! on the relation-0 vehicle's `shipId`, falling back to "Spectator" (the
//! egui app's `t!("ui.replay.spectator")` value, hardcoded here since this
//! crate has no i18n lookup wired -- see `panel.rs`'s equivalent hardcoded
//! chat-tooltip string) when no vehicle or no translation resolves; `map_name`
//! via `translate_map_name`. Before `game_data` is adopted (`None`), both fall
//! back to the untranslated raw string -- exactly what `translate_map_name`
//! itself falls back to when no translation is found, so this is the same
//! text the egui app would show if translation were simply unavailable, not a
//! new format.
//!
//! **`battle_result` is always `None` from this scan.** The egui app only
//! learns a replay's win/loss/draw outcome after a full packet parse resolves
//! the battle-result packet (`Replay::battle_result` reads `battle_report`/
//! `ui_report`, both parse products); a header-only read cannot see it. Group
//! win-rate labels and leaf colors are therefore blank/plain for every replay
//! until Milestone 5's background parser (`load.rs`) fills a result in.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::tree::TreeItem;
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::tree::tree;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use wows_replays::ReplayFile;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::GameParamId;
use wows_toolkit_config::ReplayGrouping;
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
    ship_id: Option<GameParamId>,
    raw_ship: String,
    raw_map: String,
    game_time: String,
}

/// A leaf's path and (usually absent, see the module doc) battle result,
/// looked up by tree-item id during rendering. Groups need no side table:
/// their label already encodes everything they display.
#[derive(Clone)]
struct LeafInfo {
    path: PathBuf,
    battle_result: Option<BattleResult>,
}

/// How far to the right of the pointer the hover preview sits, so it never
/// covers the row it belongs to.
const PREVIEW_CURSOR_OFFSET: Pixels = px(24.);

/// The preview's edge length. Square, as the minimap is; the egui app's own
/// popup uses 384 (`preview_popup::PREVIEW_SIZE`).
const PREVIEW_SIZE: f32 = 384.;

/// Background scan progress, driving the panel's content below the header.
enum ScanStatus {
    Loading,
    Loaded,
    Empty,
    Failed(ScanError),
}

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
}

pub struct ReplayBrowser {
    files: Vec<RawReplay>,
    grouping: ReplayGrouping,
    tree_state: Entity<TreeState>,
    status: ScanStatus,
    /// Rebuilt only by `rebuild_tree` (on scan/grouping/game-data changes),
    /// never per render: `render` clones this `Rc` (a pointer bump) instead
    /// of the map itself, which would otherwise deep-clone every leaf's
    /// owned `PathBuf` on every render of the browser.
    leaf_info: Rc<HashMap<SharedString, LeafInfo>>,
    /// The most recently single- or double-clicked leaf's path.
    selected_path: Option<PathBuf>,
    /// The most recently double-clicked leaf's path -- the "open" intent's
    /// minimal stand-in for Milestone 5's dock wiring (see the module doc).
    open_requested: Option<PathBuf>,
    /// The shared preloaded game data (see `set_game_data`). Anything but
    /// `Ready` translates every label to its untranslated raw fallback (see
    /// `translate_replay`).
    game_data: GameData,
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
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        Self {
            files: Vec::new(),
            grouping: ReplayGrouping::default(),
            tree_state,
            status: ScanStatus::Loading,
            leaf_info: Rc::new(HashMap::new()),
            selected_path: None,
            open_requested: None,
            game_data: GameData::Loading,
            build_cache: None,
            preview: PreviewHover::default(),
            preview_anchor: Point::default(),
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

    /// The pointer settled on a row: after the shared dwell, its battle plays
    /// back beside the listing.
    fn hover_leaf(&mut self, path: PathBuf, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.preview_anchor = position;
        let cache = self.build_cache.clone();
        self.preview.enter(path, cache, cx, |browser| &mut browser.preview);
    }

    /// Kicks off the background directory scan for `wows_dir`. Safe to call
    /// again later (e.g. if the user changes the WoWs directory); replaces
    /// whatever the previous scan found.
    pub fn start_scan(&mut self, wows_dir: String, cx: &mut Context<Self>) {
        if wows_dir.is_empty() {
            self.status = ScanStatus::Failed(ScanError::WowsDirMissing);
            cx.notify();
            return;
        }

        self.status = ScanStatus::Loading;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let files = cx.background_spawn(async move { scan_replays_dir(&wows_dir) }).await;
            let _ = this.update(cx, |this, cx| {
                this.status = if files.is_empty() { ScanStatus::Empty } else { ScanStatus::Loaded };
                this.files = files;
                this.rebuild_tree(cx);
                cx.notify();
            });
        })
        .detach();
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
        let provider = self.game_data.provider();
        let translated: Vec<ReplayLite> = self.files.iter().map(|raw| translate_replay(raw, provider)).collect();
        let nodes = build_browser_tree(&translated, self.grouping);
        let mut leaf_info = HashMap::new();
        let mut next_group_id = 0usize;
        let items: Vec<TreeItem> =
            nodes.into_iter().map(|node| node_to_tree_item(node, &mut next_group_id, &mut leaf_info)).collect();
        self.leaf_info = Rc::new(leaf_info);
        self.tree_state.update(cx, |state, cx| state.set_items(items, cx));
    }

    /// Handles a click on a leaf's rendered item: any click records the
    /// selection, a double-click additionally records/emits the open intent.
    fn handle_leaf_click(&mut self, path: PathBuf, click_count: usize, cx: &mut Context<Self>) {
        self.selected_path = Some(path.clone());
        if click_count >= 2 {
            tracing::info!(path = %path.display(), "replay browser: open requested");
            self.open_requested = Some(path.clone());
            cx.emit(ReplayBrowserEvent::OpenReplay(path));
        }
        cx.notify();
    }
}

/// Converts one `BrowserNode` into a `TreeItem`, assigning group ids from
/// `next_group_id` (group labels are not always unique -- see
/// `browser.rs::build_date_groups`'s out-of-order-run doc -- so an
/// incrementing counter is the only reliable id source) and recording every
/// leaf's path/battle_result into `leaf_info`, keyed by the leaf's id (its
/// full path string, which is unique per file). Groups default to expanded
/// so the browser is immediately useful without an extra click per group.
fn node_to_tree_item(
    node: BrowserNode,
    next_group_id: &mut usize,
    leaf_info: &mut HashMap<SharedString, LeafInfo>,
) -> TreeItem {
    match node {
        BrowserNode::Group { label, children } => {
            let id: SharedString = format!("replay-browser-group-{next_group_id}").into();
            *next_group_id += 1;
            let children: Vec<TreeItem> =
                children.into_iter().map(|child| node_to_tree_item(child, next_group_id, leaf_info)).collect();
            TreeItem::new(id, label).children(children).expanded(true)
        }
        BrowserNode::Leaf { label, path, battle_result } => {
            let id: SharedString = path.to_string_lossy().into_owned().into();
            leaf_info.insert(id.clone(), LeafInfo { path, battle_result });
            TreeItem::new(id, label)
        }
    }
}

/// Maps a leaf's battle result to its label color: Win/Loss/Draw get the
/// win/loss/draw palette (`table.rs::resolve_color`, the same one the player
/// table uses), an unknown result (the common case for this milestone's
/// header-only scan; see the module doc) is left uncolored.
fn leaf_label_color(battle_result: Option<BattleResult>) -> Option<Hsla> {
    let outcome = match battle_result? {
        BattleResult::Win(_) => BattleOutcome::Win,
        BattleResult::Loss(_) => BattleOutcome::Loss,
        BattleResult::Draw => BattleOutcome::Draw,
    };
    Some(resolve_color(ColorRole::WinLoss(outcome)))
}

fn render_browser_item(
    browser: Entity<ReplayBrowser>,
    ix: usize,
    entry: &TreeEntry,
    selected: bool,
    leaf_info: &HashMap<SharedString, LeafInfo>,
) -> ListItem {
    let item = entry.item();
    let is_folder = entry.is_folder();
    let leaf = (!is_folder).then(|| leaf_info.get(&item.id)).flatten();

    let mut label_el = div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap();
    if let Some(leaf) = leaf {
        label_el = label_el.when_some(leaf_label_color(leaf.battle_result), |el, color| el.text_color(color));
    }
    label_el = label_el.child(item.label.clone());

    let mut row = h_flex().gap_1().items_center().pl(px(16.) * entry.depth());
    if is_folder {
        let chevron = if entry.is_expanded() { IconName::ChevronDown } else { IconName::ChevronRight };
        row = row.child(Icon::new(chevron));
    } else {
        row = row.child(div().w(px(16.)));
    }
    row = row.child(label_el);

    let mut list_item = ListItem::new(ix).selected(selected).child(row);

    if let Some(leaf) = leaf {
        let path = leaf.path.clone();
        let hover_browser = browser.clone();
        let hover_path = path.clone();
        list_item = list_item.on_mouse_enter(move |event: &MouseMoveEvent, _window, cx: &mut App| {
            let path = hover_path.clone();
            let position = event.position;
            hover_browser.update(cx, |browser, cx| browser.hover_leaf(path, position, cx));
        });
        list_item = list_item.on_click(move |event: &ClickEvent, _window, cx: &mut App| {
            browser.update(cx, |browser, cx| browser.handle_leaf_click(path.clone(), event.click_count(), cx));
        });
    }

    list_item
}

impl Render for ReplayBrowser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let entity = cx.entity();

        let header = h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(div().flex_1().text_sm().font_weight(FontWeight::BOLD).child("Replays"));

        let body = match &self.status {
            ScanStatus::Loading => div().p_2().text_sm().opacity(0.6).child("Scanning replays...").into_any_element(),
            ScanStatus::Failed(reason) => {
                div().p_2().text_sm().opacity(0.6).child(reason.to_string()).into_any_element()
            }
            ScanStatus::Empty => div().p_2().text_sm().opacity(0.6).child("No replays found").into_any_element(),
            // Every row's ship and map name comes from the game data, so a
            // list built before it loads is a list of raw ids. The egui app
            // never shows that state: it builds its listing as part of the
            // same load (`task/replays.rs::load_wows_files`).
            ScanStatus::Loaded if matches!(self.game_data, GameData::Loading) => {
                div().p_2().text_sm().opacity(0.6).child("Loading game data...").into_any_element()
            }
            ScanStatus::Loaded => {
                let entity = entity.clone();
                let leaf_info = self.leaf_info.clone();
                let context_menu_leaf_info = self.leaf_info.clone();
                tree(&self.tree_state, move |ix, entry, selected, _window, _cx| {
                    render_browser_item(entity.clone(), ix, entry, selected, &leaf_info)
                })
                .context_menu(move |_ix, entry, menu, _window, _cx| {
                    if entry.is_folder() {
                        return menu;
                    }
                    let Some(leaf) = context_menu_leaf_info.get(&entry.item().id) else {
                        return menu;
                    };
                    let path = leaf.path.clone();
                    menu.item(PopupMenuItem::new("Copy Path").on_click(move |_event, _window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(path.to_string_lossy().into_owned()));
                    }))
                })
                .flex_1()
                .into_any_element()
            }
        };

        // The hovered row's battle, played back beside the listing. Anchored
        // at the pointer like the egui app's own hover popup
        // (`ui/replay_parser/preview_popup.rs`), and deferred so it paints
        // over the panel rather than inside its scroll area.
        let preview_content: Option<AnyElement> = match self.preview.frame() {
            Some(frame) => Some(img(frame).w(px(PREVIEW_SIZE)).h(px(PREVIEW_SIZE)).into_any_element()),
            // A bake reads the replay and loads the build it was recorded on,
            // which takes seconds the first time; the egui popup says so with
            // a spinner over the map (`preview_popup.rs`), and so does this.
            None if self.preview.is_baking() => Some(
                h_flex()
                    .w(px(PREVIEW_SIZE))
                    .h(px(PREVIEW_SIZE))
                    .items_center()
                    .justify_center()
                    .child(div().text_sm().opacity(0.6).child("Loading preview..."))
                    .into_any_element(),
            ),
            None => None,
        };
        let preview_popup = preview_content.map(|content| {
            let theme = cx.theme();
            let anchor = point(self.preview_anchor.x + PREVIEW_CURSOR_OFFSET, self.preview_anchor.y);
            deferred(
                anchored().position(anchor).snap_to_window_with_margin(px(8.)).child(
                    div()
                        .p_1()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .child(content),
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
fn scan_replay_files(replays_dir: &Path) -> Vec<RawReplay> {
    let Ok(entries) = std::fs::read_dir(replays_dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
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

        match ReplayFile::meta_from_file(&path) {
            Ok(meta) => {
                let ship_id = meta.vehicles.iter().find(|vehicle| vehicle.relation == 0).map(|vehicle| vehicle.shipId);
                out.push(RawReplay {
                    ship_id,
                    raw_ship: meta.playerVehicle,
                    raw_map: meta.mapName,
                    game_time: meta.dateTime,
                    path,
                })
            }
            Err(err) => tracing::warn!(path = %path.display(), error = ?err, "failed to read replay meta"),
        }
    }
    out
}

/// The full background-scan step: resolve the replays directory, then read
/// every replay's header. Run on `cx.background_spawn` (see `start_scan`),
/// never on the UI thread.
fn scan_replays_dir(wows_dir: &str) -> Vec<RawReplay> {
    let replays_dir = resolve_replays_dir(Path::new(wows_dir));
    scan_replay_files(&replays_dir)
}

/// Resolves one `RawReplay` into a `ReplayLite` against `provider` (the
/// browser's currently adopted game data, `None` before `set_game_data` first
/// sees `GameDataStatus::Ready` -- see the module doc). Mirrors the egui
/// app's `Replay::vehicle_name`/`map_name` (`mod.rs:2580`/`2592`) exactly
/// when `provider` is present; falls back to the untranslated raw fields
/// otherwise.
fn translate_replay(raw: &RawReplay, provider: Option<&GameMetadataProvider>) -> ReplayLite {
    let ship = translate_ship_name(raw.ship_id, &raw.raw_ship, provider);
    let map = match provider {
        Some(provider) => translate_map_name(&raw.raw_map, provider),
        None => raw.raw_map.clone(),
    };
    ReplayLite { path: raw.path.clone(), ship, map, game_time: raw.game_time.clone(), battle_result: None }
}

/// Mirrors `Replay::vehicle_name` (`mod.rs:2580`): resolves `ship_id`'s
/// translation id via `param_localization_id`, then looks that id up in the
/// provider's catalog via `localized_name_from_id`. Falls back to
/// `SPECTATOR_LABEL` when `provider` is present but either `ship_id` is
/// absent (no relation-0 vehicle in the replay's header) or no translation
/// resolves, and to `raw_ship` when `provider` itself is not yet loaded.
fn translate_ship_name(
    ship_id: Option<GameParamId>,
    raw_ship: &str,
    provider: Option<&GameMetadataProvider>,
) -> String {
    let Some(provider) = provider else { return raw_ship.to_string() };
    let Some(ship_id) = ship_id else { return SPECTATOR_LABEL.to_string() };
    provider
        .param_localization_id(ship_id)
        .and_then(|translation_id| provider.localized_name_from_id(&TranslationKey::new(translation_id)))
        .unwrap_or_else(|| SPECTATOR_LABEL.to_string())
}

#[cfg(test)]
mod tests {
    use super::last_server_version;
    use super::resolve_replays_dir;

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
