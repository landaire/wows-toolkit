//! Active-effects engine: compute a ship's TTX card under a per-effect activation state
//! (commander-skill triggers, dynamic skills like Adrenaline Rush, consumables like the
//! spotter). Pipeline: `Loadout::effects` -> `Effects::resolve` -> `EffectiveModifiers::stats`.

use std::collections::HashMap;

use crate::data::Version;
use crate::game_params::ttx::model::ShipStats;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::ttx::modifiers::ModifierError;
use crate::game_params::ttx::modifiers::modifier_identity;
use crate::game_params::ttx::provenance::InputId;
use crate::game_params::ttx::provenance::ModifierSources;
use crate::game_params::ttx::provenance::Recorder;
use crate::game_params::ttx::selection::ShipUpgradeSelection;
use crate::game_params::types::CrewSkill;
use crate::game_params::types::CrewSkillLogicTrigger;
use crate::game_params::types::CrewSkillModifier;
use crate::game_params::types::CrewSkillName;
use crate::game_params::types::GameParamProvider;
use crate::game_params::types::InnateSkillBreakpoint;
use crate::game_params::types::Interpolator;
use crate::game_params::types::KnownCrewSkill;
use crate::game_params::types::Param;
use crate::game_params::types::Species;
use crate::game_types::Consumable;
use crate::recognized::Recognized;

const TRIGGER_ACTIVATION_ON_BURN_FLOOD: &str = "activationOnBurnFlood";
const TRIGGER_POTENTIAL_DAMAGE_RATIO: &str = "potentialDamageRatio";
const TRIGGER_ATBA_HEAT: &str = "atbaHeat";

/// The deob clamps the stacking count to `PACKED_STATE_LIMITS` (1023).
const STACK_LIMIT: u32 = 1023;

/// Stack count carried by an activation; any non-`Stacks` activation (including the
/// `Stacks(0)` default) yields 0.
fn stacks(activation: EffectActivation) -> u32 {
    match activation {
        EffectActivation::Stacks(n) => n,
        _ => 0,
    }
}

/// The client packs the heat coefficient into 0..100 (`int(ratio*100)`); replicate that
/// 1% quantization. `ratio` is in `[0, 1]` (clamped by `eval`), so `floor` matches `int`.
fn quantize_heat_ratio(ratio: f32) -> f32 {
    (ratio * 100.0).floor() / 100.0
}

/// The active innate-adrenaline modifier set at health fraction `hf`: the bounding breakpoint
/// (clamped, no extrapolation) or a per-key lerp between the two bracketing breakpoints,
/// evaluated for `species`. `breakpoints` is descending by health fraction and non-empty.
fn active_innate_modifiers(breakpoints: &[InnateSkillBreakpoint], hf: f32, species: Species) -> Vec<CrewSkillModifier> {
    let hi_end = &breakpoints[0];
    let lo_end = &breakpoints[breakpoints.len() - 1];
    if hf >= hi_end.health_fraction() {
        return hi_end.modifiers().to_vec();
    }
    if hf <= lo_end.health_fraction() {
        return lo_end.modifiers().to_vec();
    }
    for pair in breakpoints.windows(2) {
        let (hi, lo) = (&pair[0], &pair[1]);
        if hf <= hi.health_fraction() && hf >= lo.health_fraction() {
            let span = lo.health_fraction() - hi.health_fraction();
            let t = if span == 0.0 { 0.0 } else { ((hf - hi.health_fraction()) / span).clamp(0.0, 1.0) };
            return hi
                .modifiers()
                .iter()
                .map(|m| {
                    let a = m.get_for_species(&species);
                    let b = lo
                        .modifiers()
                        .iter()
                        .find(|o| o.name() == m.name())
                        .map(|o| o.get_for_species(&species))
                        .unwrap_or(a);
                    CrewSkillModifier::uniform(m.name(), a + (b - a) * t)
                })
                .collect();
        }
    }
    Vec::new()
}

/// The condition that gates a `Binary` effect's activation: what game-world state must hold
/// for the effect to be `On`.
#[derive(Clone, Debug, PartialEq)]
pub enum TriggerCondition {
    /// Ship is currently detected by an enemy (`entityIsVisibleTrigger`).
    Detected,
    /// Ship is currently undetected (`entityIsInvisibleTrigger`).
    Undetected,
    /// Ship was detected and the effect persists for `duration` seconds (`activationOnDetectTrigger`).
    OnDetected { duration: f32 },
    /// An enemy is within this ship's detection range (`enemyWithinVisibilityTrigger`).
    EnemyWithinDetectionRange,
    /// No enemy is within this ship's detection range (`noEnemiesWithinVisibilityTrigger`).
    NoEnemyWithinDetectionRange,
    /// An enemy is within main-gun range (`VisibleEnemyWithinGmTrigger`).
    EnemyWithinMainGunRange,
    /// An enemy is within secondary-gun range (`visibleEnemyWithinGsTrigger`).
    EnemyWithinSecondaryRange,
    /// Enemies on the team's side of the map are not fewer than allies (`EnemiesNotLessThanAlliesWithinGMTrigger`).
    Outnumbered,
    /// Ship's AA defense is active (`activeAirDefense`).
    ActiveAirDefense,
    /// Submarine battery is low (`activationOnBattery`).
    SubmarineBatteryLow,
    /// A recognized consumable of this type is currently active (`activationOnConsumable`).
    OnConsumableActive(Consumable),
    /// Trigger type not yet modeled; the raw string is preserved for diagnostics.
    Other(String),
}

impl TriggerCondition {
    /// Classify a trigger into its condition. Reads `trigger.duration()` for `OnDetected` and
    /// `trigger.consumable_type(version)` for `OnConsumableActive`. `None` when the trigger
    /// carries no `triggerType` (absent on some builds) and thus has no classifiable condition.
    pub fn from_trigger_type(trigger: &CrewSkillLogicTrigger, version: Version) -> Option<TriggerCondition> {
        let condition = match trigger.trigger_type()? {
            "entityIsVisibleTrigger" => TriggerCondition::Detected,
            "entityIsInvisibleTrigger" => TriggerCondition::Undetected,
            // A detect-window trigger with no duration field can't be modeled as one.
            "activationOnDetectTrigger" => match trigger.duration() {
                Some(duration) => TriggerCondition::OnDetected { duration },
                None => TriggerCondition::Other("activationOnDetectTrigger".to_owned()),
            },
            "enemyWithinVisibilityTrigger" => TriggerCondition::EnemyWithinDetectionRange,
            "noEnemiesWithinVisibilityTrigger" => TriggerCondition::NoEnemyWithinDetectionRange,
            "VisibleEnemyWithinGmTrigger" => TriggerCondition::EnemyWithinMainGunRange,
            "visibleEnemyWithinGsTrigger" => TriggerCondition::EnemyWithinSecondaryRange,
            "EnemiesNotLessThanAlliesWithinGMTrigger" => TriggerCondition::Outnumbered,
            "activeAirDefense" => TriggerCondition::ActiveAirDefense,
            "activationOnBattery" => TriggerCondition::SubmarineBatteryLow,
            "activationOnConsumable" => match trigger.consumable_type(version).and_then(|c| c.into_known()) {
                Some(c) => TriggerCondition::OnConsumableActive(c),
                None => TriggerCondition::Other("activationOnConsumable".to_owned()),
            },
            other => TriggerCondition::Other(other.to_owned()),
        };
        Some(condition)
    }

    /// Whether this condition is satisfied by `facts`. `Other` is never satisfied.
    pub fn holds(&self, facts: &SituationFacts) -> bool {
        match self {
            TriggerCondition::Detected => facts.detected,
            TriggerCondition::Undetected => !facts.detected,
            TriggerCondition::OnDetected { duration } => facts.seconds_since_detected.is_some_and(|t| t <= *duration),
            TriggerCondition::EnemyWithinDetectionRange => facts.enemy_within_detection_range,
            TriggerCondition::NoEnemyWithinDetectionRange => !facts.enemy_within_detection_range,
            TriggerCondition::EnemyWithinMainGunRange => facts.enemy_within_main_gun_range,
            TriggerCondition::EnemyWithinSecondaryRange => facts.enemy_within_secondary_range,
            TriggerCondition::Outnumbered => facts.outnumbered,
            TriggerCondition::ActiveAirDefense => facts.active_air_defense,
            TriggerCondition::SubmarineBatteryLow => facts.submarine_battery_low,
            TriggerCondition::OnConsumableActive(c) => facts.active_consumables.contains(c),
            TriggerCondition::Other(_) => false,
        }
    }
}

/// A snapshot of battle-context facts used to derive per-effect activations without
/// requiring the caller to know the internal `EffectsState` layout. Pass a default
/// instance (stock, full-health) to `Effects::situation_state` to reproduce the
/// identity card.
pub struct SituationFacts {
    /// Current HP fraction in `[0.0, 1.0]`; `1.0` = full health. Drives
    /// `HealthScaledReload` (Adrenaline Rush) and `InnateAdrenaline` effects.
    pub hp_fraction: f32,
    /// Number of active burns and floods on the ship right now. Drives `StackingPerCount`
    /// (Furious) effects.
    pub burn_flood_count: u32,
    /// Cumulative potential damage received since the start of battle (the raw game counter,
    /// not normalized). Drives `StackingRepeated` (Potential Damage ratio) effects: stacks =
    /// `floor(potential_damage / max_health)`.
    pub potential_damage: f32,
    /// The ship's maximum health pool (used as the denominator for `potential_damage`
    /// stacking). Must be `> 0.0` for stacking to be non-zero.
    pub max_health: f32,
    /// Seconds the manual-secondaries target has been continuously held. Drives `Heat`
    /// (ATBA manual-secondaries accuracy) effects.
    pub secondary_fire_seconds: f32,
    /// Consumable types currently active on this ship. Drives `Consumable` effects and
    /// `OnConsumableActive` binary triggers.
    pub active_consumables: Vec<Consumable>,
    /// `true` if the ship is currently spotted by at least one enemy. Independent of
    /// `seconds_since_detected`: a ship can be spotted (`detected = true`) while
    /// `seconds_since_detected` is `None` (the detection event was not recorded), or
    /// unspotted (`detected = false`) while `seconds_since_detected` is `Some(t)` (the
    /// ship was detected `t` seconds ago but has since gone dark). Drives `Detected` and
    /// `Undetected` trigger conditions.
    pub detected: bool,
    /// Seconds elapsed since the most recent detection event began, or `None` if no
    /// detection event is being tracked. This is the elapsed time since the ship *became*
    /// detected, not a live "currently spotted for X seconds" counter -- once the ship goes
    /// dark the value keeps counting up (or the caller may clear it). Drives `OnDetected {
    /// duration }`: the condition holds while `seconds_since_detected <= duration`.
    pub seconds_since_detected: Option<f32>,
    /// `true` if at least one enemy is within this ship's detection range. Drives
    /// `EnemyWithinDetectionRange` and `NoEnemyWithinDetectionRange` trigger conditions.
    pub enemy_within_detection_range: bool,
    /// `true` if at least one enemy is within main-gun range. Drives
    /// `EnemyWithinMainGunRange` trigger condition.
    pub enemy_within_main_gun_range: bool,
    /// `true` if at least one enemy is within secondary-gun range. Drives
    /// `EnemyWithinSecondaryRange` trigger condition.
    pub enemy_within_secondary_range: bool,
    /// `true` if the team's side of the map has at least as many enemies as allies. Drives
    /// the `Outnumbered` trigger condition.
    pub outnumbered: bool,
    /// `true` if the ship's AA defense is currently active. Drives `ActiveAirDefense`
    /// trigger condition.
    pub active_air_defense: bool,
    /// `true` if the submarine's battery charge is low. Drives `SubmarineBatteryLow`
    /// trigger condition.
    pub submarine_battery_low: bool,
}

impl Default for SituationFacts {
    fn default() -> Self {
        SituationFacts {
            hp_fraction: 1.0,
            burn_flood_count: 0,
            potential_damage: 0.0,
            max_health: 0.0,
            secondary_fire_seconds: 0.0,
            active_consumables: Vec::new(),
            detected: false,
            seconds_since_detected: None,
            enemy_within_detection_range: false,
            enemy_within_main_gun_range: false,
            enemy_within_secondary_range: false,
            outnumbered: false,
            active_air_defense: false,
            submarine_battery_low: false,
        }
    }
}

/// A ship's equipped build: the source of effects. Borrowed; the caller assembles it
/// (a replay's parsed build, or a calculator selection).
pub struct Loadout<'a> {
    pub skills: &'a [CrewSkill],
    /// Equipped modernizations, one entry per upgrade: `(upgrade name, its modifiers)`.
    pub modernizations: &'a [(String, Vec<CrewSkillModifier>)],
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

/// How an effect is activated. `Off` / `On` / `Health` / `Stacks` / `Heat`; `Stacks(n)` drives
/// the stacking triggers (Furious burn+flood count, potential-damage health-multiplier);
/// `Heat(seconds)` drives manual-secondaries heat triggers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectActivation {
    Off,
    On,
    Health(HealthFraction),
    Stacks(u32),
    Heat(f32),
}

/// Identifies a toggleable effect within a loadout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EffectId {
    /// A commander skill, keyed by its stable internal name.
    Skill(CrewSkillName),
    /// One equipped modernization, keyed by its upgrade name.
    Upgrade(String),
    /// A ship consumable, keyed by its recognized type (e.g. `Consumable::SpottingAircraft`).
    Consumable(Consumable),
    /// A ship innate skill, keyed by its `skill_type` (e.g. "adrenalineRush").
    Innate(String),
}

/// How an effect activates and what coefficient it carries.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectKind {
    /// Always applied (modernizations, always-on skill modifiers); no toggle.
    AlwaysOn,
    /// On/off conditional trigger (most skill triggers). `condition` is metadata describing
    /// what game-world state activates it; resolution still keys off `On`/`Off` activation.
    Binary { condition: TriggerCondition },
    /// HP-scaled reload (Adrenaline Rush): the modifiers are `lastChanceReloadCoefficient_*`
    /// per-armament coefficients, evaluated by the gameplay formula at the health fraction.
    HealthScaledReload,
    /// A consumable that, while active, applies its `modifiers` block and (for the spotter)
    /// an `artillery_dist_coeff` range multiplier.
    Consumable { artillery_dist_coeff: f32 },
    /// Furious (`activationOnBurnFlood`): at `Stacks(n)`, contribute every per-count block
    /// with `count <= n` (accumulative). Capped at the highest count present.
    StackingPerCount { blocks: Vec<(u32, Vec<CrewSkillModifier>)> },
    /// Potential-damage (`potentialDamageRatio`): at `Stacks(n)`, contribute `Effect.modifiers`
    /// `n` times (the modifier folds multiplicatively to `^n`).
    StackingRepeated,
    /// Manual-secondaries heat (`atbaHeat`): at `Heat(seconds)`, `ratio =
    /// heat_interpolator.eval(seconds)` (quantized to 1% as the client does), then each
    /// modifier is lerped from its identity to its configured value by `ratio`.
    Heat { heat_interpolator: Interpolator },
    /// Ship innate adrenaline (HP-breakpoint dispersion + reload). At `Health(hf)`, clamp to
    /// the highest/lowest breakpoint or lerp between the two bracketing breakpoints. Held
    /// descending by health fraction.
    InnateAdrenaline { breakpoints: Vec<InnateSkillBreakpoint> },
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
    /// `AlwaysOn -> On`, `Binary`/`Consumable -> Off`, `HealthScaledReload`/`InnateAdrenaline -> Health(FULL)`,
    /// `StackingPerCount`/`StackingRepeated -> Stacks(0)`, `Heat -> Heat(0.0)`.
    pub fn default_activation(&self) -> EffectActivation {
        match self.kind {
            EffectKind::AlwaysOn => EffectActivation::On,
            EffectKind::Binary { .. } | EffectKind::Consumable { .. } => EffectActivation::Off,
            EffectKind::HealthScaledReload => EffectActivation::Health(HealthFraction::FULL),
            EffectKind::StackingPerCount { .. } | EffectKind::StackingRepeated => EffectActivation::Stacks(0),
            EffectKind::Heat { .. } => EffectActivation::Heat(0.0),
            EffectKind::InnateAdrenaline { .. } => EffectActivation::Health(HealthFraction::FULL),
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

/// Per-armament reload multipliers from dynamic effects (Adrenaline Rush), evaluated at
/// the state's health fraction. `1.0` = no change. Kept separate from the bundle because
/// `lastChanceReloadCoefficient_*` is additive in `MODIFIER_SETTINGS` (the raw per-HP
/// coefficient sums across sources), whereas the evaluated value is a multiplier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReloadCoeffs {
    pub main: f32,
    pub secondary: f32,
    pub torpedo: f32,
}

impl Default for ReloadCoeffs {
    fn default() -> Self {
        ReloadCoeffs { main: 1.0, secondary: 1.0, torpedo: 1.0 }
    }
}

/// The `InputId` source for each per-armament reload channel populated by dynamic
/// effects (Adrenaline Rush). `None` when the channel was not modified.
#[derive(Clone, Debug, Default)]
pub struct ReloadCoeffSources {
    pub main: Option<InputId>,
    pub secondary: Option<InputId>,
    pub torpedo: Option<InputId>,
}

/// The resolved modifiers for a state: the aggregated static bundle, the consumable
/// artillery range coefficient, and the dynamic per-armament reload multipliers -- all
/// applied by the artillery/torpedo factories.
pub struct EffectiveModifiers {
    bundle: ModifierBundle,
    sources: ModifierSources,
    artillery_dist_coeff: f32,
    artillery_dist_coeff_source: Option<InputId>,
    reload_coeffs: ReloadCoeffs,
    reload_coeff_sources: ReloadCoeffSources,
    version: Version,
}

impl EffectiveModifiers {
    pub fn bundle(&self) -> &ModifierBundle {
        &self.bundle
    }
    pub fn sources(&self) -> &ModifierSources {
        &self.sources
    }
    pub fn artillery_dist_coeff(&self) -> f32 {
        self.artillery_dist_coeff
    }
    pub fn artillery_dist_coeff_source(&self) -> Option<&InputId> {
        self.artillery_dist_coeff_source.as_ref()
    }
    pub fn reload_coeffs(&self) -> ReloadCoeffs {
        self.reload_coeffs
    }
    pub fn reload_coeff_sources(&self) -> &ReloadCoeffSources {
        &self.reload_coeff_sources
    }
    pub fn version(&self) -> Version {
        self.version
    }
}

/// The attribution identity for an effect's contributions.
fn input_id_for(id: &EffectId) -> InputId {
    match id {
        EffectId::Skill(name) => InputId::Skill { name: name.clone() },
        EffectId::Upgrade(name) => InputId::Upgrade { name: name.clone() },
        EffectId::Consumable(c) => InputId::Consumable(Recognized::Known(*c)),
        EffectId::Innate(t) => InputId::Innate { skill_type: t.clone() },
    }
}

/// Walk the ship's ability slots, invoking `f` for each resolved consumable.
/// Skips slots whose param or category cannot be resolved. Does NOT filter by
/// `into_known()`; callers apply that filter themselves if needed.
pub(crate) fn walk_ability_slots<F>(ship: &Param, provider: &dyn GameParamProvider, version: Version, mut f: F)
where
    F: FnMut(Recognized<crate::game_types::Consumable>, &crate::game_params::types::AbilityCategory),
{
    let Some(vehicle) = ship.vehicle() else { return };
    let Some(ability_slots) = vehicle.abilities() else { return };
    for slot in ability_slots {
        for (ability_name, variant_name) in slot {
            let Some(param) = provider.game_param_by_name(ability_name) else { continue };
            let Some(ability) = param.ability() else { continue };
            let Some(cat) = ability.get_category(variant_name) else { continue };
            let consumable = cat.consumable_type(version);
            f(consumable, cat);
        }
    }
}

impl Loadout<'_> {
    pub fn effects(&self, provider: &dyn GameParamProvider, version: Version) -> Effects {
        let mut effects: Vec<Effect> = Vec::new();

        for (name, mods) in self.modernizations {
            if mods.is_empty() {
                continue;
            }
            effects.push(Effect {
                id: EffectId::Upgrade(name.clone()),
                kind: EffectKind::AlwaysOn,
                modifiers: mods.clone(),
            });
        }

        for skill in self.skills {
            let name = skill.internal_name().clone();
            if let Some(mods) = skill.modifiers().filter(|m| !m.is_empty()) {
                effects.push(Effect {
                    id: EffectId::Skill(name.clone()),
                    kind: EffectKind::AlwaysOn,
                    modifiers: mods.clone(),
                });
            }
            if let Some(trigger) = skill.logic_trigger() {
                match trigger.trigger_type() {
                    Some(TRIGGER_ACTIVATION_ON_BURN_FLOOD) if !trigger.count_to_modifier().is_empty() => {
                        effects.push(Effect {
                            id: EffectId::Skill(name),
                            kind: EffectKind::StackingPerCount { blocks: trigger.count_to_modifier().to_vec() },
                            modifiers: Vec::new(),
                        });
                    }
                    Some(TRIGGER_POTENTIAL_DAMAGE_RATIO) => {
                        if let Some(tmods) = trigger.modifiers().filter(|m| !m.is_empty()) {
                            effects.push(Effect {
                                id: EffectId::Skill(name),
                                kind: EffectKind::StackingRepeated,
                                modifiers: tmods.clone(),
                            });
                        }
                    }
                    Some(TRIGGER_ATBA_HEAT) => {
                        if let Some(tmods) = trigger.modifiers().filter(|m| !m.is_empty()) {
                            effects.push(Effect {
                                id: EffectId::Skill(name),
                                kind: EffectKind::Heat { heat_interpolator: trigger.heat_interpolator().clone() },
                                modifiers: tmods.clone(),
                            });
                        }
                    }
                    _ => {
                        if let Some(tmods) = trigger.modifiers().filter(|m| !m.is_empty()) {
                            let recognized = KnownCrewSkill::recognize(skill.internal_name(), skill.skill_type());
                            let kind = match recognized.known() {
                                Some(KnownCrewSkill::AdrenalineRush)
                                | Some(KnownCrewSkill::SubmarineAdrenalineRush) => Some(EffectKind::HealthScaledReload),
                                // A trigger with no classifiable condition (no `triggerType`) yields no effect.
                                _ => TriggerCondition::from_trigger_type(trigger, version)
                                    .map(|condition| EffectKind::Binary { condition }),
                            };
                            if let Some(kind) = kind {
                                effects.push(Effect { id: EffectId::Skill(name), kind, modifiers: tmods.clone() });
                            }
                        }
                    }
                }
            }
        }

        walk_ability_slots(self.ship, provider, version, |recognized, cat| {
            let Some(consumable) = recognized.into_known() else { return };
            let artillery_dist_coeff = cat.effect_fields().get("artilleryDistCoeff").copied().unwrap_or(1.0);
            let cat_modifiers = cat.modifiers();
            if artillery_dist_coeff != 1.0 || !cat_modifiers.is_empty() {
                effects.push(Effect {
                    id: EffectId::Consumable(consumable),
                    kind: EffectKind::Consumable { artillery_dist_coeff },
                    modifiers: cat_modifiers.to_vec(),
                });
            }
        });

        if let Some(vehicle) = self.ship.vehicle() {
            for innate in vehicle.innate_skills() {
                if innate.breakpoints().is_empty() {
                    continue;
                }
                let mut breakpoints = innate.breakpoints().to_vec();
                breakpoints.sort_by(|a, b| {
                    b.health_fraction().partial_cmp(&a.health_fraction()).unwrap_or(std::cmp::Ordering::Equal)
                });
                effects.push(Effect {
                    id: EffectId::Innate(innate.skill_type().to_owned()),
                    kind: EffectKind::InnateAdrenaline { breakpoints },
                    modifiers: Vec::new(),
                });
            }
        }

        Effects(effects)
    }
}

impl Effects {
    pub fn iter(&self) -> std::slice::Iter<'_, Effect> {
        self.0.iter()
    }

    #[cfg(test)]
    pub(crate) fn for_test(effects: Vec<Effect>) -> Self {
        Effects(effects)
    }

    /// Derive a per-effect `EffectsState` from a tactical snapshot. Each effect's activation
    /// is computed from its kind and `facts`; `AlwaysOn` effects keep their default.
    pub fn situation_state(&self, facts: &SituationFacts) -> EffectsState {
        let mut state = EffectsState::default();
        for effect in &self.0 {
            let activation = match effect.kind() {
                EffectKind::AlwaysOn => continue,
                EffectKind::HealthScaledReload | EffectKind::InnateAdrenaline { .. } => {
                    EffectActivation::Health(HealthFraction::new(facts.hp_fraction))
                }
                EffectKind::StackingPerCount { .. } => EffectActivation::Stacks(facts.burn_flood_count),
                EffectKind::StackingRepeated => {
                    let n = if facts.max_health > 0.0 { (facts.potential_damage / facts.max_health) as u32 } else { 0 };
                    EffectActivation::Stacks(n)
                }
                EffectKind::Heat { .. } => EffectActivation::Heat(facts.secondary_fire_seconds),
                EffectKind::Consumable { .. } => {
                    let on = matches!(effect.id(), EffectId::Consumable(c) if facts.active_consumables.contains(c));
                    if on { EffectActivation::On } else { EffectActivation::Off }
                }
                EffectKind::Binary { condition } => {
                    if condition.holds(facts) {
                        EffectActivation::On
                    } else {
                        EffectActivation::Off
                    }
                }
            };
            state = state.set(effect.id().clone(), activation);
        }
        state
    }

    pub fn resolve(
        &self,
        state: &EffectsState,
        species: Species,
        version: Version,
    ) -> Result<EffectiveModifiers, ModifierError> {
        const EPSILON: f32 = 1e-6;
        let mut contributed: Vec<CrewSkillModifier> = Vec::new();
        let mut sources = ModifierSources::default();
        let mut dist: f32 = 1.0;
        let mut dist_source: Option<InputId> = None;
        let mut reload_coeffs = ReloadCoeffs::default();
        let mut reload_coeff_sources = ReloadCoeffSources::default();

        for effect in &self.0 {
            let activation = state.get(effect.id()).unwrap_or_else(|| effect.default_activation());
            match (&effect.kind, activation) {
                (EffectKind::AlwaysOn, _) => {
                    for m in &effect.modifiers {
                        sources.record(m.name(), input_id_for(effect.id()), m.get_for_species(&species));
                        contributed.push(m.clone());
                    }
                }
                (EffectKind::Binary { .. }, EffectActivation::On) => {
                    for m in &effect.modifiers {
                        sources.record(m.name(), input_id_for(effect.id()), m.get_for_species(&species));
                        contributed.push(m.clone());
                    }
                }
                (EffectKind::HealthScaledReload, activation) => {
                    let hf = match activation {
                        EffectActivation::Health(hf) => hf,
                        _ => HealthFraction::FULL,
                    };
                    let lost = 1.0 - hf.value();
                    for m in &effect.modifiers {
                        let name = m.name();
                        if let Some(suffix) = name.strip_prefix("lastChanceReloadCoefficient_") {
                            // `clamp` matches `_getLastChanceReloadCoefficient` (ModifiersApply.py:551).
                            // Multiplying per modifier is exact for SP1's single health-scaled source;
                            // SP2 stacking/innate must instead sum `c` per channel and evaluate once.
                            let c = m.get_for_species(&species).clamp(0.0, 1.0);
                            let mul = (1.0 - lost * c).max(EPSILON);
                            let src = input_id_for(effect.id());
                            match suffix {
                                "Main" => {
                                    reload_coeffs.main *= mul;
                                    reload_coeff_sources.main = Some(src);
                                }
                                "Sec" => {
                                    reload_coeffs.secondary *= mul;
                                    reload_coeff_sources.secondary = Some(src);
                                }
                                "Torp" => {
                                    reload_coeffs.torpedo *= mul;
                                    reload_coeff_sources.torpedo = Some(src);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                (EffectKind::Consumable { artillery_dist_coeff }, EffectActivation::On) => {
                    for m in &effect.modifiers {
                        sources.record(m.name(), input_id_for(effect.id()), m.get_for_species(&species));
                        contributed.push(m.clone());
                    }
                    if *artillery_dist_coeff != 1.0 {
                        dist_source = Some(input_id_for(effect.id()));
                    }
                    dist *= artillery_dist_coeff;
                }
                (EffectKind::StackingPerCount { blocks }, activation) => {
                    let n = stacks(activation);
                    for (count, block) in blocks {
                        if *count <= n {
                            for m in block {
                                sources.record(m.name(), input_id_for(effect.id()), m.get_for_species(&species));
                                contributed.push(m.clone());
                            }
                        }
                    }
                }
                (EffectKind::StackingRepeated, activation) => {
                    let n = stacks(activation).min(STACK_LIMIT);
                    for _ in 0..n {
                        for m in &effect.modifiers {
                            sources.record(m.name(), input_id_for(effect.id()), m.get_for_species(&species));
                            contributed.push(m.clone());
                        }
                    }
                }
                (EffectKind::Heat { heat_interpolator }, activation) => {
                    let seconds = match activation {
                        EffectActivation::Heat(s) => s,
                        _ => 0.0,
                    };
                    let ratio = quantize_heat_ratio(heat_interpolator.eval(seconds));
                    for m in &effect.modifiers {
                        let identity = modifier_identity(version, m.name())?;
                        let effective = identity + (m.get_for_species(&species) - identity) * ratio;
                        let synthesized = CrewSkillModifier::uniform(m.name(), effective);
                        sources.record(
                            synthesized.name(),
                            input_id_for(effect.id()),
                            synthesized.get_for_species(&species),
                        );
                        contributed.push(synthesized);
                    }
                }
                (EffectKind::InnateAdrenaline { breakpoints }, activation) => {
                    let hf = match activation {
                        EffectActivation::Health(h) => h.value(),
                        _ => HealthFraction::FULL.value(),
                    };
                    let innate_mods = active_innate_modifiers(breakpoints, hf, species);
                    for m in &innate_mods {
                        sources.record(m.name(), input_id_for(effect.id()), m.get_for_species(&species));
                    }
                    contributed.extend(innate_mods);
                }
                _ => {}
            }
        }

        let bundle = ModifierBundle::from_modifiers(&contributed, species, version)?;
        Ok(EffectiveModifiers {
            bundle,
            sources,
            artillery_dist_coeff: dist,
            artillery_dist_coeff_source: dist_source,
            reload_coeffs,
            reload_coeff_sources,
            version,
        })
    }
}

impl EffectiveModifiers {
    /// The ship's stat card under these effective modifiers, threading the spotter range
    /// coefficient and per-armament reload multipliers into the factories.
    pub fn stats(
        &self,
        ship: &Param,
        selection: &ShipUpgradeSelection,
        level: u32,
        provider: &dyn GameParamProvider,
    ) -> ShipStats {
        crate::game_params::ttx::orchestration::ship_stats_with(
            ship,
            selection,
            &self.bundle,
            &self.sources,
            self.reload_coeffs,
            self.artillery_dist_coeff,
            self.artillery_dist_coeff_source.clone(),
            self.reload_coeff_sources.clone(),
            level,
            self.version,
            provider,
            &mut crate::game_params::ttx::provenance::Off,
        )
    }

    /// The ship's stat card plus per-stat provenance under these effective modifiers.
    pub fn stats_explained(
        &self,
        ship: &Param,
        selection: &ShipUpgradeSelection,
        level: u32,
        provider: &dyn GameParamProvider,
    ) -> (ShipStats, crate::game_params::ttx::provenance::ShipStatsProvenance) {
        let mut rec = crate::game_params::ttx::provenance::On::default();
        let stats = crate::game_params::ttx::orchestration::ship_stats_with(
            ship,
            selection,
            &self.bundle,
            &self.sources,
            self.reload_coeffs,
            self.artillery_dist_coeff,
            self.artillery_dist_coeff_source.clone(),
            self.reload_coeff_sources.clone(),
            level,
            self.version,
            provider,
            &mut rec,
        );
        (stats, rec.into_provenance())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::HashMap;

    use super::*;
    use crate::Rc;
    use crate::game_params::types::Ability;
    use crate::game_params::types::AbilityCategory;
    use crate::game_params::types::CrewSkillLogicTrigger;
    use crate::game_params::types::CrewSkillName;
    use crate::game_params::types::CrewSkillTiers;
    use crate::game_params::types::CrewSkillType;
    use crate::game_params::types::InnateSkill;
    use crate::game_params::types::InnateSkillBreakpoint;
    use crate::game_params::types::Interpolator;
    use crate::game_params::types::ParamData;
    use crate::game_params::types::SkillPointCost;
    use crate::game_params::types::Vehicle;
    use crate::game_types::GameParamId;
    use crate::recognized::Recognized;

    struct EmptyProvider;
    impl GameParamProvider for EmptyProvider {
        fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_index(&self, _index: &str) -> Option<Rc<Param>> {
            None
        }
        fn game_param_by_name(&self, _name: &str) -> Option<Rc<Param>> {
            None
        }
        fn params(&self) -> &[Rc<Param>] {
            &[]
        }
    }

    fn tiers() -> CrewSkillTiers {
        CrewSkillTiers::builder()
            .aircraft_carrier(SkillPointCost::new(1))
            .auxiliary(SkillPointCost::new(1))
            .battleship(SkillPointCost::new(1))
            .cruiser(SkillPointCost::new(1))
            .destroyer(SkillPointCost::new(1))
            .submarine(SkillPointCost::new(1))
            .build()
    }

    fn uniform_modifier(name: &str, value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_owned())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }

    fn skill_only_modifiers(name: &str, modifiers: Vec<CrewSkillModifier>) -> CrewSkill {
        CrewSkill::builder()
            .internal_name(CrewSkillName::from(name))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::new(1))
            .ui_treat_as_trigger(false)
            .tier(tiers())
            .modifiers(modifiers)
            .build()
    }

    fn skill_with_trigger(
        name: &str,
        skill_type: u32,
        trigger_type: &str,
        trigger_mods: Vec<CrewSkillModifier>,
    ) -> CrewSkill {
        let trigger = CrewSkillLogicTrigger::builder()
            .consumable_type(String::new())
            .cooling_delay(0.0)
            .cooling_interpolator(Interpolator::default())
            .count_to_modifier(Vec::new())
            .duration(0.0)
            .energy_coeff(0.0)
            .heat_interpolator(Interpolator::default())
            .modifiers(trigger_mods)
            .trigger_desc_ids(String::new())
            .trigger_type(trigger_type.to_owned())
            .build();
        CrewSkill::builder()
            .internal_name(CrewSkillName::from(name))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::new(skill_type))
            .ui_treat_as_trigger(false)
            .tier(tiers())
            .logic_trigger(trigger)
            .build()
    }

    fn ship_no_abilities() -> Param {
        let vehicle = Vehicle::builder()
            .level(10)
            .group("g".to_string())
            .maybe_abilities(None)
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .maybe_ttx_components(None)
            .innate_skills(Vec::new())
            .build();
        Param::builder()
            .id(GameParamId::from(1u32))
            .index("IDX".to_string())
            .name("TestShip".to_string())
            .nation("USA".to_string())
            .species(Recognized::Known(Species::Destroyer))
            .data(ParamData::Vehicle(vehicle))
            .build()
    }

    #[test]
    fn health_fraction_clamps() {
        assert_eq!(HealthFraction::new(1.5).value(), 1.0);
        assert_eq!(HealthFraction::new(-0.2).value(), 0.0);
        assert_eq!(HealthFraction::new(0.5).value(), 0.5);
        assert_eq!(HealthFraction::FULL.value(), 1.0);
    }

    fn furious_skill(name: &str, blocks: Vec<(u32, Vec<CrewSkillModifier>)>) -> CrewSkill {
        let trigger = CrewSkillLogicTrigger::builder()
            .consumable_type(String::new())
            .cooling_delay(0.0)
            .cooling_interpolator(Interpolator::default())
            .count_to_modifier(blocks)
            .duration(0.0)
            .energy_coeff(0.0)
            .heat_interpolator(Interpolator::default())
            .maybe_modifiers(None)
            .trigger_desc_ids(String::new())
            .trigger_type(TRIGGER_ACTIVATION_ON_BURN_FLOOD.to_owned())
            .build();
        CrewSkill::builder()
            .internal_name(CrewSkillName::from(name))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::new(0))
            .ui_treat_as_trigger(false)
            .tier(tiers())
            .logic_trigger(trigger)
            .build()
    }

    #[test]
    fn furious_trigger_emits_stacking_per_count() {
        let ship = ship_no_abilities();
        let blocks = vec![
            (1u32, vec![uniform_modifier("GMShotDelay", 0.9)]),
            (2u32, vec![uniform_modifier("GMShotDelay", 0.95)]),
        ];
        let skill = furious_skill("TriggerBurnGmReload", blocks.clone());
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill(CrewSkillName::from("TriggerBurnGmReload")));
        assert_eq!(effects[0].kind(), &EffectKind::StackingPerCount { blocks });
    }

    #[test]
    fn potential_damage_trigger_emits_stacking_repeated() {
        let ship = ship_no_abilities();
        let skill = skill_with_trigger(
            "DefenceUw",
            0,
            "potentialDamageRatio",
            vec![uniform_modifier("regenCrewReloadCoeff", 0.992)],
        );
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill(CrewSkillName::from("DefenceUw")));
        assert_eq!(effects[0].kind(), &EffectKind::StackingRepeated);
    }

    #[test]
    fn default_activation_per_kind() {
        let mk = |kind: EffectKind| Effect::for_test(EffectId::Upgrade("PCM000".to_string()), kind, Vec::new());
        assert_eq!(mk(EffectKind::AlwaysOn).default_activation(), EffectActivation::On);
        assert_eq!(
            mk(EffectKind::Binary { condition: TriggerCondition::Outnumbered }).default_activation(),
            EffectActivation::Off
        );
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
            .set(EffectId::Skill(CrewSkillName::from("X")), EffectActivation::On)
            .set(EffectId::Consumable(Consumable::DamageControl), EffectActivation::Off);
        assert_eq!(s.get(&EffectId::Skill(CrewSkillName::from("X"))), Some(EffectActivation::On));
        assert_eq!(s.get(&EffectId::Consumable(Consumable::DamageControl)), Some(EffectActivation::Off));
        assert_eq!(s.get(&EffectId::Upgrade("PCM000".to_string())), None);
    }

    #[test]
    fn modernization_modifiers_emit_always_on() {
        let ship = ship_no_abilities();
        let mods = vec![("PCM030".to_string(), vec![uniform_modifier("GMShotDelay", 0.9)])];
        let loadout = Loadout { skills: &[], modernizations: &mods, ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Upgrade("PCM030".to_string()));
        assert_eq!(effects[0].kind(), &EffectKind::AlwaysOn);
    }

    #[test]
    fn empty_modernization_modifiers_emit_nothing() {
        let ship = ship_no_abilities();
        let loadout = Loadout { skills: &[], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert!(effects.is_empty());
    }

    #[test]
    fn skill_with_only_modifiers_emits_always_on() {
        let ship = ship_no_abilities();
        let skill = skill_only_modifiers("GunFeeder", vec![uniform_modifier("GMShotDelay", 0.9)]);
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill(CrewSkillName::from("GunFeeder")));
        assert_eq!(effects[0].kind(), &EffectKind::AlwaysOn);
    }

    #[test]
    fn non_adrenaline_trigger_emits_binary() {
        let ship = ship_no_abilities();
        let skill =
            skill_with_trigger("TriggerGmReload", 0, "triggerBattleLosing", vec![uniform_modifier("GMShotDelay", 0.8)]);
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill(CrewSkillName::from("TriggerGmReload")));
        assert_eq!(
            effects[0].kind(),
            &EffectKind::Binary { condition: TriggerCondition::Other("triggerBattleLosing".to_string()) }
        );
    }

    #[test]
    fn adrenaline_rush_trigger_emits_health_scaled_reload() {
        let ship = ship_no_abilities();
        let skill = skill_with_trigger(
            "ArmamentReloadAaDamage",
            0,
            "lifeBonus",
            vec![uniform_modifier("lastChanceReloadCoefficient_Main", 0.25)],
        );
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill(CrewSkillName::from("ArmamentReloadAaDamage")));
        assert_eq!(effects[0].kind(), &EffectKind::HealthScaledReload);
    }

    #[test]
    fn submarine_adrenaline_trigger_emits_health_scaled_reload() {
        let ship = ship_no_abilities();
        let skill = skill_with_trigger(
            "ArmamentReloadSubmarine",
            0,
            "lifeBonus",
            vec![uniform_modifier("lastChanceReloadCoefficient_Torp", 0.25)],
        );
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].kind(), &EffectKind::HealthScaledReload);
    }

    #[test]
    fn consumable_with_artillery_dist_coeff_emits_consumable_effect() {
        let ship_name = "PCY012_Scout";
        let variant = "Scout";
        let mut fields = BTreeMap::new();
        fields.insert("artilleryDistCoeff".to_string(), 1.2f32);
        let cat = AbilityCategory::builder()
            .consumable_type("scout".to_string())
            .group("ship".to_string())
            .icon_id(String::new())
            .num_consumables(2)
            .preparation_time(0.0)
            .reload_time(240.0)
            .work_time(20.0)
            .effect_fields(fields)
            .build();
        let ability = Ability::builder()
            .can_buy(false)
            .cost_credits(0)
            .cost_gold(0)
            .is_free(true)
            .categories(HashMap::from([(variant.to_string(), cat)]))
            .build();
        let ability_param = Param::builder()
            .id(GameParamId::from(2u32))
            .index("PCY012".to_string())
            .name(ship_name.to_string())
            .nation(String::new())
            .maybe_species(None)
            .data(ParamData::Ability(ability))
            .build();
        let ability_param_rc = Rc::new(ability_param);

        struct ScoutProvider(Rc<Param>);
        impl GameParamProvider for ScoutProvider {
            fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
                None
            }
            fn game_param_by_index(&self, _: &str) -> Option<Rc<Param>> {
                None
            }
            fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
                if name == "PCY012_Scout" { Some(self.0.clone()) } else { None }
            }
            fn params(&self) -> &[Rc<Param>] {
                &[]
            }
        }

        let vehicle = Vehicle::builder()
            .level(10)
            .group("g".to_string())
            .abilities(vec![vec![(ship_name.to_string(), variant.to_string())]])
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .maybe_ttx_components(None)
            .innate_skills(Vec::new())
            .build();
        let ship = Param::builder()
            .id(GameParamId::from(3u32))
            .index("SHIP".to_string())
            .name("TestShipWithScout".to_string())
            .nation("USA".to_string())
            .species(Recognized::Known(Species::Cruiser))
            .data(ParamData::Vehicle(vehicle))
            .build();

        let loadout = Loadout { skills: &[], modernizations: &[], ship: &ship };
        let effects: Vec<_> =
            loadout.effects(&ScoutProvider(ability_param_rc), test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Consumable(Consumable::SpottingAircraft));
        assert_eq!(effects[0].kind(), &EffectKind::Consumable { artillery_dist_coeff: 1.2 });
    }

    #[test]
    fn consumable_with_modifiers_and_coeff_1_emits_effect() {
        let ship_name = "PCY020_SpeedBoost";
        let variant = "SpeedBoost";
        let mut fields = BTreeMap::new();
        fields.insert("artilleryDistCoeff".to_string(), 1.0f32);
        let cat = AbilityCategory::builder()
            .consumable_type("speedBoosters".to_string())
            .group("ship".to_string())
            .icon_id(String::new())
            .num_consumables(3)
            .preparation_time(0.0)
            .reload_time(180.0)
            .work_time(120.0)
            .effect_fields(fields)
            .modifiers(vec![uniform_modifier("GMIdealRadius", 0.9)])
            .build();
        let ability = Ability::builder()
            .can_buy(false)
            .cost_credits(0)
            .cost_gold(0)
            .is_free(true)
            .categories(HashMap::from([(variant.to_string(), cat)]))
            .build();
        let ability_param = Param::builder()
            .id(GameParamId::from(6u32))
            .index("PCY020".to_string())
            .name(ship_name.to_string())
            .nation(String::new())
            .maybe_species(None)
            .data(ParamData::Ability(ability))
            .build();
        let ability_param_rc = Rc::new(ability_param);

        struct SpeedBoostProvider(Rc<Param>);
        impl GameParamProvider for SpeedBoostProvider {
            fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
                None
            }
            fn game_param_by_index(&self, _: &str) -> Option<Rc<Param>> {
                None
            }
            fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
                if name == "PCY020_SpeedBoost" { Some(self.0.clone()) } else { None }
            }
            fn params(&self) -> &[Rc<Param>] {
                &[]
            }
        }

        let vehicle = Vehicle::builder()
            .level(10)
            .group("g".to_string())
            .abilities(vec![vec![(ship_name.to_string(), variant.to_string())]])
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .maybe_ttx_components(None)
            .innate_skills(Vec::new())
            .build();
        let ship = Param::builder()
            .id(GameParamId::from(7u32))
            .index("SHIP3".to_string())
            .name("TestShipWithSpeedBoost".to_string())
            .nation("USA".to_string())
            .species(Recognized::Known(Species::Destroyer))
            .data(ParamData::Vehicle(vehicle))
            .build();

        let loadout = Loadout { skills: &[], modernizations: &[], ship: &ship };
        let effects: Vec<_> =
            loadout.effects(&SpeedBoostProvider(ability_param_rc), test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1, "non-empty modifiers should emit a consumable effect even with coeff==1.0");
        assert_eq!(effects[0].id(), &EffectId::Consumable(Consumable::SpeedBoost));
        assert_eq!(effects[0].kind(), &EffectKind::Consumable { artillery_dist_coeff: 1.0 });
    }

    #[test]
    fn consumable_with_dist_coeff_1_and_no_modifiers_emits_nothing() {
        let mut fields = BTreeMap::new();
        fields.insert("artilleryDistCoeff".to_string(), 1.0f32);
        let cat = AbilityCategory::builder()
            .consumable_type("crashCrew".to_string())
            .group("ship".to_string())
            .icon_id(String::new())
            .num_consumables(-1)
            .preparation_time(0.0)
            .reload_time(120.0)
            .work_time(15.0)
            .effect_fields(fields)
            .build();
        let ability = Ability::builder()
            .can_buy(false)
            .cost_credits(0)
            .cost_gold(0)
            .is_free(true)
            .categories(HashMap::from([("CrashCrew".to_string(), cat)]))
            .build();
        let ability_param = Param::builder()
            .id(GameParamId::from(4u32))
            .index("PCY001".to_string())
            .name("PCY001_CrashCrew".to_string())
            .nation(String::new())
            .maybe_species(None)
            .data(ParamData::Ability(ability))
            .build();
        let ability_param_rc = Rc::new(ability_param);

        struct CrashCrewProvider(Rc<Param>);
        impl GameParamProvider for CrashCrewProvider {
            fn game_param_by_id(&self, _id: GameParamId) -> Option<Rc<Param>> {
                None
            }
            fn game_param_by_index(&self, _: &str) -> Option<Rc<Param>> {
                None
            }
            fn game_param_by_name(&self, name: &str) -> Option<Rc<Param>> {
                if name == "PCY001_CrashCrew" { Some(self.0.clone()) } else { None }
            }
            fn params(&self) -> &[Rc<Param>] {
                &[]
            }
        }

        let vehicle = Vehicle::builder()
            .level(10)
            .group("g".to_string())
            .abilities(vec![vec![("PCY001_CrashCrew".to_string(), "CrashCrew".to_string())]])
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .maybe_ttx_components(None)
            .innate_skills(Vec::new())
            .build();
        let ship = Param::builder()
            .id(GameParamId::from(5u32))
            .index("SHIP2".to_string())
            .name("TestShipWithCC".to_string())
            .nation("USA".to_string())
            .species(Recognized::Known(Species::Destroyer))
            .data(ParamData::Vehicle(vehicle))
            .build();

        let loadout = Loadout { skills: &[], modernizations: &[], ship: &ship };
        let effects: Vec<_> =
            loadout.effects(&CrashCrewProvider(ability_param_rc), test_version()).iter().cloned().collect();
        assert!(effects.is_empty(), "crashCrew with dist_coeff=1.0 and no modifiers should emit no effect");
    }

    #[test]
    fn default_activation_stacking_kinds() {
        let per_count = Effect::for_test(
            EffectId::Skill("Furious".into()),
            EffectKind::StackingPerCount { blocks: Vec::new() },
            Vec::new(),
        );
        assert_eq!(per_count.default_activation(), EffectActivation::Stacks(0));

        let repeated = Effect::for_test(EffectId::Skill("DefenceUw".into()), EffectKind::StackingRepeated, Vec::new());
        assert_eq!(repeated.default_activation(), EffectActivation::Stacks(0));
    }

    fn atba_heat_skill(name: &str, points: Vec<(f32, f32)>, modifiers: Vec<CrewSkillModifier>) -> CrewSkill {
        let trigger = CrewSkillLogicTrigger::builder()
            .consumable_type(String::new())
            .cooling_delay(15.0)
            .cooling_interpolator(Interpolator::default())
            .count_to_modifier(Vec::new())
            .duration(0.0)
            .energy_coeff(0.0)
            .heat_interpolator(Interpolator::from_points(points))
            .modifiers(modifiers)
            .trigger_desc_ids(String::new())
            .trigger_type("atbaHeat".to_owned())
            .build();
        CrewSkill::builder()
            .internal_name(CrewSkillName::from(name))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::new(0))
            .ui_treat_as_trigger(false)
            .tier(tiers())
            .logic_trigger(trigger)
            .build()
    }

    #[test]
    fn default_activation_heat_kind() {
        let effect = Effect::for_test(
            EffectId::Skill("AtbaAccuracy".into()),
            EffectKind::Heat { heat_interpolator: Interpolator::from_points(vec![(0.0, 0.0), (45.0, 1.0)]) },
            Vec::new(),
        );
        assert_eq!(effect.default_activation(), EffectActivation::Heat(0.0));
    }

    #[test]
    fn atba_heat_trigger_emits_heat_effect() {
        let ship = ship_no_abilities();
        let points = vec![(0.0, 0.0), (10.0, 0.5), (45.0, 1.0)];
        let skill =
            atba_heat_skill("AtbaAccuracy", points.clone(), vec![uniform_modifier("GSPriorityTargetIdealRadius", 0.5)]);
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill(CrewSkillName::from("AtbaAccuracy")));
        assert_eq!(effects[0].kind(), &EffectKind::Heat { heat_interpolator: Interpolator::from_points(points) });
    }

    fn test_version() -> crate::data::Version {
        crate::data::Version::base(15, 4, 0)
    }

    #[test]
    fn resolve_always_on_always_contributes() {
        let effect = Effect::for_test(
            EffectId::Upgrade("PCM030".to_string()),
            EffectKind::AlwaysOn,
            vec![uniform_modifier("GMShotDelay", 0.9)],
        );
        let effects = Effects(vec![effect]);
        let state = EffectsState::default();
        let result = effects.resolve(&state, Species::Cruiser, test_version()).unwrap();
        assert!((result.bundle().coef("GMShotDelay") - 0.9).abs() < 1e-6);
    }

    #[test]
    fn resolve_binary_off_omits_on_includes() {
        let effect = Effect::for_test(
            EffectId::Skill("Outnumbered".into()),
            EffectKind::Binary { condition: TriggerCondition::Outnumbered },
            vec![uniform_modifier("GMIdealRadius", 0.85)],
        );
        let effects = Effects(vec![effect.clone()]);

        let state_off = EffectsState::default();
        let result_off = effects.resolve(&state_off, Species::Cruiser, test_version()).unwrap();
        assert!((result_off.bundle().coef("GMIdealRadius") - 1.0).abs() < 1e-6);

        let state_on = EffectsState::default().set(EffectId::Skill("Outnumbered".into()), EffectActivation::On);
        let result_on = effects.resolve(&state_on, Species::Cruiser, test_version()).unwrap();
        assert!((result_on.bundle().coef("GMIdealRadius") - 0.85).abs() < 1e-6);

        assert!(
            result_on.bundle().coef("GMIdealRadius") != result_off.bundle().coef("GMIdealRadius"),
            "binary on/off must differ"
        );
    }

    #[test]
    fn resolve_adrenaline_full_hp_is_identity() {
        let raw = 0.2f32;
        let effect = Effect::for_test(
            EffectId::Skill("ArmamentReloadAaDamage".into()),
            EffectKind::HealthScaledReload,
            vec![uniform_modifier("lastChanceReloadCoefficient_Main", raw)],
        );
        let effects = Effects(vec![effect]);

        let state_full = EffectsState::default();
        let result = effects.resolve(&state_full, Species::Cruiser, test_version()).unwrap();
        assert!((result.reload_coeffs().main - 1.0).abs() < 1e-6, "at full HP reload_coeffs.main == 1.0");
    }

    #[test]
    fn resolve_adrenaline_half_hp() {
        let raw = 0.2f32;
        let effect = Effect::for_test(
            EffectId::Skill("ArmamentReloadAaDamage".into()),
            EffectKind::HealthScaledReload,
            vec![uniform_modifier("lastChanceReloadCoefficient_Main", raw)],
        );
        let effects = Effects(vec![effect]);

        let state_half = EffectsState::default()
            .set(EffectId::Skill("ArmamentReloadAaDamage".into()), EffectActivation::Health(HealthFraction::new(0.5)));
        let result = effects.resolve(&state_half, Species::Cruiser, test_version()).unwrap();
        let expected = 1.0 - 0.5 * raw;
        assert!((result.reload_coeffs().main - expected).abs() < 1e-6, "at 50% HP reload_coeffs.main == {expected}");
    }

    #[test]
    fn resolve_consumable_on_applies_dist_coeff() {
        let effect = Effect::for_test(
            EffectId::Consumable(Consumable::SpottingAircraft),
            EffectKind::Consumable { artillery_dist_coeff: 1.2 },
            vec![],
        );
        let effects = Effects(vec![effect]);

        let state_off = EffectsState::default();
        let result_off = effects.resolve(&state_off, Species::Cruiser, test_version()).unwrap();
        assert!((result_off.artillery_dist_coeff() - 1.0).abs() < 1e-6, "off -> dist 1.0");

        let state_on =
            EffectsState::default().set(EffectId::Consumable(Consumable::SpottingAircraft), EffectActivation::On);
        let result_on = effects.resolve(&state_on, Species::Cruiser, test_version()).unwrap();
        assert!((result_on.artillery_dist_coeff() - 1.2).abs() < 1e-6, "on -> dist 1.2");
    }

    #[test]
    fn resolve_stacking_per_count_accumulates() {
        let blocks = vec![
            (1u32, vec![uniform_modifier("GMShotDelay", 0.9)]),
            (2u32, vec![uniform_modifier("GMShotDelay", 0.95)]),
            (3u32, vec![uniform_modifier("GMShotDelay", 0.95)]),
            (4u32, vec![uniform_modifier("GMShotDelay", 0.95)]),
            (5u32, vec![uniform_modifier("GMShotDelay", 0.95)]),
            (6u32, vec![uniform_modifier("GMShotDelay", 0.95)]),
        ];
        let effect =
            Effect::for_test(EffectId::Skill("Furious".into()), EffectKind::StackingPerCount { blocks }, Vec::new());
        let effects = Effects::for_test(vec![effect]);
        let id = EffectId::Skill("Furious".into());

        let r0 = effects.resolve(&EffectsState::default(), Species::Cruiser, test_version()).unwrap();
        assert!((r0.bundle().coef("GMShotDelay") - 1.0).abs() < 1e-6, "Stacks(0) -> identity");

        let r1 = effects
            .resolve(
                &EffectsState::default().set(id.clone(), EffectActivation::Stacks(1)),
                Species::Cruiser,
                test_version(),
            )
            .unwrap();
        assert!((r1.bundle().coef("GMShotDelay") - 0.9).abs() < 1e-6, "Stacks(1) -> 0.9");

        let r2 = effects
            .resolve(
                &EffectsState::default().set(id.clone(), EffectActivation::Stacks(2)),
                Species::Cruiser,
                test_version(),
            )
            .unwrap();
        assert!((r2.bundle().coef("GMShotDelay") - 0.9 * 0.95).abs() < 1e-6, "Stacks(2) -> 0.9*0.95");

        let r10 = effects
            .resolve(&EffectsState::default().set(id, EffectActivation::Stacks(10)), Species::Cruiser, test_version())
            .unwrap();
        let expected = 0.9 * 0.95f32.powi(5);
        assert!((r10.bundle().coef("GMShotDelay") - expected).abs() < 1e-5, "Stacks(10) capped at 6");
    }

    #[test]
    fn resolve_stacking_repeated_multiplies_and_clamps() {
        let effect = Effect::for_test(
            EffectId::Skill("DefenceUw".into()),
            EffectKind::StackingRepeated,
            vec![uniform_modifier("regenCrewReloadCoeff", 0.992)],
        );
        let effects = Effects::for_test(vec![effect]);
        let id = EffectId::Skill("DefenceUw".into());

        let r0 = effects.resolve(&EffectsState::default(), Species::Cruiser, test_version()).unwrap();
        assert!((r0.bundle().coef("regenCrewReloadCoeff") - 1.0).abs() < 1e-6, "Stacks(0) -> identity");

        let r3 = effects
            .resolve(
                &EffectsState::default().set(id.clone(), EffectActivation::Stacks(3)),
                Species::Cruiser,
                test_version(),
            )
            .unwrap();
        assert!((r3.bundle().coef("regenCrewReloadCoeff") - 0.992f32.powi(3)).abs() < 1e-6, "Stacks(3) -> 0.992^3");

        let r_big = effects
            .resolve(
                &EffectsState::default().set(id.clone(), EffectActivation::Stacks(2000)),
                Species::Cruiser,
                test_version(),
            )
            .unwrap();
        let r_cap = effects
            .resolve(&EffectsState::default().set(id, EffectActivation::Stacks(1023)), Species::Cruiser, test_version())
            .unwrap();
        assert_eq!(
            r_big.bundle().coef("regenCrewReloadCoeff"),
            r_cap.bundle().coef("regenCrewReloadCoeff"),
            "Stacks past the cap equals Stacks(1023)"
        );
    }

    #[test]
    fn crew_skill_modifier_uniform_is_species_uniform() {
        let m = CrewSkillModifier::uniform("GSPriorityTargetIdealRadius", 0.75);
        assert_eq!(m.name(), "GSPriorityTargetIdealRadius");
        assert!((m.get_for_species(&Species::Cruiser) - 0.75).abs() < 1e-6);
        assert!((m.get_for_species(&Species::Battleship) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn resolve_heat_lerps_and_quantizes() {
        let points = vec![(0.0, 0.0), (10.0, 0.5), (45.0, 1.0)];
        let effect = Effect::for_test(
            EffectId::Skill("AtbaAccuracy".into()),
            EffectKind::Heat { heat_interpolator: Interpolator::from_points(points) },
            vec![uniform_modifier("GSPriorityTargetIdealRadius", 0.5)],
        );
        let effects = Effects::for_test(vec![effect]);
        let id = EffectId::Skill("AtbaAccuracy".into());
        let coef = |state: &EffectsState| {
            effects
                .resolve(state, Species::Cruiser, test_version())
                .unwrap()
                .bundle()
                .coef("GSPriorityTargetIdealRadius")
        };

        assert!((coef(&EffectsState::default()) - 1.0).abs() < 1e-6, "Heat(0) default -> identity");
        let at = |s: f32| EffectsState::default().set(id.clone(), EffectActivation::Heat(s));
        assert!((coef(&at(10.0)) - 0.75).abs() < 1e-6, "ratio 0.5 -> lerp(1,0.5,0.5)=0.75");
        assert!((coef(&at(45.0)) - 0.5).abs() < 1e-6, "ratio 1.0 -> 0.5");
        assert!((coef(&at(100.0)) - 0.5).abs() < 1e-6, "clamped -> 0.5");
        // 6.667s -> continuous ratio 0.33335, quantized floor to 0.33 -> lerp(1,0.5,0.33)=0.835,
        // distinct from the continuous 0.833325.
        assert!((coef(&at(6.667)) - 0.835).abs() < 1e-4, "1% quantization applied");
    }

    fn innate_breakpoint(hf: f32, ideal: f32, shot_delay: f32) -> InnateSkillBreakpoint {
        InnateSkillBreakpoint::new(
            hf,
            vec![uniform_modifier("GMIdealRadius", ideal), uniform_modifier("GMShotDelay", shot_delay)],
        )
    }

    fn oregon_breakpoints() -> Vec<InnateSkillBreakpoint> {
        vec![innate_breakpoint(1.0, 1.0, 1.0), innate_breakpoint(0.5, 0.83, 0.9), innate_breakpoint(0.25, 0.77, 0.85)]
    }

    fn ship_with_innate() -> Param {
        let skill = InnateSkill::new("adrenalineRush".to_owned(), oregon_breakpoints());
        let vehicle = Vehicle::builder()
            .level(10)
            .group("g".to_string())
            .maybe_abilities(None)
            .upgrades(Vec::new())
            .maybe_config_data(None)
            .maybe_model_path(None)
            .maybe_armor(None)
            .maybe_hit_locations(None)
            .permoflages(Vec::new())
            .camera_trajectories(Vec::new())
            .maybe_ttx_components(None)
            .innate_skills(vec![skill])
            .build();
        Param::builder()
            .id(GameParamId::from(20u32))
            .index("OREG".to_string())
            .name("TestOregon".to_string())
            .nation("USA".to_string())
            .species(Recognized::Known(Species::Battleship))
            .data(ParamData::Vehicle(vehicle))
            .build()
    }

    #[test]
    fn default_activation_innate_adrenaline() {
        let effect = Effect::for_test(
            EffectId::Innate("adrenalineRush".into()),
            EffectKind::InnateAdrenaline { breakpoints: oregon_breakpoints() },
            Vec::new(),
        );
        assert_eq!(effect.default_activation(), EffectActivation::Health(HealthFraction::FULL));
    }

    #[test]
    fn innate_skill_emits_innate_adrenaline() {
        let ship = ship_with_innate();
        let loadout = Loadout { skills: &[], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Innate("adrenalineRush".to_owned()));
        assert_eq!(effects[0].kind(), &EffectKind::InnateAdrenaline { breakpoints: oregon_breakpoints() });
    }

    #[test]
    fn ship_without_innate_emits_none() {
        let ship = ship_no_abilities();
        let loadout = Loadout { skills: &[], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert!(effects.is_empty());
    }

    #[test]
    fn active_innate_modifiers_clamp_and_lerp() {
        let bps = oregon_breakpoints();
        let coef = |hf: f32, name: &str| {
            active_innate_modifiers(&bps, hf, Species::Battleship)
                .iter()
                .find(|m| m.name() == name)
                .unwrap()
                .get_for_species(&Species::Battleship)
        };

        assert!((coef(1.0, "GMShotDelay") - 1.0).abs() < 1e-6, "full HP -> identity");
        assert!((coef(1.0, "GMIdealRadius") - 1.0).abs() < 1e-6);
        assert!((coef(0.5, "GMShotDelay") - 0.9).abs() < 1e-6, "exact half breakpoint");
        assert!((coef(0.5, "GMIdealRadius") - 0.83).abs() < 1e-6);
        assert!((coef(0.25, "GMShotDelay") - 0.85).abs() < 1e-6, "quarter breakpoint");
        assert!((coef(0.1, "GMShotDelay") - 0.85).abs() < 1e-6, "below lowest clamps to quarter");
        assert!((coef(0.75, "GMShotDelay") - 0.95).abs() < 1e-6, "lerp t=0.5 -> 1.0..0.9");
        assert!((coef(0.75, "GMIdealRadius") - 0.915).abs() < 1e-6, "lerp t=0.5 -> 1.0..0.83");
    }

    #[test]
    fn resolve_innate_adrenaline() {
        let effect = Effect::for_test(
            EffectId::Innate("adrenalineRush".into()),
            EffectKind::InnateAdrenaline { breakpoints: oregon_breakpoints() },
            Vec::new(),
        );
        let effects = Effects::for_test(vec![effect]);
        let id = EffectId::Innate("adrenalineRush".into());
        let coef = |state: &EffectsState, name: &str| {
            effects.resolve(state, Species::Battleship, test_version()).unwrap().bundle().coef(name)
        };

        // Default Health(FULL) -> identity.
        assert!((coef(&EffectsState::default(), "GMShotDelay") - 1.0).abs() < 1e-6);
        assert!((coef(&EffectsState::default(), "GMIdealRadius") - 1.0).abs() < 1e-6);

        let half = EffectsState::default().set(id.clone(), EffectActivation::Health(HealthFraction::new(0.5)));
        assert!((coef(&half, "GMShotDelay") - 0.9).abs() < 1e-6);
        assert!((coef(&half, "GMIdealRadius") - 0.83).abs() < 1e-6);

        let three_q = EffectsState::default().set(id, EffectActivation::Health(HealthFraction::new(0.75)));
        assert!((coef(&three_q, "GMShotDelay") - 0.95).abs() < 1e-6);
        assert!((coef(&three_q, "GMIdealRadius") - 0.915).abs() < 1e-6);
    }

    #[test]
    fn situation_facts_default_is_stock() {
        let f = SituationFacts::default();
        assert_eq!(f.hp_fraction, 1.0);
        assert_eq!(f.seconds_since_detected, None);
        assert_eq!(f.burn_flood_count, 0);
        assert!(!f.detected && !f.outnumbered && f.active_consumables.is_empty());
    }

    #[test]
    fn trigger_condition_holds_maps_facts() {
        let mut f = SituationFacts::default();
        assert!(TriggerCondition::Undetected.holds(&f));
        assert!(!TriggerCondition::Detected.holds(&f));
        f.detected = true;
        assert!(TriggerCondition::Detected.holds(&f));
        assert!(!TriggerCondition::Undetected.holds(&f));

        f.outnumbered = true;
        assert!(TriggerCondition::Outnumbered.holds(&f));

        f.active_consumables = vec![Consumable::Hydrophone];
        assert!(TriggerCondition::OnConsumableActive(Consumable::Hydrophone).holds(&f));
        assert!(!TriggerCondition::OnConsumableActive(Consumable::Radar).holds(&f));

        f.seconds_since_detected = Some(10.0);
        assert!(TriggerCondition::OnDetected { duration: 15.0 }.holds(&f));
        f.seconds_since_detected = Some(20.0);
        assert!(!TriggerCondition::OnDetected { duration: 15.0 }.holds(&f));
        f.seconds_since_detected = None;
        assert!(!TriggerCondition::OnDetected { duration: 15.0 }.holds(&f));

        assert!(!TriggerCondition::Other("x".to_string()).holds(&SituationFacts::default()));
    }

    #[test]
    fn trigger_condition_from_trigger_type_classifies() {
        let v = test_version();
        let mk = |tt: &str| {
            let s = skill_with_trigger("S", 0, tt, vec![uniform_modifier("speedCoef", 1.08)]);
            TriggerCondition::from_trigger_type(s.logic_trigger().unwrap(), v)
        };
        assert_eq!(mk("entityIsVisibleTrigger"), Some(TriggerCondition::Detected));
        assert_eq!(mk("entityIsInvisibleTrigger"), Some(TriggerCondition::Undetected));
        assert_eq!(mk("EnemiesNotLessThanAlliesWithinGMTrigger"), Some(TriggerCondition::Outnumbered));
        assert_eq!(mk("activeAirDefense"), Some(TriggerCondition::ActiveAirDefense));
        assert_eq!(mk("activationOnBattery"), Some(TriggerCondition::SubmarineBatteryLow));
        assert_eq!(mk("somethingNewUnmodeled"), Some(TriggerCondition::Other("somethingNewUnmodeled".to_string())));
    }

    #[test]
    fn trigger_condition_on_detect_carries_duration() {
        let trigger = CrewSkillLogicTrigger::builder()
            .consumable_type(String::new())
            .cooling_delay(0.0)
            .cooling_interpolator(Interpolator::default())
            .count_to_modifier(Vec::new())
            .duration(15.0)
            .energy_coeff(0.0)
            .heat_interpolator(Interpolator::default())
            .modifiers(vec![uniform_modifier("shootShift", 1.2)])
            .trigger_desc_ids(String::new())
            .trigger_type("activationOnDetectTrigger".to_owned())
            .build();
        assert_eq!(
            TriggerCondition::from_trigger_type(&trigger, test_version()),
            Some(TriggerCondition::OnDetected { duration: 15.0 })
        );
    }

    #[test]
    fn trigger_condition_on_consumable_carries_type() {
        let trigger = CrewSkillLogicTrigger::builder()
            .consumable_type("hydrophone".to_owned())
            .cooling_delay(0.0)
            .cooling_interpolator(Interpolator::default())
            .count_to_modifier(Vec::new())
            .duration(30.0)
            .energy_coeff(0.0)
            .heat_interpolator(Interpolator::default())
            .modifiers(vec![uniform_modifier("firstSectorTimeCoeff", 1.25)])
            .trigger_desc_ids(String::new())
            .trigger_type("activationOnConsumable".to_owned())
            .build();
        assert_eq!(
            TriggerCondition::from_trigger_type(&trigger, test_version()),
            Some(TriggerCondition::OnConsumableActive(Consumable::Hydrophone))
        );
    }

    #[test]
    fn binary_trigger_emits_condition() {
        let ship = ship_no_abilities();
        let skill = skill_with_trigger(
            "Outnum",
            0,
            "EnemiesNotLessThanAlliesWithinGMTrigger",
            vec![uniform_modifier("speedCoef", 1.08)],
        );
        let loadout = Loadout { skills: &[skill], modernizations: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider, test_version()).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].kind(), &EffectKind::Binary { condition: TriggerCondition::Outnumbered });
    }

    #[test]
    fn situation_state_default_reproduces_stock() {
        let modern = Effect::for_test(
            EffectId::Upgrade("PCM030".to_string()),
            EffectKind::AlwaysOn,
            vec![uniform_modifier("GMShotDelay", 0.9)],
        );
        let adren = Effect::for_test(
            EffectId::Skill(CrewSkillName::from("Adren")),
            EffectKind::HealthScaledReload,
            vec![uniform_modifier("lastChanceReloadCoefficient_Main", 0.2)],
        );
        let furious = Effect::for_test(
            EffectId::Skill(CrewSkillName::from("Furious")),
            EffectKind::StackingPerCount { blocks: vec![(1, vec![uniform_modifier("GMShotDelay", 0.9)])] },
            Vec::new(),
        );
        let effects = Effects::for_test(vec![modern, adren, furious]);

        let state = effects.situation_state(&SituationFacts::default());
        let r = effects.resolve(&state, Species::Cruiser, test_version()).unwrap();
        assert!((r.bundle().coef("GMShotDelay") - 0.9).abs() < 1e-6, "only the always-on modifier");
        assert!((r.reload_coeffs().main - 1.0).abs() < 1e-6, "adrenaline identity at full HP");
    }

    #[test]
    fn situation_state_sets_each_kind() {
        let furious = Effect::for_test(
            EffectId::Skill(CrewSkillName::from("Furious")),
            EffectKind::StackingPerCount {
                blocks: vec![
                    (1, vec![uniform_modifier("GMShotDelay", 0.9)]),
                    (2, vec![uniform_modifier("GMShotDelay", 0.95)]),
                ],
            },
            Vec::new(),
        );
        let outnum = Effect::for_test(
            EffectId::Skill(CrewSkillName::from("Outnum")),
            EffectKind::Binary { condition: TriggerCondition::Outnumbered },
            vec![uniform_modifier("GMIdealRadius", 0.9)],
        );
        let scout = Effect::for_test(
            EffectId::Consumable(Consumable::SpottingAircraft),
            EffectKind::Consumable { artillery_dist_coeff: 1.2 },
            Vec::new(),
        );
        let effects = Effects::for_test(vec![furious, outnum, scout]);

        let facts = SituationFacts {
            burn_flood_count: 2,
            outnumbered: true,
            active_consumables: vec![Consumable::SpottingAircraft],
            ..Default::default()
        };
        let state = effects.situation_state(&facts);

        assert_eq!(state.get(&EffectId::Skill(CrewSkillName::from("Furious"))), Some(EffectActivation::Stacks(2)));
        assert_eq!(state.get(&EffectId::Skill(CrewSkillName::from("Outnum"))), Some(EffectActivation::On));
        assert_eq!(state.get(&EffectId::Consumable(Consumable::SpottingAircraft)), Some(EffectActivation::On));
    }

    #[test]
    fn situation_state_potential_damage_stacks() {
        let pot = Effect::for_test(
            EffectId::Skill(CrewSkillName::from("Pot")),
            EffectKind::StackingRepeated,
            vec![uniform_modifier("regenCrewReloadCoeff", 0.992)],
        );
        let effects = Effects::for_test(vec![pot]);
        let facts = SituationFacts { potential_damage: 2.0e6, max_health: 1.0e5, ..Default::default() };
        let state = effects.situation_state(&facts);
        assert_eq!(state.get(&EffectId::Skill(CrewSkillName::from("Pot"))), Some(EffectActivation::Stacks(20)));
    }

    #[test]
    fn resolve_mixed_state_folds_correctly() {
        let always_on = Effect::for_test(
            EffectId::Upgrade("PCM030".to_string()),
            EffectKind::AlwaysOn,
            vec![uniform_modifier("GMShotDelay", 0.9)],
        );
        let binary = Effect::for_test(
            EffectId::Skill("Outnumbered".into()),
            EffectKind::Binary { condition: TriggerCondition::Outnumbered },
            vec![uniform_modifier("GMIdealRadius", 0.85)],
        );
        let adrenaline = Effect::for_test(
            EffectId::Skill("ArmamentReloadAaDamage".into()),
            EffectKind::HealthScaledReload,
            vec![uniform_modifier("lastChanceReloadCoefficient_Main", 0.2)],
        );
        let consumable = Effect::for_test(
            EffectId::Consumable(Consumable::SpottingAircraft),
            EffectKind::Consumable { artillery_dist_coeff: 1.2 },
            vec![uniform_modifier("GMIdealRadius", 0.95)],
        );

        let effects = Effects(vec![always_on, binary, adrenaline, consumable]);
        let state = EffectsState::default()
            .set(EffectId::Skill("Outnumbered".into()), EffectActivation::On)
            .set(EffectId::Skill("ArmamentReloadAaDamage".into()), EffectActivation::Health(HealthFraction::new(0.5)))
            .set(EffectId::Consumable(Consumable::SpottingAircraft), EffectActivation::On);

        let result = effects.resolve(&state, Species::Cruiser, test_version()).unwrap();

        assert!((result.bundle().coef("GMShotDelay") - 0.9).abs() < 1e-6, "always-on GMShotDelay");
        assert!(
            (result.bundle().coef("GMIdealRadius") - (0.85 * 0.95)).abs() < 1e-5,
            "binary + consumable GMIdealRadius"
        );
        assert!((result.reload_coeffs().main - 0.9).abs() < 1e-6, "adrenaline at 50% HP");
        assert!((result.artillery_dist_coeff() - 1.2).abs() < 1e-6, "consumable dist coeff");
    }

    #[test]
    fn resolve_tags_modifier_sources_per_upgrade() {
        use crate::game_params::ttx::provenance::InputId;
        let ship = ship_no_abilities();
        let mods = vec![("PCM030".to_string(), vec![uniform_modifier("GMShotDelay", 0.9)])];
        let loadout = Loadout { skills: &[], modernizations: &mods, ship: &ship };
        let effects = loadout.effects(&EmptyProvider, test_version());
        let em = effects.resolve(&EffectsState::default(), Species::Destroyer, test_version()).unwrap();
        let srcs = em.sources().get("GMShotDelay");
        assert_eq!(srcs.len(), 1);
        assert_eq!(srcs[0].0, InputId::Upgrade { name: "PCM030".to_string() });
        assert!((srcs[0].1 - 0.9).abs() < 1e-6);
        // Bundle value is unchanged by the side-channel.
        assert!((em.bundle().coef("GMShotDelay") - 0.9).abs() < 1e-6);
    }
}
