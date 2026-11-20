//! A window inside the application's own window: moved by its title bar, resized by its edges,
//! folded to its title bar, and closed by its own button.

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::geom::Point;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::WithUntracked;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::ChildrenFn;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::chevron::CHEVRON_DOWN;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;
use zgui_ui_icons::set::mark::CROSS;

use crate::dock::layout::Share;
use crate::tooltip::TooltipContentProps;
use crate::tooltip::TooltipProps;
use crate::tooltip::TooltipTriggerProps;

const SHEET: &str = "landaire-ui-floating-window";

const CSS: &str = css!(
    ".lui-floating-area {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        z-index: 10;
        pointer-events: none;
    }
    .lui-floating-window {
        position: absolute;
        left: var(--lui-float-x);
        top: var(--lui-float-y);
        width: var(--lui-float-width);
        height: var(--lui-float-height);
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        min-height: 0;
        pointer-events: auto;
        border: 1px solid var(--lui-color-border-bright);
        border-radius: var(--lui-radius-lg);
        background-color: var(--lui-color-panel);
        color: var(--lui-color-text);
        box-shadow: var(--lui-shadow-overlay);
    }
    .lui-floating-window[data-collapsed] {
        height: auto;
    }
    .lui-floating-window[data-follows] {
        height: auto;
        min-height: var(--lui-float-min-height);
        max-height: var(--lui-float-max-height);
    }
    .lui-floating-window__bar {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: none;
        gap: 2px;
        height: 30px;
        padding: 0 4px;
        border-bottom: 1px solid var(--lui-color-border);
        cursor: grab;
    }
    .lui-floating-window[data-collapsed] > .lui-floating-window__bar {
        border-bottom: none;
    }
    .lui-floating-window__title {
        flex: 1 1 auto;
        min-width: 0;
        padding: 0 4px;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        font-weight: 600;
        color: var(--lui-color-text-bright);
    }
    .lui-floating-window__button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        width: 24px;
        height: 22px;
        border-radius: var(--lui-radius-lg);
        color: var(--lui-color-text-dim);
        cursor: pointer;
    }
    .lui-floating-window__button:hover {
        background-color: var(--lui-color-widget-hot);
        color: var(--lui-color-text-bright);
    }
    .lui-floating-window__button:focus-visible {
        outline: 1px solid var(--lui-color-accent);
        outline-offset: -2px;
    }
    .lui-floating-window__body {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-height: 0;
        padding: 8px;
        overflow-y: auto;
    }
    .lui-floating-window__grip {
        position: absolute;
        z-index: 1;
    }
    .lui-floating-window__grip[data-grip=\"north\"] { top: -3px; left: 6px; right: 6px; height: 6px; cursor: ns-resize; }
    .lui-floating-window__grip[data-grip=\"south\"] { bottom: -3px; left: 6px; right: 6px; height: 6px; cursor: ns-resize; }
    .lui-floating-window__grip[data-grip=\"west\"] { left: -3px; top: 6px; bottom: 6px; width: 6px; cursor: ew-resize; }
    .lui-floating-window__grip[data-grip=\"east\"] { right: -3px; top: 6px; bottom: 6px; width: 6px; cursor: ew-resize; }
    .lui-floating-window__grip[data-grip=\"north-west\"] { top: -3px; left: -3px; width: 12px; height: 12px; cursor: nwse-resize; }
    .lui-floating-window__grip[data-grip=\"south-east\"] { bottom: -3px; right: -3px; width: 12px; height: 12px; cursor: nwse-resize; }
    .lui-floating-window__grip[data-grip=\"north-east\"] { top: -3px; right: -3px; width: 12px; height: 12px; cursor: nesw-resize; }
    .lui-floating-window__grip[data-grip=\"south-west\"] { bottom: -3px; left: -3px; width: 12px; height: 12px; cursor: nesw-resize; }"
);

/// A width and a height, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatingSize {
    pub width: f32,
    pub height: f32,
}

/// The names a [`FloatingWindow`]'s buttons are read out and shown as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FloatingWindowStrings {
    pub collapse: String,
    pub expand: String,
    pub close: String,
}

impl FloatingWindowStrings {
    pub fn new() -> Self {
        Self { collapse: "Collapse".to_owned(), expand: "Expand".to_owned(), close: "Close".to_owned() }
    }
}

impl Default for FloatingWindowStrings {
    fn default() -> Self {
        Self::new()
    }
}

/// Where a window is and how large, in CSS pixels from its container's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// The limits a frame is held to.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Limits {
    /// The container's size, once it has been laid out.
    container: Option<FloatingSize>,
    min: FloatingSize,
    max_height: Option<Share>,
}

impl Limits {
    fn widest(self) -> f32 {
        self.container.map_or(f32::INFINITY, |container| container.width.max(0.0))
    }

    fn tallest(self) -> f32 {
        let Some(container) = self.container else { return f32::INFINITY };
        let share = self.max_height.map_or(1.0, |share| share.percent() as f32 / 100.0);

        (container.height * share).max(0.0)
    }

    /// The minimum width, given up where the container is narrower.
    fn narrowest(self) -> f32 {
        self.min.width.min(self.widest())
    }

    /// The minimum height, given up where the container or `max_height` allows less.
    fn shortest(self) -> f32 {
        self.min.height.min(self.tallest())
    }

    /// `frame` made to fit: no larger than the container and `max_height` allow, no smaller than
    /// the minimum where they allow that much, and inside the container.
    fn fit(self, frame: Frame) -> Frame {
        let width = frame.width.clamp(self.narrowest(), self.widest());
        let height = frame.height.clamp(self.shortest(), self.tallest());
        let Some(container) = self.container else { return Frame { width, height, ..frame } };

        Frame {
            x: frame.x.clamp(0.0, (container.width - width).max(0.0)),
            y: frame.y.clamp(0.0, (container.height - height).max(0.0)),
            width,
            height,
        }
    }

    /// [`Self::fit`], with the window `showing` high on screen rather than its full height, as it
    /// is while folded to its title bar. The full height is kept for when it unfolds.
    fn fit_showing(self, frame: Frame, showing: Option<f32>) -> Frame {
        let fitted = self.fit(frame);
        let (Some(container), Some(showing)) = (self.container, showing) else { return fitted };
        let showing = showing.min(fitted.height);

        Frame { y: frame.y.clamp(0.0, (container.height - showing).max(0.0)), ..fitted }
    }

    /// A frame of `size` in the middle of the container.
    fn centred(self, size: FloatingSize) -> Frame {
        let frame = self.fit(Frame { x: 0.0, y: 0.0, width: size.width, height: size.height });
        let Some(container) = self.container else { return frame };

        self.fit(Frame {
            x: (container.width - frame.width) / 2.0,
            y: (container.height - frame.height) / 2.0,
            ..frame
        })
    }
}

/// Which edges of the window a grip moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edges {
    north: bool,
    south: bool,
    west: bool,
    east: bool,
}

/// The eight grips around a window, by the name each is written with.
const GRIPS: [(&str, Edges); 8] = [
    ("north", Edges { north: true, south: false, west: false, east: false }),
    ("south", Edges { north: false, south: true, west: false, east: false }),
    ("west", Edges { north: false, south: false, west: true, east: false }),
    ("east", Edges { north: false, south: false, west: false, east: true }),
    ("north-west", Edges { north: true, south: false, west: true, east: false }),
    ("north-east", Edges { north: true, south: false, west: false, east: true }),
    ("south-west", Edges { north: false, south: true, west: true, east: false }),
    ("south-east", Edges { north: false, south: true, west: false, east: true }),
];

/// What a drag in progress does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    Move,
    Resize(Edges),
}

/// A drag in progress: what it does, and where the pointer and the window were when it began.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drag {
    gesture: Gesture,
    pointer: Point<CssPx, Css>,
    frame: Frame,
}

impl Drag {
    /// The frame the drag asks for with the pointer at `pointer`, before it is fitted.
    fn frame_at(self, pointer: Point<CssPx, Css>, limits: Limits) -> Frame {
        let dx = pointer.x.0 - self.pointer.x.0;
        let dy = pointer.y.0 - self.pointer.y.0;
        let start = self.frame;
        let edges = match self.gesture {
            Gesture::Move => return Frame { x: start.x + dx, y: start.y + dy, ..start },
            Gesture::Resize(edges) => edges,
        };

        let mut frame = start;
        // A west or north edge moves the window's origin, and the opposite edge stays put.
        if edges.west {
            let east = start.x + start.width;
            let x = (start.x + dx).clamp((east - limits.widest()).max(0.0), (east - limits.narrowest()).max(0.0));
            frame.x = x;
            frame.width = east - x;
        }
        if edges.east {
            frame.width = start.width + dx;
        }
        if edges.north {
            let south = start.y + start.height;
            let y = (start.y + dy).clamp((south - limits.tallest()).max(0.0), (south - limits.shortest()).max(0.0));
            frame.y = y;
            frame.height = south - y;
        }
        if edges.south {
            frame.height = start.height + dy;
        }
        // Growing past the container's far edge is held there rather than pushing the window.
        if let Some(container) = limits.container {
            frame.width = frame.width.min(container.width - frame.x);
            frame.height = frame.height.min(container.height - frame.y);
        }

        frame
    }
}

/// A window inside its nearest positioned ancestor, which it never leaves: a title bar that moves
/// it, a button that folds it to that bar and one that asks to close it, and edges and corners
/// that resize it. It is not modal; what is beside it stays usable.
///
/// It opens at `default_size` in the middle of its container, and keeps where it was put and its
/// size while it is shut. `max_height` limits its height to that share of the container's. The
/// container and `max_height` bound it before `min_size` does: where they allow less than the
/// minimum, the window is made smaller than the minimum rather than let out of its container.
/// With `height_follows_content`, its height is its content's, within those limits, and
/// `default_size`'s height is ignored; once a grip resizes its height, it keeps the height given.
/// Closing is the caller's: the close button runs `on_close`, which is expected to make `open`
/// false. The content is built each time it opens.
#[allow(clippy::too_many_arguments, reason = "each prop is a named field of the props builder")]
#[component]
pub fn FloatingWindow(
    #[prop(into)] open: Signal<bool, LocalStorage>,
    /// Run by the close button.
    on_close: UnsyncCallback<()>,
    /// Shown in the title bar, and what a screen reader calls the window.
    #[prop(into)]
    title: Signal<String, LocalStorage>,
    #[prop(default = FloatingSize { width: 500.0, height: 400.0 })] default_size: FloatingSize,
    #[prop(default = FloatingSize { width: 160.0, height: 80.0 })] min_size: FloatingSize,
    #[prop(optional)] max_height: Option<Share>,
    /// Makes the window as high as its content until a grip resizes its height.
    #[prop(optional)]
    height_follows_content: bool,
    /// The buttons' names. Left out, English.
    #[prop(optional)]
    strings: Option<FloatingWindowStrings>,
    /// Classes merged after the window's own.
    #[prop(into, optional)]
    class: Classes,
    /// Forwarded to the window.
    #[prop(attrs)]
    attrs: Attrs,
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let area = NodeRef::new();
    let measured = area.observe_border_size();
    let container = move |size: Option<zgui::geom::Size<zgui::geom::DevicePx, zgui::geom::Device>>| {
        let scale = area.scale();
        let scale = if scale > 0.0 { scale } else { 1.0 };

        size.map(|size| FloatingSize { width: size.width.0 / scale, height: size.height.0 / scale })
    };
    let limits = Memo::new(move |_| Limits { container: container(measured.get()), min: min_size, max_height });

    // `None` until the window is first moved or resized: until then it follows the container's
    // middle, so a container laid out after the window opened still centres it.
    let placed = RwSignal::new_local(None::<Frame>);
    let collapsed = RwSignal::new_local(false);
    let frame_node = NodeRef::new();
    let frame_size = frame_node.observe_border_size();
    let height_given = RwSignal::new_local(false);
    let follows = Memo::new(move |_| height_follows_content && !height_given.get());
    // The window's height as laid out, which is its content's while it follows its content.
    let laid_out_height = move || {
        let scale = area.scale();
        let scale = if scale > 0.0 { scale } else { 1.0 };

        frame_size.get().map(|size| size.height.0 / scale)
    };
    // How high the window shows while folded, which is what its position is held to then.
    let folded_height = Memo::new(move |_| if collapsed.get() { laid_out_height() } else { None });
    let content_height = Memo::new(move |_| if follows.get() && !collapsed.get() { laid_out_height() } else { None });
    let shown = Memo::new(move |_| {
        let limits = limits.get();
        let content = content_height.get();
        match placed.get() {
            Some(frame) => {
                let frame = Frame { height: content.unwrap_or(frame.height), ..frame };
                limits.fit_showing(frame, folded_height.get())
            }
            None => limits.centred(FloatingSize { height: content.unwrap_or(default_size.height), ..default_size }),
        }
    });
    // Once its content has a height, a window that follows it stays where it was centred at that
    // height, so a change of content, or of where the pixel grid falls, does not move it.
    let settling = RenderEffect::new(move |_| {
        if placed.with_untracked(Option::is_some) {
            return;
        }
        let (Some(height), limits) = (content_height.get(), limits.get()) else { return };
        if limits.container.is_some() {
            placed.set(Some(limits.centred(FloatingSize { height, ..default_size })));
        }
    });
    on_cleanup_local(move || drop(settling));
    let dragging = RwSignal::new_local(None::<Drag>);

    let strings = StoredValue::new_local(strings.unwrap_or_default());
    let class = StoredValue::new_local(Classes::from("lui-floating-window").merged(class));
    let attrs = StoredValue::new_local(attrs);
    let children = StoredValue::new_local(children);

    let begin = move |gesture: Gesture, pointer: Point<CssPx, Css>| {
        let frame = shown.get_untracked();
        if let Gesture::Resize(edges) = gesture
            && (edges.north || edges.south)
            && follows.get_untracked()
        {
            // The height the content gave it becomes the height it is resized from.
            placed.set(Some(frame));
            height_given.set(true);
        }
        dragging.set(Some(Drag { gesture, pointer, frame }));
    };
    let follow = move |pointer: Point<CssPx, Css>| {
        let Some(drag) = dragging.get_untracked() else { return };
        let limits = limits.get_untracked();

        placed.set(Some(limits.fit_showing(drag.frame_at(pointer, limits), folded_height.get_untracked())));
    };
    let end = move || dragging.set(None);

    let window = move || {
        // A drag the window closed during never saw its release.
        on_cleanup_local(end);
        let semantics = A11yBinding::new(Role::Dialog).label(move || title.get());
        let collapse_label = Signal::derive_local(move || {
            let strings = strings.get_value();
            if collapsed.get() { strings.expand } else { strings.collapse }
        });
        let collapse_icon = Signal::derive_local(move || if collapsed.get() { CHEVRON_RIGHT } else { CHEVRON_DOWN });
        let close_label = Signal::stored_local(strings.get_value().close);
        let grips = move || {
            GRIPS
                .into_iter()
                .map(|(name, edges)| {
                    AnyView::new(view! {
                        box(
                            class = "lui-floating-window__grip",
                            attr:data-grip = name,
                            a11y:hidden = true,
                            on:pointer_down = move |ev| {
                                if ev.button != Some(PointerButton::Primary) {
                                    return;
                                }
                                ev.stop_propagation();
                                ev.capture_pointer();
                                begin(Gesture::Resize(edges), ev.position);
                            },
                            on:pointer_move = move |ev| follow(ev.position),
                            on:pointer_up = move |ev| {
                                end();
                                ev.release_pointer();
                            },
                            on:pointer_cancel = move |ev| {
                                end();
                                ev.release_pointer();
                            }
                        )
                    })
                })
                .collect::<Vec<_>>()
        };

        view! {
            column(
                node_ref = frame_node,
                var:--lui-float-x = move || Some(format!("{}px", shown.get().x)),
                var:--lui-float-y = move || Some(format!("{}px", shown.get().y)),
                var:--lui-float-width = move || Some(format!("{}px", shown.get().width)),
                var:--lui-float-height = move || Some(format!("{}px", shown.get().height)),
                var:--lui-float-min-height = move || Some(format!("{}px", limits.get().shortest())),
                var:--lui-float-max-height = move || {
                    let tallest = limits.get().tallest();
                    tallest.is_finite().then(|| format!("{tallest}px"))
                },
                attr:data-collapsed = move || collapsed.get().then(String::new),
                attr:data-follows = move || (follows.get() && !collapsed.get()).then(String::new),
                {..Attrs::new().a11y_from(semantics)},
                {..attrs.get_value()},
                class = class.get_value()
            ) {
                row(
                    class = "lui-floating-window__bar",
                    on:pointer_down = move |ev| {
                        if ev.button != Some(PointerButton::Primary) {
                            return;
                        }
                        ev.capture_pointer();
                        begin(Gesture::Move, ev.position);
                    },
                    on:pointer_move = move |ev| follow(ev.position),
                    on:pointer_up = move |ev| {
                        end();
                        ev.release_pointer();
                    },
                    on:pointer_cancel = move |ev| {
                        end();
                        ev.release_pointer();
                    }
                ) {
                    {bar_button("collapse", collapse_icon, collapse_label, move || {
                        end();
                        collapsed.update(|folded| *folded = !*folded);
                    })}
                    text(class = "lui-floating-window__title") {{move || title.get()}}
                    {bar_button("close", Signal::stored_local(CROSS), close_label, move || on_close.run(()))}
                }
                if move || !collapsed.get() {
                    column(class = "lui-floating-window__body") {
                        {children.get_value().view()}
                    }
                    {grips()}
                } else {}
            }
        }
    };

    view! {
        box(class = "lui-floating-area", node_ref = area) {
            if move || open.get() {
                {window()}
            } else {}
        }
    }
}

/// One of the title bar's buttons.
fn bar_button(
    kind: &'static str,
    icon: Signal<IconData, LocalStorage>,
    label: Signal<String, LocalStorage>,
    act: impl Fn() + 'static,
) -> AnyView {
    let semantics = A11yBinding::new(Role::Button).label(move || label.get());

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                control(
                    class = "lui-floating-window__button",
                    attr:data-kind = kind,
                    tabindex = {Focus::Sequential},
                    // A press here is not the start of a move.
                    on:pointer_down = |ev| ev.stop_propagation(),
                    on:click = move |_| act(),
                    {..Attrs::new().a11y_from(semantics)}
                ) {
                    Icon(icon = icon, size = IconSize::Sm)
                }
            }
            TooltipContent {{move || label.get()}}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        container: Some(FloatingSize { width: 800.0, height: 600.0 }),
        min: FloatingSize { width: 100.0, height: 50.0 },
        max_height: None,
    };

    fn at(x: f32, y: f32) -> Point<CssPx, Css> {
        Point::new(CssPx(x), CssPx(y))
    }

    #[test]
    fn a_west_edge_dragged_past_the_minimum_stops_without_moving_the_east_edge() {
        let drag = Drag {
            gesture: Gesture::Resize(GRIPS[2].1),
            pointer: at(100.0, 100.0),
            frame: Frame { x: 100.0, y: 100.0, width: 300.0, height: 200.0 },
        };

        let frame = LIMITS.fit(drag.frame_at(at(500.0, 100.0), LIMITS));

        assert_eq!(frame.x + frame.width, 400.0, "the east edge stays where it was");
        assert_eq!(frame.width, 100.0, "the window keeps its minimum width");
    }

    #[test]
    fn a_window_larger_than_its_container_is_held_to_it_at_the_origin() {
        let frame = LIMITS.fit(Frame { x: 50.0, y: 50.0, width: 2000.0, height: 2000.0 });

        assert_eq!(frame, Frame { x: 0.0, y: 0.0, width: 800.0, height: 600.0 });
    }
}
