//! Rays cast through a ship's armor meshes.
//!
//! Coordinates are the meshes' own: [`InteractiveArmorMesh`] space, which is
//! the model's space with Z negated (bow toward -Z, +Y up), in ship-model units.

use wows_core::units::Degrees;
use wows_core::units::ShipModelDistance;

use super::gltf_export::InteractiveArmorMesh;

/// A triangle a ray crossed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArmorRayHit {
    /// Index into the meshes the ray was cast through.
    pub mesh_index: usize,

    /// Index into that mesh's `triangle_info`.
    pub triangle_index: usize,

    /// Distance from the ray origin, along a unit direction.
    pub distance: ShipModelDistance,
    pub position: [f32; 3],

    /// Unit face normal, `(p1 - p0) x (p2 - p0)`, after the mesh transform.
    pub normal: [f32; 3],
}

/// Moller-Trumbore ray-triangle intersection, double-sided. The distance along
/// `direction` to the hit, when the triangle is ahead of `origin`.
pub fn ray_triangle_intersect(origin: [f32; 3], direction: [f32; 3], triangle: &[[f32; 3]; 3]) -> Option<f32> {
    const EPSILON: f32 = 1e-7;

    let [v0, v1, v2] = triangle;
    let edge1 = sub(*v1, *v0);
    let edge2 = sub(*v2, *v0);
    let h = cross(direction, edge2);
    let a = dot(edge1, h);
    if a.abs() < EPSILON {
        return None;
    }

    let f = 1.0 / a;
    let s = sub(origin, *v0);
    let u = f * dot(s, h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = cross(s, edge1);
    let v = f * dot(direction, q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * dot(edge2, q);
    if t > EPSILON { Some(t) } else { None }
}

/// Every triangle of `meshes` the ray crosses, nearest first. Each mesh's
/// transform is applied to its triangles. `direction` must be a unit vector.
pub fn cast_ray(meshes: &[InteractiveArmorMesh], origin: [f32; 3], direction: [f32; 3]) -> Vec<ArmorRayHit> {
    let mut hits = Vec::new();

    for (mesh_index, mesh) in meshes.iter().enumerate() {
        for triangle_index in 0..mesh.triangle_info.len() {
            let Some(triangle) = mesh_triangle(mesh, triangle_index) else {
                continue;
            };
            let Some(distance) = ray_triangle_intersect(origin, direction, &triangle) else {
                continue;
            };

            let [p0, p1, p2] = triangle;
            hits.push(ArmorRayHit {
                mesh_index,
                triangle_index,
                distance: ShipModelDistance::new(distance),
                position: add(origin, scale(direction, distance)),
                normal: normalize(cross(sub(p1, p0), sub(p2, p0))),
            });
        }
    }

    hits.sort_by(|a, b| a.distance.value().total_cmp(&b.distance.value()));

    hits
}

/// The corners of one triangle of `mesh`, with the mesh transform applied.
pub fn mesh_triangle(mesh: &InteractiveArmorMesh, triangle_index: usize) -> Option<[[f32; 3]; 3]> {
    let indices = mesh.indices.get(triangle_index * 3..triangle_index * 3 + 3)?;
    let mut corners = [[0.0; 3]; 3];
    for (corner, &index) in corners.iter_mut().zip(indices) {
        let position = *mesh.positions.get(index as usize)?;
        *corner = match &mesh.transform {
            Some(transform) => transform_point(transform, position),
            None => position,
        };
    }

    Some(corners)
}

/// Strike angle of a ray on a plate, from the plate normal: 0 is head-on, 90
/// is glancing. The game's ricochet angles are stated in this convention.
pub fn angle_from_normal(direction: [f32; 3], normal: [f32; 3]) -> Degrees {
    let cos_angle = dot(direction, normal).abs().min(1.0);

    Degrees::from(cos_angle.acos().to_degrees())
}

/// Applies a column-major 4x4 transform to a point.
pub fn transform_point(transform: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
    let t = transform;

    [
        t[0] * point[0] + t[4] * point[1] + t[8] * point[2] + t[12],
        t[1] * point[0] + t[5] * point[1] + t[9] * point[2] + t[13],
        t[2] * point[0] + t[6] * point[1] + t[10] * point[2] + t[14],
    ]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], factor: f32) -> [f32; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let length = dot(a, a).sqrt();
    if length == 0.0 { a } else { scale(a, 1.0 / length) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate(z: f32) -> [[f32; 3]; 3] {
        [[-1.0, -1.0, z], [1.0, -1.0, z], [0.0, 1.0, z]]
    }

    fn mesh(triangles: &[[[f32; 3]; 3]], transform: Option<[f32; 16]>) -> InteractiveArmorMesh {
        let model = crate::models::geometry::ArmorModel {
            name: "test".to_owned(),
            triangles: triangles
                .iter()
                .map(|vertices| crate::models::geometry::ArmorTriangle {
                    // from_armor_model negates Z; pre-negate so the mesh holds `vertices`.
                    vertices: vertices.map(|[x, y, z]| [x, y, -z]),
                    normals: [[0.0, 0.0, 1.0]; 3],
                    material_id: 1,
                    layer_index: 1,
                })
                .collect(),
        };
        let mut mesh = InteractiveArmorMesh::from_armor_model(&model, None, None);
        mesh.transform = transform;
        mesh
    }

    #[test]
    fn hits_are_ordered_by_distance_and_carry_the_face_normal() {
        let meshes = [mesh(&[plate(5.0), plate(2.0)], None)];

        let hits = cast_ray(&meshes, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

        let distances: Vec<f32> = hits.iter().map(|hit| hit.distance.value()).collect();
        assert_eq!(distances, [2.0, 5.0]);
        assert_eq!(hits[0].triangle_index, 1);
        assert_eq!(hits[0].position, [0.0, 0.0, 2.0]);
        assert_eq!(hits[0].normal, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_mesh_transform_moves_its_triangles() {
        let mut translate = [0.0; 16];
        for diagonal in [0, 5, 10, 15] {
            translate[diagonal] = 1.0;
        }
        translate[14] = 3.0;
        let meshes = [mesh(&[plate(0.0)], Some(translate))];

        let hits = cast_ray(&meshes, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].distance.value(), 3.0);
    }

    #[test]
    fn transform_point_applies_translation() {
        #[rustfmt::skip]
        let translated: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            5.0, 6.0, 7.0, 1.0,
        ];

        assert_eq!(transform_point(&translated, [1.0, 2.0, 3.0]), [6.0, 8.0, 10.0]);
    }

    #[test]
    fn a_triangle_behind_the_origin_or_beside_the_ray_is_missed() {
        assert_eq!(ray_triangle_intersect([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], &plate(-1.0)), None);
        assert_eq!(ray_triangle_intersect([5.0, 0.0, 0.0], [0.0, 0.0, 1.0], &plate(1.0)), None);
    }

    #[test]
    fn strike_angle_is_measured_from_the_normal_on_either_face() {
        let diagonal = normalize([1.0, 0.0, 1.0]);

        assert_eq!(angle_from_normal([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]), Degrees::from(0.0));
        assert!((angle_from_normal(diagonal, [0.0, 0.0, 1.0]).value() - 45.0).abs() < 1e-4);
    }
}
