use std::hash::Hash;
use std::rc::Rc;

use zgui::elements::Focus;
use zgui::reactive::RenderEffect;
use zgui::reactive::UnsyncCallback;
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
use zgui::view::events;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;
use zgui_ui::MenuItemProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::arrow::ARROW_DOWN;
use zgui_ui_icons::set::arrow::ARROW_UP;

use crate::grid::column::ColumnId;
use crate::grid::layout;
use crate::grid::layout::Placement;
use crate::grid::layout::Section;
use crate::grid::sort::SortDirection;
use crate::grid::state::Carry;
use crate::grid::state::GridCx;
use crate::grid::state::MenuTarget;
use crate::grid::state::Resize;

/// How far a pressed header travels before it moves its column rather than sorting by it.
const CARRY_THRESHOLD: f32 = 4.0;

/// How far one arrow key moves a grip, in CSS pixels.
const GRIP_STEP: f32 = 8.0;

/// The header row: one cell per column as drawn, then the cell over the spare room.
pub(crate) fn header_row<T, K>(cx: Rc<GridCx<T, K>>) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let filler = NodeRef::new();
    let filler_size = filler.observe_border_size();
    let header_height = cx.header_height;
    let scale = {
        let cx = Rc::clone(&cx);
        move || cx.scale()
    };
    let measuring = RenderEffect::new(move |_| {
        if let Some(size) = filler_size.get() {
            header_height.set(size.height.0 / scale());
        }
    });
    on_cleanup_local(move || drop(measuring));

    let ids = cx.drawn_ids;

    view! {
        box(class = "lui-grid__row", a11y:role = Role::Row) {
            for id in move || ids.get(), key = |id: &ColumnId| id.clone() {
                {header_cell(Rc::clone(&cx), id)}
            }
            box(class = "lui-grid__head-filler", node_ref = filler, a11y:hidden = true)
        }
    }
}

fn header_cell<T, K>(cx: Rc<GridCx<T, K>>, id: ColumnId) -> AnyView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let Some(column) = cx.column(&id).cloned() else {
        return AnyView::new(());
    };

    let cell = NodeRef::new();
    let size = cell.observe_border_size();
    let measuring = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        RenderEffect::new(move |_| {
            if let Some(size) = size.get() {
                cx.measured(&id, size.width.0 / cx.scale());
            }
        })
    };
    on_cleanup_local(move || drop(measuring));

    let sort = cx.sort;
    let sorted = {
        let id = id.clone();
        move || sort.with(|sort| sort.as_ref().filter(|sort| sort.column == id).map(|sort| sort.direction))
    };
    let mut semantics = A11yBinding::new(Role::ColumnHeader).label(column.label().to_owned());
    if column.is_sortable() {
        let sorted = sorted.clone();
        semantics = semantics.step(move |a11y| match sorted() {
            Some(direction) => a11y.sort_direction(direction.a11y()),
            None => a11y,
        });
    }

    let pins = cx.pins;
    let carried_column = cx.carried;
    let own = {
        let pinned = id.clone();
        let left = id.clone();
        let edge = id.clone();
        let carried = id.clone();
        Attrs::new()
            .class_toggle(zgui::view::ClassName::new("lui-grid__head"), true)
            .attribute(AttrName::new("data-align"), Some(column.align_value().attribute().to_owned()))
            .attribute(AttrName::new("data-sortable"), column.is_sortable().then(String::new))
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
    let shifts = cx.shifts;
    let shift = {
        let id = id.clone();
        move || shifts.with(|shifts| shifts.get(&id).map(|shift| format!("{shift}px 0px")))
    };

    let on_pointer_down = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            cx.press_answered.set(false);
            match ev.button {
                Some(PointerButton::Secondary) => {
                    cx.menu_target.set(Some(MenuTarget::Header(id.clone())));
                    cx.menu_claimed.set(true);
                }
                Some(PointerButton::Primary) => {
                    let Some(bounds) = cell.window_bounds() else {
                        return;
                    };
                    let x = ev.position.x.0;
                    let grab = x - bounds.origin.x.0 / cx.scale();
                    cx.carry.set(Some(Carry::Pressed { column: id.clone(), from: x, grab }));
                    // Held from the press so a press that crosses into a neighbour before it
                    // counts as a carry still ends here. The release then answers the press
                    // itself, since a capture keeps it from activating what was pressed.
                    ev.capture_pointer();
                }
                _ => {}
            }
        }
    };
    let on_context_menu = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        move |_: &mut EventCx<'_, events::ContextMenu>| {
            cx.menu_target.set(Some(MenuTarget::Header(id.clone())));
            cx.menu_claimed.set(true);
        }
    };
    let on_pointer_move = {
        let cx = Rc::clone(&cx);
        let reorderable = column.is_reorderable();
        move |ev: &mut EventCx<'_, events::PointerMove>| {
            let Some(carrying) = cx.carry.get_untracked() else {
                return;
            };
            let x = ev.position.x.0;
            let (column, grab) = match carrying {
                Carry::Pressed { column, from, grab } => {
                    if !reorderable || (x - from).abs() < CARRY_THRESHOLD {
                        return;
                    }
                    (column, grab)
                }
                Carry::Moving { column, grab, .. } => (column, grab),
            };
            let Some(pointer) = cx.pointer_offset(x) else {
                return;
            };

            follow(&cx, &column, pointer);
            cx.carry.set(Some(Carry::Moving { column, grab, pointer }));
        }
    };
    let on_pointer_up = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        let sortable = column.is_sortable();
        move |ev: &mut EventCx<'_, events::PointerUp>| {
            ev.release_pointer();
            let Some(carrying) = cx.carry.get_untracked() else {
                return;
            };
            cx.carry.set(None);
            cx.press_answered.set(true);
            if carrying.moving().is_none() {
                if sortable {
                    cx.press_header(&id);
                }
                return;
            }

            if let Some(preview) = cx.preview.get_untracked() {
                cx.rearrange(preview);
            }
        }
    };
    let on_pointer_cancel = {
        let cx = Rc::clone(&cx);
        move |ev: &mut EventCx<'_, events::PointerCancel>| {
            ev.release_pointer();
            cx.carry.set(None);
            if cx.preview.get_untracked().is_some() {
                let committed = cx.layout.get_untracked();
                cx.rearrange(committed);
            }
        }
    };
    let on_click = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        let sortable = column.is_sortable();
        move |_: &mut EventCx<'_, events::Click>| {
            if cx.press_answered.get_untracked() {
                cx.press_answered.set(false);
                return;
            }
            if sortable {
                cx.press_header(&id);
            }
        }
    };
    let on_key_down = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        let reorderable = column.is_reorderable();
        move |ev: &mut EventCx<'_, events::KeyDown>| {
            if opens_menu(&ev.key, ev.modifiers) {
                open_menu_at(cell, MenuTarget::Header(id.clone()), &cx);
                ev.prevent_default();
                return;
            }

            let by: isize = match (&ev.key, ev.modifiers.alt()) {
                (Key::Named(NamedKey::ArrowLeft), true) => -1,
                (Key::Named(NamedKey::ArrowRight), true) => 1,
                _ => return,
            };
            if reorderable {
                nudge(&cx, &id, by);
            }
            ev.prevent_default();
            ev.stop_propagation();
        }
    };

    let label = column.header_view();
    let grip = column.is_resizable().then(|| AnyView::new(grip(Rc::clone(&cx), id.clone(), cell)));

    AnyView::new(view! {
        control(
            node_ref = cell,
            tabindex = Focus::Sequential,
            style:translate = shift,
            on:pointer_down = on_pointer_down,
            on:pointer_move = on_pointer_move,
            on:pointer_up = on_pointer_up,
            on:pointer_cancel = on_pointer_cancel,
            on:context_menu = on_context_menu,
            on:click = on_click,
            on:key_down = on_key_down,
            {..own}
        ) {
            box(class = "lui-grid__label") {{label}}
            {move || sorted().map(|direction| {
                let icon = match direction {
                    SortDirection::Ascending => ARROW_UP,
                    SortDirection::Descending => ARROW_DOWN,
                };
                AnyView::new(view! { Icon(icon = icon, size = IconSize::Sm, class = "lui-grid__sort") })
            })}
            {grip}
        }
    })
}

/// The strip on a header's trailing edge that resizes its column.
fn grip<T, K>(cx: Rc<GridCx<T, K>>, id: ColumnId, cell: NodeRef) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let label = cx.column(&id).map(|column| format!("Resize {}", column.label())).unwrap_or_default();
    let semantics = A11yBinding::new(Role::Splitter).label(label).orientation(zgui::vocab::Orientation::Vertical);
    let resize = cx.resize;
    let active = {
        let id = id.clone();
        move || resize.with(|resize| resize.as_ref().is_some_and(|resize| resize.column == id)).then(String::new)
    };

    let on_pointer_down = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            // The press belongs to the grip, not to the header it sits in, which would otherwise
            // also start carrying its column.
            ev.stop_propagation();
            if ev.button != Some(PointerButton::Primary) {
                return;
            }
            let Some(bounds) = cell.bounds() else {
                return;
            };

            ev.prevent_default();
            ev.capture_pointer();
            let start = bounds.size.width.0 / cx.scale();
            let Some(column) = cx.column(&id) else {
                return;
            };
            let width = column.clamp(start);
            cx.resize.set(Some(Resize { column: id.clone(), from: ev.position.x.0, start, width }));
        }
    };
    let on_pointer_move = {
        let cx = Rc::clone(&cx);
        move |ev: &mut EventCx<'_, events::PointerMove>| {
            let Some(resize) = cx.resize.get_untracked() else {
                return;
            };
            let Some(column) = cx.column(&resize.column) else {
                return;
            };

            let width = column.clamp(resize.start + ev.position.x.0 - resize.from);
            if width != resize.width {
                cx.resize.set(Some(Resize { width, ..resize }));
            }
        }
    };
    let finish = {
        let cx = Rc::clone(&cx);
        move || {
            let Some(resize) = cx.resize.get_untracked() else {
                return;
            };
            cx.resize.set(None);
            cx.layout.update(|layout| layout.set_width(&resize.column, Some(resize.width)));
        }
    };
    let on_pointer_up = {
        let finish = finish.clone();
        move |ev: &mut EventCx<'_, events::PointerUp>| {
            ev.release_pointer();
            ev.stop_propagation();
            finish();
        }
    };
    let on_pointer_cancel = move |ev: &mut EventCx<'_, events::PointerCancel>| {
        ev.release_pointer();
        finish();
    };
    let on_double_click = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        move |ev: &mut EventCx<'_, events::DoubleClick>| {
            ev.stop_propagation();
            cx.resize.set(None);
            cx.layout.update(|layout| layout.set_width(&id, None));
        }
    };
    let on_click = move |ev: &mut EventCx<'_, events::Click>| ev.stop_propagation();
    let on_key_down = {
        let cx = Rc::clone(&cx);
        move |ev: &mut EventCx<'_, events::KeyDown>| {
            let by = match &ev.key {
                Key::Named(NamedKey::ArrowLeft) => -GRIP_STEP,
                Key::Named(NamedKey::ArrowRight) => GRIP_STEP,
                _ => return,
            };
            ev.prevent_default();
            ev.stop_propagation();
            let Some(column) = cx.column(&id) else {
                return;
            };

            let width = column.clamp(cx.width_of(&id) + by);
            cx.layout.update(|layout| layout.set_width(&id, Some(width)));
        }
    };

    view! {
        control(
            class = "lui-grid__grip",
            tabindex = Focus::Sequential,
            attr:data-active = active,
            on:pointer_down = on_pointer_down,
            on:pointer_move = on_pointer_move,
            on:pointer_up = on_pointer_up,
            on:pointer_cancel = on_pointer_cancel,
            on:double_click = on_double_click,
            on:click = on_click,
            on:key_down = on_key_down,
            {..Attrs::new().a11y_from(semantics)}
        )
    }
}

/// Moves the preview to where the carried `column` lands with the pointer at `pointer`.
fn follow<T, K>(cx: &GridCx<T, K>, column: &ColumnId, pointer: f32)
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let committed = cx.layout.get_untracked();
    let may_pin = cx.may_pin(column) || committed.is_pinned(column);
    let placement = layout::placement_at(&committed, column, pointer, cx.scroll_x(), |id| cx.width_of(id), may_pin);

    let mut preview = committed;
    preview.place(column, placement);
    let drawn = cx.drawn.get_untracked();
    if drawn.columns() != preview.columns() {
        cx.slide(&drawn, &preview);
        cx.preview.set(Some(preview));
    }
}

/// Moves `id` `by` places within its section.
fn nudge<T, K>(cx: &GridCx<T, K>, id: &ColumnId, by: isize)
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let mut next = cx.layout.get_untracked();
    let Some(position) = next.position(id) else {
        return;
    };

    let pinned = next.pinned_count();
    let (section, index, count) = if next.is_pinned(id) {
        (Section::Pinned, position, pinned)
    } else {
        (Section::Scrolling, position - pinned, next.columns().len() - pinned)
    };
    let Some(target) = index.checked_add_signed(by).filter(|target| *target < count) else {
        return;
    };

    next.place(id, Placement { section, index: target });
    cx.rearrange(next);
}

/// Whether a key press asks for a context menu: the menu key, or shift and F10.
pub(crate) fn opens_menu(key: &Key, modifiers: Modifiers) -> bool {
    match key {
        Key::Named(NamedKey::ContextMenu) => true,
        Key::Named(NamedKey::F10) => modifiers.shift(),
        _ => false,
    }
}

/// Opens the grid's menu for `target` at the bottom-left corner of `anchor`.
pub(crate) fn open_menu_at<T, K>(anchor: NodeRef, target: MenuTarget<K>, cx: &GridCx<T, K>)
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let Some((place, bounds)) = cx.menu_place.get().zip(anchor.window_bounds()) else {
        return;
    };

    let scale = cx.scale();
    cx.menu_target.set(Some(target));
    place.open_at(zgui::geom::Point::new(
        zgui::geom::CssPx(bounds.origin.x.0 / scale),
        zgui::geom::CssPx((bounds.origin.y.0 + bounds.size.height.0) / scale),
    ));
}

/// The items of the menu a header opens.
pub(crate) fn header_menu<T, K>(cx: Rc<GridCx<T, K>>, id: ColumnId) -> AnyView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let pinned = cx.layout.with_untracked(|layout| layout.is_pinned(&id));
    let refused = !pinned && !cx.may_pin(&id);
    let resized = cx.layout.with_untracked(|layout| layout.width(&id).is_some());
    let pin_label = if pinned { cx.strings.unpin.clone() } else { cx.strings.pin.clone() };
    let reset_label = cx.strings.reset_width.clone();

    let on_pin = {
        let cx = Rc::clone(&cx);
        let id = id.clone();
        UnsyncCallback::new(move |()| {
            let mut next = cx.layout.get_untracked();
            if pinned {
                next.unpin(&id);
            } else {
                next.pin(&id);
            }
            cx.rearrange(next);
        })
    };
    let on_reset = UnsyncCallback::new(move |()| cx.layout.update(|layout| layout.set_width(&id, None)));

    AnyView::new(view! {
        MenuItem(disabled = refused, on_select = on_pin) {{pin_label}}
        MenuItem(disabled = !resized, on_select = on_reset) {{reset_label}}
    })
}

/// What shows while a column is carried: the pinned zone tinted to its edge, and the carried
/// header following the pointer. Draws over the grid and takes no pointer events.
pub(crate) fn carry_overlay<T, K>(cx: Rc<GridCx<T, K>>) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let carry = cx.carry;

    view! {
        if move || carry.with(|carry| carry.as_ref().and_then(Carry::moving).is_some()) {
            {carried_views(Rc::clone(&cx))}
        } else {}
    }
}

fn carried_views<T, K>(cx: Rc<GridCx<T, K>>) -> impl IntoView
where
    T: Clone + PartialEq + 'static,
    K: Clone + Eq + Hash + 'static,
{
    let Some(Carry::Moving { column, .. }) = cx.carry.get_untracked() else {
        return AnyView::new(());
    };
    let Some(header) = cx.column(&column).map(|column| column.header_view()) else {
        return AnyView::new(());
    };

    let refused = !cx.may_pin(&column) && !cx.layout.with_untracked(|layout| layout.is_pinned(&column));
    let zone_width = {
        let cx = Rc::clone(&cx);
        move || {
            let width = cx.drawn.with(|drawn| layout::pin_zone(drawn, |id| cx.width_of(id)));
            Some(format!("{width}px"))
        }
    };
    let width = cx.width_of(&column);
    let ghost_left = {
        let cx = Rc::clone(&cx);
        move || {
            let Some(Carry::Moving { grab, pointer, .. }) = cx.carry.get() else {
                return None;
            };
            let room = cx.port.bounds().map_or(f32::MAX, |port| port.size.width.0 / cx.scale());
            Some(format!("{}px", (pointer - grab).min(room - width).max(0.0)))
        }
    };
    let header_height = cx.header_height;

    AnyView::new(view! {
        box(class = "lui-grid__zone", attr:data-refused = {refused.then(String::new)}, style:width = zone_width)
        box(
            class = "lui-grid__ghost",
            a11y:hidden = true,
            style:left = ghost_left,
            style:width = {Some(format!("{width}px"))},
            style:height = move || Some(format!("{}px", header_height.get()))
        ) {
            {header}
        }
    })
}
