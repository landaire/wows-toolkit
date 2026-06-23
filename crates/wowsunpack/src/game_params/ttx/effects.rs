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
use crate::game_params::types::KnownCrewSkill;
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

/// How an effect is activated. `Off` / `On` / `Health` / `Stacks`; `Stacks(n)` drives the
/// stacking triggers (Furious burn+flood count, potential-damage health-multiplier).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EffectActivation {
    Off,
    On,
    Health(HealthFraction),
    Stacks(u32),
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
    /// Furious (`activationOnBurnFlood`): at `Stacks(n)`, contribute every per-count block
    /// with `count <= n` (accumulative). Capped at the highest count present.
    StackingPerCount { blocks: Vec<(u32, Vec<CrewSkillModifier>)> },
    /// Potential-damage (`potentialDamageRatio`): at `Stacks(n)`, contribute `Effect.modifiers`
    /// `n` times (the modifier folds multiplicatively to `^n`).
    StackingRepeated,
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
    /// `AlwaysOn -> On`, `Binary`/`Consumable -> Off`, `HealthScaledReload -> Health(FULL)`,
    /// `StackingPerCount`/`StackingRepeated -> Stacks(0)`.
    pub fn default_activation(&self) -> EffectActivation {
        match self.kind {
            EffectKind::AlwaysOn => EffectActivation::On,
            EffectKind::Binary | EffectKind::Consumable { .. } => EffectActivation::Off,
            EffectKind::HealthScaledReload => EffectActivation::Health(HealthFraction::FULL),
            EffectKind::StackingPerCount { .. } | EffectKind::StackingRepeated => EffectActivation::Stacks(0),
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

/// The resolved modifiers for a state: the aggregated static bundle, the consumable
/// artillery range coefficient, and the dynamic per-armament reload multipliers -- all
/// applied by the artillery/torpedo factories.
pub struct EffectiveModifiers {
    bundle: ModifierBundle,
    artillery_dist_coeff: f32,
    reload_coeffs: ReloadCoeffs,
}

impl EffectiveModifiers {
    pub fn bundle(&self) -> &ModifierBundle {
        &self.bundle
    }
    pub fn artillery_dist_coeff(&self) -> f32 {
        self.artillery_dist_coeff
    }
    pub fn reload_coeffs(&self) -> ReloadCoeffs {
        self.reload_coeffs
    }
}

impl Loadout<'_> {
    pub fn effects(&self, provider: &dyn GameParamProvider) -> Effects {
        let mut effects: Vec<Effect> = Vec::new();

        if !self.modernization_modifiers.is_empty() {
            effects.push(Effect {
                id: EffectId::Modernizations,
                kind: EffectKind::AlwaysOn,
                modifiers: self.modernization_modifiers.to_vec(),
            });
        }

        for skill in self.skills {
            let name = skill.internal_name().as_str().to_owned();
            if let Some(mods) = skill.modifiers().filter(|m| !m.is_empty()) {
                effects.push(Effect {
                    id: EffectId::Skill(name.clone()),
                    kind: EffectKind::AlwaysOn,
                    modifiers: mods.clone(),
                });
            }
            if let Some(trigger) = skill.logic_trigger()
                && let Some(tmods) = trigger.modifiers().filter(|m| !m.is_empty())
            {
                let recognized = KnownCrewSkill::recognize(skill.internal_name(), skill.skill_type());
                let kind = match recognized.known() {
                    Some(KnownCrewSkill::AdrenalineRush) | Some(KnownCrewSkill::SubmarineAdrenalineRush) => {
                        EffectKind::HealthScaledReload
                    }
                    _ => EffectKind::Binary,
                };
                effects.push(Effect { id: EffectId::Skill(name), kind, modifiers: tmods.clone() });
            }
        }

        if let Some(vehicle) = self.ship.vehicle()
            && let Some(ability_slots) = vehicle.abilities()
        {
            for slot in ability_slots {
                for (ability_name, variant_name) in slot {
                    let param = match provider.game_param_by_name(ability_name) {
                        Some(p) => p,
                        None => continue,
                    };
                    let ability = match param.ability() {
                        Some(a) => a,
                        None => continue,
                    };
                    let cat = match ability.get_category(variant_name) {
                        Some(c) => c,
                        None => continue,
                    };
                    let artillery_dist_coeff = cat.effect_fields().get("artilleryDistCoeff").copied().unwrap_or(1.0);
                    let cat_modifiers = cat.modifiers();
                    if artillery_dist_coeff != 1.0 || !cat_modifiers.is_empty() {
                        effects.push(Effect {
                            id: EffectId::Consumable(ability_name.clone()),
                            kind: EffectKind::Consumable { artillery_dist_coeff },
                            modifiers: cat_modifiers.to_vec(),
                        });
                    }
                }
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

    pub fn resolve(
        &self,
        state: &EffectsState,
        species: Species,
        version: Version,
    ) -> Result<EffectiveModifiers, ModifierError> {
        const EPSILON: f32 = 1e-6;
        let mut contributed: Vec<CrewSkillModifier> = Vec::new();
        let mut dist: f32 = 1.0;
        let mut reload_coeffs = ReloadCoeffs::default();

        for effect in &self.0 {
            let activation = state.get(effect.id()).unwrap_or_else(|| effect.default_activation());
            match (&effect.kind, activation) {
                (EffectKind::AlwaysOn, _) => {
                    contributed.extend(effect.modifiers.iter().cloned());
                }
                (EffectKind::Binary, EffectActivation::On) => {
                    contributed.extend(effect.modifiers.iter().cloned());
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
                            match suffix {
                                "Main" => reload_coeffs.main *= mul,
                                "Sec" => reload_coeffs.secondary *= mul,
                                "Torp" => reload_coeffs.torpedo *= mul,
                                _ => {}
                            }
                        }
                    }
                }
                (EffectKind::Consumable { artillery_dist_coeff }, EffectActivation::On) => {
                    contributed.extend(effect.modifiers.iter().cloned());
                    dist *= artillery_dist_coeff;
                }
                _ => {}
            }
        }

        let bundle = ModifierBundle::from_modifiers(&contributed, species, version)?;
        Ok(EffectiveModifiers { bundle, artillery_dist_coeff: dist, reload_coeffs })
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
            self.reload_coeffs,
            self.artillery_dist_coeff,
            level,
            provider,
        )
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

    #[test]
    fn modernization_modifiers_emit_always_on() {
        let ship = ship_no_abilities();
        let mods = vec![uniform_modifier("GMShotDelay", 0.9)];
        let loadout = Loadout { skills: &[], modernization_modifiers: &mods, ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Modernizations);
        assert_eq!(effects[0].kind(), &EffectKind::AlwaysOn);
    }

    #[test]
    fn empty_modernization_modifiers_emit_nothing() {
        let ship = ship_no_abilities();
        let loadout = Loadout { skills: &[], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider).iter().cloned().collect();
        assert!(effects.is_empty());
    }

    #[test]
    fn skill_with_only_modifiers_emits_always_on() {
        let ship = ship_no_abilities();
        let skill = skill_only_modifiers("GunFeeder", vec![uniform_modifier("GMShotDelay", 0.9)]);
        let loadout = Loadout { skills: &[skill], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill("GunFeeder".to_string()));
        assert_eq!(effects[0].kind(), &EffectKind::AlwaysOn);
    }

    #[test]
    fn non_adrenaline_trigger_emits_binary() {
        let ship = ship_no_abilities();
        let skill =
            skill_with_trigger("TriggerGmReload", 0, "triggerBattleLosing", vec![uniform_modifier("GMShotDelay", 0.8)]);
        let loadout = Loadout { skills: &[skill], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill("TriggerGmReload".to_string()));
        assert_eq!(effects[0].kind(), &EffectKind::Binary);
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
        let loadout = Loadout { skills: &[skill], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Skill("ArmamentReloadAaDamage".to_string()));
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
        let loadout = Loadout { skills: &[skill], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&EmptyProvider).iter().cloned().collect();
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

        let loadout = Loadout { skills: &[], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&ScoutProvider(ability_param_rc)).iter().cloned().collect();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &EffectId::Consumable(ship_name.to_string()));
        assert_eq!(effects[0].kind(), &EffectKind::Consumable { artillery_dist_coeff: 1.2 });
    }

    #[test]
    fn consumable_with_modifiers_and_coeff_1_emits_effect() {
        let ship_name = "PCY020_SpeedBoost";
        let variant = "SpeedBoost";
        let mut fields = BTreeMap::new();
        fields.insert("artilleryDistCoeff".to_string(), 1.0f32);
        let cat = AbilityCategory::builder()
            .consumable_type("speedBoost".to_string())
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

        let loadout = Loadout { skills: &[], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&SpeedBoostProvider(ability_param_rc)).iter().cloned().collect();
        assert_eq!(effects.len(), 1, "non-empty modifiers should emit a consumable effect even with coeff==1.0");
        assert_eq!(effects[0].id(), &EffectId::Consumable(ship_name.to_string()));
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

        let loadout = Loadout { skills: &[], modernization_modifiers: &[], ship: &ship };
        let effects: Vec<_> = loadout.effects(&CrashCrewProvider(ability_param_rc)).iter().cloned().collect();
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

        let repeated =
            Effect::for_test(EffectId::Skill("DefenceUw".into()), EffectKind::StackingRepeated, Vec::new());
        assert_eq!(repeated.default_activation(), EffectActivation::Stacks(0));
    }

    fn test_version() -> crate::data::Version {
        crate::data::Version::base(15, 4, 0)
    }

    #[test]
    fn resolve_always_on_always_contributes() {
        let effect = Effect::for_test(
            EffectId::Modernizations,
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
            EffectKind::Binary,
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
            EffectId::Consumable("PCY012_Scout".into()),
            EffectKind::Consumable { artillery_dist_coeff: 1.2 },
            vec![],
        );
        let effects = Effects(vec![effect]);

        let state_off = EffectsState::default();
        let result_off = effects.resolve(&state_off, Species::Cruiser, test_version()).unwrap();
        assert!((result_off.artillery_dist_coeff() - 1.0).abs() < 1e-6, "off -> dist 1.0");

        let state_on = EffectsState::default().set(EffectId::Consumable("PCY012_Scout".into()), EffectActivation::On);
        let result_on = effects.resolve(&state_on, Species::Cruiser, test_version()).unwrap();
        assert!((result_on.artillery_dist_coeff() - 1.2).abs() < 1e-6, "on -> dist 1.2");
    }

    #[test]
    fn resolve_mixed_state_folds_correctly() {
        let always_on = Effect::for_test(
            EffectId::Modernizations,
            EffectKind::AlwaysOn,
            vec![uniform_modifier("GMShotDelay", 0.9)],
        );
        let binary = Effect::for_test(
            EffectId::Skill("Outnumbered".into()),
            EffectKind::Binary,
            vec![uniform_modifier("GMIdealRadius", 0.85)],
        );
        let adrenaline = Effect::for_test(
            EffectId::Skill("ArmamentReloadAaDamage".into()),
            EffectKind::HealthScaledReload,
            vec![uniform_modifier("lastChanceReloadCoefficient_Main", 0.2)],
        );
        let consumable = Effect::for_test(
            EffectId::Consumable("PCY012_Scout".into()),
            EffectKind::Consumable { artillery_dist_coeff: 1.2 },
            vec![uniform_modifier("GMIdealRadius", 0.95)],
        );

        let effects = Effects(vec![always_on, binary, adrenaline, consumable]);
        let state = EffectsState::default()
            .set(EffectId::Skill("Outnumbered".into()), EffectActivation::On)
            .set(EffectId::Skill("ArmamentReloadAaDamage".into()), EffectActivation::Health(HealthFraction::new(0.5)))
            .set(EffectId::Consumable("PCY012_Scout".into()), EffectActivation::On);

        let result = effects.resolve(&state, Species::Cruiser, test_version()).unwrap();

        assert!((result.bundle().coef("GMShotDelay") - 0.9).abs() < 1e-6, "always-on GMShotDelay");
        assert!(
            (result.bundle().coef("GMIdealRadius") - (0.85 * 0.95)).abs() < 1e-5,
            "binary + consumable GMIdealRadius"
        );
        assert!((result.reload_coeffs().main - 0.9).abs() < 1e-6, "adrenaline at 50% HP");
        assert!((result.artillery_dist_coeff() - 1.2).abs() < 1e-6, "consumable dist coeff");
    }
}
