//! A side panel on the left of a main area, resizable at its trailing edge
//! and sliding open and shut.
//!
//! The main area keeps one element path whether the panel is open, moving
//! or shut, so element state under it (fades, scroll offsets, measured
//! sizes) survives a toggle. A resizable panel group cannot give that: the
//! main area sits under the group's panel ids, which a layout without the
//! side panel does not have.

use std::ops::Range;
use std::sync::Arc;

use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Axis;
use gpui_kit::Context;
use gpui_kit::DragMoveEvent;
use gpui_kit::ElementId;
use gpui_kit::Empty;
use gpui_kit::Entity;
use gpui_kit::EntityId;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::Pixels;
use gpui_kit::Render;
use gpui_kit::RenderOnce;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::ElementExt as _;
use gpui_kit::component::resizable::resize_handle;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;

use crate::motion;

/// How wide a side panel is, kept within its range.
pub struct SideWidth {
    width: Pixels,
    range: Range<Pixels>,

    /// The split's own width as last laid out; `None` before its first frame.
    container: Option<Pixels>,
}

impl SideWidth {
    pub fn new(width: Pixels, range: Range<Pixels>) -> Self {
        Self { width: width.clamp(range.start, range.end), range, container: None }
    }

    /// The width drawn: the one asked for, narrowed to leave the main area
    /// room in a split too small for it. The width asked for is kept for
    /// when the split grows again.
    fn shown(&self) -> Pixels {
        const MAIN_MIN_WIDTH: Pixels = px(160.);

        let Some(container) = self.container else {
            return self.width;
        };

        self.width.min(container - MAIN_MIN_WIDTH).max(self.range.start)
    }

    fn set_width(&mut self, width: Pixels, cx: &mut Context<Self>) {
        let width = width.clamp(self.range.start, self.range.end);
        if width != self.width {
            self.width = width;
            cx.notify();
        }
    }
}

/// What a drag on a side panel's edge carries: the width it resizes. Drags
/// are told apart by payload type, so every split hears every edge drag.
#[derive(Clone)]
struct ResizeSide(EntityId);

impl Render for ResizeSide {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// `side` on the left of `main`, shown while `open` and sliding between.
#[derive(IntoElement)]
pub struct SideSplit {
    id: ElementId,
    width: Entity<SideWidth>,
    open: bool,
    side: AnyElement,
    main: AnyElement,
}

impl SideSplit {
    /// `id` keys the slide and the edge, so it has to be unique within the
    /// view that draws the split.
    pub fn new(
        id: impl Into<ElementId>,
        width: &Entity<SideWidth>,
        open: bool,
        side: impl IntoElement,
        main: impl IntoElement,
    ) -> Self {
        Self { id: id.into(), width: width.clone(), open, side: side.into_any_element(), main: main.into_any_element() }
    }
}

impl RenderOnce for SideSplit {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let shown = motion::slide_progress(self.id.clone(), self.open, window, cx);
        let settled = motion::settled(shown);
        let width = self.width.read(cx).shown();
        let border = cx.theme().border;

        // While it moves the handle is away, and the line it draws at rest
        // goes with the panel.
        let side = (self.open || !settled).then(|| {
            let side = div().relative().size_full().bg(cx.theme().background).child(self.side).when(!settled, |side| {
                side.child(div().absolute().top_0().bottom_0().right(px(-1.)).w(px(1.)).bg(border))
            });
            motion::slide_frame(width, shown.value, motion::Edge::Left, side.into_any_element())
        });

        // Last, so it draws over the main area's edge as well as the side's.
        let edge = (self.open && settled).then(|| {
            let id = ElementId::NamedChild(Arc::new(self.id.clone()), "edge".into());
            let handle =
                resize_handle(id, Axis::Horizontal).on_drag(ResizeSide(self.width.entity_id()), |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                });
            div().absolute().top_0().bottom_0().left(width).child(handle)
        });

        let main =
            div().flex_1().min_w(px(0.)).h_full().ml(motion::slide_inset(width, shown, self.open)).child(self.main);

        let state = self.width;
        let measured = state.clone();
        div()
            .relative()
            .flex()
            .flex_row()
            .size_full()
            .overflow_hidden()
            .on_prepaint(move |bounds, window, cx| {
                let container = Some(bounds.size.width);
                if measured.read(cx).container == container {
                    return;
                }
                measured.update(cx, |state, _| state.container = container);
                // Laid out this frame at the old width, and a notify raised
                // during a draw does not schedule the next one.
                let measured = measured.clone();
                window.defer(cx, move |_, cx| measured.update(cx, |_, cx| cx.notify()));
            })
            .on_drag_move(move |event: &DragMoveEvent<ResizeSide>, _, cx| {
                if event.drag(cx).0 != state.entity_id() {
                    return;
                }
                let width = event.event.position.x - event.bounds.left();
                state.update(cx, |state, cx| state.set_width(width, cx));
            })
            .child(main)
            .children(side)
            .children(edge)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use gpui_kit::AppContext as _;
    use gpui_kit::Context;
    use gpui_kit::Entity;
    use gpui_kit::InteractiveElement as _;
    use gpui_kit::IntoElement;
    use gpui_kit::Pixels;
    use gpui_kit::Render;
    use gpui_kit::RenderOnce;
    use gpui_kit::Styled as _;
    use gpui_kit::TestAppContext;
    use gpui_kit::Window;
    use gpui_kit::WindowHandle;
    use gpui_kit::base::TestSupportExt as _;
    use gpui_kit::div;
    use gpui_kit::point;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt as _;

    use super::SideSplit;
    use super::SideWidth;

    const WIDTH: Pixels = px(200.);

    /// Counts how often its keyed state is made, which happens again only
    /// when its element path changes.
    #[derive(IntoElement)]
    struct Probe {
        made: Rc<Cell<usize>>,
    }

    impl RenderOnce for Probe {
        fn render(self, window: &mut Window, cx: &mut gpui_kit::App) -> impl gpui_kit::IntoElement {
            let made = self.made;
            window.use_keyed_state("probe", cx, move |_, _| made.set(made.get() + 1));

            div().id("main").test_support().size_full()
        }
    }

    struct Harness {
        width: Entity<SideWidth>,
        open: bool,
        made: Rc<Cell<usize>>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui_kit::IntoElement {
            SideSplit::new(
                "side",
                &self.width,
                self.open,
                div().id("side").test_support().size_full(),
                Probe { made: Rc::clone(&self.made) },
            )
        }
    }

    fn open_harness(cx: &mut TestAppContext) -> (WindowHandle<gpui_kit::component::Root>, Entity<Harness>) {
        open_harness_at(px(800.), WIDTH, cx)
    }

    fn open_harness_at(
        window_width: Pixels,
        side_width: Pixels,
        cx: &mut TestAppContext,
    ) -> (WindowHandle<gpui_kit::component::Root>, Entity<Harness>) {
        cx.update(gpui_kit::init);
        let harness = std::cell::RefCell::new(None);
        let window = cx.open_window(size(window_width, px(600.)), |window, cx| {
            let view = cx.new(|cx| Harness {
                width: cx.new(|_| SideWidth::new(side_width, px(100.)..px(400.))),
                open: true,
                made: Rc::default(),
            });
            *harness.borrow_mut() = Some(view.clone());
            gpui_kit::component::Root::new(view, window, cx)
        });
        let harness = harness.borrow_mut().take().expect("the harness was built inside the window");
        (window, harness)
    }

    /// Where the side and the main area are on the next frame, after `ms`.
    fn frame_after(
        ms: u64,
        window: WindowHandle<gpui_kit::component::Root>,
        cx: &mut TestAppContext,
    ) -> (Option<gpui_kit::Bounds<Pixels>>, gpui_kit::Bounds<Pixels>) {
        cx.executor().advance_clock(Duration::from_millis(ms));
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            (window.try_find("side").map(|side| side.bounds()), window.find("main").bounds())
        })
        .expect("the window is open")
    }

    fn set_open(open: bool, harness: &Entity<Harness>, cx: &mut TestAppContext) {
        harness.update(cx, |harness, cx| {
            harness.open = open;
            cx.notify();
        });
    }

    /// The main area keeps its element state through a close and a reopen,
    /// and is laid out anew once per move: wide as a close starts, narrow
    /// as an open ends.
    #[gpui_kit::test]
    fn the_main_area_survives_the_side_sliding(cx: &mut TestAppContext) {
        let (window, harness) = open_harness(cx);
        let made = harness.read_with(cx, |harness, _| Rc::clone(&harness.made));

        let (side, main) = frame_after(0, window, cx);
        assert_eq!(side.map(|side| side.left()), Some(px(0.)));
        assert_eq!(main.left(), WIDTH);

        set_open(false, &harness, cx);
        frame_after(0, window, cx);
        let (side, main) = frame_after(60, window, cx);
        let side = side.expect("the side shows while it slides shut");
        assert!(side.left() < px(0.) && side.right() > px(0.), "the side slides whole, at {side:?}");
        assert_eq!(main.left(), px(0.), "the main area widens as the close starts");
        let (side, main) = frame_after(400, window, cx);
        assert!(side.is_none(), "the side is gone once shut");
        assert_eq!(main.left(), px(0.));

        set_open(true, &harness, cx);
        frame_after(0, window, cx);
        let (side, main) = frame_after(60, window, cx);
        let side = side.expect("the side shows while it slides open");
        assert!(side.left() < px(0.) && side.right() > px(0.), "the side slides whole, at {side:?}");
        assert_eq!(main.left(), px(0.), "the main area keeps its width while the side slides over it");
        let (side, main) = frame_after(400, window, cx);
        assert_eq!(side.map(|side| side.left()), Some(px(0.)));
        assert_eq!(main.left(), WIDTH, "the main area narrows once the open ends");

        assert_eq!(made.get(), 1, "the main area's keyed state was made once");
    }

    /// The edge drags once the side has been shut and opened again.
    #[gpui_kit::test]
    fn the_edge_drags_after_a_reopen(cx: &mut TestAppContext) {
        let (window, harness) = open_harness(cx);

        frame_after(0, window, cx);
        set_open(false, &harness, cx);
        frame_after(0, window, cx);
        frame_after(400, window, cx);
        set_open(true, &harness, cx);
        frame_after(0, window, cx);
        let (_, main) = frame_after(400, window, cx);
        assert_eq!(main.left(), WIDTH);

        cx.update_window(window.into(), |_, window, cx| {
            let from = point(WIDTH, px(300.));
            window.drag(from, point(from.x + px(60.), from.y), cx);
        })
        .expect("the window is open");

        let (side, main) = frame_after(0, window, cx);
        assert_eq!(main.left(), WIDTH + px(60.));
        assert_eq!(side.map(|side| side.size.width), Some(WIDTH + px(60.)));
    }

    /// A split narrower than the side asks for leaves the main area room,
    /// and the edge in view to drag.
    #[gpui_kit::test]
    fn a_narrow_split_leaves_the_main_area_room(cx: &mut TestAppContext) {
        let (window, _harness) = open_harness_at(px(450.), px(400.), cx);

        frame_after(0, window, cx);
        let (side, main) = frame_after(0, window, cx);

        assert_eq!(main.left(), px(290.), "the main area keeps its minimum");
        assert_eq!(side.map(|side| side.size.width), Some(px(290.)));
    }
}
