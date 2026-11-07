//! The ship a battery fires at, and how its hull and build resist fire.

use serde::Serialize;
use wows_core::units::Percent;
use wows_ship_stats::FireResistance;

use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::round_to;
use crate::loadout::AppliedSkill;
use crate::loadout::AppliedUpgrade;
use crate::loadout::LoadoutRequest;
use crate::modules::Module;
use crate::names::Interpretations;
use crate::ships::ShipSummary;

/// The ship fired at and its build, as named in tool arguments.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TargetRequest<'a> {
    pub(crate) ship: &'a str,
    pub(crate) modules: &'a [String],
    pub(crate) skills: &'a [String],
    pub(crate) upgrades: &'a [String],
}

/// The target ship and how its hull and build resist fire.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FireTarget {
    pub ship: ShipSummary,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Module>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<AppliedSkill>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upgrades: Vec<AppliedUpgrade>,

    /// The hull's fire-node probability, lower at higher tiers.
    pub hull_fire_coefficient: f32,

    /// The build's `burnProb`: 0.9 with Fire Prevention Expert, 1 without
    /// anything that lowers it.
    pub build_fire_coefficient: f32,

    /// Hull times build coefficient: what a shell's fire chance is multiplied
    /// by against this ship.
    pub fire_chance_factor: f32,

    /// Fires that can burn at once.
    pub fire_zones: u32,

    #[serde(skip)]
    pub(crate) resistance: FireResistance,
}

impl FireTarget {
    /// Chance that one hit by a shell with `shell` fire chance starts a fire
    /// in a zone not already burning, capped at 100 percent.
    pub(crate) fn chance_against(&self, shell: Percent) -> Percent {
        self.resistance.chance_against(shell)
    }
}

impl GameData {
    /// The target's hull, stock unless `request` names modules, with its
    /// skills and upgrades applied.
    pub(crate) fn fire_target(
        &self,
        request: TargetRequest<'_>,
        interpretations: &mut Interpretations,
    ) -> Result<FireTarget, QueryError> {
        let ship = self.resolve_ship(request.ship, interpretations)?;
        let modules = self.resolve_modules(ship, request.modules)?;
        let loadout = self.resolve_loadout(
            ship,
            &LoadoutRequest {
                skills: request.skills,
                upgrades: request.upgrades,
                conditions_met: false,
                hp_percent: None,
                commander: None,
            },
            interpretations,
        )?;
        let effects = self.effective_modifiers(ship, &loadout)?;

        let missing = || QueryError::FireNodesMissing { ship: ship.name.clone() };
        let hull = ship
            .param
            .vehicle()
            .and_then(|vehicle| vehicle.ttx_components())
            .and_then(|components| components.hull(modules.selection.hull.as_deref()?))
            .ok_or_else(missing)?;
        let resistance = FireResistance::new(hull, effects.build.modifiers.bundle(), effects.build.merges_fire_zones)
            .ok_or_else(missing)?;

        Ok(FireTarget {
            ship: ship.summary(),
            modules: modules.chosen,
            skills: loadout.applied_skills,
            upgrades: loadout.applied_upgrades,
            hull_fire_coefficient: resistance.hull_coefficient,
            build_fire_coefficient: round_to(resistance.build_coefficient, 10_000.0),
            fire_chance_factor: round_to(resistance.chance_factor(), 10_000.0),
            fire_zones: resistance.zones,
            resistance,
        })
    }
}
