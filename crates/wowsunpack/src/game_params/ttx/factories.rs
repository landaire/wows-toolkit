//! TTX factory functions: apply per-species formulas + the equipped
//! `ModifierBundle` to base component stats, producing unit-carrying `ShipStats`
//! sections. Formulas are transcribed verbatim from the deob; each is cited at
//! its application site.
//!
//! This module currently covers the direct-field species `durability` and
//! `mobility`; armor/battery/hull-summary land in later M2 tasks.

use crate::game_params::ttx::armor_materials::armor_type_classifies;
use crate::game_params::ttx::armor_materials::collision_material_name;
use crate::game_params::ttx::components::EngineComponentStats;
use crate::game_params::ttx::components::HullComponentStats;
use crate::game_params::ttx::constants::HULL_HEALTH_ROUND;
use crate::game_params::ttx::model::Armor;
use crate::game_params::ttx::model::Battery;
use crate::game_params::ttx::model::Durability;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Knots;
use crate::game_params::ttx::model::Mobility;
use crate::game_params::ttx::model::Percent;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::Meters;
use crate::game_params::types::Millimeters;

/// Survivability section (`ma6320f36/ttx/FactoryDurability.py:5`).
///
/// `health` transcribes `Modifiers/ModifiersApply.py:142`'s `calculateVehicleHealth`:
/// `(hull.health + healthPerLevel*level) * healthHullCoeff`, then rounded up to a
/// multiple of `HULL_HEALTH_ROUND` (line 143). `healthPerLevel` is additive
/// (base_value 0.0 -> `bonus`); `healthHullCoeff` is multiplicative (base_value 1.0
/// -> `coef`).
///
/// `torpedo_protection` (ptz, FactoryDurability.py:8) is
/// `hull.floodProb * uwCoeffMultiplier * 100 + uwCoeffBonus`. `floodProb` is derived
/// at parse time from `floodNodes` (HullComponentStats::flood_prob, PreprocessedHull.py:12);
/// `uwCoeffMultiplier` is a coefficient (`*`) and `uwCoeffBonus` an additive bonus (`+`,
/// MODIFIER_SETTINGS base_value 0.0). `None` when `flood_prob` is absent.
pub fn durability(hull: &HullComponentStats, modifiers: &ModifierBundle, level: u32) -> Durability {
    let health = hull.health.map(|base| {
        let raw = (base + modifiers.bonus("healthPerLevel") * level as f32) * modifiers.coef("healthHullCoeff");
        // ceil(raw / round) * round (ModifiersApply.py:143).
        let rounded = (raw / HULL_HEALTH_ROUND).ceil() * HULL_HEALTH_ROUND;
        Hp::from(rounded)
    });

    let torpedo_protection = hull
        .flood_prob
        .map(|prob| Percent::from(prob * modifiers.coef("uwCoeffMultiplier") * 100.0 + modifiers.bonus("uwCoeffBonus")));

    Durability { health, torpedo_protection }
}

/// Maneuverability section (`ma6320f36/ttx/FactoryMobility.py:6`).
///
/// `speed` = `calculateMaxSpeedKnots(prepared) * speedCoef` (FactoryMobility.py:8),
/// where `calculateMaxSpeedKnots` (ShipParams.py:348) is
/// `hull.maxSpeed * clamp(hull.speedCoef + engine.speedCoef, 0.0, 1.0)`. `speedCoef`
/// is multiplicative (base_value 1.0 -> `coef`).
///
/// `turning_radius` = `hull.turningRadius` directly (FactoryMobility.py:9, no modifier).
///
/// `rudder_time` = `hull.rudderTime * SGRudderTime` (FactoryMobility.py:10).
/// `SGRudderTime` is multiplicative (base_value 1.0 -> `coef`).
pub fn mobility(hull: &HullComponentStats, engine: &EngineComponentStats, modifiers: &ModifierBundle) -> Mobility {
    let speed = match (hull.max_speed, hull.speed_coef) {
        (Some(max_speed), Some(hull_speed_coef)) => {
            let engine_speed_coef = engine.speed_coef.unwrap_or(0.0);
            let coef = (hull_speed_coef + engine_speed_coef).clamp(0.0, 1.0);
            Some(Knots::from(max_speed * coef * modifiers.coef("speedCoef")))
        }
        _ => None,
    };

    let turning_radius = hull.turning_radius.map(Meters::from);

    let rudder_time = hull.rudder_time.map(|t| Seconds::from(t * modifiers.coef("SGRudderTime")));

    Mobility { speed, turning_radius, rudder_time }
}

/// Submarine battery section (`ma6320f36/ttx/FactoryBattery.py:4`).
///
/// `capacity = hull.batteryCapacity * batteryCapacityCoeff` (FactoryBattery.py:10),
/// `regeneration = hull.batteryRegenRate * batteryRegenCoeff` (FactoryBattery.py:11).
/// Both coeffs are multiplicative (MODIFIER_SETTINGS base_value 1.0). The deob returns
/// `None` when `batteryCapacity == 0` (FactoryBattery.py:5); here the hull carries
/// battery fields only for submarines, so `None` when they are absent.
pub fn battery(hull: &HullComponentStats, modifiers: &ModifierBundle) -> Option<Battery> {
    let (capacity, regen) = (hull.battery_capacity?, hull.battery_regen_rate?);
    if capacity == 0.0 {
        return None;
    }
    Some(Battery {
        capacity: Some(capacity * modifiers.coef("batteryCapacityCoeff")),
        regeneration: Some(regen * modifiers.coef("batteryRegenCoeff")),
    })
}

/// Default lowest armor thickness when an `armorList` yields no classified plate
/// (`PreprocessedArmor.py:8`'s `armorMin`/`armorMax` seed, `ArmorConstants.py`
/// `DEFAULT_LOWEST_ARMOR_THICKNESS`).
const DEFAULT_LOWEST_ARMOR_THICKNESS: f32 = 6.0;

/// Reduce one `armorList` (an [`ArmorMap`]) to its `(min, max)` over the plates
/// `getArmorType` classifies, transcribing `PreprocessedArmor.__init__`
/// (`PreprocessedArmor.py:7-15`).
///
/// The deob filter is `[thk for matId, thk in armorList.items()
/// if getArmorType(collisionMaterialName(matId)) if thk > 0]`. The collision
/// material id is the low byte of the outer key (`Avatar.py:1386` masks `& 255`;
/// `getGunArmorBits`/`gunArmorMask` reserve the other bits for gun/model
/// indices). When no plate is classified, both extremes are
/// `DEFAULT_LOWEST_ARMOR_THICKNESS`.
fn armor_list_min_max(armor: &ArmorMap) -> (f32, f32) {
    let mut min = DEFAULT_LOWEST_ARMOR_THICKNESS;
    let mut max = DEFAULT_LOWEST_ARMOR_THICKNESS;
    let mut found = false;

    for (&material_key, layers) in armor {
        let material_id = (material_key & 0xFF) as u8;
        if !armor_type_classifies(collision_material_name(material_id)) {
            continue;
        }
        for &thickness in layers.values() {
            if thickness <= 0.0 {
                continue;
            }
            if found {
                min = min.min(thickness);
                max = max.max(thickness);
            } else {
                min = thickness;
                max = thickness;
                found = true;
            }
        }
    }

    (min, max)
}

/// Armor section (`ma6320f36/ttx/FactoryArmor.py:5` `createArmorTTX`).
///
/// `min`/`max` are the extremes of classified plate thicknesses across the hull
/// `armorList` and, when the ship has artillery, the combined artillery
/// `armorList` (`getArmorDictByComponent`, all gun mounts). Per
/// `FactoryArmor.py:7-12`: with artillery, `min = min(arti.min, hull.min)` and
/// `max = max(arti.max, hull.max)`; otherwise hull-only.
///
/// `hull_armor` is the selected hull's `module.armor` (= `Vehicle.armor`).
/// `artillery_armor` yields each main-battery mount's armor map
/// (`MountPoint::mount_armor`); an empty iterator is the no-artillery branch.
/// `None` when the hull carries no armor data.
pub fn armor<'a>(hull_armor: &ArmorMap, artillery_armor: impl IntoIterator<Item = &'a ArmorMap>) -> Option<Armor> {
    if hull_armor.is_empty() {
        return None;
    }

    let (hull_min, hull_max) = armor_list_min_max(hull_armor);

    let mut arti_min: Option<f32> = None;
    let mut arti_max: Option<f32> = None;
    for map in artillery_armor {
        let (m, x) = armor_list_min_max(map);
        arti_min = Some(arti_min.map_or(m, |cur: f32| cur.min(m)));
        arti_max = Some(arti_max.map_or(x, |cur: f32| cur.max(x)));
    }

    let (min, max) = match (arti_min, arti_max) {
        (Some(amin), Some(amax)) => (amin.min(hull_min), amax.max(hull_max)),
        _ => (hull_min, hull_max),
    };

    Some(Armor { min: Some(Millimeters::from(min)), max: Some(Millimeters::from(max)) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::ttx::constants::DEFAULT_UW_DAMAGE_COEFF;
    use crate::game_params::types::CrewSkillModifier;
    use crate::game_params::types::Species;

    /// The build whose `MODIFIER_SETTINGS` table is transcribed in the toolkit.
    const BUILD: u32 = 11791718;

    /// Gearing's real default-hull base stats (GameParams `PASD013_Gearing_1945`
    /// `A_Hull`): health 19400, maxSpeed 36, speedCoef 1.0, turningRadius 640,
    /// rudderTime 4.25, visibilityFactor 7.33. `floodNodes[0][0]` is 0.333 (==
    /// DEFAULT_UW_DAMAGE_COEFF), so flood_prob is 0.0; no SubmarineBattery (DD).
    fn gearing_hull() -> HullComponentStats {
        HullComponentStats {
            health: Some(19400.0),
            max_speed: Some(36.0),
            speed_coef: Some(1.0),
            turning_radius: Some(640.0),
            rudder_time: Some(4.25),
            visibility_factor: Some(7.33),
            flood_prob: Some(0.0),
            battery_capacity: None,
            battery_regen_rate: None,
        }
    }

    /// Yamato's real default-hull `floodNodes[0][0]` is 0.15 (GameParams
    /// `PJSB018_Yamato_1944` `A_Hull`); flood_prob = (0.333 - 0.15) / 0.333.
    fn yamato_hull() -> HullComponentStats {
        let flood_prob = (DEFAULT_UW_DAMAGE_COEFF - 0.15) / DEFAULT_UW_DAMAGE_COEFF;
        HullComponentStats { flood_prob: Some(flood_prob), ..Default::default() }
    }

    /// Balao's real submarine battery (GameParams `PASS110_Balao` `A_Hull`
    /// `SubmarineBattery`): capacity 240, regenRate 1.2.
    fn balao_hull() -> HullComponentStats {
        HullComponentStats { battery_capacity: Some(240.0), battery_regen_rate: Some(1.2), ..Default::default() }
    }

    /// Gearing's engine `speedCoef` is 0.0 (hull carries the full coef).
    fn gearing_engine() -> EngineComponentStats {
        EngineComponentStats { speed_coef: Some(0.0) }
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

    #[test]
    fn gearing_stock_durability_health() {
        let durability = durability(&gearing_hull(), &ModifierBundle::empty(Species::Destroyer), 10);
        // ceil(19400 / 50) * 50 = 19400 (healthPerLevel 0, healthHullCoeff 1).
        assert_eq!(durability.health, Some(Hp::from(19400.0)));
    }

    #[test]
    fn gearing_stock_durability_ptz_zero() {
        // Gearing floodNodes[0][0] == DEFAULT_UW_DAMAGE_COEFF -> flood_prob 0 -> ptz 0.
        let durability = durability(&gearing_hull(), &ModifierBundle::empty(Species::Destroyer), 10);
        assert_eq!(durability.torpedo_protection, Some(Percent::from(0.0)));
    }

    #[test]
    fn yamato_stock_durability_ptz() {
        // flood_prob * 1.0 (stock uwCoeffMultiplier) * 100 + 0 (stock uwCoeffBonus).
        // (0.333 - 0.15) / 0.333 * 100 = 54.9549... (Yamato in-game ptz ~55%).
        let durability = durability(&yamato_hull(), &ModifierBundle::empty(Species::Battleship), 10);
        let ptz = durability.torpedo_protection.expect("ptz computed").value();
        let expected = (DEFAULT_UW_DAMAGE_COEFF - 0.15) / DEFAULT_UW_DAMAGE_COEFF * 100.0;
        assert!((ptz - expected).abs() < 1e-4, "got {ptz}, expected {expected}");
        assert!((ptz - 54.954956).abs() < 1e-3, "got {ptz}");
    }

    #[test]
    fn yamato_ptz_modifier_applies() {
        // uwCoeffBonus +25 (additive) shifts ptz by +25: 54.9549... + 25 = 79.9549...
        let mods = [modifier("uwCoeffBonus", 25.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let durability = durability(&yamato_hull(), &bundle, 10);
        let ptz = durability.torpedo_protection.expect("ptz computed").value();
        let expected = (DEFAULT_UW_DAMAGE_COEFF - 0.15) / DEFAULT_UW_DAMAGE_COEFF * 100.0 + 25.0;
        assert!((ptz - expected).abs() < 1e-4, "got {ptz}, expected {expected}");
    }

    #[test]
    fn ptz_none_when_flood_absent() {
        // No floodNodes -> flood_prob None -> ptz None (not fabricated).
        let hull = HullComponentStats::default();
        let durability = durability(&hull, &ModifierBundle::empty(Species::Destroyer), 10);
        assert!(durability.torpedo_protection.is_none());
    }

    #[test]
    fn gearing_stock_mobility() {
        let mobility = mobility(&gearing_hull(), &gearing_engine(), &ModifierBundle::empty(Species::Destroyer));
        // 36 * clamp(1.0 + 0.0) * 1.0 = 36.
        assert_eq!(mobility.speed, Some(Knots::from(36.0)));
        // turningRadius is a direct field.
        assert_eq!(mobility.turning_radius, Some(Meters::from(640.0)));
        // 4.25 * 1.0 (stock SGRudderTime) = 4.25.
        assert_eq!(mobility.rudder_time, Some(Seconds::from(4.25)));
    }

    #[test]
    fn speed_coef_modifier_applies() {
        // speedCoef 1.05 -> 36 * 1.0 * 1.05 = 37.8.
        let mods = [modifier("speedCoef", 1.05)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Destroyer, BUILD);
        let mobility = mobility(&gearing_hull(), &gearing_engine(), &bundle);
        let speed = mobility.speed.expect("speed computed").value();
        assert!((speed - 37.8).abs() < 1e-4, "got {speed}");
    }

    #[test]
    fn health_modifier_applies() {
        // healthPerLevel 350 (bonus, +) and healthHullCoeff 1.05 (coef, *):
        // (19400 + 350*10) * 1.05 = 22900*1.05 = 24045 -> ceil(24045/50)*50 = 24050.
        let mods = [modifier("healthPerLevel", 350.0), modifier("healthHullCoeff", 1.05)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Destroyer, BUILD);
        let durability = durability(&gearing_hull(), &bundle, 10);
        assert_eq!(durability.health, Some(Hp::from(24050.0)));
    }

    #[test]
    fn absent_inputs_are_none() {
        let empty_hull = HullComponentStats::default();
        let durability = durability(&empty_hull, &ModifierBundle::empty(Species::Destroyer), 10);
        assert!(durability.health.is_none());

        let mobility = mobility(&empty_hull, &EngineComponentStats::default(), &ModifierBundle::empty(Species::Destroyer));
        assert!(mobility.speed.is_none());
        assert!(mobility.turning_radius.is_none());
        assert!(mobility.rudder_time.is_none());

        // No SubmarineBattery -> battery None.
        assert!(battery(&empty_hull, &ModifierBundle::empty(Species::Submarine)).is_none());
    }

    #[test]
    fn balao_stock_battery() {
        // capacity 240 * 1.0, regenRate 1.2 * 1.0 (stock battery coeffs).
        let battery = battery(&balao_hull(), &ModifierBundle::empty(Species::Submarine)).expect("battery computed");
        assert_eq!(battery.capacity, Some(240.0));
        assert_eq!(battery.regeneration, Some(1.2));
    }

    #[test]
    fn balao_battery_modifiers_apply() {
        // batteryCapacityCoeff 1.1 (coef) -> 240*1.1=264; batteryRegenCoeff 1.25 -> 1.2*1.25=1.5.
        let mods = [modifier("batteryCapacityCoeff", 1.1), modifier("batteryRegenCoeff", 1.25)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Submarine, BUILD);
        let battery = battery(&balao_hull(), &bundle).expect("battery computed");
        let capacity = battery.capacity.expect("capacity");
        let regen = battery.regeneration.expect("regen");
        assert!((capacity - 264.0).abs() < 1e-4, "got {capacity}");
        assert!((regen - 1.5).abs() < 1e-4, "got {regen}");
    }

    #[test]
    fn battery_none_for_non_sub() {
        // Gearing has no SubmarineBattery -> battery None.
        assert!(battery(&gearing_hull(), &ModifierBundle::empty(Species::Destroyer)).is_none());
    }

    /// Build an [`ArmorMap`] from `(raw_key, thickness)` pairs, mirroring
    /// `parse_armor_dict`'s `(model_index << 16) | material_id` keying.
    fn armor_map(entries: &[(u32, f32)]) -> ArmorMap {
        use std::collections::BTreeMap;
        let mut m: ArmorMap = std::collections::HashMap::new();
        for &(raw, thk) in entries {
            let model_index = raw >> 16;
            let material_id = raw & 0xFFFF;
            m.entry(material_id).or_insert_with(BTreeMap::new).insert(model_index, thk);
        }
        m
    }

    /// Subset of Yamato's `PJSB018_Yamato_1944 A_Hull.armor`: a Cit_Belt plate
    /// (mat 61, 410mm), a Tur1GkBar barbette (mat 134, 560mm), an unclassified
    /// RudderSide plate (mat 82, 350mm, must be excluded), and a thin SS_Side
    /// (mat 89, 19mm). Raw keys use model_index 2 (131072) like the real data.
    fn yamato_hull_armor() -> ArmorMap {
        armor_map(&[
            (131072 | 61, 410.0),  // Cit_Belt
            (131072 | 134, 560.0), // Tur1GkBar (barbette, classifies as ARTI)
            (131072 | 82, 350.0),  // RudderSide (NOT an armor type -> excluded)
            (131072 | 89, 19.0),   // SS_Side
            (1, 0.0),              // common, thickness 0 -> excluded
        ])
    }

    /// Yamato's `A_Artillery.HP_JGM_*.armor`: TurretFwd (mat 100, 650mm face),
    /// TurretSide (mat 32, 250mm), TurretDown (mat 99, 135mm). Gun bits live in
    /// the high byte (model_index 1 here); only the low byte selects the material.
    fn yamato_turret_armor() -> ArmorMap {
        armor_map(&[
            (65536 | 100, 650.0), // TurretFwd (turret face)
            (65536 | 32, 250.0),  // TurretSide
            (65536 | 99, 135.0),  // TurretDown
        ])
    }

    #[test]
    fn yamato_armor_min_max_with_artillery() {
        // hull classified: {410, 560, 19}; arti classified: {650, 250, 135}.
        // max = max(650, 560) = 650 (Yamato's in-game turret armor).
        // min = min(135, 19)  = 19.
        let hull = yamato_hull_armor();
        let arti = [yamato_turret_armor()];
        let armor = armor(&hull, arti.iter()).expect("armor computed");
        assert_eq!(armor.max, Some(Millimeters::from(650.0)));
        assert_eq!(armor.min, Some(Millimeters::from(19.0)));
    }

    #[test]
    fn hull_only_armor_excludes_unclassified() {
        // No artillery branch: extremes over classified hull plates only.
        // RudderSide (350) is excluded, so max = 560 (Tur1GkBar), min = 19.
        let hull = yamato_hull_armor();
        let armor = armor(&hull, std::iter::empty()).expect("armor computed");
        assert_eq!(armor.max, Some(Millimeters::from(560.0)));
        assert_eq!(armor.min, Some(Millimeters::from(19.0)));
    }

    #[test]
    fn armor_none_when_hull_armor_absent() {
        // No armor data at all -> None (not fabricated as the default 6mm).
        let empty: ArmorMap = std::collections::HashMap::new();
        assert!(armor(&empty, std::iter::empty()).is_none());
    }

    #[test]
    fn armor_defaults_when_no_classified_plate() {
        // Hull with only unclassified plates -> default 6mm extremes
        // (PreprocessedArmor.py:8 seed), not None: the hull map is non-empty.
        let hull = armor_map(&[(131072 | 82, 350.0), (131072 | 80, 200.0)]); // Rudder*
        let armor = armor(&hull, std::iter::empty()).expect("armor computed");
        assert_eq!(armor.min, Some(Millimeters::from(6.0)));
        assert_eq!(armor.max, Some(Millimeters::from(6.0)));
    }
}
