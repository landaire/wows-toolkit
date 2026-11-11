//! Movement holds still when the system asks for reduced motion.
//!
//! The theme's own move curve front-loads nearly all of its travel, so at
//! these lengths a move reads as a jump. The app's moves use a standard
//! ease-in-out over durations long enough to be followed by eye.

use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use gpui_kit::Animation;
use gpui_kit::AnimationExt as _;
use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::Bounds;
use gpui_kit::ContentMask;
use gpui_kit::Display;
use gpui_kit::Element;
use gpui_kit::ElementId;
use gpui_kit::GlobalElementId;
use gpui_kit::InspectorElementId;
use gpui_kit::IntoElement;
use gpui_kit::LayoutId;
use gpui_kit::ParentElement as _;
use gpui_kit::Pixels;
use gpui_kit::RenderOnce;
use gpui_kit::Style;
use gpui_kit::Styled;
use gpui_kit::Window;
use gpui_kit::base::Easing;
use gpui_kit::base::MotionStatus;
use gpui_kit::base::MotionValue;
use gpui_kit::base::Transition;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::Size;
use gpui_kit::div;
use gpui_kit::px;
use gpui_kit::radians;
use gpui_kit::relative;

/// How long a caret takes to turn.
const TURN: Duration = Duration::from_millis(240);

/// How long a block of content takes to open or close.
const REVEAL: Duration = Duration::from_millis(170);

/// How long a side panel takes to slide open or shut.
const SLIDE: Duration = Duration::from_millis(170);

/// How long a surface takes to fade in.
const FADE: Duration = Duration::from_millis(220);

/// The standard ease-in-out: a gentle start and end around a steady middle,
/// so most of the travel is visible.
fn standard() -> Easing {
    Easing::cubic_bezier(0.4, 0.0, 0.2, 1.0).expect("the curve's control points lie in 0..=1")
}

/// How far a disclosure has turned, from 0 closed to 1 open.
///
/// Two calls that share `id` in one frame with different `open` values
/// retarget each other every frame and never settle.
pub fn disclosure_turn(id: impl Into<ElementId>, open: bool, window: &mut Window, cx: &mut App) -> f32 {
    let policy = Transition::new(TURN).easing(standard());
    // The first draw adopts the state as it is, so a caret scrolled into
    // view or drawn already open does not turn.
    gpui_kit::base::transition((id.into(), "disclosure"), if open { 1.0_f32 } else { 0.0 }, policy, window, cx)
}

/// How far a block of content has opened, from 0 closed to 1 open, and
/// whether it is still moving.
///
/// The id rule of [`disclosure_turn`] applies.
pub fn reveal_progress(id: impl Into<ElementId>, open: bool, window: &mut Window, cx: &mut App) -> MotionValue<f32> {
    let policy = Transition::new(REVEAL).easing(standard());
    gpui_kit::base::transition_with_status((id.into(), "reveal"), if open { 1.0_f32 } else { 0.0 }, policy, window, cx)
}

/// Which edge a side panel is attached to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
}

/// How far a side panel has slid open, from 0 shut to 1 open, and whether
/// it is still moving. The id rule of [`disclosure_turn`] applies.
pub fn slide_progress(id: impl Into<ElementId>, open: bool, window: &mut Window, cx: &mut App) -> MotionValue<f32> {
    let policy = Transition::new(SLIDE).easing(standard());
    gpui_kit::base::transition_with_status((id.into(), "slide"), if open { 1.0_f32 } else { 0.0 }, policy, window, cx)
}

/// A side panel part way open: its slot holds the full `width` for the whole
/// move, and the content slides across it from the window's edge, clipped
/// at the slot. Only the panel moves; what sits beside it is laid out once
/// at the start of the move rather than reflowing on every frame, which in
/// a table costs more than a frame.
pub fn slide_frame(width: Pixels, progress: f32, edge: Edge, content: AnyElement) -> AnyElement {
    let hidden = width * (1.0 - progress.clamp(0.0, 1.0));
    let inner = div().absolute().top_0().h_full().w(width).child(content);
    let inner = match edge {
        Edge::Left => inner.left(-hidden),
        Edge::Right => inner.left(hidden),
    };
    div().relative().flex_none().h_full().w(width).overflow_hidden().child(inner).into_any_element()
}

/// Whether a slide or reveal has come to rest.
pub fn settled(motion: MotionValue<f32>) -> bool {
    matches!(motion.status, MotionStatus::Idle | MotionStatus::Finished)
}

/// A caret that points right at `turn` 0 and down at 1.
pub fn chevron(turn: f32) -> Icon {
    Icon::new(IconName::ChevronRight).rotate(radians(turn * FRAC_PI_2))
}

/// A caret that turns as `open` changes. Its state is keyed by `id` under
/// the enclosing element ids, so it suits sites whose parents are not
/// positional; list rows use `disclosure_turn` keyed by a stable row key.
/// The failure mode of `disclosure_turn` applies to `id`.
#[derive(IntoElement)]
pub struct DisclosureChevron {
    id: ElementId,
    open: bool,
    size: Option<Size>,
}

impl DisclosureChevron {
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self { id: id.into(), open, size: None }
    }
}

impl Sizable for DisclosureChevron {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = Some(size.into());
        self
    }
}

impl RenderOnce for DisclosureChevron {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let icon = chevron(disclosure_turn(self.id, self.open, window, cx));
        match self.size {
            Some(size) => icon.with_size(size),
            None => icon,
        }
    }
}

/// Fades `element` in once, when it is first drawn under `id`.
pub fn fade_in<E>(id: impl Into<ElementId>, element: E) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let easing = standard();
    let animation = Animation::new(FADE).with_easing(move |delta| easing.sample(delta));
    element.with_animation(id, animation, |element, delta| element.opacity(delta)).into_any_element()
}

/// A block of content shown to part of its height, clipped below.
///
/// The content is laid out in place whether moving or open, so it wraps the
/// same while it moves as once it settles; only the height shown changes.
/// Unlike the kit's `MotionReveal`, it also stays in the tree once fully
/// open, recording its height, so a close starts from that height rather
/// than from nothing.
pub struct Reveal {
    id: ElementId,
    extent: Extent,
    child: AnyElement,
}

/// How much of a [`Reveal`]'s content shows.
#[derive(Clone, Copy)]
pub enum Extent {
    /// Opening or closing, at this share of the height from 0 to 1.
    Moving(f32),
    /// Fully open, at whatever height the content takes.
    Open,
}

impl Reveal {
    pub fn new(id: impl Into<ElementId>, extent: Extent, child: AnyElement) -> Self {
        let extent = match extent {
            Extent::Moving(progress) => Extent::Moving(progress.clamp(0.0, 1.0)),
            Extent::Open => Extent::Open,
        };
        Self { id: id.into(), extent, child }
    }
}

impl IntoElement for Reveal {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The child's height when last laid out.
#[derive(Clone, Copy, Default)]
struct Measured(Option<Pixels>);

impl Element for Reveal {
    /// The content's own layout node, whose height is what gets revealed.
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child = self.child.request_layout(window, cx);
        // Block layout, the default, named because it is load-bearing: the
        // content keeps its natural height inside a shorter box rather than
        // being shrunk to it, as a flex parent would.
        let mut style = Style { display: Display::Block, ..Style::default() };
        style.size.width = relative(1.0).into();
        if let Extent::Moving(progress) = self.extent {
            let id = global_id.expect("a Reveal has an id");
            let Measured(height) = window.with_element_state(id, |state: Option<Measured>, _| {
                let state = state.unwrap_or_default();
                (state, state)
            });
            // Not yet measured: nothing shows until it is, the next frame.
            style.size.height = height.map_or(px(0.0), |height| height * progress).into();
        }
        (window.request_layout(style, [child], cx), child)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let id = global_id.expect("a Reveal has an id");
        let height = window.layout_bounds(*child).size.height;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.prepaint(window, cx);
        });
        let changed = window.with_element_state(id, |state: Option<Measured>, _| {
            (state.unwrap_or_default().0 != Some(height), Measured(Some(height)))
        });
        // A moving reveal laid out this frame at the old height.
        if changed && matches!(self.extent, Extent::Moving(_)) {
            window.request_animation_frame();
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.paint(window, cx);
        });
    }
}
