//! Map an impact position to the fire section it lands in.
//!
//! Pure geometry, no I/O: given where a shell hit in world space and how the
//! victim ship was oriented at that instant, resolve the `burningFlags` bit
//! (the [`BurnNodeIndex`]) that section occupies.

use wowsunpack::game_params::types::Meters;
use wowsunpack::game_types::Vec3;
use wowsunpack::game_types::WorldPos;
use wowsunpack::models::fire_nodes::BurnNodeIndex;
use wowsunpack::models::fire_nodes::FireSectionGeometry;

/// Ry(a): rotates +X toward -Z.
fn rotate_y(v: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(c * v.x + s * v.z, v.y, -s * v.x + c * v.z)
}

/// Rx(a): rotates +Y toward +Z.
fn rotate_x(v: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(v.x, c * v.y - s * v.z, s * v.y + c * v.z)
}

/// Rz(a): rotates +X toward +Y.
fn rotate_z(v: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(c * v.x - s * v.y, s * v.x + c * v.y, v.z)
}

/// Rotate a world-space offset into the victim's body frame.
///
/// BigWorld yaw 0 faces East (+X) and yaw increases counter-clockwise, so the
/// forward rotation is Ry(-yaw) and its inverse is Ry(+yaw). Composed as
/// Rz(roll) * Rx(pitch) * Ry(yaw), matching the armor viewer's
/// `inverse_ship_rotation`. In the body frame the bow is +X.
pub fn world_offset_to_body(offset: Vec3, yaw: f32, pitch: f32, roll: f32) -> Vec3 {
    rotate_z(rotate_x(rotate_y(offset, yaw), pitch), roll)
}

/// Burn node a hit lands in.
///
/// `impact` and `victim_position` are world-space and already in meters, so no
/// scaling happens here; [`FireSectionGeometry`] was converted to meters when it
/// was resolved. Body-frame `+X` is the bow and the geometry's longitudinal axis
/// is also bow-positive, so no axis remap or sign flip is needed here (the armor
/// viewer applies an additional Ry(-90) on top of this same rotation, but only to
/// reach GLTF mesh space, which this code does not use).
///
/// Only the longitudinal component is used, not full 3D distance: the sections
/// partition the hull lengthwise, and the nodes sit at differing heights, so a
/// full-3D nearest would pull a waterline hit toward a superstructure node.
pub fn section_for_hit(
    geometry: &FireSectionGeometry,
    impact: WorldPos,
    victim_position: WorldPos,
    victim_yaw: f32,
    victim_pitch: f32,
    victim_roll: f32,
) -> BurnNodeIndex {
    let offset = impact.0 - victim_position.0;
    let body = world_offset_to_body(offset, victim_yaw, victim_pitch, victim_roll);
    geometry.nearest_node(Meters::from(body.x))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Iowa-like: four nodes bow to stern over a 262 m hull.
    fn iowa_like() -> FireSectionGeometry {
        FireSectionGeometry::from_longitudinal([93.0, 19.0, -35.0, -99.0].map(Meters::from).to_vec()).expect("geom")
    }

    /// At yaw 0 the bow faces East (+X), so a hit 90 m east of the ship centre
    /// is a bow hit.
    #[test]
    fn bow_hit_at_zero_yaw() {
        let node =
            section_for_hit(&iowa_like(), WorldPos::new(90.0, 0.0, 0.0), WorldPos::new(0.0, 0.0, 0.0), 0.0, 0.0, 0.0);
        assert_eq!(node.get(), 0);
    }

    /// The same hit on a ship rotated 90 degrees must resolve to the same
    /// section. This is the test that catches a wrong rotation sign.
    #[test]
    fn section_is_invariant_under_yaw() {
        let geom = iowa_like();
        for yaw_deg in [0.0f32, 45.0, 90.0, 180.0, 270.0] {
            let yaw = yaw_deg.to_radians();
            // Place the impact 90 m along the ship's own forward axis. At yaw,
            // forward is (cos(yaw), 0, sin(yaw)) in world space: yaw 0 faces
            // East (+X) and increases toward +Z, matching minimap-renderer's
            // independently validated "yaw=0 east, increases counter-clockwise"
            // ship-icon rotation (drawing.rs draw_ship_icon).
            let fwd_x = yaw.cos() * 90.0;
            let fwd_z = yaw.sin() * 90.0;
            let node =
                section_for_hit(&geom, WorldPos::new(fwd_x, 0.0, fwd_z), WorldPos::new(0.0, 0.0, 0.0), yaw, 0.0, 0.0);
            assert_eq!(node.get(), 0, "yaw {yaw_deg} should still be a bow hit");
        }
    }

    /// A stern hit resolves to the last node, and a hit past the stern clamps
    /// to it rather than wrapping.
    #[test]
    fn stern_and_beyond_resolve_to_the_last_node() {
        let geom = iowa_like();
        assert_eq!(
            section_for_hit(&geom, WorldPos::new(-99.0, 0.0, 0.0), WorldPos::new(0.0, 0.0, 0.0), 0.0, 0.0, 0.0).get(),
            3
        );
        assert_eq!(
            section_for_hit(&geom, WorldPos::new(-400.0, 0.0, 0.0), WorldPos::new(0.0, 0.0, 0.0), 0.0, 0.0, 0.0).get(),
            3
        );
    }

    /// Roll must not move a hit along the hull: rolling rotates about the
    /// longitudinal axis, so the longitudinal coordinate is unchanged.
    #[test]
    fn roll_does_not_change_the_section() {
        let geom = iowa_like();
        let flat = section_for_hit(&geom, WorldPos::new(19.0, 5.0, 3.0), WorldPos::new(0.0, 0.0, 0.0), 0.0, 0.0, 0.0);
        let rolled = section_for_hit(&geom, WorldPos::new(19.0, 5.0, 3.0), WorldPos::new(0.0, 0.0, 0.0), 0.0, 0.0, 0.4);
        assert_eq!(flat.get(), rolled.get());
    }

    /// A one-node hull puts every hit in node 0.
    #[test]
    fn single_node_hull_takes_every_hit() {
        let geom = FireSectionGeometry::from_longitudinal(vec![Meters::from(0.0)]).expect("geom");
        assert_eq!(
            section_for_hit(&geom, WorldPos::new(40.0, 0.0, 0.0), WorldPos::new(0.0, 0.0, 0.0), 0.0, 0.0, 0.0).get(),
            0
        );
    }
}
