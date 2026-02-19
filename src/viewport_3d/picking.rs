use crate::viewport_3d::camera::{ArcballCamera, add, cross, dot, mat4_inverse, mat4_mul, normalize, scale, sub};
use crate::viewport_3d::types::{HitResult, MeshId};

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
) -> Option<([f32; 3], [f32; 3])> {
    let aspect = viewport_rect.width() / viewport_rect.height().max(1.0);
    let proj = camera.projection_matrix(aspect);
    let view = camera.view_matrix();
    let vp = mat4_mul(proj, view);
    let inv_vp = mat4_inverse(vp)?;

    // Normalize screen position to [-1, 1] (NDC)
    let ndc_x = ((screen_pos.x - viewport_rect.left()) / viewport_rect.width()) * 2.0 - 1.0;
    let ndc_y = 1.0 - ((screen_pos.y - viewport_rect.top()) / viewport_rect.height()) * 2.0;

    // Unproject near and far points
    let near_ndc = [ndc_x, ndc_y, 0.0, 1.0];
    let far_ndc = [ndc_x, ndc_y, 1.0, 1.0];

    let near_world = mat4_mul_vec4(inv_vp, near_ndc);
    let far_world = mat4_mul_vec4(inv_vp, far_ndc);

    if near_world[3].abs() < 1e-10 || far_world[3].abs() < 1e-10 {
        return None;
    }

    let near_pos = [near_world[0] / near_world[3], near_world[1] / near_world[3], near_world[2] / near_world[3]];
    let far_pos = [far_world[0] / far_world[3], far_world[1] / far_world[3], far_world[2] / far_world[3]];

    let dir = normalize(sub(far_pos, near_pos));
    Some((near_pos, dir))
}

/// Moller-Trumbore ray-triangle intersection.
/// Returns `Some(t)` where `t` is the distance along the ray to the hit point.
pub fn ray_triangle_intersect(
    origin: &[f32; 3],
    dir: &[f32; 3],
    v0: &[f32; 3],
    v1: &[f32; 3],
    v2: &[f32; 3],
) -> Option<f32> {
    const EPSILON: f32 = 1e-7;

    let edge1 = sub(*v1, *v0);
    let edge2 = sub(*v2, *v0);
    let h = cross(*dir, edge2);
    let a = dot(edge1, h);

    // Check both sides (double-sided)
    if a.abs() < EPSILON {
        return None;
    }

    let f = 1.0 / a;
    let s = sub(*origin, *v0);
    let u = f * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = cross(s, edge1);
    let v = f * dot(*dir, q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * dot(edge2, q);
    if t > EPSILON { Some(t) } else { None }
}

/// Pick the closest triangle across all given meshes.
pub(crate) fn pick_meshes(
    screen_pos: egui::Pos2,
    viewport_rect: egui::Rect,
    camera: &ArcballCamera,
    meshes: &[(MeshId, &PickableMesh, bool)], // (id, mesh, visible)
) -> Option<HitResult> {
    let (origin, dir) = screen_to_ray(screen_pos, viewport_rect, camera)?;

    let mut closest: Option<HitResult> = None;

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

            let v0 = &mesh.positions[i0];
            let v1 = &mesh.positions[i1];
            let v2 = &mesh.positions[i2];

            if let Some(t) = ray_triangle_intersect(&origin, &dir, v0, v1, v2) {
                let is_closer = closest.as_ref().is_none_or(|c| t < c.distance);
                if is_closer {
                    let world_pos = add(origin, scale(dir, t));
                    closest = Some(HitResult {
                        mesh_id: *mesh_id,
                        triangle_index: tri_idx,
                        distance: t,
                        world_position: world_pos,
                    });
                }
            }
        }
    }

    closest
}

/// Pick ALL triangles hit by a ray, sorted by distance (nearest first).
/// Each result includes the triangle normal for angle calculations.
pub(crate) fn pick_all_meshes(
    screen_pos: egui::Pos2,
    viewport_rect: egui::Rect,
    camera: &ArcballCamera,
    meshes: &[(MeshId, &PickableMesh, bool)],
) -> Vec<(HitResult, [f32; 3])> {
    let Some((origin, dir)) = screen_to_ray(screen_pos, viewport_rect, camera) else {
        return Vec::new();
    };

    let mut hits: Vec<(HitResult, [f32; 3])> = Vec::new();

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

            let v0 = &mesh.positions[i0];
            let v1 = &mesh.positions[i1];
            let v2 = &mesh.positions[i2];

            if let Some(t) = ray_triangle_intersect(&origin, &dir, v0, v1, v2) {
                let world_pos = add(origin, scale(dir, t));
                // Compute triangle normal
                let edge1 = sub(*v1, *v0);
                let edge2 = sub(*v2, *v0);
                let normal = normalize(cross(edge1, edge2));

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

/// Pick ALL triangles hit by a ray from an arbitrary origin and direction, sorted by distance.
/// Each result includes the triangle normal.
pub(crate) fn pick_all_ray(
    origin: [f32; 3],
    dir: [f32; 3],
    meshes: &[(MeshId, &PickableMesh, bool)],
) -> Vec<(HitResult, [f32; 3])> {
    let mut hits: Vec<(HitResult, [f32; 3])> = Vec::new();

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

            let v0 = &mesh.positions[i0];
            let v1 = &mesh.positions[i1];
            let v2 = &mesh.positions[i2];

            if let Some(t) = ray_triangle_intersect(&origin, &dir, v0, v1, v2) {
                let world_pos = add(origin, scale(dir, t));
                let edge1 = sub(*v1, *v0);
                let edge2 = sub(*v2, *v0);
                let normal = normalize(cross(edge1, edge2));

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

/// Multiply a 4x4 matrix by a 4-component vector.
fn mat4_mul_vec4(m: [[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2] + m[3][0] * v[3],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2] + m[3][1] * v[3],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2] + m[3][2] * v[3],
        m[0][3] * v[0] + m[1][3] * v[1] + m[2][3] * v[2] + m[3][3] * v[3],
    ]
}
