extern crate nalgebra as na;

use na::Vector3;

use crate::viewport_3d::types::Vertex;
use wowsunpack::game_params::types::CameraRing;

type Vec3 = Vector3<f32>;

const RING_SEGMENTS: usize = 96;
const MARKER_SEGMENTS: usize = 48;
const LINE_WIDTH: f32 = 0.03;
const MARKER_SIZE: f32 = 0.06;
const SPOKE_COUNT: usize = 24;

fn push_segment(verts: &mut Vec<Vertex>, indices: &mut Vec<u32>, p0: Vec3, p1: Vec3, color: [f32; 4]) {
    push_segment_gradient(verts, indices, p0, p1, color, color);
}

fn push_segment_gradient(
    verts: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    p0: Vec3,
    p1: Vec3,
    color0: [f32; 4],
    color1: [f32; 4],
) {
    let raw = p1 - p0;
    let len = raw.norm();
    if len < 1e-9 {
        return;
    }
    let dir = raw / len;
    let arbitrary = if dir[1].abs() < 0.9 { Vec3::y() } else { Vec3::x() };
    let perp1 = dir.cross(&arbitrary).normalize();
    let perp2 = dir.cross(&perp1).normalize();
    let normal: [f32; 3] = perp1.into();

    for perp in [perp1, perp2] {
        let off = perp * (LINE_WIDTH * 0.5);
        let b = verts.len() as u32;
        verts.push(Vertex { position: (p0 - off).into(), normal, color: color0, uv: [0.0, 0.0] });
        verts.push(Vertex { position: (p0 + off).into(), normal, color: color0, uv: [0.0, 0.0] });
        verts.push(Vertex { position: (p1 + off).into(), normal, color: color1, uv: [0.0, 0.0] });
        verts.push(Vertex { position: (p1 - off).into(), normal, color: color1, uv: [0.0, 0.0] });
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
}

/// Build a model-space overlay mesh for one resolved camera ring.
/// `waterline_dy` shifts the orbit center height by the ship's waterline offset.
/// When `with_markers` is true, also renders the orbit center marker and a
/// vertical line down to Y = 0.
pub(crate) fn build_camera_ellipse_mesh(
    ring: &CameraRing,
    waterline_dy: f32,
    color: [f32; 4],
    with_markers: bool,
) -> (Vec<Vertex>, Vec<u32>) {
    let mut verts: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let cx = ring.pos_center.x;
    let cy = ring.pos_center.y + waterline_dy;
    let cz = ring.pos_center.z;
    let center = Vec3::new(cx, cy, cz);

    let x_axis = Vec3::x();
    let z_axis = Vec3::z();

    let mut prev = center + x_axis * ring.semi_axes.x;
    for i in 1..=RING_SEGMENTS {
        let t = (i as f32 / RING_SEGMENTS as f32) * std::f32::consts::TAU;
        let p = center + x_axis * (t.cos() * ring.semi_axes.x) + z_axis * (t.sin() * ring.semi_axes.y);
        push_segment(&mut verts, &mut indices, prev, p, color);
        prev = p;
    }

    if with_markers {
        for axis in [Vec3::x(), Vec3::y(), Vec3::z()] {
            push_segment(&mut verts, &mut indices, center - axis * MARKER_SIZE, center + axis * MARKER_SIZE, color);
        }
        let foot = Vec3::new(center.x, 0.0, center.z);
        push_segment(&mut verts, &mut indices, center, foot, color);
    }

    (verts, indices)
}

/// Model-space points around one resolved ring, matching the drawn ellipse
/// (`waterline_dy` folded into Y). Used for screen-space cursor proximity.
pub(crate) fn sample_ring_points(ring: &CameraRing, waterline_dy: f32, n: usize) -> Vec<Vec3> {
    let center = Vec3::new(ring.pos_center.x, ring.pos_center.y + waterline_dy, ring.pos_center.z);
    (0..n)
        .map(|i| {
            let t = (i as f32 / n as f32) * std::f32::consts::TAU;
            center + Vec3::x() * (t.cos() * ring.semi_axes.x) + Vec3::z() * (t.sin() * ring.semi_axes.y)
        })
        .collect()
}

/// Build a model-space overlay mesh of radial spokes from the inner ring to the
/// outer ring at `SPOKE_COUNT` evenly spaced azimuths. Each spoke is one segment
/// whose color fades from `color_inner` at the inner endpoint to `color_outer`
/// at the outer endpoint, tracing the camera eye's straight-line zoom path for
/// that azimuth. `waterline_dy` folds into both rings' Y, matching the drawn
/// ellipses.
pub(crate) fn build_zoom_path_mesh(
    inner: &CameraRing,
    outer: &CameraRing,
    waterline_dy: f32,
    color_inner: [f32; 4],
    color_outer: [f32; 4],
) -> (Vec<Vertex>, Vec<u32>) {
    let mut verts: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let inner_pts = sample_ring_points(inner, waterline_dy, SPOKE_COUNT);
    let outer_pts = sample_ring_points(outer, waterline_dy, SPOKE_COUNT);
    for (p_inner, p_outer) in inner_pts.into_iter().zip(outer_pts) {
        push_segment_gradient(&mut verts, &mut indices, p_inner, p_outer, color_inner, color_outer);
    }

    (verts, indices)
}

/// Terse hover label for one ring: which mode/orbit it is and its key values.
pub(crate) fn ring_label(mode: &str, kind: &str, fov_tag: &str, ring: &CameraRing) -> String {
    format!(
        "{mode} {kind} ({fov_tag})\ny {:.2}  semiH {:.2}  semiV {:.2}",
        ring.pos_center.y, ring.semi_axes.x, ring.semi_axes.y
    )
}

/// Build a flat ring + center cross lying in the XZ plane at `center` (its `y`
/// is the water height), for the perspective aim-point marker. World-space.
pub(crate) fn build_water_marker_mesh(center: Vec3, radius: f32, color: [f32; 4]) -> (Vec<Vertex>, Vec<u32>) {
    let mut verts: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let x_axis = Vec3::x();
    let z_axis = Vec3::z();

    let mut prev = center + x_axis * radius;
    for i in 1..=MARKER_SEGMENTS {
        let t = (i as f32 / MARKER_SEGMENTS as f32) * std::f32::consts::TAU;
        let p = center + x_axis * (t.cos() * radius) + z_axis * (t.sin() * radius);
        push_segment(&mut verts, &mut indices, prev, p, color);
        prev = p;
    }
    let arm = radius * 0.5;
    push_segment(&mut verts, &mut indices, center - x_axis * arm, center + x_axis * arm, color);
    push_segment(&mut verts, &mut indices, center - z_axis * arm, center + z_axis * arm, color);

    (verts, indices)
}

/// A 3-axis cross marker centered at `center` (model space), each arm `half` long.
pub(crate) fn build_center_marker_mesh(center: Vec3, half: f32, color: [f32; 4]) -> (Vec<Vertex>, Vec<u32>) {
    let mut verts: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for axis in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 1.0)] {
        push_segment(&mut verts, &mut indices, center - axis * half, center + axis * half, color);
    }
    (verts, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring() -> CameraRing {
        use wowsunpack::game_types::Vec2 as CoreVec2;
        use wowsunpack::game_types::Vec3 as CoreVec3;
        CameraRing { pos_center: CoreVec3::new(0.0, 1.958, 0.0), semi_axes: CoreVec2::new(6.552, 9.6) }
    }

    #[test]
    fn ring_is_non_empty_and_indexed() {
        let (verts, indices) = build_camera_ellipse_mesh(&ring(), 0.0, [0.0, 1.0, 1.0, 1.0], true);
        assert!(!verts.is_empty());
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0);
        for &i in &indices {
            assert!((i as usize) < verts.len());
        }
    }

    #[test]
    fn ring_extents_match_semi_axes() {
        let (verts, _) = build_camera_ellipse_mesh(&ring(), 0.0, [0.0, 1.0, 1.0, 1.0], true);
        let max_x = verts.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        let max_z = verts.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
        assert!((max_x - 6.552).abs() < 0.3, "max_x={max_x}");
        assert!((max_z - 9.6).abs() < 0.3, "max_z={max_z}");
    }

    #[test]
    fn height_line_reaches_waterline() {
        let (verts, _) = build_camera_ellipse_mesh(&ring(), 0.0, [0.0, 1.0, 1.0, 1.0], true);
        let min_y = verts.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        assert!(min_y.abs() < 0.2, "min_y={min_y}");
    }

    #[test]
    fn center_marker_non_empty_valid_indices_bounded() {
        let center = Vec3::new(0.0, 0.0, 0.0);
        let half = 0.4_f32;
        let (verts, indices) = build_center_marker_mesh(center, half, [1.0, 0.85, 0.1, 1.0]);
        assert!(!verts.is_empty());
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0);
        for &i in &indices {
            assert!((i as usize) < verts.len(), "index {i} out of bounds (len {})", verts.len());
        }
        let max_coord = verts.iter().flat_map(|v| v.position).map(f32::abs).fold(0.0_f32, f32::max);
        assert!(max_coord <= half + LINE_WIDTH, "max coord {max_coord} exceeds half+line_width");
    }

    #[test]
    fn sample_points_count_and_extents() {
        let pts = sample_ring_points(&ring(), 0.0, 32);
        assert_eq!(pts.len(), 32);
        let max_x = pts.iter().map(|p| p.x).fold(f32::MIN, f32::max);
        let max_z = pts.iter().map(|p| p.z).fold(f32::MIN, f32::max);
        assert!((max_x - 6.552).abs() < 0.3, "max_x={max_x}");
        assert!((max_z - 9.6).abs() < 0.3, "max_z={max_z}");
    }

    #[test]
    fn sample_points_y_includes_waterline() {
        for p in sample_ring_points(&ring(), 0.5, 16) {
            assert!((p.y - (1.958 + 0.5)).abs() < 1e-4, "y={}", p.y);
        }
    }

    #[test]
    fn ring_label_has_name_and_values() {
        let label = ring_label("Observe", "outer", "FOV max", &ring());
        assert!(label.contains("Observe outer (FOV max)"), "{label}");
        assert!(label.contains("semiH 6.55"), "{label}");
        assert!(label.contains("semiV 9.60"), "{label}");
    }

    fn ring_pair() -> (CameraRing, CameraRing) {
        use wowsunpack::game_types::Vec2 as CoreVec2;
        use wowsunpack::game_types::Vec3 as CoreVec3;
        let inner = CameraRing { pos_center: CoreVec3::new(0.0, 2.0, 0.0), semi_axes: CoreVec2::new(6.0, 9.0) };
        let outer = CameraRing { pos_center: CoreVec3::new(0.0, 4.0, 0.0), semi_axes: CoreVec2::new(10.0, 14.0) };
        (inner, outer)
    }

    #[test]
    fn zoom_path_non_empty_valid_indices() {
        let (inner, outer) = ring_pair();
        let (verts, indices) =
            build_zoom_path_mesh(&inner, &outer, 0.0, [0.0, 0.9, 1.0, 0.85], [1.0, 0.6, 0.1, 0.85]);
        assert!(!verts.is_empty());
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0);
        for &i in &indices {
            assert!((i as usize) < verts.len());
        }
    }

    #[test]
    fn zoom_path_has_one_segment_per_spoke() {
        let (inner, outer) = ring_pair();
        let (verts, _) =
            build_zoom_path_mesh(&inner, &outer, 0.0, [0.0, 0.9, 1.0, 0.85], [1.0, 0.6, 0.1, 0.85]);
        // Each spoke is one gradient segment = 2 quads = 8 vertices.
        assert_eq!(verts.len(), SPOKE_COUNT * 8);
    }

    #[test]
    fn zoom_path_endpoints_lie_on_both_ellipses() {
        let (inner, outer) = ring_pair();
        let (verts, _) =
            build_zoom_path_mesh(&inner, &outer, 0.0, [0.0, 0.9, 1.0, 0.85], [1.0, 0.6, 0.1, 0.85]);
        let on_ellipse = |c: &CameraRing, p: &[f32; 3]| {
            let dx = (p[0] - c.pos_center.x) / c.semi_axes.x;
            let dz = (p[2] - c.pos_center.z) / c.semi_axes.y;
            (dx * dx + dz * dz - 1.0).abs() < 0.1 && (p[1] - c.pos_center.y).abs() < 0.1
        };
        let inner_hits = verts.iter().filter(|v| on_ellipse(&inner, &v.position)).count();
        let outer_hits = verts.iter().filter(|v| on_ellipse(&outer, &v.position)).count();
        assert!(inner_hits >= SPOKE_COUNT, "inner_hits={inner_hits}");
        assert!(outer_hits >= SPOKE_COUNT, "outer_hits={outer_hits}");
    }

    #[test]
    fn zoom_path_endpoints_respect_waterline() {
        let (inner, outer) = ring_pair();
        let dy = 0.5_f32;
        let (verts, _) =
            build_zoom_path_mesh(&inner, &outer, dy, [0.0, 0.9, 1.0, 0.85], [1.0, 0.6, 0.1, 0.85]);
        let on_ellipse = |c: &CameraRing, p: &[f32; 3]| {
            let dx = (p[0] - c.pos_center.x) / c.semi_axes.x;
            let dz = (p[2] - c.pos_center.z) / c.semi_axes.y;
            (dx * dx + dz * dz - 1.0).abs() < 0.1 && (p[1] - (c.pos_center.y + dy)).abs() < 0.1
        };
        let inner_hits = verts.iter().filter(|v| on_ellipse(&inner, &v.position)).count();
        let outer_hits = verts.iter().filter(|v| on_ellipse(&outer, &v.position)).count();
        assert!(inner_hits >= SPOKE_COUNT, "inner_hits={inner_hits}");
        assert!(outer_hits >= SPOKE_COUNT, "outer_hits={outer_hits}");
    }

    #[test]
    fn zoom_path_colors_are_per_endpoint() {
        let (inner, outer) = ring_pair();
        let ci = [0.0, 0.9, 1.0, 0.85];
        let co = [1.0, 0.6, 0.1, 0.85];
        let (verts, _) = build_zoom_path_mesh(&inner, &outer, 0.0, ci, co);
        let inner_colored = verts.iter().filter(|v| v.color == ci).count();
        let outer_colored = verts.iter().filter(|v| v.color == co).count();
        assert_eq!(inner_colored, verts.len() / 2);
        assert_eq!(outer_colored, verts.len() / 2);
    }

    #[test]
    fn water_marker_non_empty_on_radius_colored() {
        let c = Vec3::new(5.0, 0.0, -3.0);
        let r = 4.0;
        let col = [1.0, 0.6, 0.1, 0.85];
        let (verts, indices) = build_water_marker_mesh(c, r, col);
        assert!(!verts.is_empty() && !indices.is_empty());
        assert_eq!(indices.len() % 3, 0);
        for &i in &indices {
            assert!((i as usize) < verts.len());
        }
        for v in &verts {
            assert_eq!(v.color, col);
        }
        let on_ring = verts.iter().any(|v| {
            let dx = v.position[0] - c.x;
            let dz = v.position[2] - c.z;
            ((dx * dx + dz * dz).sqrt() - r).abs() < 0.2
        });
        assert!(on_ring, "no vertex near ring radius");
    }
}
