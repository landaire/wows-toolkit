use std::collections::HashMap;

use wowsunpack::game_params::types::CrewSkillModifier;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

/// Accumulated, species-resolved modifier values from a player's build.
///
/// Modifiers combine in three ways:
///
/// 1. Flat multiplicative coefficients (default): `vis *= modifier.value`.
/// 2. Flat additive bonuses (names in [`is_additive`]): `bonus += modifier.value`.
/// 3. Scoped consumable modifiers: stored with their `excludedConsumables` list
///    and applied per-consumable at lookup time via
///    [`Self::consumable_reload_factor`] and [`Self::consumable_charge_bonus`].
///
/// The scoped path exists because some skills (e.g. Survival Expert with
/// `reloadFactor: 0.925, excludedConsumables: ["crashCrew", "regenCrew"]`)
/// only affect a subset of a ship's consumables.
#[derive(Debug, Clone, Default)]
pub struct ModifierSet {
    multiplicative: HashMap<String, f32>,
    additive: HashMap<String, f32>,
    scoped: Vec<ScopedModifier>,
}

#[derive(Debug, Clone)]
struct ScopedModifier {
    name: String,
    value: f32,
    excluded: Vec<String>,
}

impl ModifierSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn coefficient(&self, name: &str) -> f32 {
        self.multiplicative.get(name).copied().unwrap_or(1.0)
    }

    pub fn bonus(&self, name: &str) -> f32 {
        self.additive.get(name).copied().unwrap_or(0.0)
    }

    pub fn bonus_u32(&self, name: &str) -> u32 {
        self.bonus(name).max(0.0) as u32
    }

    pub fn apply(&mut self, modifier: &CrewSkillModifier, species: &Species) {
        let value = modifier.get_for_species(species);
        let name = modifier.name();
        let excluded = modifier.excluded_consumables();
        if !excluded.is_empty() {
            self.scoped.push(ScopedModifier { name: name.to_owned(), value, excluded: excluded.to_vec() });
            return;
        }
        if is_additive(name) {
            *self.additive.entry(name.to_owned()).or_insert(0.0) += value;
        } else {
            *self.multiplicative.entry(name.to_owned()).or_insert(1.0) *= value;
        }
    }

    pub fn apply_modernization(&mut self, modernization: &Param, species: &Species) {
        let Some(m) = modernization.modernization() else {
            return;
        };
        for modifier in m.modifiers() {
            self.apply(modifier, species);
        }
    }

    pub fn apply_captain_skills(&mut self, captain: &Param, skill_types: &[u8], species: &Species) {
        let Some(crew) = captain.crew() else {
            return;
        };
        for &skill_type in skill_types {
            let Some(skill) = crew.skill_by_type(wowsunpack::game_params::types::CrewSkillType::from(skill_type))
            else {
                continue;
            };
            let Some(modifiers) = skill.modifiers() else {
                continue;
            };
            for modifier in modifiers {
                self.apply(modifier, species);
            }
        }
    }

    pub fn apply_exterior(&mut self, exterior: &Param, species: &Species) {
        let Some(ext) = exterior.exterior() else {
            return;
        };
        for modifier in ext.modifiers() {
            self.apply(modifier, species);
        }
    }

    /// Effective reload coefficient for a given consumable type (raw GameParams
    /// name like `"crashCrew"`). Combines:
    ///
    /// * Universal reload modifiers (`ConsumableReloadTime` from Worcester
    ///   Special Mod, etc.) from the flat multiplicative map.
    /// * Per-type modifiers like `crashCrewReloadCoeff` from the flat map.
    /// * Scoped reload modifiers (e.g. Survival Expert's `reloadFactor` with
    ///   `excludedConsumables`) that apply unless this type is excluded.
    pub fn consumable_reload_factor(&self, consumable_type: &str) -> f32 {
        let universal = self.coefficient("ConsumableReloadTime");
        let per_type = self.coefficient(&format!("{consumable_type}ReloadCoeff"));
        let scoped: f32 = self
            .scoped
            .iter()
            .filter(|m| is_reload_name(&m.name) && !m.excluded.iter().any(|t| t == consumable_type))
            .map(|m| m.value)
            .product();
        universal * per_type * scoped
    }

    /// Effective work-time coefficient for a given consumable type.
    pub fn consumable_work_time_factor(&self, consumable_type: &str) -> f32 {
        let universal = self.coefficient("ConsumablesWorkTime");
        let scoped: f32 = self
            .scoped
            .iter()
            .filter(|m| m.name == "ConsumablesWorkTime" && !m.excluded.iter().any(|t| t == consumable_type))
            .map(|m| m.value)
            .product();
        universal * scoped
    }

    /// Additional charges granted by build modifiers for a given consumable
    /// type. Sums the universal `additionalConsumables` bonus, the per-type
    /// `{type}AdditionalConsumables`, and any scoped variants whose exclusion
    /// list doesn't cover `consumable_type`.
    pub fn consumable_charge_bonus(&self, consumable_type: &str) -> u32 {
        let universal = self.bonus("additionalConsumables");
        let per_type = self.bonus(&format!("{consumable_type}AdditionalConsumables"));
        let scoped: f32 = self
            .scoped
            .iter()
            .filter(|m| m.name == "additionalConsumables" && !m.excluded.iter().any(|t| t == consumable_type))
            .map(|m| m.value)
            .sum();
        (universal + per_type + scoped).max(0.0) as u32
    }
}

fn is_additive(name: &str) -> bool {
    name == "additionalConsumables"
        || name == "additionalFighters"
        || name == "additionalSlots"
        || name == "additionalCharges"
        || name.ends_with("AdditionalConsumables")
}

fn is_reload_name(name: &str) -> bool {
    name == "reloadFactor" || name == "ConsumableReloadTime"
}
