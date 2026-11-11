//! What shells and depth charges do to submarines: splash damage by distance
//! from the burst, splash radius, the deepest depth they reach, damage per
//! submarine depth state and damage per target class.

use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;
use wows_core::units::Meters;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::BuoyancyDamage;
use wowsunpack::game_params::types::ClassVulnerability;
use wowsunpack::game_params::types::Projectile;
use wowsunpack::game_params::types::SplashDamagePoint;
use wowsunpack::game_types::BuoyancyState;
use wowsunpack::recognized::Recognized;

use super::AppliesTo;
use super::Evaluator;
use super::Parameter;
use super::ParameterType;
use super::Precision;
use super::QuantityDefinition;
use super::QuantityValue;
use super::ShipEvaluation;
use super::Source;
use super::read_params;
use crate::armor_trace::largest_shell;
use crate::builds::Better;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::data::GameData;
use crate::error::QueryError;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ships::Ship;
use crate::ships::ShipClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnderwaterQuantity {
    SplashCoefficient,
    SplashRadius,
    MaxDepth,
    DepthStateDamage,
    ClassDamage,
}

impl UnderwaterQuantity {
    const ALL: [Self; 5] =
        [Self::SplashCoefficient, Self::SplashRadius, Self::MaxDepth, Self::DepthStateDamage, Self::ClassDamage];

    fn name(self) -> &'static str {
        match self {
            Self::SplashCoefficient => "underwater.splash_coefficient",
            Self::SplashRadius => "underwater.splash_radius",
            Self::MaxDepth => "underwater.max_depth",
            Self::DepthStateDamage => "underwater.depth_state_damage",
            Self::ClassDamage => "underwater.class_damage_multiplier",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::SplashCoefficient | Self::DepthStateDamage | Self::ClassDamage => "x",
            Self::SplashRadius | Self::MaxDepth => "m",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::SplashCoefficient => {
                "Splash damage to a submarine: the share of a shell's or depth charge's damage dealt by a burst near it, by distance from the burst as a fraction of the splash radius (pointsOfDamage). Most AP shells deal 0.1, HE and SAP 0.165; depth charges deal full damage near the burst and less farther out."
            }
            Self::SplashRadius => {
                "Splash radius of a shell or depth charge under water (depthSplashRadius), in the game world's meters (the scale ship models are drawn at): how far from a submerged submarine a burst still damages it."
            }
            Self::MaxDepth => {
                "Deepest depth a shell or depth charge hits, meters below the surface (maxDepth): a submarine whose waterline is deeper cannot be hit by it."
            }
            Self::DepthStateDamage => {
                "Share of a shell's or depth charge's damage a submarine takes in each depth state (surface, periscope, semi_deep_water, deep_water, deep_water_invul), 0 where it cannot hit a submarine at that depth (ignoreBuoyancyStates): why AP does little to submerged submarines while HE and SAP still hit at semi-deep water."
            }
            Self::ClassDamage => {
                "Damage multiplier of a shell or depth charge against each target class, submarines included (damageVulnerabilityForShipClasses)."
            }
        }
    }

    fn sources(self) -> Vec<Source> {
        const DEFAULTS: Source = Source::ClientScript(
            "m65e88a3f.py classes Ammo and DepthCharge (defaults when a projectile omits the field)",
        );
        const CAN_HIT: Source = Source::ClientScript("VehicleUtils.py canHit (ignoreBuoyancyStates, maxDepth)");

        match self {
            Self::SplashCoefficient => vec![Source::GameParams("Projectile.pointsOfDamage"), DEFAULTS],
            Self::SplashRadius => vec![
                Source::GameParams("Projectile.depthSplashRadius"),
                DEFAULTS,
                Source::ClientScript(
                    "Modifiers/ModifiersApply.py getDepthSplashRadius (times dcSplashRadiusMultiplier or ammoSplashRadiusMultiplier)",
                ),
            ],
            Self::MaxDepth => vec![Source::GameParams("Projectile.maxDepth"), DEFAULTS, CAN_HIT],
            Self::DepthStateDamage => vec![
                Source::GameParams("Projectile.buoyancyToDamageCoeff, Projectile.ignoreBuoyancyStates"),
                DEFAULTS,
                CAN_HIT,
            ],
            Self::ClassDamage => vec![
                Source::GameParams("Projectile.damageVulnerabilityForShipClasses"),
                Source::ClientScript("m65e88a3f.py GAME_PARAMS_COMPONENTS_NAMES.CONFIGURABLE_DAMAGER and class Ammo"),
            ],
        }
    }

    fn parameters(self) -> Vec<Parameter> {
        const PROJECTILE: Parameter = Parameter {
            name: "projectile",
            kind: ParameterType::Choice { options: &["AP", "HE", "SAP", "depth_charge"] },
            required: false,
            default: None,
            description: "Only shells of this ammo type, or only depth charges. Without it, every shell and depth charge; a group needs it.",
        };
        const BATTERY: Parameter = Parameter {
            name: "battery",
            kind: ParameterType::Choice { options: &["main", "secondary"] },
            required: false,
            default: None,
            description: "Only shells of this battery. Without it, both; a group takes the main battery.",
        };
        const RELATIVE_DISTANCE: Parameter = Parameter {
            name: "relative_distance",
            kind: ParameterType::Number { unit: "fraction of the splash radius, 0 to 1" },
            required: false,
            default: Some("0, at the burst"),
            description: "Distance from the burst over the splash radius.",
        };
        const STATE: Parameter = Parameter {
            name: "state",
            kind: ParameterType::Choice {
                options: &["surface", "periscope", "semi_deep_water", "deep_water", "deep_water_invul"],
            },
            required: false,
            default: None,
            description: "Submarine depth state. Without it, every state; a group needs it.",
        };
        const TARGET_CLASS: Parameter = Parameter {
            name: "target_class",
            kind: ParameterType::ShipClass,
            required: false,
            default: None,
            description: "Class of the target. Without it, every class; a group needs it.",
        };

        match self {
            Self::SplashCoefficient => vec![PROJECTILE, BATTERY, RELATIVE_DISTANCE],
            Self::SplashRadius | Self::MaxDepth => vec![PROJECTILE, BATTERY],
            Self::DepthStateDamage => vec![PROJECTILE, BATTERY, STATE],
            Self::ClassDamage => vec![PROJECTILE, BATTERY, TARGET_CLASS],
        }
    }

    pub(super) fn prepare(&self, params: &Map<String, Value>) -> Result<PreparedUnderwater, QueryError> {
        let params: UnderwaterParams = read_params(self.name(), params)?;
        let relative_distance = params.relative_distance.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&relative_distance) {
            return Err(QueryError::InvalidArguments { reason: "relative_distance must be from 0 to 1" });
        }
        let accepted = match self {
            Self::SplashCoefficient => params.state.is_none() && params.target_class.is_none(),
            Self::SplashRadius | Self::MaxDepth => {
                params.state.is_none() && params.target_class.is_none() && params.relative_distance.is_none()
            }
            Self::DepthStateDamage => params.target_class.is_none() && params.relative_distance.is_none(),
            Self::ClassDamage => params.state.is_none() && params.relative_distance.is_none(),
        };
        if !accepted {
            return Err(QueryError::InvalidArguments {
                reason: "a parameter given is not one of this quantity's; quantity_search lists them",
            });
        }

        Ok(PreparedUnderwater {
            quantity: *self,
            projectile: params.projectile,
            battery: params.battery,
            relative_distance,
            state: params.state,
            target_class: params.target_class,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnderwaterParams {
    projectile: Option<ProjectileKind>,
    battery: Option<Battery>,
    relative_distance: Option<f32>,
    state: Option<DepthState>,
    target_class: Option<ShipClass>,
}

/// A shell ammo type or depth charges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum ProjectileKind {
    #[serde(rename = "AP")]
    Ap,
    #[serde(rename = "HE")]
    He,
    #[serde(rename = "SAP")]
    Sap,
    #[serde(rename = "depth_charge")]
    DepthCharge,
}

impl ProjectileKind {
    fn ammo(self) -> Option<Ammo> {
        match self {
            Self::Ap => Some(Ammo::Ap),
            Self::He => Some(Ammo::He),
            Self::Sap => Some(Ammo::Sap),
            Self::DepthCharge => None,
        }
    }
}

/// A submarine depth state as players and the parameter name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DepthState {
    Surface,
    Periscope,
    SemiDeepWater,
    DeepWater,
    DeepWaterInvul,
}

impl DepthState {
    const ALL: [Self; 5] = [Self::Surface, Self::Periscope, Self::SemiDeepWater, Self::DeepWater, Self::DeepWaterInvul];

    fn state(self) -> BuoyancyState {
        match self {
            Self::Surface => BuoyancyState::Surface,
            Self::Periscope => BuoyancyState::Periscope,
            Self::SemiDeepWater => BuoyancyState::SemiDeepWater,
            Self::DeepWater => BuoyancyState::DeepWater,
            Self::DeepWaterInvul => BuoyancyState::DeepWaterInvul,
        }
    }
}

pub(super) struct PreparedUnderwater {
    quantity: UnderwaterQuantity,
    projectile: Option<ProjectileKind>,
    battery: Option<Battery>,
    relative_distance: f32,
    state: Option<DepthState>,
    target_class: Option<ShipClass>,
}

/// A shell or depth charge of a ship.
struct Carried {
    label: String,
    param: String,
    depth_charge: bool,
}

impl PreparedUnderwater {
    pub(super) fn counts(&self) -> String {
        let build = if self.quantity == UnderwaterQuantity::SplashRadius { " on its best-fit build" } else { "" };
        match self.projectile {
            Some(ProjectileKind::DepthCharge) => {
                format!("each ship's first ship-mounted depth charge, else its air strike's{build}")
            }
            Some(kind) => format!(
                "the largest-caliber {} {} shell of each ship{build}",
                self.battery.unwrap_or(Battery::Main).label(),
                kind.ammo().map_or("", Ammo::label)
            ),
            None => String::new(),
        }
    }

    pub(super) fn check_group(&self) -> Result<(), QueryError> {
        if self.projectile.is_none() {
            return Err(QueryError::InvalidArguments {
                reason: "a group needs projectile (AP, HE, SAP or depth_charge) in the params of underwater quantities",
            });
        }
        let one_value = match self.quantity {
            UnderwaterQuantity::DepthStateDamage => self.state.is_some(),
            UnderwaterQuantity::ClassDamage => self.target_class.is_some(),
            UnderwaterQuantity::SplashCoefficient | UnderwaterQuantity::SplashRadius | UnderwaterQuantity::MaxDepth => {
                true
            }
        };
        if !one_value {
            return Err(QueryError::InvalidArguments {
                reason: "a group needs state for underwater.depth_state_damage and target_class for underwater.class_damage_multiplier",
            });
        }

        Ok(())
    }

    pub(super) fn evaluate(&self, game: &GameData, ship: &Ship) -> Result<ShipEvaluation, QueryError> {
        let (carried, counted_param) = self.carried(game, ship);
        if carried.is_empty() {
            return Ok(ShipEvaluation::absent(format!("{} has no such shells or depth charges", ship.name)));
        }

        let planner = Planner::new(game, ship);
        let mut values = Vec::new();
        let mut counted = None;
        for item in &carried {
            let projectile = game.projectile(&item.param)?;
            if self.quantity == UnderwaterQuantity::SplashRadius {
                let (readings, best) = self.splash_radius_readings(&planner, &projectile, item)?;
                if counted_param.as_deref() == Some(item.param.as_str()) {
                    counted = best;
                }
                values.extend(readings);
                continue;
            }

            let readings = self.readings(&projectile);
            if counted_param.as_deref() == Some(item.param.as_str()) {
                counted = readings.first().and_then(|(_, value, _)| *value);
            }
            for (qualifier, value, note) in readings {
                let option = match qualifier {
                    Some(qualifier) => format!("{}, {qualifier}", item.label),
                    None => item.label.clone(),
                };
                values.push(QuantityValue::new(Some(option), None, value, note));
            }
        }

        Ok(ShipEvaluation::new(values, None, counted))
    }

    /// The ship's shells and depth charges this quantity covers, and the one
    /// a group counts.
    fn carried(&self, game: &GameData, ship: &Ship) -> (Vec<Carried>, Option<String>) {
        let ammo = self.projectile.and_then(ProjectileKind::ammo);
        let shells: Vec<Shell> = if self.projectile == Some(ProjectileKind::DepthCharge) {
            Vec::new()
        } else {
            game.shells(ship)
                .into_iter()
                .filter(|shell| self.battery.is_none_or(|battery| shell.battery == battery))
                .filter(|shell| ammo.is_none_or(|ammo| shell.ammo == ammo))
                .collect()
        };
        let depth_charges = if self.projectile.is_none_or(|kind| kind == ProjectileKind::DepthCharge) {
            game.depth_charge_params(ship)
        } else {
            Vec::new()
        };

        let counted = match self.projectile {
            Some(ProjectileKind::DepthCharge) => depth_charges.first().map(|(_, param)| param.clone()),
            Some(_) => {
                let main: Vec<Shell> = shells
                    .iter()
                    .filter(|shell| shell.battery == self.battery.unwrap_or(Battery::Main))
                    .cloned()
                    .collect();
                ammo.and_then(|ammo| largest_shell(&main, ammo)).map(|shell| shell.param.clone())
            }
            None => None,
        };

        let mut carried: Vec<Carried> = shells
            .into_iter()
            .map(|shell| Carried {
                label: format!("{} {}", shell.battery.label(), shell.name),
                param: shell.param,
                depth_charge: false,
            })
            .collect();
        carried.extend(depth_charges.into_iter().map(|(weapon, param)| Carried {
            label: format!("{} {}", weapon.label(), game.projectile_name(&param)),
            param,
            depth_charge: true,
        }));

        (carried, counted)
    }

    fn splash_radius_readings(
        &self,
        planner: &Planner<'_>,
        projectile: &Projectile,
        item: &Carried,
    ) -> Result<(Vec<QuantityValue>, Option<f32>), QueryError> {
        let Some(radius) = projectile.depth_splash_radius() else {
            let value = QuantityValue::new(
                Some(item.label.clone()),
                None,
                None,
                Some("game data lists no depthSplashRadius for it".to_owned()),
            );
            return Ok((vec![value], None));
        };

        let extract =
            |outcome: &Outcome| Some(SplashRadius(effective_splash_radius(radius, &outcome.bundle, item.depth_charge)));
        let plan = StatPlan::new("underwater splash radius".to_owned(), Better::Higher, &extract);
        let Some(builds) = planner.best(&plan)? else {
            return Ok((Vec::new(), None));
        };

        let base = builds.base.0.value();
        let best = builds.best.value.0.value();
        let mut values = vec![QuantityValue::new(Some(item.label.clone()), Some("base".to_owned()), Some(base), None)];
        if !builds.best.assumed.is_empty() && best != base {
            values.push(QuantityValue::new(
                Some(item.label.clone()),
                Some(format!("with {}", builds.best.assumed.join(", "))),
                Some(best),
                None,
            ));
        }

        Ok((values, Some(best)))
    }

    /// Values of one projectile, each with what it is for when there are
    /// several (a depth state or target class).
    fn readings(&self, projectile: &Projectile) -> Vec<(Option<String>, Option<f32>, Option<String>)> {
        let absent = |field: &str| (None, None, Some(format!("game data lists no {field} for it")));

        match self.quantity {
            UnderwaterQuantity::SplashCoefficient => match projectile.points_of_damage() {
                Some(points) if !points.is_empty() => {
                    let curve = points
                        .iter()
                        .map(|point| format!("{} at {}", point.coefficient, point.relative_distance))
                        .collect::<Vec<_>>()
                        .join(", ");
                    vec![(
                        None,
                        splash_coefficient(points, self.relative_distance)
                            .map(|coefficient| Precision::Thousandths.round(coefficient)),
                        Some(format!("pointsOfDamage: {curve}")),
                    )]
                }
                _ => vec![absent("pointsOfDamage")],
            },
            UnderwaterQuantity::SplashRadius => {
                unreachable!("splash radius readings require the build planner")
            }
            UnderwaterQuantity::MaxDepth => match projectile.max_depth() {
                Some(depth) => vec![(None, Some(-depth.value()), None)],
                None => vec![absent("maxDepth")],
            },
            UnderwaterQuantity::DepthStateDamage => {
                let states: Vec<DepthState> = match self.state {
                    Some(state) => vec![state],
                    None => DepthState::ALL.to_vec(),
                };
                let ignored = projectile.ignore_buoyancy_states().unwrap_or_default();
                let coefficients = projectile.buoyancy_to_damage_coeff().unwrap_or_default();
                states
                    .into_iter()
                    .map(|state| {
                        let name = state.state().name();
                        let qualifier = Some(name.to_ascii_lowercase());
                        if ignored.iter().any(|ignored| ignored.known() == Some(&state.state())) {
                            return (
                                qualifier,
                                Some(0.0),
                                Some(format!("cannot hit: ignoreBuoyancyStates lists {name}")),
                            );
                        }
                        match coefficient_for(coefficients, state.state()) {
                            Some(coefficient) => (qualifier, Some(coefficient), None),
                            None => (qualifier, None, Some(format!("buoyancyToDamageCoeff lists no {name}"))),
                        }
                    })
                    .collect()
            }
            UnderwaterQuantity::ClassDamage => {
                let Some(vulnerabilities) = projectile.damage_vulnerability_for_ship_classes() else {
                    return vec![absent("damageVulnerabilityForShipClasses (no per-class multiplier)")];
                };
                let classes: Vec<ShipClass> = match self.target_class {
                    Some(class) => vec![class],
                    None => ShipClass::ALL.to_vec(),
                };
                classes
                    .into_iter()
                    .map(|class| {
                        let qualifier = Some(class.label().replace('_', " "));
                        match class_coefficient(vulnerabilities, class) {
                            Some(coefficient) => (qualifier, Some(coefficient), None),
                            None => (
                                qualifier,
                                None,
                                Some("damageVulnerabilityForShipClasses lists no such class".to_owned()),
                            ),
                        }
                    })
                    .collect()
            }
        }
    }
}

fn effective_splash_radius(radius: Meters, bundle: &ModifierBundle, depth_charge: bool) -> Meters {
    let modifier = if depth_charge { "dcSplashRadiusMultiplier" } else { "ammoSplashRadiusMultiplier" };

    radius * bundle.coef(modifier)
}

#[derive(Debug, Clone, Copy)]
struct SplashRadius(Meters);

impl StatAmount for SplashRadius {
    fn amount(self) -> f32 {
        self.0.value()
    }
}

/// The splash damage share at `relative_distance`, linear between points and
/// the nearest point's share beyond them.
fn splash_coefficient(points: &[SplashDamagePoint], relative_distance: f32) -> Option<f32> {
    let first = points.first()?;
    let last = points.last()?;
    if relative_distance <= first.relative_distance {
        return Some(first.coefficient);
    }
    if relative_distance >= last.relative_distance {
        return Some(last.coefficient);
    }

    points.windows(2).find_map(|pair| {
        let [near, far] = pair else {
            return None;
        };
        if relative_distance > far.relative_distance {
            return None;
        }

        let span = far.relative_distance - near.relative_distance;
        let along = if span > 0.0 { (relative_distance - near.relative_distance) / span } else { 1.0 };

        Some(near.coefficient + (far.coefficient - near.coefficient) * along)
    })
}

fn coefficient_for(coefficients: &[BuoyancyDamage], state: BuoyancyState) -> Option<f32> {
    coefficients.iter().find(|entry| entry.state.known() == Some(&state)).map(|entry| entry.coefficient)
}

fn class_coefficient(vulnerabilities: &[ClassVulnerability], class: ShipClass) -> Option<f32> {
    vulnerabilities
        .iter()
        .find(|entry| matches!(&entry.class, Recognized::Known(species) if *species == class.species()))
        .map(|entry| entry.coefficient)
}

pub(super) fn definitions() -> Vec<QuantityDefinition> {
    UnderwaterQuantity::ALL
        .into_iter()
        .map(|quantity| QuantityDefinition {
            name: quantity.name().to_owned(),
            unit: quantity.unit(),
            applies_to: AppliesTo::ShellOrDepthCharge,
            description: quantity.description().to_owned(),
            parameters: quantity.parameters(),
            sources: quantity.sources(),
            evaluator: Evaluator::Underwater(quantity),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_charge_splash_steps_down_past_the_inner_blast() {
        let point = |relative_distance, coefficient| SplashDamagePoint { relative_distance, coefficient };
        let points = [point(0.0, 1.0), point(0.15, 1.0), point(0.151, 0.33), point(1.0, 0.33)];

        let shares: Vec<Option<f32>> =
            [0.0, 0.1, 0.15, 0.5, 1.0].into_iter().map(|distance| splash_coefficient(&points, distance)).collect();
        assert_eq!(shares, [Some(1.0), Some(1.0), Some(1.0), Some(0.33), Some(0.33)]);
    }
}

#[cfg(test)]
mod splash_radius_tests {
    use wows_core::Version;
    use wowsunpack::game_params::types::CrewSkillModifier;
    use wowsunpack::game_params::types::Species;

    use super::*;

    fn modifier(name: &str, value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_owned())
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
    fn splash_radius_uses_the_projectile_kind_modifier() {
        let bundle = ModifierBundle::from_modifiers(
            &[modifier("ammoSplashRadiusMultiplier", 1.15), modifier("dcSplashRadiusMultiplier", 1.2)],
            Species::Cruiser,
            Version::base(15, 0, 0),
        )
        .expect("known splash modifiers");
        let radius = Meters::new(10.0);

        assert_eq!(effective_splash_radius(radius, &bundle, false), Meters::new(11.5));
        assert_eq!(effective_splash_radius(radius, &bundle, true), Meters::new(12.0));
    }
}
