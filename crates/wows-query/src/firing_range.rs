//! Main battery and secondary firing range on the build players run.
//!
//! A consumable with an `artilleryDistCoeff`, such as Spotting Aircraft,
//! multiplies the main battery range after every modifier
//! (`artillery_range_km`, FactoryArtillery.py:42), so it is listed as an
//! alternative on top of the best-fit build.

use serde::Serialize;
use wows_core::units::Km;

use crate::builds::Better;
use crate::builds::BuildAlternative;
use crate::builds::BuildStat;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;

/// Firing range of the main battery and the secondary battery.
#[derive(Debug, Clone, Serialize)]
pub struct FiringRanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_battery: Option<BuildStat<Km>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<BuildStat<Km>>,
}

impl GameData {
    pub(crate) fn firing_ranges(&self, planner: &Planner<'_>) -> Result<FiringRanges, QueryError> {
        let secondary = |outcome: &Outcome| Some(Km::new(round(outcome.stats.secondaries.as_ref()?.range.value())));

        Ok(FiringRanges {
            main_battery: self.main_battery_range(planner)?,
            secondary: planner.plan(&StatPlan::new(
                "secondary battery range".to_owned(),
                Better::Higher,
                &secondary,
            ))?,
        })
    }

    /// The builds of main battery range, with each range consumable of the
    /// ship active as an alternative, alone and together with each
    /// conditional skill that raises the range further.
    fn main_battery_range(&self, planner: &Planner<'_>) -> Result<Option<BuildStat<Km>>, QueryError> {
        let range = |outcome: &Outcome| outcome.stats.artillery.as_ref()?.range.map(|range| range.value());
        let extract = |outcome: &Outcome| range(outcome).map(|range| Km::new(round(range)));
        let Some(mut stat) = planner.plan(&StatPlan::new("main battery range".to_owned(), Better::Higher, &extract))?
        else {
            return Ok(None);
        };

        for (name, coefficient) in self.range_consumables(planner.ship()) {
            let extract = |outcome: &Outcome| range(outcome).map(|range| Km::new(round(range * coefficient.0)));
            let active = StatPlan::new(format!("main battery range with {name} active"), Better::Higher, &extract);
            let Some(with_consumable) = planner.plan(&active)? else {
                continue;
            };

            let best = with_consumable.builds.best;
            let combined: Vec<BuildAlternative<Km>> = with_consumable
                .builds
                .alternatives
                .into_iter()
                .filter(|alternative| Better::Higher.improves(alternative.value.value(), best.value.value()))
                .map(|mut alternative| {
                    alternative.label = format!("{} and {name} active", alternative.label);
                    alternative.assumed.push(name.clone());
                    alternative
                })
                .collect();

            let mut assumed = best.assumed;
            assumed.push(name.clone());
            stat.builds.alternatives.push(BuildAlternative {
                label: format!("with {name} active"),
                value: best.value,
                assumed,
                tier_fillers: best.tier_fillers,
                note: None,
            });
            stat.builds.alternatives.extend(combined);
        }

        Ok(Some(stat))
    }
}
