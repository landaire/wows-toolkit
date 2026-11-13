use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use zgui::component;
use zgui::elements::Focus;
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
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::runtime::clipboard::try_use_clipboard;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::Timers;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuPlace;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::virtualize::MeasuredRows;
use zgui_ui::virtualize::MeasuredVirtualize;
use zgui_ui::virtualize::Virtualize;

use crate::grid::body;
use crate::grid::column::GridColumn;
use crate::grid::header;
use crate::grid::layout::ColumnLayout;
use crate::grid::mode::Building;
use crate::grid::mode::RowMark;
use crate::grid::mode::RowMode;
use crate::grid::mode::RowParts;
use crate::grid::selection::GridSelectionMode;
use crate::grid::selection::Selection;
use crate::grid::sort::ColumnSort;
use crate::grid::state;
use crate::grid::state::Carry;
use crate::grid::state::GridCx;
use crate::grid::state::GridStrings;
use crate::grid::state::MenuTarget;
use crate::grid::state::Pins;
use crate::grid::state::RowHover;
use crate::grid::state::RowMenu;
use crate::grid::state::RowMenuHandle;
use crate::grid::style;
use crate::motion::motion_preference;

/// A table of keyed rows under a header that sorts, resizes, reorders and pins its columns.
///
/// The header and every row scroll sideways together under one scroll position, and the pinned
/// columns hold still at the leading edge while the rest pass under them. The application owns
/// the rows and their order: a press on a sortable header asks for a sort through `on_sort`, and
/// the rows that come back are shown in the order given.
///
/// A row's cells are built once for its key and again when its value changes, which is why `T`
/// is compared.
///
/// # Keyboard
///
/// The grid is one tab stop, and the arrows, Home and End move along the rows, selecting as they
/// go. Shift extends a run and ctrl moves without selecting when several rows may be selected,
/// and Space adds or takes out the row. Enter and a double click activate a row, and the right and
/// left arrows open and close its detail. Ctrl+C copies the selected rows as tab-separated text.
/// Each header is a tab stop that sorts on Enter and moves its column with Alt and the arrows.
/// Each grip is a separator the arrows resize. The menu key opens the menu of the header or row
/// the keyboard is on.
// A component's props are its arguments.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn Grid<T, K, F>(
    /// Every row, in the order they are shown.
    #[prop(into)]
    rows: Signal<Vec<T>, LocalStorage>,
    columns: Vec<GridColumn<T>>,
    /// What identifies a row across changes to the rows.
    row_key: F,
    #[prop(default = RowMode::natural())] mode: RowMode<T>,
    /// The order, pins and widths of the columns, for an application that keeps them. Written
    /// once per completed drag, pin or reset.
    #[prop(optional)]
    layout: Option<RwSignal<ColumnLayout, LocalStorage>>,
    /// The sort the rows are in, shown on its header.
    #[prop(into, default = Signal::stored_local(None))]
    sort: Signal<Option<ColumnSort>, LocalStorage>,
    /// Told the sort a press on a sortable header asks for.
    #[prop(optional)]
    on_sort: Option<UnsyncCallback<Option<ColumnSort>>>,
    #[prop(default = GridSelectionMode::Single)] selection_mode: GridSelectionMode,
    /// The selected rows, for an application that reads or sets them.
    #[prop(optional)]
    selection: Option<RwSignal<Selection<K>, LocalStorage>>,
    /// The rows whose detail is open, for an application that reads or sets them.
    #[prop(optional)]
    expanded: Option<RwSignal<HashSet<K>, LocalStorage>>,
    /// Told the row a double click or Enter activated. Without it, activating a row with a detail
    /// opens or closes the detail.
    #[prop(optional)]
    on_activate: Option<UnsyncCallback<K>>,
    /// Told when the pointer comes onto a row and when it leaves it.
    #[prop(optional)]
    on_row_hover: Option<UnsyncCallback<RowHover<K>>>,
    /// Names each row's mark: a bar down the row's leading edge, its detail included, which the
    /// application's styles colour by the name under `.lui-grid__mark[data-mark]`.
    #[prop(optional)]
    row_mark: Option<RowMark<T>>,
    /// The items of the menu a row opens, for that row.
    #[prop(optional)]
    row_menu: Option<RowMenu<K>>,
    /// Opens the row menu from outside the grid and tells which row's menu is open.
    #[prop(optional)]
    row_menu_handle: Option<RowMenuHandle<K>>,
    /// Told the text a copy produces. Without it the text goes to the clipboard.
    #[prop(optional)]
    on_copy: Option<UnsyncCallback<String>>,
    #[prop(optional)] strings: Option<GridStrings>,
    /// Names the grid for a screen reader.
    #[prop(into)]
    label: String,
    /// Where to record the scrolling element.
    #[prop(optional)]
    node_ref: Option<NodeRef>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
    F: Fn(&T) -> K + 'static,
{
    install_stylesheet(style::SHEET, style::CSS);

    let known = state::known_ids(&columns);
    let layout = layout.unwrap_or_else(|| RwSignal::new_local(ColumnLayout::new(known.iter().cloned())));
    let reconciled = layout.with_untracked(|layout| layout.reconciled(&known));
    if layout.with_untracked(|layout| *layout != reconciled) {
        layout.set(reconciled);
    }

    let preview: RwSignal<Option<ColumnLayout>, LocalStorage> = RwSignal::new_local(None);
    let drawn = {
        let known = std::sync::Arc::clone(&known);
        Memo::new(move |_| match preview.get() {
            Some(preview) => preview,
            None => layout.with(|layout| layout.reconciled(&known)),
        })
    };
    let drawn_ids = Memo::new(move |_| drawn.with(|drawn| drawn.ids().cloned().collect::<Vec<_>>()));
    let widths = RwSignal::new_local(HashMap::new());
    let pins = Memo::new(move |_| {
        drawn.with(|drawn| {
            widths.with(|widths| {
                let mut left = 0.0;
                let mut pins = Vec::new();
                for column in drawn.columns().iter().take_while(|column| column.is_pinned()) {
                    pins.push((column.id().clone(), left));
                    left += widths.get(column.id()).copied().unwrap_or(0.0);
                }
                Pins::new(pins)
            })
        })
    });

    let port = node_ref.unwrap_or_default();
    let RowParts { detail, groups, building } = mode.into_parts();
    let fixed = match building {
        Building::Fixed { height, .. } => Some(height),
        Building::All | Building::Measured { .. } => None,
    };
    let row_key: Rc<dyn Fn(&T) -> K> = Rc::new(row_key);
    // A detail region and a group heading stay as wide as the view while the columns scroll.
    let follows_view = detail.is_some() || groups.is_some();
    let port_size = port.observe_border_size_while(move || follows_view);
    let view_width = Signal::derive_local(move || {
        let scale = port.scale();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        port_size.get().map(|size| format!("{}px", size.width.0 / scale))
    });
    let order: RwSignal<Rc<Vec<K>>, LocalStorage> = RwSignal::new_local(Rc::new(Vec::new()));
    let count = Signal::derive_local(move || order.with(|order| order.len()));
    let window = match building {
        Building::Fixed { height, overscan } => {
            let seen = Virtualize::new(port, count, height.0, overscan);
            Some(Signal::derive_local(move || seen.window()))
        }
        Building::All | Building::Measured { .. } => None,
    };
    let measured = match building {
        Building::Measured { estimate, overscan } => {
            // A measurement follows its row through a sort.
            let rows = MeasuredRows::new(estimate.0).keyed(move |index| order.with(|order| order.get(index).cloned()));
            Some(MeasuredVirtualize::new(port, count, rows, overscan))
        }
        Building::All | Building::Fixed { .. } => None,
    };
    let carry = RwSignal::new_local(None);
    let carried =
        Memo::new(move |_| carry.with(|carry: &Option<Carry>| carry.as_ref().and_then(Carry::moving).cloned()));

    let cx = Rc::new(GridCx {
        columns: columns.into(),
        row_key: Rc::clone(&row_key),
        rows,
        order,
        positions: RwSignal::new_local(Rc::new(HashMap::new())),
        layout,
        preview,
        drawn,
        drawn_ids,
        pins,
        carry,
        carried,
        press_answered: RwSignal::new_local(false),
        resize: RwSignal::new_local(None),
        widths,
        fitted: RwSignal::new_local(HashMap::new()),
        shifts: RwSignal::new_local(HashMap::new()),
        instant: RwSignal::new_local(false),
        slide: Rc::new(RefCell::new(None)),
        settle: Rc::new(RefCell::new(None)),
        motion: motion_preference(),
        sort,
        on_sort,
        selection_mode,
        selection: selection.unwrap_or_else(|| RwSignal::new_local(Selection::new())),
        cursor: RwSignal::new_local(None),
        expanded: expanded.unwrap_or_else(|| RwSignal::new_local(HashSet::new())),
        detail,
        groups,
        row_mark,
        fixed,
        window,
        measured,
        on_activate,
        on_row_hover,
        row_menu,
        menu_target: RwSignal::new_local(None),
        menu_claimed: Cell::new(false),
        menu_place: Cell::new(None),
        on_copy,
        clipboard: try_use_clipboard(),
        strings: strings.unwrap_or_default(),
        port,
        scroll: port.observe_scroll(),
        row_cells: RefCell::new(HashMap::new()),
        header_height: RwSignal::new_local(0.0),
        view_width,
        timers: Timers::current(),
    });

    // The keys and their indices are written after the rows change, and a fitted column may then
    // be narrower than the widest it has been. The keyboard leaves a row that is gone.
    let indexing = {
        let positions = cx.positions;
        let fitted = cx.fitted;
        let cursor = cx.cursor;
        RenderEffect::new(move |_| {
            let keys: Vec<K> = rows.with(|rows| rows.iter().map(|row| row_key(row)).collect());
            let indices: HashMap<K, usize> = keys.iter().enumerate().map(|(index, key)| (key.clone(), index)).collect();
            if cursor.with_untracked(|cursor| cursor.as_ref().is_some_and(|key| !indices.contains_key(key))) {
                cursor.set(None);
            }
            positions.set(Rc::new(indices));
            order.set(Rc::new(keys));
            fitted.set(HashMap::new());
        })
    };
    // A layout the application sets later may name another set of columns.
    let reconciling = {
        let known = std::sync::Arc::clone(&known);
        RenderEffect::new(move |_| {
            let reconciled = layout.with(|layout| layout.reconciled(&known));
            if layout.with_untracked(|layout| *layout != reconciled) {
                layout.set(reconciled);
            }
        })
    };
    on_cleanup_local(move || {
        drop(indexing);
        drop(reconciling);
    });

    let menu_open = RwSignal::new_local(false);
    // A menu for a row that is gone has nothing left to act on.
    let closing = {
        let positions = cx.positions;
        let menu_target = cx.menu_target;
        RenderEffect::new(move |_| {
            let gone = positions.with(|positions| {
                menu_target.with_untracked(
                    |target| matches!(target, Some(MenuTarget::Row(key)) if !positions.contains_key(key)),
                )
            });
            if gone && menu_open.get_untracked() {
                menu_open.set(false);
            }
        })
    };
    on_cleanup_local(move || drop(closing));
    if let Some(handle) = row_menu_handle {
        let opener = {
            let cx = Rc::clone(&cx);
            move |key: K, anchor: NodeRef| {
                if cx.row_menu.is_none() || !cx.positions.with_untracked(|positions| positions.contains_key(&key)) {
                    return;
                }
                cx.target_row(&key);
                header::open_menu_at(anchor, MenuTarget::Row(key), &cx);
            }
        };
        handle.attach(Rc::new(opener));
        let menu_target = cx.menu_target;
        let telling = RenderEffect::new(move |_| {
            let open = menu_open.get();
            let row = menu_target.with(|target| match target {
                Some(MenuTarget::Row(key)) if open => Some(key.clone()),
                _ => None,
            });
            handle.set_open_row(row);
        });
        on_cleanup_local(move || {
            drop(telling);
            handle.detach();
        });
    }

    let instant = cx.instant;
    let tracks = {
        let cx = Rc::clone(&cx);
        move || Some(cx.tracks())
    };
    let row_height = fixed.map(|height| format!("{}px", height.0));
    let column_count = cx.columns.len();
    let order = cx.order;
    let on_menu_press = {
        let cx = Rc::clone(&cx);
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button == Some(PointerButton::Secondary) && !cx.menu_claimed.replace(false) {
                ev.stop_propagation();
            }
        }
    };
    let on_menu_request = {
        let cx = Rc::clone(&cx);
        move |ev: &mut EventCx<'_, events::ContextMenu>| {
            if !cx.menu_claimed.replace(false) {
                ev.stop_propagation();
            }
        }
    };
    let semantics =
        A11yBinding::new(Role::Grid).label(label).table_size(move || order.with(|order| order.len()) + 1, column_count);
    let own = Attrs::new().a11y_from(semantics);

    let on_key_down = {
        let cx = Rc::clone(&cx);
        move |ev: &mut EventCx<'_, events::KeyDown>| body::key_down(&cx, ev)
    };
    let menu = {
        let cx = Rc::clone(&cx);
        move || match cx.menu_target.get() {
            Some(MenuTarget::Header(id)) => Some(header::header_menu(Rc::clone(&cx), id)),
            Some(MenuTarget::Row(key)) => cx.row_menu.as_ref().map(|menu| menu(key)),
            None => None,
        }
    };
    // Built inside the region the context menu is asked for over, which is where the menu's place
    // can be read.
    let region = {
        let cx = Rc::clone(&cx);
        move || {
            cx.menu_place.set(ContextMenuPlace::current());
            let header = header::header_row(Rc::clone(&cx));
            let body = body::body(Rc::clone(&cx));
            view! {
                scroll(
                    class = "lui-grid__port",
                    node_ref = port,
                    tabindex = Focus::Sequential,
                    on:key_down = on_key_down,
                    on:pointer_down = on_menu_press,
                    on:context_menu = on_menu_request,
                    {..own}
                ) {
                    box(class = "lui-grid__table", style:grid-template-columns = tracks) {
                        {header}
                        {body}
                    }
                }
            }
        }
    };
    let overlay = header::carry_overlay(Rc::clone(&cx));

    view! {
        box(
            class = "lui-grid",
            attr:data-instant = move || instant.get().then(String::new),
            attr:data-fixed = {fixed.map(|_| String::new())},
            var:--lui-grid-row = {row_height},
            {..attrs},
            class = class
        ) {
            ContextMenu(open = menu_open) {
                ContextMenuTrigger {
                    {region()}
                }
                ContextMenuContent {
                    {menu.clone()}
                }
            }
            {overlay}
        }
    }
}
