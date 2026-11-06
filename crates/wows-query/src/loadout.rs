use std::collections::HashMap;

use serde::Serialize;
use wows_core::units::Percent;
use wowsunpack::game_params::ttx::EffectActivation;
use wowsunpack::game_params::ttx::EffectKind;
use wowsunpack::game_params::ttx::EffectiveModifiers;
use wowsunpack::game_params::ttx::EffectsState;
use wowsunpack::game_params::ttx::HealthFraction;
use wowsunpack::game_params::ttx::Loadout;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::CrewSkillModifier;

use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Ship;

/// What a player chose for a ship, as named in tool arguments.
#[derive(Debug, Clone, Default)]
pub(crate) struct LoadoutRequest<'a> {
    pub(crate) skills: &'a [String],
    pub(crate) upgrades: &'a [String],

    /// Treat conditional skill triggers as met.
    pub(crate) conditions_met: bool,

    /// Remaining health for health-scaled effects, 0 to 100.
    pub(crate) hp_percent: Option<Percent>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppliedSkill {
    pub name: String,
    pub points: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppliedUpgrade {
    pub name: String,

    /// 1-based slot.
    pub slot: u8,
}

/// A loadout resolved against one ship.
pub(crate) struct ResolvedLoadout {
    pub(crate) skills: Vec<CrewSkill>,
    pub(crate) applied_skills: Vec<AppliedSkill>,
    pub(crate) modernizations: Vec<(String, Vec<CrewSkillModifier>)>,
    pub(crate) applied_upgrades: Vec<AppliedUpgrade>,
    pub(crate) conditions_met: bool,
    pub(crate) hp_percent: Option<Percent>,

    /// Skill internal names and upgrade param names to English names, for
    /// relabeling provenance.
    pub(crate) labels: HashMap<String, String>,
}

impl ResolvedLoadout {
    pub(crate) fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.modernizations.is_empty()
    }

    pub(crate) fn skill_points(&self) -> u32 {
        self.applied_skills.iter().map(|skill| u32::from(skill.points)).sum()
    }
}

/// Effective modifiers plus the conditional effects that were switched on.
pub(crate) struct ResolvedEffects {
    pub(crate) modifiers: EffectiveModifiers,
    pub(crate) active_conditions: Vec<String>,
}

impl GameData {
    pub(crate) fn resolve_loadout(
        &self,
        ship: &Ship,
        request: &LoadoutRequest<'_>,
    ) -> Result<ResolvedLoadout, QueryError> {
        let mut skills: Vec<CrewSkill> = Vec::new();
        let mut applied_skills = Vec::new();
        let mut labels = HashMap::new();
        for query in request.skills {
            let (skill, points) = self.resolve_skill_for_class(query, ship.class)?;
            if skills.iter().any(|chosen| chosen.internal_name() == skill.internal_name()) {
                continue;
            }
            let name = self.skill_name(skill);
            labels.insert(skill.internal_name().as_str().to_owned(), name.clone());
            applied_skills.push(AppliedSkill { name, points });
            skills.push(skill.clone());
        }

        let mut chosen: Vec<&crate::upgrades::Upgrade> = Vec::new();
        for query in request.upgrades {
            let upgrade = self.resolve_upgrade_for_ship(query, ship)?;
            if chosen.iter().any(|other| other.param.id() == upgrade.param.id()) {
                continue;
            }
            if let Some(other) = chosen.iter().find(|other| other.slot == upgrade.slot) {
                return Err(QueryError::UpgradeSlotConflict {
                    first: other.name.clone(),
                    second: upgrade.name.clone(),
                    slot: upgrade.port_slot(),
                });
            }
            chosen.push(upgrade);
        }
        let modernizations = chosen
            .iter()
            .map(|upgrade| (upgrade.param.name().to_owned(), upgrade.modernization().modifiers().to_vec()))
            .collect();
        let applied_upgrades = chosen
            .iter()
            .map(|upgrade| AppliedUpgrade { name: upgrade.name.clone(), slot: upgrade.port_slot() })
            .collect();
        for upgrade in &chosen {
            labels.insert(upgrade.param.name().to_owned(), upgrade.name.clone());
        }

        Ok(ResolvedLoadout {
            skills,
            applied_skills,
            modernizations,
            applied_upgrades,
            conditions_met: request.conditions_met,
            hp_percent: request.hp_percent,
            labels,
        })
    }

    /// Folds the loadout into the modifiers the stat factories apply.
    pub(crate) fn effective_modifiers(
        &self,
        ship: &Ship,
        loadout: &ResolvedLoadout,
    ) -> Result<ResolvedEffects, QueryError> {
        let version = self.version();
        let effects = Loadout { skills: &loadout.skills, modernizations: &loadout.modernizations, ship: &ship.param }
            .effects(self.provider(), version);

        let mut state = EffectsState::default();
        let mut active_conditions = Vec::new();
        for effect in effects.iter() {
            let activation = match effect.kind() {
                EffectKind::Binary { .. } if loadout.conditions_met => Some(EffectActivation::On),
                EffectKind::StackingPerCount { .. } if loadout.conditions_met => {
                    Some(EffectActivation::Stacks(u32::MAX))
                }
                EffectKind::Heat { heat_interpolator } if loadout.conditions_met => {
                    Some(EffectActivation::Heat(heat_interpolator.max_x()))
                }
                EffectKind::HealthScaledReload | EffectKind::InnateAdrenaline { .. } => loadout
                    .hp_percent
                    .map(|percent| EffectActivation::Health(HealthFraction::new(percent.to_fraction()))),
                _ => None,
            };
            let Some(activation) = activation else {
                continue;
            };

            if let wowsunpack::game_params::ttx::EffectId::Skill(name) = effect.id()
                && matches!(
                    effect.kind(),
                    EffectKind::Binary { .. } | EffectKind::StackingPerCount { .. } | EffectKind::Heat { .. }
                )
            {
                active_conditions
                    .push(loadout.labels.get(name.as_str()).cloned().unwrap_or_else(|| name.as_str().to_owned()));
            }
            state = state.set(effect.id().clone(), activation);
        }

        let modifiers = effects
            .resolve(&state, ship.class.species(), version)
            .map_err(|source| QueryError::Modifiers { ship: ship.name.clone(), source })?;

        Ok(ResolvedEffects { modifiers, active_conditions })
    }

    /// The same path with nothing chosen, for diffs against the stock card.
    pub(crate) fn stock_loadout(&self) -> ResolvedLoadout {
        ResolvedLoadout {
            skills: Vec::new(),
            applied_skills: Vec::new(),
            modernizations: Vec::new(),
            applied_upgrades: Vec::new(),
            conditions_met: false,
            hp_percent: None,
            labels: HashMap::new(),
        }
    }
}
