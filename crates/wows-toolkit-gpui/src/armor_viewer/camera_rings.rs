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

/// How near the pointer has to come to an orbit's drawn curve to read it.
///
/// Measured on screen rather than by raycast: the orbits are drawn a few
/// centimetres wide, which no pick would reliably land on.
const HOVER_THRESHOLD_PX: f32 = 8.0;

/// One drawn orbit and what it reads as, for a pointer resting on it.
pub(crate) struct RingHover {
    pub label: String,
    pub ring: CameraRing,
    pub waterline_dy: f32,
}

/// What an orbit reads as: which mode it belongs to, which of the two orbits
/// it is, at which field of view, and the numbers behind it.
pub(crate) fn ring_label(mode: &str, kind: &str, fov_tag: &str, ring: &CameraRing) -> String {
    format!(
        "{mode} {kind} ({fov_tag})
y {:.2}  semiH {:.2}  semiV {:.2}",
        ring.pos_center.y, ring.semi_axes.x, ring.semi_axes.y
    )
}

/// What the orbit nearest `cursor` reads as, if the pointer is near one.
///
/// `project` puts a point of the orbit where it is drawn on screen, and
/// answers `None` for one behind the camera. The distance is measured to the
/// drawn curve rather than to the orbit's centre, so resting on the far side
/// of a ring reads it just as the near side does.
pub(crate) fn nearest_ring_label(
    hovers: &[RingHover],
    project: impl Fn(Vec3) -> Option<[f32; 2]>,
    cursor: [f32; 2],
) -> Option<&str> {
    let mut best: Option<(f32, &str)> = None;
    for hover in hovers {
        let points: Vec<Option<[f32; 2]>> =
            sample_ring(&hover.ring, hover.waterline_dy, RING_SEGMENTS).into_iter().map(&project).collect();
        let mut nearest = f32::MAX;
        for step in 0..points.len() {
            if let (Some(from), Some(to)) = (points[step], points[(step + 1) % points.len()]) {
                nearest = nearest.min(distance_to_segment(cursor, from, to));
            }
        }
        if best.is_none_or(|(was, _)| nearest < was) {
            best = Some((nearest, hover.label.as_str()));
        }
    }
    best.filter(|(distance, _)| *distance <= HOVER_THRESHOLD_PX).map(|(_, label)| label)
}

/// How far `point` is from the segment `from`-`to`, in the same units.
fn distance_to_segment(point: [f32; 2], from: [f32; 2], to: [f32; 2]) -> f32 {
    let along = [to[0] - from[0], to[1] - from[1]];
    let length_squared = along[0] * along[0] + along[1] * along[1];
    let to_point = [point[0] - from[0], point[1] - from[1]];
    // A segment of no length is a point, and its own start is the nearest
    // thing on it.
    let travel = if length_squared <= f32::EPSILON {
        0.0
    } else {
        ((to_point[0] * along[0] + to_point[1] * along[1]) / length_squared).clamp(0.0, 1.0)
    };
    let nearest = [from[0] + along[0] * travel, from[1] + along[1] * travel];
    ((point[0] - nearest[0]).powi(2) + (point[1] - nearest[1]).powi(2)).sqrt()
}

/// What to draw for one mode, and how it was asked for.
pub(crate) struct RingRequest<'a> {
    /// The mode these orbits belong to, which is what a hover names them by.
    pub mode: &'a str,
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
///
/// The orbits are handed back alongside the mesh so a pointer resting on one
/// can be told which it is without the caller laying them out again.
pub(crate) fn build_camera_rings(request: &RingRequest<'_>) -> (Vec<Vertex>, Vec<u32>, Vec<RingHover>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut hovers: Vec<RingHover> = Vec::new();
    let dy = request.waterline_dy;
    let mode = request.mode;

    let mut draw = |ring: CameraRing, kind: &str, fov_tag: &str, color: [f32; 4], markers: bool| {
        let (mut v, i) = build_ring_mesh(&ring, dy, color, markers);
        let base = vertices.len() as u32;
        indices.extend(i.into_iter().map(|index| index + base));
        vertices.append(&mut v);
        hovers.push(RingHover { label: ring_label(mode, kind, fov_tag, &ring), ring, waterline_dy: dy });
    };

    // The selected orbit solid, the extremes faint: the reader is choosing
    // between them, so only one should read as the answer.
    draw(request.trajectory.resolve(request.fov, request.height), "inner", CURRENT_FOV, INNER_COLOR, true);
    for (fov, tag) in [(0.0_f32, MIN_FOV), (1.0, MAX_FOV)] {
        draw(request.trajectory.resolve(fov, request.height), "inner", tag, faded(INNER_COLOR), false);
    }

    if let Some(outer) = request.trajectory.resolve_outer(request.fov, request.height) {
        draw(outer, "outer", CURRENT_FOV, OUTER_COLOR, true);
        for (fov, tag) in [(0.0_f32, MIN_FOV), (1.0, MAX_FOV)] {
            if let Some(outer) = request.trajectory.resolve_outer(fov, request.height) {
                draw(outer, "outer", tag, faded(OUTER_COLOR), false);
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

    (vertices, indices, hovers)
}

/// How a hover names the field of view an orbit was resolved at.
const CURRENT_FOV: &str = "current FOV";
const MIN_FOV: &str = "FOV min";
const MAX_FOV: &str = "FOV max";

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

/// A flat ring with a cross through it, lying on the water at `center`.
///
/// Where the locked camera is looking, which is the only thing on screen that
/// says how far down the view is tilted. World space, since it is a point on
/// the water rather than a point on the ship.
pub(crate) fn build_water_marker(center: Vec3, radius: f32, color: [f32; 4]) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let mut previous = center + Vec3::x() * radius;
    for step in 1..=RING_SEGMENTS {
        let around = (step as f32 / RING_SEGMENTS as f32) * std::f32::consts::TAU;
        let point = center + Vec3::x() * (around.cos() * radius) + Vec3::z() * (around.sin() * radius);
        push_segment(&mut vertices, &mut indices, previous, point, color, color);
        previous = point;
    }

    let arm = radius * 0.5;
    for axis in [Vec3::x(), Vec3::z()] {
        push_segment(&mut vertices, &mut indices, center - axis * arm, center + axis * arm, color, color);
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

    /// A flattened projection, so a ring's own coordinates are its screen
    /// ones and a distance can be read off by hand.
    fn flat(point: Vec3) -> Option<[f32; 2]> {
        Some([point.x, point.z])
    }

    fn hover(label: &str, ring: CameraRing) -> RingHover {
        RingHover { label: label.to_string(), ring, waterline_dy: 0.0 }
    }

    /// A label names the orbit and carries the numbers behind it, which is
    /// what the reader is comparing between modes.
    #[test]
    fn a_ring_reads_as_its_mode_its_orbit_and_its_measurements() {
        let label = ring_label("Observe", "outer", "FOV max", &ring(3.4, 6.55, 9.6));

        assert!(label.contains("Observe outer (FOV max)"), "{label}");
        assert!(label.contains("y 3.40"), "{label}");
        assert!(label.contains("semiH 6.55"), "{label}");
        assert!(label.contains("semiV 9.60"), "{label}");
    }

    /// A pointer on an orbit's drawn curve reads it, and one out in open
    /// space reads nothing.
    #[test]
    fn an_orbit_is_read_from_its_curve_rather_than_its_centre() {
        let hovers = [hover("outer orbit", ring(0.0, 30.0, 20.0))];

        assert_eq!(nearest_ring_label(&hovers, flat, [30.0, 0.0]), Some("outer orbit"), "on the curve");
        assert_eq!(nearest_ring_label(&hovers, flat, [0.0, 0.0]), None, "the centre is not the ring");
        assert_eq!(nearest_ring_label(&hovers, flat, [80.0, 0.0]), None, "and neither is open water");
    }

    /// With two orbits drawn the nearer one is read, not the first.
    #[test]
    fn the_nearer_of_two_orbits_is_the_one_read() {
        let hovers = [hover("inner", ring(0.0, 10.0, 10.0)), hover("outer", ring(0.0, 30.0, 30.0))];

        assert_eq!(nearest_ring_label(&hovers, flat, [30.0, 0.0]), Some("outer"));
        assert_eq!(nearest_ring_label(&hovers, flat, [10.0, 0.0]), Some("inner"));
    }

    /// An orbit swinging behind the camera still reads from the part of it
    /// that is on screen.
    #[test]
    fn an_orbit_half_off_screen_still_reads() {
        let hovers = [hover("inner", ring(0.0, 30.0, 20.0))];
        let half = |point: Vec3| if point.x < 0.0 { None } else { Some([point.x, point.z]) };

        assert_eq!(nearest_ring_label(&hovers, half, [30.0, 0.0]), Some("inner"));
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
