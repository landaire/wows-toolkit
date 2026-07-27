//! Distance newtypes and their unit conversions.
//!
//! The game mixes several distance units: real meters, BigWorld engine units
//! (1 BW unit = 30 m), ship-model units (1 unit = 15 m, used by hull geometry),
//! plus kilometers and millimeters. These newtypes keep units honest at the type
//! level; cross-unit arithmetic and comparison convert to a common unit.

use std::fmt;
use std::ops::Add;
use std::ops::Mul;
use std::ops::Sub;

/// Conversion factor: 1 BigWorld unit = 30 meters.
const BW_TO_METERS: f32 = 30.0;

/// Conversion factor: 1 ship-model unit = 15 meters.
const BW_TO_SHIP: f32 = 15.0;

/// Distance in meters.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Meters(f32);

/// Distance in BigWorld coordinate units (1 BW unit = 30 meters).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct BigWorldDistance(f32);

/// Distance in ship-model coordinate units (1 unit = 15 meters). Hull geometry
/// uses this space: `.visual` node matrices, the `.skel_ext` nodes hung off them,
/// and the meshes they position.
///
/// Measured against the live roster, not assumed: for 1158 hulls the root
/// visual's longitudinal extent times 15 reproduces `A_Hull.size[0]` with a
/// median ratio of 1.014 (p5..p95 of 1.001..1.070, the residual being bow and
/// stern overhang the published length excludes). The exported hierarchy carries
/// no scale of its own; `Scene Root`, `export` and the hull nodes are identity.
/// The same 15 falls out of the main-battery dispersion formula against published
/// port values (`wowsunpack::game_params::ttx::constants::BW_TO_SHIP`). See
/// `docs/FIRE_CHANCE.md` section 5.1.
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
        ShipModelDistance(self.0 / BW_TO_SHIP)
    }
    pub fn to_km(self) -> Km {
        Km(self.0 / 1000.0)
    }
    pub fn to_mm(self) -> Millimeters {
        Millimeters(self.0 * 1000.0)
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
        Meters(self.0 * BW_TO_SHIP)
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        self.to_meters().to_bigworld()
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
}
