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

/// Base hull-component stats, raw from the `*_Hull` component sub-object.
/// Fields are `Option` and left `None` when the source field is absent; nothing
/// is defaulted. Values are unconverted GameParams units (the factory layer
/// applies rounding/unit conversions).
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct HullComponentStats {
    /// Raw `health` field (hit points before rounding/modifiers).
    pub health: Option<f32>,
    /// Raw `maxSpeed` field.
    pub max_speed: Option<f32>,
    /// Raw hull `speedCoef` field.
    pub speed_coef: Option<f32>,
    /// Raw `turningRadius` field.
    pub turning_radius: Option<f32>,
    /// Raw `rudderTime` field.
    pub rudder_time: Option<f32>,
    /// Raw `visibilityFactor` field (sea detection coefficient).
    pub visibility_factor: Option<f32>,
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
}

impl ShipTtxComponents {
    /// True when no hull or engine stats were extracted.
    pub fn is_empty(&self) -> bool {
        self.hulls.is_empty() && self.engines.is_empty()
    }

    /// Look up hull stats for a given `_Hull` upgrade name.
    pub fn hull(&self, upgrade_name: &str) -> Option<&HullComponentStats> {
        self.hulls.get(upgrade_name)
    }

    /// Look up engine stats for a given `_Engine` upgrade name.
    pub fn engine(&self, upgrade_name: &str) -> Option<&EngineComponentStats> {
        self.engines.get(upgrade_name)
    }
}
