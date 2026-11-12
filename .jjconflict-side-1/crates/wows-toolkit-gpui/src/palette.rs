//! The command palette: everything the window can do, by name.
//!
//! Ports the egui app's `ui/command_palette.rs`, which opens on ctrl+k or
//! ctrl+p. The root list is built once and filtered here as the reader types,
//! as is every second layer held in memory. Some root
//! entries open a second layer (`ModeDialog`) that queries the replay index,
//! filters the ship catalogue, lists the replay listing's game versions, or
//! lists the recently opened replay directories as the reader types; Escape,
//! or Backspace in an empty field, returns to the root.

use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use futures::FutureExt as _;
use futures::future::Shared;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::KeyDownEvent;
use gpui_kit::ParentElement as _;
use gpui_kit::Render;
use gpui_kit::SharedString;
use gpui_kit::Styled as _;
use gpui_kit::Subscription;
use gpui_kit::Task;
use gpui_kit::WeakEntity;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::command::Command;
use gpui_kit::component::command::CommandItem;
use gpui_kit::component::command::CommandState;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use rust_i18n::t;
use wows_toolkit_replay_data::force_reindex::ReindexScope;
use wows_toolkit_replay_data::force_reindex::VersionCount;
use wows_toolkit_replay_data::force_reindex::version_said;
use wows_toolkit_replay_data::paths::shown_path;
use wows_toolkit_viewmodel::name_lookup::PlayerLookup;
use wows_toolkit_viewmodel::name_lookup::ShipLookup;
use wowsunpack::game_params::types::Species;

use crate::app::App;
use crate::app::AppTab;
use crate::armor_viewer::ArmorViewerPane;
use crate::armor_viewer::ShipCatalog;
use crate::armor_viewer::catalog::species_name;
use crate::armor_viewer::catalog::tier_roman;
use crate::replay_inspector::ReplayInspectorView;
use crate::runtime;
use crate::settings_store;
use crate::text_filter::FilterKey;
use crate::text_filter::FilterQuery;
use crate::text_filter::MarkedQuery;
use crate::text_filter::highlighted_text;
use crate::text_filter::no_ranges;
use wows_toolkit_viewmodel::settings::ThemeChoice;

/// What confirming an entry asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    GoTo(AppTab),
    /// Open the Settings drawer.
    OpenSettings,
    SetTheme(ThemeChoice),
    OpenReplayFile,
    /// Put a query in the Search tab's bar and run it.
    SearchFor(String),
    /// Fill the palette with what one of the cascading modes offers.
    ///
    /// The egui palette's own three (`ui/command_palette.rs`'s `SubKind`): a
    /// player to find matches with, a ship of the reader's own to list their
    /// matches in, and a ship to look at the armor of.
    EnterMode(PaletteMode),
    /// Open one ship in the Armor Viewer, by the parameter that names it.
    ViewArmor {
        param_index: String,
        display_name: String,
    },
    /// Put the newest log file on the clipboard, for a bug report.
    CopyLatestLog,
    /// Put the small support summary on the clipboard.
    CopyBugReportInfo,
    /// Ask for a directory of replays and open it as a tab of the listing.
    BrowseReplayFolder,
    /// Open a recently opened directory as a tab of the listing.
    OpenReplayFolder(PathBuf),
    /// Read every replay that is not indexed yet.
    IndexAllReplays,
    /// Read every replay again and rewrite what the index holds, once the reader
    /// has confirmed it.
    RefreshPersistedData,
    /// Contribute every listed battle, as the data-sharing setting asks.
    ContributeAllReplays(wows_toolkit_services::upload::LedgerUse),
    /// Take a result mapping the reader points at as the loaded build's.
    ImportConstants,
    /// Read the listed replays in scope again and rewrite their index entries,
    /// once the reader has confirmed it.
    ForceReindex(ReindexScope),
}

/// Which cascading list the palette is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    /// Players the index has seen, most met first.
    Players,
    /// Ships the reader has played, most played first.
    MyShips,
    /// Every ship in the loaded build, to open in the Armor Viewer.
    ArmorShips,
    /// The game versions the listing holds, to force a re-index of one.
    ReindexVersions,
    /// "Browse..." and the directories most recently opened, to open one in
    /// the listing.
    ReplayFolders,
}

impl PaletteMode {
    /// The modes the root list leads into from its searches. The version list
    /// is offered beside the other re-index command instead.
    pub const ALL: [PaletteMode; 3] = [Self::Players, Self::MyShips, Self::ArmorShips];

    /// What the root entry that enters this mode is called.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.search_players",
            Self::MyShips => "ui.palette.my_matches_in_ship",
            Self::ArmorShips => "ui.palette.open_ship_in_armor_viewer",
            Self::ReindexVersions => "ui.reindex.palette_by_version",
            Self::ReplayFolders => "ui.replay.open_directory",
        }
    }

    /// What the layer is called in its header, after the root's name.
    pub const fn title_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_title",
            Self::MyShips => "ui.palette.my_ships_title",
            Self::ArmorShips => "ui.palette.armor_ships_title",
            Self::ReindexVersions => "ui.reindex.versions_title",
            Self::ReplayFolders => "ui.replay_folders.recent_title",
        }
    }

    pub const fn placeholder_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_placeholder",
            Self::MyShips => "ui.palette.my_ships_placeholder",
            Self::ArmorShips => "ui.palette.armor_ships_placeholder",
            Self::ReindexVersions => "ui.reindex.versions_placeholder",
            Self::ReplayFolders => "ui.replay_folders.recent_placeholder",
        }
    }
}

/// Maximum rows one index-backed layer lists.
pub const MODE_LIMIT: usize = 50;

/// One entry: what it is called and what it does.
#[derive(Clone)]
pub struct PaletteEntry {
    pub label: String,
    pub action: PaletteAction,
}

/// One ship the ship layer lists.
#[derive(Clone, Debug)]
pub struct ArmorShipRow {
    param_index: String,
    display_name: String,
    /// The display name folded for the filter.
    key: FilterKey,
    /// The name with tier, class and nation, which is what the row shows.
    label: String,
}

impl ArmorShipRow {
    pub fn new(param_index: String, display_name: String, tier: u32, species: Species, nation: &str) -> Self {
        let key = FilterKey::new(&display_name);
        let label = t!(
            "ui.palette.armor_ship_row",
            name = display_name,
            tier = tier_roman(tier),
            class = species_name(&species),
            nation = nation
        )
        .into_owned();
        Self { param_index, display_name, key, label }
    }
}

/// Every ship in `catalog`, in its nation, class and tier order.
pub fn armor_ship_rows(catalog: &ShipCatalog) -> Vec<ArmorShipRow> {
    let mut rows = Vec::new();
    for nation in &catalog.nations {
        for class in &nation.classes {
            for ship in &class.ships {
                rows.push(ArmorShipRow::new(
                    ship.param_index.clone(),
                    ship.display_name.clone(),
                    ship.tier,
                    class.species,
                    &nation.display_name,
                ));
            }
        }
    }
    rows
}

/// The ship layer's rows for `query`: every ship in catalogue order when it
/// is inactive, else the matches by name, best first.
pub fn armor_ship_entries(rows: &[ArmorShipRow], query: &FilterQuery) -> Vec<PaletteEntry> {
    query
        .rank(rows, |row| &row.key)
        .into_iter()
        .map(|row| PaletteEntry {
            label: row.label.clone(),
            action: PaletteAction::ViewArmor {
                param_index: row.param_index.clone(),
                display_name: row.display_name.clone(),
            },
        })
        .collect()
}

/// The version layer's rows for `query`: every version newest first when it
/// is inactive, else the versions whose `major.minor.patch` matches, best
/// first.
pub fn version_entries(rows: &[VersionCount], query: &FilterQuery) -> Vec<PaletteEntry> {
    let keyed = rows.iter().map(|row| {
        let said = version_said(&row.version);
        let entry = PaletteEntry {
            label: t!("ui.reindex.version_row", version = said, count = row.replays).into_owned(),
            action: PaletteAction::ForceReindex(ReindexScope::Version(row.version)),
        };
        (FilterKey::new(&said), entry)
    });

    query.rank(keyed, |(key, _)| key).into_iter().map(|(_, entry)| entry).collect()
}

/// Whether a recent directory is still there, as far as the layer knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderPresence {
    /// Not looked for yet: listed as present until the check returns.
    Unchecked,
    Present,
    /// Listed dimmed.
    Missing,
}

/// A recently opened directory as the directory layer lists it.
#[derive(Clone, Debug)]
pub struct FolderRow {
    pub path: PathBuf,
    pub presence: FolderPresence,
}

impl FolderRow {
    /// A row whose directory has not been looked for: that happens off the UI
    /// thread, since a directory on an offline share can take many seconds
    /// to answer.
    pub fn new(path: PathBuf) -> Self {
        Self { path, presence: FolderPresence::Unchecked }
    }
}

/// What a directory row is called: the directory's own name.
fn folder_label(path: &Path) -> String {
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => shown_path(path),
    }
}

/// The directory layer's rows for `query`: "Browse..." first, then the
/// recently opened directories, most recent first when the query is
/// inactive, else those whose shown path matches, best first.
pub fn folder_entries(rows: &[FolderRow], query: &FilterQuery) -> Vec<PaletteEntry> {
    let browse =
        PaletteEntry { label: t!("ui.replay_folders.browse").into_owned(), action: PaletteAction::BrowseReplayFolder };
    let keyed = rows.iter().map(|row| {
        let entry =
            PaletteEntry { label: folder_label(&row.path), action: PaletteAction::OpenReplayFolder(row.path.clone()) };
        (FilterKey::new(&shown_path(&row.path)), entry)
    });
    let recent = query.rank(keyed, |(key, _)| key).into_iter().map(|(_, entry)| entry);

    std::iter::once(browse).chain(recent).collect()
}

/// The directory layer's rows as the component draws them: the name, with
/// the whole path dimmed after it, and the row dimmed and marked when the
/// directory is gone.
fn folder_items(entries: &[PaletteEntry], rows: &[FolderRow], query: &MarkedQuery) -> Vec<CommandItem> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let PaletteAction::OpenReplayFolder(path) = &entry.action else {
                return marked_item(entry, query, Some(IconName::FolderOpen));
            };
            let missing = rows.iter().any(|row| row.path == *path && row.presence == FolderPresence::Missing);
            let name = SharedString::from(entry.label.clone());
            let shown = SharedString::from(shown_path(path));
            // The row is kept by its path; the name is marked where it also
            // matches, and drawn unmarked otherwise.
            let name_marks = query.ranges(&name).unwrap_or_else(no_ranges);
            let shown_marks = query.ranges(&shown).unwrap_or_else(no_ranges);
            CommandItem::new().label(entry.label.clone()).child(move |_window, cx| {
                let dim = crate::theme::text_dim();
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .when(missing, |row| row.opacity(0.5))
                    .child(crate::icons::icon(crate::icons::Glyph::Folder).text_color(dim))
                    .child(
                        div()
                            .flex_none()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(highlighted_text(name.clone(), &name_marks)),
                    )
                    .when(missing, |row| {
                        row.child(
                            crate::icons::icon(crate::icons::Glyph::TriangleAlert)
                                .id(("palette-folder-missing", index))
                                .test_support()
                                .text_color(cx.theme().warning),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(dim)
                            .child(highlighted_text(shown.clone(), &shown_marks)),
                    )
            })
        })
        .collect()
}

/// The ship catalogue as the ship layer sees it.
pub enum ShipList {
    /// Still loading; the layer fills in once it lands.
    Pending,
    Ready(Rc<[ArmorShipRow]>),
    /// Why there is no catalogue to list.
    Unavailable(String),
}

/// Where a second layer's rows come from.
pub enum ModeSource {
    /// The replay index, read once when first searched and then ranked as
    /// the reader types. The read is shared, so a keystroke made while it
    /// runs waits on it rather than starting it again.
    Index { list: IndexList, read: Option<LookupRead> },
    /// The ship catalogue, filtered in memory.
    Ships(ShipList),
    /// The game versions the listing holds, newest first.
    Versions(Rc<[VersionCount]>),
    /// The directories most recently opened, most recent first.
    Folders(Rc<[FolderRow]>),
}

impl ModeSource {
    /// `list`, to be read from the index when first searched.
    pub fn index(list: IndexList) -> Self {
        Self::Index { list, read: None }
    }
}

/// What an index-backed layer lists.
#[derive(Clone, Copy, Debug)]
pub enum IndexList {
    Players,
    /// The ships the reader has played.
    MyShips,
}

/// An index-backed layer's rows, read once and ranked for each query.
pub enum IndexLookup {
    Players(PlayerLookup),
    Ships(ShipLookup),
}

/// The one read of an index-backed layer's rows.
type LookupRead = Shared<Task<Result<Arc<IndexLookup>, String>>>;

impl IndexList {
    /// Reads the rows `self` ranks.
    pub async fn load(self, pool: &sqlx::sqlite::SqlitePool) -> Result<IndexLookup, String> {
        match self {
            Self::Players => PlayerLookup::load(pool)
                .await
                .map(IndexLookup::Players)
                .map_err(|err| format!("The indexed players could not be read: {err}")),
            Self::MyShips => ShipLookup::load_played(pool)
                .await
                .map(IndexLookup::Ships)
                .map_err(|err| format!("The indexed ships could not be read: {err}")),
        }
    }
}

/// Stands in for the ship catalogue in tests, which have no game data to
/// build one from, and records the ships the palette asked the viewer to open.
#[cfg(test)]
#[derive(Default)]
pub struct TestArmorShips {
    pub rows: Vec<ArmorShipRow>,
    pub opened: Vec<String>,
}

#[cfg(test)]
impl gpui_kit::Global for TestArmorShips {}

/// A second-layer list: one of the `PaletteMode`s, with a header leading back
/// to the root.
pub struct ModeDialog {
    command: Entity<CommandState>,
    owner: WeakEntity<App>,
    mode: PaletteMode,
    source: ModeSource,
    entries: Rc<Vec<PaletteEntry>>,
    /// The query `entries` were found for, which their rows mark.
    query: MarkedQuery,
    error: Option<String>,
    generation: u64,
    _query_task: Option<Task<()>>,
    /// Held while the ship catalogue is still loading.
    _catalog_watch: Vec<Subscription>,
    /// Looks for the recent directories, while that runs.
    _folder_check: Option<Task<()>>,
}

impl ModeDialog {
    pub fn new(command: Entity<CommandState>, owner: WeakEntity<App>, mode: PaletteMode, source: ModeSource) -> Self {
        Self {
            command,
            owner,
            mode,
            source,
            entries: Rc::default(),
            query: MarkedQuery::default(),
            error: None,
            generation: 0,
            _query_task: None,
            _catalog_watch: Vec::new(),
            _folder_check: None,
        }
    }

    /// Refills the ship list as the views that load the catalogue change,
    /// while it is still pending.
    pub fn wait_for_ships(
        &mut self,
        inspector: &Entity<ReplayInspectorView>,
        armor_pane: &Entity<ArmorViewerPane>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.source, ModeSource::Ships(ShipList::Pending)) {
            return;
        }
        self._catalog_watch = vec![
            cx.observe_in(inspector, window, |this, _, window, cx| this.refresh_ships(window, cx)),
            cx.observe_in(armor_pane, window, |this, _, window, cx| this.refresh_ships(window, cx)),
        ];
    }

    /// Looks for the recent directories off the UI thread, and dims the ones
    /// that are gone once it knows.
    pub fn check_folders(&mut self, cx: &mut Context<Self>) {
        let ModeSource::Folders(rows) = &self.source else { return };

        let paths: Vec<PathBuf> = rows.iter().map(|row| row.path.clone()).collect();
        self._folder_check = Some(cx.spawn(async move |this, cx| {
            let checked: Vec<FolderRow> = cx
                .background_spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let presence =
                                if path.is_dir() { FolderPresence::Present } else { FolderPresence::Missing };
                            FolderRow { path, presence }
                        })
                        .collect()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.source = ModeSource::Folders(checked.into());
                cx.notify();
            });
        }));
    }

    fn refresh_ships(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.source, ModeSource::Ships(ShipList::Pending)) {
            return;
        }
        let Ok(ships) = self.owner.update(cx, |app, cx| app.armor_ship_list(window, cx)) else { return };
        if matches!(ships, ShipList::Pending) {
            return;
        }
        self.source = ModeSource::Ships(ships);
        self._catalog_watch.clear();
        let query = self.command.read(cx).query(cx).to_string();
        self.search(query, window, cx);
    }

    pub fn search(&mut self, needle: String, window: &mut Window, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.error = None;
        self.query = MarkedQuery::new(&needle);

        let (list, read) = match &self.source {
            ModeSource::Ships(ShipList::Ready(rows)) => {
                self.entries = Rc::new(armor_ship_entries(rows, &self.query));
                cx.notify();
                return;
            }
            ModeSource::Ships(ShipList::Pending | ShipList::Unavailable(_)) => {
                self.entries = Rc::default();
                cx.notify();
                return;
            }
            ModeSource::Versions(rows) => {
                self.entries = Rc::new(version_entries(rows, &self.query));
                cx.notify();
                return;
            }
            ModeSource::Folders(rows) => {
                self.entries = Rc::new(folder_entries(rows, &self.query));
                cx.notify();
                return;
            }
            ModeSource::Index { list, read } => (*list, read.clone()),
        };

        self.entries = Rc::default();
        self.command.update(cx, |command, cx| command.set_loading(true, window, cx));
        cx.notify();

        let Some(pool) = settings_store::pool(cx) else {
            self.error = Some("The replay index is not open".to_string());
            self.command.update(cx, |command, cx| command.set_loading(false, window, cx));
            cx.notify();
            return;
        };
        let Some(runtime) = runtime::runtime(cx) else {
            self.error = Some("The background runtime is not open".to_string());
            self.command.update(cx, |command, cx| command.set_loading(false, window, cx));
            cx.notify();
            return;
        };
        let read = match read {
            Some(read) => read,
            None => {
                let read = cx
                    .background_spawn(async move { runtime.handle().block_on(list.load(&pool)).map(Arc::new) })
                    .shared();
                if let ModeSource::Index { read: held, .. } = &mut self.source {
                    *held = Some(read.clone());
                }
                read
            }
        };
        let query = self.query.query().clone();

        self._query_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
            let current = this.update(cx, |this, _cx| this.generation == generation).unwrap_or(false);
            if !current {
                return;
            }

            let found = match read.await {
                Ok(lookup) => Ok(cx.background_spawn(async move { mode_entries(&lookup, &query) }).await),
                Err(error) => Err(error),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                // A failed read is dropped so that the next query tries again.
                if found.is_err()
                    && let ModeSource::Index { read, .. } = &mut this.source
                {
                    *read = None;
                }
                if this.generation != generation {
                    return;
                }
                match found {
                    Ok(entries) => {
                        this.entries = Rc::new(entries);
                        this.error = None;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "palette: mode query failed");
                        this.entries = Rc::default();
                        this.error = Some(error);
                    }
                }
                this.command.update(cx, |command, cx| command.set_loading(false, window, cx));
                cx.notify();
            });
        }));
    }

    /// What the list says when it has no rows.
    fn empty_message(&self) -> String {
        match (&self.source, &self.error) {
            (ModeSource::Ships(ShipList::Pending), _) => t!("ui.palette.ships_loading").into_owned(),
            (ModeSource::Ships(ShipList::Unavailable(reason)), _) => {
                t!("ui.palette.ships_unavailable", reason = reason).into_owned()
            }
            (ModeSource::Versions(rows), _) if rows.is_empty() => t!("ui.reindex.no_versions").into_owned(),
            (_, Some(error)) => t!("ui.search.failed", reason = error).into_owned(),
            (_, None) => t!("ui.search.no_matches").into_owned(),
        }
    }
}

/// Puts the root list back in place of a second layer.
///
/// Deferred: it replaces the dialog the request came from.
fn back_to_root(owner: &WeakEntity<App>, window: &mut Window, cx: &mut gpui_kit::App) {
    let owner = owner.clone();
    window.defer(cx, move |window, cx| {
        let _ = owner.update(cx, |app, cx| app.open_palette(window, cx));
    });
}

impl Render for ModeDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = Rc::clone(&self.entries);
        let owner = self.owner.clone();
        let command_state = self.command.clone();
        let dialog = cx.entity();
        let render_generation = self.generation;
        let title = t!(self.mode.title_key()).into_owned();

        let command = Command::new(&self.command)
            .bordered(false)
            // Matches the dialog's corners, which the command's own square
            // background would otherwise show past.
            .rounded(cx.theme().radius_lg)
            .filterable(false)
            .placeholder(t!(self.mode.placeholder_key()).to_string())
            .max_h(px(400.))
            .items(match &self.source {
                ModeSource::Folders(rows) => folder_items(&entries, rows, &self.query),
                _ => items(&entries, &self.query),
            })
            .header({
                let owner = self.owner.clone();
                move |_state, _window, cx| {
                    let owner = owner.clone();
                    let muted = cx.theme().muted_foreground;
                    h_flex()
                        .px_2()
                        .py_1()
                        .gap_1()
                        .items_center()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            Button::new("palette-back")
                                .ghost()
                                .xsmall()
                                .icon(IconName::ArrowLeft)
                                .label(t!("ui.palette.title").to_string())
                                .on_click(move |_, window, cx| back_to_root(&owner, window, cx)),
                        )
                        .child(Icon::new(IconName::ChevronRight).xsmall().text_color(muted))
                        .child(div().text_sm().child(title.clone()))
                        .child(
                            div().ml_auto().text_xs().text_color(muted).child(t!("ui.palette.back_hint").to_string()),
                        )
                }
            })
            .empty({
                let dialog = dialog.clone();
                move |_state, _window, cx| {
                    let dialog = dialog.read(cx);
                    let message = dialog.empty_message();
                    if matches!(dialog.source, ModeSource::Ships(ShipList::Pending)) {
                        return crate::loading::Loading::new("palette-ships-loading", message).top().into_any_element();
                    }
                    div().p_4().text_sm().text_color(crate::theme::text_dim()).child(message).into_any_element()
                }
            })
            .on_query({
                let dialog = dialog.clone();
                move |query, window, cx| {
                    dialog.update(cx, |dialog, cx| dialog.search(query.to_string(), window, cx));
                }
            })
            .on_cancel({
                let owner = self.owner.clone();
                move |window, cx| back_to_root(&owner, window, cx)
            })
            .on_confirm(move |index_path, window, cx| {
                if command_state.read(cx).is_loading() || dialog.read(cx).generation != render_generation {
                    return;
                }
                let Some(entry) = entries.get(index_path.row) else { return };
                let action = entry.action.clone();
                let _ = owner.update(cx, |app, cx| app.run_palette_action(action, window, cx));
                window.close_all_dialogs(cx);
            });

        v_flex()
            .size_full()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let keystroke = &event.keystroke;
                if keystroke.key == "backspace"
                    && !keystroke.modifiers.modified()
                    && this.command.read(cx).query(cx).is_empty()
                {
                    cx.stop_propagation();
                    back_to_root(&this.owner, window, cx);
                }
            }))
            .child(command)
    }
}

/// Every entry the palette offers, in the order it lists them.
///
/// Tabs first because going somewhere is the common reason to open it, then
/// the searches, then the settings-like actions.
pub fn entries() -> Vec<PaletteEntry> {
    let mut entries = Vec::new();

    for tab in AppTab::ALL {
        if tab == AppTab::Search {
            continue;
        }
        entries.push(PaletteEntry {
            label: t!("ui.palette.go_to", tab = t!(tab.label_key())).into_owned(),
            action: PaletteAction::GoTo(tab),
        });
    }

    entries
        .push(PaletteEntry { label: t!("ui.settings.panel.open").into_owned(), action: PaletteAction::OpenSettings });

    entries.push(PaletteEntry {
        label: t!("ui.palette.advanced_search").into_owned(),
        action: PaletteAction::GoTo(AppTab::Search),
    });

    // The cascading modes first, as the egui palette lists them: they are the
    // entries that lead somewhere rather than doing something.
    for mode in PaletteMode::ALL {
        entries.push(PaletteEntry { label: t!(mode.label_key()).into_owned(), action: PaletteAction::EnterMode(mode) });
    }

    // The two seeded searches the egui palette offers. The queries are the
    // grammar the Search tab parses, so confirming one fills the bar with
    // something the reader can then edit.
    entries.push(PaletteEntry {
        label: t!("ui.palette.games_i_died_in").into_owned(),
        // A battle lost in which the reader's own ship sank, which is the egui
        // seed (`query_bar::seed::games_i_died_in`). `survived:false` alone also
        // returns the wins they sank in.
        action: PaletteAction::SearchFor("outcome:loss self.survived:false".to_owned()),
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.games_i_won").into_owned(),
        action: PaletteAction::SearchFor("outcome:win".to_owned()),
    });

    entries.push(PaletteEntry {
        label: t!("ui.replay.open_manually").into_owned(),
        action: PaletteAction::OpenReplayFile,
    });
    entries.push(PaletteEntry {
        label: t!(PaletteMode::ReplayFolders.label_key()).into_owned(),
        action: PaletteAction::EnterMode(PaletteMode::ReplayFolders),
    });
    entries.push(PaletteEntry {
        label: t!("ui.settings.replay.index_all_replays").into_owned(),
        action: PaletteAction::IndexAllReplays,
    });
    entries.push(PaletteEntry {
        label: t!(PaletteMode::ReindexVersions.label_key()).into_owned(),
        action: PaletteAction::EnterMode(PaletteMode::ReindexVersions),
    });
    entries.push(PaletteEntry {
        label: t!("ui.reindex.palette_all").into_owned(),
        action: PaletteAction::ForceReindex(ReindexScope::All),
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.refresh_persisted_data").into_owned(),
        action: PaletteAction::RefreshPersistedData,
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.import_constants").into_owned(),
        action: PaletteAction::ImportConstants,
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.send_all_replays").into_owned(),
        action: PaletteAction::ContributeAllReplays(wows_toolkit_services::upload::LedgerUse::Consult),
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.send_all_replays_again").into_owned(),
        action: PaletteAction::ContributeAllReplays(wows_toolkit_services::upload::LedgerUse::Ignore),
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.copy_latest_log").into_owned(),
        action: PaletteAction::CopyLatestLog,
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.copy_bug_report_info").into_owned(),
        action: PaletteAction::CopyBugReportInfo,
    });

    for (key, choice) in [
        ("ui.settings.app.theme_system", ThemeChoice::System),
        ("ui.settings.app.theme_dark", ThemeChoice::Dark),
        ("ui.settings.app.theme_light", ThemeChoice::Light),
    ] {
        entries.push(PaletteEntry {
            label: t!("ui.palette.set_theme", theme = t!(key)).into_owned(),
            action: PaletteAction::SetTheme(choice),
        });
    }

    entries
}

/// The root list, filtered by the query typed into it.
pub struct RootPalette {
    entries: Vec<PaletteEntry>,
    /// Each entry's label folded, in `entries` order.
    keys: Vec<FilterKey>,

    query: MarkedQuery,
    /// The entries `query` keeps, best first.
    shown: Rc<Vec<PaletteEntry>>,
}

impl RootPalette {
    pub fn new(entries: Vec<PaletteEntry>) -> Self {
        let keys = entries.iter().map(|entry| FilterKey::new(&entry.label)).collect();
        let shown = Rc::new(entries.clone());
        Self { entries, keys, query: MarkedQuery::default(), shown }
    }

    pub fn search(&mut self, typed: &str) {
        self.query = MarkedQuery::new(typed);
        let kept = self.query.rank(self.entries.iter().zip(&self.keys), |(_, key)| *key);
        self.shown = Rc::new(kept.into_iter().map(|(entry, _)| entry.clone()).collect());
    }

    pub fn shown(&self) -> Rc<Vec<PaletteEntry>> {
        Rc::clone(&self.shown)
    }

    pub fn query(&self) -> &MarkedQuery {
        &self.query
    }
}

/// The entries as the component's own items, in the same order, so an index
/// into one is an index into the other. Each row marks where `query`
/// matched it.
pub fn items(entries: &[PaletteEntry], query: &MarkedQuery) -> Vec<CommandItem> {
    entries.iter().map(|entry| marked_item(entry, query, None)).collect()
}

/// One entry's row: the label, marked where `query` matched the text the
/// layer filtered on, and named by the label for accessibility.
fn marked_item(entry: &PaletteEntry, query: &MarkedQuery, icon: Option<IconName>) -> CommandItem {
    let label = SharedString::from(entry.label.clone());
    let marks = entry_marks(entry, query);

    // The component re-measures every custom row each time the list is
    // installed, so only a row with marks to draw is one.
    if marks.is_empty() {
        let item = CommandItem::new().label(label);
        return match icon {
            Some(icon) => item.icon(icon),
            None => item,
        };
    }

    CommandItem::new().label(label.clone()).child(move |_window, cx| {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .flex_1()
            .gap_2()
            .items_center()
            .when_some(icon.clone(), |row, icon| row.child(Icon::new(icon).size_4().text_color(muted)))
            .child(highlighted_text(label.clone(), &marks))
    })
}

/// The byte ranges of `entry.label` to mark for `query`. A layer that
/// filters on part of the label (a ship's name, a version) marks only that
/// part; the others mark the label itself.
fn entry_marks(entry: &PaletteEntry, query: &MarkedQuery) -> Rc<[Range<usize>]> {
    let filtered = match &entry.action {
        PaletteAction::ViewArmor { display_name, .. } => Some(display_name.clone()),
        PaletteAction::ForceReindex(ReindexScope::Version(version)) => Some(version_said(version)),
        _ => None,
    };
    // A row kept by a text other than the one marked draws unmarked.
    let Some(filtered) = filtered else { return query.ranges(&entry.label).unwrap_or_else(no_ranges) };
    let Some(offset) = entry.label.find(&filtered) else { return no_ranges() };

    query
        .ranges(&filtered)
        .unwrap_or_else(no_ranges)
        .iter()
        .map(|range| range.start + offset..range.end + offset)
        .collect()
}

/// The rows `lookup` offers for `query`, best first.
pub fn mode_entries(lookup: &IndexLookup, query: &FilterQuery) -> Vec<PaletteEntry> {
    use wows_toolkit_config::index::query_text::print_query;
    use wows_toolkit_viewmodel::query_bar::seed;

    match lookup {
        IndexLookup::Players(players) => players
            .find(query, MODE_LIMIT)
            .into_iter()
            .map(|player| {
                let named = match player.clan.as_str() {
                    "" => player.latest_name.clone(),
                    clan => format!("[{clan}] {}", player.latest_name),
                };
                PaletteEntry {
                    label: format!("{named} -- {}", t!("ui.palette.matches_found", count = player.match_count)),
                    action: PaletteAction::SearchFor(print_query(&seed::matches_with_player(player.account_id))),
                }
            })
            .collect(),
        IndexLookup::Ships(ships) => ships
            .find(query, MODE_LIMIT)
            .into_iter()
            .map(|ship| PaletteEntry {
                label: format!("{} -- {}", ship.ship_name, t!("ui.palette.matches_found", count = ship.match_count)),
                action: PaletteAction::SearchFor(print_query(&seed::my_matches_in_ship(ship.ship_id))),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::ParentElement as _;
    use gpui_kit::test::TestWindowExt as _;

    use super::IndexList;
    use super::IndexLookup;
    use super::PaletteAction;
    use super::PaletteMode;
    use super::entries;
    use super::mode_entries;
    use crate::app::AppTab;
    use crate::text_filter::FilterQuery;

    /// Every tab and the Settings drawer are reachable by name: the palette is
    /// the keyboard route to the tab strip and the gear beside it, so one
    /// missing from it cannot be reached that way.
    #[test]
    fn every_tab_and_the_settings_are_offered() {
        let offered: Vec<AppTab> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::GoTo(tab) => Some(tab),
                _ => None,
            })
            .collect();
        for tab in AppTab::ALL {
            assert!(offered.contains(&tab), "{tab:?} is not in the palette");
        }
        assert!(entries().iter().any(|entry| entry.action == PaletteAction::OpenSettings), "the settings are not");
    }

    /// Every seeded search parses, and reads as what its label promises: the
    /// egui palette seeds the same searches as an expression, so the text here
    /// has to come out the same way.
    #[test]
    fn the_seeded_searches_mean_what_they_say() {
        use wows_toolkit_config::index::query_ast::MatchExpr;
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::seed;

        let seeded: Vec<String> = entries()
            .iter()
            .filter_map(|entry| match &entry.action {
                PaletteAction::SearchFor(query) => Some(query.clone()),
                _ => None,
            })
            .collect();
        assert!(!seeded.is_empty(), "the palette offers at least one search");

        let parsed: Vec<MatchExpr> = seeded
            .iter()
            .map(|query| {
                query_text::parse_query(query).unwrap_or_else(|err| panic!("{query:?} does not parse: {err:?}"))
            })
            .collect();

        assert!(
            parsed.contains(&seed::games_i_died_in()),
            "a battle the reader sank in is a loss and a sinking, got {parsed:?}"
        );
        assert!(parsed.contains(&seed::games_i_won()), "got {parsed:?}");
    }

    /// All three cascading modes are offered from the root, as the egui palette
    /// offers them: each is the way into a list the reader then filters.
    #[test]
    fn every_cascading_mode_is_offered() {
        let offered: Vec<PaletteMode> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::EnterMode(mode) => Some(mode),
                _ => None,
            })
            .collect();

        for mode in PaletteMode::ALL {
            assert!(offered.contains(&mode), "{mode:?} is not in the palette");
        }
    }

    /// Both forced re-index commands are on the root list. The version layer
    /// lists newest first until a query is typed, then the best match first.
    #[test]
    fn the_reindex_commands_are_offered_and_versions_filter_in_order() {
        use wows_toolkit_replay_data::force_reindex::ReindexScope;
        use wows_toolkit_replay_data::force_reindex::VersionCount;

        use super::version_entries;
        use wowsunpack::data::Version;

        let offered: Vec<PaletteAction> = entries().into_iter().map(|entry| entry.action).collect();
        assert!(offered.contains(&PaletteAction::EnterMode(PaletteMode::ReindexVersions)));
        assert!(offered.contains(&PaletteAction::ForceReindex(ReindexScope::All)));

        let rows = [
            VersionCount { version: Version::base(14, 3, 0), replays: 4 },
            VersionCount { version: Version::base(14, 2, 1), replays: 1 },
            VersionCount { version: Version::base(13, 4, 0), replays: 7 },
        ];
        let chosen = |query: &str| -> Vec<Version> {
            version_entries(&rows, &FilterQuery::new(query))
                .into_iter()
                .map(|entry| match entry.action {
                    PaletteAction::ForceReindex(ReindexScope::Version(version)) => version,
                    other => panic!("a version row re-indexes that version, not {other:?}"),
                })
                .collect()
        };

        assert_eq!(chosen(""), [Version::base(14, 3, 0), Version::base(14, 2, 1), Version::base(13, 4, 0)]);
        assert_eq!(chosen("14.2"), [Version::base(14, 2, 1)]);
        // A "4" that starts a component ranks above one inside "14".
        assert_eq!(chosen("4"), [Version::base(13, 4, 0), Version::base(14, 3, 0), Version::base(14, 2, 1)]);
        assert!(chosen("15").is_empty());
    }

    /// A player row leads to the search that finds their battles, and a ship row
    /// to the reader's own battles in it, both as the shared seeds express them.
    #[tokio::test]
    async fn a_mode_row_leads_to_the_search_the_egui_palette_seeds() {
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::seed;

        let pool = wows_toolkit_config::test_pool().await;
        // Nothing indexed returns an empty result rather than a query error.
        for list in [IndexList::Players, IndexList::MyShips] {
            let lookup = list.load(&pool).await.expect("the empty index reads");
            assert!(mode_entries(&lookup, &FilterQuery::default()).is_empty(), "{list:?}");
        }

        // What a row would do, checked against the seed rather than the SQL: a
        // query the Search tab cannot parse back is a row that does nothing.
        let account = wows_replays::types::AccountId(7);
        let printed = query_text::print_query(&seed::matches_with_player(account));
        assert_eq!(
            query_text::parse_query(&printed).expect("the seeded query parses"),
            seed::matches_with_player(account)
        );
    }

    /// An indexed layer matches as every other list does: by letters in
    /// order and folded, ranked over every row before the list is capped.
    #[test]
    fn an_indexed_ship_is_found_by_its_letters() {
        use wows_toolkit_config::index::rows::ShipFacet;
        use wows_toolkit_viewmodel::name_lookup::ShipLookup;
        use wowsunpack::game_types::GameParamId;

        let ship =
            |id: u64, name: &str| ShipFacet { ship_id: GameParamId::from(id), ship_name: name.into(), match_count: 1 };
        let mut ships: Vec<ShipFacet> = (0..super::MODE_LIMIT as u64).map(|id| ship(id, "Filler")).collect();
        ships.extend([ship(100, "Yamato"), ship(101, "Sm\u{e5}land")]);
        let lookup = IndexLookup::Ships(ShipLookup::new(ships));
        let found = |typed: &str| -> Vec<String> {
            mode_entries(&lookup, &FilterQuery::new(typed)).into_iter().map(|entry| entry.label).collect()
        };

        assert_eq!(found("ymt"), [format!("Yamato -- {}", rust_i18n::t!("ui.palette.matches_found", count = 1))]);
        assert_eq!(
            found("smaland"),
            [format!("Sm\u{e5}land -- {}", rust_i18n::t!("ui.palette.matches_found", count = 1))]
        );
    }

    /// Both bulk contributions are offered: the ordinary pass over what has not
    /// been sent, and the one that sends it all again for a reader told the
    /// service lost it. The egui palette offers the same two.
    #[test]
    fn both_bulk_contributions_are_offered() {
        let offered: Vec<wows_toolkit_services::upload::LedgerUse> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::ContributeAllReplays(ledger) => Some(ledger),
                _ => None,
            })
            .collect();

        assert!(offered.contains(&wows_toolkit_services::upload::LedgerUse::Consult));
        assert!(offered.contains(&wows_toolkit_services::upload::LedgerUse::Ignore));
    }

    /// Labels are what the reader searches, so an empty one is unreachable.
    #[test]
    fn every_entry_is_named() {
        for entry in entries() {
            assert!(!entry.label.trim().is_empty(), "{:?} has no label", entry.action);
        }
    }

    /// The ship search folds diacritics, lists the name's own prefix ahead of a
    /// later word's, and finds a name by its words' first letters or with
    /// letters left out.
    #[test]
    fn ship_search_folds_and_ranks_names() {
        use super::ArmorShipRow;
        use super::armor_ship_entries;
        use wowsunpack::game_params::types::Species;

        let rows: Vec<ArmorShipRow> = ["Musashi", "Yamato", "Sm\u{e5}land", "Des Moines", "Moskva"]
            .into_iter()
            .map(|name| ArmorShipRow::new(name.to_lowercase(), name.to_owned(), 10, Species::Cruiser, "Nation"))
            .collect();
        let found = |query: &str| -> Vec<String> {
            armor_ship_entries(&rows, &FilterQuery::new(query))
                .into_iter()
                .map(|entry| match entry.action {
                    PaletteAction::ViewArmor { display_name, .. } => display_name,
                    other => panic!("a ship row opens a ship, not {other:?}"),
                })
                .collect()
        };

        assert_eq!(found("").len(), rows.len(), "an empty search lists every ship");
        assert_eq!(found("smaland"), ["Sm\u{e5}land"]);
        assert_eq!(found(" MO "), ["Moskva", "Des Moines"]);
        assert_eq!(found("dm"), ["Des Moines"]);
        assert_eq!(found("dsmns"), ["Des Moines"]);
        assert!(found("zzz").is_empty());
    }

    /// The root list folds its query and finds an entry by its words' first
    /// letters, best match first.
    #[test]
    fn the_root_list_matches_loosely_and_ranks() {
        use super::RootPalette;

        let mut root = RootPalette::new(entries());
        let shown =
            |root: &RootPalette| -> Vec<String> { root.shown().iter().map(|entry| entry.label.clone()).collect() };

        assert_eq!(shown(&root).len(), entries().len(), "nothing typed lists every entry");

        root.search("games won");
        assert_eq!(shown(&root), ["Games I won"]);

        root.search("search");
        let searches: Vec<String> =
            shown(&root).into_iter().filter(|label| label.to_lowercase().contains("search")).collect();
        assert_eq!(searches, ["Search players...", "Advanced search..."], "a label that starts with it ranks first");
    }

    /// The directory layer keeps "Browse..." first and ranks the recent
    /// directories by how their path matches, folding diacritics.
    #[test]
    fn the_directory_layer_keeps_browse_first_and_ranks() {
        use super::FolderRow;
        use super::folder_entries;

        let rows = [FolderRow::new("/r/Prism".into()), FolderRow::new("/r/Sm\u{e5}land games".into())];
        let found = |query: &str| -> Vec<PaletteAction> {
            folder_entries(&rows, &FilterQuery::new(query)).into_iter().map(|entry| entry.action).collect()
        };
        let open = |path: &str| PaletteAction::OpenReplayFolder(path.into());

        assert_eq!(found(""), [PaletteAction::BrowseReplayFolder, open("/r/Prism"), open("/r/Sm\u{e5}land games")]);
        assert_eq!(found("smaland"), [PaletteAction::BrowseReplayFolder, open("/r/Sm\u{e5}land games")]);
        // A word that starts with the query ranks above one that holds it.
        assert_eq!(found("sm"), [PaletteAction::BrowseReplayFolder, open("/r/Sm\u{e5}land games"), open("/r/Prism")]);
        assert_eq!(found("zzz"), [PaletteAction::BrowseReplayFolder]);
    }

    /// Draws `window`, lets deferred work and spawned focus run, then draws
    /// again.
    fn settle(cx: &mut gpui_kit::TestAppContext, window: gpui_kit::WindowHandle<gpui_kit::component::Root>) {
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    /// Opens a second layer over `source`, lets `fill` set what it lists,
    /// and draws it.
    fn draw_layer(
        cx: &mut gpui_kit::TestAppContext,
        source: super::ModeSource,
        fill: impl FnOnce(&mut super::ModeDialog, &mut gpui_kit::Window, &mut gpui_kit::Context<super::ModeDialog>)
        + 'static,
    ) {
        use gpui_kit::component::WindowExt as _;
        use gpui_kit::component::command::CommandState;

        let (window, app) = crate::interaction_tests::open_app_in_root(cx);
        cx.update_window(window.into(), |_, window, cx| {
            let command = cx.new(|cx| CommandState::new(window, cx));
            let layer = cx.new(|_| super::ModeDialog::new(command, app.downgrade(), PaletteMode::Players, source));
            layer.update(cx, |layer, cx| fill(layer, window, cx));
            window.open_dialog(cx, move |dialog, _window, _cx| {
                let layer = layer.clone();
                dialog.content(move |content, _window, _cx| content.child(layer.clone()))
            });
        })
        .expect("the test window stays open");
        crate::text_filter::take_marks();
        settle(cx, window);
    }

    /// A ship row marks the matched letters of the name, not the tier or
    /// nation text drawn after it.
    #[gpui_kit::test]
    fn a_ship_row_marks_the_matched_name(cx: &mut gpui_kit::TestAppContext) {
        use super::ArmorShipRow;
        use super::ModeSource;
        use super::ShipList;
        use crate::text_filter::marks_for;
        use wowsunpack::game_params::types::Species;

        let rows = vec![
            ArmorShipRow::new("PASB018".into(), "Montana".into(), 10, Species::Battleship, "U.S.A."),
            ArmorShipRow::new("PZSD109".into(), "Sm\u{e5}land".into(), 10, Species::Destroyer, "Europe"),
        ];
        let labels: Vec<String> = rows.iter().map(|row| row.label.clone()).collect();
        draw_layer(cx, ModeSource::Ships(ShipList::Ready(rows.into())), |layer, window, cx| {
            layer.search("smaland".into(), window, cx)
        });

        assert_eq!(marks_for(&labels[1]), Some(std::iter::once(0..8).collect()));
        assert_eq!(marks_for(&labels[0]), None, "a ship that does not match is not drawn");
    }

    /// A version row marks the version, not its replay count.
    #[gpui_kit::test]
    fn a_version_row_marks_the_version(cx: &mut gpui_kit::TestAppContext) {
        use wows_toolkit_replay_data::force_reindex::VersionCount;
        use wowsunpack::data::Version;

        use super::ModeSource;
        use crate::text_filter::marks_for;

        let rows = [
            VersionCount { version: Version::base(14, 3, 0), replays: 4 },
            VersionCount { version: Version::base(13, 4, 0), replays: 7 },
        ];
        draw_layer(cx, ModeSource::Versions(rows.into()), |layer, window, cx| layer.search("4".into(), window, cx));

        assert_eq!(marks_for("13.4.0 -- 7 replay(s)"), Some(std::iter::once(3..4).collect()));
        assert_eq!(marks_for("14.3.0 -- 4 replay(s)"), Some(std::iter::once(1..2).collect()));
    }

    /// A recent directory row marks both its name and its path.
    #[gpui_kit::test]
    fn a_directory_row_marks_its_name_and_path(cx: &mut gpui_kit::TestAppContext) {
        use super::FolderRow;
        use super::ModeSource;
        use crate::text_filter::marks_for;

        let rows = [FolderRow::new("/r/Sm\u{e5}land games".into())];
        draw_layer(cx, ModeSource::Folders(rows.into()), |layer, window, cx| {
            layer.search("smaland".into(), window, cx)
        });

        assert_eq!(marks_for("Sm\u{e5}land games"), Some(std::iter::once(0..8).collect()));
        assert_eq!(marks_for("/r/Sm\u{e5}land games"), Some(std::iter::once(3..11).collect()));
    }

    /// A row the replay index returned for a player is marked where the typed
    /// name matched it.
    #[gpui_kit::test]
    fn an_index_row_marks_the_typed_name(cx: &mut gpui_kit::TestAppContext) {
        use super::ModeSource;
        use super::PaletteEntry;
        use crate::text_filter::marks_for;

        draw_layer(cx, ModeSource::index(super::IndexList::Players), |layer, _window, _cx| {
            layer.entries = std::rc::Rc::new(vec![PaletteEntry {
                label: "[KLAN] J\u{fc}rgen -- 3 match(es)".to_owned(),
                action: PaletteAction::SearchFor("player:7".to_owned()),
            }]);
            layer.query = crate::text_filter::MarkedQuery::new("jurgen");
        });

        assert_eq!(marks_for("[KLAN] J\u{fc}rgen -- 3 match(es)"), Some(std::iter::once(7..14).collect()));
    }

    /// Typing in the root list filters it here, by word-start letters the
    /// component's own substring filter would miss, and marks the match.
    #[gpui_kit::test]
    fn the_root_list_marks_what_it_matched(cx: &mut gpui_kit::TestAppContext) {
        use crate::text_filter::marks_for;
        use gpui_kit::component::IndexPath;
        use gpui_kit::test::TestWindowExt as _;

        let (window, app) = crate::interaction_tests::open_app_in_root(cx);
        cx.update_window(window.into(), |_, window, cx| app.update(cx, |app, cx| app.open_palette(window, cx)))
            .expect("the test window stays open");
        settle(cx, window);
        cx.update_window(window.into(), |_, window, cx| window.input("games won", cx))
            .expect("the test window stays open");
        crate::text_filter::take_marks();
        settle(cx, window);

        cx.update_window(window.into(), |_, window, _cx| {
            assert_eq!(window.find(IndexPath::new(0)).label(), Some("Games I won"));
            assert!(window.try_find(IndexPath::new(1)).is_none(), "only the match is listed");
        })
        .expect("the test window stays open");
        assert_eq!(marks_for("Games I won"), Some(vec![0..5, 8..11]));
    }
}
