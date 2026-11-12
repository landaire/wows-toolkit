//! What gives the viewport depth: edges shaded toward the frame, a grid on
//! the waterline that fades away from the ship with heavier lines at its
//! length marks, and a ring sent out from a hit as it lands while a battle
//! is followed.

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

use super::length_marks::LengthMarks;

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

/// Lays the grid under a model whose marks are `marks`. In world space, so
/// it stays level while the hull heels over. On the hull's layer, which is
/// tested against the armor's depth without writing its own, so the model
/// stands on the grid and hides it.
pub(crate) fn upload_waterline_grid(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    marks: &LengthMarks,
    show_marks: bool,
) {
    let (vertices, indices) = build_waterline_grid(marks, show_marks);
    if !indices.is_empty() {
        viewport.add_world_space_mesh(device, &vertices, &indices, LAYER_HULL);
    }
}

/// How far the grid reaches from the model's middle, in model lengths.
const GRID_REACH: f32 = 0.9;

/// How strong the grid is at `position`, from 1 under the model's middle to
/// 0 at the grid's edge.
pub(crate) fn grid_fade(marks: &LengthMarks, position: Vec3) -> f32 {
    let (low, high) = marks.bounds();
    let middle = (low + high) * 0.5;
    let reach = (high.x - low.x).max(high.z - low.z) * GRID_REACH;
    let away = ((position.x - middle.x).powi(2) + (position.z - middle.z).powi(2)).sqrt() / reach;

    (1. - away * away).max(0.)
}

/// A square grid on the plane of `marks` under a model, fading from its
/// middle to nothing at its edge. Drawn as thin flat quads, each line cut
/// into short pieces so the fade follows it. The lines fall on the marks'
/// spacing, or halfway between when there are few marks, counted from where
/// the marks are; with `show_marks` the lines on a mark within the model are
/// drawn heavier.
pub(crate) fn build_waterline_grid(marks: &LengthMarks, show_marks: bool) -> (Vec<Vertex>, Vec<u32>) {
    /// How wide a line is drawn, as a share of a cell.
    const LINE: f32 = 0.03;
    /// How wide a mark is drawn, as a share of the mark spacing.
    const MARK_LINE: f32 = 0.04;
    /// The colour of a line at the model's middle.
    const COLOR: [f32; 4] = [0.62, 0.68, 0.78, 0.3];
    /// The colour of a mark at the model's middle.
    const MARK_COLOR: [f32; 4] = [0.75, 0.82, 0.92, 0.6];
    /// Fewer marks than this along the longest side get a line between each.
    const SPLIT_BELOW: f32 = 10.;

    let (low, high) = marks.bounds();
    let middle = (low + high) * 0.5;
    let longest = (high.x - low.x).max(high.z - low.z);
    let spacing = marks.spacing().to_ship_model().value();
    let lines_per_mark = if longest / spacing < SPLIT_BELOW { 2 } else { 1 };
    let cell = spacing / lines_per_mark as f32;
    let reach = longest * GRID_REACH;
    let piece = cell * 0.5;
    let pieces = (reach * 2. / piece).ceil() as i32;
    let plane_y = marks.plane_y();
    let fade = |x: f32, z: f32| grid_fade(marks, Vec3::new(x, plane_y, z));

    // Lines across X count from the -X side, lines across Z from the +Z end,
    // as the marks do. `index` 0 is the line through that edge.
    let first = |from: f32| ((from - reach) / cell).ceil() as i32;
    let last = |from: f32| ((from + reach) / cell).floor() as i32;
    let is_mark = |index: i32, steps: u32| {
        show_marks && index >= 0 && index % lines_per_mark == 0 && (index / lines_per_mark) as u32 <= steps
    };
    let x_lines = (first(middle.x - low.x)..=last(middle.x - low.x))
        .map(|index| (low.x + index as f32 * cell, is_mark(index, marks.beam_steps())));
    let z_lines = (first(high.z - middle.z)..=last(high.z - middle.z))
        .map(|index| (high.z - index as f32 * cell, is_mark(index, marks.length_steps())));

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut push = |start: [f32; 2], end: [f32; 2], across: [f32; 2], color: [f32; 4]| {
        let (start_fade, end_fade) = (fade(start[0], start[1]), fade(end[0], end[1]));
        if start_fade <= 0. && end_fade <= 0. {
            return;
        }

        let base = vertices.len() as u32;
        let corners = [
            (start[0] - across[0], start[1] - across[1], start_fade),
            (start[0] + across[0], start[1] + across[1], start_fade),
            (end[0] + across[0], end[1] + across[1], end_fade),
            (end[0] - across[0], end[1] - across[1], end_fade),
        ];
        for (x, z, faded) in corners {
            let color = [color[0], color[1], color[2], color[3] * faded];
            vertices.push(Vertex { position: [x, plane_y, z], normal: [0., 1., 0.], color, uv: [0., 0.] });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let style = |mark: bool| if mark { (spacing * MARK_LINE * 0.5, MARK_COLOR) } else { (cell * LINE * 0.5, COLOR) };

    for (x, mark) in x_lines {
        let (half_line, color) = style(mark);
        for step in 0..pieces {
            let from = middle.z - reach + step as f32 * piece;
            let to = (from + piece).min(middle.z + reach);
            push([x, from], [x, to], [half_line, 0.], color);
        }
    }
    for (z, mark) in z_lines {
        let (half_line, color) = style(mark);
        for step in 0..pieces {
            let from = middle.x - reach + step as f32 * piece;
            let to = (from + piece).min(middle.x + reach);
            push([from, z], [to, z], [0., half_line], color);
        }
    }

    (vertices, indices)
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
    use crate::armor_viewer::length_marks::Mark;

    /// The strongest vertex within a cell of `x`, `z`.
    fn alpha_at(vertices: &[Vertex], x: f32, z: f32) -> Option<f32> {
        const NEAR: f32 = 15.;

        vertices
            .iter()
            .filter(|vertex| (vertex.position[0] - x).abs() < NEAR && (vertex.position[2] - z).abs() < NEAR)
            .map(|vertex| vertex.color[3])
            .reduce(f32::max)
    }

    fn ship_marks() -> LengthMarks {
        LengthMarks::new((Vec3::new(-15., -10., -100.), Vec3::new(15., 20., 100.)), 0.).expect("marks")
    }

    /// Vertices drawn in a mark's colour rather than a plain line's.
    fn mark_vertices(vertices: &[Vertex]) -> Vec<&Vertex> {
        let plain = |vertex: &&Vertex| vertex.color[..3] == [0.62, 0.68, 0.78];
        vertices.iter().filter(|vertex| !plain(vertex)).collect()
    }

    /// The grid lies on the waterline and is strongest under the ship's
    /// middle, fading to nothing toward its reach.
    #[test]
    fn the_grid_fades_away_from_the_ship() {
        let (vertices, indices) = build_waterline_grid(&ship_marks(), false);

        assert!(!indices.is_empty());
        assert!(vertices.iter().all(|vertex| vertex.position[1] == 0.), "on the waterline");
        let middle = alpha_at(&vertices, 0., 0.).expect("a line crosses the middle");
        let near = alpha_at(&vertices, 0., -100.).expect("a line reaches the bow");
        assert!(middle > near && near > 0., "fades with distance: {middle} then {near}");
        assert!(vertices.iter().all(|vertex| vertex.color[3] <= middle));
    }

    /// With the marks shown, the lines on them are drawn in their own colour,
    /// one along each mark within the ship and nowhere else; with them
    /// hidden the grid is plain.
    #[test]
    fn the_marks_are_drawn_on_their_lines_only_while_shown() {
        let marks = ship_marks();
        let (shown, _) = build_waterline_grid(&marks, true);
        let (hidden, _) = build_waterline_grid(&marks, false);

        assert!(mark_vertices(&hidden).is_empty());
        let drawn = mark_vertices(&shown);
        let half_width = marks.spacing().to_ship_model().value() * 0.04;
        let on = |marks: &[Mark], at: f32| marks.iter().any(|mark| (mark.at - at).abs() <= half_width);
        let (along, across) = (marks.along_length(), marks.across_beam());
        assert!(drawn.iter().all(|vertex| on(&along, vertex.position[2]) || on(&across, vertex.position[0])));
        for mark in &along {
            assert!(drawn.iter().any(|vertex| (vertex.position[2] - mark.at).abs() <= half_width), "{mark:?}");
        }
        for mark in &across {
            assert!(drawn.iter().any(|vertex| (vertex.position[0] - mark.at).abs() <= half_width), "{mark:?}");
        }
    }

    /// The grid lies on the plane its marks were taken on, so a model's sits
    /// under it.
    #[test]
    fn the_grid_lies_on_the_marks_plane() {
        let marks = LengthMarks::new((Vec3::new(-1., -0.5, -2.), Vec3::new(1., 1., 2.)), -0.5).expect("marks");
        let (vertices, _) = build_waterline_grid(&marks, true);

        assert!(!vertices.is_empty());
        assert!(vertices.iter().all(|vertex| vertex.position[1] == -0.5));
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
