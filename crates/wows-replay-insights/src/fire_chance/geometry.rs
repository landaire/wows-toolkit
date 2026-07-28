//! Map an impact position to the fire section it lands in.
//!
//! Pure geometry, no I/O: given where a shell hit in world space and how the
//! victim ship was oriented at that instant, resolve the `burningFlags` bit
//! (the [`BurnNodeIndex`]) that section occupies.
//!
//! Two spaces meet here and they are not the same size. Replay positions are in
//! [`WorldDistance`] units; [`FireSectionGeometry`] is in [`Meters`].
//! Everything a caller passes in is world units and everything the geometry
//! holds is meters, so exactly one conversion belongs in this file, in
//! [`section_for_hit`].
//!
//! Three allowances decide whether an impact was on a hull at all, and they
//! share one correctness argument. Refusing a hit that really was on this hull
//! costs a sample without biasing the rate, because the refusal does not depend
//! on whether a fire started; admitting a hit that was actually on a neighbour
//! is what corrupts, since the ribbon then finds no burn transition on this
//! victim and the trial can only ever score a miss. Each allowance is therefore
//! set where no plausible impact on the hull is refused and no further, and each
//! is read off the same corpus of real impacts: `tests/fire_chance_corpus.rs`
//! prints the body-frame distributions the numbers come from.
//!
//! A ship is long, tall and narrow, and the three axes carry different
//! information, so they are gated apart. One radius over all three cannot tell a
//! mast strike on a battleship from a shell in the water beside a destroyer:
//! wide enough for the first it admits the second, tight enough for the second
//! it refuses the first.

use wowsunpack::game_params::types::Meters;
use wowsunpack::game_params::types::WorldDistance;
use wowsunpack::game_types::Vec3;
use wowsunpack::game_types::WorldPos;
use wowsunpack::models::fire_nodes::BurnNodeIndex;
use wowsunpack::models::fire_nodes::FireSectionGeometry;

/// How far past a hull's burn-node span an impact can sit and still be on that
/// hull.
///
/// The nodes do not reach the hull's ends: on an Iowa-like hull the bow-most
/// node sits about 38 m inboard of the bow, so requiring an impact to fall
/// inside the span would refuse every bow and stern hit.
const HULL_LONGITUDINAL_MARGIN: Meters = Meters::new(75.0);

/// How far above the hull origin an impact can sit.
///
/// Ships are tall and the origin is at the waterline, so a legitimate hit is
/// routinely tens of meters up: a pagoda mast or a carrier's island tops out
/// near 50 m above the water on the tallest hulls in the game. The corpus's
/// highest impact inside a hull's node span is 35 m, with the 99th percentile at
/// 24 m, so this sits above every real impact measured and above the tallest
/// structure that could carry one.
const HULL_VERTICAL_ALLOWANCE_ABOVE: Meters = Meters::new(60.0);

/// How far below the hull origin an impact can sit.
///
/// Negative because shells strike below the waterline: the deepest draft in the
/// game is about 11 m, and an underwater hit lands on the hull under it. The
/// corpus's deepest impact inside a node span is 6 m down, so this is roughly
/// double the measured depth and still nowhere near the lateral scale.
const HULL_VERTICAL_ALLOWANCE_BELOW: Meters = Meters::new(-20.0);

/// How far off the hull's centreline an impact can sit.
///
/// This is the allowance the old single radius got badly wrong, because ships
/// are narrow. The widest hull in the game is about 42 m across a Midway-class
/// flight deck, so 21 m from the centreline covers every beam there is; the rest
/// is for the victim's pose being the last one the client was told about rather
/// than its pose at impact, which a ship at 30 knots turns into about 15 m per
/// second of lag.
///
/// The corpus agrees and shows where the populations part: over impacts inside a
/// hull's node span the lateral offset is 6.3 m at the median and 22.9 m at the
/// 99th percentile, then jumps to 87 m at the 99.9th and past a kilometre beyond
/// that. Everything under this bound is a hull; the decade above it is a
/// different ship.
const HULL_LATERAL_ALLOWANCE: Meters = Meters::new(35.0);

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
///
/// A rotation preserves length, so the result carries the same unit the offset
/// arrived in: [`WorldDistance`] per component.
pub fn world_offset_to_body(offset: Vec3, yaw: f32, pitch: f32, roll: f32) -> Vec3 {
    rotate_z(rotate_x(rotate_y(offset, yaw), pitch), roll)
}

/// Burn node a hit lands in.
///
/// `impact` and `victim_position` come off the packet stream, so their
/// components are [`WorldDistance`] and **not** meters, while
/// [`FireSectionGeometry`] holds [`Meters`]. The conversion between them
/// happens here and is load-bearing: without it every hit shrinks by a factor
/// of fifteen toward the ship's origin and the whole hull collapses onto
/// whichever node sits nearest zero. That is not a visible failure, it is a
/// plausible-looking one, so it survived a full unit-test suite and only a
/// corpus of real matches caught it.
///
/// Body-frame `+X` is the bow and the geometry's longitudinal axis is also
/// bow-positive, so no axis remap or sign flip is needed (the armor viewer
/// applies an additional Ry(-90) on top of this same rotation, but only to
/// reach GLTF mesh space, which this code does not use).
///
/// Only the longitudinal component picks the section, not full 3D distance: the
/// sections partition the hull lengthwise, and the nodes sit at differing
/// heights, so a full-3D nearest would pull a waterline hit toward a
/// superstructure node. Where the impact sits vertically and laterally says
/// nothing about which section it belongs to and is read only to decide whether
/// it was on this hull at all.
///
/// `None` when the impact lies too far from the hull's nodes to have been a hit
/// on it. The victim a hit is keyed to is a nearest-ship heuristic that can pick
/// a neighbour in a tight formation, and [`FireSectionGeometry::nearest_node`]
/// would clamp such an impact onto this hull's bow or stern without complaint.
pub fn section_for_hit(
    geometry: &FireSectionGeometry,
    impact: WorldPos,
    victim_position: WorldPos,
    victim_yaw: f32,
    victim_pitch: f32,
    victim_roll: f32,
) -> Option<BurnNodeIndex> {
    let offset = impact.0 - victim_position.0;
    let body = world_offset_to_body(offset, victim_yaw, victim_pitch, victim_roll);
    let longitudinal = WorldDistance::from(body.x).to_meters();
    let vertical = WorldDistance::from(body.y).to_meters();
    // Unsigned: port and starboard are symmetric, so the sign carries nothing
    // the gate reads.
    let lateral = WorldDistance::from(body.z.abs()).to_meters();
    on_this_hull(geometry, longitudinal, vertical, lateral).then(|| geometry.nearest_node(longitudinal))
}

/// Whether a body-frame impact is close enough to the hull's burn nodes to have
/// landed on it: within [`HULL_LONGITUDINAL_MARGIN`] of the node span at either
/// end, between [`HULL_VERTICAL_ALLOWANCE_BELOW`] and
/// [`HULL_VERTICAL_ALLOWANCE_ABOVE`] of the waterline, and within
/// [`HULL_LATERAL_ALLOWANCE`] of the centreline.
fn on_this_hull(geometry: &FireSectionGeometry, longitudinal: Meters, vertical: Meters, lateral: Meters) -> bool {
    let nodes = geometry.longitudinal();
    let bow = nodes.iter().copied().reduce(|a, b| if b > a { b } else { a });
    let stern = nodes.iter().copied().reduce(|a, b| if b < a { b } else { a });
    // `FireSectionGeometry` rejects an empty node list, so this is unreachable;
    // it refuses rather than admitting a hull with nothing to place a hit on.
    let (Some(bow), Some(stern)) = (bow, stern) else { return false };
    longitudinal <= bow + HULL_LONGITUDINAL_MARGIN
        && longitudinal >= stern - HULL_LONGITUDINAL_MARGIN
        && vertical <= HULL_VERTICAL_ALLOWANCE_ABOVE
        && vertical >= HULL_VERTICAL_ALLOWANCE_BELOW
        && lateral <= HULL_LATERAL_ALLOWANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    use wowsunpack::game_params::types::Meters;

    /// A point so many **meters** from the world origin, handed over in the
    /// world units [`section_for_hit`] actually takes.
    ///
    /// Every case below is naturally written in meters, because the quantities
    /// that make it a bow hit or a stern hit are hull dimensions. Converting at
    /// the call site rather than pre-dividing the literals keeps both spaces on
    /// the page: a reader can see which one each number is in, and a test cannot
    /// silently agree with a `section_for_hit` that skips the conversion. The
    /// old tests did exactly that, passed, and let a fifteen-fold scale error
    /// reach a corpus run.
    fn meters_from_origin(east: f32, up: f32, south: f32) -> WorldPos {
        let world = |m: f32| Meters::from(m).to_world().value();
        WorldPos::new(world(east), world(up), world(south))
    }

    fn origin() -> WorldPos {
        WorldPos::new(0.0, 0.0, 0.0)
    }

    /// Iowa-like: four nodes bow to stern over a 262 m hull, in meters, which is
    /// the space [`FireSectionGeometry`] holds.
    fn iowa_like() -> FireSectionGeometry {
        FireSectionGeometry::from_longitudinal([93.0, 19.0, -35.0, -99.0].map(Meters::from).to_vec()).expect("geom")
    }

    /// At yaw 0 the bow faces East (+X), so a hit 90 m east of the ship centre
    /// is a bow hit.
    #[test]
    fn bow_hit_at_zero_yaw() {
        let node = section_for_hit(&iowa_like(), meters_from_origin(90.0, 0.0, 0.0), origin(), 0.0, 0.0, 0.0);
        assert_eq!(node.expect("on the hull").get(), 0);
    }

    /// The offset arrives in world units and the geometry is in meters, so the
    /// conversion between them decides which section a hit lands in. Read as
    /// meters instead, the same bow hit is 6 m from the ship's origin and
    /// resolves amidships, which is the failure the corpus run surfaced: every
    /// hit on every hull collapsing onto the node nearest zero.
    #[test]
    fn the_offset_is_converted_out_of_world_units() {
        let geom = iowa_like();
        let bow = meters_from_origin(90.0, 0.0, 0.0);
        assert_eq!(section_for_hit(&geom, bow, origin(), 0.0, 0.0, 0.0).expect("on the hull").get(), 0);
        assert_eq!(geom.nearest_node(Meters::from(bow.0.x)).get(), 1, "without the conversion this is not a bow hit");
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
            let impact = meters_from_origin(yaw.cos() * 90.0, 0.0, yaw.sin() * 90.0);
            let node = section_for_hit(&geom, impact, origin(), yaw, 0.0, 0.0);
            assert_eq!(node.expect("on the hull").get(), 0, "yaw {yaw_deg} should still be a bow hit");
        }
    }

    /// A stern hit resolves to the last node, and so does one past the stern
    /// but still on the hull: the stern-most node sits well inboard of the
    /// stern itself.
    #[test]
    fn stern_and_the_overhang_past_it_resolve_to_the_last_node() {
        let geom = iowa_like();
        let node = |m: f32| section_for_hit(&geom, meters_from_origin(m, 0.0, 0.0), origin(), 0.0, 0.0, 0.0);
        assert_eq!(node(-99.0).expect("on the hull").get(), 3);
        assert_eq!(node(-140.0).expect("on the hull").get(), 3);
    }

    /// An impact far past the hull's ends was not on this hull, and clamping it
    /// to the stern node would be a fabricated section. The victim a hit is
    /// keyed to is a nearest-ship guess, so this is what a hit mis-keyed to a
    /// neighbour looks like.
    #[test]
    fn an_impact_beyond_the_hull_is_refused_rather_than_clamped() {
        let geom = iowa_like();
        assert_eq!(section_for_hit(&geom, meters_from_origin(-400.0, 0.0, 0.0), origin(), 0.0, 0.0, 0.0), None);
        assert_eq!(section_for_hit(&geom, meters_from_origin(400.0, 0.0, 0.0), origin(), 0.0, 0.0, 0.0), None);
    }

    /// Sections partition the hull lengthwise, so a hit abeam carries no
    /// longitudinal signal that it missed. The off-axis distance is what
    /// separates a superstructure hit from a shell that landed on the ship
    /// alongside.
    #[test]
    fn an_impact_far_off_the_hulls_axis_is_refused() {
        let geom = iowa_like();
        assert!(section_for_hit(&geom, meters_from_origin(19.0, 25.0, 0.0), origin(), 0.0, 0.0, 0.0).is_some());
        assert_eq!(section_for_hit(&geom, meters_from_origin(19.0, 0.0, 200.0), origin(), 0.0, 0.0, 0.0), None);
    }

    /// Ships are tall and narrow, so the two off-axis directions are not
    /// interchangeable. A hit 40 m up a pagoda mast is an ordinary
    /// superstructure hit; a hit 40 m abeam is open water beside a hull half
    /// that wide. One radius admits both or refuses both, and this is the case
    /// that catches a gate that has collapsed back into one.
    #[test]
    fn height_is_allowed_where_the_same_distance_abeam_is_not() {
        let geom = iowa_like();
        let up = section_for_hit(&geom, meters_from_origin(19.0, 40.0, 0.0), origin(), 0.0, 0.0, 0.0);
        let out = section_for_hit(&geom, meters_from_origin(19.0, 0.0, 40.0), origin(), 0.0, 0.0, 0.0);
        assert_eq!(up.expect("a mast hit is on the hull").get(), 1);
        assert_eq!(out, None, "40 m abeam is past any hull's half beam");
    }

    /// Shells strike below the waterline, so the vertical allowance reaches
    /// under the hull origin as well as over it, but not as far: no hull draws
    /// as deep as its superstructure is tall.
    #[test]
    fn an_impact_below_the_waterline_is_still_on_the_hull() {
        let geom = iowa_like();
        assert!(section_for_hit(&geom, meters_from_origin(19.0, -12.0, 0.0), origin(), 0.0, 0.0, 0.0).is_some());
        assert_eq!(section_for_hit(&geom, meters_from_origin(19.0, -50.0, 0.0), origin(), 0.0, 0.0, 0.0), None);
    }

    /// Port and starboard are the same hull, so the lateral gate reads the
    /// distance from the centreline rather than a signed offset.
    #[test]
    fn the_lateral_gate_is_symmetric_about_the_centreline() {
        let geom = iowa_like();
        let port = section_for_hit(&geom, meters_from_origin(19.0, 5.0, 15.0), origin(), 0.0, 0.0, 0.0);
        let starboard = section_for_hit(&geom, meters_from_origin(19.0, 5.0, -15.0), origin(), 0.0, 0.0, 0.0);
        assert_eq!(port, starboard);
        assert!(port.is_some());
    }

    /// Roll must not move a hit along the hull: rolling rotates about the
    /// longitudinal axis, so the longitudinal coordinate is unchanged.
    #[test]
    fn roll_does_not_change_the_section() {
        let geom = iowa_like();
        let impact = meters_from_origin(19.0, 5.0, 3.0);
        let flat = section_for_hit(&geom, impact, origin(), 0.0, 0.0, 0.0);
        let rolled = section_for_hit(&geom, impact, origin(), 0.0, 0.0, 0.4);
        assert_eq!(flat, rolled);
    }

    /// A one-node hull puts every hit on it in node 0, and still refuses one
    /// that was nowhere near it.
    #[test]
    fn single_node_hull_takes_every_hit_on_it() {
        let geom = FireSectionGeometry::from_longitudinal(vec![Meters::from(0.0)]).expect("geom");
        let node = |m: f32| section_for_hit(&geom, meters_from_origin(m, 0.0, 0.0), origin(), 0.0, 0.0, 0.0);
        assert_eq!(node(40.0).expect("on the hull").get(), 0);
        assert_eq!(node(-40.0).expect("on the hull").get(), 0);
        assert_eq!(node(300.0), None);
    }
}
