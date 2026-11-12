//! Model Viewer sidebar: one tree with the ships (nation > class > ship, each
//! ship listing its parts), a "Models" heading, then every catalogue model
//! (nation > kind > sub-kind > model, each model listing its variants).
//!
//! Built on the same gpui-component `tree` the Replay Inspector's file browser
//! uses, with a side id-to-row map the row renderer consults. Folders are
//! collapsed until opened; a search drops the folders holding no match and
//! opens the rest when it names few enough rows. The tree is rebuilt from
//! [`sidebar_tree::build_tree`] on every search, expansion or data change,
//! which only materializes the expanded folders.
//!
//! A ship row's context menu offers "Compare (split view)" ([`CompareSplit`])
//! and "Export ship model" ([`ExportModelRequested`]); a model or part row's offers
//! "Open", "Export model" ([`ExportCatalogModelRequested`]) and "Copy model
//! path".

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::slider::SliderEvent;
use gpui_kit::component::slider::SliderState;
use gpui_kit::component::slider::SliderValue;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::tree::TreeEntry;
use gpui_kit::component::tree::TreeEvent;
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::tree::tree;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use rust_i18n::t;

use wows_toolkit_viewmodel::settings::keys;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::export::model_library::ModelLibrary;

use super::assets::ArmorAssetsBundle;
use super::assets::CLASS_ICON_TINT;
use super::catalog::TierRange;
use super::catalog::tier_roman;
use super::model_refs::HardpointTarget;
use super::model_refs::OpenTarget;
use super::model_refs::ShipPartRef;
use super::model_refs::VehicleParamIndex;
use super::model_tree::ModelReferences;
use super::model_tree::ModelTreeIndex;
use super::model_tree::ShipPartRows;
use super::model_tree::SkinUse;
use super::model_tree::list_all_ship_parts;
use super::search_key::fold;
use super::sidebar_tree::Expansion;
use super::sidebar_tree::ModelSection;
use super::sidebar_tree::RowKind;
use super::sidebar_tree::SearchMatches;
use super::sidebar_tree::TreeSources;
use super::sidebar_tree::build_tree;
use super::sidebar_tree::open_row_id;
use super::viewport_view::OpeningLayer;

/// Emitted when the user clicks a ship row: the sidebar's caller (`pane.rs`)
/// starts the ship's background armor load in response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipSelected {
    pub param_index: String,
    pub display_name: String,
}

/// What the comparison panes share, and whether there is more than one pane
/// to share it between.
///
/// Held here so the controls can sit beside the button that opens a pane;
/// `ArmorViewerPane` owns the values and pushes them in ([`Sidebar::
/// set_common`]), and a change made here comes back out as this same struct.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommonPaneSettings {
    pub comparing: bool,
    pub mirror_cameras: bool,
    pub sync_options: bool,
    /// Whether the panes are stacked rather than side by side.
    pub stack_panes: bool,
    /// What a newly opened model shows first. Not a comparison setting.
    pub opening_layer: OpeningLayer,
}

/// Asks for a new comparison pane.
///
/// `ship` names the one to open in it, which is how the ship row's own
/// "Compare (split view)" item works; the header button leaves it `None`,
/// which opens a copy of the pane that is already showing.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CompareSplit {
    pub ship: Option<ExportModelRequested>,
}

/// Emitted when the user picks "Export ship model" from a ship row's right-click
/// context menu: `pane.rs` responds by exporting this ship at stock
/// hull/default LOD, independent of any pane's own selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportModelRequested {
    pub param_index: String,
    pub display_name: String,
}

/// Emitted when the user clicks a catalogue model or a ship part row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSelected {
    pub model: ModelPath,
    pub from_ship: Option<ShipPartRef>,
}

/// Emitted when the user ticks or clears a row's hardpoint checkbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardpointsToggled {
    pub target: HardpointTarget,
    pub visible: bool,
}

/// Emitted when the user picks "Export model" on a catalogue model or ship
/// part row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportCatalogModelRequested {
    pub model: ModelPath,
    pub from_ship: Option<ShipPartRef>,
}

/// What one level of the tree indents by, and the width its guide is drawn in.
const INDENT: Pixels = px(16.);

/// The pane-sharing menu's box.
const COMMON_MENU_WIDTH: Pixels = px(220.);

/// Where the model catalogue listing stands.
enum ModelsState {
    Loading,
    Unavailable,
    Ready(Arc<ModelTreeIndex>),
}

/// What a tree rebuild keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rebuild {
    /// The search or the listed ships or models changed: match again and
    /// open the folders holding matches.
    Rematch,
    /// Keep the matches and the folders the user opened or closed.
    Keep,
}

pub struct Sidebar {
    bundle: Option<Arc<ArmorAssetsBundle>>,
    search_state: Entity<InputState>,
    /// The search as applied to the tree: ASCII-folded and lowercased.
    query: String,
    /// `query` matched against the listing; `None` when not searching.
    search: Option<SearchMatches>,
    tree_state: Entity<TreeState>,
    /// Rebuilt with the tree, never per render.
    row_info: Rc<HashMap<SharedString, RowKind>>,
    /// The field each row was found by, drawn after its name.
    row_notes: Rc<HashMap<SharedString, SharedString>>,
    /// What the comparison panes share. Owned by `ArmorViewerPane`; kept here
    /// so the header can draw it beside the button that opens a pane.
    common: CommonPaneSettings,

    model_library: Option<Arc<ModelLibrary>>,
    models: ModelsState,
    ship_parts: Arc<HashMap<VehicleParamIndex, ShipPartRows>>,

    /// Folders open while not searching. Kept across searches.
    browse_expanded: HashSet<SharedString>,
    /// Folders open during the current search.
    search_expanded: HashSet<SharedString>,
    /// Folders the user closed during the current search, which a later
    /// match leaves closed.
    search_collapsed: HashSet<SharedString>,

    /// Targets whose hardpoints the pane shows.
    hardpoint_visibility: Rc<HashSet<HardpointTarget>>,
    /// What the active pane shows, highlighted in the tree.
    open_model: Option<OpenTarget>,

    /// The tiers the Ships section lists, and the slider choosing them.
    tiers: TierRange,
    tier_slider: Entity<SliderState>,
    /// The saved tiers are read once, with the first bundle: the config
    /// database is not open yet when the sidebar is made.
    tiers_restored: bool,

    _search_debounce: Option<Task<()>>,
    _model_index_task: Option<Task<()>>,
    _tiers_restore: Option<Task<()>>,
    _tier_slider_subscription: Subscription,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CommonPaneSettings> for Sidebar {}
impl EventEmitter<ShipSelected> for Sidebar {}
impl EventEmitter<CompareSplit> for Sidebar {}
impl EventEmitter<ExportModelRequested> for Sidebar {}
impl EventEmitter<ModelSelected> for Sidebar {}
impl EventEmitter<HardpointsToggled> for Sidebar {}
impl EventEmitter<ExportCatalogModelRequested> for Sidebar {}

impl Sidebar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let search_state = cx.new(|cx| InputState::new(window, cx).placeholder(t!("ui.models.search").to_string()));
        let search_subscription = cx.subscribe_in(&search_state, window, Self::on_search_event);
        let tree_subscription = cx.subscribe(&tree_state, Self::on_tree_event);
        let (tier_slider, tier_slider_subscription) = tier_slider(TierRange::ALL, cx);

        Self {
            bundle: None,
            search_state,
            query: String::new(),
            search: None,
            tree_state,
            row_info: Rc::new(HashMap::new()),
            row_notes: Rc::new(HashMap::new()),
            common: CommonPaneSettings::default(),
            model_library: None,
            models: ModelsState::Loading,
            ship_parts: Arc::new(HashMap::new()),
            browse_expanded: HashSet::new(),
            search_expanded: HashSet::new(),
            search_collapsed: HashSet::new(),
            hardpoint_visibility: Rc::new(HashSet::new()),
            open_model: None,
            tiers: TierRange::ALL,
            tier_slider,
            tiers_restored: false,
            _search_debounce: None,
            _model_index_task: None,
            _tiers_restore: None,
            _tier_slider_subscription: tier_slider_subscription,
            _subscriptions: vec![search_subscription, tree_subscription],
        }
    }

    /// The header's pane-sharing menu: what every comparison pane follows.
    ///
    /// Always in the header, whether or not a comparison is open, so opening
    /// one does not move the controls around it; the items are refused while
    /// there is nothing to share with, which is what says they are for
    /// comparing.
    fn common_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let common = self.common;
        let tiers = self.tiers;
        let tier_slider = self.tier_slider.clone();
        let entity = cx.entity();
        Popover::new("armor-sidebar-common")
            .trigger(
                Button::new("armor-sidebar-common-trigger")
                    .icon(IconName::Settings2)
                    .compact()
                    .tooltip(t!("ui.armor.common_settings").to_string()),
            )
            .content(move |_state, _window, cx| {
                let mirror_entity = entity.clone();
                let sync_entity = entity.clone();
                let stack_entity = entity.clone();
                let layer_entity = entity.clone();
                v_flex()
                    .w(COMMON_MENU_WIDTH)
                    .gap_2()
                    .p_2()
                    .child(
                        Checkbox::new("armor-pane-show-armor-by-default")
                            .label(t!("ui.armor.show_armor_by_default").to_string())
                            .checked(common.opening_layer.shows_armor())
                            .tooltip(t!("ui.armor.show_armor_by_default_tooltip").to_string())
                            .on_click(move |checked, _window, cx| {
                                let opening_layer = OpeningLayer::from_show_armor(*checked);
                                layer_entity.update(cx, |sidebar, cx| {
                                    cx.emit(CommonPaneSettings { opening_layer, ..sidebar.common })
                                });
                            }),
                    )
                    .child(
                        Checkbox::new("armor-pane-mirror-cameras")
                            .label(t!("ui.armor.mirror_cameras").to_string())
                            .checked(common.mirror_cameras)
                            .disabled(!common.comparing)
                            .tooltip(t!("ui.armor.mirror_cameras_tooltip").to_string())
                            .on_click(move |checked, _window, cx| {
                                let mirror_cameras = *checked;
                                mirror_entity.update(cx, |sidebar, cx| {
                                    cx.emit(CommonPaneSettings { mirror_cameras, ..sidebar.common })
                                });
                            }),
                    )
                    .child(
                        Checkbox::new("armor-pane-stack")
                            .label(t!("ui.armor.stack_panes").to_string())
                            .checked(common.stack_panes)
                            .disabled(!common.comparing)
                            .tooltip(t!("ui.armor.stack_panes_tooltip").to_string())
                            .on_click(move |checked, _window, cx| {
                                let stack_panes = *checked;
                                stack_entity.update(cx, |sidebar, cx| {
                                    cx.emit(CommonPaneSettings { stack_panes, ..sidebar.common })
                                });
                            }),
                    )
                    .child(
                        Checkbox::new("armor-pane-sync-options")
                            .label(t!("ui.armor.sync_options").to_string())
                            .checked(common.sync_options)
                            .disabled(!common.comparing)
                            .tooltip(t!("ui.armor.sync_tooltip").to_string())
                            .on_click(move |checked, _window, cx| {
                                let sync_options = *checked;
                                sync_entity.update(cx, |sidebar, cx| {
                                    cx.emit(CommonPaneSettings { sync_options, ..sidebar.common })
                                });
                            }),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .pt_2()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                div().text_sm().child(
                                    t!(
                                        "ui.armor.tier_filter",
                                        lowest = tier_roman(tiers.lowest()),
                                        highest = tier_roman(tiers.highest())
                                    )
                                    .into_owned(),
                                ),
                            )
                            .child(Slider::new(&tier_slider)),
                    )
                    .into_any_element()
            })
    }

    /// Adopts what the panes currently share, so the header's own controls
    /// report it.
    pub fn set_common(&mut self, common: CommonPaneSettings, cx: &mut Context<Self>) {
        if self.common == common {
            return;
        }
        self.common = common;
        cx.notify();
    }

    /// Adopts the loaded ship catalog and icons. Called once the background
    /// asset load (`assets::spawn_load_armor_assets`) completes. Any model
    /// library belongs to the previous bundle, so the Models section waits
    /// for the one built over this bundle.
    pub fn set_bundle(&mut self, bundle: Arc<ArmorAssetsBundle>, cx: &mut Context<Self>) {
        self.bundle = Some(bundle);
        if !std::mem::replace(&mut self.tiers_restored, true) {
            self.restore_tiers(cx);
        }
        self.model_library = None;
        self.models = ModelsState::Loading;
        self.ship_parts = Arc::new(HashMap::new());
        self._model_index_task = None;
        self.rebuild_tree(Rebuild::Rematch, cx);
        cx.notify();
    }

    /// Reads back the tiers saved by an earlier session.
    fn restore_tiers(&mut self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else {
            return;
        };

        self._tiers_restore = Some(cx.spawn(async move |this, cx| {
            let stored = crate::runtime::spawn(cx, async move {
                wows_toolkit_config::queries::get_setting::<TierRange>(&pool, keys::MODEL_VIEWER_SHIP_TIERS).await
            })
            .await;
            let Ok(Some(tiers)) = stored else {
                return;
            };

            let _ = this.update(cx, |this, cx| this.adopt_tiers(tiers, cx));
        }));
    }

    /// Shows `tiers` on the slider and lists only their ships.
    fn adopt_tiers(&mut self, tiers: TierRange, cx: &mut Context<Self>) {
        let (slider, subscription) = tier_slider(tiers, cx);
        self.tier_slider = slider;
        self._tier_slider_subscription = subscription;
        self.set_tiers(tiers, cx);
    }

    fn on_tier_slider(&mut self, _slider: Entity<SliderState>, event: &SliderEvent, cx: &mut Context<Self>) {
        // The reader's choice wins over a saved one still being read.
        self._tiers_restore = None;

        match event {
            SliderEvent::Change(value) => self.set_tiers(tiers_from_slider(*value), cx),
            // Saved once the handle is let go, not on every step of a drag.
            SliderEvent::Release(value) => {
                let tiers = tiers_from_slider(*value);
                self.set_tiers(tiers, cx);
                crate::settings_store::save(keys::MODEL_VIEWER_SHIP_TIERS, &tiers, cx);
            }
        }
    }

    fn set_tiers(&mut self, tiers: TierRange, cx: &mut Context<Self>) {
        if self.tiers == tiers {
            return;
        }

        self.tiers = tiers;
        self.rebuild_tree(Rebuild::Rematch, cx);
        cx.notify();
    }

    /// Adopts the model library whose catalogue fills the Models section. The
    /// listing is grouped on the background executor.
    pub fn set_model_library(&mut self, library: Arc<ModelLibrary>, cx: &mut Context<Self>) {
        self.model_library = Some(library);
        self.start_model_index(cx);
    }

    /// Marks the Models section unavailable: the library could not be built.
    pub fn set_model_library_failed(&mut self, cx: &mut Context<Self>) {
        self.models = ModelsState::Unavailable;
        self.rebuild_tree(Rebuild::Rematch, cx);
        cx.notify();
    }

    /// Adopts the targets whose hardpoints the pane shows.
    pub fn set_hardpoint_visibility(&mut self, visible: &HashSet<HardpointTarget>, cx: &mut Context<Self>) {
        if *self.hardpoint_visibility == *visible {
            return;
        }
        self.hardpoint_visibility = Rc::new(visible.clone());
        cx.notify();
    }

    /// Adopts what the active pane shows, so its row is highlighted.
    pub fn set_open_model(&mut self, open: Option<OpenTarget>, cx: &mut Context<Self>) {
        if self.open_model == open {
            return;
        }
        self.open_model = open;
        self.highlight_open_row(cx);
        cx.notify();
    }

    fn start_model_index(&mut self, cx: &mut Context<Self>) {
        let (Some(bundle), Some(library)) = (self.bundle.clone(), self.model_library.clone()) else {
            return;
        };

        // The index is listed as soon as it is ready; the parts take longer.
        self._model_index_task = Some(cx.spawn(async move |this, cx| {
            let references_bundle = Arc::clone(&bundle);
            let references = cx
                .background_spawn(async move {
                    Arc::new(ModelReferences::build(&references_bundle.catalog, references_bundle.assets.metadata()))
                })
                .await;

            let (index_bundle, index_library, index_references) =
                (Arc::clone(&bundle), Arc::clone(&library), Arc::clone(&references));
            let index = cx.background_spawn(async move {
                let metadata = index_bundle.assets.metadata();
                ModelTreeIndex::build(index_library.catalog(), &index_bundle.catalog, &index_references, metadata)
            });
            let parts = cx.background_spawn(async move {
                list_all_ship_parts(&bundle.catalog, bundle.assets.metadata(), &library, &references)
            });

            let index = index.await;
            // An error is the sidebar having been dropped.
            if this.update(cx, |this, cx| this.adopt_model_index(index, cx)).is_err() {
                return;
            }

            let parts = parts.await;
            let _ = this.update(cx, |this, cx| this.adopt_ship_parts(parts, cx));
        }));
    }

    fn adopt_model_index(&mut self, index: ModelTreeIndex, cx: &mut Context<Self>) {
        self.models = ModelsState::Ready(Arc::new(index));
        self.rebuild_tree(Rebuild::Rematch, cx);
        cx.notify();
    }

    fn adopt_ship_parts(&mut self, parts: HashMap<VehicleParamIndex, ShipPartRows>, cx: &mut Context<Self>) {
        self.ship_parts = Arc::new(parts);
        // A search names parts as well as ships. Folders the user closed
        // meanwhile stay closed.
        self.rebuild_tree(Rebuild::Rematch, cx);
        cx.notify();
    }

    fn on_search_event(
        &mut self,
        _search_state: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Coalesces typing so a 15000-model search runs once per pause.
        const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

        if let InputEvent::PressEnter { .. } = event {
            self.open_from_search(cx);
            return;
        }

        let InputEvent::Change = event else { return };
        let text = self.search_state.read(cx).value().to_string();
        let query = fold(text.trim());
        if query == self.query {
            self._search_debounce = None;
            return;
        }

        if query.is_empty() {
            self._search_debounce = None;
            self.apply_query(query, cx);
            return;
        }

        self._search_debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| this.apply_query(query, cx));
        }));
    }

    /// Enter in the search field: applies a search still waiting out its
    /// pause, then opens the row the reader highlighted, or the first one
    /// that opens.
    fn open_from_search(&mut self, cx: &mut Context<Self>) {
        self.apply_pending_query(cx);

        // The open model's row is highlighted for the pane, not chosen.
        let open = self.open_model.as_ref().map(open_row_id);
        let highlighted = self
            .tree_state
            .read(cx)
            .selected_item()
            .map(|item| item.id.clone())
            .filter(|id| Some(id) != open.as_ref() && self.row_info.get(id).is_some_and(opens_on_its_own));
        if let Some(id) = highlighted.or_else(|| self.first_openable_row(cx)) {
            self.open_row(&id, cx);
        }
    }

    /// Applies the search field's text now if it is still waiting out its
    /// pause.
    fn apply_pending_query(&mut self, cx: &mut Context<Self>) {
        let text = self.search_state.read(cx).value().to_string();
        let query = fold(text.trim());
        if query != self.query {
            self._search_debounce = None;
            self.apply_query(query, cx);
        }
    }

    /// The first listed ship, model or part row.
    fn first_openable_row(&self, cx: &App) -> Option<SharedString> {
        let state = self.tree_state.read(cx);
        (0..)
            .map_while(|ix| state.entry(ix))
            .map(|entry| &entry.item().id)
            .find(|id| self.row_info.get(*id).is_some_and(opens_on_its_own))
            .cloned()
    }

    /// Does what a click on row `id` does: a ship or model row opens, a
    /// folder opens or closes.
    fn open_row(&mut self, id: &SharedString, cx: &mut Context<Self>) {
        let Some(kind) = self.row_info.get(id).cloned() else {
            return;
        };

        match &kind {
            RowKind::Ship { param_index, display_name } => {
                cx.emit(ShipSelected { param_index: param_index.clone(), display_name: display_name.clone() })
            }
            RowKind::Model(_) | RowKind::ShipPart { .. } => {
                if let Some(selection) = model_selection(&kind) {
                    cx.emit(selection);
                }
            }
            RowKind::Nation(_)
            | RowKind::Class(_)
            | RowKind::PartFolder
            | RowKind::ModelNation { .. }
            | RowKind::ModelGroup => self.toggle_folder(id, cx),
            // A heading and a status line do nothing.
            RowKind::Separator | RowKind::ModelsStatus => {}
        }
    }

    fn toggle_folder(&mut self, id: &SharedString, cx: &mut Context<Self>) {
        let state = self.tree_state.read(cx);
        let expanded = state.index_of(id).and_then(|ix| state.entry(ix)).map(TreeEntry::is_expanded);
        let event = match expanded {
            Some(true) => TreeEvent::Collapsed(id.clone()),
            Some(false) => TreeEvent::Expanded(id.clone()),
            None => return,
        };

        let tree_state = self.tree_state.clone();
        self.on_tree_event(tree_state, &event, cx);
    }

    /// Enter on the highlighted row of the list opens it.
    fn on_tree_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "enter" || event.keystroke.modifiers.modified() || event.is_held {
            return;
        }
        let Some(id) = self.tree_state.read(cx).selected_item().map(|item| item.id.clone()) else {
            return;
        };

        cx.stop_propagation();
        self.open_row(&id, cx);
    }

    /// Down in the search field moves into the list, onto the first row that
    /// opens.
    fn on_search_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key != "down" || event.keystroke.modifiers.modified() {
            return;
        }
        self.apply_pending_query(cx);
        let Some(id) = self.first_openable_row(cx) else {
            return;
        };

        cx.stop_propagation();
        self.tree_state.update(cx, |state, cx| {
            let ix = state.index_of(&id);
            state.set_selected_index(ix, cx);
            state.focus(window, cx);
        });
    }

    fn apply_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.query = query;
        self.search_expanded.clear();
        self.search_collapsed.clear();
        self.rebuild_tree(Rebuild::Rematch, cx);
        cx.notify();
    }

    fn on_tree_event(&mut self, _tree_state: Entity<TreeState>, event: &TreeEvent, cx: &mut Context<Self>) {
        let searching = !self.query.is_empty();
        let expanded = if searching { &mut self.search_expanded } else { &mut self.browse_expanded };
        match event {
            TreeEvent::Expanded(id) => {
                expanded.insert(id.clone());
                self.search_collapsed.remove(id);
            }
            TreeEvent::Collapsed(id) => {
                expanded.remove(id);
                if searching {
                    self.search_collapsed.insert(id.clone());
                }
            }
        }

        self.rebuild_tree(Rebuild::Keep, cx);
        // The rows' renderer and menu read the row map this rebuild replaced.
        cx.notify();
    }

    fn rebuild_tree(&mut self, rebuild: Rebuild, cx: &mut Context<Self>) {
        let Some(bundle) = self.bundle.clone() else {
            self.row_info = Rc::new(HashMap::new());
            self.row_notes = Rc::new(HashMap::new());
            self.tree_state.update(cx, |state, cx| state.set_items(Vec::new(), cx));
            return;
        };

        let models = match &self.models {
            ModelsState::Loading => ModelSection::Loading,
            ModelsState::Unavailable => ModelSection::Unavailable,
            ModelsState::Ready(index) => ModelSection::Ready(index),
        };
        let sources = TreeSources { ships: &bundle.catalog, ship_parts: &self.ship_parts, models };
        if rebuild == Rebuild::Rematch {
            self.search = SearchMatches::from_query(&sources, &self.query, self.tiers);
        }

        let searching = !self.query.is_empty();
        let set = if searching { &mut self.search_expanded } else { &mut self.browse_expanded };
        let auto = rebuild == Rebuild::Rematch && searching;
        let mut expansion = Expansion { set, closed: &self.search_collapsed, auto };
        let built = build_tree(&sources, self.search.as_ref(), &mut expansion);

        // A folder opened or closed keeps the row the reader is on.
        let kept = match rebuild {
            Rebuild::Keep => self.tree_state.read(cx).selected_item().map(|item| item.id.clone()),
            Rebuild::Rematch => None,
        };

        self.row_info = Rc::new(built.rows);
        self.row_notes = Rc::new(built.notes);
        self.tree_state.update(cx, |state, cx| state.set_items(built.items, cx));
        let kept_ix = kept.and_then(|id| self.tree_state.read(cx).index_of(&id));
        match kept_ix {
            Some(ix) => self.tree_state.update(cx, |state, cx| state.set_selected_index(Some(ix), cx)),
            None => self.highlight_open_row(cx),
        }
    }

    /// Selects the open model's row, or nothing when it is not listed.
    fn highlight_open_row(&mut self, cx: &mut Context<Self>) {
        let id = self.open_model.as_ref().map(open_row_id);
        self.tree_state.update(cx, |state, cx| {
            let ix = id.and_then(|id| state.index_of(&id));
            if state.selected_index() != ix {
                state.set_selected_index(ix, cx);
            }
        });
    }
}

/// The hardpoint target a row's checkbox controls, if it has one.
fn hardpoint_target(kind: &RowKind) -> Option<HardpointTarget> {
    match kind {
        RowKind::Model(model) => Some(HardpointTarget::Model(model.clone())),
        RowKind::ShipPart { ship, part, .. } => {
            Some(HardpointTarget::ShipPart { vehicle_param_index: ship.clone(), part: part.clone() })
        }
        _ => None,
    }
}

/// A slider over the tiers, I to XI, with its handles on `tiers`. Its step
/// keeps each handle on a whole tier, and a handle stops at the other.
fn tier_slider(tiers: TierRange, cx: &mut Context<Sidebar>) -> (Entity<SliderState>, Subscription) {
    let slider = cx.new(|_| {
        SliderState::new()
            .min(TierRange::LOWEST as f32)
            .max(TierRange::HIGHEST as f32)
            .step(1.)
            .default_value((tiers.lowest() as f32, tiers.highest() as f32))
    });
    let subscription = cx.subscribe(&slider, Sidebar::on_tier_slider);

    (slider, subscription)
}

/// The tiers a slider's handles stand on. A value between tiers, which the
/// slider's step does not let a handle take, goes to the nearer one.
fn tiers_from_slider(value: SliderValue) -> TierRange {
    TierRange::new(value.start().round() as u32, value.end().round() as u32)
}

/// The model a row opens and the ship part it was opened from.
fn model_selection(kind: &RowKind) -> Option<ModelSelected> {
    match kind {
        RowKind::Model(model) => Some(ModelSelected { model: model.clone(), from_ship: None }),
        RowKind::ShipPart { ship, part, model, .. } => Some(ModelSelected {
            model: model.clone(),
            from_ship: Some(ShipPartRef { vehicle_param_index: ship.clone(), part: part.clone() }),
        }),
        _ => None,
    }
}

/// Whether a row opens a ship or model, rather than being a folder or heading.
fn opens_on_its_own(kind: &RowKind) -> bool {
    matches!(kind, RowKind::Ship { .. } | RowKind::Model(_) | RowKind::ShipPart { .. })
}

/// What a row's tooltip says: its whole name, which the row may cut short,
/// and the model path or GameParams index behind it. A skin row also says
/// what it replaces, from `skin_uses`.
fn row_tooltip(kind: &RowKind, skin_uses: &[SkinUse]) -> Option<String> {
    let mut text = match kind {
        RowKind::Ship { param_index, display_name } => format!("{display_name}\n{param_index}"),
        RowKind::Model(model) | RowKind::ShipPart { node: None, model, .. } => format!("{}\n{model}", model.stem()),
        RowKind::ShipPart { node: Some(node), model, .. } => format!("{node} {}\n{model}", model.stem()),
        RowKind::Nation(_)
        | RowKind::Class(_)
        | RowKind::PartFolder
        | RowKind::Separator
        | RowKind::ModelsStatus
        | RowKind::ModelNation { .. }
        | RowKind::ModelGroup => return None,
    };

    // A line per exterior, so the sentence reads the same in every locale.
    for skin in skin_uses {
        for exterior in &skin.exteriors {
            text.push('\n');
            text.push_str(&t!("ui.models.skin_replaces", model = skin.stock.stem(), exterior = exterior));
        }
    }

    Some(text)
}

/// Renders one row: indent guides, a chevron or spacer, the nation flag or
/// class icon, the label, and the hardpoint checkbox on model rows.
#[allow(clippy::too_many_arguments)]
fn render_sidebar_item(
    sidebar: Entity<Sidebar>,
    ix: usize,
    entry: &TreeEntry,
    bundle: &ArmorAssetsBundle,
    row_info: &HashMap<SharedString, RowKind>,
    notes: &HashMap<SharedString, SharedString>,
    index: Option<&ModelTreeIndex>,
    hardpoints: &HashSet<HardpointTarget>,
    window: &mut Window,
    cx: &mut App,
) -> ListItem {
    let item = entry.item();
    let kind = row_info.get(&item.id);

    if let Some(RowKind::Separator) = kind {
        let heading = h_flex()
            .id(("armor-sidebar-row", ix))
            .test_support()
            .w_full()
            .h_full()
            .items_center()
            .border_t_1()
            .border_color(cx.theme().border)
            .text_sm()
            .font_weight(FontWeight::BOLD)
            .child(item.label.clone());
        return crate::ui::tree_row(ListItem::new(ix).child(heading), cx);
    }

    // The tree's scrollbar is drawn over the rows' right edge, so the row
    // ends short of it and the checkbox there stays clickable. The kit's
    // scrollbar takes this much: its thumb plus padding either side.
    const SCROLLBAR_GUTTER: Pixels = px(16.);

    let mut row = h_flex()
        .id(("armor-sidebar-row", ix))
        .test_support()
        .w_full()
        .gap_1()
        .items_center()
        .pr(SCROLLBAR_GUTTER)
        .child(crate::ui::indent_guides(entry.depth(), INDENT, cx));
    if entry.is_folder() {
        let turn = crate::motion::disclosure_turn(item.id.clone(), entry.is_expanded(), window, cx);
        row = row.child(crate::motion::chevron(turn));
    } else {
        row = row.child(div().flex_none().w(INDENT));
    }

    let flag = match kind {
        Some(RowKind::Nation(nation)) => Some(nation.as_str()),
        Some(RowKind::ModelNation { flag }) => flag.as_deref(),
        _ => None,
    };
    if let Some(image) = flag.and_then(|nation| bundle.icons.get_keyed(&format!("nation:{nation}"))) {
        row = row.child(div().flex_none().child(img(image).w(px(23.)).h(px(16.))));
    }
    if let Some(RowKind::Class(species)) = kind
        && let Some(image) = bundle.icons.get(*species, CLASS_ICON_TINT)
    {
        row = row.child(div().flex_none().child(img(image).w(px(16.)).h(px(16.))));
    }

    let text = || div().min_w(px(0.)).overflow_hidden().text_ellipsis().whitespace_nowrap();
    let label = h_flex().flex_1().min_w(px(0.)).gap_1().overflow_hidden();
    let label = match kind {
        Some(RowKind::ShipPart { node: Some(node), model, .. }) => label
            .child(div().flex_none().whitespace_nowrap().text_color(cx.theme().muted_foreground).child(node.clone()))
            .child(text().child(model.stem().to_string())),
        Some(RowKind::ModelsStatus) => {
            label.child(text().text_color(cx.theme().muted_foreground).child(item.label.clone()))
        }
        _ => label.child(text().child(item.label.clone())),
    };
    let label = match notes.get(&item.id) {
        Some(note) => label.child(text().text_color(cx.theme().muted_foreground).child(note.clone())),
        None => label,
    };
    row = row.child(label);
    // The text is built on hover; a row only clones what it shows.
    let hover = kind.filter(|kind| opens_on_its_own(kind));
    if let Some(kind) = hover.cloned() {
        let skin_uses = match &kind {
            RowKind::Model(model) => index.and_then(|index| index.skins.get(model)).cloned(),
            _ => None,
        };
        row = row.tooltip(move |window, cx| {
            let skin_uses = skin_uses.as_deref().unwrap_or_default();
            Tooltip::new(row_tooltip(&kind, skin_uses).unwrap_or_default()).build(window, cx)
        });
    }

    if let Some(target) = kind.and_then(hardpoint_target) {
        let visible = hardpoints.contains(&target);
        let entity = sidebar.clone();
        row = row.child(
            div().flex_none().child(
                Checkbox::new(("armor-sidebar-hardpoints", ix))
                    .checked(visible)
                    .small()
                    .tooltip(t!("ui.models.hardpoints_tooltip").to_string())
                    .on_click(move |checked, _window, cx| {
                        // The row underneath would otherwise open the model.
                        cx.stop_propagation();
                        let toggled = HardpointsToggled { target: target.clone(), visible: *checked };
                        entity.update(cx, |_sidebar, cx| cx.emit(toggled));
                    }),
            ),
        );
    }

    let mut list_item = ListItem::new(ix).child(row);
    match kind {
        Some(RowKind::Ship { param_index, display_name }) => {
            let selected = ShipSelected { param_index: param_index.clone(), display_name: display_name.clone() };
            list_item = list_item.on_click(move |_event: &ClickEvent, _window, cx: &mut App| {
                let selected = selected.clone();
                sidebar.update(cx, |_sidebar, cx| cx.emit(selected));
            });
        }
        Some(kind) => {
            if let Some(selection) = model_selection(kind) {
                list_item = list_item.on_click(move |_event: &ClickEvent, _window, cx: &mut App| {
                    let selection = selection.clone();
                    sidebar.update(cx, |_sidebar, cx| cx.emit(selection));
                });
            }
        }
        None => {}
    }

    crate::ui::tree_row(list_item, cx)
}

/// The right-click menu for one row.
fn row_context_menu(sidebar: &Entity<Sidebar>, kind: Option<&RowKind>, menu: PopupMenu) -> PopupMenu {
    let Some(kind) = kind else {
        return menu;
    };

    if let RowKind::Ship { param_index, display_name } = kind {
        let ship = ExportModelRequested { param_index: param_index.clone(), display_name: display_name.clone() };
        let compare_ship = ship.clone();
        let compare_entity = sidebar.clone();
        let export_entity = sidebar.clone();
        return menu
            .item(PopupMenuItem::new(t!("ui.armor.compare_split").into_owned()).on_click(move |_event, _window, cx| {
                let request = CompareSplit { ship: Some(compare_ship.clone()) };
                compare_entity.update(cx, |_sidebar, cx| cx.emit(request));
            }))
            .item(PopupMenuItem::new(t!("ui.armor.export_model").into_owned()).on_click(
                move |_event, _window, cx| {
                    let request = ship.clone();
                    export_entity.update(cx, |_sidebar, cx| cx.emit(request));
                },
            ));
    }

    let Some(selection) = model_selection(kind) else {
        return menu;
    };
    let export = ExportCatalogModelRequested { model: selection.model.clone(), from_ship: selection.from_ship.clone() };
    let path = selection.model.as_str().to_string();
    let open_entity = sidebar.clone();
    let export_entity = sidebar.clone();
    menu.item(PopupMenuItem::new(t!("ui.models.open").into_owned()).on_click(move |_event, _window, cx| {
        let request = selection.clone();
        open_entity.update(cx, |_sidebar, cx| cx.emit(request));
    }))
    .item(PopupMenuItem::new(t!("ui.models.export").into_owned()).on_click(move |_event, _window, cx| {
        let request = export.clone();
        export_entity.update(cx, |_sidebar, cx| cx.emit(request));
    }))
    .item(PopupMenuItem::new(t!("ui.models.copy_path").into_owned()).on_click(move |_event, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
        crate::toast::ok(t!("ui.models.path_copied", path = path).into_owned(), window, cx);
    }))
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
            .child(div().flex_1().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.armor.ships").to_string()))
            .child(
                Button::new("armor-sidebar-compare")
                    .icon(IconName::LayoutDashboard)
                    .label(t!("ui.armor.compare").to_string())
                    .compact()
                    .tooltip(t!("ui.armor.compare_tooltip").to_string())
                    .on_click(cx.listener(|_this, _event, _window, cx| cx.emit(CompareSplit::default()))),
            )
            .child(self.common_menu(cx));

        let search_row = h_flex()
            .capture_key_down(cx.listener(Self::on_search_key_down))
            .flex_none()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(Icon::new(IconName::Search))
            .child(Input::new(&self.search_state).id("armor-sidebar-search").small().flex_1().min_w(px(0.)));

        let body: AnyElement = match &self.bundle {
            None => crate::loading::Loading::new("armor-sidebar-loading", t!("ui.armor.loading_catalog").into_owned())
                .top()
                .into_any_element(),
            Some(bundle) => {
                let entity = cx.entity();
                let bundle = Arc::clone(bundle);
                let row_info = Rc::clone(&self.row_info);
                let notes = Rc::clone(&self.row_notes);
                let index = match &self.models {
                    ModelsState::Ready(index) => Some(Arc::clone(index)),
                    ModelsState::Loading | ModelsState::Unavailable => None,
                };
                let hardpoints = Rc::clone(&self.hardpoint_visibility);
                let menu_entity = entity.clone();
                let menu_row_info = Rc::clone(&self.row_info);
                tree(&self.tree_state, move |ix, entry, _selected, window, cx| {
                    let index = index.as_deref();
                    render_sidebar_item(
                        entity.clone(),
                        ix,
                        entry,
                        &bundle,
                        &row_info,
                        &notes,
                        index,
                        &hardpoints,
                        window,
                        cx,
                    )
                })
                .context_menu(move |_ix, entry, menu, _window, _cx| {
                    row_context_menu(&menu_entity, menu_row_info.get(&entry.item().id), menu)
                })
                .flex_1()
                .into_any_element()
            }
        };

        let list = div().flex_1().min_h(px(0.)).on_key_down(cx.listener(Self::on_tree_key_down)).child(body);

        v_flex().size_full().child(header).child(search_row).child(list)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use gpui_kit::AppContext;
    use gpui_kit::EventEmitter;
    use gpui_kit::SharedString;
    use gpui_kit::TestAppContext;
    use gpui_kit::WindowHandle;
    use gpui_kit::component::slider::SliderEvent;
    use gpui_kit::component::slider::SliderValue;
    use gpui_kit::component::tree::TreeEvent;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestAppContextExt;
    use gpui_kit::test::TestWindowExt;
    use wows_toolkit_viewmodel::settings::keys;
    use wowsunpack::export::hardpoint::HardpointName;
    use wowsunpack::export::ship::ShipAssets;
    use wowsunpack::game_params::provider::GameMetadataProvider;
    use wowsunpack::vfs::MemoryFS;
    use wowsunpack::vfs::VfsPath;

    use crate::replay_inspector::icons::IconCache;

    use super::super::assets::ArmorAssetsBundle;
    use super::super::catalog::TierRange;
    use super::super::model_refs::HardpointTarget;
    use super::super::model_refs::OpenTarget;
    use super::super::model_refs::ShipPartKey;
    use super::super::model_refs::ShipPartRef;
    use super::super::model_refs::VehicleParamIndex;
    use super::super::model_tree::ModelTreeIndex;
    use super::super::sidebar_tree::ModelSection;
    use super::super::sidebar_tree::RowKind;
    use super::super::sidebar_tree::SearchMatches;
    use super::super::sidebar_tree::TreeSources;
    use super::super::sidebar_tree::fixture;
    use super::super::sidebar_tree::model_row_id;
    use super::super::sidebar_tree::part_row_id;
    use super::super::sidebar_tree::ship_row_id;
    use super::HardpointsToggled;
    use super::ModelSelected;
    use super::ModelsState;
    use super::Rebuild;
    use super::ShipSelected;
    use super::Sidebar;

    /// Ship assets over an empty store: the sidebar reads only the catalog and
    /// icons from the bundle.
    fn bundle() -> Arc<ArmorAssetsBundle> {
        let vfs: VfsPath = MemoryFS::new().into();
        let metadata = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider builds");
        let assets = ShipAssets::from_parts(&vfs, Arc::new(metadata), Arc::new(Vec::new()));

        Arc::new(ArmorAssetsBundle { assets, catalog: fixture::ships(), icons: IconCache::new() })
    }

    fn open_sidebar(cx: &mut TestAppContext) -> WindowHandle<Sidebar> {
        rust_i18n::set_locale("en");
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(500.), px(1000.)), Sidebar::new);
        window
            .update(cx, |sidebar, _window, cx| {
                sidebar.set_bundle(bundle(), cx);
                let catalog = fixture::catalog();
                let ships = fixture::ships();
                let references = fixture::references(&ships);
                let index = ModelTreeIndex::build(&catalog, &ships, &references, &fixture::metadata());
                sidebar.adopt_model_index(index, cx);
                sidebar.adopt_ship_parts(fixture::ship_parts(&catalog, &references), cx);
            })
            .expect("the window is open");

        window
    }

    fn record<E: Clone + 'static>(window: &WindowHandle<Sidebar>, cx: &mut TestAppContext) -> Rc<RefCell<Vec<E>>>
    where
        Sidebar: EventEmitter<E>,
    {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sidebar = window.entity(cx).expect("the window has a root view");
        let recorder = Rc::clone(&seen);
        cx.update(|cx| {
            cx.subscribe(&sidebar, move |_sidebar, event: &E, _cx| recorder.borrow_mut().push(event.clone()))
        })
        .detach();

        seen
    }

    fn row_index(window: &WindowHandle<Sidebar>, id: &SharedString, cx: &mut TestAppContext) -> Option<usize> {
        window.update(cx, |sidebar, _window, cx| sidebar.tree_state.read(cx).index_of(id)).expect("the window is open")
    }

    fn selected_row(window: &WindowHandle<Sidebar>, cx: &mut TestAppContext) -> Option<usize> {
        window
            .update(cx, |sidebar, _window, cx| sidebar.tree_state.read(cx).selected_index())
            .expect("the window is open")
    }

    /// Opens row `row`'s context menu and lists what it offers, each with the
    /// menu row it answers to.
    fn open_row_menu(window: WindowHandle<Sidebar>, row: usize, cx: &mut TestAppContext) -> Vec<(u64, String)> {
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.right_click(("armor-sidebar-row", row), cx);
        })
        .expect("the window is open");
        // The menu opens through a deferred effect.
        cx.run_until_parked();

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            crate::interaction_tests::menu_items(window)
                .into_iter()
                .filter_map(|(menu_row, item)| Some((menu_row, item.label()?.to_owned())))
                .collect()
        })
        .expect("the window is open")
    }

    /// Opens row `row`'s context menu and lists the labels it offers, sorted.
    fn row_menu_labels(window: WindowHandle<Sidebar>, row: usize, cx: &mut TestAppContext) -> Vec<String> {
        let mut labels: Vec<String> = open_row_menu(window, row, cx).into_iter().map(|(_, label)| label).collect();
        labels.sort();

        labels
    }

    fn choose_from_row_menu(window: WindowHandle<Sidebar>, row: usize, label: &str, cx: &mut TestAppContext) {
        let offered = open_row_menu(window, row, cx);
        let (menu_row, _) = offered
            .iter()
            .find(|(_, offer)| offer == label)
            .unwrap_or_else(|| panic!("the row menu offers {label:?}, offered {offered:?}"));
        cx.update_window(window.into(), |_, window, cx| {
            window.within("popup-menu").click(gpui_kit::ElementId::Integer(*menu_row), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();
    }

    /// The row reached by opening its folders with the pointer, as a reader
    /// browsing the listing does.
    #[gpui_kit::test]
    fn copy_model_path_puts_the_path_on_the_clipboard(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let usa = "model-folder/nation-usa";
        for folder in [usa.to_string(), format!("{usa}/Gun"), format!("{usa}/Gun/MainBattery")] {
            let row = row_index(&window, &folder.into(), cx).expect("the folder is listed");
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                window.click(("armor-sidebar-row", row), cx);
            })
            .expect("the window is open");
            cx.run_until_parked();
        }
        let turret = fixture::path(fixture::IOWA_TURRET);
        let row = row_index(&window, &model_row_id(&turret), cx).expect("the opened folder lists the turret");

        choose_from_row_menu(window, row, "Copy model path", cx);

        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some(fixture::IOWA_TURRET));
    }

    #[test]
    fn a_row_tooltip_names_the_whole_row_and_what_it_opens() {
        let turret = fixture::path(fixture::IOWA_TURRET);
        let ship = RowKind::Ship { param_index: fixture::IOWA_INDEX.to_string(), display_name: "Iowa".to_string() };
        let part = RowKind::ShipPart {
            ship: VehicleParamIndex::new(fixture::IOWA_INDEX),
            part: ShipPartKey::Mount(HardpointName::new("HP_AGM_1")),
            model: turret.clone(),
            node: Some("HP_AGM_1".into()),
        };

        assert_eq!(super::row_tooltip(&ship, &[]).as_deref(), Some("Iowa\nPASB018"));
        assert_eq!(
            super::row_tooltip(&RowKind::Model(turret), &[]).as_deref(),
            Some(format!("AGM034_16in50_Mk7\n{}", fixture::IOWA_TURRET).as_str())
        );
        assert_eq!(
            super::row_tooltip(&part, &[]).as_deref(),
            Some(format!("HP_AGM_1 AGM034_16in50_Mk7\n{}", fixture::IOWA_TURRET).as_str())
        );
        assert_eq!(super::row_tooltip(&RowKind::PartFolder, &[]), None, "a folder shows its whole name already");
    }

    #[test]
    fn a_skin_row_tooltip_names_the_model_it_replaces_and_the_exteriors() {
        rust_i18n::set_locale("en");
        let catalog = fixture::catalog();
        let ships = fixture::ships();
        let index = ModelTreeIndex::build(&catalog, &ships, &fixture::references(&ships), &fixture::metadata());
        let skin = fixture::path(fixture::IOWA_SKIN);
        let uses = index.skins.get(&skin).expect("the skin's exteriors are known");

        let tooltip = super::row_tooltip(&RowKind::Model(skin.clone()), uses);

        let expected = format!(
            "ASB128_Iowa_Halloween\n{}\nReplaces ASB028_Iowa_1945 when Halloween is equipped\nReplaces ASB028_Iowa_1945 when Spooky is equipped",
            fixture::IOWA_SKIN
        );
        assert_eq!(tooltip.as_deref(), Some(expected.as_str()));
    }

    #[gpui_kit::test]
    fn a_catalogue_model_row_offers_to_export_the_model(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        window.update(cx, |sidebar, _window, cx| sidebar.apply_query("agm034".to_string(), cx)).expect("open");
        let turret = fixture::path(fixture::IOWA_TURRET);
        let row = row_index(&window, &model_row_id(&turret), cx).expect("the search lists the turret");

        assert_eq!(row_menu_labels(window, row, cx), ["Copy model path", "Export Model", "Open"]);
    }

    #[gpui_kit::test]
    fn the_hardpoint_checkbox_toggles_without_opening_the_row_and_a_row_click_opens_it(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let selected = record::<ModelSelected>(&window, cx);
        let toggled = record::<HardpointsToggled>(&window, cx);
        window.update(cx, |sidebar, _window, cx| sidebar.apply_query("agm034".to_string(), cx)).expect("open");
        let turret = fixture::path(fixture::IOWA_TURRET);
        let row = row_index(&window, &model_row_id(&turret), cx).expect("the search lists the turret");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("armor-sidebar-hardpoints", row), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();

        assert_eq!(
            *toggled.borrow(),
            [HardpointsToggled { target: HardpointTarget::Model(turret.clone()), visible: true }]
        );
        assert!(selected.borrow().is_empty(), "the checkbox does not open the model");
        assert_eq!(selected_row(&window, cx), None, "nor select its row");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("armor-sidebar-row", row), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();

        assert_eq!(*selected.borrow(), [ModelSelected { model: turret, from_ship: None }]);
        assert_eq!(toggled.borrow().len(), 1, "the row click is not a hardpoint toggle");
    }

    #[gpui_kit::test]
    fn the_pane_drives_the_checkbox_state_and_the_highlighted_row(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        window.update(cx, |sidebar, _window, cx| sidebar.apply_query("agm034".to_string(), cx)).expect("open");
        let turret = fixture::path(fixture::IOWA_TURRET);
        let row = row_index(&window, &model_row_id(&turret), cx).expect("the search lists the turret");

        window
            .update(cx, |sidebar, _window, cx| {
                sidebar.set_hardpoint_visibility(&HashSet::from([HardpointTarget::Model(turret.clone())]), cx);
                sidebar.set_open_model(Some(OpenTarget::Model { model: turret.clone(), from_ship: None }), cx);
            })
            .expect("the window is open");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("armor-sidebar-hardpoints", row)).checked(), Some(true));
        })
        .expect("the window is open");
        assert_eq!(selected_row(&window, cx), Some(row), "the open model's row is highlighted");
    }

    #[gpui_kit::test]
    fn the_tier_slider_narrows_the_ships_and_saves_the_tiers_when_let_go(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let iowa = ship_row_id(fixture::IOWA_INDEX);
        let south_carolina = ship_row_id("PASB001");
        window
            .update(cx, |sidebar, _window, cx| {
                sidebar.browse_expanded.extend(["armor-nation-USA".into(), "armor-class-USA-Battleship".into()]);
                let slider = sidebar.tier_slider.clone();
                sidebar.on_tier_slider(slider.clone(), &SliderEvent::Change(SliderValue::Range(2.8, 8.)), cx);
                sidebar.on_tier_slider(slider, &SliderEvent::Release(SliderValue::Range(3., 8.)), cx);
            })
            .expect("the window is open");

        assert!(row_index(&window, &south_carolina, cx).is_some(), "tier III is listed");
        assert_eq!(row_index(&window, &iowa, cx), None, "tier IX is not");
        let saved: Vec<String> = crate::settings_store::saved_for_test()
            .into_iter()
            .filter(|(key, _)| *key == keys::MODEL_VIEWER_SHIP_TIERS)
            .map(|(_, json)| json)
            .collect();
        assert_eq!(saved, ["[3,8]"], "saved once, when the handle is let go");
    }

    #[gpui_kit::test]
    fn saved_tiers_move_the_slider_and_filter_the_ships(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);

        let (value, tiers) = window
            .update(cx, |sidebar, _window, cx| {
                sidebar.browse_expanded.extend(["armor-nation-USA".into(), "armor-class-USA-Battleship".into()]);
                sidebar.adopt_tiers(TierRange::new(9, 11), cx);
                (sidebar.tier_slider.read(cx).value(), sidebar.tiers)
            })
            .expect("the window is open");

        assert_eq!(value, SliderValue::Range(9., 11.));
        assert_eq!(tiers, TierRange::new(9, 11));
        assert!(row_index(&window, &ship_row_id(fixture::IOWA_INDEX), cx).is_some(), "Iowa, tier IX, is listed");
        assert_eq!(row_index(&window, &ship_row_id("PASB001"), cx), None, "S. Carolina, tier III, is not");
    }

    #[test]
    fn slider_handles_give_whole_tiers_in_order() {
        assert_eq!(super::tiers_from_slider(SliderValue::Range(3.2, 8.6)), TierRange::new(3, 9));
        assert_eq!(super::tiers_from_slider(SliderValue::Range(8., 3.)), TierRange::new(3, 8), "crossed handles");
        assert_eq!(super::tiers_from_slider(SliderValue::Range(0., 30.)), TierRange::ALL);
    }

    /// Highlights row `row` with the list focused, as arrowing to it does,
    /// and presses `keys`.
    fn press_on_row(window: WindowHandle<Sidebar>, row: usize, keys: &str, cx: &mut TestAppContext) {
        window
            .update(cx, |sidebar, window, cx| {
                sidebar.tree_state.update(cx, |state, cx| {
                    state.set_selected_index(Some(row), cx);
                    state.focus(window, cx);
                });
            })
            .expect("the window is open");
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the window is open");
        cx.simulate_keystrokes(window.into(), keys);
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn enter_on_the_highlighted_row_opens_it_or_its_folder(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let ships = record::<ShipSelected>(&window, cx);
        let models = record::<ModelSelected>(&window, cx);
        let usa = "model-folder/nation-usa";
        window
            .update(cx, |sidebar, _window, cx| {
                let folders = ["armor-nation-USA".to_string(), "armor-class-USA-Battleship".to_string()];
                let model_folders = [usa.to_string(), format!("{usa}/Gun"), format!("{usa}/Gun/MainBattery")];
                sidebar.browse_expanded.extend(folders.into_iter().chain(model_folders).map(SharedString::from));
                sidebar.rebuild_tree(Rebuild::Keep, cx);
            })
            .expect("the window is open");
        let turret = fixture::path(fixture::IOWA_TURRET);

        let row = row_index(&window, &model_row_id(&turret), cx).expect("the turret is listed");
        press_on_row(window, row, "enter", cx);
        assert_eq!(*models.borrow(), [ModelSelected { model: turret, from_ship: None }]);

        let row = row_index(&window, &ship_row_id(fixture::IOWA_INDEX), cx).expect("Iowa is listed");
        press_on_row(window, row, "enter", cx);
        assert_eq!(
            *ships.borrow(),
            [ShipSelected { param_index: fixture::IOWA_INDEX.to_string(), display_name: "Iowa".to_string() }]
        );

        let japan: SharedString = "model-folder/nation-japan".into();
        let row = row_index(&window, &japan, cx).expect("Japan's models are listed");
        press_on_row(window, row, "enter", cx);
        assert!(row_index(&window, &"model-folder/nation-japan/Ship".into(), cx).is_some(), "the folder opened");
        assert_eq!(selected_row(&window, cx), row_index(&window, &japan, cx), "and stays highlighted");
    }

    #[gpui_kit::test]
    fn enter_in_the_search_field_opens_the_first_match_and_down_moves_into_the_list(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let ships = record::<ShipSelected>(&window, cx);
        let models = record::<ModelSelected>(&window, cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("armor-sidebar-search", cx);
            window.input("yamato", cx);
        })
        .expect("the window is open");
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();

        assert_eq!(
            *ships.borrow(),
            [ShipSelected { param_index: "PJSB018".to_string(), display_name: "Yamato".to_string() }],
            "the search applies at once and its first row opens"
        );

        // A search naming only a model, typed and still waiting out its
        // pause when Down is pressed.
        window
            .update(cx, |sidebar, window, cx| {
                sidebar.search_state.update(cx, |state, cx| state.set_value("lazarus", window, cx));
            })
            .expect("the window is open");
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("armor-sidebar-search", cx);
        })
        .expect("the window is open");
        cx.simulate_keystrokes(window.into(), "down enter");
        cx.run_until_parked();

        let lazarus =
            fixture::path("content/gameplay/events/director/XD006_Lazarus_Director/XD006_Lazarus_Director.model");
        assert_eq!(*models.borrow(), [ModelSelected { model: lazarus, from_ship: None }], "down moved into the list");
        assert_eq!(ships.borrow().len(), 1);
    }

    #[gpui_kit::test]
    fn typing_a_search_filters_ships_and_models(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("armor-sidebar-search", cx);
            window.input("iowa", cx);
        })
        .expect("the window is open");
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();

        let iowa_ship = ship_row_id(fixture::IOWA_INDEX);
        let iowa_model = model_row_id(&fixture::path(fixture::IOWA));
        assert!(row_index(&window, &iowa_ship, cx).is_some(), "the ship is listed");
        assert!(row_index(&window, &iowa_model, cx).is_some(), "the model is listed");
        assert_eq!(row_index(&window, &ship_row_id("PJSB018"), cx), None, "a ship not matching is not");
        let yamato_model =
            fixture::path("content/gameplay/japan/ship/battleship/JSB018_Yamato_1944/JSB018_Yamato_1944.model");
        assert_eq!(row_index(&window, &model_row_id(&yamato_model), cx), None, "nor a model not matching");
    }

    #[gpui_kit::test]
    fn a_folder_closed_during_a_search_stays_closed_when_the_ship_parts_arrive(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let iowa = ship_row_id(fixture::IOWA_INDEX);
        window.update(cx, |sidebar, _window, cx| sidebar.apply_query("agm034".to_string(), cx)).expect("open");
        assert!(row_index(&window, &iowa, cx).is_some(), "the search opens the ship found by its parts");

        window
            .update(cx, |sidebar, _window, cx| {
                let tree_state = sidebar.tree_state.clone();
                sidebar.on_tree_event(tree_state, &TreeEvent::Collapsed("armor-nation-USA".into()), cx);
                let catalog = fixture::catalog();
                let references = fixture::references(&fixture::ships());
                sidebar.adopt_ship_parts(fixture::ship_parts(&catalog, &references), cx);
            })
            .expect("the window is open");

        assert!(row_index(&window, &"armor-nation-USA".into(), cx).is_some(), "the nation is listed");
        assert_eq!(row_index(&window, &iowa, cx), None, "and stays closed");
    }

    #[gpui_kit::test]
    fn opening_a_folder_during_a_search_lists_from_the_matches_already_found(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let yamato_model =
            fixture::path("content/gameplay/japan/ship/battleship/JSB018_Yamato_1944/JSB018_Yamato_1944.model");
        let turret = fixture::path(fixture::IOWA_TURRET);

        window
            .update(cx, |sidebar, _window, cx| {
                sidebar.apply_query("agm034".to_string(), cx);

                // Matches for another query stand in for the stored ones, so a
                // rebuild that matched "agm034" again would list the turret.
                let stand_in = {
                    let ModelsState::Ready(index) = &sidebar.models else { panic!("the model index is adopted") };
                    let bundle = sidebar.bundle.clone().expect("a bundle is set");
                    let sources = TreeSources {
                        ships: &bundle.catalog,
                        ship_parts: &sidebar.ship_parts,
                        models: ModelSection::Ready(index),
                    };
                    SearchMatches::from_query(&sources, "yamato", TierRange::ALL)
                };
                sidebar.search = stand_in;

                let japan = "model-folder/nation-japan";
                for id in [japan.to_string(), format!("{japan}/Ship"), format!("{japan}/Ship/Battleship")] {
                    let tree_state = sidebar.tree_state.clone();
                    sidebar.on_tree_event(tree_state, &TreeEvent::Expanded(id.into()), cx);
                }
            })
            .expect("the window is open");

        assert!(row_index(&window, &model_row_id(&yamato_model), cx).is_some(), "the stored matches are listed");
        assert_eq!(row_index(&window, &model_row_id(&turret), cx), None, "the query is not matched again");
    }

    #[gpui_kit::test]
    fn a_ship_row_loads_the_ship_and_a_part_row_opens_the_part_model(cx: &mut TestAppContext) {
        let window = open_sidebar(cx);
        let ships = record::<ShipSelected>(&window, cx);
        let selected = record::<ModelSelected>(&window, cx);
        let iowa = ship_row_id(fixture::IOWA_INDEX);
        window
            .update(cx, |sidebar, _window, cx| {
                for id in ["armor-nation-USA", "armor-class-USA-Battleship", "armor-part-PASB018-group-MainBattery"] {
                    sidebar.browse_expanded.insert(id.into());
                }
                sidebar.rebuild_tree(Rebuild::Keep, cx);
            })
            .expect("the window is open");
        let ship_row = row_index(&window, &iowa, cx).expect("the ship is listed");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("armor-sidebar-row", ship_row), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();

        assert_eq!(
            *ships.borrow(),
            [ShipSelected { param_index: fixture::IOWA_INDEX.to_string(), display_name: "Iowa".to_string() }]
        );
        let vehicle = VehicleParamIndex::new(fixture::IOWA_INDEX);
        let part = ShipPartKey::Mount(HardpointName::new("HP_AGM_1"));
        let part_row =
            row_index(&window, &part_row_id(&vehicle, &part), cx).expect("opening the ship lists its mounts");

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click(("armor-sidebar-row", part_row), cx);
        })
        .expect("the window is open");
        cx.run_until_parked();

        let from_ship = Some(ShipPartRef { vehicle_param_index: vehicle, part });
        assert_eq!(*selected.borrow(), [ModelSelected { model: fixture::path(fixture::IOWA_TURRET), from_ship }]);
    }
}
