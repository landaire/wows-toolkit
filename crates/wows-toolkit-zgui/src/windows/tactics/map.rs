//! The map itself: the drawn frame, the pointer on it, its menu and the turning handle.

use landaire_ui::LoadingProps;
use rust_i18n::t;
use wows_toolkit_replay_data::tactics::BoardTool;
use wows_toolkit_replay_data::tactics::ElementPos;
use wows_toolkit_replay_data::tactics::ElementSize;
use wows_toolkit_replay_data::tactics::HANDLE_RADIUS;
use wows_toolkit_replay_data::tactics::MAX_CAP_RADIUS;
use wows_toolkit_replay_data::tactics::MIN_CAP_RADIUS;
use wows_toolkit_replay_data::tactics::RADIUS_STEP_KM;
use wows_toolkit_replay_data::tactics::board::BoardKey;
use wows_toolkit_replay_data::tactics::board::Modifiers;
use wows_toolkit_replay_data::tactics::shortcut_modifier_name;
use zgui::component;
use zgui::css;
use zgui::geom::CssPx;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui::vocab::PointerButton;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuSeparatorProps;

use crate::frames::FrameViewProps;
use crate::services::Services;
use crate::windows::tactics::model::BoardModel;

const SHEET: &str = "wt-tactics-map";

const CSS: &str = css!(
    ".wt-tactics-map {
        position: relative;
        flex: 1 1 0;
        overflow: hidden;
        min-width: 0;
        min-height: 0;
        cursor: grab;
    }
    .wt-tactics-map[data-aiming] { cursor: crosshair; }
    .wt-tactics-map__trigger {
        display: flex;
        flex-direction: column;
        flex: 1 1 0;
        min-width: 0;
        min-height: 0;
    }
    .wt-tactics-map__said {
        flex: 1 1 auto;
        min-height: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        font-size: var(--zui-type-size-sm);
        color: var(--wt-color-text-dim);
    }
    .wt-tactics-map__overlay {
        position: absolute;
        inset: 0;
        pointer-events: none;
    }
    .wt-tactics-map__handle {
        position: absolute;
        width: 10px;
        height: 10px;
        border-radius: 5px;
        background-color: rgb(255, 255, 100);
        pointer-events: none;
    }
    .wt-tactics-map__stem {
        position: absolute;
        width: 1px;
        background-color: rgb(255, 255, 100);
        pointer-events: none;
    }"
);

/// What stands where the map would be, until there is a frame to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placeholder {
    PickAMap,
    NoArt,
    Drawing,
}

/// The map, or what is standing in its way.
#[component]
pub fn BoardMap(model: BoardModel) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    // A memo, so the surface is rebuilt only when what stands in its place changes: rebuilding it
    // on every board change would drop the pointer capture of a drag in progress.
    let placeholder = Memo::new(move |_| {
        model.with(|board| {
            if board.has_frame() {
                None
            } else if board.map().is_none() {
                Some(Placeholder::PickAMap)
            } else if board.no_art() {
                Some(Placeholder::NoArt)
            } else {
                Some(Placeholder::Drawing)
            }
        })
    });

    view! {
        {move || match placeholder.get() {
            Some(Placeholder::PickAMap) => AnyView::new(view! {
                box(class = "wt-tactics-map__said", attr:data-testid = "tactics-pick-a-map") {
                    text {{locale.text("ui.tactics.pick_a_map")}}
                }
            }),
            Some(Placeholder::NoArt) => AnyView::new(view! {
                box(class = "wt-tactics-map__said", attr:data-testid = "tactics-no-art") {
                    text {{locale.text("ui.tactics.no_map_art")}}
                }
            }),
            Some(Placeholder::Drawing) => AnyView::new(view! {
                Loading(
                    label = Signal::derive_local(locale.text("ui.tactics.drawing")),
                    attr:data-testid = "tactics-drawing"
                )
            }),
            None => AnyView::new(view! { MapSurface(model = model) }),
        }}
    }
}

/// The drawn map and everything the pointer does on it.
#[component]
fn MapSurface(model: BoardModel) -> impl IntoView {
    let area = NodeRef::new();
    // Whether the primary button is held on the map: a move reports no buttons of its own.
    let pressed = StoredValue::new(false);
    // Where the pointer is in the element, read against the box it had when the frame was painted.
    let local = move |position: zgui::geom::Point<CssPx, zgui::geom::Css>| -> Option<ElementPos> {
        let bounds = area.window_bounds()?;
        let scale = area.scale();
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        let size = ElementSize { width: bounds.size.width.0 / scale, height: bounds.size.height.0 / scale };
        model.update(|board| board.set_element(size));
        Some(ElementPos { x: position.x.0 - bounds.origin.x.0 / scale, y: position.y.0 - bounds.origin.y.0 / scale })
    };
    let aiming = move || model.with(|board| board.has_tool() || board.adding()).then(String::new);
    let sink = model.sink();

    let on_down = move |ev: &mut EventCx<'_, events::PointerDown>| {
        let Some(at) = local(ev.position) else { return };
        match ev.button {
            Some(PointerButton::Primary) => {
                ev.capture_pointer();
                pressed.set_value(true);
                let held = Modifiers { secondary: ev.modifiers.primary(), shift: ev.modifiers.shift() };
                model.update(|board| board.pointer_down(at, held));
            }
            Some(PointerButton::Secondary) => model.update(|board| board.context_down(at)),
            _ => {}
        }
    };
    let on_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
        let Some(at) = local(ev.position) else { return };
        let pressed = pressed.get_value();
        model.update(|board| board.pointer_move(at, pressed));
    };
    let on_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
        if ev.button != Some(PointerButton::Primary) {
            return;
        }
        ev.release_pointer();
        pressed.set_value(false);
        let Some(at) = local(ev.position) else {
            model.update(|board| board.release());
            return;
        };
        model.update(|board| board.pointer_up(at));
    };

    view! {
        ContextMenu(on_open_change = UnsyncCallback::new(move |open: bool| {
            if open {
                model.update(|board| board.menu_opened());
            }
        })) {
            ContextMenuTrigger(class = "wt-tactics-map__trigger") {
                box(
                    class = "wt-tactics-map",
                    node_ref = area,
                    attr:data-testid = "tactics-map",
                    attr:data-aiming = aiming,
                    on:pointer_down = on_down,
                    on:pointer_move = on_move,
                    on:pointer_up = on_up,
                    on:pointer_cancel = move |ev| {
                        ev.release_pointer();
                        pressed.set_value(false);
                        model.update(|board| board.release());
                    },
                    on:pointer_leave = move |_| model.update(|board| board.pointer_left()),
                    on:double_click = move |_| model.update(|board| board.double_click()),
                    on:wheel = move |ev| {
                        let Some(at) = local(ev.position) else { return };
                        let notches = ev.delta.to_pixels(CssPx(16.0)).height.0;
                        if notches != 0.0 {
                            model.update(|board| board.zoom_about(notches, at));
                            ev.prevent_default();
                        }
                    }
                ) {
                    FrameView(sink = sink)
                    box(class = "wt-tactics-map__overlay") {
                        {move || rotation_handle(model)}
                    }
                }
            }
            ContextMenuContent {
                {move || menu_items(model)}
            }
        }
    }
}

/// The handle a picked shape is turned by, drawn over the map.
///
/// An element rather than part of the frame: it is a control rather than something drawn on the
/// map, so it keeps its size at every zoom and stays out of a saved preset.
fn rotation_handle(model: BoardModel) -> Option<AnyView> {
    let handle = model.with(|board| board.rotation_handle())?;
    let left = format!("{}px", handle.handle.x - HANDLE_RADIUS);
    let top = format!("{}px", handle.handle.y - HANDLE_RADIUS);
    let stem_left = format!("{}px", handle.handle.x - 0.5);
    let stem_top = format!("{}px", handle.handle.y);
    let stem_height = format!("{}px", (handle.anchor.y - handle.handle.y).max(0.0));
    Some(AnyView::new(view! {
        box(class = "wt-tactics-map__stem", style:left = stem_left, style:top = stem_top, style:height = stem_height)
        box(class = "wt-tactics-map__handle", attr:data-testid = "tactics-rotation-handle", style:left = left, style:top = top)
    }))
}

/// The map's menu: the view, the tools, the selected capture point and the history.
fn menu_items(model: BoardModel) -> Vec<AnyView> {
    let snapshot = model.with(|board| {
        let locked = board.annotations_locked();
        MenuState {
            locked,
            can_edit_caps: board.can_edit_caps(),
            can_undo: board.can_undo() && !locked,
            can_redo: board.can_redo() && !locked,
            has_drawing: board.annotation_count() > 0 && !locked,
            has_map: board.has_map_info(),
            selected: board.selected_cap().map(|cap| (cap.side(), cap.radius, cap.frozen)),
        }
    });
    let item = |label: String, test_id: &'static str, run: Box<dyn Fn()>| {
        AnyView::new(view! {
            MenuItem(on_select = UnsyncCallback::new(move |()| run()), attr:data-testid = test_id) {
                {label}
            }
        })
    };
    let separator = || AnyView::new(view! { MenuSeparator() });

    let mut items = vec![item(
        t!("ui.tactics.reset_view").into_owned(),
        "tactics-menu-reset-view",
        Box::new(move || model.update(|board| board.reset_view())),
    )];

    if !snapshot.locked {
        items.push(separator());
        for tool in BoardTool::ALL {
            let label = format!("{} ({}+{})", t!(tool.label_key()), shortcut_modifier_name(), tool.shortcut());
            items.push(item(
                label,
                "tactics-menu-tool",
                Box::new(move || model.update(|board| board.set_tool(tool.tool()))),
            ));
        }
    }

    if snapshot.can_edit_caps && (snapshot.selected.is_some() || snapshot.has_map) {
        items.push(separator());
        if let Some((side, radius, frozen)) = snapshot.selected {
            items.push(item(
                format!("{}: {}", t!("ui.tactics.cap_team"), t!(side.label_key())),
                "tactics-menu-cap-team",
                Box::new(move || model.update(|board| board.cycle_selected_team())),
            ));
            if radius > MIN_CAP_RADIUS {
                items.push(item(
                    t!("ui.tactics.cap_narrow_tooltip").into_owned(),
                    "tactics-menu-cap-narrow",
                    Box::new(move || model.update(|board| board.step_selected_radius(-RADIUS_STEP_KM))),
                ));
            }
            if radius < MAX_CAP_RADIUS {
                items.push(item(
                    t!("ui.tactics.cap_widen_tooltip").into_owned(),
                    "tactics-menu-cap-widen",
                    Box::new(move || model.update(|board| board.step_selected_radius(RADIUS_STEP_KM))),
                ));
            }
            if !frozen {
                items.push(item(
                    t!("ui.tactics.delete_cap").into_owned(),
                    "tactics-menu-cap-delete",
                    Box::new(move || model.update(|board| board.remove_selected())),
                ));
            }
        }
        if snapshot.has_map {
            items.push(item(
                t!("ui.tactics.add_cap").into_owned(),
                "tactics-menu-add-cap",
                Box::new(move || model.update(|board| board.set_adding(true))),
            ));
        }
    }

    if snapshot.can_undo || snapshot.can_redo || snapshot.has_drawing {
        items.push(separator());
    }
    if snapshot.can_undo {
        items.push(item(
            t!("ui.renderer.annotations.undo").into_owned(),
            "tactics-menu-undo",
            Box::new(move || model.update(|board| board.undo())),
        ));
    }
    if snapshot.can_redo {
        items.push(item(
            t!("ui.renderer.annotations.redo").into_owned(),
            "tactics-menu-redo",
            Box::new(move || model.update(|board| board.redo())),
        ));
    }
    if snapshot.has_drawing {
        items.push(item(
            t!("ui.tactics.clear_drawing").into_owned(),
            "tactics-menu-clear-drawing",
            Box::new(move || model.update(|board| board.clear_annotations())),
        ));
    }
    items
}

/// What the menu offers, read once as it is built.
struct MenuState {
    locked: bool,
    can_edit_caps: bool,
    can_undo: bool,
    can_redo: bool,
    has_drawing: bool,
    has_map: bool,
    /// The selected capture point's side, radius and whether it is frozen.
    selected: Option<(wows_toolkit_replay_data::tactics::CapSide, f32, bool)>,
}

/// Hands a key press to the board. `true` when the board took it.
pub fn board_key(model: BoardModel, ev: &EventCx<'_, events::KeyDown>) -> bool {
    let lowered;
    let key = match &ev.key_without_modifiers {
        Key::Named(NamedKey::Escape) => BoardKey::Escape,
        Key::Named(NamedKey::Delete) => BoardKey::Delete,
        Key::Named(NamedKey::Backspace) => BoardKey::Backspace,
        Key::Character(text) => {
            lowered = text.to_lowercase();
            BoardKey::Text(&lowered)
        }
        _ => return false,
    };
    let held = Modifiers { secondary: ev.modifiers.primary(), shift: ev.modifiers.shift() };
    if ev.modifiers.alt() {
        return false;
    }
    model.change(|board| board.key(key, held)).unwrap_or(false)
}
