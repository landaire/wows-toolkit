//! Markers for a model's hardpoints: a diamond at each node, coloured by
//! kind, with a short axis triad showing which way the node faces. Drawn on
//! the overlay layer so a hardpoint inside a turret is not hidden by it.

use std::collections::HashSet;

use wowsunpack::export::hardpoint::Hardpoint;
use wowsunpack::export::hardpoint::HardpointKind;

use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_armor::viewport::types::Vertex;

use super::load_ship::LoadedShipArmor;
use super::model_refs::HardpointTarget;

/// The markers of every hardpoint target that belongs to one scene.
#[derive(Default)]
pub(crate) struct HardpointMarkers {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) indices: Vec<u32>,
    pub(crate) labels: Vec<HardpointLabel>,
}

impl HardpointMarkers {
    /// The markers `targets` ask for on `scene`. A target for another model
    /// or ship draws nothing here. `world_per_pixel` is how much of the scene
    /// one pixel spans where the camera looks, which sets the smallest size
    /// a marker is drawn at.
    pub(crate) fn for_scene(scene: &LoadedShipArmor, targets: &HashSet<HardpointTarget>, world_per_pixel: f32) -> Self {
        let mut markers = Self::default();
        let size = MarkerSize::new(scene.bounds, world_per_pixel);

        // A part opened from its ship is marked by the part's own checkbox.
        if let Some(model) = scene.model() {
            let as_part = model.from_ship.as_ref().map(|from| HardpointTarget::ShipPart {
                vehicle_param_index: from.vehicle_param_index.clone(),
                part: from.part.clone(),
            });
            if targets.contains(&HardpointTarget::Model(model.path.clone()))
                || as_part.is_some_and(|target| targets.contains(&target))
            {
                markers.add(&model.hardpoints, size);
            }
        }
        // A ship part's hardpoints are already in ship space.
        if let Some(ship) = scene.ship() {
            for part in &ship.parts {
                let target = HardpointTarget::ShipPart {
                    vehicle_param_index: ship.vehicle_param_index.clone(),
                    part: part.key.clone(),
                };
                if targets.contains(&target) {
                    markers.add(&part.hardpoints, size);
                }
            }
        }

        markers
    }

    /// Adds a marker and a label for each of `hardpoints`.
    fn add(&mut self, hardpoints: &[Hardpoint], size: MarkerSize) {
        for hardpoint in hardpoints {
            let matrix = &hardpoint.transform;
            let origin = [matrix[12], matrix[13], matrix[14]];

            push_diamond(origin, size.diamond, kind_color(hardpoint.kind), &mut self.vertices, &mut self.indices);
            for (axis, color) in AXIS_COLORS.iter().enumerate() {
                let column = [matrix[axis * 4], matrix[axis * 4 + 1], matrix[axis * 4 + 2]];
                push_needle(
                    origin,
                    normalized(column),
                    size.needle,
                    size.needle_half_width,
                    *color,
                    &mut self.vertices,
                    &mut self.indices,
                );
            }

            self.labels.push(HardpointLabel { name: hardpoint.name.as_str().to_string(), position: origin });
        }
    }
}

/// One marked hardpoint's name and where it sits in model space, for the
/// label painted over the scene.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HardpointLabel {
    pub(crate) name: String,
    pub(crate) position: [f32; 3],
}

/// How large each piece of a marker is drawn, in scene units.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MarkerSize {
    /// The diamond's half-height.
    diamond: f32,
    /// An axis needle's length.
    needle: f32,
    needle_half_width: f32,
}

impl MarkerSize {
    /// A small share of the scene's bounding radius, so a marker reads as a
    /// point on the model rather than covering it, held to a few pixels so
    /// it stays visible on a scene drawn small.
    fn new(bounds: (Vec3, Vec3), world_per_pixel: f32) -> Self {
        const DIAMOND_SHARE: f32 = 0.012;
        const NEEDLE_SHARE: f32 = 0.05;
        const WIDTH_SHARE: f32 = 0.012;
        const DIAMOND_PIXELS: f32 = 3.0;
        const NEEDLE_PIXELS: f32 = 14.0;
        const HALF_WIDTH_PIXELS: f32 = 0.6;

        let (min, max) = bounds;
        let radius = (max - min).norm() * 0.5;
        let pixel = world_per_pixel.max(0.0);
        let needle = (radius * NEEDLE_SHARE).max(NEEDLE_PIXELS * pixel);

        Self {
            diamond: (radius * DIAMOND_SHARE).max(DIAMOND_PIXELS * pixel),
            needle,
            needle_half_width: (needle * WIDTH_SHARE).max(HALF_WIDTH_PIXELS * pixel),
        }
    }
}

/// Local X, Y and Z, in the gizmo's colours.
const AXIS_COLORS: [[f32; 4]; 3] = [[0.95, 0.25, 0.25, 1.0], [0.3, 0.9, 0.3, 1.0], [0.3, 0.5, 1.0, 1.0]];

fn kind_color(kind: HardpointKind) -> [f32; 4] {
    match kind {
        HardpointKind::Mount => [1.0, 0.75, 0.1, 1.0],
        HardpointKind::Misc => [0.2, 0.85, 1.0, 1.0],
        HardpointKind::Effect => [1.0, 0.35, 0.85, 1.0],
    }
}

/// An octahedron of half-size `radius` at `center`.
fn push_diamond(center: [f32; 3], radius: f32, color: [f32; 4], vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>) {
    const FACES: [[usize; 3]; 8] =
        [[0, 2, 4], [2, 1, 4], [1, 3, 4], [3, 0, 4], [2, 0, 5], [1, 2, 5], [3, 1, 5], [0, 3, 5]];

    let base = vertices.len() as u32;
    let offsets =
        [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0]];
    for offset in offsets {
        let position = [center[0] + offset[0] * radius, center[1] + offset[1] * radius, center[2] + offset[2] * radius];
        vertices.push(Vertex { position, normal: offset, color, uv: [0.0, 0.0], tangent: [0.0; 4] });
    }
    for face in FACES {
        indices.extend(face.iter().map(|&corner| base + corner as u32));
    }
}

/// A thin square prism from `origin` along `direction` for `length`.
fn push_needle(
    origin: [f32; 3],
    direction: [f32; 3],
    length: f32,
    half_width: f32,
    color: [f32; 4],
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    const SIDES: [[u32; 6]; 4] = [[0, 1, 5, 0, 5, 4], [1, 2, 6, 1, 6, 5], [2, 3, 7, 2, 7, 6], [3, 0, 4, 3, 4, 7]];

    let (side, up) = perpendiculars(direction);
    let corners = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)];
    let tip = [origin[0] + direction[0] * length, origin[1] + direction[1] * length, origin[2] + direction[2] * length];

    let base = vertices.len() as u32;
    for end in [origin, tip] {
        for (s, u) in corners {
            let offset = [
                (side[0] * s + up[0] * u) * half_width,
                (side[1] * s + up[1] * u) * half_width,
                (side[2] * s + up[2] * u) * half_width,
            ];
            let position = [end[0] + offset[0], end[1] + offset[1], end[2] + offset[2]];
            vertices.push(Vertex { position, normal: normalized(offset), color, uv: [0.0, 0.0], tangent: [0.0; 4] });
        }
    }
    for side in SIDES {
        indices.extend(side.iter().map(|corner| base + corner));
    }
}

/// Two unit vectors perpendicular to `direction` and to each other.
fn perpendiculars(direction: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let reference = if direction[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let side = normalized(cross(direction, reference));
    let up = cross(side, direction);

    (side, up)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `v` at unit length, or `v` unchanged when it has none to scale.
fn normalized(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length <= f32::EPSILON {
        return v;
    }

    [v[0] / length, v[1] / length, v[2] / length]
}

#[cfg(test)]
mod tests {
    use wowsunpack::export::hardpoint::HardpointName;
    use wowsunpack::export::model_catalog::ModelPath;
    use wowsunpack::export::ship_parts::ShipPartKey;

    use super::*;
    use crate::armor_viewer::load_model::fixtures;
    use crate::armor_viewer::load_ship::SceneSubject;
    use crate::armor_viewer::model_refs::ShipPartRef;
    use crate::armor_viewer::model_refs::VehicleParamIndex;

    fn hardpoint(name: &str, at: [f32; 3]) -> Hardpoint {
        let mut transform = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        transform[12..15].copy_from_slice(&at);
        Hardpoint { name: HardpointName::new(name), kind: HardpointKind::Mount, transform }
    }

    fn turret() -> ModelPath {
        ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path")
    }

    #[test]
    fn a_marked_model_gets_a_diamond_three_needles_and_a_label_per_hardpoint() {
        let scene =
            fixtures::model_scene(turret(), vec![hardpoint("HP_A", [1.0, 2.0, 3.0]), hardpoint("HP_B", [0.0; 3])]);
        let targets = HashSet::from([HardpointTarget::Model(turret())]);

        let markers = HardpointMarkers::for_scene(&scene, &targets, 0.0);

        assert_eq!(markers.vertices.len(), 2 * (6 + 3 * 8));
        assert_eq!(markers.indices.len(), 2 * (8 * 3 + 3 * 24));
        assert!(markers.indices.iter().all(|&index| (index as usize) < markers.vertices.len()));
        assert_eq!(
            markers.labels,
            vec![
                HardpointLabel { name: "HP_A".to_string(), position: [1.0, 2.0, 3.0] },
                HardpointLabel { name: "HP_B".to_string(), position: [0.0, 0.0, 0.0] },
            ]
        );
    }

    #[test]
    fn an_unmarked_or_other_model_draws_nothing() {
        let scene = fixtures::model_scene(turret(), vec![hardpoint("HP_A", [1.0, 2.0, 3.0])]);
        let other = ModelPath::parse("content/gameplay/usa/gun/main/AGM035/AGM035.model").expect("a model path");

        for targets in [HashSet::new(), HashSet::from([HardpointTarget::Model(other)])] {
            let markers = HardpointMarkers::for_scene(&scene, &targets, 0.0);
            assert!(markers.indices.is_empty() && markers.labels.is_empty());
        }
    }

    #[test]
    fn a_part_opened_from_its_ship_is_marked_by_the_parts_checkbox() {
        let mut scene = fixtures::model_scene(turret(), vec![hardpoint("HP_A", [1.0, 2.0, 3.0])]);
        let vehicle_param_index = VehicleParamIndex::new("PASC001".to_string());
        let part = ShipPartKey::Mount(HardpointName::new("HP_AGM_1"));
        if let SceneSubject::Model(model) = &mut scene.subject {
            model.from_ship =
                Some(ShipPartRef { vehicle_param_index: vehicle_param_index.clone(), part: part.clone() });
        }
        let targets = HashSet::from([HardpointTarget::ShipPart { vehicle_param_index, part }]);

        let markers = HardpointMarkers::for_scene(&scene, &targets, 0.0);

        assert_eq!(markers.labels.len(), 1);
    }

    /// A marker is a few percent of the scene across, whatever its size, so
    /// it neither covers a small model nor vanishes on a large one.
    #[test]
    fn markers_scale_with_the_scene() {
        for radius in [0.5, 5.0, 150.0] {
            let bounds = (Vec3::new(-radius, 0.0, 0.0), Vec3::new(radius, 0.0, 0.0));
            let size = MarkerSize::new(bounds, 0.0);

            assert!(size.diamond > 0.0 && size.diamond <= radius * 0.03, "diamond {size:?} at radius {radius}");
            assert!(size.needle <= radius * 0.1, "needle {size:?} at radius {radius}");
            assert!(size.needle_half_width <= size.needle * 0.02, "needle {size:?} is thin");
        }
    }

    /// On a scene drawn small, a marker is held to a few pixels.
    #[test]
    fn markers_keep_a_least_size_on_screen() {
        let bounds = (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let world_per_pixel = 0.1;
        let size = MarkerSize::new(bounds, world_per_pixel);

        assert!(size.diamond >= 2.0 * world_per_pixel, "{size:?}");
        assert!(size.needle >= 10.0 * world_per_pixel, "{size:?}");
    }
}
