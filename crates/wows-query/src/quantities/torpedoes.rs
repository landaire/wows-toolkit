//! Torpedo quantities per torpedo option: travel time to a distance, damage
//! at a distance, and detectability by ships.

use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;
use wows_core::units::Km;
use wowsunpack::game_params::ttx::constants::KM_TO_M;
use wowsunpack::game_params::ttx::constants::ballistic_speed;
use wowsunpack::game_params::ttx::model::Knots;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::Projectile;

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
use crate::builds::Better;
use crate::builds::Builds;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::comparison::number;
use crate::comparison::round2;
use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::torpedoes::CarriedTorpedo;
use crate::torpedoes::arming_distance;
use crate::torpedoes::damage_share;
use crate::torpedoes::describe_falloff;
use crate::torpedoes::falloff_points;
use crate::torpedoes::torpedo_options;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TorpedoQuantity {
    TravelTime,
    DamageAtDistance,
    Detectability,
}

impl TorpedoQuantity {
    const ALL: [Self; 3] = [Self::TravelTime, Self::DamageAtDistance, Self::Detectability];

    fn name(self) -> &'static str {
        match self {
            Self::TravelTime => "torpedo.travel_time",
            Self::DamageAtDistance => "torpedo.damage_at_distance",
            Self::Detectability => "torpedo.detectability",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::TravelTime => "s",
            Self::DamageAtDistance => "HP",
            Self::Detectability => "km",
        }
    }

    fn better(self) -> Better {
        match self {
            Self::TravelTime | Self::Detectability => Better::Lower,
            Self::DamageAtDistance => Better::Higher,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::TravelTime => {
                "Torpedo travel time to a distance: how long a torpedo takes to reach a target, run time to impact, at the torpedo's speed in game knots (2.6854 m/s per game knot on the map, not 0.5144 m/s per nautical knot), on the base build and on the best-fit build for speed. No value past the torpedo's range."
            }
            Self::DamageAtDistance => {
                "Torpedo damage of one hit after running a distance: full damage reduced by the torpedo's damage falloff with distance (distanceOfDamage, counted after the arming run), on the base build and the best-fit build for damage. No value before the torpedo arms or past its range."
            }
            Self::Detectability => {
                "Torpedo detectability: the distance at which enemy ships spot (see, detect) a torpedo, on the base build and the best-fit build of the launching ship. With spotter_class, the spotting ship's captain has Vigilance, which multiplies that distance. Hydroacoustic Search spots torpedoes within its own torpedo range; radar does not spot torpedoes."
            }
        }
    }

    fn sources(self) -> Vec<Source> {
        match self {
            Self::TravelTime => vec![
                Source::GameParams("Projectile.speed (game knots), Projectile.maxDist"),
                Source::ClientScript(
                    "TorpedoesCommon/m41dd1280.py recalcTorpedoParams (speed * BW_KNOTS_TO_MPS) and m63fc2621.py BW_KNOTS_TO_MPS",
                ),
                Source::Computation(
                    "game knots x 2.6854 ballistic m/s per knot (BW_KNOTS_TO_MPS x 30 m per BigWorld unit)",
                ),
            ],
            Self::DamageAtDistance => vec![
                Source::GameParams("Projectile.alphaDamage, damage, distanceOfDamage, maneuverDist, armingTime"),
                Source::ClientScript(
                    "mab132439/ttx/FactoryTorpedoes.py (arming run and distanceOfMaxDamage) and TorpedoesCommon/m41dd1280.py recalcTorpedoParams",
                ),
                Source::Computation("linear interpolation between distanceOfDamage points"),
            ],
            Self::Detectability => vec![
                Source::GameParams(
                    "Projectile.visibilityFactor, Crew skill DetectionTorpedoRange (Vigilance) torpedoDetectionCoefficient",
                ),
                Source::ClientScript(
                    "Vehicle.py onSonarDistanceChanged (torpedo distance x torpedoDetectionCoefficient)",
                ),
            ],
        }
    }

    fn parameters(self) -> Vec<Parameter> {
        const DISTANCE: Parameter = Parameter {
            name: "distance_km",
            kind: ParameterType::Number { unit: "km" },
            required: true,
            default: None,
            description: "Distance the torpedo runs from launch to the target.",
        };
        const OPTION: Parameter = Parameter {
            name: "option",
            kind: ParameterType::Choice { options: &["stock", "best"] },
            required: false,
            default: Some("stock"),
            description: "For a group: count each ship's torpedo on stock modules, or its best torpedo option for this quantity.",
        };
        const SPOTTER: Parameter = Parameter {
            name: "spotter_class",
            kind: ParameterType::ShipClass,
            required: false,
            default: None,
            description: "Class of the enemy ship spotting the torpedo, whose captain has Vigilance. Omit for a spotter without Vigilance.",
        };

        match self {
            Self::TravelTime | Self::DamageAtDistance => vec![DISTANCE, OPTION],
            Self::Detectability => vec![SPOTTER, OPTION],
        }
    }

    pub(super) fn prepare(&self, game: &GameData, params: &Map<String, Value>) -> Result<PreparedTorpedo, QueryError> {
        let kind = match self {
            Self::TravelTime | Self::DamageAtDistance => {
                let params: DistanceParams = read_params(self.name(), params)?;
                if !is_finite_positive(params.distance_km.value()) {
                    return Err(QueryError::InvalidArguments { reason: "distance_km must be finite and more than 0" });
                }
                let distance = params.distance_km;
                let kind = match self {
                    Self::TravelTime => PreparedKind::TravelTime { distance },
                    _ => PreparedKind::Damage { distance },
                };
                (kind, params.option)
            }
            Self::Detectability => {
                let params: DetectabilityParams = read_params(self.name(), params)?;
                let vigilance = match params.spotter_class {
                    Some(class) => Some(vigilance(game, class)?),
                    None => None,
                };
                (PreparedKind::Detectability { vigilance }, params.option)
            }
        };

        let (kind, option) = kind;
        Ok(PreparedTorpedo { quantity: *self, kind, option: option.unwrap_or(GroupOption::Stock) })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DistanceParams {
    distance_km: Km,
    option: Option<GroupOption>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectabilityParams {
    spotter_class: Option<ShipClass>,
    option: Option<GroupOption>,
}

/// Which torpedo option of a ship a group counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GroupOption {
    /// The torpedo the stock modules carry, or the first module combination's.
    Stock,

    /// The option with the best value of the quantity.
    Best,
}

/// Vigilance on the spotting ship.
#[derive(Debug, Clone, Copy)]
struct Vigilance {
    class: ShipClass,
    coefficient: f32,
}

enum PreparedKind {
    TravelTime { distance: Km },
    Damage { distance: Km },
    Detectability { vigilance: Option<Vigilance> },
}

pub(super) struct PreparedTorpedo {
    quantity: TorpedoQuantity,
    kind: PreparedKind,
    option: GroupOption,
}

/// Card values of one torpedo under one build, ordered by `value`.
#[derive(Debug, Clone, Copy)]
struct Card {
    value: f32,
    speed: Knots,
    range: Km,
    damage: Option<f32>,
    visibility: Option<Km>,
}

impl StatAmount for Card {
    fn amount(self) -> f32 {
        self.value
    }
}

impl PreparedTorpedo {
    pub(super) fn counts(&self) -> String {
        let option = match self.option {
            GroupOption::Stock => "the torpedo of each ship's stock modules (else of the first modules that carry one)",
            GroupOption::Best => "each ship's best torpedo option for this quantity",
        };

        format!("{option}, on its best-fit build")
    }

    pub(super) fn evaluate(&self, game: &GameData, ship: &Ship) -> Result<ShipEvaluation, QueryError> {
        let planner = Planner::new(game, ship);
        let options = torpedo_options(&planner)?;
        if options.is_empty() {
            return Ok(ShipEvaluation::absent(format!("{} carries no torpedoes", ship.name)));
        }

        let mut values = Vec::new();
        let mut counted: Option<f32> = None;
        for (index, option) in options.iter().enumerate() {
            let projectile = match self.kind {
                PreparedKind::Damage { .. } => Some(game.projectile(&option.param)?),
                PreparedKind::TravelTime { .. } | PreparedKind::Detectability { .. } => None,
            };
            let label = option_label(game, option);
            let Some(builds) = self.builds(&planner, option, projectile.as_ref())? else {
                let outcome = planner.bare(option.selection)?;
                let reading = match self.card(&outcome, option, projectile.as_ref(), false) {
                    Some(card) => self.reading(projectile.as_ref(), &card),
                    None => Reading { value: None, note: Some("no card values".to_owned()) },
                };
                values.push(QuantityValue::new(Some(label), Some("base".to_owned()), reading.value, reading.note));
                continue;
            };

            let base = self.reading(projectile.as_ref(), &builds.base);
            let best = self.reading(projectile.as_ref(), &builds.best.value);
            // The option's own modules are part of its base.
            let best_build: Vec<String> =
                builds.best.assumed.iter().filter(|item| !option.modules.contains(item)).cloned().collect();

            values.push(QuantityValue::new(
                Some(label.clone()),
                Some("base".to_owned()),
                base.value,
                base.note.clone(),
            ));
            if !best_build.is_empty() && best.value != base.value {
                values.push(QuantityValue::new(
                    Some(label),
                    Some(format!("with {}", best_build.join(", "))),
                    best.value,
                    best.note,
                ));
            }

            let candidate = best.value;
            counted = match self.option {
                GroupOption::Stock if index == 0 => candidate,
                GroupOption::Stock => counted,
                GroupOption::Best => match (counted, candidate) {
                    (Some(kept), Some(value)) if self.quantity.better().improves(value, kept) => Some(value),
                    (None, value) => value,
                    (kept, _) => kept,
                },
            };
        }

        let note = match &self.kind {
            PreparedKind::Detectability { vigilance: Some(vigilance) } => Some(format!(
                "with Vigilance on the spotting {}: x{} (torpedoDetectionCoefficient)",
                vigilance.class.label().replace('_', " "),
                number(vigilance.coefficient)
            )),
            PreparedKind::Detectability { vigilance: None } => {
                Some("spotter without Vigilance; give spotter_class for one with it".to_owned())
            }
            PreparedKind::TravelTime { .. } | PreparedKind::Damage { .. } => None,
        };

        Ok(ShipEvaluation::new(values, note, counted))
    }

    /// Base and best-fit card of `option` for this quantity.
    fn builds(
        &self,
        planner: &Planner<'_>,
        option: &CarriedTorpedo,
        projectile: Option<&Projectile>,
    ) -> Result<Option<Builds<Card>>, QueryError> {
        let card = |outcome: &Outcome| self.card(outcome, option, projectile, true);
        // Travel time falls as speed rises: the plan raises speed.
        let better = match self.quantity {
            TorpedoQuantity::TravelTime => Better::Higher,
            quantity => quantity.better(),
        };
        let plan = StatPlan::new(self.quantity.name().to_owned(), better, &card).on_selection(option.selection);

        planner.best(&plan)
    }

    fn card(
        &self,
        outcome: &Outcome,
        option: &CarriedTorpedo,
        projectile: Option<&Projectile>,
        validate_distance: bool,
    ) -> Option<Card> {
        let torpedo =
            outcome.stats.torpedoes.as_ref()?.torpedoes.iter().find(|torpedo| torpedo.name == option.param)?;
        let speed = torpedo.speed?;
        let range = torpedo.range?;
        let damage = torpedo.damage.map(|damage| damage.value());
        let visibility = torpedo.visibility;
        let value = match self.kind {
            PreparedKind::TravelTime { distance } => {
                if validate_distance && !requested_distance_is_valid(distance, range, None) {
                    return None;
                }

                speed.value()
            }
            PreparedKind::Damage { distance } => {
                let projectile = projectile?;
                let armed = arming_distance(projectile, speed);
                if validate_distance && !requested_distance_is_valid(distance, range, armed) {
                    return None;
                }
                let share =
                    falloff_points(projectile, speed).and_then(|points| damage_share(&points, distance)).unwrap_or(1.0);
                damage? * share
            }
            PreparedKind::Detectability { .. } => visibility?.value(),
        };

        Some(Card { value, speed, range, damage, visibility })
    }

    /// The quantity on one build's card.
    fn reading(&self, projectile: Option<&Projectile>, card: &Card) -> Reading {
        let beyond = |distance: Km| {
            (distance.value() > card.range.value()).then(|| {
                format!(
                    "{} km is beyond the torpedo's {} km range on this build",
                    number(distance.value()),
                    number(card.range.value())
                )
            })
        };

        match self.kind {
            PreparedKind::TravelTime { distance } => match beyond(distance) {
                Some(note) => Reading { value: None, note: Some(note) },
                None => {
                    let speed = ballistic_speed(card.speed);
                    Reading {
                        value: Some(round2(distance.value() * KM_TO_M / speed.value())),
                        note: Some(format!("{} kn = {} m/s", number(card.speed.value()), number(speed.value()))),
                    }
                }
            },
            PreparedKind::Damage { distance } => {
                let Some(projectile) = projectile else {
                    return Reading { value: None, note: Some("projectile data missing".to_owned()) };
                };
                let armed = arming_distance(projectile, card.speed);
                match (beyond(distance), armed) {
                    (Some(note), _) => Reading { value: None, note: Some(note) },
                    (None, Some(armed)) if distance.value() < armed.value() => Reading {
                        value: None,
                        note: Some(format!(
                            "the torpedo arms after {} km; before that it does not explode",
                            number(armed.value())
                        )),
                    },
                    (None, _) => {
                        let full = card.damage.map(|damage| number(damage.round()));
                        let falloff = match falloff_points(projectile, card.speed) {
                            Some(points) => describe_falloff(&points),
                            None => "full damage at every distance (no distanceOfDamage)".to_owned(),
                        };
                        let full = full.map(|full| format!("full damage {full} HP; ")).unwrap_or_default();
                        Reading { value: Some(card.value.round()), note: Some(format!("{full}{falloff}")) }
                    }
                }
            }
            PreparedKind::Detectability { vigilance } => {
                let coefficient = vigilance.map_or(1.0, |vigilance| vigilance.coefficient);
                Reading {
                    value: card.visibility.map(|visibility| round2(visibility.value() * coefficient)),
                    note: None,
                }
            }
        }
    }
}

/// One build's value with what it assumes.
struct Reading {
    value: Option<f32>,
    note: Option<String>,
}

fn requested_distance_is_valid(distance: Km, range: Km, armed: Option<Km>) -> bool {
    distance.value() <= range.value() && armed.is_none_or(|armed| distance.value() >= armed.value())
}
/// "Type 93 mod. 3 (stock)" or "Type F3 (modules: Type F3)".
fn option_label(game: &GameData, option: &CarriedTorpedo) -> String {
    let name = game.projectile_name(&option.param);
    if option.modules.is_empty() {
        format!("{name} (stock)")
    } else {
        format!("{name} (modules: {})", option.modules.join(", "))
    }
}

/// Vigilance's torpedo detection multiplier for a spotting ship of `class`,
/// from its captain skill entry.
fn vigilance(game: &GameData, class: ShipClass) -> Result<Vigilance, QueryError> {
    /// Internal name of Vigilance.
    const VIGILANCE_SKILL: &str = "DetectionTorpedoRange";
    /// The modifier through which Vigilance extends torpedo detection
    /// (`Vehicle.py` multiplies the torpedo detection distance by it).
    const TORPEDO_DETECTION_COEFFICIENT: &str = "torpedoDetectionCoefficient";

    let skill: Option<&CrewSkill> = game.skills.iter().find(|skill| skill.internal_name().as_str() == VIGILANCE_SKILL);
    let coefficient = skill
        .filter(|skill| game.skill_points(skill, class).is_some())
        .and_then(|skill| skill.modifiers()?.iter().find(|modifier| modifier.name() == TORPEDO_DETECTION_COEFFICIENT))
        .map(|modifier| modifier.get_for_species(&class.species()));

    match coefficient {
        Some(coefficient) => Ok(Vigilance { class, coefficient }),
        None => {
            let classes = ShipClass::ALL
                .into_iter()
                .filter(|other| skill.is_some_and(|skill| game.skill_points(skill, *other).is_some()))
                .collect();
            Err(QueryError::VigilanceNotForClass { class, classes })
        }
    }
}

pub(super) fn definitions() -> Vec<QuantityDefinition> {
    TorpedoQuantity::ALL
        .into_iter()
        .map(|quantity| QuantityDefinition {
            name: quantity.name().to_owned(),
            unit: quantity.unit(),
            applies_to: AppliesTo::TorpedoOption,
            description: quantity.description().to_owned(),
            parameters: quantity.parameters(),
            sources: quantity.sources(),
            evaluator: Evaluator::Torpedo(quantity),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_distance_rejects_unarmed_and_out_of_range_candidates() {
        let range = Km::new(10.0);
        let armed = Km::new(0.5);

        assert!(!requested_distance_is_valid(Km::new(0.49), range, Some(armed)));
        assert!(requested_distance_is_valid(Km::new(0.5), range, Some(armed)));
        assert!(requested_distance_is_valid(Km::new(10.0), range, Some(armed)));
        assert!(!requested_distance_is_valid(Km::new(10.01), range, Some(armed)));
    }
}
