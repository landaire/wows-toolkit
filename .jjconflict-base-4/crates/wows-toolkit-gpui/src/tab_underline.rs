//! The line under the selected app tab. One line serves the whole strip, so a
//! change of tab moves it from the tab left to the tab chosen.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::Bounds;
use gpui_kit::Corners;
use gpui_kit::IntoElement;
use gpui_kit::Pixels;
use gpui_kit::Styled as _;
use gpui_kit::canvas;
use gpui_kit::fill;
use gpui_kit::point;
use gpui_kit::px;
use gpui_kit::size;

/// Height of the line.
const THICKNESS: Pixels = px(2.);

/// Where the line is drawn along the strip, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Span {
    left: Pixels,
    width: Pixels,
}

impl Span {
    /// The line under a tab laid out at `tab`, kept clear of its ends by `inset`.
    fn under(tab: Bounds<Pixels>, inset: Pixels) -> Self {
        let inset = inset.min(tab.size.width / 4.);
        Self { left: tab.origin.x + inset, width: tab.size.width - inset * 2. }
    }

    fn lerp(self, to: Self, progress: f32) -> Self {
        Self {
            left: self.left + (to.left - self.left) * progress,
            width: self.width + (to.width - self.width) * progress,
        }
    }

    fn shifted(self, by: Pixels) -> Self {
        Self { left: self.left + by, ..self }
    }
}

/// A move of the line begun at `started` from `from`.
#[derive(Clone, Copy)]
struct Glide {
    from: Span,
    started: Instant,
}

/// What the line was last drawn under.
#[derive(Clone, Copy)]
struct Drawn<K> {
    tab: K,
    target: Span,
    span: Span,
    glide: Option<Glide>,
}

struct State<K> {
    drawn: Option<Drawn<K>>,
    /// Where the first tab starts this frame. Spans are kept from here, so
    /// they hold while the strip scrolls.
    origin: Pixels,
    /// Where the line is this frame, once the selected tab has placed it.
    current: Option<Span>,
}

impl<K> State<K>
where
    K: Copy + PartialEq,
{
    fn new() -> Self {
        Self { drawn: None, origin: Pixels::ZERO, current: None }
    }

    /// Where to draw the line under tab `selected` at `now`, given its place
    /// `target`. A changed tab starts a move from where the line is; the same
    /// tab laid out elsewhere, by a resize or another tab closing, takes the
    /// line with it at once.
    fn step(&mut self, selected: K, target: Span, now: Instant, progress: impl FnOnce(Instant) -> Option<f32>) -> Span {
        let glide = match self.drawn {
            Some(drawn) if drawn.tab != selected => Some(Glide { from: drawn.span, started: now }),
            Some(drawn) if drawn.target == target => drawn.glide,
            Some(_) | None => None,
        };

        let (span, glide) = match glide.and_then(|glide| progress(glide.started).map(|at| (glide, at))) {
            Some((glide, at)) => (glide.from.lerp(target, at), Some(glide)),
            None => (target, None),
        };
        self.drawn = Some(Drawn { tab: selected, target, span, glide });

        span
    }
}

/// The shared line, kept by the view that draws the strip and keyed by
/// what identifies a tab whatever its place in the strip.
#[derive(Clone)]
pub struct TabUnderline<K>(Rc<RefCell<State<K>>>);

impl<K> TabUnderline<K>
where
    K: Copy + PartialEq + 'static,
{
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(State::new())))
    }

    /// The part of the line that crosses tab `tab`, for every tab of the
    /// strip, each positioned so the element covers it. A tab clips what it
    /// holds, so each draws the whole line and shows its own part; drawn
    /// within the tabs, the line scrolls with them.
    pub fn part(&self, tab: K, first: bool, selected: bool) -> impl IntoElement {
        let placing = Rc::clone(&self.0);
        let state = Rc::clone(&self.0);
        canvas(
            move |bounds, window, cx| {
                let mut placing = placing.borrow_mut();
                if first {
                    placing.origin = bounds.origin.x;
                    placing.current = None;
                }
                if !selected {
                    return;
                }

                let origin = placing.origin;
                let target = Span::under(bounds, window.rem_size() * 0.5).shifted(-origin);
                let now = cx.background_executor().now();
                let span =
                    placing.step(tab, target, now, |started| crate::motion::rearrange_progress(started, window, cx));
                placing.current = Some(span);
            },
            move |bounds, (), window, _cx| {
                let (span, origin) = {
                    let state = state.borrow();
                    let Some(span) = state.current else { return };
                    (span, state.origin)
                };

                let span = span.shifted(origin);
                let line = Bounds::new(point(span.left, bounds.bottom() - THICKNESS), size(span.width, THICKNESS));
                if !line.intersects(&bounds) {
                    return;
                }
                window.paint_drop_shadows(line, Corners::default(), &[crate::theme::phosphor_glow()]);
                window.paint_quad(fill(line, crate::theme::accent()));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

impl<K> Default for TabUnderline<K>
where
    K: Copy + PartialEq + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn tab(left: f32, width: f32) -> Span {
        Span::under(Bounds::new(point(px(left), px(0.)), size(px(width), px(30.))), px(0.))
    }

    fn state() -> State<char> {
        State::new()
    }

    /// The first draw puts the line under the selected tab, a change of tab
    /// moves it from where it was, and the move ends under the tab chosen.
    #[test]
    fn the_line_moves_from_the_tab_left_to_the_tab_chosen() {
        let mut state = state();
        let start = Instant::now();
        let first = tab(0., 100.);
        let second = tab(200., 60.);

        assert_eq!(state.step('a', first, start, |_| Some(0.5)), first, "nothing to move from on the first draw");

        let halfway = state.step('b', second, start, |_| Some(0.5));
        assert_eq!(halfway, Span { left: px(100.), width: px(80.) });

        let later = start + Duration::from_millis(50);
        let started = std::cell::Cell::new(None);
        state.step('b', second, later, |at| {
            started.set(Some(at));
            Some(0.75)
        });
        assert_eq!(started.get(), Some(start), "the move keeps its start while the tab stays chosen");

        assert_eq!(state.step('b', second, later, |_| None), second, "a finished move rests under the tab");
    }

    /// A change of tab part way through a move starts the next one from where
    /// the line is, not from the tab it left.
    #[test]
    fn a_second_change_starts_from_where_the_line_is() {
        let mut state = state();
        let start = Instant::now();
        state.step('a', tab(0., 100.), start, |_| None);
        let halfway = state.step('b', tab(200., 100.), start, |_| Some(0.5));

        assert_eq!(state.step('c', tab(400., 100.), start, |_| Some(0.)), halfway);
    }

    /// The chosen tab laid out elsewhere, as when a tab before it closes,
    /// takes the line with it at once.
    #[test]
    fn the_same_tab_moved_takes_the_line_at_once() {
        let mut state = state();
        let start = Instant::now();
        state.step('a', tab(0., 100.), start, |_| None);

        let moved = tab(-40., 100.);
        assert_eq!(state.step('a', moved, start, |_| Some(0.)), moved);
    }

    /// The chosen tab laid out elsewhere part way through a move ends the
    /// move there, rather than steering it to the new place.
    #[test]
    fn a_move_ends_when_its_tab_is_laid_out_elsewhere() {
        let mut state = state();
        let start = Instant::now();
        state.step('a', tab(0., 100.), start, |_| None);
        state.step('b', tab(200., 100.), start, |_| Some(0.5));

        let moved = tab(160., 100.);
        assert_eq!(state.step('b', moved, start, |_| Some(0.6)), moved);
    }
}
