//! TTX factory functions: apply per-species formulas + the equipped
//! `ModifierBundle` to base component stats, producing unit-carrying `ShipStats`
//! sections. Formulas are transcribed verbatim from the deob; each is cited at
//! its application site.
//!
//! This module currently covers the direct-field species `durability` and
//! `mobility`; armor/battery/hull-summary land in later M2 tasks.

use crate::game_params::ttx::components::EngineComponentStats;
use crate::game_params::ttx::components::HullComponentStats;
use crate::game_params::ttx::constants::HULL_HEALTH_ROUND;
use crate::game_params::ttx::model::Durability;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Knots;
use crate::game_params::ttx::model::Mobility;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::types::Meters;

/// Survivability section (`ma6320f36/ttx/FactoryDurability.py:5`).
///
/// `health` transcribes `Modifiers/ModifiersApply.py:142`'s `calculateVehicleHealth`:
/// `(hull.health + healthPerLevel*level) * healthHullCoeff`, then rounded up to a
/// multiple of `HULL_HEALTH_ROUND` (line 143). `healthPerLevel` is additive
/// (base_value 0.0 -> `bonus`); `healthHullCoeff` is multiplicative (base_value 1.0
/// -> `coef`).
///
/// `torpedo_protection` (ptz, FactoryDurability.py:8) needs the hull `floodProb`
/// derived from `floodNodes`, which M1 deferred and `HullComponentStats` does not
/// carry; it is `None` here rather than fabricated.
pub fn durability(hull: &HullComponentStats, modifiers: &ModifierBundle, level: u32) -> Durability {
    let health = hull.health.map(|base| {
        let raw = (base + modifiers.bonus("healthPerLevel") * level as f32) * modifiers.coef("healthHullCoeff");
        // ceil(raw / round) * round (ModifiersApply.py:143).
        let rounded = (raw / HULL_HEALTH_ROUND).ceil() * HULL_HEALTH_ROUND;
        Hp::from(rounded)
    });

    Durability { health, torpedo_protection: None }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::CrewSkillModifier;
    use crate::game_params::types::Species;

    /// The build whose `MODIFIER_SETTINGS` table is transcribed in the toolkit.
    const BUILD: u32 = 11791718;

    /// Gearing's real default-hull base stats (GameParams `PASD013_Gearing_1945`
    /// `A_Hull`): health 19400, maxSpeed 36, speedCoef 1.0, turningRadius 640,
    /// rudderTime 4.25, visibilityFactor 7.33.
    fn gearing_hull() -> HullComponentStats {
        HullComponentStats {
            health: Some(19400.0),
            max_speed: Some(36.0),
            speed_coef: Some(1.0),
            turning_radius: Some(640.0),
            rudder_time: Some(4.25),
            visibility_factor: Some(7.33),
        }
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
    fn gearing_stock_durability_ptz_deferred() {
        // floodNodes/floodProb deferred in M1 -> ptz not computable.
        let durability = durability(&gearing_hull(), &ModifierBundle::empty(Species::Destroyer), 10);
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
    }
}
