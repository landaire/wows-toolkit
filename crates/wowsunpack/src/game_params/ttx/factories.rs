//! TTX factory functions: apply per-species formulas + the equipped
//! `ModifierBundle` to base component stats, producing unit-carrying `ShipStats`
//! sections. Formulas are transcribed verbatim from the deob; each is cited at
//! its application site.
//!
//! This module currently covers the direct-field species `durability` and
//! `mobility`; armor/battery/hull-summary land in later M2 tasks.

use crate::game_params::ttx::armor_materials::armor_type_classifies;
use crate::game_params::ttx::armor_materials::collision_material_name;
use crate::game_params::ttx::components::ArtilleryComponentStats;
use crate::game_params::ttx::components::EngineComponentStats;
use crate::game_params::ttx::components::HullComponentStats;
use crate::game_params::ttx::components::SecondaryComponentStats;
use crate::game_params::ttx::components::TorpedoLauncherStats;
use crate::game_params::ttx::constants;
use crate::game_params::ttx::constants::BW_TO_BALLISTIC;
use crate::game_params::ttx::constants::HULL_HEALTH_ROUND;
use crate::game_params::ttx::constants::KM_TO_M;
use crate::game_params::ttx::constants::TORPEDO_DAMAGE_CONSTANT;
use crate::game_params::ttx::labels::TtxStat;
use crate::game_params::ttx::model::AmmoCount;
use crate::game_params::ttx::model::Armor;
use crate::game_params::ttx::model::Artillery;
use crate::game_params::ttx::model::Battery;
use crate::game_params::ttx::model::DegreesPerSecond;
use crate::game_params::ttx::model::Durability;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Knots;
use crate::game_params::ttx::model::Launcher;
use crate::game_params::ttx::model::MainGun;
use crate::game_params::ttx::model::Mobility;
use crate::game_params::ttx::model::Percent;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::model::ShellStats;
use crate::game_params::ttx::model::TorpedoStats;
use crate::game_params::ttx::model::Torpedoes;
use crate::game_params::ttx::model::Visibility;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::ttx::module_options::ModuleSlot;
use crate::game_params::ttx::provenance::InputId;
use crate::game_params::ttx::provenance::ModifierSources;
use crate::game_params::ttx::provenance::Recorder;
use crate::game_params::ttx::weapon_tables::StatWeaponType;
use crate::game_params::ttx::weapon_tables::alpha_damage_coeff;
use crate::game_params::ttx::weapon_tables::ammo_to_stat_weapon_table;
use crate::game_params::ttx::weapon_tables::artillery_damage_coeff;
use crate::game_params::ttx::weapon_tables::calculate_burn_chance;
use crate::game_params::ttx::weapon_tables::is_small_projectile;
use crate::game_params::types::ArmorMap;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::Km;
// `Meters` is named only by the test fixtures (the factory body uses `.to_meters()`).
#[cfg(test)]
use crate::game_params::types::Meters;
use crate::game_params::types::MetersPerSecond;
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
pub fn durability<R: Recorder>(
    hull: &HullComponentStats,
    hull_name: &str,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    level: u32,
    rec: &mut R,
) -> Durability {
    let health = hull.health.map(|base| {
        let raw = (base.value() + modifiers.bonus("healthPerLevel") * level as f32) * modifiers.coef("healthHullCoeff");
        // ceil(raw / round) * round (ModifiersApply.py:143).
        let rounded = (raw / HULL_HEALTH_ROUND).ceil() * HULL_HEALTH_ROUND;
        if R::ON {
            // Provenance records pre-round `raw` so replay(attr) reproduces it exactly;
            // the model stores the rounded value via Hp::from(rounded) below.
            rec.record(
                TtxStat::Health,
                None,
                base.value(),
                InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() },
                raw,
                |b| {
                    b.bonus(sources, "healthPerLevel", level as f32);
                    b.coef(sources, "healthHullCoeff");
                },
            );
        }
        Hp::from(rounded)
    });

    let torpedo_protection = hull.flood_prob.map(|prob| {
        let value = prob * modifiers.coef("uwCoeffMultiplier") * 100.0 + modifiers.bonus("uwCoeffBonus");
        if R::ON {
            rec.record(
                TtxStat::TorpedoProtection,
                None,
                prob * 100.0,
                InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() },
                value,
                |b| {
                    b.coef(sources, "uwCoeffMultiplier");
                    b.bonus(sources, "uwCoeffBonus", 1.0);
                },
            );
        }
        Percent::from(value)
    });

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
pub fn mobility<R: Recorder>(
    hull: &HullComponentStats,
    hull_name: &str,
    engine: &EngineComponentStats,
    engine_name: Option<&str>,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    rec: &mut R,
) -> Mobility {
    let speed = match (hull.max_speed, hull.speed_coef) {
        (Some(max_speed), Some(hull_speed_coef)) => {
            let engine_speed_coef = engine.speed_coef.unwrap_or(0.0);
            let coef = (hull_speed_coef + engine_speed_coef).clamp(0.0, 1.0);
            let value = max_speed.value() * coef * modifiers.coef("speedCoef");
            if R::ON {
                rec.record(
                    TtxStat::Speed,
                    None,
                    max_speed.value(),
                    InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() },
                    value,
                    |b| {
                        // hull+engine speedCoef blend, attributed to the engine (the variable part).
                        let engine_src = engine_name
                            .map(|n| InputId::Module { slot: ModuleSlot::Engine, name: n.to_string() })
                            .unwrap_or(InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() });
                        b.module(engine_src, "speedCoef", coef);
                        b.coef(sources, "speedCoef");
                    },
                );
            }
            Some(Knots::from(value))
        }
        _ => None,
    };

    let turning_radius = hull.turning_radius;
    if R::ON
        && let Some(t) = turning_radius
    {
        rec.record(
            TtxStat::TurningRadius,
            None,
            t.value(),
            InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() },
            t.value(),
            |_b| {},
        );
    }

    let rudder_time = hull.rudder_time.map(|t| {
        let value = t.value() * modifiers.coef("SGRudderTime");
        if R::ON {
            rec.record(
                TtxStat::RudderTime,
                None,
                t.value(),
                InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() },
                value,
                |b| b.coef(sources, "SGRudderTime"),
            );
        }
        Seconds::from(value)
    });

    Mobility { speed, turning_radius, rudder_time }
}

/// Submarine battery section (`ma6320f36/ttx/FactoryBattery.py:4`).
///
/// `capacity = hull.batteryCapacity * batteryCapacityCoeff` (FactoryBattery.py:10),
/// `regeneration = hull.batteryRegenRate * batteryRegenCoeff` (FactoryBattery.py:11).
/// Both coeffs are multiplicative (MODIFIER_SETTINGS base_value 1.0). The deob returns
/// `None` when `batteryCapacity == 0` (FactoryBattery.py:5); here the hull carries
/// battery fields only for submarines, so `None` when they are absent.
pub fn battery<R: Recorder>(
    hull: &HullComponentStats,
    hull_name: &str,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    rec: &mut R,
) -> Option<Battery> {
    let (capacity, regen) = (hull.battery_capacity?, hull.battery_regen_rate?);
    if capacity == 0.0 {
        return None;
    }
    let cap_value = capacity * modifiers.coef("batteryCapacityCoeff");
    let regen_value = regen * modifiers.coef("batteryRegenCoeff");
    if R::ON {
        let hull_src = || InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() };
        rec.record(TtxStat::BatteryCapacity, None, capacity, hull_src(), cap_value, |b| {
            b.coef(sources, "batteryCapacityCoeff")
        });
        rec.record(TtxStat::BatteryRegeneration, None, regen, hull_src(), regen_value, |b| {
            b.coef(sources, "batteryRegenCoeff")
        });
    }
    Some(Battery { capacity: Some(cap_value), regeneration: Some(regen_value) })
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
pub fn armor<'a, R: Recorder>(
    hull_armor: &ArmorMap,
    hull_name: &str,
    artillery_armor: impl IntoIterator<Item = &'a ArmorMap>,
    rec: &mut R,
) -> Option<Armor> {
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

    if R::ON {
        let src = || InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() };
        rec.record(TtxStat::ArmorMin, None, min, src(), min, |_b| {});
        rec.record(TtxStat::ArmorMax, None, max, src(), max, |_b| {});
    }
    Some(Armor { min: Some(Millimeters::from(min)), max: Some(Millimeters::from(max)) })
}

/// Per-ammo torpedo stats from a resolved [`Projectile`] (`createTorpedoTTX`,
/// FactoryTorpedoes.py:82-122). Pure over the projectile + bundle so the formulas
/// are testable without a provider. `name` is filled by the caller (the launcher's
/// `ammoList` entry); `disabled_underwater` is left `None` here (surface-ship path,
/// FactoryTorpedoes.py:100-101 gates it on `isSubmarine`).
///
/// `torp_name` and `sources` provide the torpedo module name and per-source modifier
/// table for provenance recording. `rec` accumulates attributions when `R::ON`.
///
/// Each field is `None` when its base projectile input is absent.
pub fn torpedo_stats<R: Recorder>(
    name: String,
    projectile: &Projectile,
    modifiers: &ModifierBundle,
    torp_name: &str,
    sources: &ModifierSources,
    rec: &mut R,
) -> TorpedoStats {
    let torp_src = || InputId::Module { slot: ModuleSlot::Torpedoes, name: torp_name.to_string() };
    // qualifier for per-ammo rows matches torpedo_qualifier in model.rs: the ammo name.
    let qualifier = name.as_str();

    // damage: getTorpedoDamage (ModifiersApply.py:477-488). Surface torpedoes take
    // the `alphaDamage / TORPEDO_DAMAGE_CONSTANT` branch (line 484); the submarine
    // citadel branch (line 480) needs SubmarineTorpedoParams not modeled here.
    // controllableWeaponDamageCoeff applies to all torpedoes (line 488, ungated).
    let damage = match (projectile.alpha_damage(), projectile.damage()) {
        (Some(alpha), Some(flood)) => {
            let base = alpha / TORPEDO_DAMAGE_CONSTANT + flood;
            let value = base * modifiers.coef("torpedoDamageCoeff") * modifiers.coef("controllableWeaponDamageCoeff");
            if R::ON {
                rec.record(TtxStat::TorpedoDamage, Some(qualifier), base, torp_src(), value, |b| {
                    b.coef(sources, "torpedoDamageCoeff");
                    b.coef(sources, "controllableWeaponDamageCoeff");
                });
            }
            Some(Hp::from(value))
        }
        _ => None,
    };

    // speed: getTorpedoSpeed (ModifiersApply.py:491-499). normalTorpedoSpeedMultiplier
    // only applies when ammoType == AMMO_TYPES.TORPEDO (line 495); deep-water/alt
    // torpedoes skip it (multiplier stays 1.0, unrecorded).
    let speed = projectile.speed().map(|s| {
        let is_torpedo = projectile.ammo_type() == AMMO_TYPE_TORPEDO;
        let normal = if is_torpedo { modifiers.coef("normalTorpedoSpeedMultiplier") } else { 1.0 };
        let value = s * modifiers.coef("torpedoSpeedMultiplier") * normal + modifiers.bonus("torpedoSpeedBonus");
        if R::ON {
            rec.record(TtxStat::TorpedoSpeed, Some(qualifier), s, torp_src(), value, |b| {
                b.coef(sources, "torpedoSpeedMultiplier");
                // normalTorpedoSpeedMultiplier only for ammoType == torpedo; skip it
                // (identity 1.0) for deep-water and alt torpedoes.
                if is_torpedo {
                    b.coef(sources, "normalTorpedoSpeedMultiplier");
                }
                b.bonus(sources, "torpedoSpeedBonus", 1.0);
            });
        }
        Knots::from(value)
    });

    // range: maxDist * torpedoRangeCoefficient * BW_TO_BALLISTIC / KM_TO_M (FactoryTorpedoes.py:93).
    let range = projectile.max_dist().map(|d| {
        let base = d.value() * BW_TO_BALLISTIC / KM_TO_M;
        let value = base * modifiers.coef("torpedoRangeCoefficient");
        if R::ON {
            rec.record(TtxStat::TorpedoRange, Some(qualifier), base, torp_src(), value, |b| {
                b.coef(sources, "torpedoRangeCoefficient");
            });
        }
        Km::from(value)
    });

    // visibility: visibilityFactor * torpedoVisibilityFactor (FactoryTorpedoes.py:92).
    let visibility = projectile.visibility_factor().map(|v| {
        let value = v * modifiers.coef("torpedoVisibilityFactor");
        if R::ON {
            rec.record(TtxStat::TorpedoVisibility, Some(qualifier), v, torp_src(), value, |b| {
                b.coef(sources, "torpedoVisibilityFactor");
            });
        }
        Km::from(value)
    });

    // isDamageIncreasing: distanceOfDamage max-coeff dist > min-coeff dist (FactoryTorpedoes.py:103-120).
    // distanceOfMaxDamage (line 119) also needs ammoParams.armingTime/maneuverDist,
    // which are not on the parsed Projectile, so it stays None (no fabrication).
    let is_damage_increasing = projectile.distance_of_damage().filter(|d| !d.is_empty()).map(|pairs| {
        let coeff_at_min_dist = pairs.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|p| p.0).unwrap_or(0.0);
        let coeff_at_max_dist = pairs.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|p| p.0).unwrap_or(0.0);
        coeff_at_max_dist > coeff_at_min_dist
    });
    if R::ON
        && let Some(flag) = is_damage_increasing
    {
        let v = if flag { 1.0 } else { 0.0 };
        rec.record(TtxStat::TorpedoIsDamageIncreasing, Some(qualifier), v, torp_src(), v, |_b| {});
    }

    let disabled_underwater: Option<bool> = None;
    if R::ON
        && let Some(flag) = disabled_underwater
    {
        let v = if flag { 1.0 } else { 0.0 };
        rec.record(TtxStat::TorpedoDisabledUnderwater, Some(qualifier), v, torp_src(), v, |_b| {});
    }

    TorpedoStats {
        name,
        damage,
        speed,
        range,
        visibility,
        distance_of_max_damage: None,
        is_damage_increasing,
        disabled_underwater,
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
/// Warn once per ammo name that an `ammoList` entry did not resolve to a
/// projectile, so the dropped shell/torpedo row is debuggable. Mirrors the
/// warn-once pattern in `armor_materials::collision_material_name`.
fn warn_unresolved_ammo(name: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    let mut warned = WARNED.lock().unwrap();
    if warned.get_or_insert_with(HashSet::new).insert(name.to_string()) {
        eprintln!("TTX: ammo '{name}' did not resolve to a projectile; shell/torpedo row dropped");
    }
}

/// `None` when `launchers` is empty (no torpedo armament).
pub fn torpedoes<R: Recorder>(
    launchers: &[TorpedoLauncherStats],
    modifiers: &ModifierBundle,
    reload_coeff: f32,
    provider: &dyn GameParamProvider,
    torp_name: &str,
    sources: &ModifierSources,
    rec: &mut R,
) -> Option<Torpedoes> {
    if launchers.is_empty() {
        return None;
    }

    let torp_src = || InputId::Module { slot: ModuleSlot::Torpedoes, name: torp_name.to_string() };

    // Real modifier names behind GunRotationSpeedModifiersStruct.yawSpeedCoef/yawSpeedBonus
    // (GunRotationSpeed.py:10-13, ModifiersApply.py:123).
    let yaw_coef = modifiers.coef("GTRotationSpeed");
    let yaw_bonus = modifiers.bonus("GTRotationSpeedBonus");
    let shot_delay_coef = modifiers.coef("GTShotDelay");

    let mut result_launchers = Vec::with_capacity(launchers.len());
    // Each element is (reload_value, shot_delay_base) for the min-reload attribution.
    let mut reload_candidates: Vec<(f32, f32)> = Vec::new();
    let mut seen_ammo: Vec<String> = Vec::new();
    let mut torpedoes = Vec::new();

    for (idx, launcher) in launchers.iter().enumerate() {
        let rotation_speed = launcher.rotation_speed.map(|r| r.value() * yaw_coef + yaw_bonus);
        let rotation_time = rotation_speed.filter(|&r| r != 0.0).map(|r| Seconds::from(180.0 / r));
        let num_barrels = launcher.num_barrels.map(|n| n as u32);

        if R::ON {
            let q = idx.to_string();
            if let Some(speed) = rotation_speed {
                let base_speed = launcher.rotation_speed.map(|r| r.value()).unwrap();
                rec.record(TtxStat::LauncherRotationSpeed, Some(&q), base_speed, torp_src(), speed, |b| {
                    b.coef(sources, "GTRotationSpeed");
                    b.bonus(sources, "GTRotationSpeedBonus", 1.0);
                });
            }
            if let Some(rt) = rotation_time {
                rec.record(TtxStat::LauncherRotationTime, Some(&q), rt.value(), torp_src(), rt.value(), |_b| {});
            }
            if let Some(nb) = num_barrels {
                rec.record(TtxStat::LauncherNumBarrels, Some(&q), nb as f32, torp_src(), nb as f32, |_b| {});
            }
        }

        result_launchers.push(Launcher {
            rotation_speed: rotation_speed.map(DegreesPerSecond::from),
            rotation_time,
            num_barrels,
        });

        if let Some(delay) = launcher.shot_delay {
            let reload = delay.value() * shot_delay_coef * reload_coeff;
            if reload != 0.0 {
                reload_candidates.push((reload, delay.value()));
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
                torpedoes.push(torpedo_stats(ammo_name.clone(), projectile, modifiers, torp_name, sources, rec));
            } else {
                warn_unresolved_ammo(ammo_name);
            }
        }
    }

    let reload_time = if reload_candidates.is_empty() {
        None
    } else {
        // Pick the launcher with the minimum reload (initAmmoReloadParams, FactoryTorpedoes.py:40).
        let (min_reload, min_base) = reload_candidates
            .iter()
            .copied()
            .reduce(|(ra, ba), (rb, bb)| if ra <= rb { (ra, ba) } else { (rb, bb) })?;
        if R::ON {
            // base * GTShotDelay * reloadCoeff == min_reload. Record the chosen launcher's
            // shot_delay as base; steps reproduce the final value exactly.
            rec.record(TtxStat::TorpedoReloadTime, None, min_base, torp_src(), min_reload, |b| {
                b.coef(sources, "GTShotDelay");
                // reload_coeff is the dynamic Adrenaline coefficient, not a player
                // input in sources. Record it as a module step so replay is exact.
                if reload_coeff != 1.0 {
                    b.module(torp_src(), "reloadCoeff", reload_coeff);
                }
            });
        }
        Some(Seconds::from(min_reload))
    };

    Some(Torpedoes { reload_time, launchers: result_launchers, torpedoes })
}

/// Caliber threshold constants (`me658a8e4.py:13,15`) are zeroed placeholders in
/// every deob source; the real values live in a compiled C++ module. Stock results
/// are unaffected (the gated coeffs read identity 1.0/0.0 from an empty bundle).
/// Passed as `0.0` here, matching the weapon_tables test convention.
const HEAVY_CRUISER_SHELL_DIAMETER_M: f32 = 0.0;
const SMALL_PROJECTILE_MAX_DIAMETER_M: f32 = 0.0;

/// The `TtxStat` variants a shell row maps to, selected by battery: the main
/// battery uses `Shell*`, the secondary battery the distinct `Secondary*` set.
/// Mirrors `model::ArtilleryStats::for_kind` so provenance keys match `rows()`.
struct ShellStatKeys {
    caliber: TtxStat,
    speed: TtxStat,
    damage: TtxStat,
    penetration: TtxStat,
    burn_chance: TtxStat,
    flood_chance: TtxStat,
    max_ammo: TtxStat,
}

impl ShellStatKeys {
    fn for_battery(is_atba: bool) -> Self {
        if is_atba {
            ShellStatKeys {
                caliber: TtxStat::SecondaryShellCaliber,
                speed: TtxStat::SecondaryShellSpeed,
                damage: TtxStat::SecondaryShellDamage,
                penetration: TtxStat::SecondaryShellPenetration,
                burn_chance: TtxStat::SecondaryShellBurnChance,
                flood_chance: TtxStat::SecondaryShellFloodChance,
                max_ammo: TtxStat::SecondaryShellMaxAmmo,
            }
        } else {
            ShellStatKeys {
                caliber: TtxStat::ShellCaliber,
                speed: TtxStat::ShellSpeed,
                damage: TtxStat::ShellDamage,
                penetration: TtxStat::ShellPenetration,
                burn_chance: TtxStat::ShellBurnChance,
                flood_chance: TtxStat::ShellFloodChance,
                max_ammo: TtxStat::ShellMaxAmmo,
            }
        }
    }
}

/// Per-shell stats from a resolved [`Projectile`] (`createAmmoTTX`,
/// FactoryArtillery.py:147-190). Pure over the projectile + bundle so the formulas
/// are testable without a provider. `level` is the ship tier (burn-chance branch,
/// ModifiersApply.py:43); main-battery shells take `max_ammo = Infinite` (the main
/// `getPreprocessedAmmoList` call passes no pool info, PreprocessedArtillery.py:33,
/// so every pool is `INFINITE_AMMO_POOL_SIZE = -1`, PreprocessedAmmo.py:6).
///
/// `is_atba` selects the secondary-battery path (FactoryArtillery.py:159-161): the
/// resolved [`StatWeaponType`] is mapped through `to_atba()` so `getAlphaDamageCoeff`
/// (ModifiersApply.py:459-463) and `getArtilleryDamageCoeff` (ModifiersApply.py:380-388)
/// read the `GS*` coefficients, and HE penetration uses `GSPenetrationCoeffHE`
/// (FactoryArtillery.py:175) rather than `GMPenetrationCoeffHE`.
///
/// `arty_name` and `sources` provide the artillery module name and per-source modifier
/// table for provenance recording. `rec` accumulates attributions when `R::ON`.
///
/// Each field is `None` when its base projectile input is absent.
// Threads recorder, modifier bundle, per-source provenance, level, and ammo-type flag alongside the base inputs.
#[allow(clippy::too_many_arguments)]
pub fn shell_stats<R: Recorder>(
    name: String,
    projectile: &Projectile,
    modifiers: &ModifierBundle,
    arty_name: &str,
    sources: &ModifierSources,
    level: u32,
    is_atba: bool,
    rec: &mut R,
) -> ShellStats {
    let ammo_kind = projectile.ammo_type();
    let mut weapon = ammo_to_stat_weapon_table(ammo_kind);
    if is_atba {
        // FactoryArtillery.py:159-161: map the main stat type to its ATBA equivalent.
        weapon = weapon.to_atba();
    }

    // Shell qualifier matches model.rs shell_qualifier: ammo_kind string (e.g. "HE", "AP").
    let qualifier = ammo_kind;
    let arty_src = || InputId::Module { slot: ModuleSlot::Artillery, name: arty_name.to_string() };

    // Select TtxStat variants based on battery path so provenance keys match rows().
    let stat = ShellStatKeys::for_battery(is_atba);

    // caliber * 1000 (FactoryArtillery.py:155). caliber (m) = bulletDiametr.
    let caliber_m = projectile.bullet_diametr();
    let caliber = caliber_m.map(|c| Millimeters::from(c * 1000.0));
    if R::ON {
        // Derived: base == final, no modifier steps.
        if let Some(c) = caliber {
            rec.record(stat.caliber, Some(qualifier), c.value(), arty_src(), c.value(), |_b| {});
        }
    }

    // speed = bulletSpeed * timeFactor (PreprocessedAmmo.py:16). timeFactor defaults to
    // 1.0 when absent (maa3520d6.py:1151, GameParams class attribute); most shells omit it,
    // but PXPA* event shells carry 0.5/0.75/2.2.
    let speed = projectile.bullet_speed().map(|s| MetersPerSecond::from(s * projectile.time_factor().unwrap_or(1.0)));
    if R::ON {
        // Derived: base == final, no modifier steps.
        if let Some(s) = speed {
            rec.record(stat.speed, Some(qualifier), s.value(), arty_src(), s.value(), |_b| {});
        }
    }

    // damage = alphaDamage * getAlphaDamageCoeff * controllableWeaponDamageCoeff
    //   * getArtilleryDamageCoeff * citadelCSAP(if weapon in WEAPONS_CSAP)
    // (FactoryArtillery.py:164). isManualATBA factor (line 156/164 `unknown_42`) is 1.0
    // on the main path. Stock coeffs are 1.0, so damage reduces to alphaDamage.
    let damage = match (projectile.alpha_damage(), caliber_m) {
        (Some(alpha), Some(cal_m)) => {
            let csap = if weapon.is_csap() { modifiers.coef("citadelDamageMultiplierCSAP") } else { 1.0 };
            let value = alpha
                * alpha_damage_coeff(weapon, modifiers, false)
                * modifiers.coef("controllableWeaponDamageCoeff")
                * artillery_damage_coeff(cal_m, weapon, modifiers, HEAVY_CRUISER_SHELL_DIAMETER_M)
                * csap;
            if R::ON {
                // alpha_damage_coeff and artillery_damage_coeff each fold several named
                // bundle coefficients internally. Record the net composite factor as a
                // single module step so replay is exact.
                let net_damage_coeff = value / alpha;
                rec.record(stat.damage, Some(qualifier), alpha, arty_src(), value, |b| {
                    b.coef(sources, "controllableWeaponDamageCoeff");
                    // Net of alpha_damage_coeff * artillery_damage_coeff * csap, collapsed
                    // to keep replay exact without decomposing the composite helpers.
                    let net_without_cwdc = net_damage_coeff / modifiers.coef("controllableWeaponDamageCoeff");
                    b.module(arty_src(), "damageCoeff", net_without_cwdc);
                });
            }
            Some(Hp::from(value))
        }
        _ => None,
    };

    // piercing: HE floor(alphaPiercingHE * penetrationCoeffHE) (FactoryArtillery.py:182);
    // the coef is GMPenetrationCoeffHE for main, GSPenetrationCoeffHE for ATBA
    // (FactoryArtillery.py:175/180). CS floor(alphaPiercingCS) (line 185); AP is a
    // ballistic sim (no closed form) -> None.
    let he_pen_coef =
        if is_atba { modifiers.coef("GSPenetrationCoeffHE") } else { modifiers.coef("GMPenetrationCoeffHE") };
    let penetration = match weapon {
        StatWeaponType::MainHe | StatWeaponType::AtbaHe => {
            projectile.alpha_piercing_he().map(|p| {
                let floored = (p * he_pen_coef).floor();
                if R::ON {
                    // Record pre-floor product so replay is exact within tolerance
                    // (floor breaks arithmetic replay by at most 1 mm).
                    let pre_floor = p * he_pen_coef;
                    let pen_coef_name = if is_atba { "GSPenetrationCoeffHE" } else { "GMPenetrationCoeffHE" };
                    rec.record(stat.penetration, Some(qualifier), p, arty_src(), pre_floor, |b| {
                        b.coef(sources, pen_coef_name)
                    });
                }
                Millimeters::from(floored)
            })
        }
        StatWeaponType::MainCs | StatWeaponType::AtbaCs => projectile.alpha_piercing_cs().map(|p| {
            if R::ON {
                rec.record(stat.penetration, Some(qualifier), p, arty_src(), p, |_b| {});
            }
            Millimeters::from(p.floor())
        }),
        _ => None,
    };

    // burnChance: calculateBurnChance(level, burnProb) for HE (line 171) and AP (line 188);
    // CS sets no burnChance. burnProb -0.5 (AP "N/A") clamps to 0 in calculate_burn_chance.
    // Stored as a percent (burnProb 0.12 -> 12%).
    let is_small = caliber_m.is_some_and(|c| is_small_projectile(c, SMALL_PROJECTILE_MAX_DIAMETER_M));
    let burn_chance = match weapon {
        StatWeaponType::MainHe | StatWeaponType::MainAp | StatWeaponType::AtbaHe | StatWeaponType::AtbaAp => {
            projectile.burn_prob().map(|bp| {
                let chance_val = calculate_burn_chance(level, bp, modifiers, is_small) * 100.0;
                if R::ON {
                    let base_pct = bp.max(0.0) * 100.0;
                    // calculate_burn_chance folds several named bundle coefficients internally.
                    // Record the net factor as one module step to keep replay exact.
                    let net_burn_coeff = if base_pct != 0.0 { chance_val / base_pct } else { 1.0 };
                    rec.record(stat.burn_chance, Some(qualifier), base_pct, arty_src(), chance_val, |b| {
                        b.module(arty_src(), "burnChanceCoeff", net_burn_coeff)
                    });
                }
                Percent::from(chance_val)
            })
        }
        _ => None,
    };

    // floodChance = uwCritical, HE only (FactoryArtillery.py:172). Stored as a percent.
    let flood_chance = match weapon {
        StatWeaponType::MainHe | StatWeaponType::AtbaHe => projectile.uw_critical().map(|f| {
            let value = f * 100.0;
            if R::ON {
                rec.record(stat.flood_chance, Some(qualifier), value, arty_src(), value, |_b| {});
            }
            Percent::from(value)
        }),
        _ => None,
    };

    // Main battery pools are unlimited (INFINITE_AMMO_POOL_SIZE = -1, PreprocessedAmmo.py:6).
    // Record as -1.0 so replay is exact; the model stores AmmoCount::Infinite.
    let max_ammo = Some(AmmoCount::Infinite);
    if R::ON {
        rec.record(stat.max_ammo, Some(qualifier), -1.0, arty_src(), -1.0, |_b| {});
    }

    ShellStats {
        name,
        ammo_kind: Some(ammo_kind.to_string()),
        damage,
        caliber,
        speed,
        penetration,
        burn_chance,
        flood_chance,
        max_ammo,
        // Deferred: FactoryArtillery.py:165 sets this from the hull's canBeUnderwater,
        // which is not threaded into this projectile-only path.
        disabled_underwater: None,
    }
}

/// Main-battery range in km: base component `maxDist` (BigWorld m) scaled by the
/// fire-control `maxDistCoef`, the `GMMaxDist` modifier, and an optional spotter
/// range coefficient (FactoryArtillery.py:42; spotter extends range by its
/// `artilleryDistCoeff`).
pub(crate) fn artillery_range_km(
    arty: &ArtilleryComponentStats,
    fc_max_dist_coef: f32,
    spotter_dist_coef: f32,
    modifiers: &ModifierBundle,
) -> Option<f32> {
    arty.max_dist.map(|d| (d.value() / KM_TO_M) * fc_max_dist_coef * modifiers.coef("GMMaxDist") * spotter_dist_coef)
}

/// Main-battery armament section (`ArtilleryTTX`, FactoryArtillery.py + TTXFactory.py).
///
/// `reload_time` = `gun.shotDelay * GMShotDelay` (FactoryArtillery.py:32 alt-fire analog;
/// the per-gun reload). `range` = `(maxDist / KM_TO_M) * fcMaxDistCoef * GMMaxDist`
/// (FactoryArtillery.py:42; `maxDist` is the component BigWorld range,
/// PreprocessedArtillery.py:32 divides by `KM_TO_M`). `dispersion` =
/// `getDispersionValue(gun, range_km, GMIdealRadius)` over the FC-adjusted range
/// (FactoryArtillery.py:47 passes the `* GMMaxDist`-scaled `unknown_12`).
/// `ammo_switch_time` = `shotDelay * ammoSwitchCoeff * GMShotDelay * switchAmmoReloadCoef`
/// (Components/Artillery.py:311, `ammoSwitchCoeff * switchAmmoReloadCoef`).
///
/// The gun (`MainGun`) mirrors `createMainGunTTX` (FactoryArtillery.py:70-76) +
/// `initGunTTX` (PreprocessedGun.py:18-23):
/// `rotation_speed = rotationSpeed[0] * GMRotationSpeed + GMRotationSpeedBonus`,
/// `rotation_time = 180 / rotation_speed`, `caliber = barrelDiameter * 1000`,
/// `num_barrels = gp.numBarrels`, `num_guns = HP_AGM mount count`.
///
/// `fc_max_dist_coef` is the fire-control `maxDistCoef` (FactoryArtillery.py:42, default
/// 1.0; M5 supplies the real value). `arty_name` and `fc_name` identify the module slots
/// for provenance attribution. `sources` and `rec` thread the recording context.
/// `None` when the component has no guns.
// Threads FC coefs, spotter coef, reload coef, recorder, modifier bundle, and per-source provenance alongside the base inputs.
#[allow(clippy::too_many_arguments)]
pub fn artillery<R: Recorder>(
    arty: &ArtilleryComponentStats,
    arty_name: &str,
    fc_name: Option<&str>,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    fc_max_dist_coef: f32,
    spotter_dist_coef: f32,
    reload_coeff: f32,
    level: u32,
    provider: &dyn GameParamProvider,
    rec: &mut R,
) -> Option<Artillery> {
    if arty.guns.is_empty() {
        return None;
    }

    let arty_src = || InputId::Module { slot: ModuleSlot::Artillery, name: arty_name.to_string() };

    let shot_delay_coef = modifiers.coef("GMShotDelay");
    let ideal_radius_coef = modifiers.coef("GMIdealRadius");
    let switch_coef = modifiers.coef("switchAmmoReloadCoef");
    let yaw_coef = modifiers.coef("GMRotationSpeed");
    let yaw_bonus = modifiers.bonus("GMRotationSpeedBonus");

    // All main-battery mounts share the same reload; take the first gun's shotDelay.
    let first = &arty.guns[0];

    let reload_time = first.shot_delay.map(|d| {
        let value = d.value() * shot_delay_coef * reload_coeff;
        if R::ON {
            rec.record(TtxStat::ArtilleryReloadTime, None, d.value(), arty_src(), value, |b| {
                b.coef(sources, "GMShotDelay");
                // reload_coeff is the dynamic Adrenaline coefficient, not a player
                // input in sources. Record it as a module step so replay is exact.
                b.module(arty_src(), "reloadCoeff", reload_coeff);
            });
        }
        Seconds::from(value)
    });

    let range_km = artillery_range_km(arty, fc_max_dist_coef, spotter_dist_coef, modifiers);
    let range = range_km.map(|rng| {
        if R::ON
            && let Some(base_km) = arty.max_dist.map(|d| d.value() / KM_TO_M)
        {
            let fc_src = fc_name
                .map(|n| InputId::Module { slot: ModuleSlot::FireControl, name: n.to_string() })
                .unwrap_or_else(arty_src);
            rec.record(TtxStat::ArtilleryRange, None, base_km, arty_src(), rng, |b| {
                b.module(fc_src, "maxDistCoef", fc_max_dist_coef);
                b.coef(sources, "GMMaxDist");
                // spotter_dist_coef is a consumable range extension, attributed
                // under the artillery module when no consumable InputId is available here.
                b.module(arty_src(), "spotterDistCoeff", spotter_dist_coef);
            });
        }
        Km::from(rng)
    });

    // dispersion ellipse over the FC-adjusted range (FactoryArtillery.py:47; getEllipse).
    let (dispersion, dispersion_vertical) = match (range_km, first.min_radius, first.ideal_radius, first.ideal_distance)
    {
        (Some(rng), Some(min_r), Some(ideal_r), Some(ideal_d)) => {
            let h = constants::dispersion_horizontal(min_r, ideal_r, ideal_d, Km::from(rng), ideal_radius_coef);
            let h_m = h.to_meters();
            let vertical = match (first.radius_on_zero, first.radius_on_delim, first.radius_on_max, first.delim) {
                (Some(z), Some(dl), Some(mx), Some(dm)) => {
                    let coeff = constants::clamped_dispersion_coeff(z, dl, mx, dm, Km::from(rng), Km::from(rng));
                    Some(((h * coeff).to_meters(), coeff))
                }
                _ => None,
            };
            if R::ON {
                let base_h = constants::dispersion_horizontal(min_r, ideal_r, ideal_d, Km::from(rng), 1.0).to_meters();
                rec.record(TtxStat::ArtilleryDispersion, None, base_h.value(), arty_src(), h_m.value(), |b| {
                    b.coef(sources, "GMIdealRadius")
                });
                if let Some((v, coeff)) = vertical {
                    // base_h * coeff absorbs the vertical scale so replay stays exact:
                    // base * GMIdealRadius = base_h * coeff * ideal_radius_coef = v.
                    rec.record(
                        TtxStat::ArtilleryDispersionVertical,
                        None,
                        base_h.value() * coeff,
                        arty_src(),
                        v.value(),
                        |b| b.coef(sources, "GMIdealRadius"),
                    );
                }
            }
            (Some(h_m), vertical.map(|(v, _)| v))
        }
        _ => (None, None),
    };

    let ammo_switch_time = match (first.shot_delay, first.ammo_switch_coeff) {
        (Some(delay), Some(coeff)) => {
            let value = delay.value() * coeff * shot_delay_coef * switch_coef;
            if R::ON {
                rec.record(TtxStat::ArtilleryAmmoSwitchTime, None, delay.value() * coeff, arty_src(), value, |b| {
                    b.coef(sources, "GMShotDelay");
                    b.coef(sources, "switchAmmoReloadCoef");
                });
            }
            Some(Seconds::from(value))
        }
        _ => None,
    };

    let rotation_speed = first.rotation_speed.map(|r| r.value() * yaw_coef + yaw_bonus);
    let rotation_time = rotation_speed.filter(|&r| r != 0.0).map(|r| Seconds::from(180.0 / r));

    if R::ON {
        if let Some(speed) = rotation_speed {
            let base_speed = first.rotation_speed.map(|r| r.value()).unwrap();
            rec.record(TtxStat::GunRotationSpeed, None, base_speed, arty_src(), speed, |b| {
                b.coef(sources, "GMRotationSpeed");
                b.bonus(sources, "GMRotationSpeedBonus", 1.0);
            });
        }
        if let Some(rt) = rotation_time {
            rec.record(TtxStat::GunRotationTime, None, rt.value(), arty_src(), rt.value(), |_b| {});
        }
    }

    let caliber = first.barrel_diameter.map(|b| b.to_mm());
    let num_barrels = first.num_barrels.map(|n| n as u32);
    let num_guns = arty.guns.len() as u32;

    if R::ON {
        if let Some(c) = caliber {
            rec.record(TtxStat::GunCaliber, None, c.value(), arty_src(), c.value(), |_b| {});
        }
        if let Some(nb) = num_barrels {
            rec.record(TtxStat::GunNumBarrels, None, nb as f32, arty_src(), nb as f32, |_b| {});
        }
        rec.record(TtxStat::GunNumGuns, None, num_guns as f32, arty_src(), num_guns as f32, |_b| {});
    }

    let gun = Some(MainGun {
        caliber,
        num_barrels,
        num_guns: Some(num_guns),
        rotation_speed: rotation_speed.map(DegreesPerSecond::from),
        rotation_time,
    });

    // Shell stats resolved by NAME from the first gun's ammoList (all mounts share ammo).
    let mut shells = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for ammo_name in &first.ammo {
        if seen.iter().any(|n| n == ammo_name) {
            continue;
        }
        seen.push(ammo_name.clone());
        if let Some(param) = provider.game_param_by_name(ammo_name)
            && let Some(projectile) = param.projectile()
        {
            shells.push(shell_stats(ammo_name.clone(), projectile, modifiers, arty_name, sources, level, false, rec));
        } else {
            warn_unresolved_ammo(ammo_name);
        }
    }

    Some(Artillery { reload_time, range, dispersion, dispersion_vertical, ammo_switch_time, gun, shells })
}

/// Secondary-battery (ATBA) armament section (`createATBAGunTTX`,
/// FactoryArtillery.py:79-87). Mirrors [`artillery`] but reads the `GS*` modifier
/// coefficients and resolves shells through the ATBA stat-weapon path.
///
/// `reload_time` = `gun.shotDelay * GSShotDelay` (FactoryArtillery.py:83).
/// `range` = `atba.maxDist * GSMaxDist` (FactoryArtillery.py:84). The deob's
/// `atba.maxDist` is in KM (PreprocessedATBA.py:30 stores `module.maxDist / KM_TO_M`);
/// [`SecondaryComponentStats::max_dist`] keeps the raw BigWorld value, so this divides
/// by `KM_TO_M` here. Secondaries have no fire-control `maxDistCoef`
/// (FactoryArtillery.py:84 omits it, unlike the main-battery line 42).
/// `dispersion` = `getDispersionValue(gun, range_km, GSIdealRadius)` over that range
/// (FactoryArtillery.py:84). The gun `rotation_speed` =
/// `rotationSpeed[0] * GSRotationSpeed + GSRotationSpeedBonus` (initGunTTX +
/// createMainGunTTX analog, FactoryArtillery.py:74 with the GS coefficient names).
///
/// Secondaries mount mixed calibers (e.g. Bismarck's 150mm + 105mm guns), so the deob
/// builds a per-gun-group `ATBAGunTTX` list (ArtilleryTTX.atba) sharing one
/// component-level `atbaMaxDist`. This single-[`Artillery`] view takes the first gun
/// group for `reload_time`/`gun` (as [`artillery`] does for main mounts) and lists a
/// shell per distinct ATBA ammo name across all mounts; `range`/`dispersion` use the
/// component `maxDist`. `None` when the component has no guns.
// Threads recorder, modifier bundle, per-source provenance, reload coef, and level alongside the base inputs.
#[allow(clippy::too_many_arguments)]
pub fn secondaries<R: Recorder>(
    atba: &SecondaryComponentStats,
    hull_name: &str,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    reload_coeff: f32,
    level: u32,
    provider: &dyn GameParamProvider,
    rec: &mut R,
) -> Option<Artillery> {
    if atba.guns.is_empty() {
        return None;
    }

    let hull_src = || InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() };

    let shot_delay_coef = modifiers.coef("GSShotDelay");
    let max_dist_coef = modifiers.coef("GSMaxDist");
    let ideal_radius_coef = modifiers.coef("GSIdealRadius");
    let yaw_coef = modifiers.coef("GSRotationSpeed");
    let yaw_bonus = modifiers.bonus("GSRotationSpeedBonus");

    // First gun group drives the displayed reload/gun (FactoryArtillery.py:83 is per gun).
    let first = &atba.guns[0];

    let reload_time = first.shot_delay.map(|d| {
        let value = d.value() * shot_delay_coef * reload_coeff;
        if R::ON {
            rec.record(TtxStat::SecondaryReloadTime, None, d.value(), hull_src(), value, |b| {
                b.coef(sources, "GSShotDelay");
                // reload_coeff is the dynamic Adrenaline coefficient. Record as a module
                // step so base * GSShotDelay * reloadCoeff replays exactly.
                b.module(hull_src(), "reloadCoeff", reload_coeff);
            });
        }
        Seconds::from(value)
    });

    // range = (maxDist / KM_TO_M) * GSMaxDist (FactoryArtillery.py:84 over the KM
    // maxDist of PreprocessedATBA.py:30). No fire-control coef for secondaries.
    let range_km = atba.max_dist.map(|d| (d.value() / KM_TO_M) * max_dist_coef);
    let range = range_km.map(|rng| {
        if R::ON
            && let Some(base_km) = atba.max_dist.map(|d| d.value() / KM_TO_M)
        {
            rec.record(TtxStat::SecondaryRange, None, base_km, hull_src(), rng, |b| {
                b.coef(sources, "GSMaxDist");
            });
        }
        Km::from(rng)
    });

    let (dispersion, dispersion_vertical) = match (range_km, first.min_radius, first.ideal_radius, first.ideal_distance)
    {
        (Some(rng), Some(min_r), Some(ideal_r), Some(ideal_d)) => {
            let h = constants::dispersion_horizontal(min_r, ideal_r, ideal_d, Km::from(rng), ideal_radius_coef);
            let h_m = h.to_meters();
            let vertical = match (first.radius_on_zero, first.radius_on_delim, first.radius_on_max, first.delim) {
                (Some(z), Some(dl), Some(mx), Some(dm)) => {
                    let coeff = constants::clamped_dispersion_coeff(z, dl, mx, dm, Km::from(rng), Km::from(rng));
                    Some(((h * coeff).to_meters(), coeff))
                }
                _ => None,
            };
            if R::ON {
                let base_h = constants::dispersion_horizontal(min_r, ideal_r, ideal_d, Km::from(rng), 1.0).to_meters();
                rec.record(TtxStat::SecondaryDispersion, None, base_h.value(), hull_src(), h_m.value(), |b| {
                    b.coef(sources, "GSIdealRadius");
                });
                if let Some((v, coeff)) = vertical {
                    // base_h * coeff absorbs the vertical scale so replay stays exact:
                    // base * GSIdealRadius = base_h * coeff * ideal_radius_coef = v.
                    rec.record(
                        TtxStat::SecondaryDispersionVertical,
                        None,
                        base_h.value() * coeff,
                        hull_src(),
                        v.value(),
                        |b| b.coef(sources, "GSIdealRadius"),
                    );
                }
            }
            (Some(h_m), vertical.map(|(v, _)| v))
        }
        _ => (None, None),
    };

    let rotation_speed = first.rotation_speed.map(|r| r.value() * yaw_coef + yaw_bonus);
    let rotation_time = rotation_speed.filter(|&r| r != 0.0).map(|r| Seconds::from(180.0 / r));

    if R::ON {
        if let Some(speed) = rotation_speed {
            let base_speed = first.rotation_speed.map(|r| r.value()).unwrap();
            rec.record(TtxStat::SecondaryGunRotationSpeed, None, base_speed, hull_src(), speed, |b| {
                b.coef(sources, "GSRotationSpeed");
                b.bonus(sources, "GSRotationSpeedBonus", 1.0);
            });
        }
        if let Some(rt) = rotation_time {
            rec.record(TtxStat::SecondaryGunRotationTime, None, rt.value(), hull_src(), rt.value(), |_b| {});
        }
    }

    let caliber = first.barrel_diameter.map(|b| b.to_mm());
    let num_barrels = first.num_barrels.map(|n| n as u32);
    let num_guns = atba.guns.len() as u32;

    if R::ON {
        if let Some(c) = caliber {
            rec.record(TtxStat::SecondaryGunCaliber, None, c.value(), hull_src(), c.value(), |_b| {});
        }
        if let Some(nb) = num_barrels {
            rec.record(TtxStat::SecondaryGunNumBarrels, None, nb as f32, hull_src(), nb as f32, |_b| {});
        }
        rec.record(TtxStat::SecondaryGunNumGuns, None, num_guns as f32, hull_src(), num_guns as f32, |_b| {});
    }

    let gun = Some(MainGun {
        caliber,
        num_barrels,
        num_guns: Some(num_guns),
        rotation_speed: rotation_speed.map(DegreesPerSecond::from),
        rotation_time,
    });

    // One shell per distinct ATBA ammo name across every mount (mixed calibers).
    let mut shells = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for gun_stats in &atba.guns {
        for ammo_name in &gun_stats.ammo {
            if seen.iter().any(|n| n == ammo_name) {
                continue;
            }
            seen.push(ammo_name.clone());
            if let Some(param) = provider.game_param_by_name(ammo_name)
                && let Some(projectile) = param.projectile()
            {
                // Pass hull_name as arty_name: shell attribution appears under the hull
                // module (the ATBA is referenced by the selected hull, not a separate
                // artillery slot).
                shells.push(shell_stats(
                    ammo_name.clone(),
                    projectile,
                    modifiers,
                    hull_name,
                    sources,
                    level,
                    true,
                    rec,
                ));
            } else {
                warn_unresolved_ammo(ammo_name);
            }
        }
    }

    Some(Artillery {
        reload_time,
        range,
        dispersion,
        dispersion_vertical,
        // Secondaries have no ammo switch (single shell type per gun, FactoryArtillery.py omits it).
        ammo_switch_time: None,
        gun,
        shells,
    })
}

/// `MINIMAL_VALID_VALUE` smoke-detection gate (createVisibilityTTX@140): the in-smoke
/// range is shown only when `visibilityCoefGKInSmoke` exceeds it. The compiled module
/// that defines the constant carries 0.01; below it the field is a zeroed placeholder.
const MINIMAL_VALID_VALUE: f32 = 0.01;

/// Detectability section (`ma6320f36/ttx/FactoryVisibility.pyc` createVisibilityTTX,
/// offsets from the bytecode disassembly).
///
/// `coeff` (@21,@49-58) = `mod.visibilityDistCoeff`, multiplied by
/// `mod.GMBigGunVisibilityCoeff` when the ship has non-small artillery
/// (`artillery and not artillery.isSmall`, @30-58); `has_big_gun_artillery` is that
/// gate. Both modifiers are coefficients (MODIFIER_SETTINGS base_value 1.0).
///
/// `sea_detection` (@65-94) = `hull.visibilityFactor * mod.visibilityFactor * coeff`.
/// `sea_detection_on_fire` (@97-128) = `sea + hull.visibilityCoefFire`.
/// `detection_in_smoke` (@131-167) = `hull.visibilityCoefGKInSmoke`, only when it
/// exceeds `MINIMAL_VALID_VALUE`. `air_detection` (@278-307) =
/// `hull.visibilityFactorByPlane * mod.visibilityFactorByPlane * coeff`;
/// `air_detection_on_fire` (@310-341) = `air + hull.visibilityCoefFireByPlane`.
/// `main_gun_range_detection` (@188-224) = `max(sea, mgMaxDist)` when the main-battery
/// range is supplied; `secondary_range_detection` (@227-272) = `max(sea, atbaMaxDist)`
/// when the secondary range is supplied. `periscope_depth_detection` (@359-384) =
/// `hull.visibilityByPeriscope * mod.visibilityForSubmarineCoeff`, present only for subs.
///
/// Per-depth submarine ranges (`byDepth`, @387-513) are a runtime entity calc
/// (`ShipParams.getVehicleParams` + `getPerDepthRangeVisiblity`) and are deferred.
///
/// Each field is `None` when its base hull input is absent.
// Threads recorder, modifier bundle, per-source provenance, big-gun flag, and range inputs alongside the base hull stats.
#[allow(clippy::too_many_arguments)]
pub fn visibility<R: Recorder>(
    hull: &HullComponentStats,
    hull_name: &str,
    modifiers: &ModifierBundle,
    sources: &ModifierSources,
    has_big_gun_artillery: bool,
    mg_max_dist_km: Option<f32>,
    atba_max_dist_km: Option<f32>,
    rec: &mut R,
) -> Visibility {
    let hull_src = || InputId::Module { slot: ModuleSlot::Hull, name: hull_name.to_string() };

    let mut coeff = modifiers.coef("visibilityDistCoeff");
    if has_big_gun_artillery {
        coeff *= modifiers.coef("GMBigGunVisibilityCoeff");
    }

    let sea = hull.visibility_factor.map(|v| v.value() * modifiers.coef("visibilityFactor") * coeff);
    let sea_detection = sea.map(Km::from);
    if R::ON
        && let (Some(sea_val), Some(base)) = (sea, hull.visibility_factor)
    {
        rec.record(TtxStat::SeaDetection, None, base.value(), hull_src(), sea_val, |b| {
            b.coef(sources, "visibilityFactor");
            b.coef(sources, "visibilityDistCoeff");
            if has_big_gun_artillery {
                b.coef(sources, "GMBigGunVisibilityCoeff");
            }
        });
    }

    let sea_detection_on_fire = match (sea, hull.visibility_coef_fire) {
        (Some(s), Some(fire)) => {
            let value = s + fire.value();
            if R::ON {
                rec.record(TtxStat::SeaDetectionOnFire, None, s, hull_src(), value, |b| {
                    b.module_add(hull_src(), "visibilityCoefFire", fire.value());
                });
            }
            Some(Km::from(value))
        }
        _ => None,
    };

    let detection_in_smoke = hull.visibility_coef_gk_in_smoke.filter(|&v| v.value() > MINIMAL_VALID_VALUE).map(|v| {
        if R::ON {
            rec.record(TtxStat::DetectionInSmoke, None, v.value(), hull_src(), v.value(), |_b| {});
        }
        Km::from(v.value())
    });

    // visibilityByShip.mg (@188-224) and .atba (@227-272) are distinct slots: each is its
    // own battery range floored by `sea`, gated on that battery being present. mg is the
    // main battery ("after firing a main gun shell"), atba the secondaries ("after firing
    // a secondary gun shell"). The max is not multiplicative; record base == final with no
    // steps so replay is exact.
    let main_gun_range_detection = match (sea, mg_max_dist_km) {
        (Some(s), Some(mg)) => {
            let value = s.max(mg);
            if R::ON {
                rec.record(TtxStat::MainGunRangeDetection, None, value, hull_src(), value, |_b| {});
            }
            Some(Km::from(value))
        }
        _ => None,
    };
    let secondary_range_detection = match (sea, atba_max_dist_km) {
        (Some(s), Some(atba)) => {
            let value = s.max(atba);
            if R::ON {
                rec.record(TtxStat::SecondaryRangeDetection, None, value, hull_src(), value, |_b| {});
            }
            Some(Km::from(value))
        }
        _ => None,
    };

    let air = hull.visibility_factor_by_plane.map(|v| v.value() * modifiers.coef("visibilityFactorByPlane") * coeff);
    let air_detection = air.map(Km::from);
    if R::ON
        && let (Some(air_val), Some(base)) = (air, hull.visibility_factor_by_plane)
    {
        rec.record(TtxStat::AirDetection, None, base.value(), hull_src(), air_val, |b| {
            b.coef(sources, "visibilityFactorByPlane");
            b.coef(sources, "visibilityDistCoeff");
            if has_big_gun_artillery {
                b.coef(sources, "GMBigGunVisibilityCoeff");
            }
        });
    }

    let air_detection_on_fire = match (air, hull.visibility_coef_fire_by_plane) {
        (Some(a), Some(fire)) => {
            let value = a + fire.value();
            if R::ON {
                rec.record(TtxStat::AirDetectionOnFire, None, a, hull_src(), value, |b| {
                    b.module_add(hull_src(), "visibilityCoefFireByPlane", fire.value());
                });
            }
            Some(Km::from(value))
        }
        _ => None,
    };

    let periscope_depth_detection = hull.visibility_factor_by_periscope.map(|v| {
        let value = v.value() * modifiers.coef("visibilityForSubmarineCoeff");
        if R::ON {
            rec.record(TtxStat::PeriscopeDepthDetection, None, v.value(), hull_src(), value, |b| {
                b.coef(sources, "visibilityForSubmarineCoeff");
            });
        }
        Km::from(value)
    });

    Visibility {
        sea_detection,
        sea_detection_on_fire,
        air_detection,
        air_detection_on_fire,
        detection_in_smoke,
        main_gun_range_detection,
        secondary_range_detection,
        periscope_depth_detection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::ttx::components::ArtilleryGunStats;
    use crate::game_params::ttx::constants::DEFAULT_UW_DAMAGE_COEFF;
    use crate::game_params::ttx::provenance::ModifierSources;
    use crate::game_params::ttx::provenance::Off;
    use crate::game_params::types::CrewSkillModifier;
    use crate::game_params::types::Species;

    /// The version at which the toolkit's `MODIFIER_SETTINGS` table takes effect.
    const VERSION: crate::data::Version = crate::data::Version::base(15, 0, 0);

    /// Gearing's real default-hull base stats (GameParams `PASD013_Gearing_1945`
    /// `A_Hull`): health 19400, maxSpeed 36, speedCoef 1.0, turningRadius 640,
    /// rudderTime 4.25, visibilityFactor 7.33. `floodNodes[0][0]` is 0.333 (==
    /// DEFAULT_UW_DAMAGE_COEFF), so flood_prob is 0.0; no SubmarineBattery (DD).
    fn gearing_hull() -> HullComponentStats {
        HullComponentStats {
            health: Some(Hp::from(19400.0)),
            max_speed: Some(Knots::from(36.0)),
            speed_coef: Some(1.0),
            turning_radius: Some(Meters::from(640.0)),
            rudder_time: Some(Seconds::from(4.25)),
            visibility_factor: Some(Km::from(7.33)),
            visibility_factor_by_plane: Some(Km::from(3.41)),
            visibility_coef_fire: Some(Km::from(2.0)),
            visibility_coef_fire_by_plane: Some(Km::from(2.0)),
            visibility_coef_gk: Some(Km::from(1e-6)),
            visibility_coef_gk_in_smoke: Some(Km::from(2.83)),
            visibility_factor_by_periscope: None,
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
        let durability = durability(
            &gearing_hull(),
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            10,
            &mut Off,
        );
        // ceil(19400 / 50) * 50 = 19400 (healthPerLevel 0, healthHullCoeff 1).
        assert_eq!(durability.health, Some(Hp::from(19400.0)));
    }

    #[test]
    fn gearing_stock_durability_ptz_zero() {
        // Gearing floodNodes[0][0] == DEFAULT_UW_DAMAGE_COEFF -> flood_prob 0 -> ptz 0.
        let durability = durability(
            &gearing_hull(),
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            10,
            &mut Off,
        );
        assert_eq!(durability.torpedo_protection, Some(Percent::from(0.0)));
    }

    #[test]
    fn yamato_stock_durability_ptz() {
        // flood_prob * 1.0 (stock uwCoeffMultiplier) * 100 + 0 (stock uwCoeffBonus).
        // (0.333 - 0.15) / 0.333 * 100 = 54.9549... (Yamato in-game ptz ~55%).
        let durability = durability(
            &yamato_hull(),
            "H",
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            10,
            &mut Off,
        );
        let ptz = durability.torpedo_protection.expect("ptz computed").value();
        let expected = (DEFAULT_UW_DAMAGE_COEFF - 0.15) / DEFAULT_UW_DAMAGE_COEFF * 100.0;
        assert!((ptz - expected).abs() < 1e-4, "got {ptz}, expected {expected}");
        assert!((ptz - 54.954956).abs() < 1e-3, "got {ptz}");
    }

    #[test]
    fn yamato_ptz_modifier_applies() {
        // uwCoeffBonus +25 (additive) shifts ptz by +25: 54.9549... + 25 = 79.9549...
        let mods = [modifier("uwCoeffBonus", 25.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let durability = durability(&yamato_hull(), "H", &bundle, &ModifierSources::default(), 10, &mut Off);
        let ptz = durability.torpedo_protection.expect("ptz computed").value();
        let expected = (DEFAULT_UW_DAMAGE_COEFF - 0.15) / DEFAULT_UW_DAMAGE_COEFF * 100.0 + 25.0;
        assert!((ptz - expected).abs() < 1e-4, "got {ptz}, expected {expected}");
    }

    #[test]
    fn ptz_none_when_flood_absent() {
        // No floodNodes -> flood_prob None -> ptz None (not fabricated).
        let hull = HullComponentStats::default();
        let durability = durability(
            &hull,
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            10,
            &mut Off,
        );
        assert!(durability.torpedo_protection.is_none());
    }

    #[test]
    fn gearing_stock_mobility() {
        let mobility = mobility(
            &gearing_hull(),
            "H",
            &gearing_engine(),
            Some("E"),
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            &mut Off,
        );
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
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let mobility = mobility(
            &gearing_hull(),
            "H",
            &gearing_engine(),
            Some("E"),
            &bundle,
            &ModifierSources::default(),
            &mut Off,
        );
        let speed = mobility.speed.expect("speed computed").value();
        assert!((speed - 37.8).abs() < 1e-4, "got {speed}");
    }

    #[test]
    fn health_modifier_applies() {
        // healthPerLevel 350 (bonus, +) and healthHullCoeff 1.05 (coef, *):
        // (19400 + 350*10) * 1.05 = 22900*1.05 = 24045 -> ceil(24045/50)*50 = 24050.
        let mods = [modifier("healthPerLevel", 350.0), modifier("healthHullCoeff", 1.05)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let durability = durability(&gearing_hull(), "H", &bundle, &ModifierSources::default(), 10, &mut Off);
        assert_eq!(durability.health, Some(Hp::from(24050.0)));
    }

    #[test]
    fn absent_inputs_are_none() {
        let empty_hull = HullComponentStats::default();
        let durability = durability(
            &empty_hull,
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            10,
            &mut Off,
        );
        assert!(durability.health.is_none());

        let mobility = mobility(
            &empty_hull,
            "H",
            &EngineComponentStats::default(),
            Some("E"),
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            &mut Off,
        );
        assert!(mobility.speed.is_none());
        assert!(mobility.turning_radius.is_none());
        assert!(mobility.rudder_time.is_none());

        // No SubmarineBattery -> battery None.
        assert!(
            battery(
                &empty_hull,
                "H",
                &ModifierBundle::empty(Species::Submarine),
                &ModifierSources::default(),
                &mut Off
            )
            .is_none()
        );
    }

    #[test]
    fn balao_stock_battery() {
        // capacity 240 * 1.0, regenRate 1.2 * 1.0 (stock battery coeffs).
        let battery = battery(
            &balao_hull(),
            "H",
            &ModifierBundle::empty(Species::Submarine),
            &ModifierSources::default(),
            &mut Off,
        )
        .expect("battery computed");
        assert_eq!(battery.capacity, Some(240.0));
        assert_eq!(battery.regeneration, Some(1.2));
    }

    #[test]
    fn balao_battery_modifiers_apply() {
        // batteryCapacityCoeff 1.1 (coef) -> 240*1.1=264; batteryRegenCoeff 1.25 -> 1.2*1.25=1.5.
        let mods = [modifier("batteryCapacityCoeff", 1.1), modifier("batteryRegenCoeff", 1.25)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Submarine, VERSION).expect("test modifiers are all known");
        let battery =
            battery(&balao_hull(), "H", &bundle, &ModifierSources::default(), &mut Off).expect("battery computed");
        let capacity = battery.capacity.expect("capacity");
        let regen = battery.regeneration.expect("regen");
        assert!((capacity - 264.0).abs() < 1e-4, "got {capacity}");
        assert!((regen - 1.5).abs() < 1e-4, "got {regen}");
    }

    #[test]
    fn battery_none_for_non_sub() {
        // Gearing has no SubmarineBattery -> battery None.
        assert!(
            battery(
                &gearing_hull(),
                "H",
                &ModifierBundle::empty(Species::Destroyer),
                &ModifierSources::default(),
                &mut Off
            )
            .is_none()
        );
    }

    #[test]
    fn durability_records_health_provenance() {
        use crate::game_params::ttx::provenance::{InputId, ModifierSources, On, Op, Recorder, ShipStatsProvenance};
        let mods = [modifier("healthPerLevel", 350.0), modifier("healthHullCoeff", 1.05)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).unwrap();
        let mut sources = ModifierSources::default();
        sources.record("healthPerLevel", InputId::Upgrade { name: "U".into() }, 350.0);
        sources.record("healthHullCoeff", InputId::Skill { name: "S".into() }, 1.05);

        let mut rec = On::default();
        let _ = durability(&gearing_hull(), "HULL", &bundle, &sources, 10, &mut rec);
        let prov = rec.into_provenance();
        let health = prov.attributions.iter().find(|a| a.stat == TtxStat::Health).unwrap();
        assert_eq!(health.base_value, 19400.0);
        assert!(
            matches!(&health.base_source, InputId::Module { slot, .. } if *slot == crate::game_params::ttx::module_options::ModuleSlot::Hull)
        );
        // base + 350*10 (add), * 1.05 (mul): replay reproduces the pre-round raw.
        let expected = (19400.0 + 350.0 * 10.0) * 1.05;
        assert!((ShipStatsProvenance::replay(health) - expected).abs() < 1e-1);
        assert_eq!(health.steps.iter().filter(|c| c.op == Op::Add).count(), 1);
        assert_eq!(health.steps.iter().filter(|c| c.op == Op::Mul).count(), 1);
    }

    /// Build an [`ArmorMap`] from `(raw_key, thickness)` pairs, mirroring
    /// `parse_armor_dict`'s `(model_index << 16) | material_id` keying.
    fn armor_map(entries: &[(u32, f32)]) -> ArmorMap {
        let mut m: ArmorMap = std::collections::HashMap::new();
        for &(raw, thk) in entries {
            let model_index = raw >> 16;
            let material_id = raw & 0xFFFF;
            m.entry(material_id).or_default().insert(model_index, thk);
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
        let armor = armor(&hull, "HULL", arti.iter(), &mut Off).expect("armor computed");
        assert_eq!(armor.max, Some(Millimeters::from(650.0)));
        assert_eq!(armor.min, Some(Millimeters::from(19.0)));
    }

    #[test]
    fn hull_only_armor_excludes_unclassified() {
        // No artillery branch: extremes over classified hull plates only.
        // RudderSide (350) is excluded, so max = 560 (Tur1GkBar), min = 19.
        let hull = yamato_hull_armor();
        let armor = armor(&hull, "HULL", std::iter::empty(), &mut Off).expect("armor computed");
        assert_eq!(armor.max, Some(Millimeters::from(560.0)));
        assert_eq!(armor.min, Some(Millimeters::from(19.0)));
    }

    #[test]
    fn armor_none_when_hull_armor_absent() {
        // No armor data at all -> None (not fabricated as the default 6mm).
        let empty: ArmorMap = std::collections::HashMap::new();
        assert!(armor(&empty, "HULL", std::iter::empty(), &mut Off).is_none());
    }

    #[test]
    fn armor_defaults_when_no_classified_plate() {
        // Hull with only unclassified plates -> default 6mm extremes
        // (PreprocessedArmor.py:8 seed), not None: the hull map is non-empty.
        let hull = armor_map(&[(131072 | 82, 350.0), (131072 | 80, 200.0)]); // Rudder*
        let armor = armor(&hull, "HULL", std::iter::empty(), &mut Off).expect("armor computed");
        assert_eq!(armor.min, Some(Millimeters::from(6.0)));
        assert_eq!(armor.max, Some(Millimeters::from(6.0)));
    }

    use crate::Rc;
    use crate::game_params::types::Param;
    use crate::game_params::types::ParamData;
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
            shot_delay: Some(Seconds::from(103.0)),
            rotation_speed: Some(DegreesPerSecond::from(25.0)),
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
        let stats = torpedo_stats(
            "PAPT027_Mk_16_mod_1".to_string(),
            &gearing_torpedo(),
            &ModifierBundle::empty(Species::Destroyer),
            "",
            &ModifierSources::default(),
            &mut Off,
        );
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
        let torps = torpedoes(
            &launchers,
            &ModifierBundle::empty(Species::Destroyer),
            1.0,
            &provider,
            "",
            &ModifierSources::default(),
            &mut Off,
        )
        .expect("torpedoes computed");

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
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let stats = torpedo_stats(
            "PAPT027_Mk_16_mod_1".to_string(),
            &gearing_torpedo(),
            &bundle,
            "",
            &ModifierSources::default(),
            &mut Off,
        );
        let speed = stats.speed.expect("speed").value();
        assert!(approx(speed, 71.0), "got {speed}");
    }

    #[test]
    fn torpedo_damage_coeff_applies() {
        // torpedoDamageCoeff 1.2 (coef): (53500/3 + 1200) * 1.2.
        let mods = [modifier("torpedoDamageCoeff", 1.2)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let stats = torpedo_stats(
            "PAPT027_Mk_16_mod_1".to_string(),
            &gearing_torpedo(),
            &bundle,
            "",
            &ModifierSources::default(),
            &mut Off,
        );
        let damage = stats.damage.expect("damage").value();
        assert!(approx(damage, (53500.0 / 3.0 + 1200.0) * 1.2), "got {damage}");
    }

    #[test]
    fn torpedo_launcher_traverse_coef_applies() {
        // GTRotationSpeed 1.2 (Torpedo_Mod_II, +20% traverse), real modifier name
        // mapped from GunRotationSpeedModifiersStruct.yawSpeedCoef
        // (GunRotationSpeed.py:10-13, ModifiersApply.py:123). base 25 -> 30, time 180/30 = 6.
        let mods = [modifier("GTRotationSpeed", 1.2)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let launchers = [gearing_launcher()];
        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        let torps = torpedoes(&launchers, &bundle, 1.0, &provider, "", &ModifierSources::default(), &mut Off)
            .expect("torpedoes computed");
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
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let launchers = [gearing_launcher()];
        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        let torps = torpedoes(&launchers, &bundle, 1.0, &provider, "", &ModifierSources::default(), &mut Off)
            .expect("torpedoes computed");
        let launcher = &torps.launchers[0];
        assert_eq!(launcher.rotation_speed, Some(DegreesPerSecond::from(30.0)));
        let rt = launcher.rotation_time.expect("rotation_time").value();
        assert!(approx(rt, 6.0), "got {rt}");
    }

    #[test]
    fn torpedoes_none_when_no_launchers() {
        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        assert!(
            torpedoes(
                &[],
                &ModifierBundle::empty(Species::Destroyer),
                1.0,
                &provider,
                "",
                &ModifierSources::default(),
                &mut Off
            )
            .is_none()
        );
    }

    #[test]
    fn torpedo_stats_none_when_inputs_absent() {
        // A projectile with no torpedo fields -> all stats None (no fabrication).
        let empty = Projectile::builder().ammo_type("torpedo".to_string()).build();
        let stats = torpedo_stats(
            "X".to_string(),
            &empty,
            &ModifierBundle::empty(Species::Destroyer),
            "",
            &ModifierSources::default(),
            &mut Off,
        );
        assert!(stats.damage.is_none());
        assert!(stats.speed.is_none());
        assert!(stats.range.is_none());
        assert!(stats.visibility.is_none());
    }

    /// `TorpedoSpeed` and `TorpedoRange` provenance records exact replay values.
    /// Uses Gearing's torpedo with both speed multipliers + bonus and a real
    /// torpedoRangeCoefficient, exercising the full recording table.
    #[test]
    fn torpedo_speed_and_range_records_exact_replay() {
        use crate::game_params::ttx::provenance::{InputId, On, Op, ShipStatsProvenance};

        // torpedoSpeedMultiplier 1.05, normalTorpedoSpeedMultiplier 1.1, torpedoSpeedBonus 2.0,
        // torpedoRangeCoefficient 1.15. Gearing torpedo ammoType "torpedo" -> normal multiplier applies.
        let mods = [
            modifier("torpedoSpeedMultiplier", 1.05),
            modifier("normalTorpedoSpeedMultiplier", 1.1),
            modifier("torpedoSpeedBonus", 2.0),
            modifier("torpedoRangeCoefficient", 1.15),
        ];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");

        let mut sources = ModifierSources::default();
        sources.record("torpedoSpeedMultiplier", InputId::Upgrade { name: "U1".into() }, 1.05);
        sources.record("normalTorpedoSpeedMultiplier", InputId::Skill { name: "S1".into() }, 1.1);
        sources.record("torpedoSpeedBonus", InputId::Skill { name: "S2".into() }, 2.0);
        sources.record("torpedoRangeCoefficient", InputId::Upgrade { name: "U2".into() }, 1.15);

        let provider = StubProvider::new("PAPT027_Mk_16_mod_1", gearing_torpedo());
        let launchers = [gearing_launcher()];
        let mut rec = On::default();
        let torps = torpedoes(&launchers, &bundle, 1.0, &provider, "PAUT902_D10_NEW_STOCK", &sources, &mut rec)
            .expect("torpedoes computed");
        let prov = rec.into_provenance();

        // TorpedoSpeed: base == raw speed (66), steps reproduce final.
        // final = 66 * 1.05 * 1.1 + 2.0 = 66 * 1.155 + 2.0 = 76.23 + 2.0 = 78.23.
        let speed_attr =
            prov.attributions.iter().find(|a| a.stat == TtxStat::TorpedoSpeed).expect("TorpedoSpeed recorded");
        assert_eq!(speed_attr.qualifier.as_deref(), Some("PAPT027_Mk_16_mod_1"), "qualifier must be the torpedo name");
        assert!(
            matches!(&speed_attr.base_source, InputId::Module { slot, .. } if *slot == ModuleSlot::Torpedoes),
            "base source must be the Torpedoes module"
        );
        assert_eq!(speed_attr.base_value, 66.0, "base must be raw speed");
        let mul_steps: Vec<_> = speed_attr.steps.iter().filter(|c| c.op == Op::Mul).collect();
        let add_steps: Vec<_> = speed_attr.steps.iter().filter(|c| c.op == Op::Add).collect();
        assert_eq!(mul_steps.len(), 2, "two Mul steps: torpedoSpeedMultiplier and normalTorpedoSpeedMultiplier");
        assert_eq!(add_steps.len(), 1, "one Add step: torpedoSpeedBonus");
        let tsmul = mul_steps.iter().find(|c| c.modifier_name == "torpedoSpeedMultiplier");
        assert!(tsmul.is_some(), "torpedoSpeedMultiplier step present");
        assert!((tsmul.unwrap().operand - 1.05).abs() < 1e-6);
        let ntsmul = mul_steps.iter().find(|c| c.modifier_name == "normalTorpedoSpeedMultiplier");
        assert!(ntsmul.is_some(), "normalTorpedoSpeedMultiplier step present");
        assert!((ntsmul.unwrap().operand - 1.1).abs() < 1e-6);
        let expected_speed = torps.torpedoes[0].speed.expect("speed").value();
        let replayed_speed = ShipStatsProvenance::replay(speed_attr);
        assert!(
            (replayed_speed - expected_speed).abs() < 1e-3,
            "TorpedoSpeed replay got {replayed_speed}, factory={expected_speed}"
        );

        // TorpedoRange: base = raw maxDist * BW_TO_BALLISTIC / KM_TO_M; replay exact.
        // base = 350 * 30 / 1000 = 10.5; final = 10.5 * 1.15 = 12.075.
        let range_attr =
            prov.attributions.iter().find(|a| a.stat == TtxStat::TorpedoRange).expect("TorpedoRange recorded");
        assert_eq!(range_attr.qualifier.as_deref(), Some("PAPT027_Mk_16_mod_1"), "qualifier must be the torpedo name");
        assert!((range_attr.base_value - 10.5).abs() < 1e-3, "range base must be maxDist*BW_TO_BALLISTIC/KM_TO_M");
        let rc_step = range_attr.steps.iter().find(|c| c.modifier_name == "torpedoRangeCoefficient");
        assert!(rc_step.is_some(), "torpedoRangeCoefficient step present");
        assert!((rc_step.unwrap().operand - 1.15).abs() < 1e-6);
        let expected_range = torps.torpedoes[0].range.expect("range").value();
        let replayed_range = ShipStatsProvenance::replay(range_attr);
        assert!(
            (replayed_range - expected_range).abs() < 1e-3,
            "TorpedoRange replay got {replayed_range}, factory={expected_range}"
        );
    }

    /// Worcester's real HE shell `PAPA051_152mm_HE_HC_Mark_39_Mod_0` (GameParams):
    /// ammoType "HE", alphaDamage 2200, alphaPiercingHE 30, burnProb 0.12,
    /// uwCritical 0.0, bulletDiametr 0.152, bulletSpeed 812, timeFactor 1.0.
    fn worcester_he() -> Projectile {
        Projectile::builder()
            .ammo_type("HE".to_string())
            .alpha_damage(2200.0)
            .alpha_piercing_he(30.0)
            .burn_prob(0.12)
            .uw_critical(0.0)
            .bullet_diametr(0.152)
            .bullet_speed(812.0)
            .build()
    }

    /// Worcester's real AP shell `PAPA050_152mm_AP_130lbs_Mk35` (GameParams):
    /// ammoType "AP", alphaDamage 3200, burnProb -0.5 (N/A), uwCritical 0.0,
    /// bulletDiametr 0.152, bulletSpeed 762, timeFactor 1.0.
    fn worcester_ap() -> Projectile {
        Projectile::builder()
            .ammo_type("AP".to_string())
            .alpha_damage(3200.0)
            .burn_prob(-0.5)
            .uw_critical(0.0)
            .bullet_diametr(0.152)
            .bullet_speed(762.0)
            .build()
    }

    /// Worcester's real `ArtilleryDefault` component + `HP_AGM_*` gun fields
    /// (GameParams `PASC016_Worcester_1948`): maxDist 15320 (BW), 6 guns, each
    /// barrelDiameter 0.152, shotDelay 4.6, rotationSpeed[0] 25, numBarrels 2,
    /// ammoSwitchCoeff 1.0, minRadius 1.1, idealRadius 8, idealDistance 1000, ammo HE+AP.
    fn worcester_artillery() -> ArtilleryComponentStats {
        let gun = || ArtilleryGunStats {
            shot_delay: Some(Seconds::from(4.6)),
            rotation_speed: Some(DegreesPerSecond::from(25.0)),
            num_barrels: Some(2.0),
            barrel_diameter: Some(Meters::from(0.152)),
            ammo_switch_coeff: Some(1.0),
            min_radius: Some(1.1),
            ideal_radius: Some(8.0),
            ideal_distance: Some(1000.0),
            radius_on_zero: None,
            radius_on_delim: None,
            radius_on_max: None,
            delim: None,
            ammo: vec!["PAPA051_152mm_HE_HC_Mark_39_Mod_0".to_string(), "PAPA050_152mm_AP_130lbs_Mk35".to_string()],
        };
        ArtilleryComponentStats {
            max_dist: Some(Meters::from(15320.0)),
            guns: vec![gun(), gun(), gun(), gun(), gun(), gun()],
        }
    }

    /// A multi-param provider exposing both Worcester shells by name.
    struct MultiProvider {
        params: Vec<Rc<Param>>,
    }

    impl MultiProvider {
        fn new(entries: &[(&str, Projectile)]) -> Self {
            let params = entries
                .iter()
                .enumerate()
                .map(|(i, (name, proj))| {
                    Rc::new(
                        Param::builder()
                            .id(GameParamId::from((i + 1) as u32))
                            .index(format!("S{i:04}"))
                            .name(name.to_string())
                            .nation("USA".to_string())
                            .data(ParamData::Projectile(proj.clone()))
                            .build(),
                    )
                })
                .collect();
            MultiProvider { params }
        }
    }

    impl GameParamProvider for MultiProvider {
        fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_index(&self, _index: &str) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
            self.params.iter().find(|p| p.name() == name).cloned()
        }
        fn params(&self) -> &[Rc<Param>] {
            &self.params
        }
    }

    fn worcester_provider() -> MultiProvider {
        MultiProvider::new(&[
            ("PAPA051_152mm_HE_HC_Mark_39_Mod_0", worcester_he()),
            ("PAPA050_152mm_AP_130lbs_Mk35", worcester_ap()),
        ])
    }

    #[test]
    fn worcester_stock_artillery_gun_and_range() {
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");

        // range: (15320 / 1000) * 1.0 (fc) * 1.0 (GMMaxDist) = 15.32.
        assert_eq!(arty.range, Some(Km::from(15.32)));
        // reload: 4.6 * 1.0 (GMShotDelay) = 4.6.
        assert_eq!(arty.reload_time, Some(Seconds::from(4.6)));
        // ammoSwitchTime: 4.6 * 1.0 * 1.0 * 1.0 = 4.6.
        let st = arty.ammo_switch_time.expect("switch time").value();
        assert!(approx(st, 4.6), "got {st}");

        let gun = arty.gun.expect("gun");
        // caliber: 0.152 * 1000 = 152.
        assert_eq!(gun.caliber, Some(Millimeters::from(152.0)));
        assert_eq!(gun.num_guns, Some(6));
        assert_eq!(gun.num_barrels, Some(2));
        // rotation: 25 * 1.0 + 0 = 25; time 180/25 = 7.2.
        assert_eq!(gun.rotation_speed, Some(DegreesPerSecond::from(25.0)));
        let rt = gun.rotation_time.expect("rotation_time").value();
        assert!(approx(rt, 7.2), "got {rt}");
    }

    #[test]
    fn worcester_stock_dispersion_matches_helper() {
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        // dispersion over the FC-adjusted range 15.32 km, stock GMIdealRadius 1.0.
        let expected = constants::dispersion_horizontal(1.1, 8.0, 1000.0, Km::from(15.32), 1.0).to_meters().value();
        let got = arty.dispersion.expect("dispersion").value();
        assert!((got - expected).abs() < 1e-3, "got {got} expected {expected}");
        // The transcribed formula yields ~138.7 m at 15.32 km for Worcester's gun
        // (minRadius 1.1 / idealRadius 8 / idealDistance 1000); same BW_TO_SHIP=15
        // scale that recovers NC's 271 m and Yamato's 273 m in constants.rs.
        assert!((got - 138.7).abs() < 1.0, "got {got}");
    }

    /// Worcester artillery with the four vertical-dispersion curve fields set.
    fn worcester_artillery_with_curve() -> ArtilleryComponentStats {
        let gun = || ArtilleryGunStats {
            shot_delay: Some(Seconds::from(4.6)),
            rotation_speed: Some(DegreesPerSecond::from(25.0)),
            num_barrels: Some(2.0),
            barrel_diameter: Some(Meters::from(0.152)),
            ammo_switch_coeff: Some(1.0),
            min_radius: Some(1.1),
            ideal_radius: Some(8.0),
            ideal_distance: Some(1000.0),
            radius_on_zero: Some(1.0),
            radius_on_delim: Some(1.5),
            radius_on_max: Some(2.0),
            delim: Some(0.5),
            ammo: vec!["PAPA051_152mm_HE_HC_Mark_39_Mod_0".to_string()],
        };
        ArtilleryComponentStats {
            max_dist: Some(Meters::from(15320.0)),
            guns: vec![gun(), gun(), gun(), gun(), gun(), gun()],
        }
    }

    #[test]
    fn artillery_vertical_dispersion_some_when_curve_fields_set() {
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery_with_curve(),
            "",
            None,
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        let h = arty.dispersion.expect("dispersion").value();
        let v = arty.dispersion_vertical.expect("dispersion_vertical").value();
        // At max range, clamped_dispersion_coeff with radius_on_max=2.0 yields coeff=2.0.
        assert!((v - h * 2.0).abs() < 1e-2, "got v={v} h={h}");
    }

    #[test]
    fn artillery_vertical_dispersion_none_when_curve_fields_absent() {
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        assert!(arty.dispersion.is_some(), "horizontal dispersion must be present");
        assert!(arty.dispersion_vertical.is_none(), "vertical must be None when curve fields absent");
    }

    #[test]
    fn worcester_stock_he_shell() {
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        let he = arty.shells.iter().find(|s| s.ammo_kind.as_deref() == Some("HE")).expect("HE shell");
        // stock damage reduces to alphaDamage 2200.
        assert_eq!(he.damage, Some(Hp::from(2200.0)));
        // penetration: floor(30 * 1.0) = 30.
        assert_eq!(he.penetration, Some(Millimeters::from(30.0)));
        // burnChance: 0.12 * 100 = 12%.
        assert_eq!(he.burn_chance, Some(Percent::from(12.0)));
        assert_eq!(he.caliber, Some(Millimeters::from(152.0)));
        // speed: 812 * 1.0 (timeFactor) = 812.
        assert_eq!(he.speed, Some(MetersPerSecond::from(812.0)));
        // floodChance: uwCritical 0.0 * 100 = 0.
        assert_eq!(he.flood_chance, Some(Percent::from(0.0)));
        // Main battery -> unlimited pool.
        assert_eq!(he.max_ammo, Some(AmmoCount::Infinite));
    }

    #[test]
    fn worcester_stock_ap_shell() {
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        let ap = arty.shells.iter().find(|s| s.ammo_kind.as_deref() == Some("AP")).expect("AP shell");
        // stock damage reduces to alphaDamage 3200.
        assert_eq!(ap.damage, Some(Hp::from(3200.0)));
        // AP penetration is a ballistic sim -> None (deferred).
        assert!(ap.penetration.is_none());
        // AP burnProb -0.5 (N/A) -> calculate_burn_chance clamps to 0%.
        assert_eq!(ap.burn_chance, Some(Percent::from(0.0)));
        // AP has no floodChance.
        assert!(ap.flood_chance.is_none());
        // speed: 762 * 1.0 = 762.
        assert_eq!(ap.speed, Some(MetersPerSecond::from(762.0)));
    }

    #[test]
    fn worcester_modified_reload_and_range() {
        // GMShotDelay 0.9 -> reload 4.6 * 0.9 = 4.14; GMMaxDist 1.1 -> range 15.32 * 1.1 = 16.852.
        let mods = [modifier("GMShotDelay", 0.9), modifier("GMMaxDist", 1.1)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Cruiser, VERSION).expect("test modifiers are all known");
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &bundle,
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        let reload = arty.reload_time.expect("reload").value();
        assert!(approx(reload, 4.14), "got {reload}");
        let range = arty.range.expect("range").value();
        assert!(approx(range, 16.852), "got {range}");
    }

    #[test]
    fn worcester_traverse_modifier_applies() {
        // GMRotationSpeed 1.2 (+20% traverse): 25 -> 30, rotation_time 180/30 = 6.
        let mods = [modifier("GMRotationSpeed", 1.2)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Cruiser, VERSION).expect("test modifiers are all known");
        let provider = worcester_provider();
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &bundle,
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        let gun = arty.gun.expect("gun");
        let rs = gun.rotation_speed.expect("rotation_speed").value();
        assert!(approx(rs, 30.0), "got {rs}");
        let rt = gun.rotation_time.expect("rotation_time").value();
        assert!(approx(rt, 6.0), "got {rt}");
    }

    #[test]
    fn artillery_none_when_no_guns() {
        let provider = worcester_provider();
        let empty = ArtilleryComponentStats::default();
        assert!(
            artillery(
                &empty,
                "",
                None,
                &ModifierBundle::empty(Species::Cruiser),
                &ModifierSources::default(),
                1.0,
                1.0,
                1.0,
                10,
                &provider,
                &mut Off,
            )
            .is_none()
        );
    }

    #[test]
    fn artillery_drops_unresolvable_ammo_row() {
        // Provider knows only the HE shell; the AP ammo name does not resolve. The
        // section is still produced with the resolvable HE row, and the AP row is
        // silently dropped (a warn-once diagnostic fires; no fabrication).
        let provider = MultiProvider::new(&[("PAPA051_152mm_HE_HC_Mark_39_Mod_0", worcester_he())]);
        let arty = artillery(
            &worcester_artillery(),
            "",
            None,
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut Off,
        )
        .expect("artillery computed");
        assert_eq!(arty.shells.len(), 1);
        assert!(arty.shells.iter().any(|s| s.ammo_kind.as_deref() == Some("HE")));
        assert!(!arty.shells.iter().any(|s| s.ammo_kind.as_deref() == Some("AP")));
    }

    #[test]
    fn shell_stats_none_when_inputs_absent() {
        // A projectile with no shell fields -> stat fields None (no fabrication).
        let empty = Projectile::builder().ammo_type("HE".to_string()).build();
        let stats = shell_stats(
            "X".to_string(),
            &empty,
            &ModifierBundle::empty(Species::Cruiser),
            "",
            &ModifierSources::default(),
            10,
            false,
            &mut Off,
        );
        assert!(stats.damage.is_none());
        assert!(stats.caliber.is_none());
        assert!(stats.speed.is_none());
        assert!(stats.penetration.is_none());
        assert!(stats.burn_chance.is_none());
        assert!(stats.flood_chance.is_none());
    }

    #[test]
    fn shell_speed_applies_time_factor() {
        // PXPA005_305MM_HE_RASPUTIN (GameParams): timeFactor 0.5, bulletSpeed 762 ->
        // displayed speed 762 * 0.5 = 381 (PreprocessedAmmo.py:16).
        let shell = Projectile::builder()
            .ammo_type("HE".to_string())
            .bullet_diametr(0.305)
            .bullet_speed(762.0)
            .time_factor(0.5)
            .build();
        let stats = shell_stats(
            "X".to_string(),
            &shell,
            &ModifierBundle::empty(Species::Cruiser),
            "",
            &ModifierSources::default(),
            10,
            false,
            &mut Off,
        );
        assert_eq!(stats.speed, Some(MetersPerSecond::from(381.0)));

        // A shell without timeFactor defaults to 1.0 (maa3520d6.py:1151): speed == bulletSpeed.
        let plain = Projectile::builder().ammo_type("HE".to_string()).bullet_diametr(0.152).bullet_speed(812.0).build();
        let plain_stats = shell_stats(
            "Y".to_string(),
            &plain,
            &ModifierBundle::empty(Species::Cruiser),
            "",
            &ModifierSources::default(),
            10,
            false,
            &mut Off,
        );
        assert_eq!(plain_stats.speed, Some(MetersPerSecond::from(812.0)));
    }

    use crate::game_params::ttx::components::SecondaryComponentStats;

    /// Bismarck's real 150mm secondary shell `PGPA003_150mm_HE_HE_N_F` (GameParams
    /// `PGSB108_Bismarck`): ammoType "HE", alphaDamage 1700, alphaPiercingHE 38,
    /// burnProb 0.08, bulletDiametr 0.15, bulletSpeed 875, uwCritical 0.0.
    fn bismarck_150mm_he() -> Projectile {
        Projectile::builder()
            .ammo_type("HE".to_string())
            .alpha_damage(1700.0)
            .alpha_piercing_he(38.0)
            .burn_prob(0.08)
            .uw_critical(0.0)
            .bullet_diametr(0.15)
            .bullet_speed(875.0)
            .build()
    }

    /// Bismarck's real 105mm secondary shell `PGPA085_105mm_HE_HE_33lbs`: ammoType "HE",
    /// alphaDamage 1200, alphaPiercingHE 26, burnProb 0.05, bulletDiametr 0.105,
    /// bulletSpeed 900, uwCritical 0.0.
    fn bismarck_105mm_he() -> Projectile {
        Projectile::builder()
            .ammo_type("HE".to_string())
            .alpha_damage(1200.0)
            .alpha_piercing_he(26.0)
            .burn_prob(0.05)
            .uw_critical(0.0)
            .bullet_diametr(0.105)
            .bullet_speed(900.0)
            .build()
    }

    /// Bismarck's `A_ATBA` component + `HP_GGS_*` mixed-caliber guns (GameParams
    /// `PGSB108_Bismarck`): component maxDist 7600 (BigWorld, == 7.6 km in-game range).
    /// The 150mm group (PGGS001) shotDelay 7.5, barrelDiameter 0.15, numBarrels 2,
    /// rotationSpeed[0] 60, minRadius 1.0, idealRadius 15.5, idealDistance 333.333; the
    /// 105mm group (PGGS003) shotDelay 3.35, barrelDiameter 0.105. The first gun group
    /// (150mm) drives the displayed reload/gun.
    fn bismarck_secondaries() -> SecondaryComponentStats {
        let gun_150 = ArtilleryGunStats {
            shot_delay: Some(Seconds::from(7.5)),
            rotation_speed: Some(DegreesPerSecond::from(60.0)),
            num_barrels: Some(2.0),
            barrel_diameter: Some(Meters::from(0.15)),
            ammo_switch_coeff: None,
            min_radius: Some(1.0),
            ideal_radius: Some(15.5),
            ideal_distance: Some(333.333),
            radius_on_zero: None,
            radius_on_delim: None,
            radius_on_max: None,
            delim: None,
            ammo: vec!["PGPA003_150mm_HE_HE_N_F".to_string()],
        };
        let gun_105 = ArtilleryGunStats {
            shot_delay: Some(Seconds::from(3.35)),
            rotation_speed: Some(DegreesPerSecond::from(60.0)),
            num_barrels: Some(2.0),
            barrel_diameter: Some(Meters::from(0.105)),
            ammo_switch_coeff: None,
            min_radius: Some(1.0),
            ideal_radius: Some(15.5),
            ideal_distance: Some(333.333),
            radius_on_zero: None,
            radius_on_delim: None,
            radius_on_max: None,
            delim: None,
            ammo: vec!["PGPA085_105mm_HE_HE_33lbs".to_string()],
        };
        SecondaryComponentStats {
            max_dist: Some(Meters::from(7600.0)),
            // 14 mounts total: 6 x 150mm groups + 8 x 105mm groups (calibers as 2 distinct ammo).
            guns: vec![gun_150.clone(), gun_150, gun_105.clone(), gun_105],
        }
    }

    fn bismarck_secondary_provider() -> MultiProvider {
        MultiProvider::new(&[
            ("PGPA003_150mm_HE_HE_N_F", bismarck_150mm_he()),
            ("PGPA085_105mm_HE_HE_33lbs", bismarck_105mm_he()),
        ])
    }

    #[test]
    fn bismarck_stock_secondaries_gun_and_range() {
        let provider = bismarck_secondary_provider();
        let sec = secondaries(
            &bismarck_secondaries(),
            "HULL",
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            1.0,
            8,
            &provider,
            &mut Off,
        )
        .expect("secondaries computed");

        // range: (7600 / 1000) * 1.0 (GSMaxDist) = 7.6 (Bismarck's in-game stock range).
        assert_eq!(sec.range, Some(Km::from(7.6)));
        // reload: 7.5 * 1.0 (GSShotDelay) = 7.5 (first gun group, the 150mm mount).
        assert_eq!(sec.reload_time, Some(Seconds::from(7.5)));
        // secondaries have no ammo-switch time.
        assert!(sec.ammo_switch_time.is_none());

        let gun = sec.gun.expect("gun");
        // caliber: 0.15 * 1000 = 150 (first gun group).
        assert_eq!(gun.caliber, Some(Millimeters::from(150.0)));
        // num_guns counts all mounts (mixed calibers).
        assert_eq!(gun.num_guns, Some(4));
        assert_eq!(gun.num_barrels, Some(2));
        // rotation: 60 * 1.0 + 0 = 60; time 180/60 = 3.
        assert_eq!(gun.rotation_speed, Some(DegreesPerSecond::from(60.0)));
        let rt = gun.rotation_time.expect("rotation_time").value();
        assert!(approx(rt, 3.0), "got {rt}");
    }

    #[test]
    fn bismarck_stock_secondaries_shells() {
        let provider = bismarck_secondary_provider();
        let sec = secondaries(
            &bismarck_secondaries(),
            "HULL",
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            1.0,
            8,
            &provider,
            &mut Off,
        )
        .expect("secondaries computed");

        // One shell per distinct ATBA ammo across the mixed-caliber mounts.
        assert_eq!(sec.shells.len(), 2);

        let s150 = sec.shells.iter().find(|s| s.name == "PGPA003_150mm_HE_HE_N_F").expect("150mm shell");
        // stock ATBA damage reduces to alphaDamage 1700 (GS coeffs all 1.0).
        assert_eq!(s150.damage, Some(Hp::from(1700.0)));
        // HE penetration: floor(38 * 1.0) = 38 (GSPenetrationCoeffHE stock 1.0).
        assert_eq!(s150.penetration, Some(Millimeters::from(38.0)));
        // burnChance: 0.08 * 100 = 8%.
        assert_eq!(s150.burn_chance, Some(Percent::from(8.0)));
        assert_eq!(s150.caliber, Some(Millimeters::from(150.0)));
        assert_eq!(s150.speed, Some(MetersPerSecond::from(875.0)));
        assert_eq!(s150.flood_chance, Some(Percent::from(0.0)));

        let s105 = sec.shells.iter().find(|s| s.name == "PGPA085_105mm_HE_HE_33lbs").expect("105mm shell");
        assert_eq!(s105.damage, Some(Hp::from(1200.0)));
        assert_eq!(s105.penetration, Some(Millimeters::from(26.0)));
        assert_eq!(s105.burn_chance, Some(Percent::from(5.0)));
        assert_eq!(s105.caliber, Some(Millimeters::from(105.0)));
    }

    #[test]
    fn bismarck_secondary_range_modifier_applies() {
        // GSMaxDist 1.2 (secondary-range upgrade/AtbaRange): 7.6 * 1.2 = 9.12.
        let mods = [modifier("GSMaxDist", 1.2)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let provider = bismarck_secondary_provider();
        let sec = secondaries(
            &bismarck_secondaries(),
            "HULL",
            &bundle,
            &ModifierSources::default(),
            1.0,
            8,
            &provider,
            &mut Off,
        )
        .expect("secondaries computed");
        let range = sec.range.expect("range").value();
        assert!(approx(range, 9.12), "got {range}");
    }

    #[test]
    fn bismarck_secondary_reload_modifier_applies() {
        // GSShotDelay 0.85 (secondary reload upgrade): 7.5 * 0.85 = 6.375.
        let mods = [modifier("GSShotDelay", 0.85)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let provider = bismarck_secondary_provider();
        let sec = secondaries(
            &bismarck_secondaries(),
            "HULL",
            &bundle,
            &ModifierSources::default(),
            1.0,
            8,
            &provider,
            &mut Off,
        )
        .expect("secondaries computed");
        let reload = sec.reload_time.expect("reload").value();
        assert!(approx(reload, 6.375), "got {reload}");
    }

    #[test]
    fn secondaries_none_when_no_guns() {
        let provider = bismarck_secondary_provider();
        let empty = SecondaryComponentStats::default();
        assert!(
            secondaries(
                &empty,
                "HULL",
                &ModifierBundle::empty(Species::Battleship),
                &ModifierSources::default(),
                1.0,
                8,
                &provider,
                &mut Off
            )
            .is_none()
        );
    }

    /// Secondary shell stats must be recorded under `SecondaryShell*` TtxStat variants,
    /// not the main-battery `Shell*` variants. Before the is_atba variant-selection fix,
    /// `shell_stats` hardcoded the main-battery variants regardless of `is_atba`, so
    /// provenance keys diverged from `rows()` and shells were mislabeled.
    #[test]
    fn secondary_shell_provenance_uses_secondary_variants() {
        use crate::game_params::ttx::provenance::{On, ShipStatsProvenance};

        let provider = bismarck_secondary_provider();
        let mut rec = On::default();
        let sec = secondaries(
            &bismarck_secondaries(),
            "HULL_B",
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            1.0,
            8,
            &provider,
            &mut rec,
        )
        .expect("secondaries computed");
        let prov = rec.into_provenance();

        // Every shell attribution must use a Secondary* variant.
        let shell_main_variants = [
            TtxStat::ShellDamage,
            TtxStat::ShellCaliber,
            TtxStat::ShellSpeed,
            TtxStat::ShellPenetration,
            TtxStat::ShellBurnChance,
            TtxStat::ShellFloodChance,
            TtxStat::ShellMaxAmmo,
        ];
        for attr in &prov.attributions {
            assert!(
                !shell_main_variants.contains(&attr.stat),
                "secondary path must not record main-battery Shell* variant {:?}",
                attr.stat
            );
        }

        // At least one SecondaryShellDamage attribution must be present.
        assert!(
            prov.attributions.iter().any(|a| a.stat == TtxStat::SecondaryShellDamage),
            "expected SecondaryShellDamage in provenance"
        );

        // Replay: SecondaryShellDamage for the 150mm shell reproduces the factory value.
        let damage_150 = prov
            .attributions
            .iter()
            .find(|a| a.stat == TtxStat::SecondaryShellDamage && a.qualifier.as_deref() == Some("HE"))
            .expect("SecondaryShellDamage HE recorded");
        let factory_damage = sec
            .shells
            .iter()
            .find(|s| s.name == "PGPA003_150mm_HE_HE_N_F")
            .expect("150mm shell")
            .damage
            .expect("damage")
            .value();
        let replayed = ShipStatsProvenance::replay(damage_150);
        assert!(
            (replayed - factory_damage).abs() < 1e-2,
            "SecondaryShellDamage replay got {replayed}, factory={factory_damage}"
        );
    }

    #[test]
    fn gearing_stock_visibility() {
        // Gearing hull: visibilityFactor 7.33, visibilityFactorByPlane 3.41,
        // visibilityCoefFire 2.0, visibilityCoefFireByPlane 2.0, visibilityCoefGKInSmoke 2.83.
        // Stock = empty bundle, no big-gun penalty, no secondary range.
        let vis = visibility(
            &gearing_hull(),
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            false,
            None,
            None,
            &mut Off,
        );
        // sea = 7.33 * 1.0 * 1.0 = 7.33.
        assert_eq!(vis.sea_detection, Some(Km::from(7.33)));
        // sea on fire = 7.33 + 2.0 = 9.33.
        let on_fire = vis.sea_detection_on_fire.expect("sea on fire").value();
        assert!(approx(on_fire, 9.33), "got {on_fire}");
        // air = 3.41 * 1.0 * 1.0 = 3.41.
        assert_eq!(vis.air_detection, Some(Km::from(3.41)));
        // air on fire = 3.41 + 2.0 = 5.41.
        let air_fire = vis.air_detection_on_fire.expect("air on fire").value();
        assert!(approx(air_fire, 5.41), "got {air_fire}");
        // smoke = visibilityCoefGKInSmoke 2.83 (> MINIMAL_VALID_VALUE).
        assert_eq!(vis.detection_in_smoke, Some(Km::from(2.83)));
        // No main-battery or secondary range supplied, DD has no periscope.
        assert!(vis.main_gun_range_detection.is_none());
        assert!(vis.secondary_range_detection.is_none());
        assert!(vis.periscope_depth_detection.is_none());
    }

    #[test]
    fn concealment_modifier_reduces_sea_detection() {
        // Concealment System Mod 1: visibilityFactor 0.9 (-10%) -> 7.33 * 0.9 = 6.597.
        let mods = [modifier("visibilityFactor", 0.9)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let vis = visibility(&gearing_hull(), "H", &bundle, &ModifierSources::default(), false, None, None, &mut Off);
        let sea = vis.sea_detection.expect("sea").value();
        assert!(approx(sea, 6.597), "got {sea}");
        // on fire shifts by the same base: 6.597 + 2.0 = 8.597.
        let on_fire = vis.sea_detection_on_fire.expect("sea on fire").value();
        assert!(approx(on_fire, 8.597), "got {on_fire}");
    }

    #[test]
    fn camouflage_dist_coeff_reduces_sea_detection() {
        // visibilityDistCoeff 0.97 (camouflage concealment coef) folds into `coeff`:
        // 7.33 * 1.0 * 0.97 = 7.1101.
        let mods = [modifier("visibilityDistCoeff", 0.97)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Destroyer, VERSION).expect("test modifiers are all known");
        let vis = visibility(&gearing_hull(), "H", &bundle, &ModifierSources::default(), false, None, None, &mut Off);
        let sea = vis.sea_detection.expect("sea").value();
        assert!(approx(sea, 7.1101), "got {sea}");
        // air also scaled by coeff: 3.41 * 0.97 = 3.3077.
        let air = vis.air_detection.expect("air").value();
        assert!(approx(air, 3.3077), "got {air}");
    }

    #[test]
    fn big_gun_visibility_penalty_applies_only_with_non_small_artillery() {
        // GMBigGunVisibilityCoeff 1.05 multiplies coeff only when has_big_gun_artillery.
        let mods = [modifier("GMBigGunVisibilityCoeff", 1.05)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        // BB-like hull (reuse Gearing's factor for the arithmetic): with big guns the
        // penalty applies: 7.33 * 1.05 = 7.6965.
        let with_guns =
            visibility(&gearing_hull(), "H", &bundle, &ModifierSources::default(), true, None, None, &mut Off);
        let sea = with_guns.sea_detection.expect("sea").value();
        assert!(approx(sea, 7.6965), "got {sea}");
        // Without big guns the coeff is untouched: 7.33.
        let without =
            visibility(&gearing_hull(), "H", &bundle, &ModifierSources::default(), false, None, None, &mut Off);
        assert_eq!(without.sea_detection, Some(Km::from(7.33)));
    }

    #[test]
    fn secondary_range_detection_floors_at_atba_range() {
        // BB-like sea detection below the secondary range -> max(sea, 7.6) = 7.6.
        let hull = HullComponentStats { visibility_factor: Some(Km::from(6.0)), ..gearing_hull() };
        let vis = visibility(
            &hull,
            "H",
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            false,
            None,
            Some(7.6),
            &mut Off,
        );
        // sea = 6.0; secondary floor = max(6.0, 7.6) = 7.6.
        assert_eq!(vis.sea_detection, Some(Km::from(6.0)));
        assert_eq!(vis.secondary_range_detection, Some(Km::from(7.6)));

        // When sea exceeds the secondary range, the floor stays at sea.
        let hull_far = HullComponentStats { visibility_factor: Some(Km::from(9.0)), ..gearing_hull() };
        let vis_far = visibility(
            &hull_far,
            "H",
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            false,
            None,
            Some(7.6),
            &mut Off,
        );
        assert_eq!(vis_far.secondary_range_detection, Some(Km::from(9.0)));
    }

    #[test]
    fn main_gun_range_detection_floors_at_main_gun_range() {
        // Main-battery range floors the main-gun firing detection, independent of atba.
        let hull = HullComponentStats { visibility_factor: Some(Km::from(6.0)), ..gearing_hull() };
        let vis = visibility(
            &hull,
            "H",
            &ModifierBundle::empty(Species::Battleship),
            &ModifierSources::default(),
            false,
            Some(20.0),
            None,
            &mut Off,
        );
        // sea = 6.0; main-gun floor = max(6.0, 20.0) = 20.0; no secondaries -> atba None.
        assert_eq!(vis.main_gun_range_detection, Some(Km::from(20.0)));
        assert!(vis.secondary_range_detection.is_none());
    }

    #[test]
    fn main_gun_present_without_secondaries_yields_no_secondary_detection() {
        // A gunship with no secondaries (e.g. Elbing) must not surface a secondary line.
        let hull = HullComponentStats { visibility_factor: Some(Km::from(6.0)), ..gearing_hull() };
        let vis = visibility(
            &hull,
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            false,
            Some(11.0),
            None,
            &mut Off,
        );
        assert_eq!(vis.main_gun_range_detection, Some(Km::from(11.0)));
        assert!(vis.secondary_range_detection.is_none());
    }

    #[test]
    fn near_zero_smoke_coef_yields_no_smoke_detection() {
        // visibilityCoefGKInSmoke == MINIMAL_VALID_VALUE (0.01) is not > the gate -> None.
        let hull = HullComponentStats { visibility_coef_gk_in_smoke: Some(Km::from(0.01)), ..gearing_hull() };
        let vis = visibility(
            &hull,
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            false,
            None,
            None,
            &mut Off,
        );
        assert!(vis.detection_in_smoke.is_none());

        // Below the gate -> None.
        let hull_zero = HullComponentStats { visibility_coef_gk_in_smoke: Some(Km::from(0.0)), ..gearing_hull() };
        let vis_zero = visibility(
            &hull_zero,
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            false,
            None,
            None,
            &mut Off,
        );
        assert!(vis_zero.detection_in_smoke.is_none());
    }

    #[test]
    fn submarine_periscope_detection_applies_coeff() {
        // Periscope-depth detection only when the field is present:
        // visibilityByPeriscope 5.0 * visibilityForSubmarineCoeff 1.0 (stock) = 5.0.
        let hull = HullComponentStats { visibility_factor_by_periscope: Some(Km::from(5.0)), ..gearing_hull() };
        let vis = visibility(
            &hull,
            "H",
            &ModifierBundle::empty(Species::Submarine),
            &ModifierSources::default(),
            false,
            None,
            None,
            &mut Off,
        );
        assert_eq!(vis.periscope_depth_detection, Some(Km::from(5.0)));
    }

    #[test]
    fn visibility_none_when_inputs_absent() {
        // Empty hull -> every field None (no fabrication, per-depth sub deferred).
        let empty = HullComponentStats::default();
        let vis = visibility(
            &empty,
            "H",
            &ModifierBundle::empty(Species::Destroyer),
            &ModifierSources::default(),
            false,
            None,
            None,
            &mut Off,
        );
        assert!(vis.sea_detection.is_none());
        assert!(vis.sea_detection_on_fire.is_none());
        assert!(vis.air_detection.is_none());
        assert!(vis.air_detection_on_fire.is_none());
        assert!(vis.detection_in_smoke.is_none());
        assert!(vis.main_gun_range_detection.is_none());
        assert!(vis.secondary_range_detection.is_none());
        assert!(vis.periscope_depth_detection.is_none());
    }

    /// `SeaDetection` records three Mul steps (visibilityFactor + visibilityDistCoeff +
    /// GMBigGunVisibilityCoeff) when `has_big_gun_artillery`. `SeaDetectionOnFire` records
    /// one `Op::Add` step. `ShipStatsProvenance::replay` reproduces the factory values exactly.
    #[test]
    fn visibility_records_sea_detection_and_on_fire_replay_exact() {
        use crate::game_params::ttx::provenance::{On, Op, ShipStatsProvenance};

        let mods = [
            modifier("visibilityFactor", 0.9),
            modifier("visibilityDistCoeff", 0.97),
            modifier("GMBigGunVisibilityCoeff", 1.05),
        ];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");

        let mut sources = ModifierSources::default();
        sources.record("visibilityFactor", InputId::Upgrade { name: "CSM1".into() }, 0.9);
        sources.record("visibilityDistCoeff", InputId::Upgrade { name: "Camo".into() }, 0.97);
        sources.record(
            "GMBigGunVisibilityCoeff",
            InputId::Skill { name: crate::game_params::types::CrewSkillName::from("BBSkill") },
            1.05,
        );

        let mut rec = On::default();
        let vis = visibility(&gearing_hull(), "HULL", &bundle, &sources, true, None, None, &mut rec);
        let prov = rec.into_provenance();

        // SeaDetection: base=7.33, steps: visibilityFactor * 0.9, visibilityDistCoeff * 0.97, GMBigGunVisibilityCoeff * 1.05.
        let sea_attr =
            prov.attributions.iter().find(|a| a.stat == TtxStat::SeaDetection).expect("SeaDetection recorded");
        assert_eq!(sea_attr.base_value, 7.33);
        assert!(
            matches!(&sea_attr.base_source, InputId::Module { slot, .. } if *slot == ModuleSlot::Hull),
            "base source must be Hull"
        );
        assert_eq!(
            sea_attr.steps.len(),
            3,
            "expected visibilityFactor + visibilityDistCoeff + GMBigGunVisibilityCoeff"
        );
        assert!(sea_attr.steps.iter().all(|c| c.op == Op::Mul), "all sea steps must be Mul");
        // All three steps present by name.
        assert!(sea_attr.steps.iter().any(|c| c.modifier_name == "visibilityFactor"));
        assert!(sea_attr.steps.iter().any(|c| c.modifier_name == "visibilityDistCoeff"));
        assert!(sea_attr.steps.iter().any(|c| c.modifier_name == "GMBigGunVisibilityCoeff"));

        let expected_sea = vis.sea_detection.expect("sea").value();
        let replayed_sea = ShipStatsProvenance::replay(sea_attr);
        assert!(
            (replayed_sea - expected_sea).abs() < 1e-4,
            "SeaDetection replay got {replayed_sea}, expected {expected_sea}"
        );

        // SeaDetectionOnFire: base=sea, step module_add visibilityCoefFire +2.0.
        let on_fire_attr = prov
            .attributions
            .iter()
            .find(|a| a.stat == TtxStat::SeaDetectionOnFire)
            .expect("SeaDetectionOnFire recorded");
        assert_eq!(on_fire_attr.steps.len(), 1, "SeaDetectionOnFire has one Add step");
        assert_eq!(on_fire_attr.steps[0].op, Op::Add);
        assert!((on_fire_attr.steps[0].operand - 2.0).abs() < 1e-6, "fire penalty is 2.0");

        let expected_on_fire = vis.sea_detection_on_fire.expect("sea on fire").value();
        let replayed_on_fire = ShipStatsProvenance::replay(on_fire_attr);
        assert!(
            (replayed_on_fire - expected_on_fire).abs() < 1e-4,
            "SeaDetectionOnFire replay got {replayed_on_fire}, expected {expected_on_fire}"
        );
    }

    #[test]
    fn artillery_reload_records_gmshot_delay_and_replay_exact() {
        use crate::game_params::ttx::provenance::{On, Op, ShipStatsProvenance};

        let mods = [modifier("GMShotDelay", 0.9)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Cruiser, VERSION).expect("test modifiers are all known");

        let mut sources = ModifierSources::default();
        sources.record("GMShotDelay", InputId::Upgrade { name: "U".into() }, 0.9);

        let provider = worcester_provider();
        let mut rec = On::default();
        let arty =
            artillery(&worcester_artillery(), "ARTY", None, &bundle, &sources, 1.0, 1.0, 1.0, 10, &provider, &mut rec)
                .expect("artillery computed");

        let prov = rec.into_provenance();

        let reload_attr = prov
            .attributions
            .iter()
            .find(|a| a.stat == TtxStat::ArtilleryReloadTime)
            .expect("ArtilleryReloadTime recorded");

        assert_eq!(reload_attr.base_value, 4.6);
        assert!(
            matches!(&reload_attr.base_source, InputId::Module { slot, .. } if *slot == ModuleSlot::Artillery),
            "base source must be the artillery module"
        );

        let gm_step = reload_attr.steps.iter().find(|c| c.modifier_name == "GMShotDelay");
        assert!(gm_step.is_some(), "GMShotDelay step must be recorded");
        assert_eq!(gm_step.unwrap().op, Op::Mul);
        assert!((gm_step.unwrap().operand - 0.9).abs() < 1e-6);

        let expected_reload = arty.reload_time.expect("reload").value();
        let replayed = ShipStatsProvenance::replay(reload_attr);
        assert!((replayed - expected_reload).abs() < 1e-4, "replay got {replayed}, expected {expected_reload}");
    }

    #[test]
    fn artillery_range_records_fc_coef_step_and_replay_exact() {
        use crate::game_params::ttx::provenance::{On, ShipStatsProvenance};

        let provider = worcester_provider();
        let mut rec = On::default();
        let fc_coef = 1.2f32;
        let arty = artillery(
            &worcester_artillery(),
            "ARTY",
            Some("FC"),
            &ModifierBundle::empty(Species::Cruiser),
            &ModifierSources::default(),
            fc_coef,
            1.0,
            1.0,
            10,
            &provider,
            &mut rec,
        )
        .expect("artillery computed");

        let prov = rec.into_provenance();

        let range_attr =
            prov.attributions.iter().find(|a| a.stat == TtxStat::ArtilleryRange).expect("ArtilleryRange recorded");

        let fc_step = range_attr.steps.iter().find(|c| c.modifier_name == "maxDistCoef");
        assert!(fc_step.is_some(), "maxDistCoef step must be recorded when fc_max_dist_coef != 1.0");
        assert!(
            matches!(&fc_step.unwrap().input, InputId::Module { slot, .. } if *slot == ModuleSlot::FireControl),
            "maxDistCoef step must be attributed to FireControl module"
        );
        assert!((fc_step.unwrap().operand - 1.2).abs() < 1e-6);

        let expected_range = arty.range.expect("range").value();
        let replayed = ShipStatsProvenance::replay(range_attr);
        assert!((replayed - expected_range).abs() < 1e-3, "replay got {replayed}, expected {expected_range}");
    }

    /// `ArtilleryDispersionVertical` provenance replay must reproduce the factory value exactly
    /// when the gun has non-trivial curve fields (coeff != 1.0). The vertical dispersion is
    /// `base_h * coeff * GMIdealRadius`; recording `base_h * coeff` as the base value and
    /// a single `GMIdealRadius` Mul step makes `ShipStatsProvenance::replay` reproduce it.
    #[test]
    fn artillery_vertical_dispersion_records_exact_replay() {
        use crate::game_params::ttx::provenance::{On, ShipStatsProvenance};

        let mods = [modifier("GMIdealRadius", 0.95)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Cruiser, VERSION).expect("test modifiers are all known");

        let mut sources = ModifierSources::default();
        sources.record("GMIdealRadius", InputId::Upgrade { name: "U".into() }, 0.95);

        let provider = worcester_provider();
        let mut rec = On::default();
        let arty = artillery(
            &worcester_artillery_with_curve(),
            "ARTY",
            None,
            &bundle,
            &sources,
            1.0,
            1.0,
            1.0,
            10,
            &provider,
            &mut rec,
        )
        .expect("artillery computed");

        let prov = rec.into_provenance();

        let v_attr = prov
            .attributions
            .iter()
            .find(|a| a.stat == TtxStat::ArtilleryDispersionVertical)
            .expect("ArtilleryDispersionVertical recorded");

        let expected_v = arty.dispersion_vertical.expect("dispersion_vertical").value();
        let replayed = ShipStatsProvenance::replay(v_attr);
        let rel_err = (replayed - expected_v).abs() / expected_v;
        assert!(
            rel_err < 1e-4,
            "vertical dispersion replay not exact: replay={replayed}, factory={expected_v}, rel_err={rel_err}"
        );
    }

    /// Bismarck secondaries with the four vertical-dispersion curve fields set,
    /// so `SecondaryDispersionVertical` is recorded.
    fn bismarck_secondaries_with_curve() -> SecondaryComponentStats {
        let gun_150 = ArtilleryGunStats {
            shot_delay: Some(Seconds::from(7.5)),
            rotation_speed: Some(DegreesPerSecond::from(60.0)),
            num_barrels: Some(2.0),
            barrel_diameter: Some(Meters::from(0.15)),
            ammo_switch_coeff: None,
            min_radius: Some(1.0),
            ideal_radius: Some(15.5),
            ideal_distance: Some(333.333),
            radius_on_zero: Some(1.0),
            radius_on_delim: Some(1.5),
            radius_on_max: Some(2.0),
            delim: Some(0.5),
            ammo: vec!["PGPA003_150mm_HE_HE_N_F".to_string()],
        };
        SecondaryComponentStats { max_dist: Some(Meters::from(7600.0)), guns: vec![gun_150] }
    }

    /// `SecondaryRange` records a Hull-module base + `GSMaxDist` Mul step; replay reproduces
    /// the final value exactly.
    #[test]
    fn secondary_range_records_gs_max_dist_and_replay_exact() {
        use crate::game_params::ttx::provenance::{On, ShipStatsProvenance};

        let mods = [modifier("GSMaxDist", 1.2)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");

        let mut sources = ModifierSources::default();
        sources.record("GSMaxDist", InputId::Upgrade { name: "U".into() }, 1.2);

        let provider = bismarck_secondary_provider();
        let mut rec = On::default();
        let sec = secondaries(&bismarck_secondaries(), "HULL_B", &bundle, &sources, 1.0, 8, &provider, &mut rec)
            .expect("secondaries computed");

        let prov = rec.into_provenance();

        let range_attr =
            prov.attributions.iter().find(|a| a.stat == TtxStat::SecondaryRange).expect("SecondaryRange recorded");

        assert!(
            matches!(&range_attr.base_source, InputId::Module { slot, .. } if *slot == ModuleSlot::Hull),
            "SecondaryRange base source must be the Hull module"
        );

        let gs_step = range_attr.steps.iter().find(|c| c.modifier_name == "GSMaxDist");
        assert!(gs_step.is_some(), "GSMaxDist step must be recorded");
        assert!((gs_step.unwrap().operand - 1.2).abs() < 1e-6);

        let expected_range = sec.range.expect("range").value();
        let replayed = ShipStatsProvenance::replay(range_attr);
        assert!((replayed - expected_range).abs() < 1e-4, "replay got {replayed}, expected {expected_range}");
    }

    /// `SecondaryDispersionVertical` replay must use `base_h * coeff` as the base value (not
    /// `base_h` alone), so a single `GSIdealRadius` Mul step reproduces the final value.
    /// A base of `base_h` alone would give a wrong replay when `coeff != 1.0`.
    #[test]
    fn secondary_vertical_dispersion_records_exact_replay() {
        use crate::game_params::ttx::provenance::{On, ShipStatsProvenance};

        let mods = [modifier("GSIdealRadius", 0.95)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");

        let mut sources = ModifierSources::default();
        sources.record("GSIdealRadius", InputId::Upgrade { name: "U".into() }, 0.95);

        let provider = bismarck_secondary_provider();
        let mut rec = On::default();
        let sec =
            secondaries(&bismarck_secondaries_with_curve(), "HULL_B", &bundle, &sources, 1.0, 8, &provider, &mut rec)
                .expect("secondaries computed");

        let prov = rec.into_provenance();

        let v_attr = prov
            .attributions
            .iter()
            .find(|a| a.stat == TtxStat::SecondaryDispersionVertical)
            .expect("SecondaryDispersionVertical recorded");

        let expected_v = sec.dispersion_vertical.expect("dispersion_vertical").value();
        let replayed = ShipStatsProvenance::replay(v_attr);
        let rel_err = (replayed - expected_v).abs() / expected_v;
        assert!(
            rel_err < 1e-4,
            "vertical dispersion replay not exact: replay={replayed}, factory={expected_v}, rel_err={rel_err}"
        );

        // Confirm the base is NOT bare base_h (which would give the wrong answer when coeff != 1.0).
        // The range at max_dist 7.6 km, ideal_radius_coef 1.0, produces some base_h.
        // With coeff != 1.0 (at max range, radius_on_max=2.0 -> coeff=2.0), replaying
        // base_h alone * 0.95 would not equal expected_v.
        let range_km =
            atba_range_km_for_test(&bismarck_secondaries_with_curve(), &ModifierBundle::empty(Species::Battleship));
        let base_h_only =
            constants::dispersion_horizontal(1.0, 15.5, 333.333, Km::from(range_km), 1.0).to_meters().value();
        let wrong_replay = base_h_only * 0.95;
        assert!(
            (wrong_replay - expected_v).abs() > 1e-3,
            "a base_h-only replay should NOT equal the vertical value (coeff != 1.0 case)"
        );
    }

    fn atba_range_km_for_test(atba: &SecondaryComponentStats, modifiers: &ModifierBundle) -> f32 {
        atba.max_dist.map(|d| (d.value() / KM_TO_M) * modifiers.coef("GSMaxDist")).unwrap_or(0.0)
    }
}
