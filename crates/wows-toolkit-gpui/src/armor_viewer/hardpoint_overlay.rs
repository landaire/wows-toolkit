//! Markers for a model's hardpoints: a diamond at each node, coloured by
//! kind, with a short axis triad showing which way the node faces. Drawn on
//! the overlay layer so a hardpoint inside a turret is not hidden by it.

use std::collections::HashSet;

use wowsunpack::export::hardpoint::Hardpoint;
use wowsunpack::export::hardpoint::HardpointKind;

use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;

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
    /// or ship draws nothing here.
    pub(crate) fn for_scene(scene: &LoadedShipArmor, targets: &HashSet<HardpointTarget>) -> Self {
        let mut markers = Self::default();
        let size = marker_size(scene.bounds);

        if let Some(model) = scene.model()
            && targets.contains(&HardpointTarget::Model(model.path.clone()))
        {
            markers.add(&model.hardpoints, size);
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

    /// Adds a marker and a label for each of `hardpoints`, `size` long.
    fn add(&mut self, hardpoints: &[Hardpoint], size: f32) {
        for hardpoint in hardpoints {
            let matrix = &hardpoint.transform;
            let origin = [matrix[12], matrix[13], matrix[14]];

            push_diamond(
                origin,
                size * DIAMOND_SCALE,
                kind_color(hardpoint.kind),
                &mut self.vertices,
                &mut self.indices,
            );
            for (axis, color) in AXIS_COLORS.iter().enumerate() {
                let column = [matrix[axis * 4], matrix[axis * 4 + 1], matrix[axis * 4 + 2]];
                push_needle(
                    origin,
                    normalized(column),
                    size,
                    size * NEEDLE_WIDTH,
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

/// The diamond's half-height, as a share of an axis needle's length.
const DIAMOND_SCALE: f32 = 0.18;

/// A needle's half-width, as a share of its length.
const NEEDLE_WIDTH: f32 = 0.03;

/// Local X, Y and Z, in the gizmo's colours.
const AXIS_COLORS: [[f32; 4]; 3] = [[0.95, 0.25, 0.25, 1.0], [0.3, 0.9, 0.3, 1.0], [0.3, 0.5, 1.0, 1.0]];

/// How long an axis needle is: a small share of the scene, held to a size
/// that stays visible on a gun and does not swamp a ship.
fn marker_size(bounds: (Vec3, Vec3)) -> f32 {
    let (min, max) = bounds;
    let extent = (max[0] - min[0]).max(max[1] - min[1]).max(max[2] - min[2]);

    (extent * 0.03).clamp(0.15, 2.5)
}

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
        vertices.push(Vertex { position, normal: offset, color, uv: [0.0, 0.0] });
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
            vertices.push(Vertex { position, normal: normalized(offset), color, uv: [0.0, 0.0] });
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

    use super::*;
    use crate::armor_viewer::load_model::fixtures;

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

        let markers = HardpointMarkers::for_scene(&scene, &targets);

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
            let markers = HardpointMarkers::for_scene(&scene, &targets);
            assert!(markers.indices.is_empty() && markers.labels.is_empty());
        }
    }
}
