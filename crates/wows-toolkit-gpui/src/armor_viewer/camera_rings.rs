//! The orbits the game's own camera rides around a ship, drawn over the
//! armor.
//!
//! A ship's GameParams name one trajectory per camera mode; each resolves,
//! against a field of view and a height, to an ellipse the camera eye sits
//! on. Seeing them against the armor is what says which plates a shell is
//! actually aimed at from a given view.
//!
//! The resolution itself is `wowsunpack`'s (`CameraTrajectory::resolve`), so
//! both apps ride the same orbits; this is the geometry that draws them.

use nalgebra::Vector3;
use wowsunpack::game_params::types::CameraRing;
use wowsunpack::game_params::types::CameraTrajectory;

use crate::viewport::types::Vertex;

type Vec3 = Vector3<f32>;

/// How many straight segments an ellipse is drawn as. Enough that the curve
/// reads as one at the distance a ship is viewed from.
const RING_SEGMENTS: usize = 96;

/// How wide a drawn line is, in metres.
const LINE_WIDTH: f32 = 0.03;

/// The arm length of the cross marking an orbit's centre.
const MARKER_SIZE: f32 = 0.06;

/// How many spokes trace the path between the inner and outer orbits.
const SPOKE_COUNT: usize = 24;

/// The inner orbit, the one the camera sits on scrolled in.
const INNER_COLOR: [f32; 4] = [0.0, 0.9, 1.0, 1.0];

/// The outer orbit, scrolled out. A different hue, because the two are read
/// against each other.
const OUTER_COLOR: [f32; 4] = [1.0, 0.6, 0.1, 1.0];

/// How opaque the orbits at the ends of the field-of-view range are drawn:
/// context for the one actually selected, rather than a third reading.
const EXTREME_ALPHA: f32 = 0.25;

/// What to draw for one mode, and how it was asked for.
pub(crate) struct RingRequest<'a> {
    pub trajectory: &'a CameraTrajectory,
    /// Where between the field-of-view extremes the camera is, 0 to 1.
    pub fov: f32,
    /// How far the camera is raised, -1 to 1.
    pub height: f32,
    /// Also trace the path between the inner and outer orbits.
    pub zoom_path: bool,
    /// Trace it at the selected field of view.
    pub zoom_path_at_fov: bool,
    /// Trace it at the widest field of view, which is where the path bends
    /// most.
    pub zoom_path_at_max_fov: bool,
    /// The ship's waterline offset, folded into every orbit's height so the
    /// rings sit where the camera does relative to the water.
    pub waterline_dy: f32,
}

/// The overlay for one camera mode: the orbits at the selected field of view,
/// the same orbits at both extremes of it, and the zoom path if asked for.
pub(crate) fn build_camera_rings(request: &RingRequest<'_>) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let dy = request.waterline_dy;

    let mut draw = |ring: &CameraRing, color: [f32; 4], markers: bool| {
        let (mut v, i) = build_ring_mesh(ring, dy, color, markers);
        let base = vertices.len() as u32;
        indices.extend(i.into_iter().map(|index| index + base));
        vertices.append(&mut v);
    };

    // The selected orbit solid, the extremes faint: the reader is choosing
    // between them, so only one should read as the answer.
    draw(&request.trajectory.resolve(request.fov, request.height), INNER_COLOR, true);
    for fov in [0.0_f32, 1.0] {
        draw(&request.trajectory.resolve(fov, request.height), faded(INNER_COLOR), false);
    }

    if let Some(outer) = request.trajectory.resolve_outer(request.fov, request.height) {
        draw(&outer, OUTER_COLOR, true);
        for fov in [0.0_f32, 1.0] {
            if let Some(outer) = request.trajectory.resolve_outer(fov, request.height) {
                draw(&outer, faded(OUTER_COLOR), false);
            }
        }
    }

    if request.zoom_path {
        for (wanted, fov, alpha) in
            [(request.zoom_path_at_fov, request.fov, 0.85_f32), (request.zoom_path_at_max_fov, 1.0, 0.5)]
        {
            if !wanted {
                continue;
            }
            let inner = request.trajectory.resolve(fov, request.height);
            let Some(outer) = request.trajectory.resolve_outer(fov, request.height) else { continue };
            let (mut v, i) =
                build_zoom_path_mesh(&inner, &outer, dy, at_alpha(INNER_COLOR, alpha), at_alpha(OUTER_COLOR, alpha));
            let base = vertices.len() as u32;
            indices.extend(i.into_iter().map(|index| index + base));
            vertices.append(&mut v);
        }
    }

    (vertices, indices)
}

fn faded(color: [f32; 4]) -> [f32; 4] {
    at_alpha(color, EXTREME_ALPHA)
}

fn at_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], alpha]
}

/// One orbit as a closed ellipse, optionally with its centre marked and a
/// line dropped to the waterline.
pub(crate) fn build_ring_mesh(
    ring: &CameraRing,
    waterline_dy: f32,
    color: [f32; 4],
    with_markers: bool,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let center = ring_center(ring, waterline_dy);

    let mut previous = center + Vec3::x() * ring.semi_axes.x;
    for step in 1..=RING_SEGMENTS {
        let around = (step as f32 / RING_SEGMENTS as f32) * std::f32::consts::TAU;
        let point =
            center + Vec3::x() * (around.cos() * ring.semi_axes.x) + Vec3::z() * (around.sin() * ring.semi_axes.y);
        push_segment(&mut vertices, &mut indices, previous, point, color, color);
        previous = point;
    }

    if with_markers {
        for axis in [Vec3::x(), Vec3::y(), Vec3::z()] {
            let (from, to) = (center - axis * MARKER_SIZE, center + axis * MARKER_SIZE);
            push_segment(&mut vertices, &mut indices, from, to, color, color);
        }
        // Down to the water, which is what says how high the orbit sits.
        let foot = Vec3::new(center.x, 0.0, center.z);
        push_segment(&mut vertices, &mut indices, center, foot, color, color);
    }

    (vertices, indices)
}

/// The straight lines the camera eye travels between the two orbits, at
/// evenly spaced bearings.
pub(crate) fn build_zoom_path_mesh(
    inner: &CameraRing,
    outer: &CameraRing,
    waterline_dy: f32,
    color_inner: [f32; 4],
    color_outer: [f32; 4],
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for (from, to) in
        sample_ring(inner, waterline_dy, SPOKE_COUNT).into_iter().zip(sample_ring(outer, waterline_dy, SPOKE_COUNT))
    {
        push_segment(&mut vertices, &mut indices, from, to, color_inner, color_outer);
    }

    (vertices, indices)
}

/// `count` points evenly spaced around `ring`.
pub(crate) fn sample_ring(ring: &CameraRing, waterline_dy: f32, count: usize) -> Vec<Vec3> {
    let center = ring_center(ring, waterline_dy);
    (0..count)
        .map(|step| {
            let around = (step as f32 / count as f32) * std::f32::consts::TAU;
            center + Vec3::x() * (around.cos() * ring.semi_axes.x) + Vec3::z() * (around.sin() * ring.semi_axes.y)
        })
        .collect()
}

fn ring_center(ring: &CameraRing, waterline_dy: f32) -> Vec3 {
    Vec3::new(ring.pos_center.x, ring.pos_center.y + waterline_dy, ring.pos_center.z)
}

/// One line, drawn as two crossed quads so it is visible from any angle
/// rather than vanishing edge-on.
fn push_segment(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    from: Vec3,
    to: Vec3,
    color_from: [f32; 4],
    color_to: [f32; 4],
) {
    let along = to - from;
    let length = along.norm();
    if length < 1e-9 {
        return;
    }
    let direction = along / length;
    // Any axis not parallel to the line will do to get a perpendicular from.
    let reference = if direction[1].abs() < 0.9 { Vec3::y() } else { Vec3::x() };
    let across = direction.cross(&reference).normalize();
    let up = direction.cross(&across).normalize();
    let normal: [f32; 3] = across.into();

    for perpendicular in [across, up] {
        let offset = perpendicular * (LINE_WIDTH * 0.5);
        let base = vertices.len() as u32;
        vertices.push(Vertex { position: (from - offset).into(), normal, color: color_from, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: (from + offset).into(), normal, color: color_from, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: (to + offset).into(), normal, color: color_to, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: (to - offset).into(), normal, color: color_to, uv: [0.0, 0.0] });
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowsunpack::game_types::Vec2;
    use wowsunpack::game_types::Vec3 as ParamVec3;

    fn ring(center_y: f32, semi_x: f32, semi_y: f32) -> CameraRing {
        CameraRing { pos_center: ParamVec3 { x: 0.0, y: center_y, z: 0.0 }, semi_axes: Vec2 { x: semi_x, y: semi_y } }
    }

    /// The ellipse is an ellipse: its extents follow the two semi-axes
    /// separately rather than being a circle of one of them.
    #[test]
    fn a_rings_points_follow_both_of_its_semi_axes() {
        let points = sample_ring(&ring(0.0, 10.0, 4.0), 0.0, 4);

        assert_eq!(points.len(), 4);
        assert!((points[0].x - 10.0).abs() < 1e-4, "the first point is out along x");
        assert!((points[1].z - 4.0).abs() < 1e-4, "a quarter turn is out along z");
    }

    /// The waterline offset raises the whole orbit, which is what keeps the
    /// rings at the height the camera actually rides.
    #[test]
    fn the_waterline_offset_raises_the_orbit() {
        let level = sample_ring(&ring(5.0, 10.0, 10.0), 0.0, 4);
        let raised = sample_ring(&ring(5.0, 10.0, 10.0), 2.0, 4);

        assert!((raised[0].y - level[0].y - 2.0).abs() < 1e-4);
    }

    /// A line is drawn as two crossed quads, so it does not vanish when
    /// viewed edge-on: eight corners and two triangles per quad.
    #[test]
    fn a_line_is_drawn_as_two_crossed_quads() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_segment(&mut vertices, &mut indices, Vec3::zeros(), Vec3::x(), INNER_COLOR, INNER_COLOR);

        assert_eq!(vertices.len(), 8);
        assert_eq!(indices.len(), 12);
    }

    /// A line of no length has no direction to be drawn along, and is left
    /// out rather than drawn as a degenerate quad.
    #[test]
    fn a_line_of_no_length_is_not_drawn() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        push_segment(&mut vertices, &mut indices, Vec3::zeros(), Vec3::zeros(), INNER_COLOR, INNER_COLOR);

        assert!(vertices.is_empty());
        assert!(indices.is_empty());
    }

    /// The spokes run from one orbit to the other, one per bearing, and each
    /// fades from the inner orbit's tone to the outer's so the direction of
    /// travel reads off the drawing.
    #[test]
    fn the_zoom_path_runs_one_spoke_per_bearing() {
        let (vertices, indices) =
            build_zoom_path_mesh(&ring(5.0, 10.0, 10.0), &ring(8.0, 20.0, 20.0), 0.0, INNER_COLOR, OUTER_COLOR);

        assert_eq!(indices.len(), SPOKE_COUNT * 12, "two crossed quads per spoke");
        assert_eq!(vertices.first().map(|v| v.color), Some(INNER_COLOR));
        assert_eq!(vertices.get(2).map(|v| v.color), Some(OUTER_COLOR), "the far end carries the outer tone");
    }

    /// Marking a ring's centre adds the cross and the drop to the water; an
    /// unmarked ring is the ellipse alone.
    #[test]
    fn markers_are_drawn_only_when_asked_for() {
        let (plain, _) = build_ring_mesh(&ring(5.0, 10.0, 10.0), 0.0, INNER_COLOR, false);
        let (marked, _) = build_ring_mesh(&ring(5.0, 10.0, 10.0), 0.0, INNER_COLOR, true);

        assert_eq!(plain.len(), RING_SEGMENTS * 8);
        // Three axis arms and one drop to the water, each two crossed quads.
        assert_eq!(marked.len(), plain.len() + 4 * 8);
    }
}
