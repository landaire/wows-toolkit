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
use crate::game_params::ttx::components::TorpedoLauncherStats;
use crate::game_params::ttx::constants::BW_TO_BALLISTIC;
use crate::game_params::ttx::constants::HULL_HEALTH_ROUND;
use crate::game_params::ttx::constants::KM_TO_M;
use crate::game_params::ttx::constants::TORPEDO_DAMAGE_CONSTANT;
use crate::game_params::ttx::model::Armor;
use crate::game_params::ttx::model::Battery;
use crate::game_params::ttx::model::DegreesPerSecond;
use crate::game_params::ttx::model::Durability;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Knots;
use crate::game_params::ttx::model::Launcher;
use crate::game_params::ttx::model::Mobility;
use crate::game_params::ttx::model::Percent;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::model::TorpedoStats;
use crate::game_params::ttx::model::Torpedoes;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::Km;
use crate::game_params::types::Meters;
use crate::game_params::types::Millimeters;
use crate::game_params::types::Projectile;

/// `AMMO_TYPES.TORPEDO` string discriminant (ProjectileConstants.py:13), the gate
/// for `normalTorpedoSpeedMultiplier` in `getTorpedoSpeed` (ModifiersApply.py:495).
const AMMO_TYPE_TORPEDO: &str = "torpedo";

/// `TORPEDO_TYPE.COMMON = 0` (shared_constants/mbe76a59e.py:517), the value tested by
/// `disabledUnderwater` (FactoryTorpedoes.py:101).
const TORPEDO_TYPE_COMMON: i64 = 0;

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

/// Per-ammo torpedo stats from a resolved [`Projectile`] (`createTorpedoTTX`,
/// FactoryTorpedoes.py:82-122). Pure over the projectile + bundle so the formulas
/// are testable without a provider. `name` is filled by the caller (the launcher's
/// `ammoList` entry); `disabled_underwater` is left `None` here (surface-ship path,
/// FactoryTorpedoes.py:100-101 gates it on `isSubmarine`).
///
/// Each field is `None` when its base projectile input is absent.
pub fn torpedo_stats(name: String, projectile: &Projectile, modifiers: &ModifierBundle) -> TorpedoStats {
    // damage: getTorpedoDamage (ModifiersApply.py:477-488). Surface torpedoes take
    // the `alphaDamage / TORPEDO_DAMAGE_CONSTANT` branch (line 484); the submarine
    // citadel branch (line 480) needs SubmarineTorpedoParams not modeled here.
    // controllableWeaponDamageCoeff applies to all torpedoes (line 488, ungated).
    let damage = match (projectile.alpha_damage(), projectile.damage()) {
        (Some(alpha), Some(flood)) => {
            let base = alpha / TORPEDO_DAMAGE_CONSTANT + flood;
            Some(Hp::from(base * modifiers.coef("torpedoDamageCoeff") * modifiers.coef("controllableWeaponDamageCoeff")))
        }
        _ => None,
    };

    // speed: getTorpedoSpeed (ModifiersApply.py:491-499). normalTorpedoSpeedMultiplier
    // only applies when ammoType == AMMO_TYPES.TORPEDO (line 495); deep-water/alt
    // torpedoes skip it (multiplier stays 1.0).
    let speed = projectile.speed().map(|s| {
        let normal = if projectile.ammo_type() == AMMO_TYPE_TORPEDO {
            modifiers.coef("normalTorpedoSpeedMultiplier")
        } else {
            1.0
        };
        Knots::from(s * modifiers.coef("torpedoSpeedMultiplier") * normal + modifiers.bonus("torpedoSpeedBonus"))
    });

    // range: maxDist * torpedoRangeCoefficient * BW_TO_BALLISTIC / KM_TO_M (FactoryTorpedoes.py:93).
    let range = projectile
        .max_dist()
        .map(|d| Km::from(d.value() * modifiers.coef("torpedoRangeCoefficient") * BW_TO_BALLISTIC / KM_TO_M));

    // visibility: visibilityFactor * torpedoVisibilityFactor (FactoryTorpedoes.py:92).
    let visibility = projectile.visibility_factor().map(|v| Km::from(v * modifiers.coef("torpedoVisibilityFactor")));

    // isDamageIncreasing: distanceOfDamage max-coeff dist > min-coeff dist (FactoryTorpedoes.py:103-120).
    // distanceOfMaxDamage (line 119) also needs ammoParams.armingTime/maneuverDist,
    // which are not on the parsed Projectile, so it stays None (no fabrication).
    let is_damage_increasing = projectile.distance_of_damage().filter(|d| !d.is_empty()).map(|pairs| {
        let coeff_at_min_dist = pairs.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|p| p.0).unwrap_or(0.0);
        let coeff_at_max_dist = pairs.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|p| p.0).unwrap_or(0.0);
        coeff_at_max_dist > coeff_at_min_dist
    });

    TorpedoStats {
        name,
        damage,
        speed,
        range,
        visibility,
        distance_of_max_damage: None,
        is_damage_increasing,
        disabled_underwater: None,
    }
}

/// Torpedo armament section (`createTorpedoesTTX`, FactoryTorpedoes.py:12-24).
///
/// `launchers` mirror `createLauncherTTX` (FactoryTorpedoes.py:74-80):
/// `rotation_speed = rotationSpeed[0] * GTRotationSpeed + GTRotationSpeedBonus`,
/// `rotation_time = 180 / rotation_speed`. FactoryTorpedoes.py:78 reads the
/// `GunRotationSpeedModifiersStruct` fields `yawSpeedCoef`/`yawSpeedBonus`, which
/// ModifiersApply.py:123 builds from `modifier.GTRotationSpeed`/`GTRotationSpeedBonus`
/// (GunRotationSpeed.py:10-13 positional->field map); the bundle is keyed by the
/// real modifier names, so we look up `GTRotationSpeed`/`GTRotationSpeedBonus`.
/// `reload_time` is the launcher reload
/// `shotDelay * GTShotDelay` (createUngroupedLaunchersTTX, FactoryTorpedoes.py:49)
/// aggregated as the `min` non-zero across mounts (initAmmoReloadParams,
/// FactoryTorpedoes.py:40). Per-ammo stats are resolved by NAME from `provider`
/// (`createTorpedoTTX`, FactoryTorpedoes.py:88-89).
///
/// `None` when `launchers` is empty (no torpedo armament).
pub fn torpedoes(launchers: &[TorpedoLauncherStats], modifiers: &ModifierBundle, provider: &dyn GameParamProvider) -> Option<Torpedoes> {
    if launchers.is_empty() {
        return None;
    }

    // Real modifier names behind GunRotationSpeedModifiersStruct.yawSpeedCoef/yawSpeedBonus
    // (GunRotationSpeed.py:10-13, ModifiersApply.py:123).
    let yaw_coef = modifiers.coef("GTRotationSpeed");
    let yaw_bonus = modifiers.bonus("GTRotationSpeedBonus");
    let shot_delay_coef = modifiers.coef("GTShotDelay");

    let mut result_launchers = Vec::with_capacity(launchers.len());
    let mut reload_times: Vec<f32> = Vec::new();
    let mut seen_ammo: Vec<String> = Vec::new();
    let mut torpedoes = Vec::new();

    for launcher in launchers {
        let rotation_speed = launcher.rotation_speed.map(|r| r * yaw_coef + yaw_bonus);
        let rotation_time = rotation_speed.filter(|&r| r != 0.0).map(|r| Seconds::from(180.0 / r));
        result_launchers.push(Launcher {
            rotation_speed: rotation_speed.map(DegreesPerSecond::from),
            rotation_time,
            num_barrels: launcher.num_barrels.map(|n| n as u32),
        });

        if let Some(delay) = launcher.shot_delay {
            let reload = delay * shot_delay_coef;
            if reload != 0.0 {
                reload_times.push(reload);
            }
        }

        for ammo_name in &launcher.ammo {
            if seen_ammo.iter().any(|n| n == ammo_name) {
                continue;
            }
            seen_ammo.push(ammo_name.clone());
            if let Some(param) = provider.game_param_by_name(ammo_name)
                && let Some(projectile) = param.projectile()
            {
                torpedoes.push(torpedo_stats(ammo_name.clone(), projectile, modifiers));
            }
        }
    }

    let reload_time = reload_times.iter().copied().reduce(f32::min).map(Seconds::from);

    Some(Torpedoes { reload_time, launchers: result_launchers, torpedoes })
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

    use crate::game_params::types::Param;
    use crate::game_params::types::ParamData;
    use crate::Rc;
    use crate::game_types::GameParamId;

    /// Gearing's real `PAPT027_Mk_16_mod_1` torpedo (GameParams Projectile):
    /// ammoType "torpedo", torpedoType COMMON(0), maxDist 350 (BW), speed 66,
    /// alphaDamage 53500, damage (flood) 1200, visibilityFactor 1.4.
    fn gearing_torpedo() -> Projectile {
        Projectile::builder()
            .ammo_type("torpedo".to_string())
            .max_dist(crate::game_params::types::BigWorldDistance::from(350.0))
            .speed(66.0)
            .alpha_damage(53500.0)
            .damage(1200.0)
            .visibility_factor(1.4)
            .torpedo_type(0)
            .build()
    }

    /// Gearing's real torpedo launcher mount (`HP_AGT_*`): shotDelay 103,
    /// rotationSpeed[0] 25, numBarrels 5, one ammo PAPT027_Mk_16_mod_1.
    fn gearing_launcher() -> TorpedoLauncherStats {
        TorpedoLauncherStats {
            shot_delay: Some(103.0),
            rotation_speed: Some(25.0),
            num_barrels: Some(5.0),
            ammo_switch_coeff: None,
            ammo: vec!["PAPT027_Mk_16_mod_1".to_string()],
        }
    }

    /// A minimal in-memory provider exposing one named Projectile param, enough to
    /// exercise the name->projectile resolver in `torpedoes` without a full
    /// GameParams index.
    struct StubProvider {
        param: Rc<Param>,
    }

    impl StubProvider {
        fn new(name: &str, projectile: Projectile) -> Self {
            let param = Param::builder()
                .id(GameParamId::from(1u32))
                .index("S0001".to_string())
                .name(name.to_string())
                .nation("USA".to_string())
                .data(ParamData::Projectile(projectile))
                .build();
            StubProvider { param: Rc::new(param) }
        }
    }

    impl GameParamProvider for StubProvider {
        fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_index(&self, _index: &str) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
            (self.param.name() == name).then(|| self.param.clone())
        }
        fn params(&self) -> &[Rc<Param>] {
            std::slice::from_ref(&self.param)
        }
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-2
    }

    #[test]
    fn gearing_stock_torpedo_stats() {
        let stats = torpedo_stats("PAPT027_Mk_16_mod_1".to_string(), &gearing_torpedo(), &ModifierBundle::empty(Species::Destroyer));
        // damage: 53500/3 + 1200 = 19033.33 (alphaDamage/3 + flood, stock coeffs 1.0).
        let damage = stats.damage.expect("damage").value();
        assert!(approx(damage, 53500.0 / 3.0 + 1200.0), "got {damage}");
        // speed: 66 * 1.0 * 1.0 + 0 = 66.
        assert_eq!(stats.speed, Some(Knots::from(66.0)));
        // range: 350 * 1.0 * 30 / 1000 = 10.5.
        let range = stats.range.expect("range").value();
        assert!(approx(range, 10.5), "got {range}");
        // visibility: 1.4 * 1.0 = 1.4.
        assert_eq!(stats.visibility, Some(Km::from(1.4)));
        // No distanceOfDamage on Gearing's torpedo -> is_damage_increasing None.
        assert!(stats.is_damage_increasing.is_none());
        // distanceOfMaxDamage needs armingTime/maneuverDist (absent) -> None.
        assert!(stats.distance_of_max_damage.is_none());
        // Surface path -> disabledUnderwater not set.
        assert!(stats.disabled_underwater.is_none());
    }

    #[test]
    fn gearing_stock_torpedoes_via_provider() {
        let launchers = [gearing_launcher()];
        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        let torps = torpedoes(&launchers, &ModifierBundle::empty(Species::Destroyer), &provider).expect("torpedoes computed");

        // reload_time: shotDelay 103 * GTShotDelay 1.0 = 103 (min over one mount).
        assert_eq!(torps.reload_time, Some(Seconds::from(103.0)));

        // launcher: rotationSpeed 25 * 1.0 + 0 = 25; rotation_time = 180/25 = 7.2.
        assert_eq!(torps.launchers.len(), 1);
        let launcher = &torps.launchers[0];
        assert_eq!(launcher.rotation_speed, Some(DegreesPerSecond::from(25.0)));
        let rt = launcher.rotation_time.expect("rotation_time").value();
        assert!(approx(rt, 7.2), "got {rt}");
        assert_eq!(launcher.num_barrels, Some(5));

        // per-ammo resolved by name.
        assert_eq!(torps.torpedoes.len(), 1);
        let torp = &torps.torpedoes[0];
        assert_eq!(torp.name, "PAPT027_Mk_16_mod_1");
        assert!(approx(torp.damage.expect("damage").value(), 53500.0 / 3.0 + 1200.0));
        assert_eq!(torp.speed, Some(Knots::from(66.0)));
        assert!(approx(torp.range.expect("range").value(), 10.5));
        assert_eq!(torp.visibility, Some(Km::from(1.4)));
    }

    #[test]
    fn torpedo_speed_bonus_applies() {
        // torpedoSpeedBonus +5 (additive): 66 + 5 = 71.
        let mods = [modifier("torpedoSpeedBonus", 5.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Destroyer, BUILD);
        let stats = torpedo_stats("PAPT027_Mk_16_mod_1".to_string(), &gearing_torpedo(), &bundle);
        let speed = stats.speed.expect("speed").value();
        assert!(approx(speed, 71.0), "got {speed}");
    }

    #[test]
    fn torpedo_damage_coeff_applies() {
        // torpedoDamageCoeff 1.2 (coef): (53500/3 + 1200) * 1.2.
        let mods = [modifier("torpedoDamageCoeff", 1.2)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Destroyer, BUILD);
        let stats = torpedo_stats("PAPT027_Mk_16_mod_1".to_string(), &gearing_torpedo(), &bundle);
        let damage = stats.damage.expect("damage").value();
        assert!(approx(damage, (53500.0 / 3.0 + 1200.0) * 1.2), "got {damage}");
    }

    #[test]
    fn torpedo_launcher_traverse_coef_applies() {
        // GTRotationSpeed 1.2 (Torpedo_Mod_II, +20% traverse), real modifier name
        // mapped from GunRotationSpeedModifiersStruct.yawSpeedCoef
        // (GunRotationSpeed.py:10-13, ModifiersApply.py:123). base 25 -> 30, time 180/30 = 6.
        let mods = [modifier("GTRotationSpeed", 1.2)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Destroyer, BUILD);
        let launchers = [gearing_launcher()];
        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        let torps = torpedoes(&launchers, &bundle, &provider).expect("torpedoes computed");
        let launcher = &torps.launchers[0];
        let rs = launcher.rotation_speed.expect("rotation_speed").value();
        assert!(approx(rs, 30.0), "got {rs}");
        let rt = launcher.rotation_time.expect("rotation_time").value();
        assert!(approx(rt, 6.0), "got {rt}");
    }

    #[test]
    fn torpedo_launcher_traverse_bonus_applies() {
        // GTRotationSpeedBonus +5 (additive, base 0.0): 25 + 5 = 30, time 180/30 = 6.
        let mods = [modifier("GTRotationSpeedBonus", 5.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Destroyer, BUILD);
        let launchers = [gearing_launcher()];
        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        let torps = torpedoes(&launchers, &bundle, &provider).expect("torpedoes computed");
        let launcher = &torps.launchers[0];
        assert_eq!(launcher.rotation_speed, Some(DegreesPerSecond::from(30.0)));
        let rt = launcher.rotation_time.expect("rotation_time").value();
        assert!(approx(rt, 6.0), "got {rt}");
    }

    #[test]
    fn torpedoes_none_when_no_launchers() {
        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        assert!(torpedoes(&[], &ModifierBundle::empty(Species::Destroyer), &provider).is_none());
    }

    #[test]
    fn torpedo_stats_none_when_inputs_absent() {
        // A projectile with no torpedo fields -> all stats None (no fabrication).
        let empty = Projectile::builder().ammo_type("torpedo".to_string()).build();
        let stats = torpedo_stats("X".to_string(), &empty, &ModifierBundle::empty(Species::Destroyer));
        assert!(stats.damage.is_none());
        assert!(stats.speed.is_none());
        assert!(stats.range.is_none());
        assert!(stats.visibility.is_none());
    }
}
