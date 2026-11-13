//! Tooltips that wait half a second, show one at a time per window and stand at the pointer.

use core::time::Duration;
use std::rc::Rc;

use zgui::component;
use zgui::css;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::geom::Point;
use zgui::geom::Size;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::AttrName;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::ChildrenFn;
use zgui::view::ClassName;
use zgui::view::Classes;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::OverlayLayer;
use zgui::view::PortalProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::FocusCause;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::overlay::ElevatedProps;
use zgui_ui::overlay::Handoff;
use zgui_ui::overlay::HoverIntent;
use zgui_ui::overlay::OverlayState;
use zgui_ui::overlay::OverlaySurfaceProps;
use zgui_ui::overlay::SurfaceElevation;
use zgui_ui::tooltip::TooltipDelays;
use zgui_ui::tooltip::TooltipStyle;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::PresenceProps;

/// What the tooltip rules are installed under.
const SHEET: &str = "landaire-ui-tooltip";

/// What zgui-ui installs the tooltip's own look under, so the two installs are one.
const ZUI_SHEET: &str = "zui-tooltip";

/// How long the pointer has to rest on a trigger before its tooltip appears.
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);

/// How far a tooltip's corner stands from the pointer on each axis, in CSS pixels.
const POINTER_GAP: f32 = 1.0;

const CSS: &str = css!(
    ".lui-tooltip-trigger {
        display: inline-flex;
        align-items: center;
    }
    .zui-overlay-depth.lui-tooltip-layer {
        pointer-events: none;
    }
    .lui-tooltip-positioner {
        position: fixed;
        left: 0px;
        top: 0px;
    }"
);

/// Sets up a window's tooltips: [`TOOLTIP_DELAY`] before one opens, none before it closes, one
/// showing at a time, and the trigger rules installed before any application sheet, so a class an
/// application gives a trigger lays it out.
///
/// [`ThemeRoot`](crate::ThemeRoot) calls this, so every tooltip in a window shares it.
pub(crate) fn install_tooltips() {
    install_stylesheet(SHEET, CSS);
    TooltipDelays::new(TOOLTIP_DELAY, Duration::ZERO).provide();
    Handoff::new().provide();
}

/// Where a shown tooltip is placed against: the pointer, and the window it has to stay inside.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    /// Where the pointer was when the tooltip appeared, in window CSS pixels.
    pointer: Point<CssPx, Css>,
    /// The window's size in CSS pixels.
    window: Size<CssPx, Css>,
}

/// Where the tooltip of one [`Tooltip`] goes.
#[derive(Clone, Copy)]
struct TooltipPlace {
    /// The trigger's element.
    area: NodeRef,
    /// Where the pointer is while it is over the trigger, in window CSS pixels.
    pointer: StoredValue<Option<Point<CssPx, Css>>, LocalStorage>,
    /// Where the showing tooltip is placed against.
    spot: RwSignal<Option<Spot>, LocalStorage>,
}

impl TooltipPlace {
    fn new() -> Self {
        Self { area: NodeRef::new(), pointer: StoredValue::new_local(None), spot: RwSignal::new_local(None) }
    }

    /// Takes the spot from the pointer, or from below the trigger when the keyboard opened it.
    fn take_spot(self) {
        let Some(bounds) = self.area.window_bounds() else {
            return;
        };
        let Some(window) = self.area.window_root().and_then(|root| root.window_bounds()) else {
            return;
        };

        // Boxes are in device pixels and the pointer is in CSS pixels.
        let scale = self.area.scale();
        let pointer = self.pointer.get_value().unwrap_or_else(|| {
            Point::new(CssPx(bounds.origin.x.0 / scale), CssPx((bounds.origin.y.0 + bounds.size.height.0) / scale))
        });
        let window = Size::new(CssPx(window.size.width.0 / scale), CssPx(window.size.height.0 / scale));
        self.spot.set(Some(Spot { pointer, window }));
    }
}

/// One [`Tooltip`], as the tooltips nested inside its trigger reach it.
#[derive(Clone)]
struct TooltipLink {
    intent: HoverIntent,
    place: TooltipPlace,
    /// The tooltip whose trigger this one's trigger is inside.
    outer: Option<Rc<TooltipLink>>,
}

impl TooltipLink {
    fn current() -> Option<Self> {
        use_local_context::<Self>()
    }

    /// The pointer reached a trigger inside this one's: this one and those around it wait until it
    /// comes back.
    fn yield_to_inner(&self) {
        let mut link = Some(self);
        while let Some(current) = link {
            current.intent.close_now();
            link = current.outer.as_deref();
        }
    }

    /// The pointer came back from a trigger inside this one's, and waits for this one again. An
    /// outer tooltip already disposed, as when the pointer leaves a trigger that was removed, waits
    /// for nothing.
    fn resume(&self) {
        if self.place.pointer.try_get_value().flatten().is_some() {
            self.intent.enter();
        }
    }
}

/// A label shown beside the pointer after it rests on a control.
///
/// Opens after [`TOOLTIP_DELAY`] and closes as soon as the pointer leaves, a button is pressed or
/// the wheel turns. One shows at a time in a window. The label stands one pixel right of and below
/// the pointer, and on the other side of it on an axis where the window's edge leaves no room. A
/// tooltip whose trigger is inside another's takes over from it while the pointer is on it.
#[component]
pub fn Tooltip(
    /// Whether it is showing, when the caller holds it.
    #[prop(into, optional)]
    open: Binding<bool>,
    /// Told whenever it appears or goes.
    #[prop(optional)]
    on_open_change: Option<UnsyncCallback<bool>>,
    /// A [`TooltipTrigger`] and a [`TooltipContent`].
    children: Children,
) -> impl IntoView {
    let delays = TooltipDelays::current();
    let open_after = delays.map_or(TOOLTIP_DELAY, TooltipDelays::open);
    let close_after = delays.map_or(Duration::ZERO, TooltipDelays::close);

    let outer = TooltipLink::current().map(Rc::new);
    let place = TooltipPlace::new();
    let state = OverlayState::new(open, false, on_open_change).anchored_to(place.area).provide();
    let intent = HoverIntent::new(state, open_after, close_after).in_handoff(Handoff::current()).provide();
    provide_local_context(TooltipLink { intent, place, outer });

    // Placed as it opens, whoever opened it, and before the surface is laid out.
    let open = state.open_signal();
    let placing = RenderEffect::new(move |_| {
        if open.get() {
            place.take_spot();
        }
    });
    on_cleanup_local(move || drop(placing));

    view! { {children.into_view_once()} }
}

/// What a [`Tooltip`] is about. A wrapper around the control it names.
#[component]
pub fn TooltipTrigger(
    /// Classes merged after its own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
    /// What the tooltip is about.
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let Some(link) = TooltipLink::current() else {
        return AnyView::new(
            view! { box(class = "lui-tooltip-trigger", {..attrs}, class = class) {{children.into_view_once()}} },
        );
    };
    let TooltipLink { intent, place, outer } = link;
    let state = intent.state();

    let own = Attrs::new()
        .attribute(AttrName::new("data-state"), move || Some(state.state_name().to_owned()))
        .a11y_from(A11yBinding::unspecified().described_by(state.content()));

    let entered = intent.clone();
    let entering = outer.clone();
    let on_enter = move |ev: &mut EventCx<'_, events::PointerEnter>| {
        place.pointer.set_value(Some(ev.position));
        if let Some(outer) = &entering {
            outer.yield_to_inner();
        }
        entered.enter();
    };
    // Followed until it opens: the label stands where the pointer was when it appeared.
    let on_move = move |ev: &mut EventCx<'_, events::PointerMove>| {
        if !state.is_open_untracked() {
            place.pointer.set_value(Some(ev.position));
        }
    };
    let departed = intent.clone();
    let on_leave = move |_: &mut EventCx<'_, events::PointerLeave>| {
        place.pointer.set_value(None);
        departed.leave();
        if let Some(outer) = &outer {
            outer.resume();
        }
    };
    // On the way down, so a control that keeps the press to itself still takes the label away.
    let pressed = intent.clone();
    let on_press = move |_: &mut EventCx<'_, events::PointerDown>| pressed.close_now();
    let scrolled = intent.clone();
    let on_wheel = move |_: &mut EventCx<'_, events::Wheel>| scrolled.close_now();
    // A keyboard user reached the control on purpose, so the label shows at once.
    let focused = intent.clone();
    let on_focus = move |ev: &mut EventCx<'_, events::FocusIn>| {
        if ev.cause == FocusCause::Keyboard {
            focused.open_now();
        }
    };
    let blurred = intent.clone();
    let on_blur = move |_: &mut EventCx<'_, events::FocusOut>| blurred.close_now();
    let on_key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
        // Only while it shows, so a dialog around it still closes on Escape.
        if ev.key == Key::Named(NamedKey::Escape) && intent.state().is_open_untracked() {
            ev.stop_propagation();
            intent.close_now();
        }
    };

    AnyView::new(view! {
        box(
            class = "lui-tooltip-trigger",
            node_ref = {place.area},
            on:pointer_enter = on_enter,
            on:pointer_move = on_move,
            on:pointer_leave = on_leave,
            on:pointer_down:capture = on_press,
            on:wheel:capture:passive = on_wheel,
            on:focus_in = on_focus,
            on:focus_out = on_blur,
            on:key_down = on_key_down,
            {..own},
            {..attrs},
            class = class
        ) {
            {children.into_view_once()}
        }
    })
}

/// What a [`Tooltip`] says. It takes no pointer, so the control under it stays hovered.
#[component]
pub fn TooltipContent(
    /// Classes merged after the tooltip's own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded, which lands on the label.
    #[prop(attrs)]
    attrs: Attrs,
    /// What it says.
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    install_stylesheet(ZUI_SHEET, TooltipStyle::CSS);
    let Some(TooltipLink { intent, place, .. }) = TooltipLink::current() else {
        return AnyView::new(());
    };
    let state = intent.state();
    let instant = intent.instant();
    let open = state.open_signal();

    // Read before it is published: a tooltip inside a dialog rises above the dialog.
    let at = SurfaceElevation::raise(OverlayLayer::Popover);
    at.publish();

    // Hidden until its size is known, so it is drawn where it belongs the first time.
    let positioner = NodeRef::new();
    let size = positioner.observe_border_size_while(move || open.get());
    let origin: Signal<Option<Point<CssPx, Css>>, LocalStorage> = Signal::derive_local(move || {
        let spot = place.spot.get()?;
        let size = size.get()?;
        if size.width.0 <= 0.0 && size.height.0 <= 0.0 {
            return None;
        }

        // On whole device pixels, so its edges and text are sharp.
        let scale = positioner.scale();
        let size = Size::new(CssPx(size.width.0 / scale), CssPx(size.height.0 / scale));
        let corner = beside_pointer(spot.pointer, size, spot.window);
        Some(Point::new(CssPx((corner.x.0 * scale).round() / scale), CssPx((corner.y.0 * scale).round() / scale)))
    });
    let left = move || origin.get().map(|corner| format!("{}px", corner.x.0));
    let top = move || origin.get().map(|corner| format!("{}px", corner.y.0));
    let visibility = move || origin.get().is_none().then(|| "hidden".to_owned());

    // Held rather than moved: the surface is built again every time it opens.
    let class = StoredValue::new_local(class);
    let attrs = StoredValue::new_local(attrs);
    let children = StoredValue::new_local(children);
    let own = move || {
        Attrs::new()
            .class_toggle(ClassName::new(TooltipStyle::CLASS), true)
            .class_toggle(ClassName::new("zui-tooltip"), true)
            .class_toggle(ClassName::new("lui-tooltip"), true)
            // Set on a tooltip taken down to show the next one, which then goes with no exit.
            .attribute(AttrName::new("data-instant"), move || instant.get().then(String::new))
    };

    AnyView::new(view! {
        Portal(layer = {at.layer()}) {
            Elevated(at = at, class = "lui-tooltip-layer") {
                Presence(present = open, surface = {state.content()}) {
                    box(
                        class = "lui-tooltip-positioner",
                        node_ref = positioner,
                        style:left = left,
                        style:top = top,
                        style:visibility = visibility
                    ) {
                        OverlaySurface(
                            state = state,
                            role = {Role::Tooltip},
                            {..own()},
                            {..attrs.get_value()},
                            class = {class.get_value()}
                        ) {
                            {children.get_value().view()}
                        }
                    }
                }
            }
        }
    })
}

/// The corner of a tooltip of `size` for the pointer at `pointer`: [`POINTER_GAP`] right of and
/// below it, or left of or above it on an axis where the window has no room, or against the
/// window's start where neither side has room.
fn beside_pointer(pointer: Point<CssPx, Css>, size: Size<CssPx, Css>, window: Size<CssPx, Css>) -> Point<CssPx, Css> {
    Point::new(
        CssPx(beside(pointer.x.0, size.width.0, window.width.0)),
        CssPx(beside(pointer.y.0, size.height.0, window.height.0)),
    )
}

/// Where a span of `length` starts on one axis, beside a pointer at `at` in a window of `room`.
fn beside(at: f32, length: f32, room: f32) -> f32 {
    let after = at + POINTER_GAP;
    if after + length <= room {
        return after;
    }

    let before = at - POINTER_GAP - length;
    if before >= 0.0 { before } else { (room - length).max(0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> Point<CssPx, Css> {
        Point::new(CssPx(x), CssPx(y))
    }

    fn size(width: f32, height: f32) -> Size<CssPx, Css> {
        Size::new(CssPx(width), CssPx(height))
    }

    #[test]
    fn a_tooltip_with_room_stands_right_of_and_below_the_pointer() {
        assert_eq!(beside_pointer(point(100.0, 50.0), size(80.0, 20.0), size(400.0, 300.0)), point(101.0, 51.0));
    }

    #[test]
    fn a_tooltip_crosses_the_pointer_on_each_axis_without_room() {
        let window = size(400.0, 300.0);

        assert_eq!(beside_pointer(point(380.0, 50.0), size(80.0, 20.0), window), point(299.0, 51.0));
        assert_eq!(beside_pointer(point(100.0, 290.0), size(80.0, 20.0), window), point(101.0, 269.0));
        assert_eq!(beside_pointer(point(380.0, 290.0), size(80.0, 20.0), window), point(299.0, 269.0));
    }

    #[test]
    fn a_tooltip_with_no_room_either_side_keeps_inside_the_window() {
        assert_eq!(beside_pointer(point(50.0, 10.0), size(90.0, 20.0), size(100.0, 300.0)), point(10.0, 11.0));
        assert_eq!(beside_pointer(point(50.0, 10.0), size(150.0, 20.0), size(100.0, 300.0)), point(0.0, 11.0));
    }
}
