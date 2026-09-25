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

/// From the hull's frame to the model's.
///
/// In the hull's frame the bow is +X and up is +Y; in the exported model the
/// bow is +Z, starboard +X and up +Y. This quarter turn maps the one onto
/// the other.
///
/// A direction taken through this still needs its Z negated afterwards, to
/// account for the model exporter turning a left-handed frame into a
/// right-handed one.
pub fn into_model_axes() -> Rotation3<f32> {
    Rotation3::from_axis_angle(&Vec3::y_axis(), -std::f32::consts::FRAC_PI_2)
}

/// Where `hit` landed on its victim's model, in the model's own frame.
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
    let in_model = into_model_axes() * (into_hull_frame(pose.yaw, pose.pitch, pose.roll) * offset);
    let center = Vec3::new(model_center[0], model_center[1], model_center[2]);
    let at = center + in_model;
    Some(match bounds {
        Some((low, high)) => [at.x.clamp(low[0], high[0]), at.y, at.z.clamp(low[2], high[2])],
        None => [at.x, at.y, at.z],
    })
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

    /// A ship facing east takes a hit ahead of it on its bow, which is +Z in
    /// the model.
    #[test]
    fn a_hit_ahead_of_the_bow_lands_on_the_bow() {
        let ahead = world(150.0, 0.0, 200.0);
        let at = placed(ahead, &pose(0.0), [0.0, 0.0, 0.0]);

        assert!((at[2] - 50.0).abs() < NEAR, "fifty metres up the bow: {at:?}");
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

        assert!((at[0] - 1.0).abs() < NEAR && (at[1] - 2.0).abs() < NEAR && (at[2] - 53.0).abs() < NEAR, "{at:?}");
    }

    /// A hit beyond the hull is held at its edge, so a shell that struck
    /// something the model does not carry is not drawn out in open water.
    /// Height is left alone, since above the deck is a real reading.
    #[test]
    fn a_hit_past_the_hull_is_held_at_its_edge() {
        let far = world(400.0, 30.0, 200.0);
        let bounds = Some(([-10.0, -5.0, -100.0], [10.0, 20.0, 100.0]));
        let at = place(&far, Some(&pose(0.0)), [0.0, 0.0, 0.0], bounds).expect("a pose was given");

        assert!((at[2] - 100.0).abs() < NEAR, "held at the bow: {at:?}");
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
}
