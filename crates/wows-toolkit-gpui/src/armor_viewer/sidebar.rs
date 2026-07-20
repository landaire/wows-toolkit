//! Ship sidebar tree: nation -> class -> ship, filtered by a search box.
//! Reuses the same gpui-component `tree`/`TreeState` the Replay Inspector's
//! file browser is built on (`replay_inspector::browser_view`), and its
//! `render_browser_item`-style pattern of a side id->metadata map consulted
//! by the row renderer.
//!
//! **Parity note.** The egui app sorts nations by their *translated* display
//! name and labels each nation row with that translation
//! (`armor_viewer/ui/tab.rs:330-337`, via a `IDS_{NATION}` lookup). This
//! port has no IDS_ translation lookup wired anywhere yet (see
//! `replay_inspector::browser_view`'s own documented convention for the same
//! gap), so nation rows show the raw catalog key and sort by it -- exactly
//! what `ShipCatalog::build` already produces (`catalog.rs`'s `nations.sort_by`).
//!
//! **Expand behavior.** Matches the egui app's `egui_ltreeview` usage
//! (`tab.rs:404-447`, `default_open(searching)`): every nation/class row is
//! collapsed by default and only auto-expands while a search query matches
//! something inside it; clearing the query collapses everything again. Rows
//! with no match anywhere inside them are omitted entirely, not merely
//! collapsed.
//!
//! **Compare.** The header's "Compare" button emits [`CompareSplit`];
//! `pane.rs` responds by adding a new (empty) pane to the dock and making it
//! active, so the next ship picked here loads into it rather than replacing
//! whatever the previously active pane was showing.
//!
//! **Export model (Milestone 5 Task 10).** Right-clicking a ship row shows
//! an "Export model" item (via `gpui_component::tree::Tree::context_menu`,
//! a first-class per-row hook the tree builder already exposes); picking it
//! emits [`ExportModelRequested`], which `pane.rs` handles by exporting that
//! ship at STOCK hull/default LOD -- independent of whatever any pane
//! currently displays, unlike the toolbar's own export button
//! (`viewport_view::ViewportView::confirm_export`), which exports a pane's
//! LIVE hull/LOD/module selection. Ports the egui app's sidebar-tree
//! "Export model" context-menu item (`tab.rs:472-486`), which likewise
//! always passes `selected_hull: None`.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::*;
use gpui_component::ActiveTheme;
use gpui_component::Icon;
use gpui_component::IconName;
use gpui_component::Sizable;
use gpui_component::button::Button;
use gpui_component::h_flex;
use gpui_component::input::Input;
use gpui_component::input::InputEvent;
use gpui_component::input::InputState;
use gpui_component::list::ListItem;
use gpui_component::menu::PopupMenuItem;
use gpui_component::tree::TreeEntry;
use gpui_component::tree::TreeItem;
use gpui_component::tree::TreeState;
use gpui_component::tree::tree;
use gpui_component::v_flex;

use wowsunpack::game_params::types::Species;

use super::assets::ArmorAssetsBundle;
use super::assets::CLASS_ICON_TINT;
use super::catalog::species_name;
use super::catalog::tier_roman;

/// Emitted when the user clicks a ship row: the sidebar's caller (`pane.rs`)
/// starts the ship's background armor load in response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipSelected {
    pub param_index: String,
    pub display_name: String,
}

/// Emitted when the user clicks the sidebar header's "Compare" button:
/// `pane.rs` responds by adding a new pane to the dock (Milestone 5 Task 9b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareSplit;

/// Emitted when the user picks "Export model" from a ship row's right-click
/// context menu (Milestone 5 Task 10): `pane.rs` responds by exporting this
/// ship at stock hull/default LOD, independent of any pane's own selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportModelRequested {
    pub param_index: String,
    pub display_name: String,
}

/// What a tree row id refers to, resolved once per `rebuild_tree` and
/// consulted by the row renderer (icon choice) and the click handler (ship
/// rows only fire [`ShipSelected`]).
enum RowKind {
    Nation(String),
    Class(Species),
    Ship { param_index: String, display_name: String },
}

pub struct Sidebar {
    bundle: Option<Arc<ArmorAssetsBundle>>,
    search_state: Entity<InputState>,
    search_text: String,
    tree_state: Entity<TreeState>,
    /// Rebuilt only by `rebuild_tree` (on bundle load / search change), never
    /// per render -- mirrors `ReplayBrowser::leaf_info`'s `Rc` pointer-bump
    /// clone rationale.
    row_info: Rc<HashMap<SharedString, RowKind>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ShipSelected> for Sidebar {}
impl EventEmitter<CompareSplit> for Sidebar {}
impl EventEmitter<ExportModelRequested> for Sidebar {}

impl Sidebar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let search_state = cx.new(|cx| InputState::new(window, cx).placeholder("Search ships..."));
        let subscription = cx.subscribe_in(&search_state, window, Self::on_search_event);

        Self {
            bundle: None,
            search_state,
            search_text: String::new(),
            tree_state,
            row_info: Rc::new(HashMap::new()),
            _subscriptions: vec![subscription],
        }
    }

    /// Adopts the loaded catalog/icons and rebuilds the tree. Called once the
    /// Armor Viewer's background asset load (`assets::spawn_load_armor_assets`)
    /// completes.
    pub fn set_bundle(&mut self, bundle: Arc<ArmorAssetsBundle>, cx: &mut Context<Self>) {
        self.bundle = Some(bundle);
        self.rebuild_tree(cx);
        cx.notify();
    }

    fn on_search_event(
        &mut self,
        _search_state: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputEvent::Change = event else { return };
        let text = self.search_state.read(cx).value().to_string();
        if text == self.search_text {
            return;
        }
        self.search_text = text;
        self.rebuild_tree(cx);
        cx.notify();
    }

    fn rebuild_tree(&mut self, cx: &mut Context<Self>) {
        let Some(bundle) = self.bundle.clone() else {
            self.row_info = Rc::new(HashMap::new());
            self.tree_state.update(cx, |state, cx| state.set_items(Vec::new(), cx));
            return;
        };

        let search = unidecode::unidecode(&self.search_text).to_lowercase();
        let searching = !search.is_empty();

        let mut row_info: HashMap<SharedString, RowKind> = HashMap::new();
        let mut items: Vec<TreeItem> = Vec::new();

        for nation in &bundle.catalog.nations {
            let nation_has_match =
                !searching || nation.classes.iter().any(|c| c.ships.iter().any(|s| s.search_name.contains(&search)));
            if searching && !nation_has_match {
                continue;
            }

            let nation_id: SharedString = format!("armor-nation-{}", nation.nation).into();
            row_info.insert(nation_id.clone(), RowKind::Nation(nation.nation.clone()));

            let mut class_items: Vec<TreeItem> = Vec::new();
            for class in &nation.classes {
                let class_has_match = !searching || class.ships.iter().any(|s| s.search_name.contains(&search));
                if searching && !class_has_match {
                    continue;
                }

                let class_id: SharedString = format!("armor-class-{}-{:?}", nation.nation, class.species).into();
                row_info.insert(class_id.clone(), RowKind::Class(class.species));

                let mut ship_items: Vec<TreeItem> = Vec::new();
                for ship in &class.ships {
                    if searching && !ship.search_name.contains(&search) {
                        continue;
                    }
                    let ship_id: SharedString = format!("armor-ship-{}", ship.param_index).into();
                    row_info.insert(
                        ship_id.clone(),
                        RowKind::Ship {
                            param_index: ship.param_index.clone(),
                            display_name: ship.display_name.clone(),
                        },
                    );
                    let label = format!("{} {}", tier_roman(ship.tier), ship.display_name);
                    ship_items.push(TreeItem::new(ship_id, label));
                }

                class_items.push(
                    TreeItem::new(class_id, species_name(&class.species))
                        .children(ship_items)
                        .expanded(searching && class_has_match),
                );
            }

            items.push(
                TreeItem::new(nation_id, nation.nation.clone())
                    .children(class_items)
                    .expanded(searching && nation_has_match),
            );
        }

        self.row_info = Rc::new(row_info);
        self.tree_state.update(cx, |state, cx| state.set_items(items, cx));
    }
}

/// Renders one sidebar row: an expand chevron (folders) or a spacer (ships),
/// a nation-flag/class icon when one resolved, and the label. Ships attach a
/// click handler that emits [`ShipSelected`]; nation/class rows rely on
/// `TreeState`'s own click-to-expand behavior. Mirrors
/// `replay_inspector::browser_view::render_browser_item`'s structure.
fn render_sidebar_item(
    sidebar: Entity<Sidebar>,
    ix: usize,
    entry: &TreeEntry,
    selected: bool,
    bundle: &ArmorAssetsBundle,
    row_info: &HashMap<SharedString, RowKind>,
) -> ListItem {
    let item = entry.item();
    let kind = row_info.get(&item.id);

    let mut row = h_flex().gap_1().items_center().pl(px(16.) * entry.depth());
    if entry.is_folder() {
        let chevron = if entry.is_expanded() { IconName::ChevronDown } else { IconName::ChevronRight };
        row = row.child(Icon::new(chevron));
    } else {
        row = row.child(div().w(px(16.)));
    }

    match kind {
        Some(RowKind::Nation(nation)) => {
            if let Some(image) = bundle.icons.get_keyed(&format!("nation:{nation}")) {
                row = row.child(div().flex_none().child(img(image).w(px(23.)).h(px(16.))));
            }
        }
        Some(RowKind::Class(species)) => {
            if let Some(image) = bundle.icons.get(*species, CLASS_ICON_TINT) {
                row = row.child(div().flex_none().child(img(image).w(px(16.)).h(px(16.))));
            }
        }
        Some(RowKind::Ship { .. }) | None => {}
    }

    row = row.child(div().flex_1().overflow_hidden().text_ellipsis().whitespace_nowrap().child(item.label.clone()));

    let mut list_item = ListItem::new(ix).selected(selected).child(row);

    if let Some(RowKind::Ship { param_index, display_name }) = kind {
        let param_index = param_index.clone();
        let display_name = display_name.clone();
        list_item = list_item.on_click(move |_event: &ClickEvent, _window, cx: &mut App| {
            let selected = ShipSelected { param_index: param_index.clone(), display_name: display_name.clone() };
            sidebar.update(cx, |_sidebar, cx| cx.emit(selected));
        });
    }

    list_item
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;

        let header = h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(div().flex_1().text_sm().font_weight(FontWeight::BOLD).child("Ships"))
            .child(
                Button::new("armor-sidebar-compare")
                    .icon(IconName::LayoutDashboard)
                    .label("Compare")
                    .compact()
                    .tooltip("Open a new comparison pane")
                    .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(CompareSplit))),
            );

        let search_row = h_flex()
            .flex_none()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(Icon::new(IconName::Search))
            .child(Input::new(&self.search_state).small().w_full());

        let body: AnyElement = match &self.bundle {
            None => div().p_2().text_sm().opacity(0.6).child("Loading ship catalog...").into_any_element(),
            Some(bundle) => {
                let entity = cx.entity();
                let bundle = bundle.clone();
                let row_info = self.row_info.clone();
                let context_menu_entity = entity.clone();
                let context_menu_row_info = self.row_info.clone();
                tree(&self.tree_state, move |ix, entry, selected, _window, _cx| {
                    render_sidebar_item(entity.clone(), ix, entry, selected, &bundle, &row_info)
                })
                .context_menu(move |_ix, entry, menu, _window, _cx| {
                    let Some(RowKind::Ship { param_index, display_name }) = context_menu_row_info.get(&entry.item().id)
                    else {
                        return menu;
                    };
                    let param_index = param_index.clone();
                    let display_name = display_name.clone();
                    let entity = context_menu_entity.clone();
                    menu.item(PopupMenuItem::new("Export model").on_click(move |_event, _window, cx| {
                        let request = ExportModelRequested {
                            param_index: param_index.clone(),
                            display_name: display_name.clone(),
                        };
                        entity.update(cx, |_sidebar, cx| cx.emit(request));
                    }))
                })
                .flex_1()
                .into_any_element()
            }
        };

        v_flex().size_full().child(header).child(search_row).child(div().flex_1().min_h(px(0.)).child(body))
    }
}
