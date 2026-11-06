//! A ship with captain skills, upgrades and modules, and its port stat card.

use wowsunpack::data::Version;
use wowsunpack::game_params::ttx::EffectActivation;
use wowsunpack::game_params::ttx::EffectId;
use wowsunpack::game_params::ttx::EffectKind;
use wowsunpack::game_params::ttx::EffectiveModifiers;
use wowsunpack::game_params::ttx::EffectsState;
use wowsunpack::game_params::ttx::HealthFraction;
use wowsunpack::game_params::ttx::Loadout;
use wowsunpack::game_params::ttx::ShipStatsProvenance;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::model::ShipStats;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::error::BuildError;
use crate::resistance::merges_fire_zones;

/// Conditional skill triggers counted as met.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Triggers {
    /// The port card: conditional effects stay off.
    Unmet,
    All,

    /// Only the triggers of this skill.
    Skill(CrewSkillName),
}

/// What a ship carries besides its modules: captain skills, upgrades and the
/// situation its conditional effects read.
pub struct Build<'a> {
    pub ship: &'a Param,
    pub skills: &'a [CrewSkill],

    /// Equipped upgrades by GameParams name, with their modifiers.
    pub modernizations: &'a [(String, Vec<CrewSkillModifier>)],
    pub triggers: &'a Triggers,

    /// Remaining health for health-scaled effects such as Adrenaline Rush.
    /// `None` reads as full health.
    pub health: Option<HealthFraction>,
}

/// The modifiers a build applies, and the conditional skills switched on.
pub struct BuildEffects {
    pub modifiers: EffectiveModifiers,

    /// One entry per conditional skill effect counted as triggered.
    pub active_conditions: Vec<CrewSkillName>,

    /// Fire Prevention Expert is learned. Its `fireResistanceEnabled` flag is
    /// not a numeric modifier, so it is read off the skill.
    pub merges_fire_zones: bool,
}

/// The port stat card of a build, with the inputs behind each stat.
pub struct BuildCard {
    pub stats: ShipStats,
    pub provenance: ShipStatsProvenance,
}

impl Build<'_> {
    pub fn species(&self) -> Result<Species, BuildError> {
        self.ship
            .species()
            .and_then(|species| species.known())
            .copied()
            .ok_or_else(|| BuildError::UnknownSpecies { ship: self.ship.name().to_owned() })
    }

    /// Folds the skills and upgrades into the modifiers the stat factories
    /// apply, switching on the conditional effects `triggers` and `health`
    /// select.
    pub fn effects(&self, provider: &dyn GameParamProvider, version: Version) -> Result<BuildEffects, BuildError> {
        let species = self.species()?;
        let effects = Loadout { skills: self.skills, modernizations: self.modernizations, ship: self.ship }
            .effects(provider, version);

        let mut state = EffectsState::default();
        let mut active_conditions = Vec::new();
        for effect in effects.iter() {
            let met = match self.triggers {
                Triggers::Unmet => false,
                Triggers::All => true,
                Triggers::Skill(skill) => matches!(effect.id(), EffectId::Skill(name) if name == skill),
            };
            let activation = match effect.kind() {
                EffectKind::Binary { .. } if met => Some(EffectActivation::On),
                EffectKind::StackingPerCount { .. } if met => Some(EffectActivation::Stacks(u32::MAX)),
                EffectKind::Heat { heat_interpolator } if met => {
                    Some(EffectActivation::Heat(heat_interpolator.max_x()))
                }
                EffectKind::HealthScaledReload | EffectKind::InnateAdrenaline { .. } => {
                    self.health.map(EffectActivation::Health)
                }
                _ => None,
            };
            let Some(activation) = activation else {
                continue;
            };

            if let EffectId::Skill(name) = effect.id()
                && matches!(
                    effect.kind(),
                    EffectKind::Binary { .. } | EffectKind::StackingPerCount { .. } | EffectKind::Heat { .. }
                )
            {
                active_conditions.push(name.clone());
            }
            state = state.set(effect.id().clone(), activation);
        }

        let modifiers = effects
            .resolve(&state, species, version)
            .map_err(|source| BuildError::Modifiers { ship: self.ship.name().to_owned(), source })?;

        let merges_fire_zones = self.skills.iter().any(merges_fire_zones);

        Ok(BuildEffects { modifiers, active_conditions, merges_fire_zones })
    }
}

impl BuildEffects {
    /// The port stat card of `ship` with `selection` mounted and these
    /// effects applied.
    pub fn stats(
        &self,
        ship: &Param,
        selection: &ShipUpgradeSelection,
        provider: &dyn GameParamProvider,
    ) -> Result<ShipStats, BuildError> {
        Ok(self.modifiers.stats(ship, selection, level(ship)?, provider))
    }

    /// The same card with the inputs behind each stat.
    pub fn card(
        &self,
        ship: &Param,
        selection: &ShipUpgradeSelection,
        provider: &dyn GameParamProvider,
    ) -> Result<BuildCard, BuildError> {
        let (stats, provenance) = self.modifiers.stats_explained(ship, selection, level(ship)?, provider);

        Ok(BuildCard { stats, provenance })
    }
}

fn level(ship: &Param) -> Result<u32, BuildError> {
    ship.vehicle().map(|vehicle| vehicle.level()).ok_or_else(|| BuildError::NotAShip { param: ship.name().to_owned() })
}
