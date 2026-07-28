//! Distance newtypes and their unit conversions.
//!
//! The game mixes several distance units: real meters, GameParams BigWorld units
//! (1 unit = 30 m), replay world units (1 unit = 15 m, what packet positions and
//! radii are in), ship-model units (1 unit = 15 m, used by hull geometry), plus
//! kilometers and millimeters. These newtypes keep units honest at the type
//! level; cross-unit arithmetic and comparison convert to a common unit.
//!
//! [`BigWorldDistance`] and [`WorldDistance`] are two different spaces that a
//! name alone will not keep apart, and they differ by exactly 2. Read each
//! type's doc before reaching for either.

use std::fmt;
use std::ops::Add;
use std::ops::Mul;
use std::ops::Sub;

/// Conversion factor: 1 GameParams BigWorld unit = 30 meters.
const BW_TO_METERS: f32 = 30.0;

/// Conversion factor: 1 ship-model unit = 15 meters. The engine's own name for
/// this number is `BW_TO_SHIP`, which reads as a BigWorld-to-ship ratio and is
/// not what it does; the client multiplies by it to leave ship space for meters.
const SHIP_TO_METERS: f32 = 15.0;

/// Conversion factor: 1 replay world unit = 30 meters.
///
/// Replay world space measures the same as GameParams BigWorld space, not the
/// same as ship-model space. `Vec3::distance_xz` in `wows-core::game_types`
/// applies this factor to produce the firing range the ballistics solver runs
/// on, so a different number here would put every armor-viewer trajectory at
/// the wrong range. Corroborated by GameParams values that cross the wire
/// unconverted (a 15.0 smoke radius is 450 m, a 116.667 patrol radius is
/// 3.5 km) and by measuring the longest observed salvo against each ship's
/// artillery `maxDist` over 416 shooters, which lands on a median of 31.09.
const WORLD_TO_METERS: f32 = BW_TO_METERS;

/// Distance in meters.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Meters(f32);

/// Distance in the coordinate units **GameParams** uses (1 unit = 30 meters):
/// consumable `distShip`/`distTorpedo`, projectile `maxDist`, and the other
/// range fields read off the ship dict.
///
/// The 30 is exact against the port. Hydroacoustic Search's `C_4_7` variant
/// carries `distShip = 133.3333`, and its in-game ship-detection range is
/// 4.0 km; Black's radar carries `distShip = 250.0` against a published 7.5 km.
///
/// **Replay packet positions are not in this space.** Entity positions and hit
/// points off the wire measure 15 m per unit ([`WorldDistance`]). The two are
/// separated by measurement, not by naming.
///
/// **Unreconciled:** four live call sites type packet-borne *radii* as this, or
/// convert them at 30, and nothing here establishes which is right. Either they
/// are 2x out, which would mean the minimap has been drawing meters-derived
/// rings at half size, or `WorldDistance`'s 15 is. The hull half-beam
/// measurement behind `WorldDistance` is the tighter of the two and needs no
/// GameParams value, so the call sites are the likelier suspects, but nobody
/// has checked whether a zone radius travels in the same units as a position:
///
/// - `wows-battle-world/src/ingest/entities.rs` (smoke radius from `EntityCreate`)
/// - `wows-replays/src/analyzer/decoder/decode.rs` (`WardAdded` radius)
/// - `wows-toolkit/src/replay/minimap_view/tactics.rs` (`space_size * 30.0`)
/// - `minimap-renderer/src/renderer.rs` (`m / 30.0 / space_size`)
///
/// Until someone measures a drawn radius against a known in-game one, treat a
/// packet radius as unresolved rather than assuming either constant.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct BigWorldDistance(f32);

/// Distance in the coordinate units a **replay packet stream** is in (1 unit =
/// 15 meters): entity positions, shell impact points, and any offset between
/// two of them.
///
/// The 15 is measured, three ways, on the 15.1.0 corpus:
///
/// - **Hull length.** Shell impacts on a ship, rotated into its body frame,
///   reach 8 to 10 units from the hull origin on ships whose bow fire node sits
///   104 to 128 m out. At 15 m/unit that is 120 to 152 m, just past the bow
///   node and inside a ~135 m half-length. At 30 it would be 240 to 300 m, i.e.
///   half-kilometre ships.
/// - **Hull beam.** The same impacts reach 1.1 to 1.6 units abeam, which at
///   15 m/unit is a 34 to 49 m beam for ships whose real beams are 26 to 38 m.
///   Generous by the few meters an impact point sits proud of the plate;
///   at 30 it would put every beam past 68 m.
/// - **Gun range.** The longest shot in a replay, as `|target - origin|` from
///   the salvo packet, runs 247 to 808 units. At 15 m/unit those are 3.7 to
///   12.1 km, every one inside the firing ship's GameParams `maxDist`. At 30,
///   ten of the twenty-six replays measured put shots beyond their own ship's
///   maximum range.
///
/// A fourth, functional measurement agrees: sweeping the scale used to place
/// shell impacts into hull fire sections, and scoring each against the section
/// the server actually lit, gives a single-peaked curve topping out at 15 to 16.
///
/// So this space and [`ShipModelDistance`] measure the same, which is why
/// [`WORLD_TO_METERS`] is defined as [`SHIP_TO_METERS`]. They are still separate
/// types because they are separate spaces: a world position is not a
/// ship-model coordinate, and only one of the two is anchored by the waterline
/// and dispersion evidence on `ShipModelDistance`.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct WorldDistance(f32);

/// Distance in ship-model coordinate units (1 unit = 15 meters). Hull geometry
/// uses this space: `.visual` node matrices, the `.skel_ext` nodes hung off them,
/// and the meshes they position.
///
/// The 15 is measured, and good to roughly +/-3%. That is the precision budget
/// of anything derived from it: a position 100 m from the hull origin carries
/// about 3 m of scale uncertainty.
///
/// The tightest evidence does not come from geometry at all. Solving the
/// main-battery dispersion formula against published port dispersion gives
/// 14.976 for North Carolina and 14.851 for Yamato
/// (`wowsunpack::game_params::ttx::constants::BW_TO_SHIP`). That route is only
/// independent of the geometry if the dispersion formula's ship space and the
/// `.visual`/`.skel_ext` space are the same space, which is inferred from the
/// shared engine import name and not otherwise established.
///
/// Geometric measurements agree but individually are looser: Iowa's waterline
/// beam puts the scale at 15.06 m/unit, while the model bounding box against
/// `A_Hull.size[0]` implies about 14.8 across the roster. The exported hierarchy
/// carries no scale of its own; `Scene Root`, `export` and the hull nodes are
/// identity. See `docs/FIRE_CHANCE.md` section 5.1.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ShipModelDistance(f32);

/// Distance in kilometers.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Km(f32);

/// Distance in millimeters.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Millimeters(f32);

/// Speed in meters per second (shell muzzle velocity).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct MetersPerSecond(f32);

impl From<f32> for Meters {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Meters {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for BigWorldDistance {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for BigWorldDistance {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for ShipModelDistance {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

impl From<f32> for WorldDistance {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

impl From<f32> for Km {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Km {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for Millimeters {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for Millimeters {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl From<f32> for MetersPerSecond {
    fn from(v: f32) -> Self {
        Self(v)
    }
}
impl From<i32> for MetersPerSecond {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl Meters {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        BigWorldDistance(self.0 / BW_TO_METERS)
    }
    /// Convert to ship-model units (1 unit = 15 meters).
    /// Use this for distances that will be compared against ship geometry
    /// (hull models, armor meshes), which are in ship-model coordinates.
    pub fn to_ship_model(self) -> ShipModelDistance {
        ShipModelDistance(self.0 / SHIP_TO_METERS)
    }
    pub fn to_km(self) -> Km {
        Km(self.0 / 1000.0)
    }
    pub fn to_mm(self) -> Millimeters {
        Millimeters(self.0 * 1000.0)
    }
    /// Convert to replay world units (1 unit = 15 meters). Use this for
    /// distances that will be compared against packet positions.
    pub fn to_world(self) -> WorldDistance {
        WorldDistance(self.0 / WORLD_TO_METERS)
    }
}

impl BigWorldDistance {
    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * BW_TO_METERS)
    }
    pub fn to_km(self) -> Km {
        self.to_meters().to_km()
    }
}

impl ShipModelDistance {
    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * SHIP_TO_METERS)
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        self.to_meters().to_bigworld()
    }
}

impl WorldDistance {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * WORLD_TO_METERS)
    }
    pub fn to_km(self) -> Km {
        self.to_meters().to_km()
    }
}

impl Km {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 * 1000.0)
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        self.to_meters().to_bigworld()
    }
}

impl Millimeters {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
    pub fn to_meters(self) -> Meters {
        Meters(self.0 / 1000.0)
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        self.to_meters().to_bigworld()
    }
}

impl MetersPerSecond {
    /// Const constructor for use in static/const contexts.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// Conventional English-port rounding: whole meters.
impl fmt::Display for Meters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0} m", self.0)
    }
}

/// Conventional English-port rounding: one decimal place.
impl fmt::Display for Km {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} km", self.0)
    }
}

/// Conventional English-port rounding: whole millimeters.
impl fmt::Display for Millimeters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0} mm", self.0)
    }
}

/// Conventional English-port rounding: whole meters per second.
impl fmt::Display for MetersPerSecond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0} m/s", self.0)
    }
}

impl Mul<f32> for Meters {
    type Output = Meters;
    fn mul(self, rhs: f32) -> Meters {
        Meters(self.0 * rhs)
    }
}

impl Mul<f32> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn mul(self, rhs: f32) -> BigWorldDistance {
        BigWorldDistance(self.0 * rhs)
    }
}

impl Mul<f32> for Km {
    type Output = Km;
    fn mul(self, rhs: f32) -> Km {
        Km(self.0 * rhs)
    }
}

impl Mul<f32> for Millimeters {
    type Output = Millimeters;
    fn mul(self, rhs: f32) -> Millimeters {
        Millimeters(self.0 * rhs)
    }
}

impl Add for Meters {
    type Output = Meters;
    fn add(self, rhs: Meters) -> Meters {
        Meters(self.0 + rhs.0)
    }
}
impl Sub for Meters {
    type Output = Meters;
    fn sub(self, rhs: Meters) -> Meters {
        Meters(self.0 - rhs.0)
    }
}

impl Add for BigWorldDistance {
    type Output = BigWorldDistance;
    fn add(self, rhs: BigWorldDistance) -> BigWorldDistance {
        BigWorldDistance(self.0 + rhs.0)
    }
}
impl Sub for BigWorldDistance {
    type Output = BigWorldDistance;
    fn sub(self, rhs: BigWorldDistance) -> BigWorldDistance {
        BigWorldDistance(self.0 - rhs.0)
    }
}

impl Add for Km {
    type Output = Km;
    fn add(self, rhs: Km) -> Km {
        Km(self.0 + rhs.0)
    }
}
impl Sub for Km {
    type Output = Km;
    fn sub(self, rhs: Km) -> Km {
        Km(self.0 - rhs.0)
    }
}

impl Add for Millimeters {
    type Output = Millimeters;
    fn add(self, rhs: Millimeters) -> Millimeters {
        Millimeters(self.0 + rhs.0)
    }
}
impl Sub for Millimeters {
    type Output = Millimeters;
    fn sub(self, rhs: Millimeters) -> Millimeters {
        Millimeters(self.0 - rhs.0)
    }
}

impl Add<BigWorldDistance> for Meters {
    type Output = Meters;
    fn add(self, rhs: BigWorldDistance) -> Meters {
        Meters(self.0 + rhs.to_meters().0)
    }
}
impl Sub<BigWorldDistance> for Meters {
    type Output = Meters;
    fn sub(self, rhs: BigWorldDistance) -> Meters {
        Meters(self.0 - rhs.to_meters().0)
    }
}

impl Add<Meters> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn add(self, rhs: Meters) -> BigWorldDistance {
        BigWorldDistance(self.0 + rhs.to_bigworld().0)
    }
}
impl Sub<Meters> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn sub(self, rhs: Meters) -> BigWorldDistance {
        BigWorldDistance(self.0 - rhs.to_bigworld().0)
    }
}

impl Add<Km> for Meters {
    type Output = Meters;
    fn add(self, rhs: Km) -> Meters {
        Meters(self.0 + rhs.to_meters().0)
    }
}
impl Sub<Km> for Meters {
    type Output = Meters;
    fn sub(self, rhs: Km) -> Meters {
        Meters(self.0 - rhs.to_meters().0)
    }
}

impl Add<Meters> for Km {
    type Output = Km;
    fn add(self, rhs: Meters) -> Km {
        Km(self.0 + rhs.to_km().0)
    }
}
impl Sub<Meters> for Km {
    type Output = Km;
    fn sub(self, rhs: Meters) -> Km {
        Km(self.0 - rhs.to_km().0)
    }
}

impl std::ops::Div<f32> for Meters {
    type Output = Meters;
    fn div(self, rhs: f32) -> Meters {
        Meters(self.0 / rhs)
    }
}

impl std::ops::Div<f32> for BigWorldDistance {
    type Output = BigWorldDistance;
    fn div(self, rhs: f32) -> BigWorldDistance {
        BigWorldDistance(self.0 / rhs)
    }
}

impl std::ops::Div<f32> for Km {
    type Output = Km;
    fn div(self, rhs: f32) -> Km {
        Km(self.0 / rhs)
    }
}

impl std::ops::Div<f32> for Millimeters {
    type Output = Millimeters;
    fn div(self, rhs: f32) -> Millimeters {
        Millimeters(self.0 / rhs)
    }
}

impl std::iter::Sum for Meters {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Meters(iter.map(|m| m.0).sum())
    }
}

impl std::iter::Sum for BigWorldDistance {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        BigWorldDistance(iter.map(|d| d.0).sum())
    }
}

impl std::iter::Sum for Km {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Km(iter.map(|k| k.0).sum())
    }
}

impl std::iter::Sum for Millimeters {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        Millimeters(iter.map(|m| m.0).sum())
    }
}

impl PartialEq<BigWorldDistance> for Meters {
    fn eq(&self, other: &BigWorldDistance) -> bool {
        self.0 == other.to_meters().0
    }
}
impl PartialOrd<BigWorldDistance> for Meters {
    fn partial_cmp(&self, other: &BigWorldDistance) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.to_meters().0)
    }
}

impl PartialEq<Meters> for BigWorldDistance {
    fn eq(&self, other: &Meters) -> bool {
        self.0 == other.to_bigworld().0
    }
}
impl PartialOrd<Meters> for BigWorldDistance {
    fn partial_cmp(&self, other: &Meters) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.to_bigworld().0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters_display_rounds_to_whole() {
        assert_eq!(Meters::from(740.6).to_string(), "741 m");
    }

    #[test]
    fn km_display_one_decimal() {
        assert_eq!(Km::from(10.5).to_string(), "10.5 km");
    }

    #[test]
    fn millimeters_display_rounds_to_whole() {
        assert_eq!(Millimeters::from(406.0).to_string(), "406 mm");
    }

    #[test]
    fn meters_per_second_display_rounds_to_whole() {
        assert_eq!(MetersPerSecond::from(820.0).to_string(), "820 m/s");
    }

    /// Iowa's bow fire node sits at model z 6.489, which is 97.3 m forward of the
    /// hull origin on a 262 m ship.
    #[test]
    fn ship_model_units_are_fifteen_meters() {
        assert_eq!(ShipModelDistance::from(6.489).to_meters().value(), 97.335);
        assert_eq!(Meters::from(97.335).to_ship_model().value(), 6.489);
    }

    /// `Vec3::distance_xz` scales a world-space separation by 30 to reach the
    /// firing range the ballistics solver runs on, so this factor is already
    /// load-bearing outside this module.
    #[test]
    fn replay_world_units_are_thirty_meters() {
        assert_eq!(WorldDistance::from(8.0).to_meters().value(), 240.0);
        assert_eq!(Meters::from(240.0).to_world().value(), 8.0);
    }

    /// Replay world space and GameParams BigWorld space measure the same, which
    /// is why a GameParams radius crosses the wire unconverted and is still
    /// comparable against positions. Ship-model space is the odd one at 15, so
    /// placing an impact against a hull's nodes spans both scales.
    #[test]
    fn replay_and_game_params_spaces_agree_and_the_ship_model_does_not() {
        let one = 1.0f32;
        assert_eq!(WorldDistance::from(one).to_meters().value(), 30.0);
        assert_eq!(BigWorldDistance::from(one).to_meters().value(), 30.0);
        assert_eq!(ShipModelDistance::from(one).to_meters().value(), 15.0);
    }

    /// Hydroacoustic Search `C_4_7` carries `distShip = 133.3333` and detects
    /// ships at 4.0 km in game; Black's radar carries 250.0 against 7.5 km.
    /// These are what anchor the GameParams scale.
    #[test]
    fn game_params_distances_match_the_published_consumable_ranges() {
        assert!((BigWorldDistance::from(133.3333).to_km().value() - 4.0).abs() < 0.001);
        assert_eq!(BigWorldDistance::from(250.0).to_km().value(), 7.5);
    }
}
