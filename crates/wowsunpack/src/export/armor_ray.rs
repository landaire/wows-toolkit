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

/// A point on the plane perpendicular to a [`ParallelRays`] direction, which
/// names one ray of the set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayOffset {
    /// Along the axis perpendicular to both the direction and up.
    pub across: ShipModelDistance,

    /// Along the plane's axis closest to world up.
    pub up: ShipModelDistance,
}

/// The rectangle of [`RayOffset`]s whose rays can strike the meshes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayFootprint {
    pub min: RayOffset,
    pub max: RayOffset,
}

/// Rays sharing one direction, cast through a fixed set of meshes.
///
/// Triangles are binned by their projection onto the plane perpendicular to
/// the direction, so a ray tests only the triangles whose projection covers
/// its bin. The hits are the ones [`cast_ray`] returns for the same ray.
pub struct ParallelRays {
    direction: [f32; 3],
    across_axis: [f32; 3],
    up_axis: [f32; 3],

    /// Position along `direction` of every ray origin, ahead of every triangle.
    start: ShipModelDistance,

    footprint: RayFootprint,
    bin_size: ShipModelDistance,
    columns: usize,
    rows: usize,

    /// Per bin, row-major, indices into `triangles`.
    bins: Vec<Vec<usize>>,
    triangles: Vec<PlacedTriangle>,
}

impl ParallelRays {
    /// Bins the triangles of `meshes` for rays along `direction`, which need
    /// not be a unit vector.
    pub fn new(meshes: &[InteractiveArmorMesh], direction: [f32; 3]) -> Self {
        const BINS_ALONG_LONGER_SIDE: f32 = 256.0;
        // Origins sit this far ahead of the nearest triangle, in model units.
        const START_CLEARANCE: f32 = 1.0;

        let direction = normalize(direction);
        let in_plane = |axis: [f32; 3]| sub(axis, scale(direction, dot(axis, direction)));
        let up_in_plane = in_plane([0.0, 1.0, 0.0]);
        // A near-vertical ray has no usable up in its plane; the bow axis stands in.
        let up_axis = if dot(up_in_plane, up_in_plane) < 1e-6 {
            normalize(in_plane([0.0, 0.0, -1.0]))
        } else {
            normalize(up_in_plane)
        };
        let across_axis = cross(up_axis, direction);

        let mut triangles = Vec::new();
        for (mesh_index, mesh) in meshes.iter().enumerate() {
            for triangle_index in 0..mesh.triangle_info.len() {
                if let Some(corners) = mesh_triangle(mesh, triangle_index) {
                    triangles.push(PlacedTriangle { corners, mesh_index, triangle_index });
                }
            }
        }

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for corner in triangles.iter().flat_map(|triangle| triangle.corners) {
            let projected = [dot(corner, across_axis), dot(corner, up_axis), dot(corner, direction)];
            for axis in 0..3 {
                min[axis] = min[axis].min(projected[axis]);
                max[axis] = max[axis].max(projected[axis]);
            }
        }

        let footprint = RayFootprint {
            min: RayOffset { across: ShipModelDistance::new(min[0]), up: ShipModelDistance::new(min[1]) },
            max: RayOffset { across: ShipModelDistance::new(max[0]), up: ShipModelDistance::new(max[1]) },
        };
        let mut rays = Self {
            direction,
            across_axis,
            up_axis,
            start: ShipModelDistance::new(min[2] - START_CLEARANCE),
            footprint,
            bin_size: ShipModelDistance::new(1.0),
            columns: 0,
            rows: 0,
            bins: Vec::new(),
            triangles: Vec::new(),
        };
        if triangles.is_empty() {
            return rays;
        }

        let longer_side = (max[0] - min[0]).max(max[1] - min[1]);
        rays.bin_size = ShipModelDistance::new((longer_side / BINS_ALONG_LONGER_SIDE).max(f32::EPSILON));
        rays.columns = rays.bin_of(max[0] - min[0]) + 1;
        rays.rows = rays.bin_of(max[1] - min[1]) + 1;
        rays.bins = vec![Vec::new(); rays.columns * rays.rows];

        for (index, triangle) in triangles.iter().enumerate() {
            let across = triangle.corners.map(|corner| dot(corner, across_axis) - min[0]);
            let up = triangle.corners.map(|corner| dot(corner, up_axis) - min[1]);
            let first_column = rays.bin_of(across.into_iter().fold(f32::INFINITY, f32::min));
            let last_column = rays.bin_of(across.into_iter().fold(f32::NEG_INFINITY, f32::max)).min(rays.columns - 1);
            let first_row = rays.bin_of(up.into_iter().fold(f32::INFINITY, f32::min));
            let last_row = rays.bin_of(up.into_iter().fold(f32::NEG_INFINITY, f32::max)).min(rays.rows - 1);

            for row in first_row..=last_row {
                for column in first_column..=last_column {
                    rays.bins[row * rays.columns + column].push(index);
                }
            }
        }
        rays.triangles = triangles;

        rays
    }

    /// The unit direction every ray travels.
    pub fn direction(&self) -> [f32; 3] {
        self.direction
    }

    pub fn footprint(&self) -> RayFootprint {
        self.footprint
    }

    /// Where the ray at `offset` starts, ahead of every triangle.
    pub fn origin(&self, offset: RayOffset) -> [f32; 3] {
        add(
            add(scale(self.across_axis, offset.across.value()), scale(self.up_axis, offset.up.value())),
            scale(self.direction, self.start.value()),
        )
    }

    /// Every triangle the ray at `offset` crosses, nearest first, with
    /// distances measured from [`Self::origin`].
    pub fn cast(&self, offset: RayOffset) -> Vec<ArmorRayHit> {
        let across = offset.across.value() - self.footprint.min.across.value();
        let up = offset.up.value() - self.footprint.min.up.value();
        if across < 0.0 || up < 0.0 {
            return Vec::new();
        }
        let (column, row) = (self.bin_of(across), self.bin_of(up));
        if column >= self.columns || row >= self.rows {
            return Vec::new();
        }

        let origin = self.origin(offset);
        let mut hits: Vec<ArmorRayHit> = self.bins[row * self.columns + column]
            .iter()
            .filter_map(|&index| {
                let triangle = &self.triangles[index];
                let distance = ray_triangle_intersect(origin, self.direction, &triangle.corners)?;
                let [p0, p1, p2] = triangle.corners;

                Some(ArmorRayHit {
                    mesh_index: triangle.mesh_index,
                    triangle_index: triangle.triangle_index,
                    distance: ShipModelDistance::new(distance),
                    position: add(origin, scale(self.direction, distance)),
                    normal: normalize(cross(sub(p1, p0), sub(p2, p0))),
                })
            })
            .collect();
        hits.sort_by(|a, b| a.distance.value().total_cmp(&b.distance.value()));

        hits
    }

    fn bin_of(&self, offset: f32) -> usize {
        (offset / self.bin_size.value()) as usize
    }
}

/// A triangle with its mesh transform applied.
struct PlacedTriangle {
    corners: [[f32; 3]; 3],
    mesh_index: usize,
    triangle_index: usize,
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
    fn parallel_rays_find_the_hits_a_single_cast_finds() {
        let tilted = [[-2.0, -1.0, 4.0], [3.0, -1.5, 6.0], [0.5, 2.0, 5.0]];
        let meshes = [mesh(&[plate(5.0), plate(2.0), tilted], None), mesh(&[plate(8.0)], None)];
        let rays = ParallelRays::new(&meshes, [0.3, -0.2, 1.0]);
        let direction = rays.direction();
        let footprint = rays.footprint();

        let mut compared = 0;
        for step_across in 0..=20 {
            for step_up in 0..=20 {
                let fraction = |step: i32| step as f32 / 20.0;
                let lerp = |min: ShipModelDistance, max: ShipModelDistance, step: i32| {
                    ShipModelDistance::new(min.value() + (max.value() - min.value()) * fraction(step))
                };
                let offset = RayOffset {
                    across: lerp(footprint.min.across, footprint.max.across, step_across),
                    up: lerp(footprint.min.up, footprint.max.up, step_up),
                };

                let parallel = rays.cast(offset);
                let single = cast_ray(&meshes, rays.origin(offset), direction);

                assert_eq!(parallel, single, "ray at {offset:?}");
                compared += parallel.len();
            }
        }
        assert!(compared > 100, "the grid crossed only {compared} triangles");
    }

    /// A ray less than 0.057 degrees from vertical bins on the bow axis. A
    /// small deck far along the ray must still sit in the bin it is cast through.
    #[test]
    fn near_vertical_parallel_rays_find_the_hits_a_single_cast_finds() {
        let deck = |height: f32, half: f32| [[-half, height, -half], [half, height, -half], [0.0, height, half]];
        let meshes = [mesh(&[deck(0.0, 3.0), deck(-2000.0, 0.5)], None)];
        let rays = ParallelRays::new(&meshes, [0.0, -1.0, 5e-4]);
        let direction = rays.direction();
        let footprint = rays.footprint();

        let mut deep_hits = 0;
        for step_across in 0..=40 {
            for step_up in 0..=40 {
                let lerp = |min: ShipModelDistance, max: ShipModelDistance, step: i32| {
                    ShipModelDistance::new(min.value() + (max.value() - min.value()) * step as f32 / 40.0)
                };
                let offset = RayOffset {
                    across: lerp(footprint.min.across, footprint.max.across, step_across),
                    up: lerp(footprint.min.up, footprint.max.up, step_up),
                };

                let parallel = rays.cast(offset);
                let single = cast_ray(&meshes, rays.origin(offset), direction);

                assert_eq!(parallel, single, "ray at {offset:?}");
                deep_hits += parallel.iter().filter(|hit| hit.triangle_index == 1).count();
            }
        }
        assert!(deep_hits > 10, "the grid crossed the deep deck only {deep_hits} times");
    }

    #[test]
    fn parallel_rays_through_an_empty_mesh_hit_nothing() {
        let meshes = [mesh(&[], None)];
        let rays = ParallelRays::new(&meshes, [0.0, 0.0, 1.0]);
        let offset = RayOffset { across: ShipModelDistance::new(0.0), up: ShipModelDistance::new(0.0) };

        assert!(rays.cast(offset).is_empty());
        assert!(ParallelRays::new(&[], [0.0, 0.0, 1.0]).cast(offset).is_empty());
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
