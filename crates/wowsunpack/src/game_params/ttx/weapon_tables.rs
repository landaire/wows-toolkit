//! Weapon-type classification tables and the shell damage/burn formulas they feed.
//!
//! Transcribed from the compiled client module `me5c906db` (no Python source; the
//! tables below are recovered from its Python-2.7 bytecode, `BUILD_MAP`/`BUILD_TUPLE`
//! at offsets noted per item) plus `Modifiers/ModifiersApply.py` (which has source).
//!
//! `ammoToStatWeaponTable` (me5c906db BUILD_MAP@4844) maps an ammo-type string to a
//! stat `WeaponType`; `getAmmoToStatWeaponTable(ammo)` is `ammoToStatWeaponTable.get(
//! ammo, default)` (me5c906db getAmmoToStatWeaponTable). The `WEAPONS_*` set membership
//! drives the damage-coefficient branches in `getArtilleryDamageCoeff`
//! (ModifiersApply.py:355) and the CSAP citadel multiplier gate (FactoryArtillery.py:163).
//!
//! Modifier coefficients are read from a [`ModifierBundle`]; the bundle returns each
//! name's `MODIFIER_SETTINGS` identity (1.0 / 0.0) when no upgrade is equipped, so the
//! stock-ship result of every formula here reduces to the bare projectile value.

use crate::game_params::ttx::modifiers::ModifierBundle;

/// `MAX_SMALL_CATEGORY_LEVEL = 7` (me658a8e4.py:16). Tiers above this take the
/// high-level burn-chance branch (ModifiersApply.py:43).
pub const MAX_SMALL_CATEGORY_LEVEL: u32 = 7;

/// Stat weapon type, one identity per `me5c906db` `enum_weapon` slot used by the
/// artillery shell path. Only the artillery (main + ATBA) members are modeled; the
/// avia/torpedo/laser members of the full enum are not reached by shell stats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatWeaponType {
    /// `WEAPON_MAIN_AP` (me5c906db STORE_NAME@1738).
    MainAp,
    /// `WEAPON_MAIN_HE` (me5c906db STORE_NAME@1750).
    MainHe,
    /// `WEAPON_MAIN_CS` (me5c906db STORE_NAME@2110).
    MainCs,
    /// `WEAPON_ATBA_AP` (me5c906db STORE_NAME@1762).
    AtbaAp,
    /// `WEAPON_ATBA_HE` (me5c906db STORE_NAME@1774).
    AtbaHe,
    /// `WEAPON_ATBA_CS` (me5c906db STORE_NAME@2122).
    AtbaCs,
    /// Any non-artillery stat weapon (bombs, torpedoes, lasers, ...). Shell stats never
    /// reach the per-member branches for these; carried so `getAmmoToStatWeaponTable`
    /// can return a faithful "not artillery" answer for the `default` and for ammo types
    /// that map outside the main/ATBA set.
    Other,
}

impl StatWeaponType {
    /// `WEAPONS_MAIN = (WEAPON_MAIN_AP, WEAPON_MAIN_HE, WEAPON_MAIN_CS)`
    /// (me5c906db BUILD_TUPLE@2758).
    pub fn is_main(self) -> bool {
        matches!(self, StatWeaponType::MainAp | StatWeaponType::MainHe | StatWeaponType::MainCs)
    }

    /// `WEAPONS_ATBA = (WEAPON_ATBA_AP, WEAPON_ATBA_HE, WEAPON_ATBA_CS)`
    /// (me5c906db BUILD_TUPLE@2773).
    pub fn is_atba(self) -> bool {
        matches!(self, StatWeaponType::AtbaAp | StatWeaponType::AtbaHe | StatWeaponType::AtbaCs)
    }

    /// `WEAPONS_HECS = (WEAPON_MAIN_HE, WEAPON_MAIN_CS)` (me5c906db BUILD_TUPLE@3214).
    /// Note: main-battery only (the ATBA HE/CS counterpart is `WEAPONS_ATBA_HECS`,
    /// not consulted by `getArtilleryDamageCoeff`).
    pub fn is_hecs(self) -> bool {
        matches!(self, StatWeaponType::MainHe | StatWeaponType::MainCs)
    }

    /// `WEAPONS_CSAP = (WEAPON_ATBA_CS, WEAPON_ATBA_AP, WEAPON_MAIN_AP, WEAPON_MAIN_CS)`
    /// (me5c906db BUILD_TUPLE@3244). The `citadelDamageMultiplierCSAP` gate in
    /// `createAmmoTTX` (FactoryArtillery.py:163). HE members are deliberately absent.
    pub fn is_csap(self) -> bool {
        matches!(
            self,
            StatWeaponType::AtbaCs | StatWeaponType::AtbaAp | StatWeaponType::MainAp | StatWeaponType::MainCs
        )
    }

    /// `artilleryToAtbaStatWeaponTable` (me5c906db BUILD_MAP@5092):
    /// `{WEAPON_MAIN_AP: WEAPON_ATBA_AP, WEAPON_MAIN_HE: WEAPON_ATBA_HE,
    /// WEAPON_MAIN_CS: WEAPON_ATBA_CS}`. `createAmmoTTX` maps a main stat type to its
    /// ATBA equivalent for the ATBA shell path (FactoryArtillery.py:160-161). Returns
    /// `self` for types absent from the table (the `in` guard at FactoryArtillery.py:160).
    pub fn to_atba(self) -> StatWeaponType {
        match self {
            StatWeaponType::MainAp => StatWeaponType::AtbaAp,
            StatWeaponType::MainHe => StatWeaponType::AtbaHe,
            StatWeaponType::MainCs => StatWeaponType::AtbaCs,
            other => other,
        }
    }
}

/// `getAmmoToStatWeaponTable(ammo)` (me5c906db): `ammoToStatWeaponTable.get(ammo,
/// default)`. The dict (BUILD_MAP@4844) keys the ammo-type string to the stat weapon
/// type; for shells the relevant keys are `"AP"/"HE"/"CS"` -> the main-battery members
/// (me5c906db STORE_MAP@4853/4860/4867). Every other ammo string maps outside the
/// artillery set, returned here as [`StatWeaponType::Other`].
pub fn ammo_to_stat_weapon_table(ammo: &str) -> StatWeaponType {
    match ammo {
        "AP" => StatWeaponType::MainAp,
        "HE" => StatWeaponType::MainHe,
        "CS" => StatWeaponType::MainCs,
        _ => StatWeaponType::Other,
    }
}

/// `getAlphaDamageCoeff(ammoParams, modifier)` for the artillery path
/// (ModifiersApply.py:430-474).
///
/// The rocket/bomb/depth-charge species branches (lines 434-453) never apply to a
/// `Projectile` of `species == Artillery`, so this models the `weaponType`-keyed tail
/// (lines 458-466): `allAlphaMultiplier` times `GMAlphaFactor` for main, or
/// `GSAlphaFactor` (and `GSMAlphaFactor` in alt mode) for ATBA. `is_alt_mode` carries
/// `weaponMode & WeaponModeFlags.ALT_MODE` (line 465); stock shells are not alt-mode.
pub fn alpha_damage_coeff(weapon: StatWeaponType, modifier: &ModifierBundle, is_alt_mode: bool) -> f32 {
    let mut coeff = modifier.coef("allAlphaMultiplier");
    if weapon.is_main() {
        coeff *= modifier.coef("GMAlphaFactor");
    } else if weapon.is_atba() {
        coeff *= modifier.coef("GSAlphaFactor");
        if is_alt_mode {
            coeff *= modifier.coef("GSMAlphaFactor");
        }
    }
    coeff
}

/// `getArtilleryDamageCoeff(caliber, statWeaponType, modifier, onFire=False)`
/// (ModifiersApply.py:355-392). `caliber` is in meters (`preprocessedAmmo.caliber`,
/// FactoryArtillery.py:164 passes `preprocessedAmmo.caliber`); `onFire` is always
/// `False` in the TTX factory path (it has no default override), so the
/// `HEDamageCoeffIfOnFire` branch (line 370/388) is never taken here.
///
/// `caliber_fits_heavy_cruiser` is `caliber >= HEAVY_CRUISER_SHELL_DIAMETER`
/// (ModifiersApply.py:26). That threshold constant is `0.0` in every deob source
/// (me658a8e4.py:15 is a zeroed placeholder; the real value lives in a compiled C++
/// module), so the heavy-cruiser AP sub-multiplier cannot be reproduced from static
/// data. It is gated behind `GMHeavyCruiserCaliberDamageCoeff`, a coefficient whose
/// stock identity is 1.0, so the stock result is unaffected; the caller passes the
/// faithful threshold once it is recovered.
pub fn artillery_damage_coeff(
    caliber_m: f32,
    weapon: StatWeaponType,
    modifier: &ModifierBundle,
    heavy_cruiser_shell_diameter_m: f32,
) -> f32 {
    if weapon.is_main() {
        let mut y = modifier.coef("GMDamageCoeff");
        match weapon {
            StatWeaponType::MainAp => {
                y *= modifier.coef("GMAPDamageCoeff");
                if caliber_m >= heavy_cruiser_shell_diameter_m {
                    y *= modifier.coef("GMHeavyCruiserCaliberDamageCoeff");
                }
            }
            StatWeaponType::MainCs => y *= modifier.coef("GMCSDamageCoeff"),
            // MainHe: only the onFire branch (never taken in the TTX path) would apply.
            _ => {}
        }
        if weapon.is_hecs() {
            y *= modifier.coef("GMHECSDamageCoeff");
        }
        return y;
    }

    if weapon.is_atba() {
        let mut y = modifier.coef("GSDamageCoeff");
        match weapon {
            StatWeaponType::AtbaCs => y *= modifier.coef("GSCSDamageCoeff"),
            StatWeaponType::AtbaAp => y *= modifier.coef("GSAPDamageCoeff"),
            // AtbaHe: only the onFire branch (never taken in the TTX path) would apply.
            _ => {}
        }
        return y;
    }

    1.0
}

/// Whether a projectile counts as "small" for the burn-chance factor split
/// (`isSmallProjectile`, Modifiers/__init__.py:5-12). Artillery shells take the
/// `bulletDiametr <= SMALL_PROJECTILE_MAX_DIAMETER` test (line 10). That threshold is
/// `0.0` in every deob source (me658a8e4.py:13 is a zeroed placeholder; real value is
/// in a compiled C++ module), so the caller supplies it. The selected factor
/// (`burnChanceFactorSmall` vs `burnChanceFactorBig`) is `0.0` for both in the stock
/// `MODIFIER_SETTINGS`, so the stock burn chance is unaffected by which side is chosen.
pub fn is_small_projectile(bullet_diametr_m: f32, small_projectile_max_diameter_m: f32) -> bool {
    bullet_diametr_m <= small_projectile_max_diameter_m
}

/// `calculateBurnChance(ammoOwnerLevel, ammoParams, modifier, initialBurnProb)` for the
/// artillery path (ModifiersApply.py:39-66).
///
/// `ammo_owner_level` is the ship tier; tiers above [`MAX_SMALL_CATEGORY_LEVEL`] take
/// `burnChanceFactorHighLevel`, the rest `burnChanceFactorLowLevel` (lines 43-46). The
/// ARTILLERY species tail then applies `burnChanceGMGSMultiplier` (line 48) and adds
/// `artilleryBurnChanceBonus` (line 49). The species-agnostic tail multiplies
/// `burnChanceMultiplier`, adds `burnChanceBonus` (lines 57-58), and adds the
/// small/big factor (lines 60-65). `max(result, 0)` clamps (line 66).
///
/// `is_small` is [`is_small_projectile`]'s answer for this shell. Every coefficient
/// here reads its `MODIFIER_SETTINGS` identity from the stock bundle
/// (`*HighLevel`/`*LowLevel`/`GMGSMultiplier`/`Multiplier` -> 1.0,
/// `*Bonus`/`*Small`/`*Big` -> 0.0), so the stock burn chance is exactly
/// `max(initialBurnProb, 0)`.
pub fn calculate_burn_chance(
    ammo_owner_level: u32,
    initial_burn_prob: f32,
    modifier: &ModifierBundle,
    is_small: bool,
) -> f32 {
    let mut prob = initial_burn_prob;
    if ammo_owner_level > MAX_SMALL_CATEGORY_LEVEL {
        prob *= modifier.coef("burnChanceFactorHighLevel");
    } else {
        prob *= modifier.coef("burnChanceFactorLowLevel");
    }
    prob *= modifier.coef("burnChanceGMGSMultiplier");
    prob += modifier.bonus("artilleryBurnChanceBonus");

    prob *= modifier.coef("burnChanceMultiplier");
    prob += modifier.bonus("burnChanceBonus");

    prob += if is_small {
        modifier.bonus("burnChanceFactorSmall")
    } else {
        modifier.bonus("burnChanceFactorBig")
    };

    prob.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::{CrewSkillModifier, Species};

    /// Build whose `MODIFIER_SETTINGS` table is transcribed in the toolkit (matches
    /// modifiers.rs tests).
    const BUILD: u32 = 11791718;

    fn stock() -> ModifierBundle {
        ModifierBundle::empty(Species::Cruiser)
    }

    /// One modifier value applied to every species slot, so the bundle resolves to it
    /// regardless of the species it is built for.
    fn modifier(name: &str, value: f32) -> CrewSkillModifier {
        CrewSkillModifier::builder()
            .name(name.to_string())
            .aircraft_carrier(value)
            .auxiliary(value)
            .battleship(value)
            .cruiser(value)
            .destroyer(value)
            .submarine(value)
            .excluded_consumables(Vec::new())
            .build()
    }

    #[test]
    fn ammo_strings_map_to_main_weapon() {
        assert_eq!(ammo_to_stat_weapon_table("HE"), StatWeaponType::MainHe);
        assert_eq!(ammo_to_stat_weapon_table("AP"), StatWeaponType::MainAp);
        assert_eq!(ammo_to_stat_weapon_table("CS"), StatWeaponType::MainCs);
        assert_eq!(ammo_to_stat_weapon_table("torpedo"), StatWeaponType::Other);
    }

    /// WEAPONS_CSAP excludes the HE members (me5c906db BUILD_TUPLE@3244).
    #[test]
    fn csap_set_membership() {
        assert!(StatWeaponType::MainAp.is_csap());
        assert!(StatWeaponType::MainCs.is_csap());
        assert!(StatWeaponType::AtbaAp.is_csap());
        assert!(StatWeaponType::AtbaCs.is_csap());
        assert!(!StatWeaponType::MainHe.is_csap());
        assert!(!StatWeaponType::AtbaHe.is_csap());
    }

    /// WEAPONS_HECS is main-battery HE+CS only (me5c906db BUILD_TUPLE@3214).
    #[test]
    fn hecs_set_membership() {
        assert!(StatWeaponType::MainHe.is_hecs());
        assert!(StatWeaponType::MainCs.is_hecs());
        assert!(!StatWeaponType::MainAp.is_hecs());
        assert!(!StatWeaponType::AtbaHe.is_hecs());
    }

    #[test]
    fn main_to_atba_mapping() {
        assert_eq!(StatWeaponType::MainAp.to_atba(), StatWeaponType::AtbaAp);
        assert_eq!(StatWeaponType::MainHe.to_atba(), StatWeaponType::AtbaHe);
        assert_eq!(StatWeaponType::MainCs.to_atba(), StatWeaponType::AtbaCs);
        // Absent from the table -> returned unchanged.
        assert_eq!(StatWeaponType::AtbaAp.to_atba(), StatWeaponType::AtbaAp);
    }

    /// Stock alpha-damage coeff is the identity 1.0 (allAlphaMultiplier * GMAlphaFactor,
    /// both stock 1.0).
    #[test]
    fn stock_alpha_damage_coeff_is_one() {
        assert_eq!(alpha_damage_coeff(StatWeaponType::MainHe, &stock(), false), 1.0);
        assert_eq!(alpha_damage_coeff(StatWeaponType::AtbaHe, &stock(), false), 1.0);
    }

    /// allAlphaMultiplier and GMAlphaFactor both fold into the main coeff.
    #[test]
    fn alpha_damage_coeff_folds_main_factors() {
        let mods = [modifier("allAlphaMultiplier", 1.1), modifier("GMAlphaFactor", 1.2)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Cruiser, BUILD);
        let got = alpha_damage_coeff(StatWeaponType::MainAp, &bundle, false);
        assert!((got - 1.32).abs() < 1e-5, "got {got}");
    }

    /// Stock artillery-damage coeff is 1.0 for a main HE shell (GMDamageCoeff *
    /// GMHECSDamageCoeff, both stock 1.0; no AP/CS sub-coeff).
    #[test]
    fn stock_artillery_damage_coeff_he_is_one() {
        let got = artillery_damage_coeff(0.152, StatWeaponType::MainHe, &stock(), 0.149);
        assert_eq!(got, 1.0);
    }

    /// Main AP below the heavy-cruiser threshold takes GMDamageCoeff * GMAPDamageCoeff
    /// only; main AP at/above it additionally multiplies GMHeavyCruiserCaliberDamageCoeff.
    #[test]
    fn artillery_damage_coeff_ap_heavy_cruiser_gate() {
        let mods = [
            modifier("GMAPDamageCoeff", 1.5),
            modifier("GMHeavyCruiserCaliberDamageCoeff", 2.0),
        ];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Cruiser, BUILD);
        // caliber 0.152 < threshold 0.190 -> heavy-cruiser coeff NOT applied: 1.5.
        let below = artillery_damage_coeff(0.152, StatWeaponType::MainAp, &bundle, 0.190);
        assert!((below - 1.5).abs() < 1e-5, "got {below}");
        // caliber 0.203 >= threshold 0.190 -> heavy-cruiser coeff applied: 1.5 * 2.0.
        let above = artillery_damage_coeff(0.203, StatWeaponType::MainAp, &bundle, 0.190);
        assert!((above - 3.0).abs() < 1e-5, "got {above}");
    }

    /// Stock burn chance for a tier-10 (high-level) HE shell equals its bare burnProb:
    /// every burn modifier reads its identity (factors 1.0, bonuses 0.0).
    #[test]
    fn stock_burn_chance_high_level_is_burn_prob() {
        let got = calculate_burn_chance(10, 0.12, &stock(), false);
        assert!((got - 0.12).abs() < 1e-6, "got {got}");
    }

    /// Stock burn chance for a low-tier shell is likewise its burnProb.
    #[test]
    fn stock_burn_chance_low_level_is_burn_prob() {
        let got = calculate_burn_chance(5, 0.08, &stock(), true);
        assert!((got - 0.08).abs() < 1e-6, "got {got}");
    }

    /// Hand-computed against the transcribed formula with non-identity modifiers, a
    /// tier-10 (high-level) shell:
    /// `((0.12 * 0.9) * 0.95 + 0.02) * 1.1 + 0.01 + (-0.03)` with `burnChanceFactorBig`.
    #[test]
    fn burn_chance_full_formula_high_level() {
        let mods = [
            modifier("burnChanceFactorHighLevel", 0.9),
            modifier("burnChanceGMGSMultiplier", 0.95),
            modifier("artilleryBurnChanceBonus", 0.02),
            modifier("burnChanceMultiplier", 1.1),
            modifier("burnChanceBonus", 0.01),
            modifier("burnChanceFactorBig", -0.03),
        ];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Cruiser, BUILD);
        let got = calculate_burn_chance(10, 0.12, &bundle, false);
        // ((0.12*0.9)*0.95 + 0.02)*1.1 + 0.01 - 0.03
        let expected = ((0.12f32 * 0.9 * 0.95) + 0.02) * 1.1 + 0.01 - 0.03;
        assert!((got - expected).abs() < 1e-6, "got {got} expected {expected}");
    }

    /// Negative results clamp to 0 (max(prob, 0), ModifiersApply.py:66).
    #[test]
    fn burn_chance_clamps_to_zero() {
        let mods = [modifier("burnChanceBonus", -1.0)];
        let bundle = ModifierBundle::from_modifiers(&mods, Species::Cruiser, BUILD);
        let got = calculate_burn_chance(10, 0.12, &bundle, false);
        assert_eq!(got, 0.0);
    }

    /// AP shells carry burnProb -0.5 (the "N/A" sentinel); clamped to 0.
    #[test]
    fn burn_chance_ap_na_clamps_to_zero() {
        let got = calculate_burn_chance(10, -0.5, &stock(), false);
        assert_eq!(got, 0.0);
    }

    #[test]
    fn small_projectile_threshold() {
        assert!(is_small_projectile(0.1, 0.139));
        assert!(!is_small_projectile(0.152, 0.139));
    }
}
