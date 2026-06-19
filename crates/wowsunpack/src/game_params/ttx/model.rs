//! TTX data model: stat newtypes plus the `ShipStats` struct tree.
//!
//! Distance units (`Meters`, `Millimeters`, ...) are reused from `wows-core`
//! (re-exported by `game_params::types`). The newtypes defined here cover the
//! remaining TTX quantities. Leaf structs hold `Option` fields (absent when not
//! computable); per-species fields are filled in by later milestones.

use crate::game_params::types::Km;
use crate::game_params::types::Meters;
use crate::game_params::types::Millimeters;

/// Speed in knots.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Knots(f32);

impl Knots {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Knots {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Duration in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Seconds(f32);

impl Seconds {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Seconds {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Hit points.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Hp(f32);

impl Hp {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Hp {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// A percentage value (0..100 scale, not a 0..1 fraction).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Percent(f32);

impl Percent {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Percent {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Angular speed in degrees per second.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct DegreesPerSecond(f32);

impl DegreesPerSecond {
    pub fn value(&self) -> f32 {
        self.0
    }
}

/// Ammunition pool size. The game uses `-1` to mean an unlimited pool; that
/// sentinel is modeled as `Infinite` rather than a magic number.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum AmmoCount {
    Finite(u32),
    Infinite,
}

/// A ship's full as-shown-in-port stat card. Each section is `None` when the
/// ship has no such module or the section is not computable.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShipStats {
    pub durability: Option<Durability>,
    pub mobility: Option<Mobility>,
    pub armor: Option<Armor>,
    pub battery: Option<Battery>,
    /// Main battery: guns + shells.
    pub artillery: Option<Artillery>,
    pub secondaries: Option<Artillery>,
    /// Launchers + per-ammo.
    pub torpedoes: Option<Torpedoes>,
    pub fire_control: Option<FireControl>,
    pub visibility: Option<Visibility>,
}

/// Survivability stats (`FactoryDurability`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Durability {
    pub health: Option<Hp>,
    pub torpedo_protection: Option<Percent>,
}

/// Maneuverability stats (`FactoryMobility`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mobility {
    pub speed: Option<Knots>,
    pub turning_radius: Option<Meters>,
    pub rudder_time: Option<Seconds>,
}

/// Armor thickness extremes (`FactoryArmor`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Armor {
    pub min: Option<Millimeters>,
    pub max: Option<Millimeters>,
}

/// Submarine battery stats (`FactoryBattery`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Battery {
    pub capacity: Option<f32>,
    pub regeneration: Option<f32>,
}

/// Gun battery stats (`FactoryArtillery`). Fields added in milestone M4.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Artillery {}

/// Torpedo launcher + ammo stats (`FactoryTorpedoes`). Fields added in milestone M3.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Torpedoes {}

/// Fire-control stats (`PreprocessedFireControl`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FireControl {
    pub max_dist: Option<Km>,
}

/// Detectability stats (`FactoryVisibility`). Fields added in milestone M6.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Visibility {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ship_stats_default_is_empty() {
        let stats = ShipStats::default();
        assert!(stats.durability.is_none());
        assert!(stats.mobility.is_none());
        assert!(stats.armor.is_none());
        assert!(stats.battery.is_none());
        assert!(stats.artillery.is_none());
        assert!(stats.secondaries.is_none());
        assert!(stats.torpedoes.is_none());
        assert!(stats.fire_control.is_none());
        assert!(stats.visibility.is_none());
    }

    #[test]
    fn leaf_defaults_are_absent() {
        let durability = Durability::default();
        assert!(durability.health.is_none());
        assert!(durability.torpedo_protection.is_none());

        let mobility = Mobility::default();
        assert!(mobility.speed.is_none());
        assert!(mobility.turning_radius.is_none());
        assert!(mobility.rudder_time.is_none());
    }

    #[test]
    fn ammo_count_models_unlimited_pool() {
        assert_eq!(AmmoCount::Finite(40), AmmoCount::Finite(40));
        assert_ne!(AmmoCount::Finite(40), AmmoCount::Infinite);
    }
}
