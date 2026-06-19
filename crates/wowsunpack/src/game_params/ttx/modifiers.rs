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

/// Additive modifier names the generated `MODIFIER_SETTINGS` table does not cover,
/// transcribed from their client apply sites where they are summed (`+`) onto a
/// base stat: `yawSpeedBonus` (FactoryArtillery.py:74, FactoryTorpedoes.py:78, used
/// as `* yawSpeedCoef + yawSpeedBonus`) and `buffsStartPool` (ModifiersApply.py:521,
/// `specialParams.buffsStartPool + modifier.buffsStartPool`). Without this allowlist
/// `classify` would fall through to the multiplicative default for these names.
const KNOWN_ADDITIVE: &[&str] = &["yawSpeedBonus", "buffsStartPool"];

/// Multiplicative modifier names the generated `MODIFIER_SETTINGS` table does not
/// cover, transcribed from their client apply sites where they multiply (`*`) a base
/// stat: `uwCoeffMultiplier` (FactoryDurability.py:8, `floodProb * uwCoeffMultiplier`);
/// the burn-chance factors `burnChanceFactorHighLevel`, `burnChanceGMGSMultiplier` and
/// `burnChanceMultiplier` (ModifiersApply.py:44/48/57, each used as `initialBurnProb *=`).
/// Without this allowlist `classify` would `debug_assert` on the unknown name even
/// though it is a coefficient with the 1.0 identity.
const KNOWN_MULTIPLICATIVE: &[&str] = &[
    "uwCoeffMultiplier",
    "burnChanceFactorHighLevel",
    "burnChanceGMGSMultiplier",
    "burnChanceMultiplier",
];

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
    /// Classify a modifier name. A name is `Add` if its settings `base_value` is 0.0
    /// or it is in `KNOWN_ADDITIVE`; `Multiply` if its settings `base_value` is 1.0.
    /// A name absent from both the table and `KNOWN_ADDITIVE` falls back to
    /// multiplicative (a coefficient's 1.0 identity is the safe no-op and the dominant
    /// case), but trips a `debug_assert` so an unrecognized name surfaces in tests
    /// rather than silently defaulting.
    fn classify(build: u32, name: &str) -> Combine {
        if KNOWN_ADDITIVE.contains(&name) {
            return Combine::Add;
        }
        if KNOWN_MULTIPLICATIVE.contains(&name) {
            return Combine::Multiply;
        }
        match modifier_setting(build, name) {
            Some(s) if s.base_value == 0.0 => Combine::Add,
            Some(_) => Combine::Multiply,
            None => {
                debug_assert!(
                    false,
                    "modifier name {name:?} is absent from MODIFIER_SETTINGS and KNOWN_ADDITIVE; defaulting to Multiply (add it to KNOWN_ADDITIVE if it is a bonus)"
                );
                Combine::Multiply
            }
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
    /// Per-name combine rule used to fold the value, kept so the accessors can
    /// `debug_assert` a caller reads each name with the matching identity.
    rules: HashMap<String, Combine>,
}

impl ModifierBundle {
    /// Aggregate `mods` for `species`. Values sharing a name fold together with the
    /// combine rule classified from `build`'s `MODIFIER_SETTINGS` base value:
    /// coefficients multiply, bonuses add.
    pub fn from_modifiers(mods: &[CrewSkillModifier], species: Species, build: u32) -> ModifierBundle {
        let mut values: HashMap<String, f32> = HashMap::new();
        let mut rules: HashMap<String, Combine> = HashMap::new();

        for m in mods {
            let name = m.name();
            let combine = *rules.entry(name.to_string()).or_insert_with(|| Combine::classify(build, name));
            let entry = values.entry(name.to_string()).or_insert_with(|| combine.identity());
            *entry = combine.fold(*entry, m.get_for_species(&species));
        }

        ModifierBundle { species, values, rules }
    }

    /// The stock (no-upgrade) bundle: no modifiers equipped, every name reads back
    /// its identity.
    pub fn empty(species: Species) -> ModifierBundle {
        ModifierBundle { species, values: HashMap::new(), rules: HashMap::new() }
    }

    /// The species this bundle was resolved for.
    pub fn species(&self) -> Species {
        self.species
    }

    /// The aggregated coefficient for `name`. A present name must have folded
    /// multiplicatively (`debug_assert`ed): reading an additively-folded bonus as a
    /// coefficient is a caller bug. Returns the multiplicative identity `1.0` when
    /// `name` is absent, the one legitimate default here: it means "no such modifier
    /// equipped" and leaves the base stat unchanged when multiplied.
    pub fn coef(&self, name: &str) -> f32 {
        match self.values.get(name).copied() {
            Some(v) => {
                debug_assert_eq!(
                    self.rules.get(name),
                    Some(&Combine::Multiply),
                    "coef({name:?}) reads an additively-folded modifier; use bonus() instead"
                );
                v
            }
            None => 1.0,
        }
    }

    /// The aggregated bonus for `name`. A present name must have folded additively
    /// (`debug_assert`ed): reading a multiplicatively-folded coefficient as a bonus is
    /// a caller bug. Returns the additive identity `0.0` when `name` is absent, the one
    /// legitimate default here: it means "no such modifier equipped" and leaves the
    /// base stat unchanged when added.
    pub fn bonus(&self, name: &str) -> f32 {
        match self.values.get(name).copied() {
            Some(v) => {
                debug_assert_eq!(
                    self.rules.get(name),
                    Some(&Combine::Add),
                    "bonus({name:?}) reads a multiplicatively-folded modifier; use coef() instead"
                );
                v
            }
            None => 0.0,
        }
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

    /// `yawSpeedBonus` is absent from MODIFIER_SETTINGS but is additive in the client
    /// (FactoryArtillery.py:74 `* yawSpeedCoef + yawSpeedBonus`); the KNOWN_ADDITIVE
    /// allowlist makes two instances add (4.0 + 2.0 = 6.0), readable via `bonus()`.
    #[test]
    fn known_additive_absent_name_adds() {
        let mods = [modifier("yawSpeedBonus", 4.0), modifier("yawSpeedBonus", 2.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        assert!((bundle.bonus("yawSpeedBonus") - 6.0).abs() < 1e-6, "got {}", bundle.bonus("yawSpeedBonus"));
    }

    /// Absent accessors return identities regardless of which accessor is used.
    #[test]
    fn absent_name_returns_identity_per_accessor() {
        let bundle = ModifierBundle::from_modifiers(&[], Species::Battleship, BUILD);
        assert_eq!(bundle.coef("yawSpeedBonus"), 1.0);
        assert_eq!(bundle.bonus("speedCoef"), 0.0);
        assert_eq!(bundle.coef("nonexistentName"), 1.0);
        assert_eq!(bundle.bonus("nonexistentName"), 0.0);
    }

    /// Reading a present additive name (`yawSpeedBonus`) through `coef()` trips the
    /// classification-mismatch debug_assert in a debug build.
    #[test]
    #[should_panic(expected = "additively-folded")]
    #[cfg(debug_assertions)]
    fn coef_on_additive_name_trips_assert() {
        let mods = [modifier("yawSpeedBonus", 4.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, BUILD);
        let _ = bundle.coef("yawSpeedBonus");
    }
}
