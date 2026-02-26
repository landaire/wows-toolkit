extern crate nalgebra as na;
use na::Vector4;

use crate::viewport_3d::camera::mat4_mul;
use crate::viewport_3d::camera::{ArcballCamera, mat4_to_na};
use crate::viewport_3d::types::{HitResult, MeshId, Vec3};

/// CPU-side mesh data retained for picking.
pub(crate) struct PickableMesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// Unproject a screen point to a world-space ray (origin, direction).
pub fn screen_to_ray(
    screen_pos: egui::Pos2,
    viewport_rect: egui::Rect,
    camera: &ArcballCamera,
) -> Option<(Vec3, Vec3)> {
    let aspect = viewport_rect.width() / viewport_rect.height().max(1.0);
    let proj = camera.projection_matrix(aspect);
    let view = camera.view_matrix();
    let vp = mat4_mul(proj, view);
    let inv_vp_na = mat4_to_na(vp).try_inverse()?;

    // Normalize screen position to [-1, 1] (NDC)
    let ndc_x = ((screen_pos.x - viewport_rect.left()) / viewport_rect.width()) * 2.0 - 1.0;
    let ndc_y = 1.0 - ((screen_pos.y - viewport_rect.top()) / viewport_rect.height()) * 2.0;

    // Unproject near and far points
    let near_clip = Vector4::new(ndc_x, ndc_y, 0.0, 1.0);
    let far_clip = Vector4::new(ndc_x, ndc_y, 1.0, 1.0);

    let near_world = inv_vp_na * near_clip;
    let far_world = inv_vp_na * far_clip;

    if near_world.w.abs() < 1e-10 || far_world.w.abs() < 1e-10 {
        return None;
    }

    let near_pos = Vec3::new(near_world.x / near_world.w, near_world.y / near_world.w, near_world.z / near_world.w);
    let far_pos = Vec3::new(far_world.x / far_world.w, far_world.y / far_world.w, far_world.z / far_world.w);

    let dir = (far_pos - near_pos).normalize();
    Some((near_pos, dir))
}

/// Moller-Trumbore ray-triangle intersection.
/// Returns `Some(t)` where `t` is the distance along the ray to the hit point.
pub fn ray_triangle_intersect(origin: &Vec3, dir: &Vec3, v0: &Vec3, v1: &Vec3, v2: &Vec3) -> Option<f32> {
    const EPSILON: f32 = 1e-7;

    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = dir.cross(&edge2);
    let a = edge1.dot(&h);

    // Check both sides (double-sided)
    if a.abs() < EPSILON {
        return None;
    }

    let f = 1.0 / a;
    let s = origin - v0;
    let u = f * s.dot(&h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = s.cross(&edge1);
    let v = f * dir.dot(&q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * edge2.dot(&q);
    if t > EPSILON { Some(t) } else { None }
}

/// Pick ALL triangles hit by a ray from an arbitrary origin and direction, sorted by distance.
/// Each result includes the triangle normal.
pub(crate) fn pick_all_ray(
    origin: Vec3,
    dir: Vec3,
    meshes: &[(MeshId, &PickableMesh, bool)],
) -> Vec<(HitResult, Vec3)> {
    let mut hits: Vec<(HitResult, Vec3)> = Vec::new();

    for (mesh_id, mesh, visible) in meshes {
        if !visible {
            continue;
        }

        let num_triangles = mesh.indices.len() / 3;
        for tri_idx in 0..num_triangles {
            let i0 = mesh.indices[tri_idx * 3] as usize;
            let i1 = mesh.indices[tri_idx * 3 + 1] as usize;
            let i2 = mesh.indices[tri_idx * 3 + 2] as usize;

            if i0 >= mesh.positions.len() || i1 >= mesh.positions.len() || i2 >= mesh.positions.len() {
                continue;
            }

            // Convert from GPU [f32; 3] at the boundary
            let v0 = Vec3::from(mesh.positions[i0]);
            let v1 = Vec3::from(mesh.positions[i1]);
            let v2 = Vec3::from(mesh.positions[i2]);

            if let Some(t) = ray_triangle_intersect(&origin, &dir, &v0, &v1, &v2) {
                let world_pos = origin + dir * t;
                let edge1 = v1 - v0;
                let edge2 = v2 - v0;
                let normal = edge1.cross(&edge2).normalize();

                hits.push((
                    HitResult { mesh_id: *mesh_id, triangle_index: tri_idx, distance: t, world_position: world_pos },
                    normal,
                ));
            }
        }
    }

    hits.sort_by(|a, b| a.0.distance.partial_cmp(&b.0.distance).unwrap_or(std::cmp::Ordering::Equal));
    hits
}
