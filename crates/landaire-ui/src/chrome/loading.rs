//! What a view draws in place of content it is still waiting for.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntervalHandle;
use zgui::view::IntoView;
use zgui::view::Timers;
use zgui::view::install_stylesheet;
use zgui::vocab::Role;
use zgui_ui::ProgressProps;
use zgui_ui::SkeletonProps;
use zgui_ui::SpinnerProps;

use crate::chrome::status_bar::Fraction;
use crate::chrome::status_bar::RunningBarProps;
use crate::motion::MotionPreference;
use crate::motion::motion_preference;

const SHEET: &str = "landaire-ui-loading";

const CSS: &str = css!(
    ".lui-loading {
        display: flex;
        flex-direction: column;
        min-width: 0;
        font-family: var(--lui-font-family);
    }
    .lui-loading[data-placement=\"centered\"] {
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        align-items: center;
        justify-content: center;
        padding: 16px;
    }
    .lui-loading[data-placement=\"top\"] {
        width: 100%;
        gap: 8px;
        padding: 8px;
        font-size: 13px;
        color: var(--lui-color-text-dim);
    }
    .lui-loading__card {
        display: flex;
        flex-direction: column;
        gap: 12px;
        width: 280px;
        max-width: 100%;
        padding: 20px 24px;
        border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-card);
        box-shadow: var(--lui-shadow-overlay);
        color: var(--lui-color-text);
    }
    .lui-loading__label {
        font-size: 13px;
        text-align: center;
    }
    .lui-loading__line {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .lui-loading .lui-running-bar, .lui-loading .zui-progress {
        width: 100%;
        height: 8px;
    }
    .lui-loading-line {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 4px;
        min-width: 0;
        font-size: 12px;
        color: var(--lui-color-text-dim);
    }
    .lui-loading-line .zui-spinner {
        width: 12px;
        height: 12px;
        flex: none;
    }
    .lui-loading__line .zui-spinner {
        width: 16px;
        height: 16px;
        flex: none;
    }
    .lui-table-skeleton {
        position: relative;
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-height: 0;
        overflow: hidden;
    }
    .lui-table-skeleton__header, .lui-table-skeleton__row {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 12px;
        padding: 0 8px;
    }
    .lui-table-skeleton__header {
        gap: 8px;
        padding: 4px 8px;
    }
    .lui-table-skeleton__context {
        display: flex;
        flex: none;
        padding: 0 8px 4px 8px;
    }
    .lui-table-skeleton__row { height: 26px; }
    .lui-table-skeleton__columns { border-bottom: 1px solid var(--lui-color-border); }
    .lui-table-skeleton__name {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 8px;
        width: 200px;
    }
    .lui-table-skeleton__grow { flex: 1 1 auto; }
    .lui-table-skeleton .zui-skeleton {
        flex: none;
        height: 10px;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-border);
    }
    .lui-table-skeleton__faint { opacity: 0.6; }
    .lui-table-skeleton__overlay {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 16px;
    }
    .lui-loading .lui-spinner-paced, .lui-loading-line .lui-spinner-paced {
        animation: none;
    }
    .lui-paced-sweep {
        position: relative;
        width: 100%;
        height: 8px;
        overflow: hidden;
        border-radius: var(--lui-radius);
        background-color: color-mix(in srgb, var(--lui-color-accent) 20%, transparent);
    }
    .lui-paced-sweep__segment {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 35%;
        border-radius: var(--lui-radius);
        background-color: var(--lui-color-accent);
    }"
);

/// How long a wait may last, which sets how often its indicator moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadingPace {
    /// Work that finishes on its own: the indicator moves every frame.
    #[default]
    Bounded,
    /// A wait with no known end, such as for another program: the indicator moves ten times a
    /// second, so a wait of minutes costs little.
    Unbounded,
}

/// Where a wait sits in the space its content would fill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadingPlacement {
    /// Centred in an open content area, as a card.
    #[default]
    Centered,
    /// At the head of a narrow list, where its first rows would be.
    Top,
}

/// A label standing in for content that is not ready, with a bar showing how much of the work is
/// finished when `done` is given, or sweeping while that is not known. The label is also the
/// accessible name.
#[component]
pub fn Loading(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(optional)] placement: LoadingPlacement,
    #[prop(into, optional)] done: Option<Signal<Fraction, LocalStorage>>,
    #[prop(optional)] pace: LoadingPace,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let own = status(label);
    // The root names the wait, so the indicators inside it say nothing of their own.
    let bar = move || match (done, pace) {
        (Some(done), _) => AnyView::new(view! { RunningBar(done = done, label = label.get(), a11y:hidden = true) }),
        (None, LoadingPace::Bounded) => AnyView::new(view! { Progress(a11y:hidden = true) }),
        (None, LoadingPace::Unbounded) => AnyView::new(view! { PacedSweep() }),
    };

    match placement {
        LoadingPlacement::Centered => AnyView::new(view! {
            box(class = "lui-loading", attr:data-placement = "centered", {..own}, {..attrs}, class = class) {
                column(class = "lui-loading__card") {
                    text(class = "lui-loading__label") {{move || label.get()}}
                    {bar()}
                }
            }
        }),
        LoadingPlacement::Top => AnyView::new(view! {
            box(class = "lui-loading", attr:data-placement = "top", {..own}, {..attrs}, class = class) {
                row(class = "lui-loading__line") {
                    {spinner(pace)}
                    text {{move || label.get()}}
                }
                {done.map(|done| AnyView::new(view! { RunningBar(done = done, label = label.get(), a11y:hidden = true) }))}
            }
        }),
    }
}

/// A small spinner beside a label, for a wait reported within a line of other content rather than
/// in place of it.
#[component]
pub fn LoadingLine(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(optional)] pace: LoadingPace,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let own = status(label);

    view! {
        row(class = "lui-loading-line", {..own}, {..attrs}, class = class) {
            {spinner(pace)}
            text {{move || label.get()}}
        }
    }
}

/// The spinner a wait at `pace` turns. Hidden: the wait's root names it.
fn spinner(pace: LoadingPace) -> AnyView {
    match pace {
        LoadingPace::Bounded => AnyView::new(view! { Spinner(a11y:hidden = true) }),
        LoadingPace::Unbounded => AnyView::new(view! { PacedSpinner() }),
    }
}

/// A spinner turned a twelfth at a time, ten times a second.
#[component]
fn PacedSpinner() -> impl IntoView {
    let steps = paced_steps();
    let turn = move || Some(format!("rotate({}deg)", (steps.get() % 12) * 30));

    view! { Spinner(a11y:hidden = true, class = "lui-spinner-paced", style:transform = turn) }
}

/// A segment sweeping across its track, for work whose share done is not known, moved ten times a
/// second.
#[component]
fn PacedSweep() -> impl IntoView {
    // Steps to cross: 1.4 s, the length of the sweep a bounded wait shows.
    const CROSSING: u32 = 14;
    const SEGMENT: f32 = 35.0;

    install_stylesheet(SHEET, CSS);

    let steps = paced_steps();
    let preference = motion_preference();
    // Enters past the left edge and leaves past the right one. Held in the middle without motion,
    // where step zero would leave it out of sight.
    let left = move || {
        if preference.get() != MotionPreference::Full {
            return Some(format!("{}%", (100.0 - SEGMENT) / 2.0));
        }
        let travel = (steps.get() % CROSSING) as f32 / CROSSING as f32;
        Some(format!("{}%", -SEGMENT + travel * (100.0 + SEGMENT)))
    };

    view! {
        box(class = "lui-paced-sweep", a11y:hidden = true) {
            box(class = "lui-paced-sweep__segment", style:left = left)
        }
    }
}

/// A count that goes up ten times a second while the reader allows motion, and holds otherwise.
fn paced_steps() -> RwSignal<u32, LocalStorage> {
    const STEP: Duration = Duration::from_millis(100);

    let steps = RwSignal::new_local(0_u32);
    let preference = motion_preference();
    let timers = Timers::current();
    let held: Rc<RefCell<Option<IntervalHandle>>> = Rc::new(RefCell::new(None));
    let ticking = {
        let held = Rc::clone(&held);
        RenderEffect::new(move |_| {
            if preference.get() != MotionPreference::Full {
                held.borrow_mut().take();
                return;
            }
            if held.borrow().is_some() {
                return;
            }

            *held.borrow_mut() = timers
                .as_ref()
                .map(|timers| timers.set_interval(STEP, move || steps.update(|steps| *steps = steps.wrapping_add(1))));
        })
    };
    on_cleanup_local(move || {
        drop(ticking);
        held.borrow_mut().take();
    });

    steps
}

/// A busy status named `label`, for the root of a wait.
fn status(label: Signal<String, LocalStorage>) -> Attrs {
    Attrs::new().a11y_from(A11yBinding::new(Role::Status).label(move || label.get()).busy(true))
}

/// The outline of a header over a table of players, pulsing where the table will be drawn once it
/// is read, under a centred card naming what it waits for.
#[component]
pub fn TableSkeleton(
    #[prop(into)] label: Signal<String, LocalStorage>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    // Two full teams; the area clips whatever does not fit.
    const ROWS: usize = 24;
    const FIGURE_WIDTHS: [u32; 7] = [44, 56, 40, 64, 48, 40, 56];

    install_stylesheet(SHEET, CSS);

    let block = |width: u32, height: u32| {
        AnyView::new(view! { Skeleton(style:width = format!("{width}px"), style:height = format!("{height}px")) })
    };
    let figures = move || FIGURE_WIDTHS.map(|width| block(width, 10)).into_iter().collect::<Vec<_>>();
    // Names differ in length; equal bars would read as a drawing rather than as rows about to fill.
    let rows = (0..ROWS)
        .map(|index| {
            let name_width = 96 + (index * 37 % 72) as u32;
            AnyView::new(view! {
                row(class = "lui-table-skeleton__row") {
                    row(class = "lui-table-skeleton__name") {
                        {block(16, 16)}
                        {block(name_width, 10)}
                    }
                    {figures()}
                }
            })
        })
        .collect::<Vec<_>>();

    view! {
        box(class = "lui-table-skeleton", {..attrs}, class = class) {
            row(class = "lui-table-skeleton__header") {
                {block(56, 18)}
                {block(40, 18)}
                {block(160, 10)}
                box(class = "lui-table-skeleton__grow") {}
                {block(72, 22)}
                {block(22, 22)}
            }
            row(class = "lui-table-skeleton__context lui-table-skeleton__faint") {
                {block(360, 10)}
            }
            row(class = "lui-table-skeleton__row lui-table-skeleton__columns lui-table-skeleton__faint") {
                row(class = "lui-table-skeleton__name") {
                    {block(64, 10)}
                }
                {figures()}
            }
            {rows}
            box(class = "lui-table-skeleton__overlay") {
                Loading(label = label)
            }
        }
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::root::ThemeRootProps;
    use crate::test_support::HeadlessOptions;
    use crate::test_support::HeadlessWindow;
    use crate::test_support::WindowSize;

    fn open(pace: LoadingPace) -> HeadlessWindow {
        let mut window =
            HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 640.0, height: 400.0 }), move || {
                view! {
                    ThemeRoot {
                        LoadingLine(label = "Waiting for the game".to_owned(), pace = pace)
                        Loading(label = "Starting".to_owned(), pace = pace)
                    }
                }
            })
            .expect("opens");
        window.advance(Duration::from_millis(300));

        window
    }

    /// An open-ended wait moves on a timer rather than an animation the window draws every frame
    /// for.
    #[test]
    fn an_unbounded_wait_runs_no_per_frame_animation() {
        let window = open(LoadingPace::Unbounded);
        let spinner = window.by_class("zui-spinner").expect("the line has a spinner");
        assert_eq!(window.running_animations(spinner), 0);
        assert!(window.by_class("lui-paced-sweep").is_some(), "the card sweeps at the capped pace");
        assert!(window.by_class("zui-progress").is_none());

        let window = open(LoadingPace::Bounded);
        let spinner = window.by_class("zui-spinner").expect("the line has a spinner");
        assert!(window.running_animations(spinner) > 0, "a bounded wait turns every frame");
    }
}
