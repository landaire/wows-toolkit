//! Active-effects engine: compute a ship's TTX card under a per-effect activation state
//! (commander-skill triggers, dynamic skills like Adrenaline Rush, consumables like the
//! spotter). Pipeline: `Loadout::effects` -> `Effects::resolve` -> `EffectiveModifiers::stats`.

use std::collections::HashMap;

use crate::data::Version;
use crate::game_params::ttx::model::ShipStats;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::ttx::modifiers::ModifierError;
use crate::game_params::ttx::selection::ShipUpgradeSelection;
use crate::game_params::types::CrewSkill;
use crate::game_params::types::CrewSkillModifier;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::Param;
use crate::game_params::types::Species;

/// A ship's equipped build: the source of effects. Borrowed; the caller assembles it
/// (a replay's parsed build, or a calculator selection).
pub struct Loadout<'a> {
    pub skills: &'a [CrewSkill],
    pub modernization_modifiers: &'a [CrewSkillModifier],
    pub ship: &'a Param,
}

/// Health fraction remaining, clamped to `[0.0, 1.0]` (1.0 = full). Drives HP-scaled
/// effects (Adrenaline Rush).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct HealthFraction(f32);

impl HealthFraction {
    pub const FULL: HealthFraction = HealthFraction(1.0);
    pub fn new(value: f32) -> Self {
        HealthFraction(value.clamp(0.0, 1.0))
    }
    pub fn value(self) -> f32 {
        self.0
    }
}

/// How an effect is activated. SP1: `Off` / `On` / `Health`; later sub-projects add
/// `Stacks(u32)` and `Heat(f32)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectActivation {
    Off,
    On,
    Health(HealthFraction),
}

/// Identifies a toggleable effect within a loadout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EffectId {
    /// Commander-skill internal name; `Modernizations` is the single aggregate always-on
    /// effect for equipped upgrade modifiers.
    Skill(String),
    Modernizations,
    Consumable(String),
}

/// How an effect activates and what coefficient it carries.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectKind {
    /// Always applied (modernizations, always-on skill modifiers); no toggle.
    AlwaysOn,
    /// On/off conditional trigger (most skill triggers).
    Binary,
    /// HP-scaled reload (Adrenaline Rush): the modifiers are `lastChanceReloadCoefficient_*`
    /// per-armament coefficients, evaluated by the gameplay formula at the health fraction.
    HealthScaledReload,
    /// A consumable that, while active, applies its `modifiers` block and (for the spotter)
    /// an `artillery_dist_coeff` range multiplier.
    Consumable { artillery_dist_coeff: f32 },
}

/// One toggleable effect contributed by the loadout. Raw modifiers are private; resolution
/// folds them.
#[derive(Clone, Debug)]
pub struct Effect {
    id: EffectId,
    kind: EffectKind,
    modifiers: Vec<CrewSkillModifier>,
}

impl Effect {
    pub fn id(&self) -> &EffectId {
        &self.id
    }
    pub fn kind(&self) -> &EffectKind {
        &self.kind
    }
    /// The activation used when `EffectsState` leaves this effect unset:
    /// `AlwaysOn -> On`, `Binary`/`Consumable -> Off`, `HealthScaledReload -> Health(FULL)`.
    pub fn default_activation(&self) -> EffectActivation {
        match self.kind {
            EffectKind::AlwaysOn => EffectActivation::On,
            EffectKind::Binary | EffectKind::Consumable { .. } => EffectActivation::Off,
            EffectKind::HealthScaledReload => EffectActivation::Health(HealthFraction::FULL),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(id: EffectId, kind: EffectKind, modifiers: Vec<CrewSkillModifier>) -> Effect {
        Effect { id, kind, modifiers }
    }
}

/// The loadout's toggleable effects. Enumerate with `iter`, resolve with `resolve`.
#[derive(Clone, Debug)]
pub struct Effects(Vec<Effect>);

/// Per-effect activation overrides, built fluently. Effects absent here use their
/// `default_activation`.
#[derive(Clone, Debug, Default)]
pub struct EffectsState {
    activations: HashMap<EffectId, EffectActivation>,
}

impl EffectsState {
    /// Set one effect's activation; chainable.
    pub fn set(mut self, id: EffectId, activation: EffectActivation) -> Self {
        self.activations.insert(id, activation);
        self
    }
    pub fn get(&self, id: &EffectId) -> Option<EffectActivation> {
        self.activations.get(id).copied()
    }
}

/// The resolved modifiers for a state: the aggregated bundle plus the consumable artillery
/// range coefficient (applied out-of-band by the artillery factory).
pub struct EffectiveModifiers {
    bundle: ModifierBundle,
    artillery_dist_coeff: f32,
}

impl EffectiveModifiers {
    pub fn bundle(&self) -> &ModifierBundle {
        &self.bundle
    }
    pub fn artillery_dist_coeff(&self) -> f32 {
        self.artillery_dist_coeff
    }
}

impl Loadout<'_> {
    pub fn effects(&self, _provider: &dyn GameParamProvider) -> Effects {
        Effects(Vec::new()) // Task 2
    }
}

impl Effects {
    pub fn iter(&self) -> std::slice::Iter<'_, Effect> {
        self.0.iter()
    }
    pub fn resolve(
        &self,
        _state: &EffectsState,
        species: Species,
        _version: Version,
    ) -> Result<EffectiveModifiers, ModifierError> {
        Ok(EffectiveModifiers { bundle: ModifierBundle::empty(species), artillery_dist_coeff: 1.0 }) // Task 3
    }
}

impl EffectiveModifiers {
    pub fn stats(
        &self,
        ship: &Param,
        selection: &ShipUpgradeSelection,
        level: u32,
        provider: &dyn GameParamProvider,
    ) -> ShipStats {
        crate::game_params::ttx::orchestration::ship_stats(ship, selection, &self.bundle, level, provider) // Task 4 threads dist coeff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_fraction_clamps() {
        assert_eq!(HealthFraction::new(1.5).value(), 1.0);
        assert_eq!(HealthFraction::new(-0.2).value(), 0.0);
        assert_eq!(HealthFraction::new(0.5).value(), 0.5);
        assert_eq!(HealthFraction::FULL.value(), 1.0);
    }

    #[test]
    fn default_activation_per_kind() {
        let mk = |kind: EffectKind| Effect::for_test(EffectId::Modernizations, kind, Vec::new());
        assert_eq!(mk(EffectKind::AlwaysOn).default_activation(), EffectActivation::On);
        assert_eq!(mk(EffectKind::Binary).default_activation(), EffectActivation::Off);
        assert_eq!(
            mk(EffectKind::Consumable { artillery_dist_coeff: 1.2 }).default_activation(),
            EffectActivation::Off
        );
        assert_eq!(
            mk(EffectKind::HealthScaledReload).default_activation(),
            EffectActivation::Health(HealthFraction::FULL)
        );
    }

    #[test]
    fn effects_state_builds_and_reads() {
        let s = EffectsState::default()
            .set(EffectId::Skill("X".into()), EffectActivation::On)
            .set(EffectId::Consumable("Y".into()), EffectActivation::Off);
        assert_eq!(s.get(&EffectId::Skill("X".into())), Some(EffectActivation::On));
        assert_eq!(s.get(&EffectId::Consumable("Y".into())), Some(EffectActivation::Off));
        assert_eq!(s.get(&EffectId::Modernizations), None);
    }
}
