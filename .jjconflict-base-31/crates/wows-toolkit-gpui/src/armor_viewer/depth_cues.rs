//! What gives the viewport depth: edges shaded toward the frame, and a ring
//! sent out from a hit as it lands while a battle is followed.

use std::time::Duration;
use std::time::Instant;

use gpui_kit::Bounds;
use gpui_kit::Hsla;
use gpui_kit::Pixels;
use gpui_kit::Point;
use gpui_kit::Window;
use gpui_kit::black;
use gpui_kit::fill;
use gpui_kit::linear_color_stop;
use gpui_kit::linear_gradient;
use gpui_kit::point;
use gpui_kit::px;
use gpui_kit::quad;
use gpui_kit::size;
use wows_replays::types::GameClock;
use wows_toolkit_armor::viewport::types::Vec3;

/// How long a landing hit's ring takes to spread and fade.
pub(crate) const PULSE: Duration = Duration::from_millis(700);

/// Shades the edges of the scene at `bounds`, a narrow inset shadow that
/// sets the scene just below the panels around it.
pub(crate) fn paint_vignette(bounds: Bounds<Pixels>, window: &mut Window) {
    /// How far in from each edge the shade reaches. Fixed and narrow: a
    /// wide shade reads as a shadow cast by the panels, far from them.
    const REACH: Pixels = px(10.);
    /// The scene is drawn on a fixed dark ground whichever theme is chosen,
    /// so one strength serves both.
    const STRENGTH: f32 = 0.3;

    let reach = REACH.min(bounds.size.width.min(bounds.size.height) / 2.);
    let shade = black().opacity(STRENGTH);
    let clear = black().opacity(0.);
    let (left, top) = (bounds.origin.x, bounds.origin.y);
    let (right, bottom) = (bounds.right(), bounds.bottom());

    // Each band darkens toward its own edge; the angle points from the
    // edge into the scene.
    let bands = [
        (Bounds::new(point(left, top), size(reach, bounds.size.height)), 90.),
        (Bounds::new(point(right - reach, top), size(reach, bounds.size.height)), 270.),
        (Bounds::new(point(left, top), size(bounds.size.width, reach)), 180.),
        (Bounds::new(point(left, bottom - reach), size(bounds.size.width, reach)), 0.),
    ];
    for (band, angle) in bands {
        window
            .paint_quad(fill(band, linear_gradient(angle, linear_color_stop(shade, 0.), linear_color_stop(clear, 1.))));
    }
}

/// A ring sent out from a hit, at `at` in model space, as it landed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HitPulse {
    pub at: Vec3,
    pub started: Instant,
}

/// The hits `now` adds to `before` while a battle plays forward: those after
/// the hits already shown, when the earlier list, by the time of each hit,
/// is where the new one starts. Empty for a seek back or a different set.
pub(crate) fn landed<'a, T>(before: &[T], now: &'a [T], clock: impl Fn(&T) -> GameClock) -> &'a [T] {
    let continues =
        now.len() > before.len() && before.iter().zip(now).all(|(earlier, later)| clock(earlier) == clock(later));
    if !continues {
        return &[];
    }

    &now[before.len()..]
}

/// One ring at `center`, `progress` of the way from a dot to its widest.
pub(crate) fn paint_pulse(center: Point<Pixels>, progress: f32, color: Hsla, window: &mut Window) {
    const FROM: f32 = 4.;
    const TO: f32 = 22.;

    let progress = progress.clamp(0., 1.);
    let radius = px(FROM + (TO - FROM) * progress);
    let bounds = Bounds::new(point(center.x - radius, center.y - radius), size(radius * 2., radius * 2.));
    let ring = color.opacity(0.9 * (1. - progress));
    window.paint_quad(quad(bounds, radius, black().opacity(0.), px(1.5), ring, Default::default()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only hits after those already shown are new; a seek back, or a list
    /// that does not start with the shown hits, lands nothing.
    #[test]
    fn only_hits_after_the_shown_ones_land() {
        let clocks = |seconds: &[f32]| seconds.iter().copied().map(GameClock).collect::<Vec<_>>();
        let same = |clock: &GameClock| *clock;
        let shown = clocks(&[10., 20.]);

        assert_eq!(landed(&shown, &clocks(&[10., 20., 31., 32.]), same), clocks(&[31., 32.]).as_slice());
        assert_eq!(landed(&[], &clocks(&[5.]), same), clocks(&[5.]).as_slice());
        assert!(landed(&shown, &clocks(&[10.]), same).is_empty(), "a seek back");
        assert!(landed(&shown, &shown, same).is_empty(), "nothing new");
        assert!(landed(&shown, &clocks(&[11., 20., 30.]), same).is_empty(), "another set of hits");
    }
}
