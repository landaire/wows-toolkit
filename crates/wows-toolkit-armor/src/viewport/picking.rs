extern crate nalgebra as na;
use na::Vector4;

use crate::viewport::camera::ArcballCamera;
use crate::viewport::camera::mat4_mul;
use crate::viewport::camera::mat4_to_na;
use crate::viewport::types::Bounds;
use crate::viewport::types::HitResult;
use crate::viewport::types::MeshId;
use crate::viewport::types::Ray;
use crate::viewport::types::Vec2;
use crate::viewport::types::Vec3;
use crate::viewport::types::ViewRect;

/// CPU-side mesh data retained for picking.
pub struct PickableMesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// The box around the whole mesh, so a ray that misses it is not tested
    /// against every triangle in it. Kept rather than derived per cast: a
    /// comparison over a whole battle casts one ray per landed shell.
    bounds: Option<Bounds>,
}

impl PickableMesh {
    pub fn new(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Self {
        let bounds = positions.iter().fold(None, |held: Option<Bounds>, at| {
            let at = Vec3::from(*at);
            Some(match held {
                Some(held) => Bounds { min: held.min.inf(&at), max: held.max.sup(&at) },
                None => Bounds { min: at, max: at },
            })
        });
        Self { positions, indices, bounds }
    }
}

/// One mesh a pick is tested against.
pub struct PickTarget<'a> {
    pub id: MeshId,
    pub mesh: &'a PickableMesh,
    pub visible: bool,
}

/// Below this a ray counts as parallel to a slab, where dividing by its
/// component would run away.
const PARALLEL: f32 = 1e-7;

/// Whether a ray reaches the box at all, by the slab test.
///
/// True for a mesh whose extent is not known, which is one with no vertices in
/// it: there is nothing to reject and nothing to hit either.
fn ray_reaches(ray: &Ray, bounds: Option<Bounds>) -> bool {
    let Some(Bounds { min: low, max: high }) = bounds else { return true };
    let Ray { origin, direction: dir } = ray;
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        // A ray parallel to a pair of slabs either runs between them for its
        // whole length or misses them entirely.
        if dir[axis].abs() < PARALLEL {
            if origin[axis] < low[axis] || origin[axis] > high[axis] {
                return false;
            }
            continue;
        }
        let inverse = 1.0 / dir[axis];
        let (first, second) = ((low[axis] - origin[axis]) * inverse, (high[axis] - origin[axis]) * inverse);
        let (enter, leave) = if first <= second { (first, second) } else { (second, first) };
        near = near.max(enter);
        far = far.min(leave);
        if near > far {
            return false;
        }
    }
    far >= 0.0
}

/// Unproject a screen point to a world-space ray.
pub fn screen_to_ray(screen_pos: Vec2, viewport_rect: ViewRect, camera: &ArcballCamera) -> Option<Ray> {
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

    let direction = (far_pos - near_pos).normalize();
    Some(Ray { origin: near_pos, direction })
}

/// Moller-Trumbore ray-triangle intersection.
/// Returns `Some(t)` where `t` is the distance along the ray to the hit point.
pub fn ray_triangle_intersect(ray: &Ray, v0: &Vec3, v1: &Vec3, v2: &Vec3) -> Option<f32> {
    wowsunpack::export::armor_ray::ray_triangle_intersect(
        ray.origin.into(),
        ray.direction.into(),
        &[(*v0).into(), (*v1).into(), (*v2).into()],
    )
}

/// Pick ALL triangles hit by a ray, sorted by distance. Each result includes
/// the triangle normal.
pub fn pick_all_ray(ray: Ray, targets: &[PickTarget<'_>]) -> Vec<(HitResult, Vec3)> {
    let Ray { origin, direction: dir } = ray;
    let mut hits: Vec<(HitResult, Vec3)> = Vec::new();

    for target in targets {
        let mesh = target.mesh;
        if !target.visible || !ray_reaches(&ray, mesh.bounds) {
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

            if let Some(t) = ray_triangle_intersect(&ray, &v0, &v1, &v2) {
                let world_pos = origin + dir * t;
                let edge1 = v1 - v0;
                let edge2 = v2 - v0;
                let normal = edge1.cross(&edge2).normalize();

                hits.push((
                    HitResult { mesh_id: target.id, triangle_index: tri_idx, distance: t, world_position: world_pos },
                    normal,
                ));
            }
        }
    }

    // A degenerate triangle can yield a NaN distance; it keeps its place.
    hits.sort_by(|a, b| a.0.distance.partial_cmp(&b.0.distance).unwrap_or(std::cmp::Ordering::Equal));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube about the origin, as two triangles on its near face plus the
    /// vertices that set its extent.
    fn cube() -> PickableMesh {
        let positions = vec![
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        PickableMesh::new(positions, vec![0, 1, 2, 0, 2, 3])
    }

    /// The box is read off the vertices, not the triangles, so a mesh whose
    /// extent is set by vertices no triangle uses is still not rejected.
    #[test]
    fn the_box_covers_every_vertex() {
        let Bounds { min: low, max: high } = cube().bounds.expect("a mesh with vertices has an extent");
        assert_eq!((low.x, low.y, low.z), (-1.0, -1.0, -1.0));
        assert_eq!((high.x, high.y, high.z), (1.0, 1.0, 1.0));
    }

    /// A ray aimed at the box reaches it, and one aimed past it does not.
    #[test]
    fn a_ray_past_the_box_is_rejected() {
        let bounds = cube().bounds;
        let toward = Vec3::new(0.0, 0.0, 1.0);
        let ray = |origin: Vec3| Ray { origin, direction: toward };
        assert!(ray_reaches(&ray(Vec3::new(0.0, 0.0, -10.0)), bounds), "straight at it");
        assert!(!ray_reaches(&ray(Vec3::new(5.0, 0.0, -10.0)), bounds), "five units to the side");
        assert!(!ray_reaches(&ray(Vec3::new(0.0, 0.0, 10.0)), bounds), "and one pointing away");
    }

    /// A ray running parallel to a pair of slabs is judged on whether it lies
    /// between them, which a division by its own component cannot answer.
    #[test]
    fn a_parallel_ray_is_judged_on_where_it_lies() {
        let bounds = cube().bounds;
        let along = Vec3::new(1.0, 0.0, 0.0);
        let ray = |origin: Vec3| Ray { origin, direction: along };
        assert!(ray_reaches(&ray(Vec3::new(-10.0, 0.0, 0.0)), bounds), "level with it");
        assert!(!ray_reaches(&ray(Vec3::new(-10.0, 9.0, 0.0)), bounds), "and nine units above it");
    }

    /// A mesh with no vertices has no extent to reject against, and nothing in
    /// it to hit either.
    #[test]
    fn a_mesh_with_no_vertices_is_not_rejected() {
        let empty = PickableMesh::new(Vec::new(), Vec::new());
        assert!(ray_reaches(&Ray { origin: Vec3::zeros(), direction: Vec3::new(0.0, 0.0, 1.0) }, empty.bounds));
    }

    /// The reject does not change what a cast finds: a ray at the cube still
    /// crosses its near face.
    #[test]
    fn the_reject_leaves_a_real_hit_alone() {
        let mesh = cube();
        // Off the shared diagonal, which both triangles lie along.
        let ray = Ray { origin: Vec3::new(0.5, -0.5, -10.0), direction: Vec3::new(0.0, 0.0, 1.0) };
        let hits = pick_all_ray(ray, &[PickTarget { id: MeshId(0), mesh: &mesh, visible: true }]);
        assert_eq!(hits.len(), 1, "one of the two triangles covers that corner");
    }
}
