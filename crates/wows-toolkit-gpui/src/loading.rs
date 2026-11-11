//! What a view draws in place of content it is still waiting for.
//!
//! The spinner, the bar sweep and the skeleton pulse hold still when the
//! system asks for reduced motion.

use std::time::Duration;

use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::Size;
use gpui_kit::component::h_flex;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// How long a wait may last, which sets how often its spinner or bar redraws.
#[derive(Clone, Copy)]
enum Pace {
    /// Work that finishes on its own: the spinner and bar move every frame.
    Bounded,
    /// A wait with no known end, such as for another program: every frame
    /// re-renders the owning view, so the motion runs at a capped rate.
    Unbounded,
}

impl Pace {
    /// Low enough that an open-ended wait costs little, high enough that the
    /// motion still reads as moving.
    const UNBOUNDED_FPS: f32 = 10.;

    fn spinner(self, size: Size) -> AnyElement {
        const UNBOUNDED_TURN: Duration = Duration::from_millis(1200);

        match self {
            Pace::Bounded => Spinner::new().with_size(size).into_any_element(),
            // The kit's spinner icon is symmetric under an eighth turn, so at
            // a low frame rate it would appear to stand still; an open arc
            // does not.
            Pace::Unbounded => div()
                .child(Icon::new(IconName::LoaderCircle).with_size(size).with_animation(
                    "loading-spinner",
                    Animation::new(UNBOUNDED_TURN).repeat().with_max_fps(Self::UNBOUNDED_FPS),
                    |icon, turn| icon.transform(Transformation::rotate(percentage(turn))),
                ))
                .into_any_element(),
        }
    }

    /// A bar with a segment sweeping across its track, for work whose share
    /// done is not known. The kit's indeterminate bar cannot cap its rate.
    fn sweep(self, cx: &App) -> AnyElement {
        const SWEEP: Duration = Duration::from_millis(1400);
        const SEGMENT: f32 = 0.35;

        let color = cx.theme().tokens.progress_bar;
        let radius = cx.theme().radius.min(BAR_HEIGHT / 2.);
        let segment = div().absolute().top_0().h_full().w(relative(SEGMENT)).rounded(radius).bg(color);
        let segment = if cx.reduce_motion() {
            segment.left(relative((1. - SEGMENT) / 2.)).into_any_element()
        } else {
            let animation = Animation::new(SWEEP).repeat().with_easing(ease_in_out);
            let animation = match self {
                Pace::Bounded => animation,
                Pace::Unbounded => animation.with_max_fps(Self::UNBOUNDED_FPS),
            };
            // Enters past the left edge and leaves past the right one.
            segment
                .with_animation("loading-sweep", animation, |segment, delta| {
                    segment.left(relative(-SEGMENT + delta * (1. + SEGMENT)))
                })
                .into_any_element()
        };

        gpui_kit::base::Progress::new("loading-progress")
            .indeterminate(true)
            .relative()
            .w_full()
            .h(BAR_HEIGHT)
            .rounded(radius)
            .overflow_hidden()
            .bg(color.opacity(0.2))
            .child(segment)
            .into_any_element()
    }
}

/// Height of the bar in a centred wait, the kit's default bar height.
const BAR_HEIGHT: Pixels = px(8.);

/// The box centred in a content area that holds the label of a wait above
/// its bar.
fn card(label: SharedString, bar: AnyElement, cx: &App) -> Div {
    const CARD_WIDTH: Pixels = px(280.);

    let theme = cx.theme();
    v_flex()
        .w(CARD_WIDTH)
        .max_w_full()
        .gap_3()
        .px_6()
        .py_5()
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius)
        .shadow_md()
        .child(div().text_sm().text_center().child(label))
        .child(bar)
}

/// How much of a counted piece of work is finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub done: usize,
    pub total: usize,
}

impl Counts {
    /// The finished share as a percentage, or `None` while there is no total
    /// to measure it against.
    fn percent(self) -> Option<f32> {
        (self.total > 0).then(|| self.done.min(self.total) as f32 / self.total as f32 * 100.)
    }
}

/// Where a wait sits in the space its content would fill.
#[derive(Clone, Copy)]
enum Placement {
    /// Centred in an open content area.
    Centered,
    /// At the head of a narrow list, where its first rows would be.
    Top,
}

/// A label standing in for content that is not ready, with a bar showing how
/// much of the work is finished, or sweeping while that is not known.
#[derive(IntoElement)]
pub struct Loading {
    id: ElementId,
    label: SharedString,
    counts: Option<Counts>,
    placement: Placement,
    pace: Pace,
}

impl Loading {
    /// A wait centred in the area it fills. The label is also the element's
    /// accessible name.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into(), counts: None, placement: Placement::Centered, pace: Pace::Bounded }
    }

    /// Places the wait at the head of the area, for a list whose rows start there.
    pub fn top(mut self) -> Self {
        self.placement = Placement::Top;
        self
    }

    /// Adds a bar showing how much of the work is finished.
    pub fn counts(mut self, counts: Counts) -> Self {
        self.counts = Some(counts);
        self
    }

    /// Marks a wait with no known end, whose spinner turns at a capped rate.
    pub fn unbounded(mut self) -> Self {
        self.pace = Pace::Unbounded;
        self
    }
}

impl RenderOnce for Loading {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let percent = self.counts.and_then(Counts::percent);
        let root = v_flex().id(self.id).test_support().aria_label(self.label.clone());

        match self.placement {
            Placement::Centered => {
                // The bar takes no label of its own: the root already names the wait.
                let bar = match percent {
                    Some(percent) => Progress::new("loading-progress").value(percent).into_any_element(),
                    None => self.pace.sweep(cx),
                };
                root.size_full().items_center().justify_center().p_4().child(card(self.label, bar, cx))
            }
            Placement::Top => root
                .w_full()
                .p_2()
                .gap_2()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(self.pace.spinner(Size::Medium))
                        .child(div().min_w_0().child(self.label)),
                )
                .when_some(percent, |this, percent| this.child(Progress::new("loading-progress").value(percent))),
        }
    }
}

/// A small spinner beside a label, for a wait reported within a line of
/// other content rather than in place of it.
#[derive(IntoElement)]
pub struct LoadingLine {
    label: SharedString,
    pace: Pace,
}

impl LoadingLine {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into(), pace: Pace::Bounded }
    }

    /// Marks a wait with no known end, whose spinner turns at a capped rate.
    pub fn unbounded(mut self) -> Self {
        self.pace = Pace::Unbounded;
        self
    }
}

impl RenderOnce for LoadingLine {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        h_flex()
            .gap_1()
            .items_center()
            .min_w_0()
            .text_xs()
            .text_color(crate::theme::text_dim())
            .child(self.pace.spinner(Size::XSmall))
            .child(div().min_w_0().child(self.label))
    }
}

/// The outline of a header line over a table of players, pulsing where the
/// table will be drawn once it is read, under a centred card with the label
/// of what it waits for.
#[derive(IntoElement)]
pub struct TableSkeleton {
    id: ElementId,
    label: SharedString,
}

impl TableSkeleton {
    /// The label is also the element's accessible name.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: label.into() }
    }
}

impl RenderOnce for TableSkeleton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Two full teams; the area clips whatever does not fit.
        const ROWS: usize = 24;
        const ROW_HEIGHT: Pixels = px(26.);
        const NAME_COLUMN_WIDTH: Pixels = px(200.);
        const FIGURE_WIDTHS: [f32; 7] = [44., 56., 40., 64., 48., 40., 56.];
        const TEXT_HEIGHT: Pixels = px(10.);
        const PULSE: Duration = Duration::from_secs(2);
        const PULSE_DEPTH: f32 = 0.4;

        let radius = cx.theme().radius;
        // The border tone, since the widget tone barely differs from the panel.
        let tone = cx.theme().border;
        let block = move |width: Pixels, height: Pixels| div().flex_none().w(width).h(height).rounded(radius).bg(tone);
        let faint = move |width: Pixels, height: Pixels| block(width, height).opacity(0.6);

        let header = h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child(block(px(56.), px(18.)))
            .child(block(px(40.), px(18.)))
            .child(block(px(160.), TEXT_HEIGHT))
            .child(div().flex_1())
            .child(block(px(72.), px(22.)))
            .child(block(px(22.), px(22.)));
        let context = h_flex().flex_none().px_2().pb_1().child(faint(px(360.), TEXT_HEIGHT));

        let columns = h_flex()
            .flex_none()
            .h(ROW_HEIGHT)
            .items_center()
            .gap_3()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().flex_none().w(NAME_COLUMN_WIDTH).child(faint(px(64.), TEXT_HEIGHT)))
            .children(FIGURE_WIDTHS.map(|width| faint(px(width), TEXT_HEIGHT)));
        let rows = (0..ROWS).map(|index| {
            // Names differ in length; equal bars would read as a drawing
            // rather than as rows about to fill.
            let name_width = px(96. + (index * 37 % 72) as f32);
            h_flex()
                .flex_none()
                .h(ROW_HEIGHT)
                .items_center()
                .gap_3()
                .px_2()
                .child(
                    h_flex()
                        .flex_none()
                        .w(NAME_COLUMN_WIDTH)
                        .gap_2()
                        .items_center()
                        .child(block(px(16.), px(16.)))
                        .child(block(name_width, TEXT_HEIGHT)),
                )
                .children(FIGURE_WIDTHS.map(|width| block(px(width), TEXT_HEIGHT)))
        });

        // One pulse for the whole outline: an animation per block would each
        // ask for its own frame.
        let outline = v_flex().size_full().child(header).child(context).child(columns).children(rows).with_animation(
            "table-skeleton-pulse",
            Animation::new(PULSE).repeat().with_easing(bounce(ease_in_out)),
            |outline, delta| outline.opacity(1. - delta * PULSE_DEPTH),
        );

        let overlay = div().absolute().inset_0().flex().items_center().justify_center().p_4().child(card(
            self.label.clone(),
            Pace::Bounded.sweep(cx),
            cx,
        ));

        div()
            .id(self.id)
            .test_support()
            .aria_label(self.label)
            .relative()
            .size_full()
            .overflow_hidden()
            .child(outline)
            .child(overlay)
    }
}

#[cfg(test)]
mod tests {
    use super::Counts;
    use super::Loading;
    use gpui_kit::AnyWindowHandle;
    use gpui_kit::AppContext;
    use gpui_kit::Context;
    use gpui_kit::IntoElement;
    use gpui_kit::Render;
    use gpui_kit::TestAppContext;
    use gpui_kit::Window;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt;

    struct Host(fn() -> Loading);

    impl Render for Host {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            (self.0)()
        }
    }

    /// Opens a window over `loading` and draws its first frame.
    fn open(cx: &mut TestAppContext, loading: fn() -> Loading) -> AnyWindowHandle {
        cx.update(gpui_kit::init);
        cx.update(|cx| cx.set_reduce_motion(false));
        let window = cx.open_window(size(px(400.), px(300.)), move |_, _| Host(loading)).into();
        cx.update_window(window, |_, window, cx| window.render_frame(cx)).expect("the window is open");
        window
    }

    #[gpui_kit::test]
    fn only_a_wait_with_an_end_redraws_every_frame(cx: &mut TestAppContext) {
        let bounded = open(cx, || Loading::new("wait", "Reading"));
        cx.update_window(bounded, |_, window, cx| {
            assert!(window.simulate_next_frame(cx) > 0, "a bounded wait sweeps every frame");
        })
        .expect("the window is open");

        let unbounded = open(cx, || Loading::new("wait", "Waiting").unbounded());
        cx.update_window(unbounded, |_, window, cx| {
            assert_eq!(window.simulate_next_frame(cx), 0, "an open-ended wait sweeps on a timer instead");
        })
        .expect("the window is open");
    }

    #[gpui_kit::test]
    fn a_wait_holds_still_under_reduced_motion(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| cx.set_reduce_motion(true));
        let window: AnyWindowHandle =
            cx.open_window(size(px(400.), px(300.)), |_, _| Host(|| Loading::new("wait", "Reading"))).into();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.simulate_next_frame(cx), 0);
        })
        .expect("the window is open");
    }

    #[gpui_kit::test]
    fn a_counted_wait_is_named_once(cx: &mut TestAppContext) {
        let window = open(cx, || Loading::new("wait", "Reading").counts(Counts { done: 1, total: 4 }));
        cx.update_window(window, |_, window, _cx| {
            let named = gpui_kit::base::test_support::snapshots(window)
                .into_iter()
                .filter(|element| element.label() == Some("Reading"))
                .count();
            assert_eq!(named, 1);
        })
        .expect("the window is open");
    }

    #[test]
    fn a_bar_needs_a_total_and_never_passes_full() {
        assert_eq!(Counts { done: 0, total: 0 }.percent(), None);
        assert_eq!(Counts { done: 1, total: 4 }.percent(), Some(25.));
        assert_eq!(Counts { done: 9, total: 4 }.percent(), Some(100.));
    }
}
