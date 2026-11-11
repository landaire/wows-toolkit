//! Shell ballistics at a firing range: flight time, impact velocity, fall
//! angle and raw penetration of each shell of an ammo type and battery.

use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;
use wows_core::units::Km;
use wowsunpack::ballistics::ImpactResult;

use super::AppliesTo;
use super::Evaluator;
use super::Parameter;
use super::ParameterType;
use super::QuantityDefinition;
use super::QuantityValue;
use super::ShipEvaluation;
use super::Source;
use super::is_finite_positive;
use super::read_params;
use crate::armor_trace::largest_shell;
use crate::builds::Better;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::comparison::number;
use crate::comparison::round2;
use crate::data::GameData;
use crate::error::QueryError;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::Shell;
use crate::ships::Ship;

/// What of the impact a quantity reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BallisticQuantity {
    FlightTime,
    ImpactVelocity,
    FallAngle,
    RawPenetration,
}

impl BallisticQuantity {
    const ALL: [Self; 4] = [Self::FlightTime, Self::ImpactVelocity, Self::FallAngle, Self::RawPenetration];

    fn name(self) -> &'static str {
        match self {
            Self::FlightTime => "shell.flight_time",
            Self::ImpactVelocity => "shell.impact_velocity",
            Self::FallAngle => "shell.fall_angle",
            Self::RawPenetration => "shell.raw_penetration",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::FlightTime => "s",
            Self::ImpactVelocity => "m/s",
            Self::FallAngle => "deg",
            Self::RawPenetration => "mm",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::FlightTime => {
                "Shell flight time to a target at a range: how long a shell takes to reach it, time to target on the in-game clock, from the shell's trajectory with air drag, not range divided by muzzle velocity."
            }
            Self::ImpactVelocity => {
                "Shell velocity on arrival at a range, after air drag slows it from its muzzle velocity."
            }
            Self::FallAngle => {
                "Angle from the horizontal at which a shell falls onto a target at a range: the impact angle, steeper at longer range."
            }
            Self::RawPenetration => {
                "AP shell penetration at a range before the impact angle: the game's Krupp formula at the impact velocity. HE and SAP penetrate their fixed values instead (ship_shells)."
            }
        }
    }

    fn value(self, impact: &ImpactResult) -> f32 {
        match self {
            Self::FlightTime => impact.time_to_target.value(),
            Self::ImpactVelocity => impact.impact_velocity.value(),
            Self::FallAngle => impact.impact_angle_horizontal.to_degrees().value(),
            Self::RawPenetration => impact.raw_penetration.value(),
        }
    }

    pub(super) fn prepare(&self, params: &Map<String, Value>) -> Result<PreparedBallistic, QueryError> {
        let params: BallisticParams = read_params(self.name(), params)?;
        if !is_finite_positive(params.range_km.value()) {
            return Err(QueryError::InvalidArguments { reason: "range_km must be finite and more than 0" });
        }
        if *self == Self::RawPenetration && params.ammo.is_some_and(|ammo| ammo != Ammo::Ap) {
            return Err(QueryError::InvalidArguments {
                reason: "shell.raw_penetration applies only to AP; HE and SAP use fixed penetration listed by ship_shells",
            });
        }

        Ok(PreparedBallistic {
            quantity: *self,
            range: params.range_km,
            ammo: params.ammo,
            battery: params.battery.unwrap_or(Battery::Main),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BallisticParams {
    range_km: Km,
    ammo: Option<Ammo>,
    battery: Option<Battery>,
}

pub(super) struct PreparedBallistic {
    quantity: BallisticQuantity,
    range: Km,
    ammo: Option<Ammo>,
    battery: Battery,
}

/// A firing range of the battery, `f32` for the build planner.
#[derive(Debug, Clone, Copy)]
struct Range(f32);

impl StatAmount for Range {
    fn amount(self) -> f32 {
        self.0
    }
}

impl PreparedBallistic {
    pub(super) fn counts(&self) -> String {
        let ammo = self.ammo().map_or("", Ammo::label);
        format!(
            "the largest-caliber {} {ammo} shell of each ship (as ship_comparison does), with no value past the battery's firing range on its best-fit build",
            self.battery.label()
        )
    }

    pub(super) fn check_group(&self) -> Result<(), QueryError> {
        match self.ammo() {
            Some(_) => Ok(()),
            None => Err(QueryError::InvalidArguments {
                reason: "a group needs ammo (AP, HE or SAP) in the params of shell quantities",
            }),
        }
    }

    pub(super) fn evaluate(&self, game: &GameData, ship: &Ship) -> Result<ShipEvaluation, QueryError> {
        let planner = Planner::new(game, ship);
        let ammo = self.ammo();
        let shells = carried_shells(game, &planner, self.battery, ammo)?;
        if shells.is_empty() {
            let ammo = ammo.map(|ammo| format!(" {}", ammo.label())).unwrap_or_default();
            return Ok(ShipEvaluation::absent(format!("{} has no {}{ammo} shells", ship.name, self.battery.label())));
        }

        let shell_data: Vec<Shell> = shells.iter().map(|carried| carried.shell.clone()).collect();
        let counted_param = ammo.and_then(|ammo| largest_shell(&shell_data, ammo)).map(|shell| shell.param.clone());
        let mut values = Vec::new();
        let mut counted = None;
        for carried in &shells {
            let shell = &carried.shell;
            let option = Some(carried.label(self.battery));
            let Some(reach) = firing_ranges(&planner, carried)? else {
                values.push(QuantityValue::new(
                    option,
                    None,
                    None,
                    Some("the selected modules list no firing range".to_owned()),
                ));
                continue;
            };
            if self.range.value() > reach.best.value() {
                values.push(QuantityValue::new(option, None, None, Some(reach.beyond_note(self.range, self.battery))));
                continue;
            }

            let build = (self.range.value() > reach.base.value()).then(|| reach.build_label());
            let value = match game.shell_impact(&shell.param, self.range)? {
                Some(impact) => {
                    let value = round2(self.quantity.value(&impact));
                    if counted_param.as_deref() == Some(shell.param.as_str()) {
                        counted = Some(value);
                    }
                    QuantityValue::new(option, build, Some(value), None)
                }
                None => QuantityValue::new(
                    option,
                    build,
                    None,
                    Some(format!("{} km is beyond the shell's ballistic reach", number(self.range.value()))),
                ),
            };
            values.push(value);
        }

        Ok(ShipEvaluation::new(values, None, counted))
    }

    fn ammo(&self) -> Option<Ammo> {
        if self.quantity == BallisticQuantity::RawPenetration { Some(Ammo::Ap) } else { self.ammo }
    }
}

/// A shell and the first module selection that carries it.
struct CarriedShell {
    shell: Shell,
    selection: usize,
    modules: Vec<String>,
}

impl CarriedShell {
    fn label(&self, battery: Battery) -> String {
        let base = format!("{} {}", battery.label(), self.shell.name);
        if self.modules.is_empty() { base } else { format!("{base} (modules: {})", self.modules.join(", ")) }
    }
}

/// A battery's firing range on the shell's base selection and on the build
/// that extends it most while retaining the shell.
#[derive(Debug, Clone)]
struct Reach {
    base: Km,
    best: Km,
    assumed: Vec<String>,
}

impl Reach {
    fn build_label(&self) -> String {
        if self.assumed.is_empty() {
            "best-fit range build".to_owned()
        } else {
            format!("with {}", self.assumed.join(", "))
        }
    }

    fn beyond_note(&self, range: Km, battery: Battery) -> String {
        format!(
            "{} km is beyond the {}'s firing range of {} km on its best-fit build ({} km on the shell's base modules)",
            number(range.value()),
            battery.label(),
            number(self.best.value()),
            number(self.base.value())
        )
    }
}

fn carried_shells(
    game: &GameData,
    planner: &Planner<'_>,
    battery: Battery,
    ammo: Option<Ammo>,
) -> Result<Vec<CarriedShell>, QueryError> {
    let available: Vec<Shell> = game
        .shells(planner.ship())
        .into_iter()
        .filter(|shell| shell.battery == battery && ammo.is_none_or(|ammo| shell.ammo == ammo))
        .collect();
    let mut carried = Vec::new();
    for (selection, _) in planner.selections().enumerate() {
        let outcome = planner.bare(selection)?;
        for shell in &available {
            if outcome_carries_shell(&outcome, battery, &shell.param)
                && !carried.iter().any(|carried: &CarriedShell| carried.shell.param == shell.param)
            {
                carried.push(CarriedShell {
                    shell: shell.clone(),
                    selection,
                    modules: planner.selection_modules(selection).to_vec(),
                });
            }
        }
    }

    Ok(carried)
}

fn outcome_carries_shell(outcome: &Outcome, battery: Battery, param: &str) -> bool {
    match battery {
        Battery::Main => outcome
            .stats
            .artillery
            .as_ref()
            .is_some_and(|artillery| artillery.shells.iter().any(|shell| shell.name == param)),
        Battery::Secondary => outcome
            .stats
            .secondaries
            .as_ref()
            .is_some_and(|secondaries| secondaries.mounts.iter().any(|mount| mount.shell.name == param)),
    }
}

fn firing_ranges(planner: &Planner<'_>, carried: &CarriedShell) -> Result<Option<Reach>, QueryError> {
    let battery = carried.shell.battery;
    let param = carried.shell.param.as_str();
    let extract = |outcome: &Outcome| {
        if !outcome_carries_shell(outcome, battery, param) {
            return None;
        }
        let range = match battery {
            Battery::Main => outcome.stats.artillery.as_ref()?.range?.value(),
            Battery::Secondary => outcome.stats.secondaries.as_ref()?.range.value(),
        };
        Some(Range(range))
    };
    let plan = StatPlan::new("firing range".to_owned(), Better::Higher, &extract).on_selection(carried.selection);
    let Some(builds) = planner.best(&plan)? else {
        return Ok(None);
    };
    let assumed = builds.best.assumed.iter().filter(|item| !carried.modules.contains(item)).cloned().collect();

    Ok(Some(Reach { base: Km::new(builds.base.0), best: Km::new(builds.best.value.0), assumed }))
}

pub(super) fn definitions() -> Vec<QuantityDefinition> {
    const PARAMETERS: [Parameter; 3] = [
        Parameter {
            name: "range_km",
            kind: ParameterType::Number { unit: "km" },
            required: true,
            default: None,
            description: "Distance to the target.",
        },
        Parameter {
            name: "ammo",
            kind: ParameterType::Choice { options: &["AP", "HE", "SAP"] },
            required: false,
            default: None,
            description: "Ammo type. Without it, every shell of the battery; a group needs it.",
        },
        Parameter {
            name: "battery",
            kind: ParameterType::Choice { options: &["main", "secondary"] },
            required: false,
            default: Some("main"),
            description: "Main or secondary battery.",
        },
    ];
    const SOURCES: [Source; 3] = [
        Source::GameParams(
            "Projectile.bulletSpeed, bulletAirDrag, bulletMass, bulletDiametr, bulletKrupp (shell ballistics)",
        ),
        Source::Computation(
            "wowsunpack::ballistics::solve_for_range: the shell trajectory with air drag and gravity, the launch angle that reaches the range, times on the in-game clock",
        ),
        Source::Computation("the battery's firing range on the ship's best-fit build (ship_stats)"),
    ];

    BallisticQuantity::ALL
        .into_iter()
        .map(|quantity| QuantityDefinition {
            name: quantity.name().to_owned(),
            unit: quantity.unit(),
            applies_to: AppliesTo::Shell,
            description: quantity.description().to_owned(),
            parameters: PARAMETERS.to_vec(),
            sources: SOURCES.to_vec(),
            evaluator: Evaluator::Ballistic(quantity),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wowsunpack::game_params::ttx::model::Artillery;
    use wowsunpack::game_params::ttx::model::ShellStats;
    use wowsunpack::game_params::ttx::model::ShipStats;
    use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
    use wowsunpack::game_params::types::Species;

    use super::*;

    fn shell(name: &str) -> ShellStats {
        ShellStats {
            name: name.to_owned(),
            ammo_kind: None,
            damage: None,
            caliber: None,
            speed: None,
            penetration: None,
            burn_chance: None,
            flood_chance: None,
            max_ammo: None,
            disabled_underwater: None,
        }
    }

    fn outcome(selection: usize, shell_name: &str) -> Outcome {
        Outcome {
            stats: ShipStats {
                artillery: Some(Artillery { shells: vec![shell(shell_name)], ..Artillery::default() }),
                ..ShipStats::default()
            },
            bundle: ModifierBundle::empty(Species::Cruiser),
            merges_fire_zones: false,
            selection,
        }
    }

    #[test]
    fn shell_reach_accepts_only_selections_that_carry_the_shell() {
        let stock = outcome(0, "stock_shell");
        let upgraded = outcome(1, "upgraded_shell");

        assert!(outcome_carries_shell(&stock, Battery::Main, "stock_shell"));
        assert!(!outcome_carries_shell(&stock, Battery::Main, "upgraded_shell"));
        assert!(outcome_carries_shell(&upgraded, Battery::Main, "upgraded_shell"));
    }

    #[test]
    fn raw_penetration_rejects_fixed_penetration_ammo() {
        let params = json!({"range_km": 12, "ammo": "HE"}).as_object().expect("parameters are an object").clone();
        let error = BallisticQuantity::RawPenetration.prepare(&params).err().expect("HE is rejected");

        assert!(error.to_string().contains("applies only to AP"));
    }
}
