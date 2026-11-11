//! What gives the viewport depth: edges shaded toward the frame, a grid on
//! the waterline that fades away from the ship, and a ring sent out from a
//! hit as it lands while a battle is followed.

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
use gpui_wgpu::wgpu;
use wows_replays::types::GameClock;

use crate::viewport::renderer::LAYER_HULL;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;

/// How long a landing hit's ring takes to spread and fade.
pub(crate) const PULSE: Duration = Duration::from_millis(700);

/// Shades the edges of the scene at `bounds` toward the frame, so the eye
/// settles on the ship in the middle.
pub(crate) fn paint_vignette(bounds: Bounds<Pixels>, window: &mut Window) {
    /// How far in from each edge the shade reaches, as a share of the
    /// shorter side.
    const REACH: f32 = 0.16;
    /// The scene is drawn on a fixed dark ground whichever theme is chosen,
    /// so one strength serves both.
    const STRENGTH: f32 = 0.26;

    let reach = bounds.size.width.min(bounds.size.height) * REACH;
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

/// Lays the grid on the waterline under a ship with model-space `bounds`.
/// On the hull's layer, which is tested against the armor's depth without
/// writing its own, so the ship stands on the grid and hides it.
pub(crate) fn upload_waterline_grid(viewport: &mut Viewport3D, device: &wgpu::Device, bounds: (Vec3, Vec3)) {
    let (vertices, indices) = build_waterline_grid(bounds);
    if !indices.is_empty() {
        viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_HULL);
    }
}

/// A square grid on the waterline under a ship with model-space `bounds`,
/// fading from the middle of the ship to nothing at its edge. Drawn as thin
/// flat quads, each line cut into short pieces so the fade follows it.
pub(crate) fn build_waterline_grid(bounds: (Vec3, Vec3)) -> (Vec<Vertex>, Vec<u32>) {
    /// How far the grid reaches from the ship's middle, in ship lengths.
    const REACH: f32 = 0.9;
    /// About how many cells span the ship's length.
    const CELLS: f32 = 16.;
    /// How wide a line is drawn, as a share of a cell.
    const LINE: f32 = 0.03;
    /// The colour of a line at the ship's middle.
    const COLOR: [f32; 4] = [0.62, 0.68, 0.78, 0.3];

    let (low, high) = bounds;
    let middle = (low + high) * 0.5;
    let length = (high.x - low.x).max(high.z - low.z);
    if length <= 0. {
        return (Vec::new(), Vec::new());
    }

    let reach = length * REACH;
    let cell = round_step(length / CELLS);
    let half_line = cell * LINE * 0.5;
    let piece = cell * 0.5;
    let lines = (reach / cell).floor() as i32;
    let pieces = (reach * 2. / piece).ceil() as i32;
    let fade = |x: f32, z: f32| {
        let away = ((x - middle.x).powi(2) + (z - middle.z).powi(2)).sqrt() / reach;
        (1. - away * away).max(0.)
    };

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for line in -lines..=lines {
        let offset = line as f32 * cell;
        for step in 0..pieces {
            let from = -reach + step as f32 * piece;
            let to = (from + piece).min(reach);
            // Along x at a fixed z, then along z at a fixed x.
            let runs = [
                ([middle.x + from, middle.z + offset], [middle.x + to, middle.z + offset], [0., half_line]),
                ([middle.x + offset, middle.z + from], [middle.x + offset, middle.z + to], [half_line, 0.]),
            ];
            for (start, end, across) in runs {
                let (start_fade, end_fade) = (fade(start[0], start[1]), fade(end[0], end[1]));
                if start_fade <= 0. && end_fade <= 0. {
                    continue;
                }

                let base = vertices.len() as u32;
                let corners = [
                    (start[0] - across[0], start[1] - across[1], start_fade),
                    (start[0] + across[0], start[1] + across[1], start_fade),
                    (end[0] + across[0], end[1] + across[1], end_fade),
                    (end[0] - across[0], end[1] - across[1], end_fade),
                ];
                for (x, z, faded) in corners {
                    let color = [COLOR[0], COLOR[1], COLOR[2], COLOR[3] * faded];
                    vertices.push(Vertex { position: [x, 0., z], normal: [0., 1., 0.], color, uv: [0., 0.] });
                }
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }

    (vertices, indices)
}

/// `step` rounded to 1, 2 or 5 of its power of ten, so the cells measure a
/// whole number of metres a reader can count by.
fn round_step(step: f32) -> f32 {
    let power = 10_f32.powf(step.log10().floor());
    let leading = step / power;
    let rounded = if leading < 1.5 {
        1.
    } else if leading < 3.5 {
        2.
    } else if leading < 7.5 {
        5.
    } else {
        10.
    };

    rounded * power
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

    fn alpha_at(vertices: &[Vertex], x: f32, z: f32) -> Option<f32> {
        vertices
            .iter()
            .filter(|vertex| (vertex.position[0] - x).abs() < 0.5 && (vertex.position[2] - z).abs() < 0.5)
            .map(|vertex| vertex.color[3])
            .reduce(f32::max)
    }

    /// The grid lies on the waterline and is strongest under the ship's
    /// middle, fading to nothing toward its reach.
    #[test]
    fn the_grid_fades_away_from_the_ship() {
        let bounds = (Vec3::new(-100., -10., -15.), Vec3::new(100., 20., 15.));
        let (vertices, indices) = build_waterline_grid(bounds);

        assert!(!indices.is_empty());
        assert!(vertices.iter().all(|vertex| vertex.position[1] == 0.), "on the waterline");
        let middle = alpha_at(&vertices, 0., 0.).expect("a line crosses the middle");
        let near = alpha_at(&vertices, 100., 0.).expect("a line reaches the bow");
        assert!(middle > near && near > 0., "fades with distance: {middle} then {near}");
        assert!(vertices.iter().all(|vertex| vertex.color[3] <= middle));
    }

    #[test]
    fn cells_measure_round_numbers() {
        assert_eq!(round_step(12.5), 10.);
        assert_eq!(round_step(2.6), 2.);
        assert_eq!(round_step(0.4), 0.5);
    }

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
