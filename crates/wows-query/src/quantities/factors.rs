//! Every ship_comparison ship factor, valued per ship as ship_comparison
//! scores it before ranking.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;
use wows_core::units::Km;

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
use crate::armor_assessment::capitalize_first;
use crate::comparison::Factor;
use crate::comparison::default_reference_range;
use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Ship;
use crate::ships::ShipClass;

/// One factor.
#[derive(Debug, Clone)]
pub(super) struct FactorEvaluator {
    name: String,
    factor: Factor,
}

impl FactorEvaluator {
    pub(super) fn prepare(&self, params: &Map<String, Value>) -> Result<PreparedFactor, QueryError> {
        let params: FactorParams = read_params(&self.name, params)?;
        if params.reference_range_km.is_some_and(|range| !is_finite_positive(range.value())) {
            return Err(QueryError::InvalidArguments { reason: "reference_range_km must be finite and more than 0" });
        }

        Ok(PreparedFactor { factor: self.factor, reference_range: params.reference_range_km })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FactorParams {
    reference_range_km: Option<Km>,
}

pub(super) struct PreparedFactor {
    factor: Factor,

    /// `None`: each class's ship_comparison default.
    reference_range: Option<Km>,
}

impl PreparedFactor {
    pub(super) fn counts(&self) -> String {
        "each ship's value on its best-fit build for the factor, as ship_comparison scores it".to_owned()
    }

    /// Every ship at once: ship_comparison scores all factors of a ship
    /// together, ships of one reference range in parallel.
    pub(super) fn evaluate(&self, game: &GameData, ships: &[&Ship]) -> Vec<ShipEvaluation> {
        let range_of = |class: ShipClass| self.reference_range.unwrap_or_else(|| default_reference_range(class));
        let mut by_range: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (index, ship) in ships.iter().enumerate() {
            by_range.entry(range_of(ship.class).value().to_bits()).or_default().push(index);
        }

        let mut evaluations: Vec<Option<ShipEvaluation>> = ships.iter().map(|_| None).collect();
        for (range, indices) in by_range {
            let range = Km::new(f32::from_bits(range));
            let members: Vec<&Ship> = indices.iter().map(|index| ships[*index]).collect();
            match game.factor_values(&members, range, self.factor.per_tier()) {
                Ok(values) => {
                    for (index, values) in indices.iter().zip(values) {
                        evaluations[*index] = Some(self.reading(values.get(&self.factor).copied(), range));
                    }
                }
                Err(error) => {
                    for index in indices {
                        evaluations[index] = Some(ShipEvaluation::failed(&error));
                    }
                }
            }
        }

        evaluations.into_iter().map(|evaluation| evaluation.expect("every ship has a range")).collect()
    }

    fn reading(&self, value: Option<f32>, range: Km) -> ShipEvaluation {
        let note = self.factor.uses_reference_range().then(|| format!("at a reference range of {} km", range.value()));
        let Some(value) = value else {
            return ShipEvaluation::absent(
                "no value: the ship lacks what the factor measures, or the reference range is beyond its reach"
                    .to_owned(),
            );
        };

        ShipEvaluation::new(
            vec![QuantityValue::new(None, Some("best-fit".to_owned()), Some(value), note)],
            None,
            Some(value),
        )
    }
}

pub(super) fn definitions() -> Vec<QuantityDefinition> {
    const REFERENCE_RANGE: Parameter = Parameter {
        name: "reference_range_km",
        kind: ParameterType::Number { unit: "km" },
        required: false,
        default: Some("15 for battleships, 12 for cruisers, 10 for other classes"),
        description: "Range at which dispersion and shell flight time are measured.",
    };

    Factor::all()
        .into_iter()
        .map(|factor| {
            let meta = factor.meta();
            let name = format!("comparison.{}", meta.key);
            let protection = if factor.per_tier() { " Cruisers and battleships only." } else { "" };

            QuantityDefinition {
                evaluator: Evaluator::Factor(FactorEvaluator { name: name.clone(), factor }),
                name,
                unit: meta.unit,
                applies_to: AppliesTo::Ship,
                description: format!(
                    "{}, on the ship's best-fit build. A ship_comparison factor, without the ranking.{protection}",
                    capitalize_first(meta.description)
                ),
                parameters: if factor.uses_reference_range() { vec![REFERENCE_RANGE] } else { Vec::new() },
                sources: vec![Source::Computation("ship_comparison per-ship scoring")],
            }
        })
        .collect()
}
