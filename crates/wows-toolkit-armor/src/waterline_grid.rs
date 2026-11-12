//! A grid on the waterline that fades away from the model, with heavier
//! lines at its length marks.

use crate::length_marks::LengthMarks;
use crate::viewport::renderer::LAYER_HULL;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;

/// Which parts of the grid under a model are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridLayers {
    /// The plain crosshatch.
    pub lines: bool,
    /// The heavier lines on the length marks.
    pub marks: bool,
}

/// Lays the grid under a model whose marks are `marks`. In world space, so
/// it stays level while the hull heels over. On the hull's layer, which is
/// tested against the armor's depth without writing its own, so the model
/// stands on the grid and hides it.
pub fn upload_waterline_grid(
    viewport: &mut Viewport3D,
    device: &wgpu::Device,
    marks: &LengthMarks,
    layers: GridLayers,
) {
    let (vertices, indices) = build_waterline_grid(marks, layers);
    if !indices.is_empty() {
        viewport.add_world_space_mesh(device, &vertices, &indices, LAYER_HULL);
    }
}

/// How far the grid reaches from the model's middle, in model lengths.
const GRID_REACH: f32 = 0.9;

/// How strong the grid is at `position`, from 1 under the model's middle to
/// 0 at the grid's edge.
pub fn grid_fade(marks: &LengthMarks, position: Vec3) -> f32 {
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
/// the marks are. `layers` picks the plain lines, the heavier lines on the
/// marks within the model, or both; the marks alone read as a ruler laid on
/// the plane.
pub fn build_waterline_grid(marks: &LengthMarks, layers: GridLayers) -> (Vec<Vertex>, Vec<u32>) {
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
        layers.marks && index >= 0 && index % lines_per_mark == 0 && (index / lines_per_mark) as u32 <= steps
    };
    let drawn = |(at, mark): (f32, bool)| (mark || layers.lines).then_some((at, mark));
    let x_lines = (first(middle.x - low.x)..=last(middle.x - low.x))
        .map(|index| (low.x + index as f32 * cell, is_mark(index, marks.beam_steps())))
        .filter_map(drawn);
    let z_lines = (first(high.z - middle.z)..=last(high.z - middle.z))
        .map(|index| (high.z - index as f32 * cell, is_mark(index, marks.length_steps())))
        .filter_map(drawn);

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
            vertices.push(Vertex {
                position: [x, plane_y, z],
                normal: [0., 1., 0.],
                color,
                uv: [0., 0.],
                tangent: [0.0; 4],
            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::length_marks::Mark;

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

    const BOTH: GridLayers = GridLayers { lines: true, marks: true };
    const LINES: GridLayers = GridLayers { lines: true, marks: false };
    const MARKS: GridLayers = GridLayers { lines: false, marks: true };
    const NEITHER: GridLayers = GridLayers { lines: false, marks: false };

    fn is_plain(vertex: &Vertex) -> bool {
        vertex.color[..3] == [0.62, 0.68, 0.78]
    }

    /// Vertices drawn in a mark's colour rather than a plain line's.
    fn mark_vertices(vertices: &[Vertex]) -> Vec<&Vertex> {
        vertices.iter().filter(|vertex| !is_plain(vertex)).collect()
    }

    /// Every mark has a line drawn along it, and every mark-coloured vertex
    /// lies on one.
    fn assert_marks_drawn(marks: &LengthMarks, vertices: &[Vertex]) {
        let drawn = mark_vertices(vertices);
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

    /// The grid lies on the waterline and is strongest under the ship's
    /// middle, fading to nothing toward its reach.
    #[test]
    fn the_grid_fades_away_from_the_ship() {
        let (vertices, indices) = build_waterline_grid(&ship_marks(), LINES);

        assert!(!indices.is_empty());
        assert!(vertices.iter().all(|vertex| vertex.position[1] == 0.), "on the waterline");
        let middle = alpha_at(&vertices, 0., 0.).expect("a line crosses the middle");
        let near = alpha_at(&vertices, 0., -100.).expect("a line reaches the bow");
        assert!(middle > near && near > 0., "fades with distance: {middle} then {near}");
        assert!(vertices.iter().all(|vertex| vertex.color[3] <= middle));
    }

    /// Both layers: the plain crosshatch, with the lines on the marks drawn
    /// in their own colour.
    #[test]
    fn grid_and_marks_draw_both() {
        let marks = ship_marks();
        let (vertices, _) = build_waterline_grid(&marks, BOTH);

        assert!(vertices.iter().any(is_plain));
        assert_marks_drawn(&marks, &vertices);
    }

    #[test]
    fn the_grid_alone_is_plain() {
        let (vertices, indices) = build_waterline_grid(&ship_marks(), LINES);

        assert!(!indices.is_empty());
        assert!(mark_vertices(&vertices).is_empty());
    }

    /// The marks without the grid: only the lines on the marks, as a ruler.
    #[test]
    fn the_marks_alone_draw_no_plain_lines() {
        let marks = ship_marks();
        let (vertices, indices) = build_waterline_grid(&marks, MARKS);

        assert!(!indices.is_empty());
        assert!(!vertices.iter().any(is_plain));
        assert_marks_drawn(&marks, &vertices);
    }

    #[test]
    fn neither_layer_draws_nothing() {
        let (vertices, indices) = build_waterline_grid(&ship_marks(), NEITHER);

        assert!(vertices.is_empty() && indices.is_empty());
    }

    /// The grid lies on the plane its marks were taken on, so a model's sits
    /// under it.
    #[test]
    fn the_grid_lies_on_the_marks_plane() {
        let marks = LengthMarks::new((Vec3::new(-1., -0.5, -2.), Vec3::new(1., 1., 2.)), -0.5).expect("marks");
        let (vertices, _) = build_waterline_grid(&marks, BOTH);

        assert!(!vertices.is_empty());
        assert!(vertices.iter().all(|vertex| vertex.position[1] == -0.5));
    }
}
