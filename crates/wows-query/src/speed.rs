//! Top speed at each depth on the build players run.
//!
//! The game multiplies a ship's top speed by `speedCoef` everywhere, by
//! `speedCoefBattery` on the surface and at periscope depth, and by
//! `speedCoefUW` below periscope depth (`getShipSpeedCoef`,
//! ModifiersApply.py:531-546). Captain skills and upgrades come from the
//! build planner; signal flags carrying one of these modifiers are listed as
//! alternatives, as the port lets any ship mount them.

use serde::Serialize;
use wowsunpack::game_params::ttx::model::Knots;
use wowsunpack::game_params::types::GameParamProvider;

use crate::builds::Better;
use crate::builds::BuildAlternative;
use crate::builds::BuildStat;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::AbilityModifiers;

/// Top speed on the surface and, for submarines, at periscope depth and
/// submerged.
#[derive(Debug, Clone, Serialize)]
pub struct SpeedBuilds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<BuildStat<Knots>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub periscope_depth: Option<BuildStat<Knots>>,

    /// At the fastest of operating and maximum depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submerged: Option<BuildStat<Knots>>,
}

impl GameData {
    pub(crate) fn speed_builds(&self, planner: &Planner<'_>) -> Result<SpeedBuilds, QueryError> {
        let flagged: Vec<Flagged<'_>> = self
            .speed_signals()
            .into_iter()
            .map(|signal| {
                let mut abilities = planner.abilities().to_vec();
                abilities.push(signal.clone());
                Flagged { planner: planner.with_abilities_on(abilities), signal }
            })
            .collect();
        let knots = |value: Option<Knots>| value.map(|value| Knots::from(round(value.value())));

        let surface = |outcome: &Outcome| knots(outcome.stats.mobility.as_ref()?.speed);
        let periscope = |outcome: &Outcome| knots(outcome.stats.underwater_mobility.as_ref()?.periscope_speed);
        let submerged = |outcome: &Outcome| knots(outcome.stats.underwater_mobility.as_ref()?.speed);

        Ok(SpeedBuilds {
            surface: speed_plan(
                planner,
                &flagged,
                StatPlan::new("top speed on the surface".to_owned(), Better::Higher, &surface),
            )?,
            periscope_depth: speed_plan(
                planner,
                &flagged,
                StatPlan::new("top speed at periscope depth".to_owned(), Better::Higher, &periscope),
            )?,
            submerged: speed_plan(
                planner,
                &flagged,
                StatPlan::new("top speed submerged".to_owned(), Better::Higher, &submerged),
            )?,
        })
    }

    /// Signal flags whose modifiers change top speed.
    fn speed_signals(&self) -> Vec<AbilityModifiers> {
        const SPEED_MODIFIERS: [&str; 3] = ["speedCoef", "speedCoefBattery", "speedCoefUW"];

        self.provider()
            .params()
            .iter()
            .filter_map(|param| {
                let exterior = param.exterior().filter(|exterior| !exterior.hidden())?;
                if !exterior.modifiers().iter().any(|modifier| SPEED_MODIFIERS.contains(&modifier.name())) {
                    return None;
                }
                // Exteriors are named by param name, e.g. IDS_PCEF005_SM_SIGNALFLAG.
                let name = self.translate(&format!("IDS_{}", param.name().to_uppercase()))?;
                Some(AbilityModifiers {
                    source: param.name().to_owned(),
                    label: name,
                    modifiers: exterior.modifiers().to_vec(),
                })
            })
            .collect()
    }
}

/// The builds with one signal flag mounted.
struct Flagged<'a> {
    planner: Planner<'a>,
    signal: AbilityModifiers,
}

/// The builds of one speed, with each signal flag that raises the best-fit
/// value as an alternative, alone and together with each conditional skill
/// that raises it further. `None` when the ship lacks the speed.
fn speed_plan(
    planner: &Planner<'_>,
    flagged: &[Flagged<'_>],
    plan: StatPlan<'_, Knots>,
) -> Result<Option<BuildStat<Knots>>, QueryError> {
    let Some(mut stat) = planner.plan(&plan)? else {
        return Ok(None);
    };

    for Flagged { planner, signal } in flagged {
        let Some(with_signal) = planner.plan(&plan)? else {
            continue;
        };
        let best = with_signal.builds.best;
        if !plan.better.improves(best.value.value(), stat.builds.best.value.value()) {
            continue;
        }

        let flag = format!("{} signal", signal.label);
        let combined: Vec<BuildAlternative<Knots>> = with_signal
            .builds
            .alternatives
            .into_iter()
            .filter(|alternative| plan.better.improves(alternative.value.value(), best.value.value()))
            .map(|mut alternative| {
                alternative.label = format!("{} and {flag}", alternative.label);
                alternative.assumed.push(flag.clone());
                alternative
            })
            .collect();

        let mut assumed = best.assumed;
        assumed.push(flag.clone());
        stat.builds.alternatives.push(BuildAlternative {
            label: format!("with {flag}"),
            value: best.value,
            assumed,
            tier_fillers: best.tier_fillers,
            note: None,
        });
        stat.builds.alternatives.extend(combined);
    }

    Ok(Some(stat))
}
