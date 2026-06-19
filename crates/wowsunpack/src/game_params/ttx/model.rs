//! TTX data model: stat newtypes plus the `ShipStats` struct tree.
//!
//! Distance units (`Meters`, `Millimeters`, ...) are reused from `wows-core`
//! (re-exported by `game_params::types`). The newtypes defined here cover the
//! remaining TTX quantities. Leaf structs hold `Option` fields (absent when not
//! computable); per-species fields are filled in by later milestones.

use crate::game_params::types::Km;
use crate::game_params::types::Meters;
use crate::game_params::types::Millimeters;

// `Knots`, `Seconds`, `Hp`, `DegreesPerSecond` are defined below in this module.

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

impl From<f32> for DegreesPerSecond {
    fn from(v: f32) -> Self {
        Self(v)
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

/// Main-battery gun mount stats (`MainGunTTX`, FactoryArtillery.py:70-76 +
/// PreprocessedGun.initGunTTX, PreprocessedGun.py:18-23).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MainGun {
    /// `barrelDiameter * 1000` (PreprocessedGun.py:22).
    pub caliber: Option<Millimeters>,
    /// `gp.numBarrels` (PreprocessedGun.py:21).
    pub num_barrels: Option<u32>,
    /// Count of `HP_AGM_*` mounts (`gunsCount`, PreprocessedArtillery.py:29).
    pub num_guns: Option<u32>,
    /// `rotationSpeed[0] * GMRotationSpeed + GMRotationSpeedBonus` (FactoryArtillery.py:74).
    pub rotation_speed: Option<DegreesPerSecond>,
    /// `180 / rotationSpeed` (FactoryArtillery.py:75).
    pub rotation_time: Option<Seconds>,
}

/// Per-shell stats (`ArtilleryAmmoTTX`, createAmmoTTX, FactoryArtillery.py:147-190).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShellStats {
    /// Projectile GameParams name (`ammoParams.name`, FactoryArtillery.py:152).
    pub name: String,
    /// `ammoType` string ("HE"/"AP"/"CS") off the projectile (PreprocessedAmmo.py:13).
    pub ammo_kind: Option<String>,
    /// `damage` (FactoryArtillery.py:164, full coefficient product).
    pub damage: Option<Hp>,
    /// `caliber * 1000` (FactoryArtillery.py:155).
    pub caliber: Option<Millimeters>,
    /// `bulletSpeed * timeFactor` in m/s (PreprocessedAmmo.py:16).
    pub speed: Option<f32>,
    /// HE `floor(alphaPiercingHE * GMPenetrationCoeffHE)` (FactoryArtillery.py:182),
    /// CS `floor(alphaPiercingCS)` (FactoryArtillery.py:185). AP is a ballistic sim
    /// (no closed-form `piercing` in the deob), left `None`.
    pub penetration: Option<Millimeters>,
    /// HE/AP `calculateBurnChance(...)` as a percent (FactoryArtillery.py:171/188).
    pub burn_chance: Option<Percent>,
    /// HE `floodChance` (`uwCritical`) as a percent (FactoryArtillery.py:172).
    pub flood_chance: Option<Percent>,
    /// `maxAmmoCount` from `poolSize` (FactoryArtillery.py:167-168); `-1` -> `Infinite`.
    pub max_ammo: Option<AmmoCount>,
    /// `disabledUnderwater` (`hull.canBeUnderwater`, FactoryArtillery.py:165).
    pub disabled_underwater: Option<bool>,
}

/// Gun battery stats (`ArtilleryTTX`, FactoryArtillery.py + TTXFactory.py).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Artillery {
    /// `mgReloadTime`: `gun.shotDelay * GMShotDelay`.
    pub reload_time: Option<Seconds>,
    /// `mgMaxDist`: `(maxDist / KM_TO_M) * fcMaxDistCoef * GMMaxDist` (FactoryArtillery.py:42).
    pub range: Option<Km>,
    /// `mgDispersion`: `getDispersionValue(gun, range_km, GMIdealRadius)` (FactoryArtillery.py:47).
    pub dispersion: Option<Meters>,
    /// `ammoSwitchTime`: `shotDelay * ammoSwitchCoeff * GMShotDelay * switchAmmoReloadCoef`
    /// (FactoryTorpedoes.py:67 main-gun analog).
    pub ammo_switch_time: Option<Seconds>,
    /// The main-battery gun mount.
    pub gun: Option<MainGun>,
    /// Per-shell stats, one per resolved ammo name.
    pub shells: Vec<ShellStats>,
}

/// Torpedo launcher + ammo stats (`FactoryTorpedoes.py` `createTorpedoesTTX`).
///
/// `reload_time` is the aggregated launcher reload (`initAmmoReloadParams`,
/// FactoryTorpedoes.py:27-42: `min` of the non-zero per-mount reload times).
/// `launchers` is one [`Launcher`] per torpedo tube (`createLauncherTTX`).
/// `torpedoes` is the per-ammo stat list (`createTorpedoTTX`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Torpedoes {
    pub reload_time: Option<Seconds>,
    pub launchers: Vec<Launcher>,
    pub torpedoes: Vec<TorpedoStats>,
}

/// One torpedo launcher's traverse stats (`TorpedoLauncherTTX`,
/// FactoryTorpedoes.py:74-80).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Launcher {
    /// `rotationSpeed[0] * yawSpeedCoef + yawSpeedBonus` (FactoryTorpedoes.py:78).
    pub rotation_speed: Option<DegreesPerSecond>,
    /// `180 / rotationSpeed` (FactoryTorpedoes.py:79).
    pub rotation_time: Option<Seconds>,
    /// `gp.numBarrels` (PreprocessedTorpedoes.py:72, surfaced for group display).
    pub num_barrels: Option<u32>,
}

/// Per-ammo torpedo stats (`TorpedoTTX`, FactoryTorpedoes.py:82-122).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TorpedoStats {
    /// Projectile GameParams name (`ammoParams.name`, FactoryTorpedoes.py:89).
    pub name: String,
    /// `getTorpedoDamage` (ModifiersApply.py:477-488).
    pub damage: Option<Hp>,
    /// `getTorpedoSpeed` (ModifiersApply.py:491-499).
    pub speed: Option<Knots>,
    /// `maxDist * torpedoRangeCoefficient * BW_TO_BALLISTIC / KM_TO_M` (FactoryTorpedoes.py:93).
    pub range: Option<Km>,
    /// `visibilityFactor * torpedoVisibilityFactor` (FactoryTorpedoes.py:92).
    pub visibility: Option<Km>,
    /// `distanceOfMaxDamage` (FactoryTorpedoes.py:119); arming-distance piece needs
    /// data absent here (`armingTime`/`maneuverDist`), so left `None` (see factory note).
    pub distance_of_max_damage: Option<Km>,
    /// `isDamageIncreasing` (FactoryTorpedoes.py:120).
    pub is_damage_increasing: Option<bool>,
    /// `disabledUnderwater` (FactoryTorpedoes.py:101); submarine-only.
    pub disabled_underwater: Option<bool>,
}

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
