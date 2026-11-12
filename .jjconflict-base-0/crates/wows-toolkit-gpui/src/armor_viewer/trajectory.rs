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
use wowsunpack::ballistics::ArcProfile;
use wowsunpack::ballistics::FuseDetonation;
use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::PlateHit;
use wowsunpack::ballistics::PlateIndex;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::ShellSimResult;
use wowsunpack::ballistics::simulate_arc;
use wowsunpack::ballistics::simulate_shell_through_plates;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
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

/// How far past the last plate the shell's path is drawn, in model units.
const TRAILING_LENGTH: f32 = 2.0;

/// Strike angles that read as shallow and as steep. Between them is the
/// middling band; the three colour the impact markers.
const SHALLOW_ANGLE: f32 = 30.0;
const STEEP_ANGLE: f32 = 45.0;

const IMPACT_COLOR_SHALLOW: [f32; 4] = [0.35, 0.85, 0.35, 1.0];
const IMPACT_COLOR_MEDIUM: [f32; 4] = [0.95, 0.80, 0.25, 1.0];
const IMPACT_COLOR_STEEP: [f32; 4] = [0.95, 0.35, 0.30, 1.0];

/// The colours arcs are drawn in. Bright and saturated, so under a dark
/// outline each stands off both the thickness colours and the grey hull.
const ARC_PALETTE: [[f32; 4]; 8] = [
    [0.20, 0.80, 1.00, 1.0],
    [1.00, 0.55, 0.10, 1.0],
    [0.60, 1.00, 0.20, 1.0],
    [1.00, 0.30, 0.80, 1.0],
    [1.00, 0.92, 0.20, 1.0],
    [0.62, 0.45, 1.00, 1.0],
    [0.95, 0.95, 0.95, 1.0],
    [0.20, 1.00, 0.70, 1.0],
];

/// Names one cast arc for as long as it is on screen, whatever else is cast
/// or removed around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ArcId(u64);

impl ArcId {
    pub const FIRST: Self = Self(0);

    /// The id handed out after this one.
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Which palette colour an arc is drawn in. Kept with the arc, so removing
/// one does not recolour the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcColor(usize);

impl ArcColor {
    /// The first colour no arc in `taken` uses, or with every colour in use,
    /// the first of those fewest arcs use.
    pub fn unused(taken: impl IntoIterator<Item = ArcColor>) -> Self {
        let mut uses = [0usize; ARC_PALETTE.len()];
        for color in taken {
            uses[color.0] += 1;
        }

        let slot = (0..ARC_PALETTE.len()).min_by_key(|slot| uses[*slot]).expect("the palette is not empty");
        Self(slot)
    }

    pub fn rgba(self) -> [f32; 4] {
        ARC_PALETTE[self.0]
    }
}

/// One plate a cast shell struck, in the order the shell reached them.
#[derive(Clone, Debug)]
pub struct TrajectoryHit {
    pub position: Vec3,
    pub thickness: Millimeters,
    pub zone: String,
    pub material: ArmorMaterial,
    /// Strike angle from the plate normal: 0 is head-on, 90 is glancing,
    /// which is the convention the game states its ricochet angles in.
    pub angle_from_normal: Degrees,
    pub distance_from_start: ShipModelDistance,
    /// The plate's normal, which a new fall angle is struck against.
    pub normal: Vec3,
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
    wowsunpack::export::armor_ray::angle_from_normal((*ray_dir).into(), (*normal).into())
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

/// The same plates struck by a shell travelling along `shell_dir`.
///
/// The plates are the ones the first cast crossed rather than found again:
/// a ray cast now would miss every plate hidden since, including the ones
/// the arc's own isolation hid. A different fall changes how squarely each
/// is struck and how far apart they lie along the shell's path.
pub fn reaim(hits: Vec<TrajectoryHit>, shell_dir: &Vec3) -> Vec<TrajectoryHit> {
    let Some(first) = hits.first().map(|hit| hit.position) else { return hits };

    let direction = shell_dir / shell_dir.norm().max(1e-9);
    hits.into_iter()
        .map(|hit| TrajectoryHit {
            angle_from_normal: impact_angle_from_normal(&direction, &hit.normal),
            distance_from_start: ShipModelDistance::from((hit.position - first).dot(&direction)),
            ..hit
        })
        .collect()
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
                material: info.material,
                angle_from_normal: impact_angle_from_normal(shell_dir, normal),
                distance_from_start: ShipModelDistance::from(hit.distance - first_distance),
                normal: *normal,
            })
        })
        .collect()
}

/// Runs a shell through the plates a cast crossed.
pub(crate) fn simulate(
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
    simulate_shell_through_plates(params, impact.impact_velocity, &plates, continue_on_ricochet)
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

/// How a line of a cast shell is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stroke {
    /// The flight in and the path between the plates reached.
    Solid,
    /// Past the last plate reached, where the shell no longer travels.
    Faded,
}

/// One line of a cast shell, in world space.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcLine {
    pub points: Vec<Vec3>,
    pub stroke: Stroke,
}

/// A plate the shell reached, coloured by how square the strike was.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub position: Vec3,
    pub color: [f32; 4],
}

/// What is drawn for a cast shell. Drawn over the scene in screen space, so a
/// line keeps its width however far the camera pulls back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArcDrawing {
    pub lines: Vec<ArcLine>,
    pub impacts: Vec<Impact>,
    pub detonation: Option<Vec3>,
}

/// The lines and markers a cast shell is drawn with: the flight in, on
/// through every plate it reached, and a stub past the last one saying which
/// way it was going.
pub fn drawing(trajectory: &Trajectory) -> ArcDrawing {
    let drawn: usize = match trajectory.last_plate_drawn() {
        Some(plate) => plate.value() + 1,
        // Nothing was simulated, so every plate the ray crossed is shown.
        None => trajectory.hits.len(),
    };
    let reached = &trajectory.hits[..drawn.min(trajectory.hits.len())];

    // The arc ends on the first plate, so the path carries on from there.
    let path: Vec<Vec3> =
        trajectory.arc.iter().copied().chain(reached.iter().skip(1).map(|hit| hit.position)).collect();
    let mut lines = Vec::new();
    if path.len() >= 2 {
        lines.push(ArcLine { points: path, stroke: Stroke::Solid });
    }

    if let Some(exit) = reached.last() {
        let direction = trajectory.shell_dir / trajectory.shell_dir.norm().max(1e-9);
        lines.push(ArcLine {
            points: vec![exit.position, exit.position + direction * TRAILING_LENGTH],
            stroke: Stroke::Faded,
        });
    }

    let impacts = reached
        .iter()
        .map(|hit| Impact { position: hit.position, color: impact_color(hit.angle_from_normal) })
        .collect();

    ArcDrawing { lines, impacts, detonation: trajectory.detonation }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hull_belt() -> ArmorMaterial {
        ArmorMaterial::parse("Belt").expect("Belt is a table key")
    }

    fn hit(distance: f32, thickness: f32, angle: f32) -> TrajectoryHit {
        TrajectoryHit {
            position: Vec3::new(distance, 0.0, 0.0),
            thickness: Millimeters::from(thickness),
            zone: "hull".to_string(),
            material: hull_belt(),
            angle_from_normal: Degrees::from(angle),
            distance_from_start: ShipModelDistance::from(distance),
            normal: Vec3::y(),
        }
    }

    /// A deck is struck more squarely the steeper the shell falls, and the
    /// spacing between plates is measured along the new path.
    #[test]
    fn a_steeper_fall_strikes_a_deck_more_squarely() {
        let flat = vec![hit(0.0, 32.0, 90.0), hit(3.0, 19.0, 90.0)];
        let falling = Vec3::new(1.0, -1.0, 0.0).normalize();

        let steeper = reaim(flat, &falling);

        assert!((steeper[0].angle_from_normal.value() - 45.0).abs() < 1e-3);
        assert!((steeper[1].distance_from_start.value() - 3.0 / 2f32.sqrt()).abs() < 1e-4);
        assert_eq!(steeper[1].position, Vec3::new(3.0, 0.0, 0.0), "the plates stay where they were found");
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
                material: hull_belt(),
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
                    material: hull_belt(),
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

        assert_eq!(drawing(&cast), ArcDrawing::default());
    }

    /// The flight runs on through every plate reached as one line, so the
    /// shell reads as one path rather than a lead-in and loose segments.
    #[test]
    fn the_path_runs_from_the_flight_through_each_plate_and_fades_past_the_last() {
        let hits = vec![hit(0.0, 32.0, 10.0), hit(3.0, 19.0, 70.0)];
        let cast = cast(hits, Vec3::x(), None, 100.0, false, Km::new(10.0));

        let drawn = drawing(&cast);

        let solid = &drawn.lines[0];
        assert_eq!(solid.stroke, Stroke::Solid);
        assert_eq!(solid.points, vec![Vec3::new(-TRAILING_LENGTH, 0.0, 0.0), Vec3::zeros(), Vec3::new(3.0, 0.0, 0.0)]);
        let stub = &drawn.lines[1];
        assert_eq!(stub.stroke, Stroke::Faded);
        assert_eq!(stub.points, vec![Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0 + TRAILING_LENGTH, 0.0, 0.0)]);
        let colors: Vec<[f32; 4]> = drawn.impacts.iter().map(|impact| impact.color).collect();
        assert_eq!(colors, vec![IMPACT_COLOR_SHALLOW, IMPACT_COLOR_STEEP]);
    }

    /// Removing an arc frees its colour for the next one, and leaves the
    /// others drawn as they were.
    #[test]
    fn a_new_arc_takes_the_first_colour_no_arc_holds() {
        assert_eq!(ArcColor::unused([]), ArcColor(0));
        assert_eq!(ArcColor::unused([ArcColor(0), ArcColor(2)]), ArcColor(1));
    }

    #[test]
    fn with_every_colour_held_a_new_arc_takes_the_least_used() {
        let mut taken: Vec<ArcColor> = (0..ARC_PALETTE.len()).map(ArcColor).collect();
        taken.push(ArcColor(0));

        assert_eq!(ArcColor::unused(taken), ArcColor(1));
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
