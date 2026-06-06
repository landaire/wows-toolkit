//! Distance newtypes and their unit conversions.
//!
//! The game mixes several distance units: real meters, BigWorld engine units
//! (1 BW unit = 30 m), ship-model units (used by armor/hull geometry), plus
//! kilometers and millimeters. These newtypes keep units honest at the type
//! level; cross-unit arithmetic and comparison convert to a common unit.

use std::ops::Add;
use std::ops::Mul;
use std::ops::Sub;

/// Conversion factor: 1 BigWorld unit = 30 meters.
const BW_TO_METERS: f32 = 30.0;

/// Conversion factor: 1 BigWorld unit = 15 ship-model units.
/// Ship geometry (armor meshes, hull models) uses this coordinate space
/// where 1 ship-model unit = 2 real meters (= 30 / 15).
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

/// Distance in ship-model coordinate units (1 unit = 2 meters).
/// Ship geometry (armor meshes, hull visual models) uses this coordinate space.
/// The game defines BW_TO_SHIP = 15, meaning 1 BigWorld unit = 15 ship-model units,
/// so 1 ship-model unit = BW_TO_METERS / BW_TO_SHIP = 30 / 15 = 2 meters.
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

// --- Construction ---

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

// --- Read access and unit conversions ---

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
    /// Convert to ship-model units (1 unit = 2 meters).
    /// Use this for distances that will be compared against ship geometry
    /// (armor meshes, hull models), which are in ship-model coordinates.
    pub fn to_ship_model(self) -> ShipModelDistance {
        ShipModelDistance(self.0 * BW_TO_SHIP / BW_TO_METERS)
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
        Meters(self.0 * BW_TO_METERS / BW_TO_SHIP)
    }
    pub fn to_bigworld(self) -> BigWorldDistance {
        BigWorldDistance(self.0 / BW_TO_SHIP)
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

// --- Scalar multiplication (dimensionless coefficients) ---

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

// --- Same-type arithmetic ---

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

// --- Cross-type arithmetic (converts RHS to LHS unit, returns LHS type) ---

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

// --- Scalar division (for averaging, etc.) ---

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

// --- Sum (for iterator aggregation) ---

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

// --- Cross-type comparison (converts to common unit for PartialEq/PartialOrd) ---

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
