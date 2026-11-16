use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Size;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::slot;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::ChildrenFn;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui::vocab::Orientation;
use zgui::vocab::Role;

use crate::chrome::slide::PanelWidth;
use crate::chrome::slide::SideEdge;
use crate::chrome::slide::Slide;
use crate::motion::Movement;

const SHEET: &str = "landaire-ui-side-split";

const CSS: &str = css!(
    ".lui-side-split {
        position: relative;
        display: flex;
        flex-direction: row;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
    }
    .lui-side-split__main {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }
    .lui-side-split[data-edge=\"left\"] > .lui-side-split__main {
        margin-left: var(--lui-side-inset, 0px);
    }
    .lui-side-split[data-edge=\"right\"] > .lui-side-split__main {
        margin-right: var(--lui-side-inset, 0px);
    }
    .lui-side-split__panel {
        position: absolute;
        top: 0;
        bottom: 0;
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        width: var(--lui-side-width);
        min-height: 0;
        overflow: hidden;
        background-color: var(--lui-color-panel);
        transition: transform var(--lui-motion-slide) var(--lui-motion-ease);
    }
    .lui-side-split[data-edge=\"left\"] > .lui-side-split__panel {
        left: 0;
        border-right: 1px solid var(--lui-color-border);
        transform: translateX(-100%);
    }
    .lui-side-split[data-edge=\"right\"] > .lui-side-split__panel {
        right: 0;
        border-left: 1px solid var(--lui-color-border);
        transform: translateX(100%);
    }
    .lui-side-split > .lui-side-split__panel[data-shown] {
        transform: none;
    }
    .lui-side-split > .lui-side-split__panel[data-away] {
        display: none;
    }
    .lui-side-split__edge {
        position: absolute;
        top: 0;
        bottom: 0;
        z-index: 1;
        width: 9px;
        padding: 0;
        border: none;
        background-color: transparent;
        cursor: col-resize;
    }
    .lui-side-split[data-edge=\"left\"] > .lui-side-split__edge {
        left: calc(var(--lui-side-width) - 5px);
    }
    .lui-side-split[data-edge=\"right\"] > .lui-side-split__edge {
        right: calc(var(--lui-side-width) - 5px);
    }
    .lui-side-split__line {
        position: absolute;
        top: 0;
        bottom: 0;
        left: 4px;
        width: 1px;
        background-color: transparent;
        transition: background-color var(--lui-motion-fade) var(--lui-motion-ease);
    }
    .lui-side-split__edge:hover > .lui-side-split__line,
    .lui-side-split__edge[data-dragging] > .lui-side-split__line,
    .lui-side-split__edge:focus-visible > .lui-side-split__line {
        background-color: var(--lui-color-accent);
    }
    .lui-side-split__edge:focus-visible {
        outline: none;
    }"
);

/// The panel beside a [`SideSplit`]'s main area. Built each time the panel opens, unless the split
/// keeps it mounted.
#[slot]
pub struct SidePanel {
    children: ChildrenFn,
}

/// The widths a side panel may be dragged between. A range of one width has no edge to drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidthRange {
    pub min: PanelWidth,
    pub max: PanelWidth,
}

impl WidthRange {
    /// A panel that is always `width` wide.
    pub const fn fixed(width: PanelWidth) -> Self {
        Self { min: width, max: width }
    }

    fn is_fixed(self) -> bool {
        self.min == self.max
    }

    /// The widest a panel may be in a split `split` pixels wide, leaving the main area
    /// `main_min`. Never narrower than the range allows.
    fn widest(self, split: Option<f32>, main_min: PanelWidth) -> f32 {
        let widest = split.map_or(self.max.0, |split| self.max.0.min(split - main_min.0));

        widest.max(self.min.0)
    }
}

/// Where a drag of the panel's edge began.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DragStart {
    pointer_x: f32,
    width: f32,
}

/// A main area with a panel at one edge that slides open and shut and is resized by dragging its
/// inner edge, unless its range is one width.
///
/// The panel slides over the main area and moves whole. The main area gives up the panel's width
/// only once the panel has come to rest open, and takes it back as a close starts, so it is laid
/// out once per movement rather than on every frame of one. It is the same element throughout, so
/// whatever it holds keeps its state while the panel comes and goes.
///
/// The width shown is `width` narrowed to leave the main area `main_min`, so a split too small
/// for the panel still shows its main area. `width` itself is kept for when the split grows
/// again.
// Props are named where the split is written, so their number costs a reader nothing.
#[allow(clippy::too_many_arguments)]
#[component(slot_aware)]
pub fn SideSplit(
    side_panel: SidePanel,
    #[prop(into)] open: Signal<bool, LocalStorage>,
    /// The width asked for, written as the edge is dragged.
    width: RwSignal<PanelWidth, LocalStorage>,
    range: WidthRange,
    #[prop(default = SideEdge::Left)] edge: SideEdge,
    #[prop(default = PanelWidth(160.0))] main_min: PanelWidth,
    /// What the draggable edge is called, for a screen reader.
    #[prop(into)]
    resize_label: String,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    /// Keeps the panel built, and hidden, while it is shut, so whatever it runs keeps running.
    #[prop(optional)]
    keep_mounted: bool,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let slide = Slide::follow(open, Movement::Slide);
    let root = NodeRef::new();
    let measured = root.observe_border_size();

    let split_width = move |size: Option<Size<DevicePx, Device>>| {
        let scale = root.scale();
        let scale = if scale > 0.0 { scale } else { 1.0 };

        size.map(|size| size.width.0 / scale)
    };
    // Tracked for what is drawn, untracked for what a listener computes.
    let widest = move || range.widest(split_width(measured.get()), main_min);
    let widest_now = move || range.widest(split_width(measured.get_untracked()), main_min);
    let shown = move || width.get().0.clamp(range.min.0, range.max.0).min(widest());
    let shown_now = move || width.get_untracked().0.clamp(range.min.0, range.max.0).min(widest_now());
    let inset = move || {
        let inset = if open.get() && slide.settled.get() { shown() } else { 0.0 };
        Some(format!("{inset}px"))
    };

    let dragging = RwSignal::new_local(None::<DragStart>);
    // Dragging towards the main area widens the panel.
    let toward_main = match edge {
        SideEdge::Left => 1.0,
        SideEdge::Right => -1.0,
    };
    let resize_to = move |requested: f32| width.set(PanelWidth(requested.clamp(range.min.0, range.max.0)));

    let edge_semantics = A11yBinding::new(Role::Splitter)
        .label(resize_label)
        .orientation(Orientation::Horizontal)
        .numeric_value(move || f64::from(shown()))
        .step(move |a11y| a11y.numeric_range(f64::from(range.min.0), f64::from(widest())).numeric_step(16.0));
    let edge_attrs = StoredValue::new_local(Attrs::new().a11y_from(edge_semantics));
    let panel = StoredValue::new_local(side_panel.children);

    view! {
        row(
            class = "lui-side-split",
            node_ref = root,
            attr:data-edge = edge.name(),
            var:--lui-side-width = move || Some(format!("{}px", shown())),
            var:--lui-side-inset = inset,
            {..attrs},
            class = class
        ) {
            column(class = "lui-side-split__main") {
                {children.into_view_once()}
            }
            if move || keep_mounted || slide.mounted.get() {
                column(
                    class = "lui-side-split__panel",
                    attr:data-shown = move || slide.shown.get().then(|| "true".to_owned()),
                    attr:data-away = move || (!slide.mounted.get()).then(|| "true".to_owned())
                ) {
                    {panel.get_value().view()}
                }
            } else {}
            if move || !range.is_fixed() && open.get() && slide.settled.get() {
                control(
                    class = "lui-side-split__edge",
                    tabindex = {Focus::Sequential},
                    attr:data-dragging = move || dragging.get().map(|_| "true".to_owned()),
                    on:pointer_down = move |ev| {
                        ev.capture_pointer();
                        dragging.set(Some(DragStart { pointer_x: ev.position.x.0, width: shown_now() }));
                    },
                    on:pointer_move = move |ev| {
                        let Some(start) = dragging.get_untracked() else {
                            return;
                        };

                        resize_to(start.width + (ev.position.x.0 - start.pointer_x) * toward_main);
                    },
                    on:pointer_up = move |ev| {
                        dragging.set(None);
                        ev.release_pointer();
                    },
                    on:pointer_cancel = move |ev| {
                        dragging.set(None);
                        ev.release_pointer();
                    },
                    on:key_down = move |ev| {
                        const STEP: f32 = 16.0;

                        let widen = match edge {
                            SideEdge::Left => NamedKey::ArrowRight,
                            SideEdge::Right => NamedKey::ArrowLeft,
                        };
                        let narrow = match edge {
                            SideEdge::Left => NamedKey::ArrowLeft,
                            SideEdge::Right => NamedKey::ArrowRight,
                        };
                        let now = shown_now();
                        match &ev.key {
                            Key::Named(key) if *key == widen => resize_to(now + STEP),
                            Key::Named(key) if *key == narrow => resize_to(now - STEP),
                            Key::Named(NamedKey::Home) => resize_to(range.min.0),
                            Key::Named(NamedKey::End) => resize_to(range.max.0),
                            _ => return,
                        }
                        ev.prevent_default();
                        ev.stop_propagation();
                    },
                    {..edge_attrs.get_value()}
                ) {
                    box(class = "lui-side-split__line", a11y:hidden = true)
                }
            } else {}
        }
    }
}
