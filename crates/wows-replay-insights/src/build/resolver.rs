use std::time::Duration;

use wowsunpack::Rc;
use wowsunpack::data::Version;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::modifiers::ModifierError;
use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::GameParamId;

use wows_replays::analyzer::battle_controller::Player;

use super::consumables::ChargeCount;
use super::consumables::ConsumableSlot;
use super::modifiers::ModifierSet;

/// A player's fully resolved loadout: ship, equipped modules, upgrades, captain
/// (and learned skills), signals, consumable slots with accumulated modifier
/// effects applied.
///
/// Construct via [`Self::from_player`] (when starting from a parsed replay) or
/// [`Self::from_ids`] (when starting from bare GameParams IDs, e.g. in a bot).
#[derive(Debug, Clone)]
pub struct ResolvedBuild {
    pub ship: Rc<Param>,
    pub species: Species,
    pub modules: Vec<Rc<Param>>,
    pub upgrades: Vec<Rc<Param>>,
    pub captain: Option<Rc<Param>>,
    /// Raw learned-skill IDs for `species`, as they appear in the replay's
    /// `crew_modifiers_compact_params.learned_skills`. Preserved verbatim so
    /// build-tracker uploads and build URL exporters can round-trip them.
    pub skills: Vec<u8>,
    pub signals: Vec<Rc<Param>>,
    pub slots: Vec<ConsumableSlot>,
    pub modifiers: ModifierSet,
    /// Every modifier this build contributes, in application order: upgrades,
    /// then captain skills, then signals. Collected alongside `modifiers` in
    /// `from_ids` so this list and the folded `ModifierSet` can never disagree
    /// about what was applied.
    raw: Vec<CrewSkillModifier>,
}

impl ResolvedBuild {
    pub fn from_player<P: GameParamProvider>(player: &Player, gp: &P, version: Version) -> Option<Self> {
        let entity = player.vehicle_entity()?;
        let config = entity.props().ship_config();
        let ship = player.vehicle();
        let species = *ship.species()?.known()?;
        let captain_id = entity.captain().map(|c| c.id());
        let skills = entity.commander_skills_raw(species);

        Self::from_ids(
            config.ship_params_id(),
            config.units(),
            config.modernization(),
            captain_id,
            skills,
            config.exteriors(),
            config.abilities(),
            species,
            version,
            gp,
        )
    }

    pub fn captain_index(&self) -> &str {
        self.captain.as_ref().map(|c| c.index()).unwrap_or("PCW001")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_ids<P: GameParamProvider>(
        ship_id: GameParamId,
        modules: &[GameParamId],
        upgrades: &[GameParamId],
        captain_id: Option<GameParamId>,
        skill_types: &[u8],
        signals: &[GameParamId],
        abilities: &[GameParamId],
        species: Species,
        version: Version,
        gp: &P,
    ) -> Option<Self> {
        let ship = gp.game_param_by_id(ship_id)?;
        let modules = resolve_ids(modules, gp);
        let upgrades = resolve_ids(upgrades, gp);
        let captain = captain_id.and_then(|id| gp.game_param_by_id(id));
        let signals = resolve_ids(signals, gp);

        let mut modifiers = ModifierSet::new();
        let mut raw = Vec::new();
        for upgrade in &upgrades {
            raw.extend(modifiers.apply_modernization(upgrade, &species));
        }
        if let Some(c) = captain.as_deref() {
            raw.extend(modifiers.apply_captain_skills(c, skill_types, &species));
        }
        for signal in &signals {
            raw.extend(modifiers.apply_exterior(signal, &species));
        }

        let slots = resolve_slots(&ship, abilities, gp, version, &modifiers);

        Some(Self {
            ship,
            species,
            modules,
            upgrades,
            captain,
            skills: skill_types.to_vec(),
            signals,
            slots,
            modifiers,
            raw,
        })
    }

    /// Look up a slot by `consumable_type`. Returns the first match, or `None`
    /// if the ship has no slot of that type.
    pub fn slot_for(&self, consumable_type: wowsunpack::game_types::Consumable) -> Option<&ConsumableSlot> {
        self.slots.iter().find(|s| s.consumable_type.known() == Some(&consumable_type))
    }

    /// Every modifier this build contributes, in application order: upgrades,
    /// then captain skills, then signals. Kept raw so callers can fold them
    /// with a different rule than `ModifierSet` uses.
    pub fn raw_modifiers(&self) -> &[CrewSkillModifier] {
        &self.raw
    }

    /// The build's modifiers folded for `ttx`. `Err` when a modifier name is
    /// absent from the version's MODIFIER_SETTINGS table, which means the
    /// table needs regenerating for this build.
    pub fn modifier_bundle(&self, version: Version) -> Result<ModifierBundle, ModifierError> {
        ModifierBundle::from_modifiers(self.raw_modifiers(), self.species, version)
    }
}

fn resolve_ids<P: GameParamProvider>(ids: &[GameParamId], gp: &P) -> Vec<Rc<Param>> {
    ids.iter().filter_map(|id| gp.game_param_by_id(*id)).collect()
}

fn resolve_slots<P: GameParamProvider>(
    ship: &Param,
    chosen_abilities: &[GameParamId],
    gp: &P,
    version: Version,
    modifiers: &ModifierSet,
) -> Vec<ConsumableSlot> {
    let Some(vehicle) = ship.vehicle() else {
        return Vec::new();
    };
    let vehicle_slots = vehicle.abilities().unwrap_or(&[]);

    let mut out = Vec::with_capacity(chosen_abilities.len());
    for (slot_index, ability_id) in chosen_abilities.iter().enumerate() {
        let Some(ability_param) = gp.game_param_by_id(*ability_id) else {
            tracing::trace!(
                ?ability_id,
                ship = ship.index(),
                slot_index,
                "resolve_slots: gp.game_param_by_id returned None"
            );
            continue;
        };
        let Some(ability) = ability_param.ability() else {
            tracing::debug!(
                ?ability_id,
                ship = ship.index(),
                slot_index,
                ability_param_index = ability_param.index(),
                ability_param_type = ?std::mem::discriminant(ability_param.data()),
                "resolve_slots: param is not an Ability"
            );
            continue;
        };

        // Vehicle::abilities slots are keyed by the ability's full name
        // (e.g. "PCY009_CrashCrewPremium"), not its short index ("PCY009").
        let ability_full_name = ability_param.name();
        let variant_name = vehicle_slots
            .get(slot_index)
            .and_then(|opts| {
                opts.iter().find_map(|(name, variant)| (name == ability_full_name).then(|| variant.clone()))
            })
            .unwrap_or_else(|| "Default".to_owned());

        let Some(category) = ability.get_category(&variant_name) else {
            tracing::debug!(
                ?ability_id,
                ship = ship.index(),
                slot_index,
                ability_param_index = ability_param.index(),
                variant_name,
                available_variants = ?ability.categories().keys().collect::<Vec<_>>(),
                "resolve_slots: ability has no matching variant category"
            );
            continue;
        };

        let consumable_type_raw = category.consumable_type_raw();
        let base_charges = ChargeCount::from_game_params(category.num_consumables());
        let bonus_for_slot =
            if base_charges.is_unlimited() { 0 } else { modifiers.consumable_charge_bonus(consumable_type_raw) };
        let total_charges = base_charges.saturating_add(bonus_for_slot);

        let work_factor = modifiers.consumable_work_time_factor(consumable_type_raw);
        let reload_factor = modifiers.consumable_reload_factor(consumable_type_raw);
        let work_time = Duration::from_secs_f32((category.work_time() * work_factor).max(0.0));
        let reload_time = Duration::from_secs_f32((category.reload_time() * reload_factor).max(0.0));

        let regen_factor = modifiers.regeneration_hp_speed_factor();
        let regen_hp_speed = category.regeneration_hp_speed().map(|s| s * regen_factor);
        let regen_hp_speed_units = category.regeneration_hp_speed_units().map(|u| u * regen_factor);

        // Icon files are stored as `consumable_<full_name>.png` and the
        // minimap renderer keys its icon map by `<full_name>` (e.g.
        // `PCY009_CrashCrewPremium`). `Param::index()` is the short prefix
        // (`PCY009`) and `Param::name()` is the full key, so use `name()`.
        out.push(ConsumableSlot {
            slot_index: slot_index as u8,
            ability: Rc::clone(&ability_param),
            variant_name,
            consumable_type: category.consumable_type(version),
            consumable_type_raw: consumable_type_raw.to_owned(),
            base_charges,
            bonus_charges: bonus_for_slot,
            total_charges,
            work_time,
            reload_time,
            regen_hp_speed,
            regen_hp_speed_units,
            icon_key: ability_param.name().to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowsunpack::game_params::types::Achievement;
    use wowsunpack::game_params::types::Crew;
    use wowsunpack::game_params::types::CrewPersonality;
    use wowsunpack::game_params::types::CrewPersonalityShips;
    use wowsunpack::game_params::types::CrewSkill;
    use wowsunpack::game_params::types::CrewSkillName;
    use wowsunpack::game_params::types::CrewSkillTiers;
    use wowsunpack::game_params::types::CrewSkillType;
    use wowsunpack::game_params::types::GameParams;
    use wowsunpack::game_params::types::Modernization;
    use wowsunpack::game_params::types::ParamData;
    use wowsunpack::game_params::types::SkillPointCost;

    const VERSION: Version = Version::base(15, 0, 0);

    /// Arbitrary skill_type id for the fixture "Fire Prevention Expert" style
    /// skill; only needs to be consistent between the captain fixture and the
    /// `skill_types` passed into `from_ids`.
    const DEFENCE_FIRE_PROBABILITY_SKILL: u8 = 42;

    fn burn_modifier(value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name("burnProb".to_owned())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }

    fn skill_tiers() -> CrewSkillTiers {
        CrewSkillTiers::builder()
            .aircraft_carrier(SkillPointCost::new(1))
            .auxiliary(SkillPointCost::new(1))
            .battleship(SkillPointCost::new(1))
            .cruiser(SkillPointCost::new(1))
            .destroyer(SkillPointCost::new(1))
            .submarine(SkillPointCost::new(1))
            .build()
    }

    fn personality() -> CrewPersonality {
        CrewPersonality::builder()
            .can_reset_skills_for_free(false)
            .cost_credits(0)
            .cost_elite_xp(0)
            .cost_gold(0)
            .cost_xp(0)
            .person_name(String::new())
            .ships(
                CrewPersonalityShips::builder()
                    .groups(Vec::new())
                    .nation(Vec::new())
                    .peculiarity(Vec::new())
                    .ships(Vec::new())
                    .build(),
            )
            .tags(Vec::new())
            .build()
    }

    fn ship_param(id: GameParamId) -> Param {
        Param::builder()
            .id(id)
            .index("SHIP01".to_owned())
            .name("SHIP01".to_owned())
            .nation("USA".to_owned())
            .data(ParamData::Achievement(
                Achievement::builder()
                    .is_group(false)
                    .one_per_battle(false)
                    .ui_type(String::new())
                    .ui_name(String::new())
                    .build(),
            ))
            .build()
    }

    /// Only fixture upgrade the tests need: Damage Control System Modification
    /// 1's `burnProb: 0.95`. Other names carry no modifiers.
    fn upgrade_param(id: GameParamId, index: &str) -> Param {
        let modifiers = match index {
            "PCM020_DamageControl_Mod_I" => vec![burn_modifier(0.95)],
            _ => Vec::new(),
        };
        Param::builder()
            .id(id)
            .index(index.to_owned())
            .name(index.to_owned())
            .nation("USA".to_owned())
            .data(ParamData::Modernization(Modernization::new(
                modifiers,
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )))
            .build()
    }

    /// Captain carrying one learned-skill fixture: `DEFENCE_FIRE_PROBABILITY_SKILL`
    /// contributing `burnProb: 0.9`. Whether it is actually applied is controlled
    /// by the `skill_types` passed into `from_ids`, not by this fixture.
    fn captain_param(id: GameParamId) -> Param {
        let skill = CrewSkill::builder()
            .internal_name(CrewSkillName::from("FireResistance"))
            .can_be_learned(true)
            .is_epic(false)
            .skill_type(CrewSkillType::from(DEFENCE_FIRE_PROBABILITY_SKILL))
            .tier(skill_tiers())
            .ui_treat_as_trigger(false)
            .modifiers(vec![burn_modifier(0.9)])
            .build();

        let crew =
            Crew::builder().money_training_level(0).personality(personality()).maybe_skills(Some(vec![skill])).build();

        Param::builder()
            .id(id)
            .index("PAW001".to_owned())
            .name("PAW001_Captain".to_owned())
            .nation("USA".to_owned())
            .data(ParamData::Crew(crew))
            .build()
    }

    fn build_with(upgrade_names: &[&str], skill_types: &[u8]) -> ResolvedBuild {
        let ship_id = GameParamId::from(1u32);
        let mut params = vec![ship_param(ship_id)];

        let mut upgrade_ids = Vec::new();
        for (i, name) in upgrade_names.iter().enumerate() {
            let id = GameParamId::from(100u32 + i as u32);
            params.push(upgrade_param(id, name));
            upgrade_ids.push(id);
        }

        let captain_id = GameParamId::from(999u32);
        params.push(captain_param(captain_id));

        let gp = GameParams::from(params);

        ResolvedBuild::from_ids(
            ship_id,
            &[],
            &upgrade_ids,
            Some(captain_id),
            skill_types,
            &[],
            &[],
            Species::Cruiser,
            VERSION,
            &gp,
        )
        .expect("test fixture resolves")
    }

    /// Fire Prevention Expert and Damage Control System Modification 1 both fold
    /// into burnProb multiplicatively: 0.9 * 0.95.
    #[test]
    fn burn_prob_folds_multiplicatively_across_skill_and_upgrade() {
        let build = build_with(&["PCM020_DamageControl_Mod_I"], &[DEFENCE_FIRE_PROBABILITY_SKILL]);
        let bundle = build.modifier_bundle(VERSION).expect("known modifiers");
        assert!((bundle.coef("burnProb") - 0.855).abs() < 1e-5, "got {}", bundle.coef("burnProb"));
    }

    /// A build with neither reads the identity, not zero.
    #[test]
    fn burn_prob_defaults_to_one_without_either() {
        let build = build_with(&[], &[]);
        let bundle = build.modifier_bundle(VERSION).expect("known modifiers");
        assert!((bundle.coef("burnProb") - 1.0).abs() < 1e-6);
    }

    /// Order matters for the bundle's own folding rules, so the raw list must be
    /// upgrades, then skills, then signals.
    #[test]
    fn raw_modifiers_are_in_application_order() {
        let build = build_with(&["PCM020_DamageControl_Mod_I"], &[DEFENCE_FIRE_PROBABILITY_SKILL]);
        let names: Vec<&str> = build.raw_modifiers().iter().map(|m| m.name()).collect();
        let upgrade_at = names.iter().position(|n| *n == "burnProb").expect("upgrade burnProb");
        let skill_at = names.iter().rposition(|n| *n == "burnProb").expect("skill burnProb");
        assert!(upgrade_at < skill_at);
    }
}
