use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;
use std::time::Duration;

use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
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
use zgui_ui::virtualize::MeasuredVirtualize;

use crate::context_menu::MenuItems;
use crate::grid::column::ColumnId;
use crate::grid::header;
use crate::grid::mode::RowDetail;
use crate::grid::mode::RowGroups;
use crate::grid::selection::GridSelectionMode;
use crate::grid::state::GridCx;
use crate::grid::state::MenuTarget;
use crate::grid::state::RowHover;
use crate::grid::state::Step;
use crate::motion::MotionPreference;
use crate::motion::Movement;
use crate::surface::RowIndex;

/// The rows: every one of them, or in a virtualized grid only those in view and two spacers
/// standing in for the rest.
pub(crate) fn body<T, K>(cx: Rc<GridCx<T, K>>) -> AnyView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let order = cx.order;

    if let Some(seen) = cx.measured {
        let window = Signal::derive_local(move || seen.window());
        // Keyed by index as well as by key, since a row measures itself under its index.
        return AnyView::new(view! {
            box(
                class = "lui-grid__spacer",
                a11y:hidden = true,
                var:--lui-grid-spacer = move || Some(format!("{}px", window.get().lead))
            )
            for row in move || {
                let window = window.get();
                order.with(|order| {
                    let end = window.end().min(order.len());
                    let first = window.first.min(end);
                    order[first..end].iter().cloned().enumerate().map(|(at, key)| (first + at, key)).collect::<Vec<_>>()
                })
            }, key = |row: &(usize, K)| row.clone() {
                {body_row(Rc::clone(&cx), row.1, Some(row.0))}
            }
            box(
                class = "lui-grid__spacer",
                a11y:hidden = true,
                var:--lui-grid-spacer = move || Some(format!("{}px", window.get().trail))
            )
        });
    }

    let Some(window) = cx.window else {
        return AnyView::new(view! {
            for key in move || order.get().to_vec(), key = |key: &K| key.clone() {
                {body_row(Rc::clone(&cx), key, None)}
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
            {body_row(Rc::clone(&cx), key, None)}
        }
        box(
            class = "lui-grid__spacer",
            a11y:hidden = true,
            var:--lui-grid-spacer = move || Some(format!("{}px", window.get().trail))
        )
    })
}

/// One row. `measured_at` is the index a measured grid built the row for.
fn body_row<T, K>(cx: Rc<GridCx<T, K>>, key: K, measured_at: Option<usize>) -> impl IntoView
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
                let Some(items) = cx.row_menu_items(&key) else {
                    return;
                };
                cx.target_row(&key);
                cx.menu_target.set(Some(MenuTarget::Row(key.clone(), items)));
                cx.menu_claimed.set(true);
            }
            _ => {}
        }
    };
    let on_context_menu = {
        let cx = Rc::clone(&cx);
        let key = key.clone();
        move |_: &mut EventCx<'_, events::ContextMenu>| {
            if let Some(items) = cx.row_menu_items(&key) {
                cx.menu_target.set(Some(MenuTarget::Row(key.clone(), items)));
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
    let on_pointer_enter = {
        let cx = Rc::clone(&cx);
        let key = key.clone();
        move |_: &mut EventCx<'_, events::PointerEnter>| {
            if let Some(on_row_hover) = &cx.on_row_hover {
                on_row_hover.run(RowHover::Entered { key: key.clone(), anchor });
            }
        }
    };
    let on_pointer_leave = {
        let cx = Rc::clone(&cx);
        let key = key.clone();
        move |_: &mut EventCx<'_, events::PointerLeave>| {
            if let Some(on_row_hover) = &cx.on_row_hover {
                on_row_hover.run(RowHover::Left { key: key.clone() });
            }
        }
    };

    let ids = cx.drawn_ids;
    let detail_height = cx.measured.map(|_| RwSignal::new_local(0.0));
    let detail = cx
        .detail
        .clone()
        .map(|detail| AnyView::new(detail_region(Rc::clone(&cx), key.clone(), value, detail, detail_height)));
    let heading = cx.groups.clone().map(|groups| AnyView::new(group_heading(&cx, index, value, groups)));
    let gauge =
        cx.measured.zip(measured_at).map(|(seen, at)| AnyView::new(row_gauge(&cx, seen, at, anchor, detail_height)));

    view! {
        box(
            on:pointer_down = on_pointer_down,
            on:context_menu = on_context_menu,
            on:double_click = on_double_click,
            on:pointer_enter = on_pointer_enter,
            on:pointer_leave = on_pointer_leave,
            {..own}
        ) {
            {heading}
            for id in move || ids.get(), key = |id: &ColumnId| id.clone() {
                {body_cell(Rc::clone(&cx), id, value)}
            }
            box(class = "lui-grid__filler", node_ref = anchor, a11y:hidden = true) {}
            {detail}
            {gauge}
        }
    }
}

/// An unseen box as tall as its row, for a measured grid to measure: the row lays its line and
/// its detail region out in the grid's own tracks and has no box of its own.
fn row_gauge<T, K>(
    cx: &GridCx<T, K>,
    seen: MeasuredVirtualize,
    index: usize,
    line: NodeRef,
    detail_height: Option<RwSignal<f32, LocalStorage>>,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let gauge = NodeRef::new();
    seen.measure(index, gauge);

    let port = cx.port;
    let line = line.observe_border_size();
    let height = move || {
        let scale = port.scale();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let line = line.get()?.height.0 / scale;
        let detail = detail_height.map_or(0.0, |height| height.get());

        Some(format!("{}px", line + detail))
    };

    view! {
        box(class = "lui-grid__gauge", node_ref = gauge, a11y:hidden = true, style:height = height) {}
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

    let ids = cx.drawn_ids;
    let mut semantics = A11yBinding::new(Role::Cell);
    if let Some(position) = position {
        let at = id.clone();
        semantics = semantics.step(move |a11y| {
            let index = ids.with(|ids| ids.iter().position(|other| *other == at)).unwrap_or(position);
            a11y.column_index(index)
        });
    }
    let own = placed(&cx, &id, "lui-grid__cell")
        .attribute(AttrName::new("data-align"), Some(column.align_value().attribute().to_owned()))
        .a11y_from(semantics);
    let shift = shift(&cx, id.clone());
    let mark = row_mark(&cx, id, value);

    AnyView::new(view! {
        box(style:translate = shift, {..own}) {
            {mark}
            {move || value.with(|row| row.as_ref().map(|row| column.cell_view(row)))}
        }
    })
}

/// A box in `id`'s column with `class`: held at the leading edge while pinned, and shown carried
/// while its column is.
fn placed<T, K>(cx: &GridCx<T, K>, id: &ColumnId, class: &'static str) -> Attrs
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let pins = cx.pins;
    let carried_column = cx.carried;
    let pinned = id.clone();
    let left = id.clone();
    let edge = id.clone();
    let carried = id.clone();
    Attrs::new()
        .class_toggle(zgui::view::ClassName::new(class), true)
        .attribute(AttrName::new("data-held"), cx.held(id))
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
}

/// How far `id`'s column is drawn from its place while the columns slide.
fn shift<T, K>(cx: &GridCx<T, K>, id: ColumnId) -> impl Fn() -> Option<String> + Copy + 'static
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let shifts = cx.shifts;
    let id = StoredValue::new_local(id);
    move || shifts.with(|shifts| id.with_value(|id| shifts.get(id).map(|shift| format!("{shift}px 0px"))))
}

/// The row's mark, drawn in the box of whichever column is drawn first.
fn row_mark<T, K>(cx: &GridCx<T, K>, id: ColumnId, value: RwSignal<Option<T>, LocalStorage>) -> Option<AnyView>
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let mark = cx.row_mark.clone()?;
    let ids = cx.drawn_ids;
    let name = move || {
        if !ids.with(|ids| ids.first() == Some(&id)) {
            return None;
        }
        value.with(|row| row.as_ref().and_then(|row| mark(row))).map(str::to_owned)
    };

    Some(AnyView::new(view! { box(class = "lui-grid__mark", a11y:hidden = true, attr:data-mark = name) {} }))
}

/// The heading over a row that opens a run of its group, spanning the grid and held in view while
/// the columns scroll. Built from the row's own value, so it follows the row through a sort.
fn group_heading<T, K>(
    cx: &GridCx<T, K>,
    index: Signal<Option<usize>, LocalStorage>,
    value: RwSignal<Option<T>, LocalStorage>,
    groups: RowGroups<T>,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    // The positions are written after every change to the rows, so the rows are read untracked.
    let rows = cx.rows;
    let groups = StoredValue::new_local(groups);
    let starts = Memo::new(move |_| {
        let Some(index) = index.get() else {
            return false;
        };
        rows.with_untracked(|rows| {
            let previous = index.checked_sub(1).and_then(|previous| rows.get(previous));
            rows.get(index).is_some_and(|row| groups.with_value(|groups| groups.starts(previous, row)))
        })
    });
    let view_width = cx.view_width;
    // A press on the heading is not a press on the row it heads.
    let keep = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation();
    let keep_double = |ev: &mut EventCx<'_, events::DoubleClick>| ev.stop_propagation();
    let heading = move || value.with(|row| row.as_ref().map(|row| groups.with_value(|groups| groups.heading(row))));

    view! {
        if move || starts.get() {
            box(
                class = "lui-grid__group",
                on:pointer_down = keep,
                on:double_click = keep_double,
                a11y:role = Role::RowHeader
            ) {
                box(class = "lui-grid__group-body", var:--lui-grid-view = move || view_width.get()) {
                    {heading}
                }
            }
        } else {}
    }
}

/// What an expanded row shows: built while the row is expanded and while it closes, and revealed
/// by animating its height to that of its content. `reported` is told its height.
fn detail_region<T, K>(
    cx: Rc<GridCx<T, K>>,
    key: K,
    value: RwSignal<Option<T>, LocalStorage>,
    detail: RowDetail<T>,
    reported: Option<RwSignal<f32, LocalStorage>>,
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
    let motion = cx.motion;
    let following = {
        let closing = Rc::clone(&closing);
        RenderEffect::new(move |_| {
            let full = motion.get() == MotionPreference::Full;
            if open.get() {
                closing.replace(None);
                built.set(true);
                return;
            }
            if !built.get_untracked() {
                return;
            }

            // Under reduced motion the close is not drawn, and the columns it widened narrow with
            // it, a close already under way included.
            match timers.as_ref().filter(|_| full) {
                // The close under way runs on.
                Some(_) if closing.borrow().is_some() => {}
                Some(timers) => {
                    let handle = timers.set_timeout(CLOSE_AFTER, move || built.set(false));
                    closing.replace(Some(handle));
                }
                None => {
                    closing.replace(None);
                    built.set(false);
                }
            }
        })
    };
    on_cleanup_local(move || {
        drop(following);
        closing.replace(None);
    });

    view! {
        if move || built.get() {
            {match &detail {
                RowDetail::Spanning(detail) => {
                    AnyView::new(detail_body(open, value, Rc::clone(detail), cx.view_width, reported))
                }
                RowDetail::Columns(detail) => {
                    AnyView::new(column_details(Rc::clone(&cx), open, value, Rc::clone(detail), reported))
                }
            }}
        } else {}
    }
}

/// Each column's part of an open row, in a line of the grid's own tracks under the row's cells.
/// Every part is as tall as the tallest, and the line reveals to that height.
fn column_details<T, K>(
    cx: Rc<GridCx<T, K>>,
    open: Signal<bool, LocalStorage>,
    value: RwSignal<Option<T>, LocalStorage>,
    detail: ColumnDetail<T>,
    reported: Option<RwSignal<f32, LocalStorage>>,
) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let parts: RwSignal<HashMap<ColumnId, f32>, LocalStorage> = RwSignal::new_local(HashMap::new());
    let tallest = Memo::new(move |_| parts.with(|parts| parts.values().copied().reduce(f32::max)));
    let line = DetailLine {
        open,
        height: Signal::derive_local(move || tallest.get().map(|height| format!("{}px", height.ceil()))),
        state: Signal::derive_local(move || {
            let shown = open.get() && tallest.get().is_some();
            Some(if shown { "open" } else { "closed" }.to_owned())
        }),
    };

    let filler = NodeRef::new();
    if let Some(reported) = reported {
        report_height(filler, reported);
    }
    // A fitted column widened to an open part narrows again once the part is gone.
    let fitted = cx.fitted;
    on_cleanup_local(move || {
        fitted.try_update(HashMap::clear);
    });

    let ids = cx.drawn_ids;
    view! {
        for id in move || ids.get(), key = |id: &ColumnId| id.clone() {
            {column_part(Rc::clone(&cx), id, value, Rc::clone(&detail), parts, line)}
        }
        box(
            class = "lui-grid__detail-cell",
            node_ref = filler,
            attr:data-state = move || line.state.get(),
            var:--lui-grid-detail-height = move || line.height.get(),
            a11y:hidden = true
        ) {}
    }
}

/// Builds one column's part of an open row.
type ColumnDetail<T> = Rc<dyn Fn(&T, &ColumnId) -> Option<AnyView>>;

/// What every part of one open row shares.
#[derive(Clone, Copy)]
struct DetailLine {
    open: Signal<bool, LocalStorage>,
    /// The tallest part's height as CSS text, once measured.
    height: Signal<Option<String>, LocalStorage>,
    state: Signal<Option<String>, LocalStorage>,
}

/// `id`'s part of an open row. Its content is laid out at its own width, so a part wider than its
/// column is marked cut off.
fn column_part<T, K>(
    cx: Rc<GridCx<T, K>>,
    id: ColumnId,
    value: RwSignal<Option<T>, LocalStorage>,
    detail: ColumnDetail<T>,
    parts: RwSignal<HashMap<ColumnId, f32>, LocalStorage>,
    line: DetailLine,
) -> AnyView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let inner = NodeRef::new();
    let content = inner.observe_border_size();
    let outer = NodeRef::new();
    let room = outer.observe_border_size();
    let scale = move || {
        let scale = inner.scale();
        if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
    };

    let noting = {
        let id = id.clone();
        RenderEffect::new(move |_| {
            let Some(height) = content.get().map(|size| size.height.0 / scale()) else { return };
            if parts.with_untracked(|parts| parts.get(&id) != Some(&height)) {
                parts.update(|parts| {
                    parts.insert(id.clone(), height);
                });
            }
        })
    };
    // Whether the part has content, counted in its column while it does.
    let shown = Rc::new(Cell::new(false));
    let build = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        let shown = Rc::clone(&shown);
        move || {
            let part = value.with(|row| row.as_ref().and_then(|row| detail(row, &id)));
            if part.is_some() != shown.get() {
                shown.set(part.is_some());
                cx.note_part(&id, part.is_some());
            }
            part
        }
    };
    {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        on_cleanup_local(move || {
            drop(noting);
            parts.try_update(|parts| parts.remove(&id));
            if shown.get() {
                cx.note_part(&id, false);
            }
        });
    }

    // Within half a pixel is rounding, not content cut off.
    let cut = move || {
        let content = content.get()?;
        let room = room.get()?;
        (content.width.0 > room.width.0 + 0.5 * scale()).then(String::new)
    };
    let own = placed(&cx, &id, "lui-grid__detail-cell");
    let shift = shift(&cx, id.clone());
    let mark = row_mark(&cx, id.clone(), value);
    let open = line.open;
    let rooms = cx.rooms;
    let limit = {
        let id = id.clone();
        move || rooms.with(|rooms| rooms.get(&id).map(|room| format!("{room}px")))
    };

    AnyView::new(view! {
        box(
            node_ref = outer,
            style:translate = shift,
            attr:data-state = move || line.state.get(),
            attr:data-cut = cut,
            var:--lui-grid-detail-height = move || line.height.get(),
            var:--lui-grid-room = limit,
            a11y:hidden = move || !open.get(),
            {..own}
        ) {
            {mark}
            box(class = "lui-grid__detail-part", node_ref = inner) {
                {build}
            }
            if move || cut().is_some() {
                box(class = "lui-grid__detail-fade", a11y:hidden = true) {}
            } else {}
        }
    })
}

/// Tells `reported` the height of `node`, in CSS pixels, while it is built.
fn report_height(node: NodeRef, reported: RwSignal<f32, LocalStorage>) {
    let size = node.observe_border_size();
    let reporting = RenderEffect::new(move |_| {
        let scale = node.scale();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let height = size.get().map_or(0.0, |size| size.height.0 / scale);
        if reported.get_untracked() != height {
            reported.set(height);
        }
    });
    on_cleanup_local(move || {
        drop(reporting);
        reported.set(0.0);
    });
}

/// `reported` is told the region's height in CSS pixels while it is built.
fn detail_body<T>(
    open: Signal<bool, LocalStorage>,
    value: RwSignal<Option<T>, LocalStorage>,
    detail: Rc<dyn Fn(&T) -> AnyView>,
    view_width: Signal<Option<String>, LocalStorage>,
    reported: Option<RwSignal<f32, LocalStorage>>,
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

    // A measured row counts its detail region, at whatever height its reveal has it, as part of
    // the row.
    let outer = NodeRef::new();
    if let Some(reported) = reported {
        report_height(outer, reported);
    }

    view! {
        box(
            class = "lui-grid__detail",
            node_ref = outer,
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
        if let Some((key, items)) = cursor.and_then(|key| cx.row_menu_items(&key).map(|items| (key, items))) {
            open_row_menu(cx, key, items);
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

/// Opens the row menu of `key`, built by `items`, under its row, at the leading edge of the view.
fn open_row_menu<T, K>(cx: &GridCx<T, K>, key: K, items: MenuItems)
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    // The filler cell spans the row's height but sits after the last column.
    let filler = cx.row_cells.borrow().get(&key).copied();
    let Some((row, view)) = filler.and_then(|filler| filler.window_bounds()).zip(cx.port.window_bounds()) else {
        return;
    };

    let corner = Point::new(view.origin.x, DevicePx(row.origin.y.0 + row.size.height.0));
    header::open_menu(corner, MenuTarget::Row(key, items), cx);
}
