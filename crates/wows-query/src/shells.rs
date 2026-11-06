use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::MetersPerSecond;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wowsunpack::ballistics::OVERMATCH_RATIO;
use wowsunpack::ballistics::is_overmatch;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Ship;
use crate::ships::ShipArgs;
use crate::ships::ShipSummary;

/// Internal name of Inertia Fuse for HE Shells.
pub(crate) const IFHE_SKILL: &str = "HePenetration";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Battery {
    Main,
    Secondary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub enum Ammo {
    #[serde(rename = "AP")]
    Ap,
    #[serde(rename = "HE")]
    He,
    #[serde(rename = "SAP")]
    Sap,
}

impl Ammo {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ap => "AP",
            Self::He => "HE",
            Self::Sap => "SAP",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Shell {
    /// GameParams name of the projectile.
    pub param: String,
    pub battery: Battery,
    pub ammo: Ammo,
    pub caliber_mm: Millimeters,

    /// HE or SAP penetration. AP penetration depends on range and impact angle
    /// and has no single value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_mm: Option<Millimeters>,

    /// HE penetration with Inertia Fuse for HE Shells, for this ship's class.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ifhe_penetration_mm: Option<Millimeters>,

    /// Thickest whole-mm plate AP overmatches (caliber > 14.3 x thickness).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overmatches_up_to_mm: Option<Millimeters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_damage: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub muzzle_velocity_mps: Option<MetersPerSecond>,

    /// Base chance to start a fire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_chance_pct: Option<Percent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipShells {
    pub ship: ShipSummary,
    pub shells: Vec<Shell>,
}

/// HE penetration multipliers Inertia Fuse for HE Shells gives one class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IfheCoefficients {
    pub(crate) main: f32,
    pub(crate) secondary: f32,
}

impl IfheCoefficients {
    fn for_battery(self, battery: Battery) -> f32 {
        match battery {
            Battery::Main => self.main,
            Battery::Secondary => self.secondary,
        }
    }
}

impl GameData {
    /// Main and secondary battery shells of one ship across all its gun modules.
    pub fn ship_shells(&self, args: &ShipArgs) -> Result<ShipShells, QueryError> {
        let ship = self.resolve_ship(&args.ship)?;

        Ok(ShipShells { ship: ship.summary(), shells: self.shells(ship) })
    }

    pub(crate) fn shells(&self, ship: &Ship) -> Vec<Shell> {
        let Some(config) = ship.param.vehicle().and_then(|vehicle| vehicle.config_data()) else {
            return Vec::new();
        };
        let ifhe = self.ifhe_coefficients(ship.class.species());

        let batteries =
            [(Battery::Main, &config.main_battery_ammo), (Battery::Secondary, &config.secondary_battery_ammo)];
        let mut shells = Vec::new();
        for (battery, names) in batteries {
            let mut names: Vec<&String> = names.iter().collect();
            names.sort();
            for name in names {
                let Some(param) = self.provider().game_param_by_name(name) else {
                    continue;
                };
                let Some(shell) = shell_from_param(&param, battery, ifhe) else {
                    continue;
                };
                shells.push(shell);
            }
        }

        shells
    }

    /// `None` when the build has no Inertia Fuse for HE Shells skill.
    pub(crate) fn ifhe_coefficients(&self, species: Species) -> Option<IfheCoefficients> {
        let skill = self.skills.iter().find(|skill| skill.internal_name().as_str() == IFHE_SKILL)?;
        let modifiers = skill.modifiers()?;

        // An absent coefficient leaves penetration unchanged, as in the client.
        let coefficient = |name: &str| {
            modifiers
                .iter()
                .find(|modifier| modifier.name() == name)
                .map_or(1.0, |modifier| modifier.get_for_species(&species))
        };

        Some(IfheCoefficients {
            main: coefficient("GMPenetrationCoeffHE"),
            secondary: coefficient("GSPenetrationCoeffHE"),
        })
    }
}

fn shell_from_param(param: &Param, battery: Battery, ifhe: Option<IfheCoefficients>) -> Option<Shell> {
    let projectile = param.projectile()?;
    let ammo = match AmmoType::from_game_str(projectile.ammo_type()) {
        AmmoType::AP => Ammo::Ap,
        AmmoType::HE => Ammo::He,
        AmmoType::SAP => Ammo::Sap,
        AmmoType::Unknown(_) => return None,
    };
    let caliber_mm = Millimeters::new(round_tenth(projectile.bullet_diametr()? * 1000.0));

    // The client floors HE and SAP penetration (FactoryArtillery.py).
    let (penetration_mm, ifhe_penetration_mm) = match ammo {
        Ammo::He => {
            let base = projectile.alpha_piercing_he();
            let ifhe = base.zip(ifhe).map(|(base, ifhe)| Millimeters::new((base * ifhe.for_battery(battery)).floor()));
            (base.map(|base| Millimeters::new(base.floor())), ifhe)
        }
        Ammo::Sap => (projectile.alpha_piercing_cs().map(|base| Millimeters::new(base.floor())), None),
        Ammo::Ap => (None, None),
    };
    let overmatches_up_to_mm = if ammo == Ammo::Ap { thickest_overmatched_plate(caliber_mm) } else { None };

    Some(Shell {
        param: param.name().to_owned(),
        battery,
        ammo,
        caliber_mm,
        penetration_mm,
        ifhe_penetration_mm,
        overmatches_up_to_mm,
        alpha_damage: projectile.alpha_damage(),
        muzzle_velocity_mps: projectile.bullet_speed().map(MetersPerSecond::new),
        fire_chance_pct: projectile
            .burn_prob()
            .filter(|chance| *chance > 0.0)
            .map(|chance| Percent::new(round_tenth(chance * 100.0))),
    })
}

fn round_tenth(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/// Thickest whole-mm plate `is_overmatch` accepts for `caliber`, `None` below 1 mm.
fn thickest_overmatched_plate(caliber: Millimeters) -> Option<Millimeters> {
    let mut thickness = (caliber.value() / OVERMATCH_RATIO).ceil();
    while thickness >= 1.0 && !is_overmatch(caliber, Millimeters::new(thickness)) {
        thickness -= 1.0;
    }
    (thickness >= 1.0).then(|| Millimeters::new(thickness))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thickest_overmatched_plate_is_the_last_whole_mm_overmatch() {
        for caliber in 100..=510u16 {
            let caliber = Millimeters::new(f32::from(caliber));
            let thickness = thickest_overmatched_plate(caliber).expect("calibers above 14.3 mm overmatch 1 mm");
            assert!(is_overmatch(caliber, thickness), "{caliber:?} overmatches {thickness:?}");
            assert!(
                !is_overmatch(caliber, Millimeters::new(thickness.value() + 1.0)),
                "{caliber:?} does not overmatch past {thickness:?}"
            );
        }
    }
}
