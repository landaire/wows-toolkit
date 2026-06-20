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

use crate::game_params::ttx::model::AmmoCount;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::types::AbilityCategory;
use crate::game_params::types::Meters;

/// Consumable group strings, `ConsumableConstants.py` `ConsumableGroup`.
const GROUP_SHIP: &str = "ship";
const GROUP_SQUADRON: &str = "squadron";

/// `lifeCycleType` enum values, `ConsumableConstants.py` `ConsumableLifeCycleType`.
const LIFECYCLE_COUNT_BASED: f32 = 0.0;
const LIFECYCLE_TIME_BASED: f32 = 1.0;

/// `ConsumableNames.REGEN_CREW`, the Repair Party consumable type string.
const TYPE_REGEN_CREW: &str = "regenCrew";

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
}

/// Multiply `base` by `bundle.coef(name)` only when the consumable `type_name` is in
/// `gate`; otherwise return `base` unchanged. Mirrors the client's
/// `if type in <Set>: coeff *= getattr(modifier, type + '<Suffix>')`.
fn gated_type_coef(bundle: &ModifierBundle, base: f32, type_name: &str, gate: &[&str], suffix: &str) -> f32 {
    if gate.contains(&type_name) {
        base * bundle.coef(&format!("{type_name}{suffix}"))
    } else {
        base
    }
}

/// `getConsumableReloadTime` (ModifiersApply.py:190-214), `slotID=None`.
///
/// `coeff = allConsumableReloadTime`; SHIP `*= ConsumableReloadTime`, SQUADRON
/// `*= planeConsumableReloadTime`; then `*= <typeName>ReloadCoeff` when the type is
/// in `ConsumablesWithReloadCoefficients`.
fn consumable_reload_coeff(bundle: &ModifierBundle, type_name: &str, group: &str) -> f32 {
    // ModifiersApply.py:191
    let mut coeff = bundle.coef("allConsumableReloadTime");
    match group {
        GROUP_SHIP => coeff *= bundle.coef("ConsumableReloadTime"), // :197
        GROUP_SQUADRON => coeff *= bundle.coef("planeConsumableReloadTime"), // :206
        _ => {}
    }
    // :200-201 / :209-210
    gated_type_coef(bundle, coeff, type_name, CONSUMABLES_WITH_RELOAD_COEFFICIENTS, "ReloadCoeff")
}

/// `getConsumableWorkTime` (ModifiersApply.py:217-239), `slotID=None`.
///
/// SHIP `*= ConsumablesWorkTime`, SQUADRON `*= planeConsumablesWorkTime`; then
/// `*= <typeName>WorkTimeCoeff` when the type is in
/// `ConsumablesWithWorkTimeCoefficients`.
fn consumable_work_time_coeff(bundle: &ModifierBundle, type_name: &str, group: &str) -> f32 {
    let mut coeff = 1.0;
    match group {
        GROUP_SHIP => coeff *= bundle.coef("ConsumablesWorkTime"), // :222
        GROUP_SQUADRON => coeff *= bundle.coef("planeConsumablesWorkTime"), // :231
        _ => {}
    }
    // :225-226 / :234-235
    gated_type_coef(bundle, coeff, type_name, CONSUMABLES_WITH_WORK_TIME_COEFFICIENTS, "WorkTimeCoeff")
}

/// `getConsumableCapacity` (ModifiersApply.py:242-263).
///
/// `coeff = consumableCapacityCoeff`; SHIP `*= shipConsumableCapacityCoeff`,
/// SQUADRON `*= squadronConsumableCapacityCoeff`; then `*= <typeName>CapacityCoeff`
/// when the type is in `ConsumablesWithCapacityCoefficients`.
fn consumable_capacity_coeff(bundle: &ModifierBundle, type_name: &str, group: &str) -> f32 {
    let mut coeff = bundle.coef("consumableCapacityCoeff"); // :243
    match group {
        GROUP_SHIP => coeff *= bundle.coef("shipConsumableCapacityCoeff"), // :246
        GROUP_SQUADRON => coeff *= bundle.coef("squadronConsumableCapacityCoeff"), // :255
        _ => {}
    }
    // :249-250 / :258-259
    gated_type_coef(bundle, coeff, type_name, CONSUMABLES_WITH_CAPACITY_COEFFICIENTS, "CapacityCoeff")
}

/// `getAdditionalConsumablesCount` (ModifiersApply.py:274-280): the additive
/// per-type `<typeName>AdditionalConsumables` bonus (0 when the type is not in
/// `AdditionalConsumablesCount`).
fn additional_consumables_count(bundle: &ModifierBundle, type_name: &str) -> f32 {
    if ADDITIONAL_CONSUMABLES_COUNT.contains(&type_name) {
        // The bonus name is additive (base 0.0); read it via apply onto 0.0.
        bundle.apply(0.0, &format!("{type_name}AdditionalConsumables"))
    } else {
        0.0
    }
}

/// `getAdditionalConsumablesCountForGroup` (ModifiersApply.py:283-290): the additive
/// group-wide bonus, `additionalConsumables` (SHIP) or `planeAdditionalConsumables`
/// (SQUADRON).
fn additional_consumables_for_group(bundle: &ModifierBundle, group: &str) -> f32 {
    match group {
        GROUP_SHIP => bundle.apply(0.0, "additionalConsumables"), // :285
        GROUP_SQUADRON => bundle.apply(0.0, "planeAdditionalConsumables"), // :288
        _ => 0.0,
    }
}

/// Read a numeric field from the category's merged effect-field map.
fn field(fields: &BTreeMap<String, f32>, name: &str) -> Option<f32> {
    fields.get(name).copied()
}

/// Compute the final, modifier-applied stats for `category` under `modifiers`.
///
/// This is the consumable analog of the TTX weapon factories: it returns FINAL
/// values with the equipped bundle folded in, so callers never need to know which
/// modifier multiplies and which adds. An empty bundle yields the base values
/// unchanged.
pub fn effective_consumable(category: &AbilityCategory, modifiers: &ModifierBundle) -> EffectiveConsumable {
    let type_name = category.consumable_type_raw();
    let group = category.group();
    let fields = category.effect_fields();

    // reloadTime / preparationTime both scale by getConsumableReloadTime
    // (ConsumableUtils.py:115-116).
    let reload_coeff = consumable_reload_coeff(modifiers, type_name, group);
    let reload_time = Seconds::from(category.reload_time() * reload_coeff);
    let preparation_time = Seconds::from(category.preparation_time() * reload_coeff);

    let lifecycle = field(fields, "lifeCycleType").unwrap_or(LIFECYCLE_COUNT_BASED);

    // workTime is multiplied only for COUNT_BASED consumables (ConsumableUtils.py:120);
    // TIME_BASED consumables have no port workTime stat (capacity/minWorkTime govern).
    let work_time = if lifecycle == LIFECYCLE_COUNT_BASED {
        Some(Seconds::from(category.work_time() * consumable_work_time_coeff(modifiers, type_name, group)))
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
        let added = additional_consumables_count(modifiers, type_name) + additional_consumables_for_group(modifiers, group);
        // ConsumableUtils.py:128 `max(0, numConsumables + added)`
        let total = (base_count as f32 + added).max(0.0);
        AmmoCount::Finite(total.round() as u32)
    } else {
        AmmoCount::Finite(base_count as u32)
    };

    // maxCapacity for TIME_BASED consumables (ConsumableUtils.py:166-167). Absent for
    // COUNT_BASED.
    let max_capacity = if lifecycle == LIFECYCLE_TIME_BASED {
        field(fields, "maxCapacity").filter(|&c| c >= 0.0).map(|c| c * consumable_capacity_coeff(modifiers, type_name, group))
    } else {
        None
    };

    // regenerationHPSpeed: REGEN_CREW only, `*= regenerationHPSpeed` modifier
    // (ConsumableUtils.py:140). The base is read off the typed accessor.
    let regeneration_hp_speed = if type_name == TYPE_REGEN_CREW {
        category.regeneration_hp_speed().map(|v| v * modifiers.coef("regenerationHPSpeed"))
    } else {
        None
    };

    // detection_radius has no modifier in updateConsumableParams; the base value is
    // final.
    let detection_radius = category.detection_radius();

    EffectiveConsumable {
        reload_time,
        work_time,
        preparation_time,
        charges,
        max_capacity,
        detection_radius,
        regeneration_hp_speed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::AbilityCategory;
    use crate::game_params::types::CrewSkillModifier;
    use crate::game_params::types::Species;

    /// The build whose `MODIFIER_SETTINGS` table is transcribed in the toolkit.
    const BUILD: u32 = 11791718;

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
        let eff = effective_consumable(&cat, &ModifierBundle::empty(Species::Battleship));
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
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let eff = effective_consumable(&cat, &bundle);
        assert!((eff.reload_time.value() - 108.0).abs() < 1e-3, "got {}", eff.reload_time.value());
    }

    /// The `allConsumableReloadTime` and `ConsumableReloadTime` coefficients compound:
    /// 120 * 0.9 * 0.95 = 102.6.
    #[test]
    fn reload_coefficients_compound() {
        let cat = crash_crew();
        let mods = [modifier("ConsumableReloadTime", 0.9), modifier("allConsumableReloadTime", 0.95)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let eff = effective_consumable(&cat, &bundle);
        assert!((eff.reload_time.value() - 102.6).abs() < 1e-3, "got {}", eff.reload_time.value());
    }

    /// A `numConsumables = -1` base stays Infinite even with an additional-count
    /// modifier equipped (modifiers never make an unlimited pool finite).
    #[test]
    fn infinite_charges_stay_infinite() {
        let cat = crash_crew();
        let mods = [modifier("crashCrewAdditionalConsumables", 1.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let eff = effective_consumable(&cat, &bundle);
        assert_eq!(eff.charges, AmmoCount::Infinite);
    }

    /// A finite base count gains the additive per-type and group bonuses: 3 + 1
    /// (sonar has no per-type AdditionalConsumables, so only the group-wide
    /// `additionalConsumables` applies) = 4.
    #[test]
    fn finite_charges_add_group_bonus() {
        let cat = finite_sonar();
        let mods = [modifier("additionalConsumables", 1.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let eff = effective_consumable(&cat, &bundle);
        assert_eq!(eff.charges, AmmoCount::Finite(4));
    }

    /// A finite base count with no modifiers is unchanged.
    #[test]
    fn finite_charges_base_unchanged() {
        let cat = finite_sonar();
        let eff = effective_consumable(&cat, &ModifierBundle::empty(Species::Cruiser));
        assert_eq!(eff.charges, AmmoCount::Finite(3));
    }

    /// The per-type `sonarWorkTimeCoeff` gate fires for `sonar`: workTime 100 * 0.8
    /// (ConsumablesWorkTime is absent -> 1.0, sonarWorkTimeCoeff 0.8) ... here only
    /// the per-type coeff is equipped, so 100 * 0.8 = 80.
    #[test]
    fn per_type_work_time_coeff_applies() {
        let cat = finite_sonar();
        let mods = [modifier("sonarWorkTimeCoeff", 0.8)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let eff = effective_consumable(&cat, &bundle);
        assert_eq!(eff.work_time, Some(Seconds::from(80.0)));
    }
}
