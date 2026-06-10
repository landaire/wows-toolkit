extern crate nalgebra as na;

use na::Vector3;

use crate::viewport_3d::types::Vertex;
use wowsunpack::game_params::types::CameraRing;

type Vec3 = Vector3<f32>;

const RING_SEGMENTS: usize = 96;
const LINE_WIDTH: f32 = 0.03;
const MARKER_SIZE: f32 = 0.06;

fn push_segment(verts: &mut Vec<Vertex>, indices: &mut Vec<u32>, p0: Vec3, p1: Vec3, color: [f32; 4]) {
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
        verts.push(Vertex { position: (p0 - off).into(), normal, color, uv: [0.0, 0.0] });
        verts.push(Vertex { position: (p0 + off).into(), normal, color, uv: [0.0, 0.0] });
        verts.push(Vertex { position: (p1 + off).into(), normal, color, uv: [0.0, 0.0] });
        verts.push(Vertex { position: (p1 - off).into(), normal, color, uv: [0.0, 0.0] });
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

/// Terse hover label for one ring: which mode/orbit it is and its key values.
pub(crate) fn ring_label(mode: &str, kind: &str, fov_tag: &str, ring: &CameraRing) -> String {
    format!(
        "{mode} {kind} ({fov_tag})\ny {:.2}  semiH {:.2}  semiV {:.2}",
        ring.pos_center.y, ring.semi_axes.x, ring.semi_axes.y
    )
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
}
