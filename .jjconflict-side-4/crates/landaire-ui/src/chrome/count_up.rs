use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::FrameHandle;
use zgui::view::IntoView;
use zgui::view::Timers;
use zgui::vocab::Timestamp;

use crate::motion::MotionPreference;
use crate::motion::motion_preference;
use crate::surface::TABULAR_CLASS;

/// A figure [`count_up`] can count through.
pub trait Countable: Copy + PartialOrd + 'static {
    /// The figure `share` of the way from `from` up to `to`, with `share` from 0 to 1 and `to`
    /// greater than `from`.
    fn part_way(from: Self, to: Self, share: f64) -> Self;
}

impl Countable for u64 {
    fn part_way(from: Self, to: Self, share: f64) -> Self {
        // Floored, so a figure is not shown before it is reached.
        let figure = from + ((to - from) as f64 * share).floor() as u64;

        figure.min(to)
    }
}

impl Countable for i64 {
    fn part_way(from: Self, to: Self, share: f64) -> Self {
        let span = (i128::from(to) - i128::from(from)) as f64;
        let figure = i128::from(from) + (span * share).floor() as i128;

        figure.min(i128::from(to)) as i64
    }
}

impl Countable for f64 {
    fn part_way(from: Self, to: Self, share: f64) -> Self {
        (from + (to - from) * share).min(to)
    }
}

/// How [`CountUp`] writes the figure it is at.
#[derive(Clone)]
pub struct CountFormat(Rc<dyn Fn(f64) -> String>);

impl CountFormat {
    pub fn new(format: impl Fn(f64) -> String + 'static) -> Self {
        Self(Rc::new(format))
    }

    /// A whole number, rounded down so a figure is not shown before the count reaches it.
    pub fn whole() -> Self {
        Self::new(|value| format!("{:.0}", value.floor()))
    }

    /// A number with `places` digits after the point.
    pub fn decimal(places: usize) -> Self {
        Self::new(move |value| format!("{value:.places$}"))
    }

    /// A share, where 1 is all of it, as a percentage with `places` digits after the point.
    pub fn percent(places: usize) -> Self {
        Self::new(move |value| format!("{:.places$}%", value * 100.0))
    }

    pub fn format(&self, value: f64) -> String {
        (self.0)(value)
    }
}

impl Default for CountFormat {
    fn default() -> Self {
        Self::whole()
    }
}

/// A number that counts up to each new value over a short time rather than jumping to it. A
/// smaller value is shown at once: it is a new count, not progress on the old one.
#[component]
pub fn CountUp(
    #[prop(into)] value: Signal<f64, LocalStorage>,
    /// Where the count starts when the number is first shown. `None` shows the value as it is.
    #[prop(optional)]
    from: Option<f64>,
    #[prop(optional)] format: CountFormat,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    let shown = match from {
        Some(start) => count_up_from(start, value),
        None => count_up(value),
    };

    view! {
        text(class = TABULAR_CLASS, {..attrs}, class = class) {{move || format.format(shown.get())}}
    }
}

/// `value` as [`CountUp`] shows it, for a caller that draws the count itself or derives
/// something else from it. Called from a component's body. A count under way when motion is
/// reduced ends at its figure.
pub fn count_up<T>(value: Signal<T, LocalStorage>) -> Signal<T, LocalStorage>
where
    T: Countable,
{
    count_up_from(value.get_untracked(), value)
}

/// [`count_up`] counting from `start` to the first value, as a figure counted up once when it is
/// first shown.
pub fn count_up_from<T>(start: T, value: Signal<T, LocalStorage>) -> Signal<T, LocalStorage>
where
    T: Countable,
{
    let preference = motion_preference();
    let timers = Timers::current();
    let shown = RwSignal::new_local(start);
    let frame: Rc<RefCell<Option<FrameHandle>>> = Rc::new(RefCell::new(None));

    let following = {
        let frame = Rc::clone(&frame);
        RenderEffect::new(move |_| {
            let to = value.get();
            let from = shown.get_untracked();
            frame.borrow_mut().take();

            let animate = to > from && preference.get() == MotionPreference::Full;
            match timers.as_ref() {
                Some(timers) if animate => {
                    count_frame(timers.clone(), Rc::clone(&frame), shown, Counting { from, to, started: None });
                }
                _ => shown.set(to),
            }
        })
    };
    on_cleanup_local(move || {
        drop(following);
        frame.borrow_mut().take();
    });

    shown.into()
}

/// One count in progress.
#[derive(Clone, Copy, Debug)]
struct Counting<T> {
    from: T,
    to: T,
    /// The frame the count began on; `None` until its first frame.
    started: Option<Timestamp>,
}

/// Shows the count's next figure on the next frame, and asks for the frame after until it ends.
fn count_frame<T>(
    timers: Timers,
    frame: Rc<RefCell<Option<FrameHandle>>>,
    shown: RwSignal<T, LocalStorage>,
    counting: Counting<T>,
) where
    T: Countable,
{
    const LENGTH: Duration = Duration::from_millis(400);

    let next = Rc::clone(&frame);
    let requesting = timers.clone();
    let handle = requesting.request_frame(move |now| {
        let started = counting.started.unwrap_or(now);
        let elapsed = now.saturating_since(started);
        if elapsed >= LENGTH {
            shown.set(counting.to);
            return;
        }

        shown.set(T::part_way(counting.from, counting.to, eased(elapsed.as_secs_f64() / LENGTH.as_secs_f64())));
        count_frame(timers, next, shown, Counting { started: Some(started), ..counting });
    });
    *frame.borrow_mut() = Some(handle);
}

/// `linear`, from 0 to 1, eased out: the count slows as it nears its figure.
fn eased(linear: f64) -> f64 {
    1.0 - (1.0 - linear).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_counts_are_floored_and_end_on_their_figure() {
        assert_eq!(u64::part_way(10, 20, 0.99), 19);
        assert_eq!(u64::part_way(10, 20, 1.0), 20);
        assert_eq!(i64::part_way(-10, 10, 0.5), 0);
        assert_eq!(i64::part_way(i64::MIN, i64::MAX, 1.0), i64::MAX);
        assert_eq!(f64::part_way(0.0, 2.5, 0.5), 1.25);
    }

    #[test]
    fn formats_write_whole_decimal_and_percentage_figures() {
        assert_eq!(CountFormat::whole().format(41.9), "41");
        assert_eq!(CountFormat::decimal(2).format(12.3456), "12.35");
        assert_eq!(CountFormat::decimal(1).format(7.0), "7.0");
        assert_eq!(CountFormat::percent(1).format(0.5432), "54.3%");
        assert_eq!(CountFormat::percent(0).format(1.0), "100%");
    }

    #[test]
    fn a_count_eases_out() {
        assert_eq!(eased(0.0), 0.0);
        assert!(eased(0.5) > 0.5, "more than half the way in half the time");
        assert_eq!(eased(1.0), 1.0);
    }
}
