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

use crate::game_params::ttx::constants::DispersionCurve;
use crate::game_params::ttx::model::DegreesPerSecond;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Knots;
use crate::game_params::ttx::model::Percent;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::special_abilities::AlternativeFireMode;
use crate::game_params::ttx::special_abilities::CombatInstructions;
use crate::game_params::types::Km;
use crate::game_params::types::Meters;

/// One hull burn node: a fire section's resistance, damage rate and duration.
///
/// `probability` is the ship's hidden fire-resistance coefficient, multiplied
/// into the attacker's burn chance server side. Player ships carry 0.4339 to
/// 1.0 by tier; a few bot hulls carry 9.0.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct BurnNode {
    pub probability: f32,

    /// Maximum health lost per second, e.g. 0.3%.
    pub damage_per_sec: Percent,
    pub duration: Seconds,
}

/// One hull flood node, the bow or stern end.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct FloodNode {
    /// Torpedo damage coefficient the hull's torpedo protection derives from
    /// (`flood_prob`).
    pub torpedo_damage_coeff: f32,

    /// Maximum health lost per second, e.g. 0.5%.
    pub damage_per_sec: Percent,
    pub duration: Seconds,
}

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
    /// Hull `burnNodes`, one entry per fire section. The length is the section
    /// count and is per-hull: surface combatants carry four, most submarines one,
    /// some auxiliaries two. Empty when the hull has no `burnNodes` key.
    pub burn_nodes: Vec<BurnNode>,
    /// Hull `floodNodes`, one entry per flooding section. Empty when the hull
    /// has no `floodNodes` key.
    pub flood_nodes: Vec<FloodNode>,
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
    /// Raw gun `radiusOnZero` dispersion-curve coefficient (vertical/horizontal ratio at zero range).
    pub radius_on_zero: Option<f32>,
    /// Raw gun `radiusOnDelim` dispersion-curve coefficient (ratio at the `delim` distance).
    pub radius_on_delim: Option<f32>,
    /// Raw gun `radiusOnMax` dispersion-curve coefficient (ratio at max range).
    pub radius_on_max: Option<f32>,
    /// Raw gun `delim` dispersion-curve split point (fraction of max range).
    pub delim: Option<f32>,
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
    /// Raw component-level `sigmaCount`: how many standard deviations of the
    /// shell distribution fit within the dispersion ellipse. Higher puts more
    /// shells near the aim point.
    #[cfg_attr(feature = "serde", serde(default))]
    pub sigma_count: Option<f32>,
    /// Main-battery guns (`HP_AGM_*`) on this component.
    pub guns: Vec<ArtilleryGunStats>,
}

/// Coefficients of one fire-control component (`PreprocessedFireControl`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct FireControlStats {
    /// `maxDistCoef`, which the `artillery` factory multiplies into
    /// main-battery range. Stock is 1.0; range-extender options carry > 1.0.
    pub max_dist_coef: f32,
    /// `sigmaCountCoef`, which multiplies the main battery `sigmaCount`.
    pub sigma_count_coef: f32,
}

/// Base stats for one secondary-battery (ATBA) component, raw from the ship's
/// `*_ATBA` component sub-object. `max_dist` is the component-level base secondary
/// range (meters, before `/ KM_TO_M`); `guns` are its `HP_GGS_*` gun
/// sub-objects. Unlike the main battery, ATBA mounts can carry mixed calibers
/// (e.g. 150mm + 105mm), so `guns` may hold several distinct gun groups.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct SecondaryComponentStats {
    /// Raw component-level `maxDist` field (base secondary range, meters; the
    /// factory divides by KM_TO_M to get km).
    pub max_dist: Option<Meters>,
    /// Secondary guns (`HP_GGS_*`) on this component.
    pub guns: Vec<SecondaryGun>,
}

/// One secondary gun mount.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct SecondaryGun {
    /// Hardpoint name, e.g. `HP_JGS_2`.
    pub hardpoint: String,
    pub stats: ArtilleryGunStats,
}

/// The carrier plane slot a `ShipUpgradeInfo` upgrade fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum PlaneSlot {
    /// Rocket planes; GameParams calls the slot `fighter`.
    Fighter,
    DiveBomber,
    TorpedoBomber,
    SkipBomber,
}

impl PlaneSlot {
    pub const ALL: [PlaneSlot; 4] = [Self::Fighter, Self::DiveBomber, Self::TorpedoBomber, Self::SkipBomber];

    /// The `ucType` of upgrades in this slot.
    pub fn uc_type(self) -> &'static str {
        match self {
            Self::Fighter => crate::game_params::keys::UC_TYPE_FIGHTER,
            Self::DiveBomber => crate::game_params::keys::UC_TYPE_DIVE_BOMBER,
            Self::TorpedoBomber => crate::game_params::keys::UC_TYPE_TORPEDO_BOMBER,
            Self::SkipBomber => crate::game_params::keys::UC_TYPE_SKIP_BOMBER,
        }
    }

    /// The key of this slot's component in an upgrade's `components`.
    pub fn component_key(self) -> &'static str {
        match self {
            Self::Fighter => crate::game_params::keys::COMP_FIGHTER,
            Self::DiveBomber => crate::game_params::keys::COMP_DIVE_BOMBER,
            Self::TorpedoBomber => crate::game_params::keys::COMP_TORPEDO_BOMBER,
            Self::SkipBomber => crate::game_params::keys::COMP_SKIP_BOMBER,
        }
    }
}

/// The aircraft one plane-slot upgrade carries.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct SquadronUpgrade {
    pub slot: PlaneSlot,

    /// Aircraft param names from the component's `planes`: the regular
    /// squadron, then a tactical squadron when the slot has one
    /// (PreprocessedPlanes.py:12-20).
    pub planes: Vec<String>,

    /// The chain root of its slot, mounted on the stock ship.
    pub stock: bool,
}

/// An air strike, raw from the hull's `airSupport` component
/// (ShipParams.py:265-278).
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AirSupportStats {
    /// Aircraft param names from `ammoList`.
    pub planes: Vec<String>,
    pub charges: Option<u32>,
    pub reload_time: Option<Seconds>,
    /// Raw `maxDist`, meters.
    pub max_dist: Option<Meters>,
}

/// Number of launchers of a group that reload at the same time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct LoaderCount(pub u32);

/// Launchers that share loaders: at most `loaders` of `hardpoints` reload at
/// once (`loaders` and `groups` of a torpedo component; the client queues the
/// rest, `TorpedoesReloadManagers.RevolverCylinder`).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct TorpedoLoaderGroup {
    pub loaders: LoaderCount,
    pub hardpoints: Vec<String>,
}

/// Where a group of submarine launchers points (`groupsDirection`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum LauncherDirection {
    Forward,
    Rear,
    Rotating,
    /// A direction this crate does not recognize.
    Other(String),
}

impl LauncherDirection {
    pub fn from_game_str(direction: &str) -> Self {
        match direction {
            "forward" => Self::Forward,
            "rear" => Self::Rear,
            "rotating" => Self::Rotating,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Launchers of one direction.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct LauncherDirectionGroup {
    pub direction: LauncherDirection,
    pub hardpoints: Vec<String>,
}

/// How the launchers of one torpedo component are grouped. Launchers outside
/// every loader group reload all at once.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct TorpedoLoading {
    pub loader_groups: Vec<TorpedoLoaderGroup>,
    pub directions: Vec<LauncherDirectionGroup>,
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
    /// Launcher grouping per torpedo component name, e.g. `A_Torpedoes`, for
    /// components that group their launchers.
    #[cfg_attr(feature = "serde", serde(default))]
    pub torpedo_loading: HashMap<String, TorpedoLoading>,
    /// Main-battery base stats per `_Artillery` upgrade name.
    #[cfg_attr(feature = "serde", serde(default))]
    pub artillery: HashMap<String, ArtilleryComponentStats>,
    /// Secondary-battery (ATBA) base stats per `_Hull` upgrade name. The ATBA
    /// component is referenced by hull upgrades (e.g. Bismarck's `A_ATBA` from its
    /// A-hull, `B_ATBA` from its B-hull), so it is keyed like [`Self::hulls`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub secondaries: HashMap<String, SecondaryComponentStats>,
    /// Fire-control coefficients per `_Suo` upgrade name.
    #[cfg_attr(feature = "serde", serde(default))]
    pub fire_controls: HashMap<String, FireControlStats>,
    /// Components each `_Artillery` and `_Torpedoes` upgrade lists, one per
    /// hull it fits, e.g. `["A_Artillery", "B_Artillery"]`. A hull mounts the
    /// listed one its [`crate::game_params::types::HullUpgradeConfig`] names as
    /// its default or as an alternative.
    #[cfg_attr(feature = "serde", serde(default))]
    pub upgrade_components: HashMap<String, Vec<String>>,
    /// Carrier and hybrid squadrons per plane-slot upgrade name.
    #[cfg_attr(feature = "serde", serde(default))]
    pub squadrons: HashMap<String, SquadronUpgrade>,
    /// Air strike per `_Hull` upgrade name.
    #[cfg_attr(feature = "serde", serde(default))]
    pub air_supports: HashMap<String, AirSupportStats>,
    /// Combat Instructions per `_Hull` upgrade name, from the hull's
    /// `specials` component.
    #[cfg_attr(feature = "serde", serde(default))]
    pub combat_instructions: HashMap<String, CombatInstructions>,
    /// Alternative fire mode per artillery component name, e.g.
    /// `A_Artillery`, for components that have one.
    #[cfg_attr(feature = "serde", serde(default))]
    pub alternative_fire: HashMap<String, AlternativeFireMode>,
    /// Stock (base) upgrade selection per slot: the empty-`prev` upgrade in each
    /// `ShipUpgradeInfo` chain, captured during the same walk that fills the maps
    /// above (the raw `ShipUpgradeInfo` pickle is dropped after parsing).
    #[cfg_attr(feature = "serde", serde(default))]
    pub stock_selection: super::selection::ShipUpgradeSelection,
}

impl ShipTtxComponents {
    /// True when no hull, engine, torpedo, artillery, secondary, fire-control,
    /// squadron, air strike or ability stats were extracted.
    pub fn is_empty(&self) -> bool {
        self.hulls.is_empty()
            && self.engines.is_empty()
            && self.torpedoes.is_empty()
            && self.artillery.is_empty()
            && self.secondaries.is_empty()
            && self.fire_controls.is_empty()
            && self.squadrons.is_empty()
            && self.air_supports.is_empty()
            && self.combat_instructions.is_empty()
            && self.alternative_fire.is_empty()
    }

    /// Combat Instructions of a `_Hull` upgrade, `None` when it has none.
    pub fn combat_instructions(&self, hull_upgrade: &str) -> Option<&CombatInstructions> {
        self.combat_instructions.get(hull_upgrade)
    }

    /// Alternative fire mode of an artillery component, `None` when it has
    /// none.
    pub fn alternative_fire(&self, component: &str) -> Option<&AlternativeFireMode> {
        self.alternative_fire.get(component)
    }

    /// These stats with every artillery upgrade switched to its alternative
    /// fire mode: guns reload for the mode's full reload and load its shells
    /// when it has its own. `None` when no artillery upgrade has the mode.
    pub fn in_alternative_fire_mode(&self) -> Option<Self> {
        let mut switched = self.clone();
        let mut any = false;
        for (upgrade, artillery) in &mut switched.artillery {
            let Some(mode) =
                self.upgrade_components(upgrade).first().and_then(|component| self.alternative_fire(component))
            else {
                continue;
            };

            any = true;
            for gun in &mut artillery.guns {
                gun.shot_delay = Some(mode.full_reload);
                if !mode.ammo.is_empty() {
                    gun.ammo = mode.ammo.iter().map(|ammo| ammo.name.clone()).collect();
                }
            }
        }

        any.then_some(switched)
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

    /// Launcher grouping of a torpedo component, `None` when it does not group
    /// its launchers.
    pub fn torpedo_loading(&self, component: &str) -> Option<&TorpedoLoading> {
        self.torpedo_loading.get(component)
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
        self.fire_controls.get(upgrade_name).map(|stats| stats.max_dist_coef)
    }

    /// Look up the fire-control `sigmaCountCoef` for a given `_Suo` upgrade name.
    pub fn fire_control_sigma_count_coef(&self, upgrade_name: &str) -> Option<f32> {
        self.fire_controls.get(upgrade_name).map(|stats| stats.sigma_count_coef)
    }

    /// Every component a `_Artillery` or `_Torpedoes` upgrade lists.
    pub fn upgrade_components(&self, upgrade_name: &str) -> &[String] {
        self.upgrade_components.get(upgrade_name).map_or(&[], Vec::as_slice)
    }

    /// The component of a `_Artillery` or `_Torpedoes` upgrade that `hull`
    /// mounts: the listed one the hull names as its own or as an alternative.
    /// `None` when the upgrade does not fit the hull.
    pub fn component_on_hull<'a>(
        &'a self,
        upgrade_name: &str,
        hull: &crate::game_params::types::HullUpgradeConfig,
        component_type: crate::game_params::keys::ComponentType,
    ) -> Option<&'a str> {
        self.upgrade_components(upgrade_name).iter().map(String::as_str).find(|name| {
            hull.component_name(component_type) == Some(*name) || hull.alternative_mounts.contains_key(*name)
        })
    }

    /// The stock (base) upgrade selection per slot.
    pub fn stock_selection(&self) -> &super::selection::ShipUpgradeSelection {
        &self.stock_selection
    }
}

impl ArtilleryGunStats {
    /// This gun's dispersion ellipse curve, or `None` if any curve field is absent
    /// (old data). Pairs with `constants::dispersion_ellipse`; returning `Option`
    /// keeps a partially-populated curve from ever being built.
    pub fn dispersion_curve(&self) -> Option<DispersionCurve> {
        Some(DispersionCurve {
            min_radius: self.min_radius?,
            ideal_radius: self.ideal_radius?,
            ideal_distance: self.ideal_distance?,
            radius_on_zero: self.radius_on_zero?,
            radius_on_delim: self.radius_on_delim?,
            radius_on_max: self.radius_on_max?,
            delim: self.delim?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ArtilleryGunStats;

    #[test]
    fn dispersion_curve_fields_default_none_and_hold_values() {
        let stock = ArtilleryGunStats::default();
        assert!(stock.radius_on_zero.is_none());
        assert!(stock.delim.is_none());

        let g = ArtilleryGunStats { radius_on_max: Some(1.8), delim: Some(0.5), ..Default::default() };
        assert_eq!(g.radius_on_max, Some(1.8));
        assert_eq!(g.delim, Some(0.5));
    }

    #[test]
    fn dispersion_curve_is_all_or_nothing() {
        let partial = ArtilleryGunStats {
            min_radius: Some(2.0),
            ideal_radius: Some(12.0),
            ideal_distance: Some(1000.0),
            radius_on_zero: Some(1.0),
            radius_on_delim: Some(1.4),
            radius_on_max: Some(1.8),
            delim: None,
            ..Default::default()
        };
        assert!(partial.dispersion_curve().is_none());

        let full = ArtilleryGunStats { delim: Some(0.5), ..partial };
        let curve = full.dispersion_curve().expect("all fields present");
        assert_eq!(curve.radius_on_max, 1.8);
        assert_eq!(curve.delim, 0.5);
    }
}
