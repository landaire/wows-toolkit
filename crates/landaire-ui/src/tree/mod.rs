//! A tree of fixed-height rows, of which only the visible ones exist.

mod flat;
mod keys;
mod model;
mod state;
mod style;

use std::collections::HashSet;
use std::rc::Rc;

use zgui::component;
use zgui::elements::Focus;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Track;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::AttrName;
use zgui::view::Attrs;
use zgui::view::ClassName;
use zgui::view::Classes;
use zgui::view::CustomPropertyName;
use zgui::view::EventCx;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::FocusCause;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;
use zgui::vocab::SemanticFlags;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuPlace;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::menu::Typeahead;
use zgui_ui::prelude::Binding;
use zgui_ui::virtualize::VirtualWindow;
use zgui_ui::virtualize::Virtualize;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::surface;
use crate::surface::IndentGuidesProps;
use crate::surface::IndentWidth;
use crate::surface::RowHeight;
use crate::surface::RowIndex;
use crate::surface::STRIPE_CLASS;
use crate::surface::TreeDepth;
use crate::tree::flat::FlatRow;
use crate::tree::flat::FlatRows;
use crate::tree::keys::Command;
use crate::tree::state::TreeState;

pub use crate::tree::flat::NodeKind;
pub use crate::tree::model::TreeModel;
pub use crate::tree::state::SelectionMode;

/// What a [`Tree`]'s row slot is handed to draw one node.
///
/// The slot draws what follows the indent and the disclosure chevron, which the tree draws.
pub struct TreeRow<K> {
    pub key: K,
    /// Where the row sits among the visible rows.
    pub index: RowIndex,
    pub depth: TreeDepth,
    pub kind: NodeKind,
    pub expanded: Signal<bool, LocalStorage>,
    pub selected: Signal<bool, LocalStorage>,
    /// Whether the keyboard cursor is on the row.
    pub focused: Signal<bool, LocalStorage>,
    /// The row's element, for placing a surface beside it.
    pub anchor: NodeRef,
}

/// Which rows of a [`Tree`] are lifted, as [`STRIPE_CLASS`] lifts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TreeStripes {
    #[default]
    None,
    /// Every other visible row, starting with the first.
    Rows,
    /// The leaves among every other visible row, so branch rows keep the panel's ground and read
    /// as headings.
    Leaves,
}

/// What a leaf row puts where a branch row has its chevron.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LeafLead {
    /// Space as wide as the chevron, so a leaf's content lines up with its sibling branches'.
    #[default]
    Space,
    /// Nothing: a leaf's content starts at its indent.
    Nothing,
}

/// What a plain click on a branch row does besides selecting it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BranchClick {
    /// Only selects. The chevron and the arrow keys expand and collapse.
    #[default]
    Select,
    /// Selects and expands or collapses the node.
    SelectAndToggle,
}

/// The items of the menu a secondary click on a row of a [`Tree`] opens, built for the row's
/// key.
///
/// A secondary click on a row outside the selection selects that row first, so a menu that acts
/// on the selection acts on what was clicked.
pub struct TreeMenu<K> {
    build: Rc<dyn Fn(&K) -> AnyView>,
}

impl<K> TreeMenu<K> {
    pub fn new<V>(build: impl Fn(&K) -> V + 'static) -> Self
    where
        V: IntoView + 'static,
    {
        Self { build: Rc::new(move |key: &K| AnyView::new(build(key))) }
    }
}

impl<K> Clone for TreeMenu<K> {
    fn clone(&self) -> Self {
        Self { build: Rc::clone(&self.build) }
    }
}

/// Asks the [`Tree`] it is given to bring a node into view.
pub struct TreeHandle<K: 'static> {
    request: RwSignal<Option<K>, LocalStorage>,
}

impl<K: 'static> TreeHandle<K> {
    /// A handle on no tree yet, belonging to the current reactive scope.
    pub fn new() -> Self {
        Self { request: RwSignal::new_local(None) }
    }

    /// Expands the ancestors of `key` and scrolls its row into view.
    pub fn scroll_to_key(&self, key: K) {
        self.request.set(Some(key));
    }
}

impl<K: 'static> Default for TreeHandle<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: 'static> Clone for TreeHandle<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: 'static> Copy for TreeHandle<K> {}

/// A tree of fixed-height rows over an application's [`TreeModel`]. Only the rows in view are
/// built, so a tree of any size costs about as much as its scrollport.
///
/// The tree draws each row's indent guides and disclosure chevron; `row` draws the rest.
/// `expanded` and `selection` are the application's when bound, and the tree's own otherwise.
/// A row is keyed by its position and its node, so a `row` slot that shows changing data reads
/// it from a signal.
///
/// # Keyboard
///
/// The tree is one tab stop. Inside it:
///
/// | Key | What it does |
/// |---|---|
/// | Up, Down, Page Up, Page Down, Home, End | move the cursor and select its row |
/// | Shift with those | select the run from the anchor to the cursor |
/// | Primary modifier with those | move the cursor only |
/// | Right | expand the node, or step to its first child |
/// | Left | collapse the node, or step to its parent |
/// | Space | select the cursor's row; with the primary modifier, toggle it |
/// | Primary modifier and A | select every visible row |
/// | Enter | activate the cursor's row |
/// | Menu, Shift and F10 | open the row's context menu |
/// | Printable characters | move to the next row whose [`TreeModel::text`] begins with them |
#[allow(clippy::too_many_arguments, reason = "each prop is a named field of the props builder")]
#[component]
pub fn Tree<M, V, F>(
    /// The nodes. A signal, not `into`, so the model's type is inferred from it.
    model: Signal<M, LocalStorage>,
    /// Draws one row's content.
    row: F,
    #[prop(into, default = Signal::stored_local(RowHeight(22.0)))] row_height: Signal<RowHeight, LocalStorage>,
    #[prop(default = IndentWidth(16.0))] indent: IndentWidth,
    /// The keys of the expanded nodes.
    #[prop(into, optional)]
    expanded: Binding<HashSet<M::Key>>,
    /// The keys of the selected nodes.
    #[prop(into, optional)]
    selection: Binding<HashSet<M::Key>>,
    #[prop(optional)] selection_mode: SelectionMode,
    #[prop(optional)] branch_click: BranchClick,
    #[prop(optional)] stripes: TreeStripes,
    #[prop(optional)] leaf_lead: LeafLead,
    /// Whether each level of indent draws a guide.
    #[prop(default = true)]
    guides: bool,
    /// Told the key of a row that is double-clicked, or the cursor's when Enter is pressed.
    #[prop(optional)]
    on_activate: Option<UnsyncCallback<M::Key>>,
    #[prop(optional)] context_menu: Option<TreeMenu<M::Key>>,
    #[prop(optional)] handle: Option<TreeHandle<M::Key>>,
    /// What the tree is called, for a screen reader.
    #[prop(into, optional)]
    label: Option<String>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView
where
    M: TreeModel,
    V: IntoView + 'static,
    F: Fn(TreeRow<M::Key>) -> V + 'static,
{
    install_stylesheet(surface::SHEET, surface::CSS);
    install_stylesheet(style::SHEET, style::CSS);

    let state = TreeState::new(model, expanded, selection, selection_mode);
    let port = NodeRef::new();
    let following = follow_model(state);
    let revealing = handle.map(|handle| follow_handle(state, handle, port, row_height));
    on_cleanup_local(move || {
        drop(following);
        drop(revealing);
    });

    let menu_target: RwSignal<Option<M::Key>, LocalStorage> = RwSignal::new_local(None);
    let parts = Rc::new(RowParts {
        state,
        slot: Rc::new(row),
        indent,
        guides,
        stripes,
        leaf_lead,
        branch_click,
        on_activate,
        menu_request: context_menu.is_some().then(|| RwSignal::new_local(None)),
        cursor_row: RwSignal::new_local(None),
    });
    let body = Body { parts, port, row_height, menu_target, label, class, attrs };

    let Some(menu) = context_menu else {
        return AnyView::new(body.into_view());
    };

    AnyView::new(view! {
        ContextMenu {
            ContextMenuTrigger(class = "lui-tree-area") {
                {body.into_view()}
            }
            ContextMenuContent {
                {menu_items(menu.clone(), menu_target)}
            }
        }
    })
}

/// The items of `menu` for the row it was last opened on.
fn menu_items<K: Clone + 'static>(
    menu: TreeMenu<K>,
    target: RwSignal<Option<K>, LocalStorage>,
) -> impl Fn() -> Option<AnyView> + 'static {
    move || target.get().map(|key| (menu.build)(&key))
}

/// What every row of one tree shares.
struct RowParts<M: TreeModel, F> {
    state: TreeState<M>,
    slot: Rc<F>,
    indent: IndentWidth,
    guides: bool,
    stripes: TreeStripes,
    leaf_lead: LeafLead,
    branch_click: BranchClick,
    on_activate: Option<UnsyncCallback<M::Key>>,
    /// The row a secondary press landed on, taken by the scroll container as the press passes
    /// it. `None` when the tree has no context menu.
    menu_request: Option<RwSignal<Option<M::Key>, LocalStorage>>,
    /// The element of the row the cursor is on, while it is built.
    cursor_row: RwSignal<Option<NodeRef>, LocalStorage>,
}

/// One visible row as the keyed list of built rows knows it.
#[derive(Clone, PartialEq, Eq, Hash)]
struct RowSpec<K> {
    index: RowIndex,
    row: FlatRow<K>,
}

/// The scroll container and the rows in it.
struct Body<M: TreeModel, F> {
    parts: Rc<RowParts<M, F>>,
    port: NodeRef,
    row_height: Signal<RowHeight, LocalStorage>,
    menu_target: RwSignal<Option<M::Key>, LocalStorage>,
    label: Option<String>,
    class: Classes,
    attrs: Attrs,
}

impl<M, V, F> Body<M, F>
where
    M: TreeModel,
    V: IntoView + 'static,
    F: Fn(TreeRow<M::Key>) -> V + 'static,
{
    /// Built inside the context menu's trigger, when there is one, so the keyboard can open it.
    fn into_view(self) -> impl IntoView {
        let Self { parts, port, row_height, menu_target, label, class, attrs } = self;
        let state = parts.state;
        let cursor_row = parts.cursor_row;
        let menu_request = parts.menu_request;

        let count = Signal::derive_local(move || state.rows.with(FlatRows::len));
        let seen = Virtualize::new(port, count, Signal::derive_local(move || row_height.get().0), 4);
        let window = Signal::derive_local(move || seen.window());

        let keyboard = Keyboard {
            state,
            port,
            row_height,
            on_activate: parts.on_activate,
            place: ContextMenuPlace::current(),
            menu_target,
            cursor_row,
            typed: Typeahead::new(),
        };
        let on_key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
            if keyboard.key(&ev.key, ev.modifiers) {
                // Claimed, so an arrow does not also scroll whatever the tree is in.
                ev.prevent_default();
                ev.stop_propagation();
            }
        };

        // Not for a pointer: the press that focused the tree is about to move the cursor itself,
        // and scrolling first would move the row out from under it.
        let on_focus_in = move |ev: &mut EventCx<'_, events::FocusIn>| {
            let entered = state.enter();
            if ev.cause == FocusCause::Keyboard
                && let Some(index) = entered
            {
                let count = state.rows.with_untracked(FlatRows::len);
                bring_into_view(port, count, row_height.get_untracked(), index);
            }
        };

        // A secondary press that reached here without passing a row is on the empty space below
        // the rows, which has no menu.
        let claim_menu = move || {
            let Some(request) = menu_request else {
                return true;
            };
            let Some(key) = request.try_update(Option::take).flatten() else {
                return false;
            };
            menu_target.set(Some(key));

            true
        };
        let on_pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button == Some(PointerButton::Secondary) && !claim_menu() {
                ev.stop_propagation();
            }
        };
        let on_context_menu = move |ev: &mut EventCx<'_, events::ContextMenu>| {
            if !claim_menu() {
                ev.stop_propagation();
            }
        };

        let multiple = state.mode == SelectionMode::Multiple;
        let mut semantics = A11yBinding::new(Role::Tree).step(move |a11y| {
            let a11y = a11y.flag(SemanticFlags::MULTISELECTABLE, multiple);
            match cursor_row.get().and_then(|row| row.get()) {
                Some(node) => a11y.active_descendant(zgui::vocab::NodeId(node.as_u64())),
                None => a11y,
            }
        });
        if let Some(label) = label {
            semantics = semantics.label(label);
        }
        let own = Attrs::new()
            .custom_property(CustomPropertyName::new("lui-tree-row"), move || {
                row_height.try_get().map(|height| format!("{}px", height.0))
            })
            .a11y_from(semantics);

        view! {
            scroll(
                node_ref = port,
                class = "lui-tree",
                tabindex = Focus::Sequential,
                on:key_down = on_key_down,
                on:pointer_down = on_pointer_down,
                on:focus_in = on_focus_in,
                on:context_menu = on_context_menu,
                {..own},
                {..attrs},
                class = class
            ) {
                box(
                    class = "lui-tree__pane",
                    style:padding-top = move || window.try_get().map(|seen| format!("{}px", seen.lead)),
                    style:padding-bottom = move || window.try_get().map(|seen| format!("{}px", seen.trail))
                ) {
                    for spec in move || visible_rows(state, window.get()), key = |spec: &RowSpec<M::Key>| spec.clone() {
                        {tree_row(spec, Rc::clone(&parts))}
                    }
                }
            }
        }
    }
}

/// The rows of `window` that exist among the visible rows.
fn visible_rows<M: TreeModel>(state: TreeState<M>, window: VirtualWindow) -> Vec<RowSpec<M::Key>> {
    state.rows.with(|rows| {
        let end = window.end().min(rows.len());

        (window.first.min(end)..end)
            .filter_map(|index| {
                let index = RowIndex(index);
                rows.get(index).map(|row| RowSpec { index, row: row.clone() })
            })
            .collect()
    })
}

/// What the tree's keyboard handler holds.
struct Keyboard<M: TreeModel> {
    state: TreeState<M>,
    port: NodeRef,
    row_height: Signal<RowHeight, LocalStorage>,
    on_activate: Option<UnsyncCallback<M::Key>>,
    place: Option<ContextMenuPlace>,
    menu_target: RwSignal<Option<M::Key>, LocalStorage>,
    cursor_row: RwSignal<Option<NodeRef>, LocalStorage>,
    typed: Typeahead,
}

impl<M: TreeModel> Keyboard<M> {
    /// Carries out what `key` asks, and says whether it asked for anything.
    fn key(&self, key: &Key, modifiers: Modifiers) -> bool {
        let state = self.state;
        let Some(command) = keys::command(key, modifiers) else {
            return self.type_ahead(key, modifiers);
        };

        let landed = match command {
            Command::Move(travel) => state.travel(travel, modifiers, self.page()),
            Command::Open => state.open(),
            Command::Close => state.close(),
            Command::Activate => {
                if let (Some(activate), Some(index)) = (self.on_activate, state.cursor_index())
                    && let Some(key) = state.key_at(index)
                {
                    activate.run(key);
                }
                None
            }
            Command::Select => {
                let index = state.cursor_index().unwrap_or(RowIndex(0));
                let key = state.key_at(index);
                match key {
                    Some(key) if modifiers.primary() => state.toggle_selected(key),
                    Some(key) => state.select_only(key),
                    None => {}
                }
                None
            }
            Command::SelectAll => {
                if state.mode == SelectionMode::Single {
                    return false;
                }
                state.select_all();
                None
            }
            Command::OpenMenu => return self.open_menu(),
        };

        if let Some(index) = landed {
            self.bring_into_view(index);
        }

        true
    }

    fn type_ahead(&self, key: &Key, modifiers: Modifiers) -> bool {
        if modifiers.primary() || modifiers.alt() {
            return false;
        }
        let Some(text) = key.inserted_text().filter(|text| !text.trim().is_empty()) else {
            return false;
        };

        let typed = self.typed.push(text);
        // A longer prefix may still describe the row it found with a shorter one.
        let search = self.typed.search();
        let from_cursor = typed.chars().count() > 1 && !self.typed.is_cycling();
        let Some(index) = self.state.find(&search, from_cursor) else {
            return false;
        };
        self.state.land(index, Modifiers::NONE);
        self.bring_into_view(index);

        true
    }

    fn open_menu(&self) -> bool {
        let state = self.state;
        let Some(place) = self.place else {
            return false;
        };
        let Some(key) = state.cursor_index().and_then(|index| state.key_at(index)) else {
            return false;
        };
        let Some(row) = self.cursor_row.get_untracked() else {
            return false;
        };
        let Some(bounds) = row.window_bounds() else {
            return false;
        };

        if !state.selection.contains_untracked(&key) {
            state.select_only(key.clone());
        }
        self.menu_target.set(Some(key));

        // Below the row, so the menu does not cover the row it acts on.
        let scale = row.scale();
        let at: Point<CssPx, Css> =
            Point::new(CssPx(bounds.origin.x.0 / scale), CssPx((bounds.origin.y.0 + bounds.size.height.0) / scale));
        place.open_at(at);

        true
    }

    /// How many rows one page key moves: one fewer than the port shows, so a row of context
    /// stays in view.
    fn page(&self) -> usize {
        let height = self.row_height.get_untracked().0;
        let port = self.port.scroll_position().scrollport.height.0 / self.port.scale();
        let shown = if height > 0.0 { (port / height).floor() as usize } else { 0 };

        shown.saturating_sub(1).max(1)
    }

    fn bring_into_view(&self, index: RowIndex) {
        let count = self.state.rows.with_untracked(FlatRows::len);
        bring_into_view(self.port, count, self.row_height.get_untracked(), index);
    }
}

/// Builds one visible row.
fn tree_row<M, V, F>(spec: RowSpec<M::Key>, parts: Rc<RowParts<M, F>>) -> AnyView
where
    M: TreeModel,
    V: IntoView + 'static,
    F: Fn(TreeRow<M::Key>) -> V + 'static,
{
    let RowSpec { index, row: FlatRow { key, depth, kind, place } } = spec;
    let state = parts.state;
    let anchor = NodeRef::new();

    let expanded = {
        let key = key.clone();
        Signal::derive_local(move || state.expanded.contains(&key))
    };
    let selected = {
        let key = key.clone();
        Signal::derive_local(move || state.selection.contains(&key))
    };
    let focused = {
        let key = key.clone();
        Signal::derive_local(move || state.cursor.with(|cursor| cursor.as_ref() == Some(&key)))
    };

    // The tree names the cursor's row to a screen reader, and opens a menu from the keyboard
    // beside it.
    let cursor_row = parts.cursor_row;
    let pointing = RenderEffect::new(move |_| {
        if focused.get() && anchor.get().is_some() && cursor_row.get_untracked() != Some(anchor) {
            cursor_row.set(Some(anchor));
        }
    });
    on_cleanup_local(move || {
        drop(pointing);
        if cursor_row.try_get_untracked().flatten() == Some(anchor) {
            cursor_row.try_set(None);
        }
    });

    let striped = match parts.stripes {
        TreeStripes::None => false,
        TreeStripes::Rows => index.is_striped(),
        TreeStripes::Leaves => kind == NodeKind::Leaf && index.is_striped(),
    };
    let branch = kind == NodeKind::Branch;
    let mut semantics = A11yBinding::new(Role::TreeItem)
        .level(depth.0 + 1)
        .selected(selected)
        .step(move |a11y| a11y.set_position(place.position, place.count));
    if branch {
        semantics = semantics.expanded(expanded);
    }
    let own = Attrs::new()
        .class_toggle(ClassName::new("lui-tree__row"), true)
        .class_toggle(ClassName::new(STRIPE_CLASS), striped)
        .attribute(AttrName::new("data-selected"), move || Some(selected.get().to_string()))
        .attribute(AttrName::new("data-cursor"), move || Some(focused.get().to_string()))
        .attribute(AttrName::new("data-expanded"), move || branch.then(|| expanded.get().to_string()))
        .a11y_from(semantics);

    let pressed = key.clone();
    let toggles = branch && parts.branch_click == BranchClick::SelectAndToggle;
    let on_click = move |ev: &mut EventCx<'_, events::Click>| {
        state.press(pressed.clone(), ev.modifiers);
        if toggles && ev.modifiers.is_empty() {
            state.toggle_expanded(pressed.clone());
        }
    };

    let activated = key.clone();
    let on_activate = parts.on_activate;
    let on_double_click = move |_: &mut EventCx<'_, events::DoubleClick>| {
        if let Some(activate) = on_activate {
            activate.run(activated.clone());
        }
    };

    let asked = key.clone();
    let menu_request = parts.menu_request;
    let on_pointer_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button == Some(PointerButton::Secondary)
            && let Some(request) = menu_request
        {
            ask_menu(state, request, &asked);
        }
    };
    // A long press asks with this event alone, with no secondary press before it.
    let long_pressed = key.clone();
    let on_context_menu = move |_: &mut EventCx<'_, events::ContextMenu>| {
        if let Some(request) = menu_request {
            ask_menu(state, request, &long_pressed);
        }
    };

    let indent = if parts.guides {
        AnyView::new(view! { IndentGuides(depth = depth, indent = parts.indent) })
    } else {
        let width = format!("{}px", depth.0 as f32 * parts.indent.0);
        AnyView::new(view! { box(class = "lui-tree__spacer", style:width = width, a11y:hidden = true) })
    };
    let lead = match (kind, parts.leaf_lead) {
        (NodeKind::Branch, _) => {
            let toggled = key.clone();
            AnyView::new(view! {
                box(
                    class = "lui-tree__chevron",
                    a11y:hidden = true,
                    on:click = move |ev: &mut EventCx<'_, events::Click>| {
                        ev.stop_propagation();
                        state.toggle_expanded(toggled.clone());
                    },
                    on:double_click = move |ev: &mut EventCx<'_, events::DoubleClick>| ev.stop_propagation()
                ) {
                    Icon(icon = CHEVRON_RIGHT, size = IconSize::Sm)
                }
            })
        }
        (NodeKind::Leaf, LeafLead::Space) => {
            AnyView::new(view! { box(class = "lui-tree__spacer", a11y:hidden = true) })
        }
        (NodeKind::Leaf, LeafLead::Nothing) => AnyView::new(()),
    };

    let content = (parts.slot)(TreeRow { key, index, depth, kind, expanded, selected, focused, anchor });

    AnyView::new(view! {
        box(
            node_ref = anchor,
            on:click = on_click,
            on:double_click = on_double_click,
            on:pointer_down = on_pointer_down,
            on:context_menu = on_context_menu,
            {..own}
        ) {
            {indent}
            {lead}
            box(class = "lui-tree__content") {{content}}
        }
    })
}

/// Records `key` as the row a menu was asked for over, after making it the cursor and, unless
/// it is already among them, the selection.
fn ask_menu<M: TreeModel>(state: TreeState<M>, request: RwSignal<Option<M::Key>, LocalStorage>, key: &M::Key) {
    if state.selection.contains_untracked(key) {
        state.cursor.set(Some(key.clone()));
    } else {
        state.select_only(key.clone());
    }
    request.set(Some(key.clone()));
}

/// Keeps the visible rows in step with the model and the expanded set.
fn follow_model<M: TreeModel>(state: TreeState<M>) -> RenderEffect<(u64, HashSet<M::Key>)> {
    let model = state.model;
    // Counts the model's changes, so a change of the expanded set alone can be followed by
    // splicing rather than by walking the tree again.
    let generation = Memo::new(move |previous: Option<&u64>| {
        model.track();
        previous.map_or(0, |seen| seen + 1)
    });

    RenderEffect::new(move |previous: Option<(u64, HashSet<M::Key>)>| {
        let generation = generation.get();
        // A set whose scope is gone is being torn down with the tree, which then shows nothing.
        let after = state.expanded.snapshot().unwrap_or_default();
        model.with_untracked(|model| match previous {
            Some((seen, before)) if seen == generation => {
                state.rows.update(|rows| rows.follow(model, &before, &after));
            }
            _ => state.rows.set(FlatRows::build(model, &after)),
        });
        state.settle_cursor();

        (generation, after)
    })
}

/// Carries out the requests made through `handle`.
fn follow_handle<M: TreeModel>(
    state: TreeState<M>,
    handle: TreeHandle<M::Key>,
    port: NodeRef,
    row_height: Signal<RowHeight, LocalStorage>,
) -> RenderEffect<()> {
    RenderEffect::new(move |_| {
        let Some(key) = handle.request.get() else {
            return;
        };

        // Read with tracking, so a request waiting on its ancestors to open runs again once their
        // rows are in.
        let found = state.rows.with(|rows| rows.position(&key).map(|index| (index, rows.len())));
        if let Some((index, count)) = found {
            handle.request.set(None);
            bring_into_view(port, count, row_height.get_untracked(), index);
            return;
        }

        let closed = state.closed_ancestors(&key);
        if closed.is_empty() {
            // Every ancestor is open and the row is still not there: the model has no such node.
            handle.request.set(None);
            return;
        }
        state.expanded.update(move |set| set.extend(closed));
    })
}

/// Scrolls `port`, holding `count` rows of `height`, the least distance that shows the row at
/// `index` whole.
fn bring_into_view(port: NodeRef, count: usize, height: RowHeight, index: RowIndex) {
    let layout = zgui_ui::virtualize::window(count, height.0, 0.0, 0.0, 0);
    let scale = port.scale();
    let top = layout.offset_of(index.0) * scale;
    let bottom = top + height.0 * scale;

    let position = port.scroll_position();
    let start = position.offset.y.0;
    let span = position.scrollport.height.0;
    let target = if top < start {
        top
    } else if bottom > start + span {
        bottom - span
    } else {
        return;
    };

    port.scroll_to(ScrollTarget::Offset(Point::new(position.offset.x, DevicePx(target))), ScrollBehavior::Instant);
}
