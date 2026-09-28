//! Shell trajectories: the arc a shell rides in on, the plates it strikes,
//! and where its fuse goes off.
//!
//! The flight solver and the per-plate penetration chain are
//! `wowsunpack::ballistics`, which the egui viewer also casts through, so both
//! draw the same shell. What is here is the geometry around it: turning a
//! click and a camera ray into an approach, a ray cast into plate hits, and
//! the result into lines and markers the viewport can draw.

use crate::armor_viewer::load_ship::ArmorTriangleTooltip;
use crate::viewport::types::HitResult;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vec3;
use crate::viewport::types::Vertex;
use wowsunpack::ballistics::ArcProfile;
use wowsunpack::ballistics::FuseDetonation;
use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::PlateHit;
use wowsunpack::ballistics::PlateIndex;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::ShellSimResult;
use wowsunpack::ballistics::simulate_arc;
use wowsunpack::ballistics::simulate_shell_through_plates;
use wowsunpack::game_params::types::Degrees;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShipModelDistance;

/// How many points the drawn arc is sampled at. The egui viewer's own count,
/// so the two draw the same curve.
const ARC_POINT_COUNT: usize = 60;

/// The least the arc is allowed to rise, as a fraction of its drawn length.
/// A flat-fire shell would otherwise draw as a line lying on the water.
const MIN_ARC_HEIGHT_RATIO: f32 = 0.02;

/// Half-width of a drawn line, in model units.
const LINE_WIDTH: f32 = 0.02;

/// How far past the last plate the shell's path is drawn, in model units.
const TRAILING_LENGTH: f32 = 2.0;

/// Size of an impact marker, in model units.
const MARKER_SIZE: f32 = 0.06;

/// Size of a detonation burst, in model units.
const BURST_SIZE: f32 = 0.10;

/// Strike angles that read as shallow and as steep. Between them is the
/// middling band; the three colour the impact markers.
const SHALLOW_ANGLE: f32 = 30.0;
const STEEP_ANGLE: f32 = 45.0;

const IMPACT_COLOR_SHALLOW: [f32; 4] = [0.35, 0.85, 0.35, 1.0];
const IMPACT_COLOR_MEDIUM: [f32; 4] = [0.95, 0.80, 0.25, 1.0];
const IMPACT_COLOR_STEEP: [f32; 4] = [0.95, 0.35, 0.30, 1.0];

/// One plate a cast shell struck, in the order the shell reached them.
#[derive(Clone, Debug)]
pub struct TrajectoryHit {
    pub position: Vec3,
    pub thickness: Millimeters,
    pub zone: String,
    pub material: String,
    /// Strike angle from the plate normal: 0 is head-on, 90 is glancing,
    /// which is the convention the game states its ricochet angles in.
    pub angle_from_normal: Degrees,
    pub distance_from_start: ShipModelDistance,
}

/// A cast shell: what it flew in on, what it hit, and what became of it.
#[derive(Clone, Debug)]
pub struct Trajectory {
    /// The incoming arc, from out beyond the ship to the first plate.
    pub arc: Vec<Vec3>,
    pub hits: Vec<TrajectoryHit>,
    /// The direction the shell travels, normalized.
    pub shell_dir: Vec3,
    /// `None` when no shell was chosen to cast, in which case the hits still
    /// stand but nothing was simulated through them.
    pub sim: Option<ShellSimResult>,
    /// Where the fuse went off, when it did.
    pub detonation: Option<Vec3>,
    /// The range this arc was cast from, which sets how steeply it falls. Kept
    /// per arc so two can be compared at two ranges.
    pub range: Km,
    /// Whether this arc is hiding the plates it did not cross, and whether it is
    /// hiding the zones it did not enter. Both at once is the same as zones, so
    /// setting either one clears the other.
    pub isolating_plates: bool,
    pub isolating_zones: bool,
}

impl Trajectory {
    /// The last plate the shell reached, which is where the drawing stops:
    /// past it the shell is no longer travelling.
    pub fn last_plate_drawn(&self) -> Option<PlateIndex> {
        self.sim.as_ref().and_then(|sim| sim.last_reached_plate())
    }
}

/// The angle a ray strikes a plate at, measured from the plate's normal.
///
/// The normal's facing is not known to point at the shell, so the sign is
/// dropped: a plate hit from behind strikes at the same angle as one hit from
/// in front.
pub fn impact_angle_from_normal(ray_dir: &Vec3, normal: &Vec3) -> Degrees {
    let cosine = ray_dir.dot(normal).abs().min(1.0);
    Degrees::from(cosine.acos().to_degrees())
}

/// The horizontal direction a shell came in from.
///
/// A shell fired from directly overhead has no horizontal component to
/// recover, so one is chosen rather than normalizing a zero vector.
pub fn approach_xz(shell_dir: &Vec3) -> Vec3 {
    let flat = Vec3::new(shell_dir[0], 0.0, shell_dir[2]);
    let length = flat.norm();
    if length > 0.001 { flat / length } else { Vec3::x() }
}

/// Turns a ray cast through the armor into the plates it crossed.
///
/// A hit whose triangle carries no metadata is dropped: without a thickness
/// there is nothing to simulate against, and guessing one would report a
/// penetration the game never made.
pub fn build_hits(
    ray_hits: &[(HitResult, Vec3)],
    triangle_info: &[(MeshId, Vec<ArmorTriangleTooltip>)],
    shell_dir: &Vec3,
) -> Vec<TrajectoryHit> {
    let first_distance = ray_hits.first().map(|(hit, _)| hit.distance).unwrap_or(0.0);
    ray_hits
        .iter()
        .filter_map(|(hit, normal)| {
            let info = triangle_info
                .iter()
                .find(|(id, _)| *id == hit.mesh_id)
                .and_then(|(_, infos)| infos.get(hit.triangle_index))?;
            Some(TrajectoryHit {
                position: hit.world_position,
                thickness: Millimeters::from(info.thickness_mm),
                zone: info.zone.clone(),
                material: info.material_name.clone(),
                angle_from_normal: impact_angle_from_normal(shell_dir, normal),
                distance_from_start: ShipModelDistance::from(hit.distance - first_distance),
            })
        })
        .collect()
}

/// Runs a shell through the plates a cast crossed.
fn simulate(
    params: &ShellParams,
    impact: &ImpactResult,
    hits: &[TrajectoryHit],
    continue_on_ricochet: bool,
) -> ShellSimResult {
    let plates: Vec<PlateHit> = hits
        .iter()
        .map(|hit| PlateHit {
            thickness: hit.thickness,
            angle_from_normal: hit.angle_from_normal,
            distance_along_ray: hit.distance_from_start,
        })
        .collect();
    simulate_shell_through_plates(params, impact, &plates, continue_on_ricochet)
}

/// Where along the shell's path the fuse goes off.
///
/// `None` with no hits, which cannot happen for a real detonation: a fuse is
/// armed by a plate.
fn detonation_position(hits: &[TrajectoryHit], detonation: &FuseDetonation, shell_dir: &Vec3) -> Option<Vec3> {
    let first = hits.first()?;
    let direction = shell_dir / shell_dir.norm().max(1e-9);
    Some(first.position + direction * detonation.distance_along_ray.value())
}

/// The incoming arc, laid out so it ends at the first plate struck.
///
/// The arc is drawn to a length set by the ship rather than to the shell's
/// real flight distance: a shell fired from 15 km would otherwise put the
/// whole ship in one pixel.
pub fn build_arc(
    params: &ShellParams,
    impact: &ImpactResult,
    approach: Vec3,
    first_hit: Vec3,
    extent: f32,
) -> Vec<Vec3> {
    let length = extent * 2.0;
    let profile: ArcProfile = simulate_arc(params, impact.launch_angle, ARC_POINT_COUNT);
    let height = length * profile.height_ratio.max(MIN_ARC_HEIGHT_RATIO);
    profile
        .points
        .iter()
        .map(|point| {
            let back = (1.0 - point.along_range) * length;
            first_hit - approach * back + Vec3::new(0.0, point.height * height, 0.0)
        })
        .collect()
}

/// Casts a shell at the armor and reports where it goes.
///
/// `shell` is absent when nothing is in the comparison list, and the cast
/// still reports the plates the ray crossed; there is simply no flight to
/// draw and no penetration to resolve.
pub fn cast(
    hits: Vec<TrajectoryHit>,
    shell_dir: Vec3,
    shell: Option<(&ShellParams, &ImpactResult)>,
    extent: f32,
    continue_on_ricochet: bool,
    range: Km,
) -> Trajectory {
    let approach = approach_xz(&shell_dir);
    let (arc, sim, detonation) = match (shell, hits.first()) {
        (Some((params, impact)), Some(first)) => {
            let arc = build_arc(params, impact, approach, first.position, extent);
            let sim = simulate(params, impact, &hits, continue_on_ricochet);
            let detonation = sim.detonation.as_ref().and_then(|fuse| detonation_position(&hits, fuse, &shell_dir));
            (arc, Some(sim), detonation)
        }
        // With no shell or no hit there is no flight to draw. A short lead-in
        // still says which way the shell was going.
        (_, Some(first)) => (vec![first.position - approach * TRAILING_LENGTH, first.position], None, None),
        (_, None) => (Vec::new(), None, None),
    };
    Trajectory { arc, hits, shell_dir, sim, detonation, range, isolating_plates: false, isolating_zones: false }
}

/// The colour an impact marker takes, by how square the strike was.
fn impact_color(angle: Degrees) -> [f32; 4] {
    let degrees = angle.value();
    if degrees < SHALLOW_ANGLE {
        IMPACT_COLOR_SHALLOW
    } else if degrees < STEEP_ANGLE {
        IMPACT_COLOR_MEDIUM
    } else {
        IMPACT_COLOR_STEEP
    }
}

/// Builds the drawable geometry for a cast shell.
///
/// One mesh carries the whole thing: the incoming arc, a marker at each plate
/// reached, the path between them, and the detonation burst. `scale` follows
/// the camera so a marker stays readable as the view pulls back.
pub fn build_mesh(trajectory: &Trajectory, color: [f32; 4], scale: f32) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let width = LINE_WIDTH * scale;

    // The arc fades in along its length, so the eye follows it toward the
    // ship rather than reading it as a bar across the view.
    let spans = trajectory.arc.len().saturating_sub(1).max(1) as f32;
    let fade = |step: usize| {
        let along = step as f32 / spans;
        [color[0], color[1], color[2], color[3] * (0.7 + 0.3 * along)]
    };
    for (step, pair) in trajectory.arc.windows(2).enumerate() {
        push_segment(&mut vertices, &mut indices, pair[0], pair[1], width, fade(step), fade(step + 1));
    }

    let last = trajectory.last_plate_drawn();
    let drawn: usize = match last {
        Some(plate) => plate.value() + 1,
        // Nothing was simulated, so every plate the ray crossed is shown.
        None => trajectory.hits.len(),
    };

    for hit in trajectory.hits.iter().take(drawn) {
        push_marker(
            &mut vertices,
            &mut indices,
            hit.position,
            MARKER_SIZE * scale,
            impact_color(hit.angle_from_normal),
        );
    }

    for pair in trajectory.hits.iter().take(drawn).collect::<Vec<_>>().windows(2) {
        let through = [color[0], color[1], color[2], color[3] * 0.9];
        push_segment(&mut vertices, &mut indices, pair[0].position, pair[1].position, width, through, through);
    }

    // Past the last plate the shell is gone; the stub says which way.
    if let Some(exit) = trajectory.hits.get(drawn.saturating_sub(1)) {
        let direction = trajectory.shell_dir / trajectory.shell_dir.norm().max(1e-9);
        let faded = [color[0], color[1], color[2], color[3] * 0.3];
        push_segment(
            &mut vertices,
            &mut indices,
            exit.position,
            exit.position + direction * TRAILING_LENGTH,
            width,
            faded,
            faded,
        );
    }

    if let Some(burst) = trajectory.detonation {
        push_marker(&mut vertices, &mut indices, burst, BURST_SIZE * scale, color);
    }

    (vertices, indices)
}

/// A line drawn as two crossed quads, so it reads from any angle without a
/// geometry shader.
fn push_segment(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    from: Vec3,
    to: Vec3,
    width: f32,
    color_from: [f32; 4],
    color_to: [f32; 4],
) {
    let along = to - from;
    let length = along.norm();
    if length < 1e-9 {
        return;
    }
    let direction = along / length;
    // Any axis not parallel to the line will do to get a perpendicular from.
    let reference = if direction[1].abs() < 0.9 { Vec3::y() } else { Vec3::x() };
    let across = direction.cross(&reference).normalize();
    let up = direction.cross(&across).normalize();
    let normal: [f32; 3] = across.into();

    for perpendicular in [across, up] {
        let offset = perpendicular * (width * 0.5);
        let base = vertices.len() as u32;
        vertices.push(Vertex { position: (from - offset).into(), normal, color: color_from, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: (from + offset).into(), normal, color: color_from, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: (to + offset).into(), normal, color: color_to, uv: [0.0, 0.0] });
        vertices.push(Vertex { position: (to - offset).into(), normal, color: color_to, uv: [0.0, 0.0] });
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// An octahedron, which reads as a point from every angle.
fn push_marker(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, at: Vec3, size: f32, color: [f32; 4]) {
    let base = vertices.len() as u32;
    let corners = [
        at + Vec3::new(size, 0.0, 0.0),
        at + Vec3::new(-size, 0.0, 0.0),
        at + Vec3::new(0.0, size, 0.0),
        at + Vec3::new(0.0, -size, 0.0),
        at + Vec3::new(0.0, 0.0, size),
        at + Vec3::new(0.0, 0.0, -size),
    ];
    for corner in corners {
        let normal: [f32; 3] = (corner - at).normalize().into();
        vertices.push(Vertex { position: corner.into(), normal, color, uv: [0.0, 0.0] });
    }
    // Each face joins one x, one y and one z corner.
    for (x, y, z) in [(0u32, 2u32, 4u32), (0, 4, 3), (0, 3, 5), (0, 5, 2), (1, 4, 2), (1, 3, 4), (1, 5, 3), (1, 2, 5)] {
        indices.extend_from_slice(&[base + x, base + y, base + z]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(distance: f32, thickness: f32, angle: f32) -> TrajectoryHit {
        TrajectoryHit {
            position: Vec3::new(distance, 0.0, 0.0),
            thickness: Millimeters::from(thickness),
            zone: "hull".to_string(),
            material: "steel".to_string(),
            angle_from_normal: Degrees::from(angle),
            distance_from_start: ShipModelDistance::from(distance),
        }
    }

    #[test]
    fn a_head_on_strike_is_zero_from_the_normal_and_a_grazing_one_is_ninety() {
        let head_on = impact_angle_from_normal(&Vec3::new(1.0, 0.0, 0.0), &Vec3::new(-1.0, 0.0, 0.0));
        let grazing = impact_angle_from_normal(&Vec3::new(1.0, 0.0, 0.0), &Vec3::new(0.0, 1.0, 0.0));

        assert!(head_on.value().abs() < 1e-3, "head-on was {}", head_on.value());
        assert!((grazing.value() - 90.0).abs() < 1e-3, "grazing was {}", grazing.value());
    }

    #[test]
    fn the_normals_facing_does_not_change_the_strike_angle() {
        let ray = Vec3::new(1.0, 0.0, 0.0);
        let front = impact_angle_from_normal(&ray, &Vec3::new(-1.0, 0.0, 0.0));
        let back = impact_angle_from_normal(&ray, &Vec3::new(1.0, 0.0, 0.0));

        assert_eq!(front.value(), back.value());
    }

    #[test]
    fn a_shell_coming_straight_down_still_has_an_approach() {
        // Normalizing the horizontal part of a vertical shell would divide by
        // zero; one direction is chosen instead.
        let approach = approach_xz(&Vec3::new(0.0, -1.0, 0.0));

        assert!((approach.norm() - 1.0).abs() < 1e-6, "not a unit vector: {approach:?}");
        assert!(approach[1].abs() < 1e-6, "an approach is horizontal");
    }

    #[test]
    fn an_approach_drops_the_fall_and_keeps_the_bearing() {
        let approach = approach_xz(&Vec3::new(3.0, -4.0, 4.0));

        assert!(approach[1].abs() < 1e-6);
        assert!((approach.norm() - 1.0).abs() < 1e-6);
        // The bearing is unchanged: x and z stay in proportion.
        assert!((approach[0] / approach[2] - 3.0 / 4.0).abs() < 1e-5);
    }

    #[test]
    fn a_hit_on_a_triangle_with_no_metadata_is_dropped() {
        let mesh = MeshId(1);
        let ray_hits = vec![
            (HitResult { mesh_id: mesh, triangle_index: 0, distance: 1.0, world_position: Vec3::zeros() }, Vec3::x()),
            // Triangle 9 is past the end of the metadata, so nothing says how
            // thick it is.
            (HitResult { mesh_id: mesh, triangle_index: 9, distance: 2.0, world_position: Vec3::zeros() }, Vec3::x()),
        ];
        let info = vec![(
            mesh,
            vec![ArmorTriangleTooltip {
                material_name: "steel".into(),
                zone: "hull".into(),
                thickness_mm: 32.0,
                layers: vec![32.0],
                color: [0.0; 4],
            }],
        )];

        let built = build_hits(&ray_hits, &info, &Vec3::x());

        assert_eq!(built.len(), 1);
        assert_eq!(built[0].thickness.value(), 32.0);
    }

    #[test]
    fn hit_distances_are_measured_from_the_first_plate() {
        let mesh = MeshId(1);
        let info = vec![(
            mesh,
            vec![
                ArmorTriangleTooltip {
                    material_name: "steel".into(),
                    zone: "hull".into(),
                    thickness_mm: 32.0,
                    layers: vec![32.0],
                    color: [0.0; 4],
                };
                2
            ],
        )];
        let ray_hits = vec![
            (HitResult { mesh_id: mesh, triangle_index: 0, distance: 5.0, world_position: Vec3::zeros() }, Vec3::x()),
            (HitResult { mesh_id: mesh, triangle_index: 1, distance: 8.0, world_position: Vec3::zeros() }, Vec3::x()),
        ];

        let built = build_hits(&ray_hits, &info, &Vec3::x());

        // The ray starts well outside the ship, so a plate's distance along it
        // means nothing on its own; what matters is the spacing.
        assert_eq!(built[0].distance_from_start.value(), 0.0);
        assert_eq!(built[1].distance_from_start.value(), 3.0);
    }

    #[test]
    fn a_cast_with_no_shell_still_reports_the_plates_it_crossed() {
        let hits = vec![hit(0.0, 32.0, 10.0), hit(3.0, 19.0, 20.0)];

        let cast = cast(hits, Vec3::x(), None, 100.0, false, Km::new(10.0));

        assert_eq!(cast.hits.len(), 2);
        assert!(cast.sim.is_none(), "nothing was fired, so nothing was simulated");
        assert!(cast.detonation.is_none());
        // A lead-in still says which way the shell was going.
        assert_eq!(cast.arc.len(), 2);
    }

    #[test]
    fn a_cast_that_hit_nothing_draws_nothing() {
        let cast = cast(Vec::new(), Vec3::x(), None, 100.0, false, Km::new(10.0));

        assert!(cast.arc.is_empty());
        let (vertices, indices) = build_mesh(&cast, [1.0; 4], 1.0);
        assert!(vertices.is_empty());
        assert!(indices.is_empty());
    }

    #[test]
    fn every_index_the_mesh_emits_names_a_vertex_it_emitted() {
        let hits = vec![hit(0.0, 32.0, 10.0), hit(3.0, 19.0, 70.0)];
        let cast = cast(hits, Vec3::x(), None, 100.0, false, Km::new(10.0));

        let (vertices, indices) = build_mesh(&cast, [1.0, 0.5, 0.2, 1.0], 1.0);

        assert!(!indices.is_empty());
        assert!(indices.iter().all(|index| (*index as usize) < vertices.len()));
    }

    #[test]
    fn the_strike_angle_colours_the_marker() {
        assert_eq!(impact_color(Degrees::from(10.0)), IMPACT_COLOR_SHALLOW);
        assert_eq!(impact_color(Degrees::from(37.0)), IMPACT_COLOR_MEDIUM);
        assert_eq!(impact_color(Degrees::from(80.0)), IMPACT_COLOR_STEEP);
        // The bands meet at their own boundary rather than leaving a gap.
        assert_eq!(impact_color(Degrees::from(SHALLOW_ANGLE)), IMPACT_COLOR_MEDIUM);
        assert_eq!(impact_color(Degrees::from(STEEP_ANGLE)), IMPACT_COLOR_STEEP);
    }
}
