//! The command palette: everything the window can do, by name.
//!
//! Ports the egui app's `ui/command_palette.rs`, which opens on ctrl+k or
//! ctrl+p. The root list is built once and `Command` filters it. Some root
//! entries open a second layer (`ModeDialog`) that queries the replay index or
//! filters the ship catalogue as the reader types; Escape, or Backspace in an
//! empty field, returns to the root.

use std::rc::Rc;

use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::KeyDownEvent;
use gpui_kit::ParentElement as _;
use gpui_kit::Render;
use gpui_kit::Styled as _;
use gpui_kit::Subscription;
use gpui_kit::Task;
use gpui_kit::WeakEntity;
use gpui_kit::Window;
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
use gpui_kit::px;
use rust_i18n::t;
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
use wows_toolkit_viewmodel::settings::ThemeChoice;

/// What confirming an entry asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    GoTo(AppTab),
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
    /// List a directory of replays other than the install's.
    OpenReplayDirectory,
    /// Read every replay that is not indexed yet.
    IndexAllReplays,
    /// Read every replay again and rewrite what the index holds, once the reader
    /// has confirmed it.
    RefreshPersistedData,
    /// Contribute every listed battle, as the data-sharing setting asks.
    ContributeAllReplays(crate::upload::LedgerUse),
    /// Take a result mapping the reader points at as the loaded build's.
    ImportConstants,
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
}

impl PaletteMode {
    pub const ALL: [PaletteMode; 3] = [Self::Players, Self::MyShips, Self::ArmorShips];

    /// What the root entry that enters this mode is called.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.search_players",
            Self::MyShips => "ui.palette.my_matches_in_ship",
            Self::ArmorShips => "ui.palette.open_ship_in_armor_viewer",
        }
    }

    /// What the layer is called in its header, after the root's name.
    pub const fn title_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_title",
            Self::MyShips => "ui.palette.my_ships_title",
            Self::ArmorShips => "ui.palette.armor_ships_title",
        }
    }

    pub const fn placeholder_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_placeholder",
            Self::MyShips => "ui.palette.my_ships_placeholder",
            Self::ArmorShips => "ui.palette.armor_ships_placeholder",
        }
    }
}

/// Maximum rows returned by one cascading index query.
pub const MODE_LIMIT: i64 = 50;

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
    /// The name as `stats::normalize_ship_search` folds it.
    search_name: String,
    /// The name with tier, class and nation, which is what the row shows.
    label: String,
}

impl ArmorShipRow {
    pub fn new(param_index: String, display_name: String, tier: u32, species: Species, nation: &str) -> Self {
        let search_name = crate::stats::normalize_ship_search(&display_name);
        let label = t!(
            "ui.palette.armor_ship_row",
            name = display_name,
            tier = tier_roman(tier),
            class = species_name(&species),
            nation = nation
        )
        .into_owned();
        Self { param_index, display_name, search_name, label }
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

/// How closely a folded ship name matches the folded query. Ordered best
/// first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ShipMatch {
    Exact,
    Prefix,
    /// The query starts a later word of the name ("moines" in "des moines").
    WordPrefix,
    Contains,
    /// The query's letters appear in order ("dsmns" in "des moines").
    Subsequence,
}

fn ship_match(name: &str, needle: &str) -> Option<ShipMatch> {
    if name == needle {
        return Some(ShipMatch::Exact);
    }
    if name.starts_with(needle) {
        return Some(ShipMatch::Prefix);
    }
    let mut found = name.match_indices(needle).peekable();
    if found.peek().is_some() {
        let word_start =
            found.any(|(at, _)| name[..at].chars().next_back().is_some_and(|before| !before.is_alphanumeric()));
        return Some(if word_start { ShipMatch::WordPrefix } else { ShipMatch::Contains });
    }
    let mut rest = name.chars();
    let subsequence =
        needle.chars().filter(|c| !c.is_whitespace()).all(|wanted| rest.by_ref().any(|have| have == wanted));
    subsequence.then_some(ShipMatch::Subsequence)
}

/// The ship layer's rows for `query`: every ship when it is empty, else the
/// matches, best first.
pub fn armor_ship_entries(rows: &[ArmorShipRow], query: &str) -> Vec<PaletteEntry> {
    let needle = crate::stats::normalize_ship_search(query);
    let mut matched: Vec<(ShipMatch, &ArmorShipRow)> = if needle.is_empty() {
        rows.iter().map(|row| (ShipMatch::Exact, row)).collect()
    } else {
        rows.iter().filter_map(|row| ship_match(&row.search_name, &needle).map(|rank| (rank, row))).collect()
    };
    // Stable, so equal matches keep the catalogue's order.
    matched.sort_by_key(|(rank, _)| *rank);
    matched
        .into_iter()
        .map(|(_, row)| PaletteEntry {
            label: row.label.clone(),
            action: PaletteAction::ViewArmor {
                param_index: row.param_index.clone(),
                display_name: row.display_name.clone(),
            },
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
    /// The replay index, queried as the reader types.
    Index,
    /// The ship catalogue, filtered in memory.
    Ships(ShipList),
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
    error: Option<String>,
    generation: u64,
    _query_task: Option<Task<()>>,
    /// Held while the ship catalogue is still loading.
    _catalog_watch: Vec<Subscription>,
}

impl ModeDialog {
    pub fn new(command: Entity<CommandState>, owner: WeakEntity<App>, mode: PaletteMode, source: ModeSource) -> Self {
        Self {
            command,
            owner,
            mode,
            source,
            entries: Rc::default(),
            error: None,
            generation: 0,
            _query_task: None,
            _catalog_watch: Vec::new(),
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

        match &self.source {
            ModeSource::Ships(ShipList::Ready(rows)) => {
                self.entries = Rc::new(armor_ship_entries(rows, &needle));
                cx.notify();
                return;
            }
            ModeSource::Ships(ShipList::Pending | ShipList::Unavailable(_)) => {
                self.entries = Rc::default();
                cx.notify();
                return;
            }
            ModeSource::Index => {}
        }

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
        let mode = self.mode;

        self._query_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
            let current = this.update(cx, |this, _cx| this.generation == generation).unwrap_or(false);
            if !current {
                return;
            }

            let read =
                cx.background_spawn(async move { runtime.handle().block_on(mode_entries(mode, &pool, &needle)) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                match read {
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
            .filterable(false)
            .placeholder(t!(self.mode.placeholder_key()).to_string())
            .max_h(px(400.))
            .items(items(&entries))
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
                    let message = dialog.read(cx).empty_message();
                    div().p_4().text_sm().text_color(crate::theme::text_dim()).child(message)
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
        label: t!("ui.replay.open_directory").into_owned(),
        action: PaletteAction::OpenReplayDirectory,
    });
    entries.push(PaletteEntry {
        label: t!("ui.settings.replay.index_all_replays").into_owned(),
        action: PaletteAction::IndexAllReplays,
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
        action: PaletteAction::ContributeAllReplays(crate::upload::LedgerUse::Consult),
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.send_all_replays_again").into_owned(),
        action: PaletteAction::ContributeAllReplays(crate::upload::LedgerUse::Ignore),
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

/// The entries as the component's own items, in the same order, so an index
/// into one is an index into the other.
pub fn items(entries: &[PaletteEntry]) -> Vec<CommandItem> {
    entries.iter().map(|entry| CommandItem::new().label(entry.label.clone())).collect()
}

/// Rows one index-backed mode offers for the current query.
///
/// An empty result is distinct from a query failure so the palette can report
/// each one correctly.
pub async fn mode_entries(
    mode: PaletteMode,
    pool: &sqlx::sqlite::SqlitePool,
    needle: &str,
) -> Result<Vec<PaletteEntry>, String> {
    use wows_toolkit_config::index::query;
    use wows_toolkit_viewmodel::query_bar::seed;

    match mode {
        PaletteMode::Players => query::search_players(pool, needle, MODE_LIMIT)
            .await
            .map(|found| {
                found
                    .into_iter()
                    .map(|player| {
                        let named = match player.clan.as_str() {
                            "" => player.latest_name.clone(),
                            clan => format!("[{clan}] {}", player.latest_name),
                        };
                        PaletteEntry {
                            label: format!("{named} -- {}", t!("ui.palette.matches_found", count = player.match_count)),
                            action: PaletteAction::SearchFor(wows_toolkit_config::index::query_text::print_query(
                                &seed::matches_with_player(player.account_id),
                            )),
                        }
                    })
                    .collect()
            })
            .map_err(|err| format!("The indexed players could not be read: {err}")),
        PaletteMode::MyShips => query::search_self_ships(pool, needle, MODE_LIMIT)
            .await
            .map(|found| {
                found
                    .into_iter()
                    .map(|ship| PaletteEntry {
                        label: format!(
                            "{} -- {}",
                            ship.ship_name,
                            t!("ui.palette.matches_found", count = ship.match_count)
                        ),
                        action: PaletteAction::SearchFor(wows_toolkit_config::index::query_text::print_query(
                            &seed::my_matches_in_ship(ship.ship_id),
                        )),
                    })
                    .collect()
            })
            .map_err(|err| format!("The indexed ships could not be read: {err}")),
        // Read from the loaded build rather than the index: armor is a property of
        // the ship, not of anything the reader has played.
        PaletteMode::ArmorShips => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::PaletteAction;
    use super::PaletteMode;
    use super::entries;
    use super::mode_entries;
    use crate::app::AppTab;

    /// Every tab is reachable by name: the palette is the keyboard route to
    /// the tab strip, so a tab missing from it cannot be reached that way.
    #[test]
    fn every_tab_is_offered() {
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

    /// A player row leads to the search that finds their battles, and a ship row
    /// to the reader's own battles in it, both as the shared seeds express them.
    #[tokio::test]
    async fn a_mode_row_leads_to_the_search_the_egui_palette_seeds() {
        use wows_toolkit_config::index::query_text;
        use wows_toolkit_viewmodel::query_bar::seed;

        let pool = wows_toolkit_config::test_pool().await;
        // Nothing indexed returns an empty result rather than a query error.
        assert!(mode_entries(PaletteMode::Players, &pool, "").await.expect("query players").is_empty());
        assert!(mode_entries(PaletteMode::MyShips, &pool, "").await.expect("query ships").is_empty());

        // What a row would do, checked against the seed rather than the SQL: a
        // query the Search tab cannot parse back is a row that does nothing.
        let account = wows_replays::types::AccountId(7);
        let printed = query_text::print_query(&seed::matches_with_player(account));
        assert_eq!(
            query_text::parse_query(&printed).expect("the seeded query parses"),
            seed::matches_with_player(account)
        );
    }

    /// Both bulk contributions are offered: the ordinary pass over what has not
    /// been sent, and the one that sends it all again for a reader told the
    /// service lost it. The egui palette offers the same two.
    #[test]
    fn both_bulk_contributions_are_offered() {
        let offered: Vec<crate::upload::LedgerUse> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::ContributeAllReplays(ledger) => Some(ledger),
                _ => None,
            })
            .collect();

        assert!(offered.contains(&crate::upload::LedgerUse::Consult));
        assert!(offered.contains(&crate::upload::LedgerUse::Ignore));
    }

    /// Labels are what the reader searches, so an empty one is unreachable.
    #[test]
    fn every_entry_is_named() {
        for entry in entries() {
            assert!(!entry.label.trim().is_empty(), "{:?} has no label", entry.action);
        }
    }

    /// The ship search folds diacritics, lists the name's own prefix ahead of a
    /// later word's, and still finds a name typed with letters left out.
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
            armor_ship_entries(&rows, query)
                .into_iter()
                .map(|entry| match entry.action {
                    PaletteAction::ViewArmor { display_name, .. } => display_name,
                    other => panic!("a ship row opens a ship, not {other:?}"),
                })
                .collect()
        };

        assert_eq!(found("").len(), rows.len(), "an empty search lists every ship");
        assert_eq!(found("smal"), ["Sm\u{e5}land"]);
        assert_eq!(found(" MO "), ["Moskva", "Des Moines", "Yamato"]);
        assert_eq!(found("dsmns"), ["Des Moines"]);
        assert!(found("zzz").is_empty());
    }
}
