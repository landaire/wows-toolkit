use std::collections::HashMap;

use serde::Serialize;
use wows_core::units::Percent;
use wows_ship_stats::Build;
use wows_ship_stats::BuildEffects;
use wows_ship_stats::BuildError;
use wows_ship_stats::Triggers;
use wowsunpack::game_params::ttx::HealthFraction;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::CrewSkillModifier;

use crate::commanders::Commander;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
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

    /// The unique commander whose improved skills replace the standard ones.
    pub(crate) commander: Option<&'a Commander>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppliedSkill {
    pub name: String,
    pub points: u8,

    /// The commander's improved version replaces the standard skill.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub improved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppliedUpgrade {
    pub name: String,

    /// 1-based slot.
    pub slot: u8,
}

/// Modifiers of a ship ability counted as active, such as Combat
/// Instructions or an alternative fire mode.
#[derive(Debug, Clone)]
pub(crate) struct AbilityModifiers {
    /// Source name the stat changes cite before relabeling.
    pub(crate) source: String,

    /// English name, e.g. "Combat Instructions: Modified Secondary Munitions".
    pub(crate) label: String,

    pub(crate) modifiers: Vec<CrewSkillModifier>,
}

/// A loadout resolved against one ship.
#[derive(Clone)]
pub(crate) struct ResolvedLoadout {
    pub(crate) skills: Vec<CrewSkill>,
    pub(crate) applied_skills: Vec<AppliedSkill>,
    pub(crate) modernizations: Vec<(String, Vec<CrewSkillModifier>)>,
    pub(crate) applied_upgrades: Vec<AppliedUpgrade>,
    pub(crate) triggers: Triggers,
    pub(crate) hp_percent: Option<Percent>,

    /// Ship abilities switched on.
    pub(crate) abilities: Vec<AbilityModifiers>,

    /// Skill internal names, upgrade param names and ability sources to
    /// English names, for relabeling provenance.
    pub(crate) labels: HashMap<String, String>,
}

impl ResolvedLoadout {
    /// Whether no captain skill or upgrade is chosen. Abilities do not count.
    pub(crate) fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.modernizations.is_empty()
    }

    pub(crate) fn skill_points(&self) -> u32 {
        self.applied_skills.iter().map(|skill| u32::from(skill.points)).sum()
    }

    /// Switches `abilities` on.
    pub(crate) fn activate(&mut self, abilities: &[AbilityModifiers]) {
        for ability in abilities {
            self.labels.insert(ability.source.clone(), ability.label.clone());
        }
        self.abilities.extend_from_slice(abilities);
    }
}

/// Effective modifiers plus the conditional effects that were switched on,
/// by English name.
pub(crate) struct ResolvedEffects {
    pub(crate) build: BuildEffects,
    pub(crate) active_conditions: Vec<String>,
}

impl GameData {
    pub(crate) fn resolve_loadout(
        &self,
        ship: &Ship,
        request: &LoadoutRequest<'_>,
        interpretations: &mut Interpretations,
    ) -> Result<ResolvedLoadout, QueryError> {
        let mut skills: Vec<CrewSkill> = Vec::new();
        let mut applied_skills = Vec::new();
        let mut labels = HashMap::new();
        for query in request.skills {
            let (skill, points) = self.resolve_skill_for_class(query, ship.class, &ship.name, interpretations)?;
            if skills.iter().any(|chosen| chosen.internal_name() == skill.internal_name()) {
                continue;
            }
            let improved = request.commander.and_then(|commander| commander.improved_skill(skill));
            let name = self.skill_name(skill);
            labels.insert(skill.internal_name().as_str().to_owned(), name.clone());
            applied_skills.push(AppliedSkill { name, points, improved: improved.is_some() });
            skills.push(improved.unwrap_or(skill).clone());
        }

        let mut chosen: Vec<&crate::upgrades::Upgrade> = Vec::new();
        for query in request.upgrades {
            let upgrade = self.resolve_upgrade_for_ship(query, ship, interpretations)?;
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
            triggers: if request.conditions_met { Triggers::All } else { Triggers::Unmet },
            hp_percent: request.hp_percent,
            abilities: Vec::new(),
            labels,
        })
    }

    /// Folds the loadout into the modifiers the stat factories apply.
    pub(crate) fn effective_modifiers(
        &self,
        ship: &Ship,
        loadout: &ResolvedLoadout,
    ) -> Result<ResolvedEffects, QueryError> {
        // Abilities apply like upgrades: always on, each under its own name.
        let combined: Vec<(String, Vec<CrewSkillModifier>)>;
        let modernizations = if loadout.abilities.is_empty() {
            &loadout.modernizations
        } else {
            combined = loadout
                .modernizations
                .iter()
                .cloned()
                .chain(loadout.abilities.iter().map(|ability| (ability.source.clone(), ability.modifiers.clone())))
                .collect();
            &combined
        };
        let build = Build {
            ship: &ship.param,
            skills: &loadout.skills,
            modernizations,
            triggers: &loadout.triggers,
            health: loadout.hp_percent.map(|percent| HealthFraction::new(percent.to_fraction())),
        };
        let effects = build.effects(self.provider(), self.version()).map_err(build_failed(ship))?;
        let active_conditions = effects
            .active_conditions
            .iter()
            .map(|name| loadout.labels.get(name.as_str()).cloned().unwrap_or_else(|| name.as_str().to_owned()))
            .collect();

        Ok(ResolvedEffects { build: effects, active_conditions })
    }

    /// The same path with nothing chosen, for diffs against the stock card.
    pub(crate) fn stock_loadout(&self) -> ResolvedLoadout {
        ResolvedLoadout {
            skills: Vec::new(),
            applied_skills: Vec::new(),
            modernizations: Vec::new(),
            applied_upgrades: Vec::new(),
            triggers: Triggers::Unmet,
            hp_percent: None,
            abilities: Vec::new(),
            labels: HashMap::new(),
        }
    }
}

/// Names `ship` in a failure to compute its stats.
pub(crate) fn build_failed(ship: &Ship) -> impl FnOnce(BuildError) -> QueryError + '_ {
    move |source| QueryError::Build { ship: ship.name.clone(), source }
}
