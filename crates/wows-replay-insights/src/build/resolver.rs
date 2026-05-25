use std::time::Duration;

use wowsunpack::Rc;
use wowsunpack::data::Version;
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
        for upgrade in &upgrades {
            modifiers.apply_modernization(upgrade, &species);
        }
        if let Some(c) = captain.as_deref() {
            modifiers.apply_captain_skills(c, skill_types, &species);
        }
        for signal in &signals {
            modifiers.apply_exterior(signal, &species);
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
        })
    }

    /// Look up a slot by `consumable_type`. Returns the first match, or `None`
    /// if the ship has no slot of that type.
    pub fn slot_for(&self, consumable_type: wowsunpack::game_types::Consumable) -> Option<&ConsumableSlot> {
        self.slots.iter().find(|s| s.consumable_type.known() == Some(&consumable_type))
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
            continue;
        };
        let Some(ability) = ability_param.ability() else {
            continue;
        };

        let variant_name = vehicle_slots
            .get(slot_index)
            .and_then(|opts| {
                opts.iter().find_map(|(name, variant)| {
                    (name == ability_param.index()).then(|| variant.clone())
                })
            })
            .unwrap_or_else(|| "Default".to_owned());

        let Some(category) = ability.get_category(&variant_name) else {
            continue;
        };

        let consumable_type_raw = category.consumable_type_raw();
        let base_charges = ChargeCount::from_game_params(category.num_consumables());
        let bonus_for_slot = if base_charges.is_unlimited() {
            0
        } else {
            modifiers.consumable_charge_bonus(consumable_type_raw)
        };
        let total_charges = base_charges.saturating_add(bonus_for_slot);

        let work_factor = modifiers.consumable_work_time_factor(consumable_type_raw);
        let reload_factor = modifiers.consumable_reload_factor(consumable_type_raw);
        let work_time = Duration::from_secs_f32((category.work_time() * work_factor).max(0.0));
        let reload_time = Duration::from_secs_f32((category.reload_time() * reload_factor).max(0.0));

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
            icon_key: ability_param.index().to_owned(),
        });
    }
    out
}
