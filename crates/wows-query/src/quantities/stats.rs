//! Every stat stat_search can search, valued per ship on the build
//! stat_search ranks it by.

use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;

use super::AppliesTo;
use super::Evaluator;
use super::Parameter;
use super::ParameterType;
use super::QuantityDefinition;
use super::QuantityValue;
use super::ShipEvaluation;
use super::Source;
use super::read_params;
use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Ship;
use crate::stat_search::RankBuild;
use crate::stat_search::StatQuantity;
use crate::stat_search::StatReading;
use crate::stat_search::stat_definitions;

/// A stat by its stat_search name.
#[derive(Debug, Clone)]
pub(super) struct StatEvaluator {
    name: String,
}

impl StatEvaluator {
    pub(super) fn prepare(&self, params: &Map<String, Value>) -> Result<PreparedStat, QueryError> {
        let params: StatParams = read_params(&self.name, params)?;
        let build = match params.build {
            None | Some(StatBuild::Best) => None,
            Some(StatBuild::Base) => Some(RankBuild::Base),
            Some(StatBuild::Lowest) => Some(RankBuild::Lowest),
            Some(StatBuild::Highest) => Some(RankBuild::Highest),
        };

        Ok(PreparedStat { quantity: StatQuantity::new(&self.name, params.qualifier, build)? })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatParams {
    qualifier: Option<String>,
    build: Option<StatBuild>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StatBuild {
    Base,
    Best,
    Lowest,
    Highest,
}

pub(super) struct PreparedStat {
    quantity: StatQuantity,
}

impl PreparedStat {
    pub(super) fn counts(&self) -> String {
        format!("each ship's value on the {}", self.quantity.build_label())
    }

    pub(super) fn evaluate(&self, game: &GameData, ship: &Ship) -> Result<ShipEvaluation, QueryError> {
        let Some(reading) = game.stat_reading(ship, &self.quantity)? else {
            return Ok(ShipEvaluation::absent(format!("{} lacks this stat", ship.name)));
        };

        let counted = reading.value;
        let values = quantity_values(reading);

        Ok(ShipEvaluation::new(values, None, Some(counted)))
    }
}

fn quantity_values(reading: StatReading) -> Vec<QuantityValue> {
    let mut values = vec![QuantityValue::new(reading.base_row, Some("base".to_owned()), Some(reading.base), None)];
    if !reading.build.is_empty() && reading.value != reading.base {
        values.push(QuantityValue::new(
            reading.row,
            Some(format!("with {}", reading.build.join(", "))),
            Some(reading.value),
            None,
        ));
    }

    values
}

pub(super) fn definitions(game: &GameData) -> Vec<QuantityDefinition> {
    const PARAMETERS: [Parameter; 2] = [
        Parameter {
            name: "qualifier",
            kind: ParameterType::Text,
            required: false,
            default: None,
            description: "Only the row with this qualifier: ammo type for shells (\"HE\", \"AP\", \"SAP\"), torpedo name, secondary mount or consumable name. Without it, the row the build moves furthest.",
        },
        Parameter {
            name: "build",
            kind: ParameterType::Choice { options: &["base", "best", "lowest", "highest"] },
            required: false,
            default: Some("best"),
            description: "base: stock modules, no captain skills or upgrades. best: the build players take for the stat (lower detection and reload, higher damage and range); the base value for a stat no build improves. lowest or highest: the build that moves the stat furthest that way.",
        },
    ];

    stat_definitions(game)
        .into_iter()
        .map(|stat| {
            let rows = stat.item.map(|item| format!("; one row {item}")).unwrap_or_default();
            let builds = if stat.better.is_some() { "" } else { "; no build improves it" };

            QuantityDefinition {
                evaluator: Evaluator::Stat(StatEvaluator { name: stat.name.clone() }),
                name: stat.name,
                unit: stat.unit,
                applies_to: AppliesTo::Ship,
                description: format!("{}{rows}{builds}. A stat_search stat.", stat.label),
                parameters: PARAMETERS.to_vec(),
                sources: vec![Source::Computation(
                    "stat_search, from the ship's stat card as the port's TTX factories compute it, on the builds stat_search ranks by",
                )],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_row_values_keep_their_own_labels() {
        let values = quantity_values(StatReading {
            value: 12.0,
            base: 10.0,
            build: vec!["range upgrade".to_owned()],
            row: Some("long-range row".to_owned()),
            base_row: Some("stock row".to_owned()),
        });

        assert_eq!(values[0].option.as_deref(), Some("stock row"));
        assert_eq!(values[0].value, Some(10.0));
        assert_eq!(values[1].option.as_deref(), Some("long-range row"));
        assert_eq!(values[1].value, Some(12.0));
    }
}
