//! The border and resize grips of a window whose platform draws no frame.

use std::rc::Rc;

use zgui::component;
use zgui::css;
use zgui::platform::ResizeEdge;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::Classes;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;

use crate::chrome::frame_window::FrameWindow;
use crate::chrome::frame_window::or_current;

/// What the frame rules are installed under.
const SHEET: &str = "landaire-ui-window-frame";

// `--lui-window-grip` is read by a `TitleBar` inside, to keep its controls clear of the grips.
const CSS: &str = css!(
    ".lui-window-frame {
        position: relative;
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        box-sizing: border-box;
        overflow: hidden;
        background-color: var(--lui-color-surface);
    }
    .lui-window-frame[data-framed] {
        --lui-window-grip: 8px;
        border: 1px solid var(--lui-color-border-bright);
        border-radius: 8px;
    }
    .lui-window-frame__grip {
        position: absolute;
        z-index: 30;
    }
    .lui-window-frame__grip[data-edge=\"n\"],
    .lui-window-frame__grip[data-edge=\"s\"] {
        left: var(--lui-window-grip);
        right: var(--lui-window-grip);
        height: var(--lui-window-grip);
    }
    .lui-window-frame__grip[data-edge=\"w\"],
    .lui-window-frame__grip[data-edge=\"e\"] {
        top: var(--lui-window-grip);
        bottom: var(--lui-window-grip);
        width: var(--lui-window-grip);
    }
    .lui-window-frame__grip[data-edge=\"nw\"],
    .lui-window-frame__grip[data-edge=\"ne\"],
    .lui-window-frame__grip[data-edge=\"sw\"],
    .lui-window-frame__grip[data-edge=\"se\"] {
        width: var(--lui-window-grip);
        height: var(--lui-window-grip);
    }
    .lui-window-frame__grip[data-edge=\"n\"],
    .lui-window-frame__grip[data-edge=\"nw\"],
    .lui-window-frame__grip[data-edge=\"ne\"] { top: 0; }
    .lui-window-frame__grip[data-edge=\"s\"],
    .lui-window-frame__grip[data-edge=\"sw\"],
    .lui-window-frame__grip[data-edge=\"se\"] { bottom: 0; }
    .lui-window-frame__grip[data-edge=\"w\"],
    .lui-window-frame__grip[data-edge=\"nw\"],
    .lui-window-frame__grip[data-edge=\"sw\"] { left: 0; }
    .lui-window-frame__grip[data-edge=\"e\"],
    .lui-window-frame__grip[data-edge=\"ne\"],
    .lui-window-frame__grip[data-edge=\"se\"] { right: 0; }
    .lui-window-frame__grip[data-edge=\"n\"],
    .lui-window-frame__grip[data-edge=\"s\"] { cursor: ns-resize; }
    .lui-window-frame__grip[data-edge=\"w\"],
    .lui-window-frame__grip[data-edge=\"e\"] { cursor: ew-resize; }
    .lui-window-frame__grip[data-edge=\"nw\"],
    .lui-window-frame__grip[data-edge=\"se\"] { cursor: nwse-resize; }
    .lui-window-frame__grip[data-edge=\"ne\"],
    .lui-window-frame__grip[data-edge=\"sw\"] { cursor: nesw-resize; }"
);

/// One edge or corner a [`WindowFrame`] resizes from.
#[derive(Clone, Copy, Debug)]
struct Side {
    edge: ResizeEdge,
    /// What its grip is styled by.
    name: &'static str,
}

const SIDES: [Side; 8] = [
    Side { edge: ResizeEdge::North, name: "n" },
    Side { edge: ResizeEdge::South, name: "s" },
    Side { edge: ResizeEdge::West, name: "w" },
    Side { edge: ResizeEdge::East, name: "e" },
    Side { edge: ResizeEdge::NorthWest, name: "nw" },
    Side { edge: ResizeEdge::NorthEast, name: "ne" },
    Side { edge: ResizeEdge::SouthWest, name: "sw" },
    Side { edge: ResizeEdge::SouthEast, name: "se" },
];

/// A window's whole content, framed when the platform draws no frame for it.
///
/// There the content gets a 1px border with rounded corners and an 8px resize grip along each
/// edge and corner, unless the window is maximised or full screen. Elsewhere the content is shown
/// as it is. The corners show the desktop only in a window opened transparent.
#[component]
pub fn WindowFrame(
    /// The window it reads and acts on. Left out, the window it is in.
    #[prop(optional)]
    window: Option<Rc<dyn FrameWindow>>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let window = or_current(window);
    let source = window.decoration_source();
    let maximized = window.maximized();
    let fullscreen = window.fullscreen();
    let framed: Signal<bool, LocalStorage> =
        Signal::derive_local(move || source.get().is_application() && !maximized.get() && fullscreen.get().is_none());

    let grips = move || -> Vec<AnyView> { SIDES.into_iter().map(|side| grip(Rc::clone(&window), side)).collect() };
    let class = Classes::from("lui-window-frame").merged(class);

    view! {
        column(attr:data-framed = move || framed.get().then(|| "true".to_owned()), {..attrs}, class = class) {
            {children.into_view_once()}
            if move || framed.get() {
                {grips()}
            } else {}
        }
    }
}

/// The strip along `side` that resizes the window from it.
fn grip(window: Rc<dyn FrameWindow>, side: Side) -> AnyView {
    let Side { edge, name } = side;
    let on_press = move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button != Some(PointerButton::Primary) || !ev.primary {
            return;
        }

        ev.stop_propagation();
        window.begin_resize_drag(edge);
    };

    AnyView::new(view! {
        box(
            class = "lui-window-frame__grip",
            attr:data-edge = name,
            a11y:hidden = true,
            on:pointer_down = on_press
        ) {}
    })
}
