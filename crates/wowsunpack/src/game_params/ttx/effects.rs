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
            if let Some(trigger) = skill.logic_trigger() {
                if let Some(tmods) = trigger.modifiers().filter(|m| !m.is_empty()) {
                    let recognized =
                        KnownCrewSkill::recognize(skill.internal_name(), skill.skill_type());
                    let kind = match recognized.known() {
                        Some(KnownCrewSkill::AdrenalineRush)
                        | Some(KnownCrewSkill::SubmarineAdrenalineRush) => {
                            EffectKind::HealthScaledReload
                        }
                        _ => EffectKind::Binary,
                    };
                    effects.push(Effect {
                        id: EffectId::Skill(name),
                        kind,
                        modifiers: tmods.clone(),
                    });
                }
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
                    let artillery_dist_coeff =
                        cat.effect_fields().get("artilleryDistCoeff").copied().unwrap_or(1.0);
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
            .cooling_interpolator(Vec::new())
            .duration(0.0)
            .energy_coeff(0.0)
            .heat_interpolator(Vec::new())
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
        let skill = skill_with_trigger(
            "TriggerGmReload",
            0,
            "triggerBattleLosing",
            vec![uniform_modifier("GMShotDelay", 0.8)],
        );
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
        let effects: Vec<_> =
            loadout.effects(&CrashCrewProvider(ability_param_rc)).iter().cloned().collect();
        assert!(effects.is_empty(), "crashCrew with dist_coeff=1.0 and no modifiers should emit no effect");
    }
}
