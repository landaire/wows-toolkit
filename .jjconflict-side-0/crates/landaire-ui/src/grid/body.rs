use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;
use std::time::Duration;

use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::AttrName;
use zgui::view::Attrs;
use zgui::view::EventCx;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::TimeoutHandle;
use zgui::view::events;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;

use crate::grid::column::ColumnId;
use crate::grid::header;
use crate::grid::mode::RowDetail;
use crate::grid::selection::GridSelectionMode;
use crate::grid::state::GridCx;
use crate::grid::state::MenuTarget;
use crate::grid::state::Step;
use crate::motion::Movement;
use crate::surface::RowIndex;

/// The rows: every one of them, or with a fixed row height only those in view and two spacers
/// standing in for the rest.
pub(crate) fn body<T, K>(cx: Rc<GridCx<T, K>>) -> AnyView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let order = cx.order;

    let Some(window) = cx.window else {
        return AnyView::new(view! {
            for key in move || order.get().to_vec(), key = |key: &K| key.clone() {
                {body_row(Rc::clone(&cx), key)}
            }
        });
    };

    AnyView::new(view! {
        box(
            class = "lui-grid__spacer",
            a11y:hidden = true,
            var:--lui-grid-spacer = move || Some(format!("{}px", window.get().lead))
        )
        for key in move || {
            let window = window.get();
            order.with(|order| {
                let end = window.end().min(order.len());
                order[window.first.min(end)..end].to_vec()
            })
        }, key = |key: &K| key.clone() {
            {body_row(Rc::clone(&cx), key)}
        }
        box(
            class = "lui-grid__spacer",
            a11y:hidden = true,
            var:--lui-grid-spacer = move || Some(format!("{}px", window.get().trail))
        )
    })
}

fn body_row<T, K>(cx: Rc<GridCx<T, K>>, key: K) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let positions = cx.positions;
    let index = {
        let key = key.clone();
        Signal::derive_local(move || positions.with(|positions| positions.get(&key).copied()))
    };

    // The row's value, written only when it differs, so a sort that moves the row rebuilds none
    // of its cells. The positions are written after the rows, so a change to the rows is seen
    // here through them.
    let rows = cx.rows;
    let value: RwSignal<Option<T>, LocalStorage> = RwSignal::new_local(None);
    let following = RenderEffect::new(move |_| {
        let next = index.get().and_then(|index| rows.with_untracked(|rows| rows.get(index).cloned()));
        if value.with_untracked(|current| *current != next) {
            value.set(next);
        }
    });

    let anchor = NodeRef::new();
    cx.row_cells.borrow_mut().insert(key.clone(), anchor);
    {
        let cx = Rc::clone(&cx);
        let key = key.clone();
        on_cleanup_local(move || {
            drop(following);
            let mut cells = cx.row_cells.borrow_mut();
            if cells.get(&key) == Some(&anchor) {
                cells.remove(&key);
            }
        });
    }

    let selection = cx.selection;
    let cursor = cx.cursor;
    let expanded = cx.expanded;
    let selected = {
        let key = key.clone();
        move || selection.with(|selection| selection.contains(&key))
    };
    let mut semantics = A11yBinding::new(Role::Row).step(move |a11y| match index.get() {
        // The header is the first row.
        Some(index) => a11y.row_index(index + 1),
        None => a11y,
    });
    if cx.selection_mode != GridSelectionMode::None {
        semantics = semantics.selected(selected.clone());
    }
    if cx.detail.is_some() {
        let key = key.clone();
        semantics = semantics.expanded(move || expanded.with(|expanded| expanded.contains(&key)));
    }
    let own = {
        let on_cursor = key.clone();
        Attrs::new()
            .class_toggle(zgui::view::ClassName::new("lui-grid__row"), true)
            .attribute(AttrName::new("data-stripe"), move || {
                index.get().is_some_and(|index| RowIndex(index).is_striped()).then(String::new)
            })
            .attribute(AttrName::new("data-selected"), move || selected().then(String::new))
            .attribute(AttrName::new("data-cursor"), move || {
                cursor.with(|cursor| cursor.as_ref() == Some(&on_cursor)).then(String::new)
            })
            .a11y_from(semantics)
    };

    let on_pointer_down = {
        let cx = Rc::clone(&cx);
        let key = key.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| match ev.button {
            Some(PointerButton::Primary) => cx.press_row(&key, ev.modifiers),
            Some(PointerButton::Secondary) => {
                if !cx.selection.with_untracked(|selection| selection.contains(&key)) {
                    cx.press_row(&key, Modifiers::NONE);
                }
                cx.cursor.set(Some(key.clone()));
                if cx.row_menu.is_some() {
                    cx.menu_target.set(Some(MenuTarget::Row(key.clone())));
                    cx.menu_claimed.set(true);
                }
            }
            _ => {}
        }
    };
    let on_context_menu = {
        let cx = Rc::clone(&cx);
        let key = key.clone();
        move |_: &mut EventCx<'_, events::ContextMenu>| {
            if cx.row_menu.is_some() {
                cx.menu_target.set(Some(MenuTarget::Row(key.clone())));
                cx.menu_claimed.set(true);
            }
        }
    };
    let on_double_click = {
        let cx = Rc::clone(&cx);
        let key = key.clone();
        move |ev: &mut EventCx<'_, events::DoubleClick>| {
            if !ev.modifiers.primary() && !ev.modifiers.shift() {
                cx.activate(&key);
            }
        }
    };

    let ids = cx.drawn_ids;
    let detail =
        cx.detail.clone().map(|detail| AnyView::new(detail_region(Rc::clone(&cx), key.clone(), value, detail)));

    view! {
        box(
            on:pointer_down = on_pointer_down,
            on:context_menu = on_context_menu,
            on:double_click = on_double_click,
            {..own}
        ) {
            for id in move || ids.get(), key = |id: &ColumnId| id.clone() {
                {body_cell(Rc::clone(&cx), id, value)}
            }
            box(class = "lui-grid__filler", node_ref = anchor, a11y:hidden = true) {}
            {detail}
        }
    }
}

fn body_cell<T, K>(cx: Rc<GridCx<T, K>>, id: ColumnId, value: RwSignal<Option<T>, LocalStorage>) -> AnyView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let Some(column) = cx.column(&id).cloned() else {
        return AnyView::new(());
    };
    let position = cx.drawn_ids.with_untracked(|ids| ids.iter().position(|other| *other == id));

    let pins = cx.pins;
    let carried_column = cx.carried;
    let shifts = cx.shifts;
    let ids = cx.drawn_ids;
    let own = {
        let pinned = id.clone();
        let left = id.clone();
        let edge = id.clone();
        let carried = id.clone();
        let placed = id.clone();
        let mut semantics = A11yBinding::new(Role::Cell);
        if let Some(position) = position {
            semantics = semantics.step(move |a11y| {
                let index = ids.with(|ids| ids.iter().position(|other| *other == placed)).unwrap_or(position);
                a11y.column_index(index)
            });
        }
        Attrs::new()
            .class_toggle(zgui::view::ClassName::new("lui-grid__cell"), true)
            .attribute(AttrName::new("data-align"), Some(column.align_value().attribute().to_owned()))
            .attribute(AttrName::new("data-pinned"), move || {
                pins.with(|pins| pins.left_of(&pinned).is_some()).then(String::new)
            })
            .attribute(AttrName::new("data-pin-edge"), move || pins.with(|pins| pins.is_edge(&edge)).then(String::new))
            .attribute(AttrName::new("data-carried"), move || {
                carried_column.with(|column| column.as_ref() == Some(&carried)).then(String::new)
            })
            .custom_property(zgui::view::CustomPropertyName::new("lui-grid-left"), move || {
                pins.with(|pins| pins.left_of(&left)).map(|left| format!("{left}px"))
            })
            .a11y_from(semantics)
    };
    let shift = move || shifts.with(|shifts| shifts.get(&id).map(|shift| format!("{shift}px 0px")));

    AnyView::new(view! {
        box(style:translate = shift, {..own}) {
            {move || value.with(|row| row.as_ref().map(|row| column.cell_view(row)))}
        }
    })
}

/// The region under a row: built while the row is expanded and while it closes, and revealed by
/// animating its height to that of its content.
fn detail_region<T, K>(
    cx: Rc<GridCx<T, K>>,
    key: K,
    value: RwSignal<Option<T>, LocalStorage>,
    detail: RowDetail<T>,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    // How long a close is given before its content is taken down, a frame more than the reveal
    // itself so the last frame of it is drawn.
    const CLOSE_AFTER: Duration = Duration::from_millis(Movement::Reveal.full_duration().as_millis() as u64 + 20);

    let expanded = cx.expanded;
    let open = {
        let key = key.clone();
        Signal::derive_local(move || expanded.with(|expanded| expanded.contains(&key)))
    };
    let built = RwSignal::new_local(open.get_untracked());
    let closing: Rc<RefCell<Option<TimeoutHandle>>> = Rc::new(RefCell::new(None));
    let timers = cx.timers.clone();
    let following = {
        let closing = Rc::clone(&closing);
        RenderEffect::new(move |_| {
            if open.get() {
                closing.replace(None);
                built.set(true);
                return;
            }
            if !built.get_untracked() {
                return;
            }

            match timers.as_ref() {
                Some(timers) => {
                    let handle = timers.set_timeout(CLOSE_AFTER, move || built.set(false));
                    closing.replace(Some(handle));
                }
                None => built.set(false),
            }
        })
    };
    on_cleanup_local(move || {
        drop(following);
        closing.replace(None);
    });

    view! {
        if move || built.get() {
            {detail_body(open, value, Rc::clone(&detail), cx.view_width)}
        } else {}
    }
}

fn detail_body<T>(
    open: Signal<bool, LocalStorage>,
    value: RwSignal<Option<T>, LocalStorage>,
    detail: RowDetail<T>,
    view_width: Signal<Option<String>, LocalStorage>,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
{
    let inner = NodeRef::new();
    let measured = inner.observe_border_size();
    let scale = move || {
        let scale = inner.scale();
        if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
    };

    // Opened only once the content has been measured, so the first frame it is open already has
    // a height to animate to.
    let state = move || {
        let shown = open.get() && measured.get().is_some();
        Some(if shown { "open" } else { "closed" }.to_owned())
    };
    let height = move || measured.get().map(|size| format!("{}px", (size.height.0 / scale()).ceil()));

    view! {
        box(
            class = "lui-grid__detail",
            attr:data-state = state,
            var:--lui-grid-detail-height = height,
            var:--lui-grid-view = move || view_width.get(),
            a11y:hidden = move || !open.get()
        ) {
            box(class = "lui-grid__detail-body", node_ref = inner) {
                {move || value.with(|row| row.as_ref().map(|row| detail(row)))}
            }
        }
    }
}

/// What a key pressed on the grid itself does.
pub(crate) fn key_down<T, K>(cx: &GridCx<T, K>, ev: &mut EventCx<'_, events::KeyDown>)
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    // A key pressed on a header or a control inside a cell is that element's.
    if ev.target != ev.current {
        return;
    }

    let modifiers = ev.modifiers;
    let cursor = cx.cursor.get_untracked();
    if header::opens_menu(&ev.key, modifiers) {
        let anchor = cursor.as_ref().and_then(|key| cx.row_cells.borrow().get(key).copied());
        if let (Some(key), Some(anchor), Some(_)) = (cursor, anchor, cx.row_menu.as_ref()) {
            header::open_menu_at(anchor, MenuTarget::Row(key), cx);
        }
        ev.prevent_default();
        return;
    }

    let letter = ev.key_without_modifiers.as_str().map(str::to_ascii_lowercase);
    match (&ev.key, letter.as_deref()) {
        (Key::Named(NamedKey::ArrowDown), _) => cx.step_cursor(Step::Next, modifiers),
        (Key::Named(NamedKey::ArrowUp), _) => cx.step_cursor(Step::Previous, modifiers),
        (Key::Named(NamedKey::Home), _) => cx.step_cursor(Step::First, modifiers),
        (Key::Named(NamedKey::End), _) => cx.step_cursor(Step::Last, modifiers),
        (Key::Named(NamedKey::Enter), _) => {
            if let Some(key) = cursor {
                cx.activate(&key);
            }
        }
        (Key::Named(NamedKey::Space), _) => {
            let Some(key) = cursor else {
                return;
            };
            match cx.selection_mode {
                GridSelectionMode::Multiple => cx.selection.update(|selection| selection.toggle(key)),
                GridSelectionMode::Single | GridSelectionMode::None => cx.press_row(&key, Modifiers::NONE),
            }
        }
        (Key::Named(NamedKey::ArrowRight), _) => {
            if let Some(key) = cursor {
                cx.set_expanded(&key, true);
            }
        }
        (Key::Named(NamedKey::ArrowLeft), _) => {
            if let Some(key) = cursor {
                cx.set_expanded(&key, false);
            }
        }
        (_, Some("a")) if modifiers.primary() && cx.selection_mode == GridSelectionMode::Multiple => {
            let order = cx.order.get_untracked();
            cx.selection.update(|selection| selection.select_all(&order));
        }
        (_, Some("c")) if modifiers.primary() => cx.copy(),
        _ => return,
    }

    ev.prevent_default();
    ev.stop_propagation();
}
