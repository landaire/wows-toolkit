//! Aggregation of equipped modifiers (modernizations + commander skills) into a
//! per-name bundle resolved for one `Species`.
//!
//! Combine rule (faithful to the client). The client folds a list of same-name
//! modifier values inside the native `Modifiers.ModifierDef.mix`
//! (`Components/ModifiersComponent.py:109`, `CrewModifiers.py:95`,
//! `ma6320f36/ttx/TTXFactory.py:61`), which is a compiled module with no Python
//! source, so the fold operator cannot be read directly. Each modifier name has
//! a `baseValue` in the client `MODIFIER_SETTINGS` table
//! (`mbf4783af/ModifierSettings.py:359`), and that base value is the identity
//! the fold preserves: coefficient names have `baseValue == 1.0` and are folded
//! multiplicatively, bonus names have `baseValue == 0.0` and are folded
//! additively. This base-value classifier cross-checks exactly against every
//! apply site in `Modifiers/ModifiersApply.py`, e.g. `speedCoef` (base 1.0) is
//! used as `modifiers.speedCoef *` (line 509), `torpedoSpeedMultiplier`
//! (base 1.0) as `* modifier.torpedoSpeedMultiplier` (line 498), while
//! `torpedoSpeedBonus` (base 0.0) is used as `+ modifier.torpedoSpeedBonus`
//! (line 499) and `buffsShiftMaxLevel` (base 0.0) as `+ modifier.buffsShiftMaxLevel`
//! (line 529). We reuse `crate::game_params::modifier_settings_data::modifier_setting`,
//! which already transcribes `base_value`, as the classifier rather than guessing
//! from name suffixes.

use std::collections::HashMap;

use crate::game_params::modifier_settings_data::modifier_setting;
use crate::game_params::types::{CrewSkillModifier, Species};

/// How same-name modifier values fold, keyed off the modifier's `MODIFIER_SETTINGS`
/// `base_value` (1.0 -> coefficient, 0.0 -> bonus).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Combine {
    /// Coefficient: values multiply, identity 1.0.
    Multiply,
    /// Bonus: values add, identity 0.0.
    Add,
}

impl Combine {
    /// Classify a modifier name by its transcribed `base_value`. Names absent from
    /// the settings table, or with a base value that is neither identity, default
    /// to multiplicative: a coefficient's 1.0 identity is the safe no-op and is the
    /// dominant case in the client.
    fn classify(build: u32, name: &str) -> Combine {
        match modifier_setting(build, name) {
            Some(s) if s.base_value == 0.0 => Combine::Add,
            _ => Combine::Multiply,
        }
    }

    fn identity(self) -> f32 {
        match self {
            Combine::Multiply => 1.0,
            Combine::Add => 0.0,
        }
    }

    fn fold(self, acc: f32, value: f32) -> f32 {
        match self {
            Combine::Multiply => acc * value,
            Combine::Add => acc + value,
        }
    }
}

/// Equipped modifiers aggregated per name and resolved for a fixed `Species`.
///
/// Each entry holds the fold of every same-name modifier value using that name's
/// combine rule, so the aggregated value already carries the correct identity for
/// its kind: a coefficient name reads back 1.0 when nothing touched it, a bonus
/// name reads back 0.0.
#[derive(Clone, Debug)]
pub struct ModifierBundle {
    species: Species,
    values: HashMap<String, f32>,
}

impl ModifierBundle {
    /// Aggregate `mods` for `species`. Values sharing a name fold together with the
    /// combine rule classified from `build`'s `MODIFIER_SETTINGS` base value:
    /// coefficients multiply, bonuses add.
    pub fn from_modifiers(mods: &[CrewSkillModifier], species: Species, build: u32) -> ModifierBundle {
        let mut combines: HashMap<&str, Combine> = HashMap::new();
        let mut values: HashMap<String, f32> = HashMap::new();

        for m in mods {
            let name = m.name();
            let combine = *combines.entry(name).or_insert_with(|| Combine::classify(build, name));
            let entry = values.entry(name.to_string()).or_insert_with(|| combine.identity());
            *entry = combine.fold(*entry, m.get_for_species(&species));
        }

        ModifierBundle { species, values }
    }

    /// The stock (no-upgrade) bundle: no modifiers equipped, every name reads back
    /// its identity.
    pub fn empty(species: Species) -> ModifierBundle {
        ModifierBundle { species, values: HashMap::new() }
    }

    /// The species this bundle was resolved for.
    pub fn species(&self) -> Species {
        self.species
    }

    /// The aggregated coefficient for `name`. Returns the multiplicative identity
    /// `1.0` when `name` is absent, which is the one legitimate default here: it
    /// means "no such modifier equipped" and leaves the base stat unchanged when
    /// multiplied.
    pub fn coef(&self, name: &str) -> f32 {
        self.values.get(name).copied().unwrap_or(1.0)
    }

    /// The aggregated bonus for `name`. Returns the additive identity `0.0` when
    /// `name` is absent, which is the one legitimate default here: it means "no
    /// such modifier equipped" and leaves the base stat unchanged when added.
    pub fn bonus(&self, name: &str) -> f32 {
        self.values.get(name).copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::CrewSkillModifier;

    /// The build whose `MODIFIER_SETTINGS` table is transcribed in the toolkit.
    const BUILD: u32 = 11791718;

    fn modifier(name: &str, battleship: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_string())
            .aircraft_carrier(1.0)
            .auxiliary(1.0)
            .battleship(battleship)
            .cruiser(1.0)
            .destroyer(1.0)
            .submarine(1.0)
            .excluded_consumables(Vec::new())
            .build()
    }

    /// Per-species variant for resolution tests.
    fn modifier_per_species(name: &str, battleship: f32, cruiser: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_string())
            .aircraft_carrier(1.0)
            .auxiliary(1.0)
            .battleship(battleship)
            .cruiser(cruiser)
            .destroyer(1.0)
            .submarine(1.0)
            .excluded_consumables(Vec::new())
            .build()
    }

    /// `speedCoef` has base_value 1.0 (ModifiersApply.py:509 uses it as `*`), so two
    /// instances multiply: 0.9 * 1.05 = 0.945.
    #[test]
    fn same_multiplicative_name_multiplies() {
        let mods = [modifier("speedCoef", 0.9), modifier("speedCoef", 1.05)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        assert!((bundle.coef("speedCoef") - 0.945).abs() < 1e-6, "got {}", bundle.coef("speedCoef"));
    }

    /// `torpedoSpeedBonus` has base_value 0.0 (ModifiersApply.py:499 uses it as `+`),
    /// so two instances add: 5.0 + 3.0 = 8.0.
    #[test]
    fn same_additive_name_adds() {
        let mods = [modifier("torpedoSpeedBonus", 5.0), modifier("torpedoSpeedBonus", 3.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        assert!((bundle.bonus("torpedoSpeedBonus") - 8.0).abs() < 1e-6, "got {}", bundle.bonus("torpedoSpeedBonus"));
    }

    /// A second additive name (`buffsShiftMaxLevel`, base 0.0, ModifiersApply.py:529
    /// `+`) also adds, confirming the classifier is per-name not per-instance.
    #[test]
    fn second_additive_name_adds() {
        let mods = [modifier("buffsShiftMaxLevel", 2.0), modifier("buffsShiftMaxLevel", 1.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        assert!((bundle.bonus("buffsShiftMaxLevel") - 3.0).abs() < 1e-6);
    }

    /// Absent names read back their identity: coef 1.0, bonus 0.0.
    #[test]
    fn absent_name_is_identity() {
        let bundle = ModifierBundle::from_modifiers(&[], Species::Battleship, BUILD);
        assert_eq!(bundle.coef("speedCoef"), 1.0);
        assert_eq!(bundle.bonus("torpedoSpeedBonus"), 0.0);
    }

    /// Resolution picks the slot for the bundle's species.
    #[test]
    fn per_species_resolution() {
        let mods = [modifier_per_species("speedCoef", 0.9, 1.2)];
        let bb = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let ca = ModifierBundle::from_modifiers(&mods, Species::Cruiser, BUILD);
        assert!((bb.coef("speedCoef") - 0.9).abs() < 1e-6);
        assert!((ca.coef("speedCoef") - 1.2).abs() < 1e-6);
    }

    /// The stock bundle is all identities.
    #[test]
    fn empty_bundle_is_identity() {
        let bundle = ModifierBundle::empty(Species::Battleship);
        assert_eq!(bundle.species(), Species::Battleship);
        assert_eq!(bundle.coef("speedCoef"), 1.0);
        assert_eq!(bundle.coef("torpedoSpeedMultiplier"), 1.0);
        assert_eq!(bundle.bonus("torpedoSpeedBonus"), 0.0);
    }
}
