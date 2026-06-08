extern crate nalgebra as na;

use na::Vector3;

use wowsunpack::game_params::types::CameraTrajectory;
use crate::viewport_3d::types::Vertex;

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

/// Build a model-space overlay mesh for one camera orbit trajectory: the
/// horizontal orbit ring, a marker at the orbit center, and a vertical line
/// down to the waterline plane (Y = 0). `waterline_dy` is the waterline shift
/// hook added to the orbit center height; currently 0.
pub(crate) fn build_camera_ellipse_mesh(
    traj: &CameraTrajectory,
    waterline_dy: f32,
    color: [f32; 4],
) -> (Vec<Vertex>, Vec<u32>) {
    let mut verts: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let cx = traj.pos_center[0];
    let cy = traj.pos_center[1] + waterline_dy;
    let cz = traj.pos_center[2];
    let center = Vec3::new(cx, cy, cz);

    let x_axis = Vec3::x();
    let z_axis = Vec3::z();

    let mut prev = center + x_axis * traj.semi_axis_h;
    for i in 1..=RING_SEGMENTS {
        let t = (i as f32 / RING_SEGMENTS as f32) * std::f32::consts::TAU;
        let p = center + x_axis * (t.cos() * traj.semi_axis_h) + z_axis * (t.sin() * traj.semi_axis_v);
        push_segment(&mut verts, &mut indices, prev, p, color);
        prev = p;
    }

    for axis in [Vec3::x(), Vec3::y(), Vec3::z()] {
        push_segment(&mut verts, &mut indices, center - axis * MARKER_SIZE, center + axis * MARKER_SIZE, color);
    }

    let foot = Vec3::new(center.x, 0.0, center.z);
    push_segment(&mut verts, &mut indices, center, foot, color);

    (verts, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn traj() -> wowsunpack::game_params::types::CameraTrajectory {
        wowsunpack::game_params::types::CameraTrajectory { pos_center: [0.0, 1.958, 0.0], semi_axis_h: 6.552, semi_axis_v: 9.6 }
    }

    #[test]
    fn ring_is_non_empty_and_indexed() {
        let (verts, indices) = build_camera_ellipse_mesh(&traj(), 0.0, [0.0, 1.0, 1.0, 1.0]);
        assert!(!verts.is_empty());
        assert!(!indices.is_empty());
        assert_eq!(indices.len() % 3, 0);
        for &i in &indices { assert!((i as usize) < verts.len()); }
    }

    #[test]
    fn ring_extents_match_semi_axes() {
        let (verts, _) = build_camera_ellipse_mesh(&traj(), 0.0, [0.0, 1.0, 1.0, 1.0]);
        let max_x = verts.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max);
        let max_z = verts.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
        assert!((max_x - 6.552).abs() < 0.3, "max_x={max_x}");
        assert!((max_z - 9.6).abs() < 0.3, "max_z={max_z}");
    }

    #[test]
    fn height_line_reaches_waterline() {
        let (verts, _) = build_camera_ellipse_mesh(&traj(), 0.0, [0.0, 1.0, 1.0, 1.0]);
        let min_y = verts.iter().map(|v| v.position[1]).fold(f32::MAX, f32::min);
        assert!(min_y.abs() < 0.2, "min_y={min_y}");
    }
}
