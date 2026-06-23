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

use crate::data::Version;
use crate::game_params::modifier_settings_data::modifier_setting;
use crate::game_params::types::CrewSkillModifier;
use crate::game_params::types::Species;

/// Additive modifier names the generated `MODIFIER_SETTINGS` table does not cover,
/// transcribed from their client apply sites where they are summed (`+`) onto a
/// base stat: `yawSpeedBonus` (FactoryArtillery.py:74, FactoryTorpedoes.py:78, used
/// as `* yawSpeedCoef + yawSpeedBonus`) and `buffsStartPool` (ModifiersApply.py:521,
/// `specialParams.buffsStartPool + modifier.buffsStartPool`). `healthRegenPercent` is a
/// captain-skill bonus whose GameParams values carry the 0.0 additive identity
/// (0.0/0.006/../0.06). Without this allowlist `classify` would reject these names as unknown.
const KNOWN_ADDITIVE: &[&str] = &["yawSpeedBonus", "buffsStartPool", "healthRegenPercent"];

/// A modifier name that is neither in `MODIFIER_SETTINGS` nor either allowlist, so
/// its fold operator (multiply vs add) cannot be classified.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownModifier {
    pub name: String,
}

impl std::fmt::Display for UnknownModifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unrecognized modifier {}: not in MODIFIER_SETTINGS or the additive/multiplicative allowlists",
            self.name
        )
    }
}

impl std::error::Error for UnknownModifier {}

/// Failure aggregating modifiers into a bundle.
#[derive(Debug, Clone, PartialEq)]
pub enum ModifierError {
    /// One or more names could not be classified. Sorted and deduplicated so the
    /// caller sees the complete set.
    Unknown(Vec<String>),
}

impl std::fmt::Display for ModifierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModifierError::Unknown(names) => write!(
                f,
                "unrecognized modifiers (not in MODIFIER_SETTINGS or the additive/multiplicative allowlists): {}",
                names.join(", ")
            ),
        }
    }
}

impl std::error::Error for ModifierError {}

/// Multiplicative modifier names the generated `MODIFIER_SETTINGS` table does not
/// cover, transcribed from their client apply sites where they multiply (`*`) a base
/// stat: `uwCoeffMultiplier` (FactoryDurability.py:8, `floodProb * uwCoeffMultiplier`);
/// the burn-chance factors `burnChanceFactorHighLevel`, `burnChanceGMGSMultiplier` and
/// `burnChanceMultiplier` (ModifiersApply.py:44/48/57, each used as `initialBurnProb *=`).
/// The remaining names are captain-skill coefficients whose GameParams values carry the
/// 1.0 multiplicative identity: `reloadFactor` (0.9/0.925), `engineForwardForsagePower` /
/// `engineBackwardForsagePower` (1.0..1.3), `hydrophoneWaveSpeedCoeff` (1.0) and
/// `planeEmptyReturnSpeed` (0.5/1.0/1.2). Without this allowlist `classify` would reject
/// the unknown name even though it is a coefficient with the 1.0 identity.
const KNOWN_MULTIPLICATIVE: &[&str] = &[
    "uwCoeffMultiplier",
    "burnChanceFactorHighLevel",
    "burnChanceGMGSMultiplier",
    "burnChanceMultiplier",
    "reloadFactor",
    "engineForwardForsagePower",
    "engineBackwardForsagePower",
    "hydrophoneWaveSpeedCoeff",
    "planeEmptyReturnSpeed",
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
    /// or it is in `KNOWN_ADDITIVE`; `Multiply` if its settings `base_value` is 1.0 or
    /// it is in `KNOWN_MULTIPLICATIVE`. A name absent from both the table and the
    /// allowlists is an `UnknownModifier` error: there is no defensible fold for it,
    /// so we never guess.
    fn classify(version: Version, name: &str) -> Result<Combine, UnknownModifier> {
        if KNOWN_ADDITIVE.contains(&name) {
            return Ok(Combine::Add);
        }
        if KNOWN_MULTIPLICATIVE.contains(&name) {
            return Ok(Combine::Multiply);
        }
        match modifier_setting(version, name) {
            Some(s) if s.base_value == 0.0 => Ok(Combine::Add),
            Some(_) => Ok(Combine::Multiply),
            None => Err(UnknownModifier { name: name.to_string() }),
        }
    }

    fn identity(self) -> f32 {
        match self {
            Combine::Multiply => 1.0,
            Combine::Add => 0.0,
        }
    }

    /// Apply this rule's operator to a base value with `amount`.
    fn apply(self, base: f32, amount: f32) -> f32 {
        match self {
            Combine::Multiply => base * amount,
            Combine::Add => base + amount,
        }
    }

    fn fold(self, acc: f32, value: f32) -> f32 {
        match self {
            Combine::Multiply => acc * value,
            Combine::Add => acc + value,
        }
    }
}

/// The fold identity for `name` at `version`: 1.0 for multiplicative names, 0.0 for
/// additive ones. Errors when the name cannot be classified (same rule as `from_modifiers`).
pub(crate) fn modifier_identity(version: Version, name: &str) -> Result<f32, ModifierError> {
    Combine::classify(version, name).map(Combine::identity).map_err(|e| ModifierError::Unknown(vec![e.name]))
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
    /// combine rule classified from `version`'s `MODIFIER_SETTINGS` base value:
    /// coefficients multiply, bonuses add.
    ///
    /// Every distinct name must classify. Names that cannot be classified are
    /// collected (sorted, deduplicated) and returned as `ModifierError::Unknown`; the
    /// bundle is not partially built and no unknown is silently folded.
    pub fn from_modifiers(
        mods: &[CrewSkillModifier],
        species: Species,
        version: Version,
    ) -> Result<ModifierBundle, ModifierError> {
        let mut values: HashMap<String, f32> = HashMap::new();
        let mut rules: HashMap<String, Combine> = HashMap::new();
        let mut unknown: Vec<String> = Vec::new();

        for m in mods {
            let name = m.name();
            let combine = match rules.get(name) {
                Some(c) => *c,
                None => match Combine::classify(version, name) {
                    Ok(c) => {
                        rules.insert(name.to_string(), c);
                        c
                    }
                    Err(e) => {
                        unknown.push(e.name);
                        continue;
                    }
                },
            };
            let entry = values.entry(name.to_string()).or_insert_with(|| combine.identity());
            *entry = combine.fold(*entry, m.get_for_species(&species));
        }

        if !unknown.is_empty() {
            unknown.sort();
            unknown.dedup();
            return Err(ModifierError::Unknown(unknown));
        }

        Ok(ModifierBundle { species, values, rules })
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

    /// Apply a single named modifier to a base value, choosing multiply vs add by
    /// the modifier's own classification. Callers never need to know which it is.
    ///
    /// When `name` is absent (no such modifier equipped) the call is a no-op: the
    /// folded value is the rule's identity (coefficient 1.0, bonus 0.0), so
    /// `base * 1.0` or `base + 0.0` both return `base` unchanged. This identity
    /// default is the one legitimate default here and matches the `coef`/`bonus`
    /// accessors.
    pub fn apply(&self, base: f32, name: &str) -> f32 {
        match (self.values.get(name).copied(), self.rules.get(name).copied()) {
            (Some(value), Some(rule)) => rule.apply(base, value),
            _ => base,
        }
    }

    /// Apply several modifiers in sequence (left-to-right), each chosen multiply vs
    /// add by its own classification.
    pub fn apply_all(&self, base: f32, names: &[&str]) -> f32 {
        names.iter().fold(base, |b, n| self.apply(b, n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::CrewSkillModifier;

    /// The version at which the toolkit's `MODIFIER_SETTINGS` table takes effect.
    const VERSION: Version = Version::base(15, 0, 0);

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
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        assert!((bundle.coef("speedCoef") - 0.945).abs() < 1e-6, "got {}", bundle.coef("speedCoef"));
    }

    /// `torpedoSpeedBonus` has base_value 0.0 (ModifiersApply.py:499 uses it as `+`),
    /// so two instances add: 5.0 + 3.0 = 8.0.
    #[test]
    fn same_additive_name_adds() {
        let mods = [modifier("torpedoSpeedBonus", 5.0), modifier("torpedoSpeedBonus", 3.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        assert!((bundle.bonus("torpedoSpeedBonus") - 8.0).abs() < 1e-6, "got {}", bundle.bonus("torpedoSpeedBonus"));
    }

    /// A second additive name (`buffsShiftMaxLevel`, base 0.0, ModifiersApply.py:529
    /// `+`) also adds, confirming the classifier is per-name not per-instance.
    #[test]
    fn second_additive_name_adds() {
        let mods = [modifier("buffsShiftMaxLevel", 2.0), modifier("buffsShiftMaxLevel", 1.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        assert!((bundle.bonus("buffsShiftMaxLevel") - 3.0).abs() < 1e-6);
    }

    /// Absent names read back their identity: coef 1.0, bonus 0.0.
    #[test]
    fn absent_name_is_identity() {
        let bundle =
            ModifierBundle::from_modifiers(&[], Species::Battleship, VERSION).expect("test modifiers are all known");
        assert_eq!(bundle.coef("speedCoef"), 1.0);
        assert_eq!(bundle.bonus("torpedoSpeedBonus"), 0.0);
    }

    /// Resolution picks the slot for the bundle's species.
    #[test]
    fn per_species_resolution() {
        let mods = [modifier_per_species("speedCoef", 0.9, 1.2)];
        let bb =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let ca =
            ModifierBundle::from_modifiers(&mods, Species::Cruiser, VERSION).expect("test modifiers are all known");
        assert!((bb.coef("speedCoef") - 0.9).abs() < 1e-6);
        assert!((ca.coef("speedCoef") - 1.2).abs() < 1e-6);
    }

    /// Captain-skill modifier names absent from the generated table classify via the
    /// allowlists by their GameParams identity value (1.0 -> mult, 0.0 -> add) instead
    /// of erroring as unknown.
    #[test]
    fn captain_skill_table_gaps_classify() {
        let mods = [
            modifier("reloadFactor", 0.9),
            modifier("engineForwardForsagePower", 1.2),
            modifier("hydrophoneWaveSpeedCoeff", 1.0),
            modifier("planeEmptyReturnSpeed", 0.5),
            modifier("healthRegenPercent", 0.05),
        ];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION)
            .expect("captain-skill table-gap modifiers must classify, not error");
        assert!((bundle.coef("reloadFactor") - 0.9).abs() < 1e-6);
        assert!((bundle.coef("engineForwardForsagePower") - 1.2).abs() < 1e-6);
        assert!((bundle.coef("planeEmptyReturnSpeed") - 0.5).abs() < 1e-6);
        assert!((bundle.bonus("healthRegenPercent") - 0.05).abs() < 1e-6);
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
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        assert!((bundle.bonus("yawSpeedBonus") - 6.0).abs() < 1e-6, "got {}", bundle.bonus("yawSpeedBonus"));
    }

    /// Absent accessors return identities regardless of which accessor is used.
    #[test]
    fn absent_name_returns_identity_per_accessor() {
        let bundle =
            ModifierBundle::from_modifiers(&[], Species::Battleship, VERSION).expect("test modifiers are all known");
        assert_eq!(bundle.coef("yawSpeedBonus"), 1.0);
        assert_eq!(bundle.bonus("speedCoef"), 0.0);
        assert_eq!(bundle.coef("nonexistentName"), 1.0);
        assert_eq!(bundle.bonus("nonexistentName"), 0.0);
    }

    /// `apply` uses the multiplicative operator for a coefficient name (`speedCoef`,
    /// base 1.0): base 100 * 0.9 = 90.
    #[test]
    fn apply_multiplies_a_coefficient_name() {
        let mods = [modifier("speedCoef", 0.9)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        assert!((bundle.apply(100.0, "speedCoef") - 90.0).abs() < 1e-4, "got {}", bundle.apply(100.0, "speedCoef"));
    }

    /// `apply` uses the additive operator for a bonus name (`torpedoSpeedBonus`,
    /// base 0.0): base 60 + 5 = 65.
    #[test]
    fn apply_adds_a_bonus_name() {
        let mods = [modifier("torpedoSpeedBonus", 5.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        assert!((bundle.apply(60.0, "torpedoSpeedBonus") - 65.0).abs() < 1e-4);
    }

    /// An absent name is a no-op regardless of kind: `apply` returns the base.
    #[test]
    fn apply_absent_name_is_identity() {
        let bundle =
            ModifierBundle::from_modifiers(&[], Species::Battleship, VERSION).expect("test modifiers are all known");
        assert_eq!(bundle.apply(42.0, "speedCoef"), 42.0);
        assert_eq!(bundle.apply(42.0, "torpedoSpeedBonus"), 42.0);
        assert_eq!(bundle.apply(42.0, "nonexistentName"), 42.0);
    }

    /// `apply_all` chains left-to-right, mixing operators per name: base 100,
    /// then * 0.9 (speedCoef) = 90, then + 5 (torpedoSpeedBonus) = 95.
    #[test]
    fn apply_all_chains_left_to_right() {
        let mods = [modifier("speedCoef", 0.9), modifier("torpedoSpeedBonus", 5.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let out = bundle.apply_all(100.0, &["speedCoef", "torpedoSpeedBonus"]);
        assert!((out - 95.0).abs() < 1e-4, "got {out}");
    }

    /// `apply_all` over no names returns the base unchanged.
    #[test]
    fn apply_all_empty_is_identity() {
        let bundle =
            ModifierBundle::from_modifiers(&[], Species::Battleship, VERSION).expect("test modifiers are all known");
        assert_eq!(bundle.apply_all(7.0, &[]), 7.0);
    }

    /// Reading a present additive name (`yawSpeedBonus`) through `coef()` trips the
    /// classification-mismatch debug_assert in a debug build.
    #[test]
    #[should_panic(expected = "additively-folded")]
    #[cfg(debug_assertions)]
    fn coef_on_additive_name_trips_assert() {
        let mods = [modifier("yawSpeedBonus", 4.0)];
        let bundle =
            ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION).expect("test modifiers are all known");
        let _ = bundle.coef("yawSpeedBonus");
    }

    /// Fail-open: at an OLD version (pre-15.0.0) the default table still classifies
    /// known modifiers, so old replays still get stats. `GMShotDelay` (base 1.0,
    /// coefficient) folds multiplicatively: 0.9 * 0.8 = 0.72.
    #[test]
    fn from_modifiers_classifies_known_names_at_old_version() {
        let old = Version::base(11, 0, 0);
        let mods = [modifier("GMShotDelay", 0.9), modifier("GMShotDelay", 0.8)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Battleship, old)
            .expect("known modifiers classify even at an old version");
        assert!((bundle.coef("GMShotDelay") - 0.72).abs() < 1e-6, "got {}", bundle.coef("GMShotDelay"));
    }

    /// An unrecognized name (not in the table or either allowlist) is an error whose
    /// payload names the offending modifier. It is never silently folded.
    #[test]
    fn from_modifiers_unknown_name_errors_with_name() {
        let mods = [modifier("totallyNotARealModifier", 0.5)];
        let err = ModifierBundle::from_modifiers(&mods, Species::Battleship, VERSION)
            .expect_err("an unknown modifier must error, not silently multiply");
        assert_eq!(err, ModifierError::Unknown(vec!["totallyNotARealModifier".to_string()]), "got {err:?}");
    }

    /// `classify` at the unset version (0.0.0) fails open to the default table and
    /// reads `GMCritProb`'s base 1.0 as a coefficient.
    #[test]
    fn classify_gmcritprob_at_default_version_is_multiply() {
        assert_eq!(Combine::classify(Version::default(), "GMCritProb"), Ok(Combine::Multiply));
    }

    #[test]
    fn modifier_identity_classifies() {
        let v = crate::data::Version::base(15, 4, 0);
        assert_eq!(modifier_identity(v, "GSPriorityTargetIdealRadius").unwrap(), 1.0, "multiplicative");
        assert_eq!(modifier_identity(v, "yawSpeedBonus").unwrap(), 0.0, "additive");
        assert!(
            matches!(modifier_identity(v, "definitelyNotAModifier_xyz"), Err(ModifierError::Unknown(_))),
            "unknown name errors"
        );
    }
}
