//! Where on a ship's hull a shell landed.
//!
//! A hit is recorded in world space, and an armor viewer draws it on a model
//! in the model's own frame, so placing one means undoing the ship's
//! rotation and then changing axes. Both of those depend on conventions that
//! are easy to get backwards and hard to notice when they are: a sign error
//! puts the hit on the other side of the ship, where it still lands on real
//! plating and still looks like a measurement. So the conversion is here,
//! once, rather than in each viewer.

use nalgebra::Rotation3;
use nalgebra::Vector3;
use wows_core::units::Meters;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::analyzer::battle_controller::state::VictimPose;
use wowsunpack::game_types::ShellHitType;
use wowsunpack::game_types::WorldPos;
use wowsunpack::recognized::Recognized;

use crate::timeline::PreExtractedHit;

type Vec3 = Vector3<f32>;

/// Undoes a ship's own rotation, taking a world offset into the hull's frame.
///
/// BigWorld has yaw zero facing east (+X) and growing anticlockwise, so the
/// forward rotation is `Ry(-yaw)` and undoing it is `Ry(+yaw)`.
/// Public because a caller placing a direction rather than a point needs the
/// same rotation, and building a second one is how the two drift apart.
pub fn into_hull_frame(yaw: f32, pitch: f32, roll: f32) -> Rotation3<f32> {
    let yaw = Rotation3::from_axis_angle(&Vec3::y_axis(), yaw);
    let pitch = Rotation3::from_axis_angle(&Vec3::x_axis(), pitch);
    let roll = Rotation3::from_axis_angle(&Vec3::z_axis(), roll);
    roll * pitch * yaw
}

/// From the hull's frame to the ship-model axes GameParams states its own
/// geometry in.
///
/// In the hull's frame the bow is +X and up is +Y; in that ship-model space the
/// bow is +Z, starboard +X and up +Y. This quarter turn maps the one onto the
/// other.
///
/// Whatever is taken through this -- a point or a direction -- still needs its
/// Z negated to reach the space the exported meshes are in; see
/// [`into_mesh_space`].
pub fn into_model_axes() -> Rotation3<f32> {
    Rotation3::from_axis_angle(&Vec3::y_axis(), -std::f32::consts::FRAC_PI_2)
}

/// Into the space the exported armor and hull meshes are in.
///
/// `InteractiveArmorMesh::from_armor_model` negates Z on every vertex to leave
/// BigWorld's left-handed space for a right-handed one, so the bow sits at -Z in
/// a mesh while GameParams' own ship-model space puts it at +Z. Measured on
/// Iowa's splash boxes, whose names say which end they are: the bow boxes run
/// -8.9 to -4.7 and the stern boxes +4.7 to +8.8. A point or a direction drawn
/// against those meshes has to follow, or it lands on the other end of the ship.
pub fn into_mesh_space(in_model_axes: Vec3) -> Vec3 {
    Vec3::new(in_model_axes.x, in_model_axes.y, -in_model_axes.z)
}

/// Where `hit` landed on its victim's model, in the space the meshes are in.
///
/// `model_center` is where the hull sits in that frame, and `bounds`, when
/// given, is the hull's extent: an impact is held within it along the two
/// horizontal axes, because a shell that struck a part the model does not
/// carry would otherwise be drawn out in open water. Height is left alone,
/// since a hit above the deck is a real reading.
///
/// `None` when the victim's pose was not held at that moment -- it had left
/// the client's view, or was never resolved to a live entity. Refused rather
/// than guessed from the ship's last known position: an offset measured from
/// a guess lands on a real hull section and cannot be told from a measured
/// one.
pub fn hull_impact(
    hit: &ResolvedShotHit,
    model_center: [f32; 3],
    bounds: Option<([f32; 3], [f32; 3])>,
) -> Option<[f32; 3]> {
    place(&hit.hit.position, hit.victim_pose.as_ref(), model_center, bounds)
}

/// The same for an impact and a pose held separately, which is what a caller
/// with its own bookkeeping has.
///
/// `None` without a pose, for the reason [`hull_impact`] gives.
pub fn place(
    impact: &WorldPos,
    pose: Option<&VictimPose>,
    model_center: [f32; 3],
    bounds: Option<([f32; 3], [f32; 3])>,
) -> Option<[f32; 3]> {
    let pose = pose?;
    let offset = Vec3::new(impact.x - pose.position.x, impact.y - pose.position.y, impact.z - pose.position.z);
    let in_model = into_mesh_space(into_model_axes() * (into_hull_frame(pose.yaw, pose.pitch, pose.roll) * offset));
    let center = Vec3::new(model_center[0], model_center[1], model_center[2]);
    let at = center + in_model;
    Some(match bounds {
        Some((low, high)) => [at.x.clamp(low[0], high[0]), at.y, at.z.clamp(low[2], high[2])],
        None => [at.x, at.y, at.z],
    })
}

/// How far the shell flew, and which way it was going when it arrived.
///
/// The two things a simulation of that shell through the hull needs, and both
/// are read off the salvo it belongs to rather than off the impact: the
/// terminal-ballistics velocity a hit carries is post-impact, so it says where
/// the shell went afterwards and not where it came from.
///
/// `None` for a hit with no salvo behind it, one whose own shot the salvo does
/// not list, or one whose victim's pose was not held: each leaves the arrival
/// unknowable rather than approximate.
pub fn shell_arrival(hit: &ResolvedShotHit) -> Option<ShellArrival> {
    let pose = hit.victim_pose.as_ref()?;
    let salvo = hit.salvo.as_ref()?;
    let shot = salvo.shots.iter().find(|shot| shot.shot_id == hit.hit.shot_id)?;

    let impact = &hit.hit.position;
    let travelled = shot.origin.distance_xz(impact);
    let world = Vec3::new(impact.x - shot.origin.x, impact.y - shot.origin.y, impact.z - shot.origin.z);
    let in_model = into_mesh_space(into_model_axes() * (into_hull_frame(pose.yaw, pose.pitch, pose.roll) * world));

    let flat = Vec3::new(in_model.x, 0.0, in_model.z);
    let length = flat.norm();
    // A shell that came straight down has no bearing to read, and the fall
    // angle alone does not say which way it was going.
    if length < 1e-3 {
        return None;
    }
    Some(ShellArrival { travelled, bearing: flat / length })
}

/// Where a shell came from, in the victim model's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellArrival {
    /// How far it flew, which is what its arrival speed and angle are solved
    /// from.
    ///
    /// Read by `Vec3::distance_xz`, at 30 m to the world unit. Which scale a
    /// separation between two distant points is really in is open: `wows_core`'s
    /// `WorldDistance` measures the same coordinates at 15 for an offset against
    /// a hull, and says the two are unreconciled. This follows the firing range
    /// the ballistics solver is already run on, so a shell reads the same here
    /// as it does in the egui viewer.
    pub travelled: Meters,
    /// The way it was going, flattened and normalised: the fall angle belongs
    /// to the solve rather than to the record.
    pub bearing: Vec3,
}

/// One line of a shot log: when a shell landed and what it did.
#[derive(Clone, Debug, PartialEq)]
pub struct LoggedHit {
    /// The battle clock it landed at, as "MM:SS".
    pub at: String,
    /// What the shell did, worded the way both viewers word it.
    pub outcome: &'static str,
}

/// What `hit` reads as in a shot log.
///
/// Worded here so a log in one viewer cannot call a shatter something else
/// than a log in the other. An outcome the build does not name reads as
/// unknown rather than being left out: a hit that happened is worth a line
/// whether or not this build has a word for it.
pub fn log_line(hit: &PreExtractedHit) -> LoggedHit {
    LoggedHit { at: mmss(hit.clock.seconds()), outcome: outcome_of(hit) }
}

fn outcome_of(hit: &PreExtractedHit) -> &'static str {
    match hit.hit.hit.hit_type.shell_hit {
        Recognized::Known(ShellHitType::Normal) => "Penetration",
        Recognized::Known(ShellHitType::MajorHit) => "Citadel",
        Recognized::Known(ShellHitType::Ricochet) => "Ricochet",
        Recognized::Known(ShellHitType::NoPenetration) => "Shatter",
        Recognized::Known(ShellHitType::Overpenetration) => "Overpenetration",
        Recognized::Known(ShellHitType::ExitOverpenetration) => "Exit",
        Recognized::Known(ShellHitType::Underwater) => "Underwater",
        Recognized::Known(ShellHitType::None) | Recognized::Unknown(_) => "Unknown",
    }
}

/// A battle clock as the game shows it. Before the battle began reads as its
/// start rather than as a minus sign.
fn mmss(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wows_replays::types::EntityId;
    use wows_replays::types::GameClock;
    use wows_replays::types::GameParamId;
    use wows_replays::types::ShotId;

    const NEAR: f32 = 1e-3;

    fn pose(yaw: f32) -> VictimPose {
        VictimPose { position: WorldPos::new(100.0, 0.0, 200.0), yaw, pitch: 0.0, roll: 0.0 }
    }

    fn world(x: f32, y: f32, z: f32) -> WorldPos {
        WorldPos::new(x, y, z)
    }

    fn placed(impact: WorldPos, pose: &VictimPose, center: [f32; 3]) -> [f32; 3] {
        place(&impact, Some(pose), center, None).expect("a pose was given")
    }

    /// A ship facing east takes a hit ahead of it on its bow, which is -Z in
    /// the mesh the viewers draw.
    #[test]
    fn a_hit_ahead_of_the_bow_lands_on_the_bow() {
        let ahead = world(150.0, 0.0, 200.0);
        let at = placed(ahead, &pose(0.0), [0.0, 0.0, 0.0]);

        assert!((at[2] + 50.0).abs() < NEAR, "fifty units up the bow: {at:?}");
        assert!(at[0].abs() < NEAR, "and nothing to either side");
    }

    /// Turn the ship a quarter and the same patch of water is off its side
    /// instead.
    #[test]
    fn the_same_water_is_abeam_once_the_ship_has_turned() {
        let ahead = world(150.0, 0.0, 200.0);
        let at = placed(ahead, &pose(std::f32::consts::FRAC_PI_2), [0.0, 0.0, 0.0]);

        assert!(at[2].abs() < NEAR, "no longer ahead: {at:?}");
        assert!((at[0].abs() - 50.0).abs() < NEAR, "but fifty metres abeam");
    }

    /// Height survives the conversion, so a hit above the deck reads as one.
    #[test]
    fn a_hit_above_the_deck_stays_above_it() {
        let high = world(100.0, 12.0, 200.0);
        let at = placed(high, &pose(0.7), [0.0, 0.0, 0.0]);

        assert!((at[1] - 12.0).abs() < NEAR, "{at:?}");
    }

    /// The model's own centre moves the whole placement with it.
    #[test]
    fn the_models_centre_carries_the_placement() {
        let ahead = world(150.0, 0.0, 200.0);
        let at = placed(ahead, &pose(0.0), [1.0, 2.0, 3.0]);

        assert!((at[0] - 1.0).abs() < NEAR && (at[1] - 2.0).abs() < NEAR && (at[2] + 47.0).abs() < NEAR, "{at:?}");
    }

    /// A hit beyond the hull is held at its edge, so a shell that struck
    /// something the model does not carry is not drawn out in open water.
    /// Height is left alone, since above the deck is a real reading.
    #[test]
    fn a_hit_past_the_hull_is_held_at_its_edge() {
        let far = world(400.0, 30.0, 200.0);
        let bounds = Some(([-10.0, -5.0, -100.0], [10.0, 20.0, 100.0]));
        let at = place(&far, Some(&pose(0.0)), [0.0, 0.0, 0.0], bounds).expect("a pose was given");

        assert!((at[2] + 100.0).abs() < NEAR, "held at the bow: {at:?}");
        assert!((at[1] - 30.0).abs() < NEAR, "but its height is left alone");
    }

    /// A clock reads as the game shows it, and before the battle began reads
    /// as its start rather than as a minus sign.
    #[test]
    fn a_log_line_reads_the_clock_the_game_shows() {
        assert_eq!(mmss(0.0), "00:00");
        assert_eq!(mmss(9.7), "00:09");
        assert_eq!(mmss(75.0), "01:15");
        assert_eq!(mmss(-4.0), "00:00");
    }

    /// An impact whose victim was not being watched is refused rather than
    /// placed from a guess: an offset measured from a guessed position lands
    /// on a real hull section and cannot be told from a measured one.
    #[test]
    fn an_impact_with_no_pose_is_refused() {
        assert!(place(&world(150.0, 0.0, 200.0), None, [0.0, 0.0, 0.0], None).is_none());
    }

    fn shot(origin: WorldPos, shot_id: ShotId) -> wows_replays::analyzer::decoder::ArtilleryShotData {
        wows_replays::analyzer::decoder::ArtilleryShotData {
            origin,
            pitch: 0.0,
            speed: 800.0,
            target: WorldPos::new(0.0, 0.0, 0.0),
            shot_id,
            gun_barrel_id: 0,
            server_time_left: 0.0,
            shooter_height: 10.0,
            hit_distance: 0.0,
        }
    }

    fn landed(origin: WorldPos, impact: WorldPos, pose: Option<VictimPose>) -> ResolvedShotHit {
        let shot_id = ShotId::from(7u32);
        let owner_id = EntityId::from(1u32);
        ResolvedShotHit {
            clock: GameClock(0.0),
            hit: wows_replays::analyzer::decoder::ShotHit {
                owner_id,
                hit_type: wows_replays::analyzer::decoder::HitType {
                    collision: Recognized::Unknown("0".into()),
                    shell_hit: Recognized::Known(ShellHitType::Normal),
                    raw: 0,
                },
                shot_id,
                position: impact,
                terminal_ballistics: None,
            },
            victim_entity_id: EntityId::from(2u32),
            salvo: Some(wows_replays::analyzer::decoder::ArtillerySalvo {
                owner_id,
                params_id: GameParamId::from(3u32),
                salvo_id: 0,
                shots: vec![shot(origin, shot_id)],
            }),
            fired_at: None,
            victim_pose: pose,
        }
    }

    /// A shell fired from astern of a ship facing east arrives travelling up
    /// the hull, which is -Z in the mesh, and flew the distance between the two
    /// points. The bearing and the placement share a frame, which is the point
    /// of reading both here.
    #[test]
    fn a_shell_from_astern_arrives_up_the_hull() {
        let hit = landed(world(0.0, 40.0, 200.0), world(100.0, 0.0, 200.0), Some(pose(0.0)));
        let arrival = shell_arrival(&hit).expect("a salvo and a pose were given");

        // 3 km at the 30 m to the unit `distance_xz` reads a separation at.
        assert!((arrival.travelled.value() - 3000.0).abs() < 1.0, "{:?}", arrival.travelled);
        assert!((arrival.bearing.z + 1.0).abs() < NEAR, "up the hull: {:?}", arrival.bearing);
        assert!(arrival.bearing.x.abs() < NEAR && arrival.bearing.y.abs() < NEAR);
    }

    /// Turn the ship a quarter and the same shell arrives across it instead.
    #[test]
    fn the_same_shell_arrives_abeam_once_the_ship_has_turned() {
        let hit = landed(world(0.0, 40.0, 200.0), world(100.0, 0.0, 200.0), Some(pose(std::f32::consts::FRAC_PI_2)));
        let arrival = shell_arrival(&hit).expect("a salvo and a pose were given");

        assert!(arrival.bearing.z.abs() < NEAR, "no longer up the hull: {:?}", arrival.bearing);
        // The sign matters: port and starboard both land on real plating, so a
        // mirrored bearing reads as a measurement.
        assert!((arrival.bearing.x - 1.0).abs() < NEAR, "but across it, to starboard: {:?}", arrival.bearing);
    }

    /// A hit the salvo does not list the shot of says nothing about where the
    /// shell came from, and a guess would be read as a measurement.
    #[test]
    fn an_arrival_with_no_matching_shot_is_refused() {
        let mut hit = landed(world(0.0, 40.0, 200.0), world(100.0, 0.0, 200.0), Some(pose(0.0)));
        hit.salvo.as_mut().expect("built with a salvo").shots.clear();
        assert!(shell_arrival(&hit).is_none());
    }

    /// And one whose victim was not being watched is refused for the reason
    /// the placement is.
    #[test]
    fn an_arrival_with_no_pose_is_refused() {
        let hit = landed(world(0.0, 40.0, 200.0), world(100.0, 0.0, 200.0), None);
        assert!(shell_arrival(&hit).is_none());
    }
}
