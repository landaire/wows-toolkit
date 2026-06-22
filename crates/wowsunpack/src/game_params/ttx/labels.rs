//! Translated-label sourcing for TTX ship-stat fields.
//!
//! Each [`TtxStat`] names one displayed stat field from [`super::model`]. The
//! port stat-card labels live in the gettext catalog (`global.mo`) under the
//! `IDS_SHIP_PARAM_*` namespace; [`TtxStat::label_key`] returns the catalog key
//! whose English value names that stat, or `None` when no confident match
//! exists. [`stat_label`] resolves the key through a [`ResourceLoader`].
//!
//! No English fallback is baked in here: a stat with no key, or a key the
//! loader cannot resolve, yields `None`, and the caller (UI) decides on a
//! field-name fallback. Labels are never invented; an unverified stat is `None`.
//!
//! The secondary battery reuses the [`super::model::Artillery`] type, but the
//! client labels its rows with `IDS_SHIP_PARAM_ATBA_*` keys. The `Secondary*`
//! variants here carry those ATBA labels; the en `global.mo` only defines a
//! distinct ATBA key for range and gun count, so the remaining secondary stats
//! yield `None` rather than borrowing a main-battery label.

use crate::data::ResourceLoader;
use crate::data::TranslationKey;

/// One displayed TTX stat field. Variants are aligned with the leaf-struct
/// fields in [`super::model`]; gun/shell/launcher/torpedo sub-fields are flat
/// here so the UI can request a label per row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TtxStat {
    // Durability
    Health,
    TorpedoProtection,
    // Mobility
    Speed,
    TurningRadius,
    RudderTime,
    // Armor
    ArmorMin,
    ArmorMax,
    // Battery (submarine dive capacity)
    BatteryCapacity,
    BatteryRegeneration,
    // Artillery (main battery)
    ArtilleryReloadTime,
    ArtilleryRange,
    ArtilleryDispersion,
    ArtilleryDispersionVertical,
    ArtilleryAmmoSwitchTime,
    // Main gun mount
    GunCaliber,
    GunNumBarrels,
    GunNumGuns,
    GunRotationSpeed,
    GunRotationTime,
    // Shell
    ShellDamage,
    ShellCaliber,
    ShellSpeed,
    ShellPenetration,
    ShellBurnChance,
    ShellFloodChance,
    ShellMaxAmmo,
    ShellDisabledUnderwater,
    // Secondaries (ATBA): same Artillery model as the main battery, but the
    // client labels secondary rows with IDS_SHIP_PARAM_ATBA_* keys. Only range
    // and gun-count have distinct catalog keys; the rest have no ATBA label.
    SecondaryRange,
    SecondaryReloadTime,
    SecondaryDispersion,
    SecondaryDispersionVertical,
    SecondaryAmmoSwitchTime,
    SecondaryGunCaliber,
    SecondaryGunNumBarrels,
    SecondaryGunNumGuns,
    SecondaryGunRotationSpeed,
    SecondaryGunRotationTime,
    SecondaryShellDamage,
    SecondaryShellCaliber,
    SecondaryShellSpeed,
    SecondaryShellPenetration,
    SecondaryShellBurnChance,
    SecondaryShellFloodChance,
    SecondaryShellMaxAmmo,
    // Torpedoes
    TorpedoReloadTime,
    // Launcher
    LauncherRotationSpeed,
    LauncherRotationTime,
    LauncherNumBarrels,
    // Torpedo ammo
    TorpedoDamage,
    TorpedoSpeed,
    TorpedoRange,
    TorpedoVisibility,
    TorpedoDistanceOfMaxDamage,
    TorpedoIsDamageIncreasing,
    TorpedoDisabledUnderwater,
    // Fire control
    FireControlMaxDist,
    // Visibility
    SeaDetection,
    SeaDetectionOnFire,
    AirDetection,
    AirDetectionOnFire,
    DetectionInSmoke,
    SecondaryRangeDetection,
    PeriscopeDepthDetection,
}

impl TtxStat {
    /// Every stat, for coverage tests and exhaustive UI iteration.
    pub const ALL: &'static [TtxStat] = &[
        TtxStat::Health,
        TtxStat::TorpedoProtection,
        TtxStat::Speed,
        TtxStat::TurningRadius,
        TtxStat::RudderTime,
        TtxStat::ArmorMin,
        TtxStat::ArmorMax,
        TtxStat::BatteryCapacity,
        TtxStat::BatteryRegeneration,
        TtxStat::ArtilleryReloadTime,
        TtxStat::ArtilleryRange,
        TtxStat::ArtilleryDispersion,
        TtxStat::ArtilleryDispersionVertical,
        TtxStat::ArtilleryAmmoSwitchTime,
        TtxStat::GunCaliber,
        TtxStat::GunNumBarrels,
        TtxStat::GunNumGuns,
        TtxStat::GunRotationSpeed,
        TtxStat::GunRotationTime,
        TtxStat::ShellDamage,
        TtxStat::ShellCaliber,
        TtxStat::ShellSpeed,
        TtxStat::ShellPenetration,
        TtxStat::ShellBurnChance,
        TtxStat::ShellFloodChance,
        TtxStat::ShellMaxAmmo,
        TtxStat::ShellDisabledUnderwater,
        TtxStat::SecondaryRange,
        TtxStat::SecondaryReloadTime,
        TtxStat::SecondaryDispersion,
        TtxStat::SecondaryDispersionVertical,
        TtxStat::SecondaryAmmoSwitchTime,
        TtxStat::SecondaryGunCaliber,
        TtxStat::SecondaryGunNumBarrels,
        TtxStat::SecondaryGunNumGuns,
        TtxStat::SecondaryGunRotationSpeed,
        TtxStat::SecondaryGunRotationTime,
        TtxStat::SecondaryShellDamage,
        TtxStat::SecondaryShellCaliber,
        TtxStat::SecondaryShellSpeed,
        TtxStat::SecondaryShellPenetration,
        TtxStat::SecondaryShellBurnChance,
        TtxStat::SecondaryShellFloodChance,
        TtxStat::SecondaryShellMaxAmmo,
        TtxStat::TorpedoReloadTime,
        TtxStat::LauncherRotationSpeed,
        TtxStat::LauncherRotationTime,
        TtxStat::LauncherNumBarrels,
        TtxStat::TorpedoDamage,
        TtxStat::TorpedoSpeed,
        TtxStat::TorpedoRange,
        TtxStat::TorpedoVisibility,
        TtxStat::TorpedoDistanceOfMaxDamage,
        TtxStat::TorpedoIsDamageIncreasing,
        TtxStat::TorpedoDisabledUnderwater,
        TtxStat::FireControlMaxDist,
        TtxStat::SeaDetection,
        TtxStat::SeaDetectionOnFire,
        TtxStat::AirDetection,
        TtxStat::AirDetectionOnFire,
        TtxStat::DetectionInSmoke,
        TtxStat::SecondaryRangeDetection,
        TtxStat::PeriscopeDepthDetection,
    ];

    /// The canonical struct-field key for this stat (UI fallback when no
    /// translated label is available). Stable, ASCII, never invented.
    pub fn field_key(self) -> &'static str {
        match self {
            TtxStat::Health => "durability.health",
            TtxStat::TorpedoProtection => "durability.torpedo_protection",
            TtxStat::Speed => "mobility.speed",
            TtxStat::TurningRadius => "mobility.turning_radius",
            TtxStat::RudderTime => "mobility.rudder_time",
            TtxStat::ArmorMin => "armor.min",
            TtxStat::ArmorMax => "armor.max",
            TtxStat::BatteryCapacity => "battery.capacity",
            TtxStat::BatteryRegeneration => "battery.regeneration",
            TtxStat::ArtilleryReloadTime => "artillery.reload_time",
            TtxStat::ArtilleryRange => "artillery.range",
            TtxStat::ArtilleryDispersion => "artillery.dispersion",
            TtxStat::ArtilleryDispersionVertical => "artillery.dispersion_vertical",
            TtxStat::ArtilleryAmmoSwitchTime => "artillery.ammo_switch_time",
            TtxStat::GunCaliber => "artillery.gun.caliber",
            TtxStat::GunNumBarrels => "artillery.gun.num_barrels",
            TtxStat::GunNumGuns => "artillery.gun.num_guns",
            TtxStat::GunRotationSpeed => "artillery.gun.rotation_speed",
            TtxStat::GunRotationTime => "artillery.gun.rotation_time",
            TtxStat::ShellDamage => "artillery.shells.damage",
            TtxStat::ShellCaliber => "artillery.shells.caliber",
            TtxStat::ShellSpeed => "artillery.shells.speed",
            TtxStat::ShellPenetration => "artillery.shells.penetration",
            TtxStat::ShellBurnChance => "artillery.shells.burn_chance",
            TtxStat::ShellFloodChance => "artillery.shells.flood_chance",
            TtxStat::ShellMaxAmmo => "artillery.shells.max_ammo",
            TtxStat::ShellDisabledUnderwater => "artillery.shells.disabled_underwater",
            TtxStat::SecondaryRange => "secondary.range",
            TtxStat::SecondaryReloadTime => "secondary.reload_time",
            TtxStat::SecondaryDispersion => "secondary.dispersion",
            TtxStat::SecondaryDispersionVertical => "secondary.dispersion_vertical",
            TtxStat::SecondaryAmmoSwitchTime => "secondary.ammo_switch_time",
            TtxStat::SecondaryGunCaliber => "secondary.gun.caliber",
            TtxStat::SecondaryGunNumBarrels => "secondary.gun.num_barrels",
            TtxStat::SecondaryGunNumGuns => "secondary.gun.num_guns",
            TtxStat::SecondaryGunRotationSpeed => "secondary.gun.rotation_speed",
            TtxStat::SecondaryGunRotationTime => "secondary.gun.rotation_time",
            TtxStat::SecondaryShellDamage => "secondary.shells.damage",
            TtxStat::SecondaryShellCaliber => "secondary.shells.caliber",
            TtxStat::SecondaryShellSpeed => "secondary.shells.speed",
            TtxStat::SecondaryShellPenetration => "secondary.shells.penetration",
            TtxStat::SecondaryShellBurnChance => "secondary.shells.burn_chance",
            TtxStat::SecondaryShellFloodChance => "secondary.shells.flood_chance",
            TtxStat::SecondaryShellMaxAmmo => "secondary.shells.max_ammo",
            TtxStat::TorpedoReloadTime => "torpedoes.reload_time",
            TtxStat::LauncherRotationSpeed => "torpedoes.launchers.rotation_speed",
            TtxStat::LauncherRotationTime => "torpedoes.launchers.rotation_time",
            TtxStat::LauncherNumBarrels => "torpedoes.launchers.num_barrels",
            TtxStat::TorpedoDamage => "torpedoes.torpedoes.damage",
            TtxStat::TorpedoSpeed => "torpedoes.torpedoes.speed",
            TtxStat::TorpedoRange => "torpedoes.torpedoes.range",
            TtxStat::TorpedoVisibility => "torpedoes.torpedoes.visibility",
            TtxStat::TorpedoDistanceOfMaxDamage => "torpedoes.torpedoes.distance_of_max_damage",
            TtxStat::TorpedoIsDamageIncreasing => "torpedoes.torpedoes.is_damage_increasing",
            TtxStat::TorpedoDisabledUnderwater => "torpedoes.torpedoes.disabled_underwater",
            TtxStat::FireControlMaxDist => "fire_control.max_dist",
            TtxStat::SeaDetection => "visibility.sea_detection",
            TtxStat::SeaDetectionOnFire => "visibility.sea_detection_on_fire",
            TtxStat::AirDetection => "visibility.air_detection",
            TtxStat::AirDetectionOnFire => "visibility.air_detection_on_fire",
            TtxStat::DetectionInSmoke => "visibility.detection_in_smoke",
            TtxStat::SecondaryRangeDetection => "visibility.secondary_range_detection",
            TtxStat::PeriscopeDepthDetection => "visibility.periscope_depth_detection",
        }
    }

    /// A human-readable label derived from `field_key`, for stats with no catalog
    /// (port) label. E.g. `artillery.dispersion_vertical` -> `"Artillery Dispersion
    /// Vertical"`. Always non-empty.
    pub fn fallback_label(self) -> String {
        self.field_key()
            .split(['.', '_'])
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The `global.mo` `IDS_*` label key for this stat, or `None` when no
    /// catalog entry confidently names the stat. Each key is annotated with the
    /// English value verified from the `en` `global.mo` (build 12668706).
    pub fn label_key(self) -> Option<&'static str> {
        Some(match self {
            // "Hit Points"
            TtxStat::Health => "IDS_SHIP_PARAM_HEALTH",
            // "Torpedo Protection: Damage Reduction"
            TtxStat::TorpedoProtection => "IDS_SHIP_PARAM_PTZDAMAGEPROB",
            // "Maximum Speed"
            TtxStat::Speed => "IDS_SHIP_PARAM_MAXSPEED",
            // "Turning Circle Radius"
            TtxStat::TurningRadius => "IDS_SHIP_PARAM_TURNINGRADIUS",
            // "Rudder Shift Time"
            TtxStat::RudderTime => "IDS_SHIP_PARAM_RUDDER_TIME",
            // "Dive Capacity"
            TtxStat::BatteryCapacity => "IDS_SHIP_PARAM_BATTERY_MAX_CAPACITY",
            // "Dive Capacity Recharge Rate"
            TtxStat::BatteryRegeneration => "IDS_SHIP_PARAM_BATTERY_REGEN_RATE",
            // "Main Battery Reload Time"
            TtxStat::ArtilleryReloadTime => "IDS_SHIP_PARAM_ARTILLERY_TIME_RELOAD",
            // "Main Battery Firing Range"
            TtxStat::ArtilleryRange => "IDS_SHIP_PARAM_ARTILLERY_MAX_DIST",
            // "Maximum Dispersion"
            TtxStat::ArtilleryDispersion => "IDS_SHIP_PARAM_DISPERSION",
            // "Minimum Shell Type Switching Time"
            TtxStat::ArtilleryAmmoSwitchTime => "IDS_SHIP_PARAM_ARTILLERY_MIN_SWITCH_TIME",
            // "Caliber"
            TtxStat::GunCaliber => "IDS_SHIP_PARAM_ARTILLERY_CALIBER",
            // "Main Turrets" (gun-mount count)
            TtxStat::GunNumGuns => "IDS_SHIP_PARAM_ARTILLERY_GUNS_COUNT",
            // "Main Turret Traverse Speed"
            TtxStat::GunRotationSpeed => "IDS_SHIP_PARAM_ARTILLERY_ROTATION_SPEED",
            // "180 Turn Time" (catalog value carries a degree sign)
            TtxStat::GunRotationTime => "IDS_SHIP_PARAM_ROTATION_TIME",
            // "Maximum Damage"
            TtxStat::ShellDamage => "IDS_SHIP_PARAM_ARTILLERY_MAX_DAMAGE",
            // "Caliber"
            TtxStat::ShellCaliber => "IDS_SHIP_PARAM_ARTILLERY_CALIBER",
            // "Initial Velocity"
            TtxStat::ShellSpeed => "IDS_SHIP_PARAM_ARTILLERY_AMMO_SPEED",
            // "Armor Penetration Capacity"
            TtxStat::ShellPenetration => "IDS_SHIP_PARAM_ARTILLERY_ALPHA_PIERCING",
            // "Chances of Causing a Fire on Target"
            TtxStat::ShellBurnChance => "IDS_SHIP_PARAM_ARTILLERY_BURN_PROB",
            // "Chances of Causing Flooding"
            TtxStat::ShellFloodChance => "IDS_SHIP_PARAM_ARTILLERY_FLOOD_GENERATION",
            // "Number of Shells"
            TtxStat::ShellMaxAmmo => "IDS_SHIP_PARAM_ARTILLERY_MAX_AMMO_COUNT",
            // "Firing Range" (secondary-battery range)
            TtxStat::SecondaryRange => "IDS_SHIP_PARAM_ATBA_MAX_DIST",
            // "Secondary Gun Turrets" (secondary mount count)
            TtxStat::SecondaryGunNumGuns => "IDS_SHIP_PARAM_ATBA_GUNS_COUNT",
            // "Torpedo Tube Reload Time"
            TtxStat::TorpedoReloadTime => "IDS_SHIP_PARAM_TORPEDOES_TIME_RELOAD",
            // "Torpedo Tube Traverse Speed"
            TtxStat::LauncherRotationSpeed => "IDS_SHIP_PARAM_TORPEDOES_ROTATION_SPEED",
            // "180 Turn Time" (catalog value carries a degree sign)
            TtxStat::LauncherRotationTime => "IDS_SHIP_PARAM_ROTATION_TIME",
            // "Torpedo Tubes" (tube count)
            TtxStat::LauncherNumBarrels => "IDS_SHIP_PARAM_TORPEDOES_GUNS_COUNT",
            // "Maximum Damage"
            TtxStat::TorpedoDamage => "IDS_SHIP_PARAM_TORPEDO_DAMAGE",
            // "Torpedo Speed"
            TtxStat::TorpedoSpeed => "IDS_SHIP_PARAM_TORPEDO_SPEED",
            // "Torpedo Range"
            TtxStat::TorpedoRange => "IDS_SHIP_PARAM_TORPEDO_MAX_DIST",
            // "Torpedo Detectability Range by Sea"
            TtxStat::TorpedoVisibility => "IDS_SHIP_PARAM_TORPEDO_VISIBILITY_DIST",
            // "Maximum Damage Threshold"
            TtxStat::TorpedoDistanceOfMaxDamage => "IDS_SHIP_PARAM_TORPEDO_DISTANCE_OF_MAX_DAMAGE",
            // "Cannot be launched at maximum depth"
            TtxStat::TorpedoDisabledUnderwater => "IDS_SHIP_PARAM_TORPEDO_DISABLED_UNDERWATER",
            // "Maximum Firing Range"
            TtxStat::FireControlMaxDist => "IDS_SHIP_PARAM_MAXIMUM_DISTANCE",
            // "Detectability Range by Sea"
            TtxStat::SeaDetection => "IDS_SHIP_PARAM_VISIBILITY_DIST_BY_SHIP",
            // "Detectability When Ship Is on Fire"
            TtxStat::SeaDetectionOnFire => "IDS_SHIP_PARAM_VISIBILITY_DIST_BY_FIRE",
            // "Detectability Range by Air"
            TtxStat::AirDetection => "IDS_SHIP_PARAM_VISIBILITY_DIST_BY_PLANE",
            // "Detectability after firing a secondary gun shell" (visibilityByShip.atba slot)
            TtxStat::SecondaryRangeDetection => "IDS_SHIP_PARAM_VISIBILITY_DIST_BY_ATBA",
            // "At periscope depth"
            TtxStat::PeriscopeDepthDetection => "IDS_SHIP_PARAM_VISIBILITY_DIST_BY_DEPTH_PERISCOPE",

            // No confident catalog match: the section header (IDS_SHIP_PARAM_ARMOR
            // = "Armor") names the whole armor block, not min/max thickness.
            TtxStat::ArmorMin | TtxStat::ArmorMax => return None,
            // No port label: the client shows only horizontal "Maximum Dispersion".
            TtxStat::ArtilleryDispersionVertical => return None,
            // Barrels-per-mount has no distinct port label (only mount/tube counts).
            TtxStat::GunNumBarrels => return None,
            // No shell-specific underwater label (only the torpedo one exists).
            TtxStat::ShellDisabledUnderwater => return None,
            // No flag label for whether torpedo damage scales with distance.
            TtxStat::TorpedoIsDamageIncreasing => return None,
            // No air-on-fire detection label (only the sea-on-fire one exists).
            TtxStat::AirDetectionOnFire => return None,
            // Smoke catalog keys all describe detection after firing in smoke,
            // not the baseline visibilityByShip.smoke slot this field models.
            TtxStat::DetectionInSmoke => return None,

            // Secondaries: the en global.mo only defines IDS_SHIP_PARAM_ATBA_*
            // for range ("Firing Range") and gun count ("Secondary Gun Turrets").
            // No distinct ATBA key exists for reload, dispersion, ammo-switch,
            // gun caliber/barrels/rotation, or any shell stat (damage, caliber,
            // speed, penetration, burn, flood, ammo), so each is None rather than
            // reusing a main-battery IDS_SHIP_PARAM_ARTILLERY_* label.
            TtxStat::SecondaryReloadTime
            | TtxStat::SecondaryDispersion
            | TtxStat::SecondaryDispersionVertical
            | TtxStat::SecondaryAmmoSwitchTime
            | TtxStat::SecondaryGunCaliber
            | TtxStat::SecondaryGunNumBarrels
            | TtxStat::SecondaryGunRotationSpeed
            | TtxStat::SecondaryGunRotationTime
            | TtxStat::SecondaryShellDamage
            | TtxStat::SecondaryShellCaliber
            | TtxStat::SecondaryShellSpeed
            | TtxStat::SecondaryShellPenetration
            | TtxStat::SecondaryShellBurnChance
            | TtxStat::SecondaryShellFloodChance
            | TtxStat::SecondaryShellMaxAmmo => return None,
        })
    }
}

/// Resolve the translated port label for `stat` via `resource_loader`, or
/// `None` when no `IDS_*` key is known or the loader cannot resolve it. The
/// caller decides on any English/field-name fallback.
pub fn stat_label(stat: TtxStat, resource_loader: &dyn ResourceLoader) -> Option<String> {
    let key = stat.label_key()?;
    resource_loader.localized_name_from_id(&TranslationKey::new(key))
}

/// The display label for a stat: the localized catalog label, or a humanized fallback
/// from `field_key` when the catalog has no entry. Always non-empty -- callers never
/// have to handle a missing label.
pub fn stat_display_label(stat: TtxStat, loader: &dyn ResourceLoader) -> String {
    stat_label(stat, loader).unwrap_or_else(|| stat.fallback_label())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_params::types::Param;
    use crate::game_types::GameParamId;
    use crate::rpc::entitydefs::EntitySpec;

    /// Echoes any requested id back as its own "translation".
    struct EchoLoader;
    impl ResourceLoader for EchoLoader {
        fn localized_name_from_param(&self, _p: &Param) -> Option<String> {
            None
        }
        fn localized_name_from_id(&self, id: &crate::data::TranslationKey) -> Option<String> {
            Some(id.as_str().to_string())
        }
        fn game_param_by_id(&self, _id: GameParamId) -> Option<crate::Rc<Param>> {
            None
        }
        fn entity_specs(&self) -> &[EntitySpec] {
            &[]
        }
    }

    #[test]
    fn known_key_resolves_through_loader() {
        let label = stat_label(TtxStat::Health, &EchoLoader);
        assert_eq!(label.as_deref(), Some("IDS_SHIP_PARAM_HEALTH"));
    }

    #[test]
    fn secondary_range_resolves_to_atba_key() {
        let label = stat_label(TtxStat::SecondaryRange, &EchoLoader);
        assert_eq!(label.as_deref(), Some("IDS_SHIP_PARAM_ATBA_MAX_DIST"));
    }

    #[test]
    fn secondary_gun_count_resolves_to_atba_key() {
        let label = stat_label(TtxStat::SecondaryGunNumGuns, &EchoLoader);
        assert_eq!(label.as_deref(), Some("IDS_SHIP_PARAM_ATBA_GUNS_COUNT"));
    }

    #[test]
    fn secondary_without_atba_key_returns_none() {
        // No distinct IDS_SHIP_PARAM_ATBA_* key exists for these.
        assert!(TtxStat::SecondaryReloadTime.label_key().is_none());
        assert!(TtxStat::SecondaryShellDamage.label_key().is_none());
        assert!(stat_label(TtxStat::SecondaryReloadTime, &EchoLoader).is_none());
    }

    #[test]
    fn stat_without_key_returns_none() {
        assert!(TtxStat::ArmorMin.label_key().is_none());
        assert!(stat_label(TtxStat::ArmorMin, &EchoLoader).is_none());
    }

    #[test]
    fn all_keys_use_ship_param_namespace() {
        for &stat in TtxStat::ALL {
            if let Some(key) = stat.label_key() {
                assert!(key.starts_with("IDS_SHIP_PARAM_"), "{stat:?} -> {key}");
            }
        }
    }

    #[test]
    fn display_label_falls_back_to_humanized_field_key() {
        assert_eq!(TtxStat::ArtilleryDispersionVertical.fallback_label(), "Artillery Dispersion Vertical");
        assert_eq!(TtxStat::SecondaryDispersionVertical.fallback_label(), "Secondary Dispersion Vertical");
    }

    #[test]
    fn field_keys_are_unique() {
        let mut keys: Vec<&str> = TtxStat::ALL.iter().map(|s| s.field_key()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate field_key");
    }

    /// Coverage lock: the exact set of stats that currently have no IDS key.
    /// Adding a confident key (or losing one) should fail this so the gap stays
    /// visible.
    #[test]
    fn unmatched_stats_are_exactly_the_known_gap() {
        let mut unmatched: Vec<TtxStat> = TtxStat::ALL.iter().copied().filter(|s| s.label_key().is_none()).collect();
        unmatched.sort_by_key(|s| s.field_key());

        let mut expected = vec![
            TtxStat::ArmorMin,
            TtxStat::ArmorMax,
            TtxStat::GunNumBarrels,
            TtxStat::ArtilleryDispersionVertical,
            TtxStat::ShellDisabledUnderwater,
            TtxStat::TorpedoIsDamageIncreasing,
            TtxStat::AirDetectionOnFire,
            TtxStat::DetectionInSmoke,
            // Secondaries: only ATBA range and gun-count have distinct keys.
            TtxStat::SecondaryReloadTime,
            TtxStat::SecondaryDispersion,
            TtxStat::SecondaryDispersionVertical,
            TtxStat::SecondaryAmmoSwitchTime,
            TtxStat::SecondaryGunCaliber,
            TtxStat::SecondaryGunNumBarrels,
            TtxStat::SecondaryGunRotationSpeed,
            TtxStat::SecondaryGunRotationTime,
            TtxStat::SecondaryShellDamage,
            TtxStat::SecondaryShellCaliber,
            TtxStat::SecondaryShellSpeed,
            TtxStat::SecondaryShellPenetration,
            TtxStat::SecondaryShellBurnChance,
            TtxStat::SecondaryShellFloodChance,
            TtxStat::SecondaryShellMaxAmmo,
        ];
        expected.sort_by_key(|s| s.field_key());

        assert_eq!(unmatched, expected);
    }
}
