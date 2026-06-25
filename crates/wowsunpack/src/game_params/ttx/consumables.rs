//! Effective (modifier-applied) consumable stats.
//!
//! Transcribes the client's consumable modifier pipeline so consumers get the
//! FINAL consumable stats with an equipped [`ModifierBundle`] applied, the
//! consumable analog of the TTX weapon factories (e.g. `Artillery.reload_time`).
//!
//! The reduction follows `m8074268a/consumables/ConsumableUtils.py`
//! `updateConsumableParams` (lines 113-206) and the `getConsumable*` helpers in
//! `Modifiers/ModifiersApply.py`. Each coefficient cites its deob source. All
//! coefficient names resolve through [`ModifierBundle::coef`] (multiplicative,
//! identity 1.0 when absent) or [`ModifierBundle::apply`] (operator chosen by the
//! modifier's own classification); add-vs-multiply is never hardcoded. The one
//! legitimate default is the absent-modifier identity (a missing coefficient
//! leaves the base value unchanged). A base field that is itself absent maps to an
//! `Option::None` output rather than a fabricated value.

use std::collections::BTreeMap;

use crate::game_params::ttx::constants::BW_TO_BALLISTIC;
use crate::game_params::ttx::constants::KM_TO_M;
use crate::game_params::ttx::model::AmmoCount;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::ttx::provenance::Op;
use crate::game_params::types::AbilityCategory;
use crate::game_params::types::Km;
use crate::game_params::types::Meters;
use crate::recognized::Recognized;

/// One modifier a consumable computation applied, in application order. Owned
/// `String` because the per-type names are `format!`-built (`{type}ReloadCoeff`).
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedConsumableModifier {
    pub name: String,
    pub op: Op,
}

/// The ordered applied-modifier list per consumable stat (parallels the
/// `EffectiveConsumable` fields that can carry modifier steps). A field with no
/// applied modifiers has an empty Vec.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConsumableApplied {
    pub reload_time: Vec<AppliedConsumableModifier>,
    pub work_time: Vec<AppliedConsumableModifier>,
    pub preparation_time: Vec<AppliedConsumableModifier>,
    pub charges: Vec<AppliedConsumableModifier>,
    pub max_capacity: Vec<AppliedConsumableModifier>,
    pub regeneration_hp_speed: Vec<AppliedConsumableModifier>,
    pub smoke_radius: Vec<AppliedConsumableModifier>,
    pub smoke_lifetime: Vec<AppliedConsumableModifier>,
    pub fighters_count: Vec<AppliedConsumableModifier>,
    pub call_fighters_radius: Vec<AppliedConsumableModifier>,
    pub call_fighters_time_delay: Vec<AppliedConsumableModifier>,
    pub call_fighters_time_from_heaven: Vec<AppliedConsumableModifier>,
    pub plane_regeneration_rate: Vec<AppliedConsumableModifier>,
}

/// Consumable group strings, `ConsumableConstants.py` `ConsumableGroup`.
const GROUP_SHIP: &str = "ship";
const GROUP_SQUADRON: &str = "squadron";

/// `lifeCycleType` enum values, `ConsumableConstants.py` `ConsumableLifeCycleType`.
const LIFECYCLE_COUNT_BASED: f32 = 0.0;
const LIFECYCLE_TIME_BASED: f32 = 1.0;

/// `ConsumableNames.REGEN_CREW`, the Repair Party consumable type string.
const TYPE_REGEN_CREW: &str = "regenCrew";

/// `ConsumableNames.SMOKE_GENERATOR` (`ConsumableConstants.py:162`).
const TYPE_SMOKE_GENERATOR: &str = "smokeGenerator";
/// `ConsumableNames.PLANE_SMOKE_GENERATOR` (`ConsumableConstants.py:205`).
const TYPE_PLANE_SMOKE_GENERATOR: &str = "planeSmokeGenerator";
/// `ConsumableNames.FIGHTER` (`ConsumableConstants.py:164`).
const TYPE_FIGHTER: &str = "fighter";
/// `ConsumableNames.REGENERATE_HEALTH` (`ConsumableConstants.py:188`).
const TYPE_REGENERATE_HEALTH: &str = "regenerateHealth";

/// `CallFightersConsumables` (`ConsumableConstants.py:226`), the set whose members
/// take the `callFighters*` per-type effect modifiers (ConsumableUtils.py:149-152).
const CALL_FIGHTERS_CONSUMABLES: &[&str] = &["callFighters", "planeTacticalFighters"];

/// `ConsumablesWithReloadCoefficients` (`ConsumableConstants.py`). Membership gates
/// the per-type `<typeName>ReloadCoeff` multiplier in `getConsumableReloadTime`
/// (ModifiersApply.py:200-201, 209-210).
const CONSUMABLES_WITH_RELOAD_COEFFICIENTS: &[&str] = &[
    "artilleryBoosters",
    "torpedoReloader",
    "crashCrew",
    "airDefenseDisp",
    "scout",
    "fighter",
    "sonar",
    "rls",
    "smokeGenerator",
    "speedBoosters",
    "regenCrew",
    "healForsage",
    "regenerateHealth",
    "planeSmokeGenerator",
    "hydrophone",
    "submarineLocator",
    "activeManeuvering",
];

/// `ConsumablesWithWorkTimeCoefficients` (`ConsumableConstants.py`). Gates the
/// per-type `<typeName>WorkTimeCoeff` multiplier in `getConsumableWorkTime`
/// (ModifiersApply.py:225-226, 234-235).
const CONSUMABLES_WITH_WORK_TIME_COEFFICIENTS: &[&str] = &[
    "speedBoosters",
    "smokeGenerator",
    "scout",
    "crashCrew",
    "regenCrew",
    "airDefenseDisp",
    "sonar",
    "rls",
    "fighter",
    "regenerateHealth",
    "callFighters",
    "planeSmokeGenerator",
    "subsEnergyFreeze",
    "fastRudders",
    "submarineLocator",
    "planeTacticalFighters",
    "activeManeuvering",
];

/// `ConsumablesWithCapacityCoefficients` (`ConsumableConstants.py`). Gates the
/// per-type `<typeName>CapacityCoeff` multiplier in `getConsumableCapacity`
/// (ModifiersApply.py:249-250, 258-259).
const CONSUMABLES_WITH_CAPACITY_COEFFICIENTS: &[&str] = &[
    "crashCrew",
    "regenCrew",
    "smokeGenerator",
    "planeSmokeGenerator",
    "speedBoosters",
    "sonar",
    "rls",
    "artilleryBoosters",
    "regenerateHealth",
    "subsEnergyFreeze",
    "scout",
    "fighter",
    "callFighters",
];

/// `AdditionalConsumablesCount` (`ConsumableConstants.py`). Gates the per-type
/// `<typeName>AdditionalConsumables` additive count in `getAdditionalConsumablesCount`
/// (ModifiersApply.py:277-278).
const ADDITIONAL_CONSUMABLES_COUNT: &[&str] = &[
    "crashCrew",
    "regenCrew",
    "regenerateHealth",
    "callFighters",
    "scout",
    "torpedoReloader",
    "smokeGenerator",
    "planeTacticalFighters",
    "activeManeuvering",
];

/// Final, modifier-applied consumable stats. Each field is the base value after the
/// equipped [`ModifierBundle`] has been folded in. Fields that are not applicable to
/// a given consumable (or whose base value is absent) are `None`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EffectiveConsumable {
    /// Cooldown between charges, `reloadTime * getConsumableReloadTime`
    /// (ConsumableUtils.py:115).
    pub reload_time: Seconds,
    /// Active duration. `Some` for COUNT_BASED consumables (`workTime`,
    /// ConsumableUtils.py:120); `None` for TIME_BASED consumables, which are governed
    /// by `max_capacity`/`min_work_time` instead.
    pub work_time: Option<Seconds>,
    /// Activation delay, `preparationTime * getConsumableReloadTime`
    /// (ConsumableUtils.py:116).
    pub preparation_time: Seconds,
    /// Resource pool. COUNT_BASED uses `numConsumables` plus the additional-count
    /// bonuses (ConsumableUtils.py:122-128); `-1` means an unlimited pool, modeled as
    /// [`AmmoCount::Infinite`] and never made finite by modifiers.
    pub charges: AmmoCount,
    /// TIME_BASED capacity pool, `maxCapacity * getConsumableCapacity`
    /// (ConsumableUtils.py:166-167). `None` for COUNT_BASED consumables.
    pub max_capacity: Option<f32>,
    /// Detection radius in meters (radar / hydro / sublocator). Read from the base
    /// category (`distShip` / `hydrophoneWaveRadius`); no consumable detection-radius
    /// modifier exists in `updateConsumableParams`, so this is the base value.
    pub detection_radius: Option<Meters>,
    /// Repair Party heal rate as a fraction of max HP per second,
    /// `regenerationHPSpeed * regenerationHPSpeed` modifier (ConsumableUtils.py:140).
    /// `None` for non-heal consumables.
    pub regeneration_hp_speed: Option<f32>,
    /// Smoke screen radius, `logic.radius *= smokeScreenRadiusCoefficient`
    /// (ConsumableUtils.py:134). KILOMETER measure with `BW_TO_BALLISTIC / KM_TO_M`
    /// scale (MODIFIER_SETTINGS `radius`). `Some` only for smoke generators with a base
    /// `radius`.
    pub smoke_radius: Option<Km>,
    /// Smoke screen lifetime, `logic.lifeTime *= smokeGeneratorLifeTime` (ship) /
    /// `planeSmokeGeneratorLifeTime` (plane) (ConsumableUtils.py:133/137). SECOND
    /// measure, multiplier 1.0 (MODIFIER_SETTINGS `lifeTime`). `Some` only for smoke
    /// generators with a base `lifeTime`.
    pub smoke_lifetime: Option<Seconds>,
    /// Fighter count, `logic.fightersNum += extraFighterCount` (ConsumableUtils.py:147,
    /// additive). NONE measure (a raw count). `Some` only for the `fighter` type with a
    /// base `fightersNum`.
    pub fighters_count: Option<f32>,
    /// Call-fighters patrol radius, `logic.radius *= callFightersRadiusCoeff`
    /// (ConsumableUtils.py:150). KILOMETER measure with `BW_TO_BALLISTIC / KM_TO_M`
    /// scale. `Some` only for `CallFightersConsumables` types with a base `radius`.
    pub call_fighters_radius: Option<Km>,
    /// Call-fighters attack delay, `logic.timeDelayAttack *= callFightersTimeDelayAttack`
    /// (ConsumableUtils.py:151). Seconds, multiplier 1.0 (no MODIFIER_SETTINGS display
    /// override). `Some` only for `CallFightersConsumables` types with a base value.
    pub call_fighters_time_delay: Option<Seconds>,
    /// Call-fighters appearance delay, `logic.timeFromHeaven *= callFightersAppearDelay`
    /// (ConsumableUtils.py:152). Seconds, multiplier 1.0. `Some` only for
    /// `CallFightersConsumables` types with a base value.
    pub call_fighters_time_from_heaven: Option<Seconds>,
    /// Plane heal rate, `logic.regenerationRate *= planeRegenerationRate`
    /// (ConsumableUtils.py:144). Raw rate (PERCENT measure, multiplier 1.0). `Some` only
    /// for the `regenerateHealth` type with a base `regenerationRate`.
    pub plane_regeneration_rate: Option<f32>,
}

/// One consumable the ship can mount, with its modifier-folded stats.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConsumableCard {
    /// Consumable identity (known or unknown), from `AbilityCategory::consumable_type`.
    pub consumable: Recognized<crate::game_types::Consumable>,
    /// Row-qualifier label: the raw consumable type, disambiguated "type (2)" only
    /// if two cards share a type. Stored so recorder and rows() use one qualifier.
    pub label: String,
    pub stats: EffectiveConsumable,
}

/// Multiply `base` by `bundle.coef(name)` only when the consumable `type_name` is in
/// `gate`; otherwise return `base` unchanged. Mirrors the client's
/// `if type in <Set>: coeff *= getattr(modifier, type + '<Suffix>')`.
fn gated_type_coef(
    bundle: &ModifierBundle,
    base: f32,
    type_name: &str,
    gate: &[&str],
    suffix: &str,
    out: &mut Vec<AppliedConsumableModifier>,
) -> f32 {
    if gate.contains(&type_name) {
        let name = format!("{type_name}{suffix}");
        out.push(AppliedConsumableModifier { name: name.clone(), op: Op::Mul });
        base * bundle.coef(&name)
    } else {
        base
    }
}

/// `getConsumableReloadTime` (ModifiersApply.py:190-214), `slotID=None`.
///
/// `coeff = allConsumableReloadTime`; SHIP `*= ConsumableReloadTime`, SQUADRON
/// `*= planeConsumableReloadTime`; then `*= <typeName>ReloadCoeff` when the type is
/// in `ConsumablesWithReloadCoefficients`.
fn consumable_reload_coeff(
    bundle: &ModifierBundle,
    type_name: &str,
    group: &str,
    out: &mut Vec<AppliedConsumableModifier>,
) -> f32 {
    // ModifiersApply.py:191
    let mut coeff = bundle.coef("allConsumableReloadTime");
    out.push(AppliedConsumableModifier { name: "allConsumableReloadTime".into(), op: Op::Mul });
    match group {
        GROUP_SHIP => {
            coeff *= bundle.coef("ConsumableReloadTime"); // :197
            out.push(AppliedConsumableModifier { name: "ConsumableReloadTime".into(), op: Op::Mul });
        }
        GROUP_SQUADRON => {
            coeff *= bundle.coef("planeConsumableReloadTime"); // :206
            out.push(AppliedConsumableModifier { name: "planeConsumableReloadTime".into(), op: Op::Mul });
        }
        _ => {}
    }
    // :200-201 / :209-210
    gated_type_coef(bundle, coeff, type_name, CONSUMABLES_WITH_RELOAD_COEFFICIENTS, "ReloadCoeff", out)
}

/// `getConsumableWorkTime` (ModifiersApply.py:217-239), `slotID=None`.
///
/// SHIP `*= ConsumablesWorkTime`, SQUADRON `*= planeConsumablesWorkTime`; then
/// `*= <typeName>WorkTimeCoeff` when the type is in
/// `ConsumablesWithWorkTimeCoefficients`.
fn consumable_work_time_coeff(
    bundle: &ModifierBundle,
    type_name: &str,
    group: &str,
    out: &mut Vec<AppliedConsumableModifier>,
) -> f32 {
    let mut coeff = 1.0;
    match group {
        GROUP_SHIP => {
            coeff *= bundle.coef("ConsumablesWorkTime"); // :222
            out.push(AppliedConsumableModifier { name: "ConsumablesWorkTime".into(), op: Op::Mul });
        }
        GROUP_SQUADRON => {
            coeff *= bundle.coef("planeConsumablesWorkTime"); // :231
            out.push(AppliedConsumableModifier { name: "planeConsumablesWorkTime".into(), op: Op::Mul });
        }
        _ => {}
    }
    // :225-226 / :234-235
    gated_type_coef(bundle, coeff, type_name, CONSUMABLES_WITH_WORK_TIME_COEFFICIENTS, "WorkTimeCoeff", out)
}

/// `getConsumableCapacity` (ModifiersApply.py:242-263).
///
/// `coeff = consumableCapacityCoeff`; SHIP `*= shipConsumableCapacityCoeff`,
/// SQUADRON `*= squadronConsumableCapacityCoeff`; then `*= <typeName>CapacityCoeff`
/// when the type is in `ConsumablesWithCapacityCoefficients`.
fn consumable_capacity_coeff(
    bundle: &ModifierBundle,
    type_name: &str,
    group: &str,
    out: &mut Vec<AppliedConsumableModifier>,
) -> f32 {
    let mut coeff = bundle.coef("consumableCapacityCoeff"); // :243
    out.push(AppliedConsumableModifier { name: "consumableCapacityCoeff".into(), op: Op::Mul });
    match group {
        GROUP_SHIP => {
            coeff *= bundle.coef("shipConsumableCapacityCoeff"); // :246
            out.push(AppliedConsumableModifier { name: "shipConsumableCapacityCoeff".into(), op: Op::Mul });
        }
        GROUP_SQUADRON => {
            coeff *= bundle.coef("squadronConsumableCapacityCoeff"); // :255
            out.push(AppliedConsumableModifier { name: "squadronConsumableCapacityCoeff".into(), op: Op::Mul });
        }
        _ => {}
    }
    // :249-250 / :258-259
    gated_type_coef(bundle, coeff, type_name, CONSUMABLES_WITH_CAPACITY_COEFFICIENTS, "CapacityCoeff", out)
}

/// `getAdditionalConsumablesCount` (ModifiersApply.py:274-280): the additive
/// per-type `<typeName>AdditionalConsumables` bonus (0 when the type is not in
/// `AdditionalConsumablesCount`).
fn additional_consumables_count(
    bundle: &ModifierBundle,
    type_name: &str,
    out: &mut Vec<AppliedConsumableModifier>,
) -> f32 {
    if ADDITIONAL_CONSUMABLES_COUNT.contains(&type_name) {
        // The bonus name is additive (base 0.0); read it via apply onto 0.0.
        let name = format!("{type_name}AdditionalConsumables");
        out.push(AppliedConsumableModifier { name: name.clone(), op: Op::Add });
        bundle.apply(0.0, &name)
    } else {
        0.0
    }
}

/// `getAdditionalConsumablesCountForGroup` (ModifiersApply.py:283-290): the additive
/// group-wide bonus, `additionalConsumables` (SHIP) or `planeAdditionalConsumables`
/// (SQUADRON).
fn additional_consumables_for_group(
    bundle: &ModifierBundle,
    group: &str,
    out: &mut Vec<AppliedConsumableModifier>,
) -> f32 {
    match group {
        GROUP_SHIP => {
            out.push(AppliedConsumableModifier { name: "additionalConsumables".into(), op: Op::Add });
            bundle.apply(0.0, "additionalConsumables") // :285
        }
        GROUP_SQUADRON => {
            out.push(AppliedConsumableModifier { name: "planeAdditionalConsumables".into(), op: Op::Add });
            bundle.apply(0.0, "planeAdditionalConsumables") // :288
        }
        _ => 0.0,
    }
}

/// Read a numeric field from the category's merged effect-field map.
fn field(fields: &BTreeMap<String, f32>, name: &str) -> Option<f32> {
    fields.get(name).copied()
}

/// Convert a BigWorld ballistic `radius` into kilometers, MODIFIER_SETTINGS `radius`
/// uses `Measures.KILOMETER` with scale `BW_TO_BALLISTIC / KM_TO_M`
/// (`mbf4783af/ModifierSettings.py`). The multiplicative effect modifier commutes with
/// this scale, so the conversion order is irrelevant.
fn km_from_ballistic(radius: f32) -> Km {
    Km::from(radius * (BW_TO_BALLISTIC / KM_TO_M))
}

/// Compute the final, modifier-applied stats for `category` under `modifiers`.
///
/// Returns the effective stats and the ordered applied-modifier lists per stat.
/// An empty bundle yields the base values unchanged and empty applied lists.
pub fn effective_consumable(
    category: &AbilityCategory,
    modifiers: &ModifierBundle,
) -> (EffectiveConsumable, ConsumableApplied) {
    let type_name = category.consumable_type_raw();
    let group = category.group();
    let fields = category.effect_fields();

    let mut applied = ConsumableApplied::default();

    // reloadTime / preparationTime both scale by getConsumableReloadTime
    // (ConsumableUtils.py:115-116). reload_time and preparation_time share the same
    // coefficient steps; record once and clone for preparation_time.
    let reload_coeff = consumable_reload_coeff(modifiers, type_name, group, &mut applied.reload_time);
    applied.preparation_time = applied.reload_time.clone();
    let reload_time = Seconds::from(category.reload_time() * reload_coeff);
    let preparation_time = Seconds::from(category.preparation_time() * reload_coeff);

    let lifecycle = field(fields, "lifeCycleType").unwrap_or(LIFECYCLE_COUNT_BASED);

    // workTime is multiplied only for COUNT_BASED consumables (ConsumableUtils.py:120);
    // TIME_BASED consumables have no port workTime stat (capacity/minWorkTime govern).
    let work_time = if lifecycle == LIFECYCLE_COUNT_BASED {
        let wt = consumable_work_time_coeff(modifiers, type_name, group, &mut applied.work_time);
        Some(Seconds::from(category.work_time() * wt))
    } else {
        None
    };

    // charges: COUNT_BASED applies numConsumables + additional counts
    // (ConsumableUtils.py:122-128). A base of -1 is an unlimited pool and stays
    // Infinite regardless of modifiers.
    let base_count = category.num_consumables();
    let charges = if base_count < 0 {
        AmmoCount::Infinite
    } else if lifecycle == LIFECYCLE_COUNT_BASED {
        let added = additional_consumables_count(modifiers, type_name, &mut applied.charges)
            + additional_consumables_for_group(modifiers, group, &mut applied.charges);
        // ConsumableUtils.py:128 `max(0, numConsumables + added)`
        let total = (base_count as f32 + added).max(0.0);
        AmmoCount::Finite(total.round() as u32)
    } else {
        AmmoCount::Finite(base_count as u32)
    };

    // maxCapacity for TIME_BASED consumables (ConsumableUtils.py:166-167). Absent for
    // COUNT_BASED.
    let max_capacity = if lifecycle == LIFECYCLE_TIME_BASED {
        field(fields, "maxCapacity")
            .filter(|&c| c >= 0.0)
            .map(|c| c * consumable_capacity_coeff(modifiers, type_name, group, &mut applied.max_capacity))
    } else {
        None
    };

    // regenerationHPSpeed: REGEN_CREW only, `*= regenerationHPSpeed` modifier
    // (ConsumableUtils.py:140). The base is read off the typed accessor.
    let regeneration_hp_speed = if type_name == TYPE_REGEN_CREW {
        category.regeneration_hp_speed().map(|v| {
            applied
                .regeneration_hp_speed
                .push(AppliedConsumableModifier { name: "regenerationHPSpeed".into(), op: Op::Mul });
            v * modifiers.coef("regenerationHPSpeed")
        })
    } else {
        None
    };

    // detection_radius has no modifier in updateConsumableParams; the base value is
    // final.
    let detection_radius = category.detection_radius();

    // Per-type effect fields (ConsumableUtils.py:132-152, identical in both the
    // COUNT_BASED and TIME_BASED branches). Each reads its base off effect_fields() and
    // applies the per-type modifier via bundle.apply (operator picked by the modifier's
    // own classification); an absent base field yields None.
    let mut smoke_radius = None;
    let mut smoke_lifetime = None;
    let mut fighters_count = None;
    let mut call_fighters_radius = None;
    let mut call_fighters_time_delay = None;
    let mut call_fighters_time_from_heaven = None;
    let mut plane_regeneration_rate = None;

    if type_name == TYPE_SMOKE_GENERATOR {
        // :133 lifeTime *= smokeGeneratorLifeTime; :134 radius *= smokeScreenRadiusCoefficient
        smoke_lifetime = field(fields, "lifeTime").map(|v| {
            applied
                .smoke_lifetime
                .push(AppliedConsumableModifier { name: "smokeGeneratorLifeTime".into(), op: Op::Mul });
            Seconds::from(modifiers.apply(v, "smokeGeneratorLifeTime"))
        });
        smoke_radius = field(fields, "radius").map(|v| {
            applied
                .smoke_radius
                .push(AppliedConsumableModifier { name: "smokeScreenRadiusCoefficient".into(), op: Op::Mul });
            km_from_ballistic(modifiers.apply(v, "smokeScreenRadiusCoefficient"))
        });
    } else if type_name == TYPE_PLANE_SMOKE_GENERATOR {
        // :137 lifeTime *= planeSmokeGeneratorLifeTime (plane smoke has no radius modifier)
        smoke_lifetime = field(fields, "lifeTime").map(|v| {
            applied
                .smoke_lifetime
                .push(AppliedConsumableModifier { name: "planeSmokeGeneratorLifeTime".into(), op: Op::Mul });
            Seconds::from(modifiers.apply(v, "planeSmokeGeneratorLifeTime"))
        });
    } else if type_name == TYPE_REGENERATE_HEALTH {
        // :144 regenerationRate *= planeRegenerationRate
        plane_regeneration_rate = field(fields, "regenerationRate").map(|v| {
            applied
                .plane_regeneration_rate
                .push(AppliedConsumableModifier { name: "planeRegenerationRate".into(), op: Op::Mul });
            modifiers.apply(v, "planeRegenerationRate")
        });
    } else if type_name == TYPE_FIGHTER {
        // :147 fightersNum += extraFighterCount (additive, base 0.0)
        fighters_count = field(fields, "fightersNum").map(|v| {
            applied.fighters_count.push(AppliedConsumableModifier { name: "extraFighterCount".into(), op: Op::Add });
            modifiers.apply(v, "extraFighterCount")
        });
    } else if CALL_FIGHTERS_CONSUMABLES.contains(&type_name) {
        // :150 radius *= callFightersRadiusCoeff
        call_fighters_radius = field(fields, "radius").map(|v| {
            applied
                .call_fighters_radius
                .push(AppliedConsumableModifier { name: "callFightersRadiusCoeff".into(), op: Op::Mul });
            km_from_ballistic(modifiers.apply(v, "callFightersRadiusCoeff"))
        });
        // :151 timeDelayAttack *= callFightersTimeDelayAttack
        call_fighters_time_delay = field(fields, "timeDelayAttack").map(|v| {
            applied
                .call_fighters_time_delay
                .push(AppliedConsumableModifier { name: "callFightersTimeDelayAttack".into(), op: Op::Mul });
            Seconds::from(modifiers.apply(v, "callFightersTimeDelayAttack"))
        });
        // :152 timeFromHeaven *= callFightersAppearDelay
        call_fighters_time_from_heaven = field(fields, "timeFromHeaven").map(|v| {
            applied
                .call_fighters_time_from_heaven
                .push(AppliedConsumableModifier { name: "callFightersAppearDelay".into(), op: Op::Mul });
            Seconds::from(modifiers.apply(v, "callFightersAppearDelay"))
        });
    }

    (
        EffectiveConsumable {
            reload_time,
            work_time,
            preparation_time,
            charges,
            max_capacity,
            detection_radius,
            regeneration_hp_speed,
            smoke_radius,
            smoke_lifetime,
            fighters_count,
            call_fighters_radius,
            call_fighters_time_delay,
            call_fighters_time_from_heaven,
            plane_regeneration_rate,
        },
        applied,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::AbilityCategory;
    use crate::game_params::types::CrewSkillModifier;
    use crate::game_params::types::Species;

    /// The version at which the toolkit's `MODIFIER_SETTINGS` table takes effect.
    const VERSION: crate::data::Version = crate::data::Version::base(15, 0, 0);

    /// Build a Damage Control Party (`crashCrew`) category from the real GameParams
    /// values (`PCY001_CrashCrew`, queried from G:\wows_dump\GameParams.json):
    /// consumableType "crashCrew", group "ship", reloadTime 120, preparationTime 0,
    /// numConsumables -1, workTime 15, COUNT_BASED.
    fn crash_crew() -> AbilityCategory {
        let mut fields = BTreeMap::new();
        fields.insert("lifeCycleType".to_string(), LIFECYCLE_COUNT_BASED);
        fields.insert("reloadTime".to_string(), 120.0);
        fields.insert("workTime".to_string(), 15.0);
        AbilityCategory::builder()
            .consumable_type("crashCrew".to_string())
            .group("ship".to_string())
            .icon_id(String::new())
            .num_consumables(-1)
            .preparation_time(0.0)
            .reload_time(120.0)
            .work_time(15.0)
            .effect_fields(fields)
            .build()
    }

    /// A finite-charge ship consumable (`sonar`, COUNT_BASED) for charge tests:
    /// numConsumables 3, reloadTime 90, workTime 100.
    fn finite_sonar() -> AbilityCategory {
        let mut fields = BTreeMap::new();
        fields.insert("lifeCycleType".to_string(), LIFECYCLE_COUNT_BASED);
        AbilityCategory::builder()
            .consumable_type("sonar".to_string())
            .group("ship".to_string())
            .icon_id(String::new())
            .num_consumables(3)
            .preparation_time(0.0)
            .reload_time(90.0)
            .work_time(100.0)
            .effect_fields(fields)
            .build()
    }

    /// A Smoke Generator (`smokeGenerator`, COUNT_BASED) with the real
    /// `PCY007_SmokeGenerator` logic values (jaq-verified on GameParams.json): logic
    /// `radius` 15.0, `lifeTime` 77.0.
    fn smoke_generator() -> AbilityCategory {
        let mut fields = BTreeMap::new();
        fields.insert("lifeCycleType".to_string(), LIFECYCLE_COUNT_BASED);
        fields.insert("radius".to_string(), 15.0);
        fields.insert("lifeTime".to_string(), 77.0);
        AbilityCategory::builder()
            .consumable_type("smokeGenerator".to_string())
            .group("ship".to_string())
            .icon_id(String::new())
            .num_consumables(2)
            .preparation_time(0.0)
            .reload_time(240.0)
            .work_time(20.0)
            .effect_fields(fields)
            .build()
    }

    /// A Call Fighters (`callFighters`, SQUADRON) with the real logic values: `radius`
    /// 116.667, `timeDelayAttack` 5.0, `timeFromHeaven` 3.0.
    fn call_fighters() -> AbilityCategory {
        let mut fields = BTreeMap::new();
        fields.insert("lifeCycleType".to_string(), LIFECYCLE_COUNT_BASED);
        fields.insert("radius".to_string(), 116.667);
        fields.insert("timeDelayAttack".to_string(), 5.0);
        fields.insert("timeFromHeaven".to_string(), 3.0);
        AbilityCategory::builder()
            .consumable_type("callFighters".to_string())
            .group("squadron".to_string())
            .icon_id(String::new())
            .num_consumables(-1)
            .preparation_time(0.0)
            .reload_time(60.0)
            .work_time(60.0)
            .effect_fields(fields)
            .build()
    }

    /// A Fighter (`fighter`, COUNT_BASED) with the real `fightersNum` 1.
    fn fighter() -> AbilityCategory {
        let mut fields = BTreeMap::new();
        fields.insert("lifeCycleType".to_string(), LIFECYCLE_COUNT_BASED);
        fields.insert("fightersNum".to_string(), 1.0);
        AbilityCategory::builder()
            .consumable_type("fighter".to_string())
            .group("ship".to_string())
            .icon_id(String::new())
            .num_consumables(3)
            .preparation_time(0.0)
            .reload_time(90.0)
            .work_time(60.0)
            .effect_fields(fields)
            .build()
    }

    /// A plane Regenerate Health (`regenerateHealth`, SQUADRON) with the real
    /// `regenerationRate` 0.1.
    fn regenerate_health() -> AbilityCategory {
        let mut fields = BTreeMap::new();
        fields.insert("lifeCycleType".to_string(), LIFECYCLE_COUNT_BASED);
        fields.insert("regenerationRate".to_string(), 0.1);
        AbilityCategory::builder()
            .consumable_type("regenerateHealth".to_string())
            .group("squadron".to_string())
            .icon_id(String::new())
            .num_consumables(-1)
            .preparation_time(0.0)
            .reload_time(60.0)
            .work_time(60.0)
            .effect_fields(fields)
            .build()
    }

    fn modifier(name: &str, value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_string())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }

    /// An empty bundle leaves every base value unchanged.
    #[test]
    fn empty_bundle_yields_base_values() {
        let cat = crash_crew();
        let (eff, _applied) = effective_consumable(&cat, &ModifierBundle::empty(Species::Battleship));
        assert_eq!(eff.reload_time, Seconds::from(120.0));
        assert_eq!(eff.preparation_time, Seconds::from(0.0));
        assert_eq!(eff.work_time, Some(Seconds::from(15.0)));
        assert_eq!(eff.charges, AmmoCount::Infinite);
        assert_eq!(eff.max_capacity, None);
    }

    /// An equipped `ConsumableReloadTime` coefficient (real modifier name, base 1.0,
    /// multiplicative) scales reload by its value: 120 * 0.9 = 108.
    #[test]
    fn reload_modifier_scales_reload_time() {
        let cat = crash_crew();
        let mods = [modifier("ConsumableReloadTime", 0.9)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert!((eff.reload_time.value() - 108.0).abs() < 1e-3, "got {}", eff.reload_time.value());
    }

    /// The `allConsumableReloadTime` and `ConsumableReloadTime` coefficients compound:
    /// 120 * 0.9 * 0.95 = 102.6.
    #[test]
    fn reload_coefficients_compound() {
        let cat = crash_crew();
        let mods = [modifier("ConsumableReloadTime", 0.9), modifier("allConsumableReloadTime", 0.95)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert!((eff.reload_time.value() - 102.6).abs() < 1e-3, "got {}", eff.reload_time.value());
    }

    /// A `numConsumables = -1` base stays Infinite even with an additional-count
    /// modifier equipped (modifiers never make an unlimited pool finite).
    #[test]
    fn infinite_charges_stay_infinite() {
        let cat = crash_crew();
        let mods = [modifier("crashCrewAdditionalConsumables", 1.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert_eq!(eff.charges, AmmoCount::Infinite);
    }

    /// A finite base count gains the additive per-type and group bonuses: 3 + 1
    /// (sonar has no per-type AdditionalConsumables, so only the group-wide
    /// `additionalConsumables` applies) = 4.
    #[test]
    fn finite_charges_add_group_bonus() {
        let cat = finite_sonar();
        let mods = [modifier("additionalConsumables", 1.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert_eq!(eff.charges, AmmoCount::Finite(4));
    }

    /// A finite base count with no modifiers is unchanged.
    #[test]
    fn finite_charges_base_unchanged() {
        let cat = finite_sonar();
        let (eff, _applied) = effective_consumable(&cat, &ModifierBundle::empty(Species::Cruiser));
        assert_eq!(eff.charges, AmmoCount::Finite(3));
    }

    /// The per-type `sonarWorkTimeCoeff` gate fires for `sonar`: workTime 100 * 0.8
    /// (ConsumablesWorkTime is absent -> 1.0, sonarWorkTimeCoeff 0.8) ... here only
    /// the per-type coeff is equipped, so 100 * 0.8 = 80.
    #[test]
    fn per_type_work_time_coeff_applies() {
        let cat = finite_sonar();
        let mods = [modifier("sonarWorkTimeCoeff", 0.8)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert_eq!(eff.work_time, Some(Seconds::from(80.0)));
    }

    /// Smoke generator base: radius 15 -> 0.45 km (15 * 30/1000), lifeTime 77 -> 77 s,
    /// empty bundle leaves both at the converted base. Other per-type fields are None.
    #[test]
    fn smoke_effects_base() {
        let cat = smoke_generator();
        let (eff, _applied) = effective_consumable(&cat, &ModifierBundle::empty(Species::Cruiser));
        assert_eq!(eff.smoke_radius, Some(Km::from(0.45)));
        assert_eq!(eff.smoke_lifetime, Some(Seconds::from(77.0)));
        assert_eq!(eff.call_fighters_radius, None);
        assert_eq!(eff.fighters_count, None);
        assert_eq!(eff.plane_regeneration_rate, None);
    }

    /// `smokeScreenRadiusCoefficient` 1.2 (real modifier, base 1.0, multiplicative)
    /// scales radius: 15 * 1.2 * 30/1000 = 0.54 km.
    #[test]
    fn smoke_radius_modifier_applies() {
        let cat = smoke_generator();
        let mods = [modifier("smokeScreenRadiusCoefficient", 1.2)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Cruiser, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert!((eff.smoke_radius.unwrap().value() - 0.54).abs() < 1e-4, "got {}", eff.smoke_radius.unwrap().value());
    }

    /// Call fighters base: radius 116.667 -> 3.5 km (116.667 * 30/1000), timeDelayAttack
    /// 5 s, timeFromHeaven 3 s, empty bundle. Smoke/fighter/regen fields are None.
    #[test]
    fn call_fighters_effects_base() {
        let cat = call_fighters();
        let (eff, _applied) = effective_consumable(&cat, &ModifierBundle::empty(Species::AirCarrier));
        assert!(
            (eff.call_fighters_radius.unwrap().value() - 3.5).abs() < 1e-3,
            "got {}",
            eff.call_fighters_radius.unwrap().value()
        );
        assert_eq!(eff.call_fighters_time_delay, Some(Seconds::from(5.0)));
        assert_eq!(eff.call_fighters_time_from_heaven, Some(Seconds::from(3.0)));
        assert_eq!(eff.smoke_radius, None);
        assert_eq!(eff.fighters_count, None);
    }

    /// `callFightersTimeDelayAttack` 0.8 (real modifier, base 1.0, multiplicative)
    /// scales timeDelayAttack: 5 * 0.8 = 4 s.
    #[test]
    fn call_fighters_time_modifier_applies() {
        let cat = call_fighters();
        let mods = [modifier("callFightersTimeDelayAttack", 0.8)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::AirCarrier, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert_eq!(eff.call_fighters_time_delay, Some(Seconds::from(4.0)));
    }

    /// `extraFighterCount` 1 (real modifier, base 0.0, ADDITIVE) adds to fightersNum:
    /// 1 + 1 = 2.
    #[test]
    fn fighter_count_additive_modifier() {
        let cat = fighter();
        let mods = [modifier("extraFighterCount", 1.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Cruiser, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert_eq!(eff.fighters_count, Some(2.0));
        assert_eq!(eff.smoke_radius, None);
    }

    /// Fighter base with no modifiers leaves fightersNum at 1.
    #[test]
    fn fighter_count_base() {
        let cat = fighter();
        let (eff, _applied) = effective_consumable(&cat, &ModifierBundle::empty(Species::Cruiser));
        assert_eq!(eff.fighters_count, Some(1.0));
    }

    /// `planeRegenerationRate` 1.5 (real modifier, base 1.0, multiplicative) scales
    /// regenerationRate: 0.1 * 1.5 = 0.15.
    #[test]
    fn plane_regen_rate_modifier_applies() {
        let cat = regenerate_health();
        let mods = [modifier("planeRegenerationRate", 1.5)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::AirCarrier, VERSION).expect("test modifiers are all known");
        let (eff, _applied) = effective_consumable(&cat, &bundle);
        assert!(
            (eff.plane_regeneration_rate.unwrap() - 0.15).abs() < 1e-6,
            "got {}",
            eff.plane_regeneration_rate.unwrap()
        );
    }

    /// A non-matching consumable (`crashCrew`) has all the new per-type effect fields
    /// None (no base fields, no matching type gate).
    #[test]
    fn non_matching_consumable_has_no_effect_fields() {
        let cat = crash_crew();
        let (eff, _applied) = effective_consumable(&cat, &ModifierBundle::empty(Species::Battleship));
        assert_eq!(eff.smoke_radius, None);
        assert_eq!(eff.smoke_lifetime, None);
        assert_eq!(eff.fighters_count, None);
        assert_eq!(eff.call_fighters_radius, None);
        assert_eq!(eff.call_fighters_time_delay, None);
        assert_eq!(eff.call_fighters_time_from_heaven, None);
        assert_eq!(eff.plane_regeneration_rate, None);
    }

    /// `applied.reload_time` contains the expected names in order when reload
    /// modifiers are in the bundle: `allConsumableReloadTime` (always), then
    /// `ConsumableReloadTime` (SHIP group), then `crashCrewReloadCoeff` (crashCrew
    /// is in CONSUMABLES_WITH_RELOAD_COEFFICIENTS).
    #[test]
    fn applied_reload_time_names_in_order() {
        let cat = crash_crew();
        let mods = [modifier("ConsumableReloadTime", 0.9), modifier("allConsumableReloadTime", 0.95)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let (_eff, applied) = effective_consumable(&cat, &bundle);
        let names: Vec<&str> = applied.reload_time.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["allConsumableReloadTime", "ConsumableReloadTime", "crashCrewReloadCoeff"]);
        assert!(applied.reload_time.iter().all(|m| m.op == Op::Mul));
    }
}
