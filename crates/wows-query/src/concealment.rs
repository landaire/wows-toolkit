//! Detectability of one ship on its best-fit build.

use serde::Serialize;
use wows_core::units::Km;

use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::ships::ShipArgs;
use crate::ships::ShipSummary;

/// How the client computes detectability after firing in smoke.
const IN_SMOKE_RULE: &str = "Detectability after firing the main battery from smoke is the hull's visibilityCoefGKInSmoke (plus visibilityFactorInSmoke, near zero), capped at the gun range. The client builds it without the ship's modifiers (CommonBattleLogicClasses/EntityVisibility.py EntityVisibilityBySmoke: constructed from params alone, and _calculateDistance ignores the modifier), unlike surface detectability, which multiplies visibilityDistCoeff. The port card shows the same hull value (ttx/FactoryVisibility.py). Concealment Expert and the concealment upgrades change only visibilityDistCoeff, so they do not change it.";

#[derive(Debug, Clone, Serialize)]
pub struct ShipConcealment {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    /// Detectability by ships.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<BuildStat<Km>>,

    /// Detectability by aircraft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub air: Option<BuildStat<Km>>,

    /// Detectability after firing the main battery in open water.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_main_battery: Option<BuildStat<Km>>,

    /// Detectability after firing the main battery from inside smoke.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_smoke_after_firing: Option<BuildStat<Km>>,

    /// Why concealment skills and upgrades do not change the in-smoke value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_smoke_evidence: Option<InSmokeEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InSmokeEvidence {
    pub rule: &'static str,

    /// Skills and upgrades of the best surface build, each with the surface
    /// and in-smoke values it gives on its own.
    pub checked: Vec<CheckedItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckedItem {
    pub name: String,
    pub surface: Km,
    pub in_smoke_after_firing: Km,
}

impl GameData {
    /// Surface, air, after-firing and in-smoke detectability on the best-fit build.
    pub fn ship_concealment(&self, args: &ShipArgs) -> Result<ShipConcealment, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let planner = Planner::new(self, ship);

        let km = |value: Option<wowsunpack::game_params::types::Km>| value.map(|value| Km::new(round(value.value())));
        let surface_of = |outcome: &Outcome| km(outcome.stats.visibility.as_ref()?.sea_detection);
        let smoke_of = |outcome: &Outcome| km(outcome.stats.visibility.as_ref()?.detection_in_smoke);

        let surface = planner.plan(&StatPlan::new("detectability by ships".to_owned(), Better::Lower, &surface_of))?;
        let air = planner.plan(&StatPlan::new("detectability by aircraft".to_owned(), Better::Lower, &|outcome| {
            km(outcome.stats.visibility.as_ref()?.air_detection)
        }))?;
        let after_main_battery = planner.plan(&StatPlan::new(
            "detectability after firing the main battery".to_owned(),
            Better::Lower,
            &|outcome| km(outcome.stats.visibility.as_ref()?.main_gun_range_detection),
        ))?;
        let in_smoke_after_firing = planner.plan(&StatPlan::new(
            "detectability after firing the main battery in smoke".to_owned(),
            Better::Lower,
            &smoke_of,
        ))?;

        let in_smoke_evidence = match (&surface, &in_smoke_after_firing) {
            (Some(surface), Some(_)) => {
                let mut checked = Vec::new();
                for item in planner.items_named(&surface.builds.best.assumed) {
                    let outcome = planner.with_items(&[item])?;
                    if let (Some(surface), Some(smoke)) = (surface_of(&outcome), smoke_of(&outcome)) {
                        checked.push(CheckedItem { name: planner.name(item), surface, in_smoke_after_firing: smoke });
                    }
                }
                Some(InSmokeEvidence { rule: IN_SMOKE_RULE, checked })
            }
            _ => None,
        };

        Ok(ShipConcealment {
            interpretations,
            ship: ship.summary(),
            surface,
            air,
            after_main_battery,
            in_smoke_after_firing,
            in_smoke_evidence,
        })
    }
}
