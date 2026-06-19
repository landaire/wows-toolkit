//! Parse-time TTX component base stats.
//!
//! These structs hold raw GameParams values for ship components (hull, engine,
//! and later torpedoes/artillery) read straight off the ship dict during parsing.
//! No formulas or modifier coefficients are applied here; the query-time factory
//! layer converts these into unit-carrying stats and folds in the equipped
//! `ModifierBundle`. Retaining typed `f32`/`Option` values (not the raw pickle)
//! keeps the GameParams footprint comparable to the sibling `HullUpgradeConfig`.
//!
//! Reference-chain (gun/ammo) component stats for torpedoes and artillery are
//! added by later milestones; this module currently covers hull and engine.

use std::collections::HashMap;

use crate::game_params::ttx::model::DegreesPerSecond;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Knots;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::types::Km;
use crate::game_params::types::Meters;

/// Base hull-component stats, raw from the `*_Hull` component sub-object.
/// Fields are `Option` and left `None` when the source field is absent; nothing
/// is defaulted. Values are unconverted GameParams units (the factory layer
/// applies rounding/unit conversions).
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct HullComponentStats {
    /// Raw `health` field (hit points before rounding/modifiers).
    pub health: Option<Hp>,
    /// Raw `maxSpeed` field.
    pub max_speed: Option<Knots>,
    /// Raw hull `speedCoef` field (dimensionless ratio).
    pub speed_coef: Option<f32>,
    /// Raw `turningRadius` field.
    pub turning_radius: Option<Meters>,
    /// Raw `rudderTime` field.
    pub rudder_time: Option<Seconds>,
    /// Raw `visibilityFactor` field (sea detection range, km).
    pub visibility_factor: Option<Km>,
    /// Raw `visibilityFactorByPlane` field (air detection range, km).
    /// FactoryVisibility createVisibilityTTX@278: visibilityByPlane.normal.
    pub visibility_factor_by_plane: Option<Km>,
    /// Raw `visibilityCoefFire` field, added to sea detection while burning.
    /// FactoryVisibility createVisibilityTTX@109: visibilityByShip.fire.
    pub visibility_coef_fire: Option<Km>,
    /// Raw `visibilityCoefFireByPlane` field, added to air detection while burning.
    /// FactoryVisibility createVisibilityTTX@322: visibilityByPlane.fire.
    pub visibility_coef_fire_by_plane: Option<Km>,
    /// Raw `visibilityCoefGK` field (gun-fire detection range, km; near-zero
    /// sentinel `1e-6` when unset in GameParams). Copied by PreprocessedHull.
    pub visibility_coef_gk: Option<Km>,
    /// Raw `visibilityCoefGKInSmoke` field, the in-smoke detection range used when
    /// it exceeds MINIMAL_VALID_VALUE (0.01). FactoryVisibility createVisibilityTTX@137:
    /// visibilityByShip.smoke.
    pub visibility_coef_gk_in_smoke: Option<Km>,
    /// `visibilityFactorsBySubmarine['PERISCOPE']` (submarine periscope-depth detection
    /// range, km). PreprocessedHull.py:13; FactoryVisibility createVisibilityTTX@359
    /// reads it as hull.visibilityByPeriscope for visibilityFromDepth.max. `None` for
    /// non-subs (dict or key absent).
    pub visibility_factor_by_periscope: Option<Km>,
    /// Hull flood probability, derived at parse time from `floodNodes[0][0]` per
    /// `PreprocessedHull.py:11-12` (`(DEFAULT_UW_DAMAGE_COEFF - floodNodes[0][0]) /
    /// DEFAULT_UW_DAMAGE_COEFF`, or 0.0 when equal to the constant). Derived here so
    /// `FactoryDurability`'s `hull.floodProb` reference stays a direct field read.
    /// `None` when `floodNodes` is absent or empty.
    pub flood_prob: Option<f32>,
    /// Submarine `SubmarineBattery.capacity` (charge units). `None` for non-subs
    /// (hulls without a `SubmarineBattery` sub-object).
    pub battery_capacity: Option<f32>,
    /// Submarine `SubmarineBattery.regenRate` (charge units per second). `None` for
    /// non-subs.
    pub battery_regen_rate: Option<f32>,
}

/// Base engine-component stats, raw from the `*_Engine` component sub-object.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct EngineComponentStats {
    /// Raw engine `speedCoef` field.
    pub speed_coef: Option<f32>,
}

/// Base stats for a single torpedo launcher, raw from an `HP_AGT_*` gun
/// sub-object of the torpedo component. Ammo PROJECTILE stats live on the
/// parsed `Projectile` (resolved by name); only the launcher fields and the
/// ammo NAME list are retained here.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct TorpedoLauncherStats {
    /// Raw `shotDelay` field (reload, seconds).
    pub shot_delay: Option<Seconds>,
    /// Raw `rotationSpeed[0]` field (traverse, deg/s).
    pub rotation_speed: Option<DegreesPerSecond>,
    /// Raw `numBarrels` field (count).
    pub num_barrels: Option<f32>,
    /// Raw `ammoSwitchCoeff` field (ratio).
    pub ammo_switch_coeff: Option<f32>,
    /// Projectile names from `ammoList`; resolved to `Projectile` stats at query time.
    pub ammo: Vec<String>,
}

/// Base stats for a single main-battery gun, raw from an `HP_AGM_*` gun
/// sub-object of the artillery component. Shell PROJECTILE stats live on the
/// parsed `Projectile` (resolved by name); only the gun fields and the ammo
/// NAME list are retained here.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ArtilleryGunStats {
    /// Raw `shotDelay` field (reload, seconds).
    pub shot_delay: Option<Seconds>,
    /// Raw `rotationSpeed[0]` field (traverse, deg/s).
    pub rotation_speed: Option<DegreesPerSecond>,
    /// Raw `numBarrels` field (count).
    pub num_barrels: Option<f32>,
    /// Raw `barrelDiameter` field (meters; caliber in mm is this * 1000).
    pub barrel_diameter: Option<Meters>,
    /// Raw `ammoSwitchCoeff` field (ratio).
    pub ammo_switch_coeff: Option<f32>,
    /// Raw gun `minRadius` dispersion field.
    pub min_radius: Option<f32>,
    /// Raw gun `idealRadius` dispersion field.
    pub ideal_radius: Option<f32>,
    /// Raw gun `idealDistance` dispersion field.
    pub ideal_distance: Option<f32>,
    /// Shell projectile names from `ammoList`; resolved to `Projectile` stats at query time.
    pub ammo: Vec<String>,
}

/// Base stats for one artillery component, raw from the `*_Artillery` component
/// sub-object. `max_dist` is the component-level base gun range (meters, before
/// FC coef); `guns` are its `HP_AGM_*` gun sub-objects.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ArtilleryComponentStats {
    /// Raw component-level `maxDist` field (base gun range, meters; the factory
    /// divides by KM_TO_M to get km).
    pub max_dist: Option<Meters>,
    /// Main-battery guns (`HP_AGM_*`) on this component.
    pub guns: Vec<ArtilleryGunStats>,
}

/// Base stats for one secondary-battery (ATBA) component, raw from the ship's
/// `*_ATBA` component sub-object. `max_dist` is the component-level base secondary
/// range (meters, before `/ KM_TO_M`); `guns` are its `HP_GGS_*` gun
/// sub-objects. Reuses [`ArtilleryGunStats`] for the gun fields. Unlike the main
/// battery, ATBA mounts can carry mixed calibers (e.g. 150mm + 105mm), so `guns`
/// may hold several distinct gun groups.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct SecondaryComponentStats {
    /// Raw component-level `maxDist` field (base secondary range, meters; the
    /// factory divides by KM_TO_M to get km).
    pub max_dist: Option<Meters>,
    /// Secondary guns (`HP_GGS_*`) on this component.
    pub guns: Vec<ArtilleryGunStats>,
}

/// Per-ship TTX component base stats, keyed by upgrade selection.
///
/// Hull stats are keyed by the `_Hull` upgrade name (mirroring
/// `HullUpgradeConfig`'s keying); engine stats are keyed by the `_Engine`
/// upgrade name (the engine is a separate `ShipUpgradeInfo` entry, not nested
/// in the hull upgrade's components). Later milestones add torpedo/artillery
/// sections as additional fields.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ShipTtxComponents {
    /// Hull base stats per `_Hull` upgrade name.
    #[cfg_attr(feature = "serde", serde(default))]
    pub hulls: HashMap<String, HullComponentStats>,
    /// Engine base stats per `_Engine` upgrade name.
    #[cfg_attr(feature = "serde", serde(default))]
    pub engines: HashMap<String, EngineComponentStats>,
    /// Torpedo launcher base stats per `_Torpedoes` upgrade name; the `Vec` is
    /// the launchers (`HP_AGT_*` guns) on that mount.
    #[cfg_attr(feature = "serde", serde(default))]
    pub torpedoes: HashMap<String, Vec<TorpedoLauncherStats>>,
    /// Main-battery base stats per `_Artillery` upgrade name.
    #[cfg_attr(feature = "serde", serde(default))]
    pub artillery: HashMap<String, ArtilleryComponentStats>,
    /// Secondary-battery (ATBA) base stats per `_Hull` upgrade name. The ATBA
    /// component is referenced by hull upgrades (e.g. Bismarck's `A_ATBA` from its
    /// A-hull, `B_ATBA` from its B-hull), so it is keyed like [`Self::hulls`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub secondaries: HashMap<String, SecondaryComponentStats>,
    /// Fire-control `maxDistCoef` per `_Suo` upgrade name. The FC component
    /// contributes only this coefficient (PreprocessedFireControl.py:7), which the
    /// `artillery` factory multiplies into main-battery range. Stock is 1.0;
    /// range-extender FC options carry > 1.0.
    #[cfg_attr(feature = "serde", serde(default))]
    pub fire_controls: HashMap<String, f32>,
    /// Stock (base) upgrade selection per slot: the empty-`prev` upgrade in each
    /// `ShipUpgradeInfo` chain, captured during the same walk that fills the maps
    /// above (the raw `ShipUpgradeInfo` pickle is dropped after parsing).
    #[cfg_attr(feature = "serde", serde(default))]
    pub stock_selection: super::selection::ShipUpgradeSelection,
}

impl ShipTtxComponents {
    /// True when no hull, engine, torpedo, artillery, secondary, or fire-control stats were extracted.
    pub fn is_empty(&self) -> bool {
        self.hulls.is_empty()
            && self.engines.is_empty()
            && self.torpedoes.is_empty()
            && self.artillery.is_empty()
            && self.secondaries.is_empty()
            && self.fire_controls.is_empty()
    }

    /// Look up hull stats for a given `_Hull` upgrade name.
    pub fn hull(&self, upgrade_name: &str) -> Option<&HullComponentStats> {
        self.hulls.get(upgrade_name)
    }

    /// Look up engine stats for a given `_Engine` upgrade name.
    pub fn engine(&self, upgrade_name: &str) -> Option<&EngineComponentStats> {
        self.engines.get(upgrade_name)
    }

    /// Look up torpedo launcher stats for a given `_Torpedoes` upgrade name.
    pub fn torpedoes(&self, upgrade_name: &str) -> Option<&[TorpedoLauncherStats]> {
        self.torpedoes.get(upgrade_name).map(|v| v.as_slice())
    }

    /// Look up artillery (main battery) stats for a given `_Artillery` upgrade name.
    pub fn artillery(&self, upgrade_name: &str) -> Option<&ArtilleryComponentStats> {
        self.artillery.get(upgrade_name)
    }

    /// Look up secondary-battery (ATBA) stats for a given `_Hull` upgrade name.
    pub fn secondaries(&self, upgrade_name: &str) -> Option<&SecondaryComponentStats> {
        self.secondaries.get(upgrade_name)
    }

    /// Look up the fire-control `maxDistCoef` for a given `_Suo` upgrade name.
    pub fn fire_control_max_dist_coef(&self, upgrade_name: &str) -> Option<f32> {
        self.fire_controls.get(upgrade_name).copied()
    }

    /// The stock (base) upgrade selection per slot.
    pub fn stock_selection(&self) -> &super::selection::ShipUpgradeSelection {
        &self.stock_selection
    }
}
