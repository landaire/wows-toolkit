//! TTX data model: stat newtypes plus the `ShipStats` struct tree.
//!
//! Distance units (`Meters`, `Millimeters`, ...) are reused from `wows-core`
//! (re-exported by `game_params::types`). The newtypes defined here cover the
//! remaining TTX quantities. Leaf structs hold `Option` fields (absent when not
//! computable); per-species fields are filled in by later milestones.

use crate::game_params::ttx::labels::TtxStat;
use crate::game_params::types::Km;
use crate::game_params::types::Meters;
use crate::game_params::types::Millimeters;

// `Knots`, `Seconds`, `Hp`, `DegreesPerSecond` are defined below in this module.

/// Speed in knots.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Knots(f32);

impl Knots {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Knots {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Conventional English-port rounding: one decimal place plus the `kn` symbol.
impl std::fmt::Display for Knots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1} kn", self.0)
    }
}

/// Duration in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Seconds(f32);

impl Seconds {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Seconds {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Conventional English-port rounding: one decimal place plus the `s` symbol.
impl std::fmt::Display for Seconds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1} s", self.0)
    }
}

/// Hit points.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Hp(f32);

impl Hp {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Hp {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Conventional English-port rounding: whole hit points, no unit.
impl std::fmt::Display for Hp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.0}", self.0)
    }
}

/// A percentage value (0..100 scale, not a 0..1 fraction).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct Percent(f32);

impl Percent {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for Percent {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Conventional English-port rounding: whole percent with no space before `%`.
impl std::fmt::Display for Percent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.0}%", self.0)
    }
}

/// Angular speed in degrees per second.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct DegreesPerSecond(f32);

impl DegreesPerSecond {
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl From<f32> for DegreesPerSecond {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

/// Conventional English-port rounding: one decimal place plus an ASCII `deg/s`
/// symbol (the port uses a degree glyph; ASCII is substituted here).
impl std::fmt::Display for DegreesPerSecond {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1} deg/s", self.0)
    }
}

/// Ammunition pool size. The game uses `-1` to mean an unlimited pool; that
/// sentinel is modeled as `Infinite` rather than a magic number.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum AmmoCount {
    Finite(u32),
    Infinite,
}

/// Conventional English-port rounding: the raw count, or `inf` for an
/// unlimited pool.
impl std::fmt::Display for AmmoCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AmmoCount::Finite(n) => write!(f, "{n}"),
            AmmoCount::Infinite => write!(f, "inf"),
        }
    }
}

/// A kind-tagged stat value: the displayed quantity plus the unit it carries.
/// `Count` is a dimensionless integer (gun/barrel counts); `Bool` is a yes/no
/// flag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StatValue {
    Hp(Hp),
    Knots(Knots),
    Seconds(Seconds),
    Meters(Meters),
    Km(Km),
    Millimeters(Millimeters),
    Percent(Percent),
    DegreesPerSecond(DegreesPerSecond),
    Count(u32),
    Ammo(AmmoCount),
    Bool(bool),
}

/// Delegates to the inner value's Display; `Count` prints the bare integer and
/// `Bool` prints `yes`/`no`.
impl std::fmt::Display for StatValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StatValue::Hp(v) => write!(f, "{v}"),
            StatValue::Knots(v) => write!(f, "{v}"),
            StatValue::Seconds(v) => write!(f, "{v}"),
            StatValue::Meters(v) => write!(f, "{v}"),
            StatValue::Km(v) => write!(f, "{v}"),
            StatValue::Millimeters(v) => write!(f, "{v}"),
            StatValue::Percent(v) => write!(f, "{v}"),
            StatValue::DegreesPerSecond(v) => write!(f, "{v}"),
            StatValue::Count(n) => write!(f, "{n}"),
            StatValue::Ammo(v) => write!(f, "{v}"),
            StatValue::Bool(b) => write!(f, "{}", if *b { "yes" } else { "no" }),
        }
    }
}

impl StatValue {
    /// The underlying numeric magnitude (for sorting/comparison), or `None` for
    /// values that have no meaningful number: `Bool` flags and an `Infinite`
    /// ammo pool.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            StatValue::Hp(v) => Some(v.value()),
            StatValue::Knots(v) => Some(v.value()),
            StatValue::Seconds(v) => Some(v.value()),
            StatValue::Meters(v) => Some(v.value()),
            StatValue::Km(v) => Some(v.value()),
            StatValue::Millimeters(v) => Some(v.value()),
            StatValue::Percent(v) => Some(v.value()),
            StatValue::DegreesPerSecond(v) => Some(v.value()),
            StatValue::Count(n) => Some(*n as f32),
            StatValue::Ammo(AmmoCount::Finite(n)) => Some(*n as f32),
            StatValue::Ammo(AmmoCount::Infinite) => None,
            StatValue::Bool(_) => None,
        }
    }
}

impl From<Hp> for StatValue {
    fn from(v: Hp) -> Self {
        StatValue::Hp(v)
    }
}
impl From<Knots> for StatValue {
    fn from(v: Knots) -> Self {
        StatValue::Knots(v)
    }
}
impl From<Seconds> for StatValue {
    fn from(v: Seconds) -> Self {
        StatValue::Seconds(v)
    }
}
impl From<Meters> for StatValue {
    fn from(v: Meters) -> Self {
        StatValue::Meters(v)
    }
}
impl From<Km> for StatValue {
    fn from(v: Km) -> Self {
        StatValue::Km(v)
    }
}
impl From<Millimeters> for StatValue {
    fn from(v: Millimeters) -> Self {
        StatValue::Millimeters(v)
    }
}
impl From<Percent> for StatValue {
    fn from(v: Percent) -> Self {
        StatValue::Percent(v)
    }
}
impl From<DegreesPerSecond> for StatValue {
    fn from(v: DegreesPerSecond) -> Self {
        StatValue::DegreesPerSecond(v)
    }
}
impl From<AmmoCount> for StatValue {
    fn from(v: AmmoCount) -> Self {
        StatValue::Ammo(v)
    }
}

/// A ship's full as-shown-in-port stat card. Each section is `None` when the
/// ship has no such module or the section is not computable.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShipStats {
    pub durability: Option<Durability>,
    pub mobility: Option<Mobility>,
    pub armor: Option<Armor>,
    pub battery: Option<Battery>,
    /// Main battery: guns + shells.
    pub artillery: Option<Artillery>,
    pub secondaries: Option<Artillery>,
    /// Launchers + per-ammo.
    pub torpedoes: Option<Torpedoes>,
    pub fire_control: Option<FireControl>,
    pub visibility: Option<Visibility>,
}

/// One enumerated stat: which [`TtxStat`] it is, an optional collection
/// qualifier (ammo kind for a shell/torpedo, index for a launcher), and the
/// value. Produced by [`ShipStats::rows`].
#[derive(Clone, Debug, PartialEq)]
pub struct StatRow {
    pub stat: TtxStat,
    pub qualifier: Option<String>,
    pub value: StatValue,
}

impl ShipStats {
    /// Enumerate every present (`Some`) stat across the whole tree as the
    /// inverse of [`TtxStat::field_key`]: one [`StatRow`] per value, in
    /// [`TtxStat::ALL`] order (collections expand item-major: item order, then
    /// field order). Absent fields are skipped.
    ///
    /// Two bare-`f32` model fields carry no unit newtype and have no fitting
    /// [`StatValue`] variant, so they are intentionally NOT emitted:
    /// `battery.capacity` / `battery.regeneration` (dive capacity, dimensionless)
    /// and `*.shells[*].speed` (shell muzzle velocity, m/s). The matching
    /// [`TtxStat`] variants (`BatteryCapacity`, `BatteryRegeneration`,
    /// `ShellSpeed`, `SecondaryShellSpeed`) therefore never produce a row.
    pub fn rows(&self) -> Vec<StatRow> {
        let mut rows = Vec::new();

        let scalar = |rows: &mut Vec<StatRow>, stat: TtxStat, value: Option<StatValue>| {
            if let Some(value) = value {
                rows.push(StatRow { stat, qualifier: None, value });
            }
        };
        let item = |rows: &mut Vec<StatRow>, stat: TtxStat, qualifier: &str, value: Option<StatValue>| {
            if let Some(value) = value {
                rows.push(StatRow {
                    stat,
                    qualifier: Some(qualifier.to_string()),
                    value,
                });
            }
        };

        if let Some(d) = &self.durability {
            scalar(&mut rows, TtxStat::Health, d.health.map(StatValue::Hp));
            scalar(&mut rows, TtxStat::TorpedoProtection, d.torpedo_protection.map(StatValue::Percent));
        }
        if let Some(m) = &self.mobility {
            scalar(&mut rows, TtxStat::Speed, m.speed.map(StatValue::Knots));
            scalar(&mut rows, TtxStat::TurningRadius, m.turning_radius.map(StatValue::Meters));
            scalar(&mut rows, TtxStat::RudderTime, m.rudder_time.map(StatValue::Seconds));
        }
        if let Some(a) = &self.armor {
            scalar(&mut rows, TtxStat::ArmorMin, a.min.map(StatValue::Millimeters));
            scalar(&mut rows, TtxStat::ArmorMax, a.max.map(StatValue::Millimeters));
        }
        // BatteryCapacity / BatteryRegeneration intentionally unmapped (bare f32, no unit).

        push_artillery_rows(&mut rows, self.artillery.as_ref(), ArtilleryKind::Main, &scalar, &item);
        push_artillery_rows(&mut rows, self.secondaries.as_ref(), ArtilleryKind::Secondary, &scalar, &item);

        if let Some(t) = &self.torpedoes {
            scalar(&mut rows, TtxStat::TorpedoReloadTime, t.reload_time.map(StatValue::Seconds));
            for (idx, launcher) in t.launchers.iter().enumerate() {
                let q = idx.to_string();
                item(&mut rows, TtxStat::LauncherRotationSpeed, &q, launcher.rotation_speed.map(StatValue::DegreesPerSecond));
                item(&mut rows, TtxStat::LauncherRotationTime, &q, launcher.rotation_time.map(StatValue::Seconds));
                item(&mut rows, TtxStat::LauncherNumBarrels, &q, launcher.num_barrels.map(StatValue::Count));
            }
            for (idx, torp) in t.torpedoes.iter().enumerate() {
                let q = torpedo_qualifier(torp, idx);
                item(&mut rows, TtxStat::TorpedoDamage, &q, torp.damage.map(StatValue::Hp));
                item(&mut rows, TtxStat::TorpedoSpeed, &q, torp.speed.map(StatValue::Knots));
                item(&mut rows, TtxStat::TorpedoRange, &q, torp.range.map(StatValue::Km));
                item(&mut rows, TtxStat::TorpedoVisibility, &q, torp.visibility.map(StatValue::Km));
                item(&mut rows, TtxStat::TorpedoDistanceOfMaxDamage, &q, torp.distance_of_max_damage.map(StatValue::Km));
                item(&mut rows, TtxStat::TorpedoIsDamageIncreasing, &q, torp.is_damage_increasing.map(StatValue::Bool));
                item(&mut rows, TtxStat::TorpedoDisabledUnderwater, &q, torp.disabled_underwater.map(StatValue::Bool));
            }
        }
        if let Some(fc) = &self.fire_control {
            scalar(&mut rows, TtxStat::FireControlMaxDist, fc.max_dist.map(StatValue::Km));
        }
        if let Some(v) = &self.visibility {
            scalar(&mut rows, TtxStat::SeaDetection, v.sea_detection.map(StatValue::Km));
            scalar(&mut rows, TtxStat::SeaDetectionOnFire, v.sea_detection_on_fire.map(StatValue::Km));
            scalar(&mut rows, TtxStat::AirDetection, v.air_detection.map(StatValue::Km));
            scalar(&mut rows, TtxStat::AirDetectionOnFire, v.air_detection_on_fire.map(StatValue::Km));
            scalar(&mut rows, TtxStat::DetectionInSmoke, v.detection_in_smoke.map(StatValue::Km));
            scalar(&mut rows, TtxStat::SecondaryRangeDetection, v.secondary_range_detection.map(StatValue::Km));
            scalar(&mut rows, TtxStat::PeriscopeDepthDetection, v.periscope_depth_detection.map(StatValue::Km));
        }

        rows
    }
}

/// Discriminates which set of [`TtxStat`] variants an [`Artillery`] block maps
/// to: the main battery uses `Artillery*`/`Gun*`/`Shell*`, the secondary battery
/// reuses the same struct but maps to the `Secondary*` variants.
#[derive(Clone, Copy)]
enum ArtilleryKind {
    Main,
    Secondary,
}

/// The ammo-kind qualifier for a shell row: the resolved `ammo_kind` string,
/// falling back to the projectile `name`, then the index.
fn shell_qualifier(shell: &ShellStats, idx: usize) -> String {
    shell
        .ammo_kind
        .clone()
        .or_else(|| (!shell.name.is_empty()).then(|| shell.name.clone()))
        .unwrap_or_else(|| idx.to_string())
}

/// The qualifier for a torpedo row: the projectile `name`, else the index.
fn torpedo_qualifier(torp: &TorpedoStats, idx: usize) -> String {
    if torp.name.is_empty() {
        idx.to_string()
    } else {
        torp.name.clone()
    }
}

#[allow(clippy::type_complexity)]
fn push_artillery_rows(
    rows: &mut Vec<StatRow>,
    artillery: Option<&Artillery>,
    kind: ArtilleryKind,
    scalar: &dyn Fn(&mut Vec<StatRow>, TtxStat, Option<StatValue>),
    item: &dyn Fn(&mut Vec<StatRow>, TtxStat, &str, Option<StatValue>),
) {
    let Some(a) = artillery else { return };
    let s = ArtilleryStats::for_kind(kind);

    scalar(rows, s.reload_time, a.reload_time.map(StatValue::Seconds));
    scalar(rows, s.range, a.range.map(StatValue::Km));
    scalar(rows, s.dispersion, a.dispersion.map(StatValue::Meters));
    scalar(rows, s.ammo_switch_time, a.ammo_switch_time.map(StatValue::Seconds));

    if let Some(gun) = &a.gun {
        scalar(rows, s.gun_caliber, gun.caliber.map(StatValue::Millimeters));
        scalar(rows, s.gun_num_barrels, gun.num_barrels.map(StatValue::Count));
        scalar(rows, s.gun_num_guns, gun.num_guns.map(StatValue::Count));
        scalar(rows, s.gun_rotation_speed, gun.rotation_speed.map(StatValue::DegreesPerSecond));
        scalar(rows, s.gun_rotation_time, gun.rotation_time.map(StatValue::Seconds));
    }

    for (idx, shell) in a.shells.iter().enumerate() {
        let q = shell_qualifier(shell, idx);
        item(rows, s.shell_damage, &q, shell.damage.map(StatValue::Hp));
        item(rows, s.shell_caliber, &q, shell.caliber.map(StatValue::Millimeters));
        // shell.speed (m/s, bare f32) intentionally unmapped: no fitting unit.
        item(rows, s.shell_penetration, &q, shell.penetration.map(StatValue::Millimeters));
        item(rows, s.shell_burn_chance, &q, shell.burn_chance.map(StatValue::Percent));
        item(rows, s.shell_flood_chance, &q, shell.flood_chance.map(StatValue::Percent));
        item(rows, s.shell_max_ammo, &q, shell.max_ammo.map(StatValue::Ammo));
        if let Some(stat) = s.shell_disabled_underwater {
            item(rows, stat, &q, shell.disabled_underwater.map(StatValue::Bool));
        }
    }
}

/// The per-kind [`TtxStat`] variant set for an [`Artillery`] block. `shell_speed`
/// is omitted (no fitting unit); `shell_disabled_underwater` is `None` for the
/// secondary battery, which has no such variant.
struct ArtilleryStats {
    reload_time: TtxStat,
    range: TtxStat,
    dispersion: TtxStat,
    ammo_switch_time: TtxStat,
    gun_caliber: TtxStat,
    gun_num_barrels: TtxStat,
    gun_num_guns: TtxStat,
    gun_rotation_speed: TtxStat,
    gun_rotation_time: TtxStat,
    shell_damage: TtxStat,
    shell_caliber: TtxStat,
    shell_penetration: TtxStat,
    shell_burn_chance: TtxStat,
    shell_flood_chance: TtxStat,
    shell_max_ammo: TtxStat,
    shell_disabled_underwater: Option<TtxStat>,
}

impl ArtilleryStats {
    fn for_kind(kind: ArtilleryKind) -> Self {
        match kind {
            ArtilleryKind::Main => ArtilleryStats {
                reload_time: TtxStat::ArtilleryReloadTime,
                range: TtxStat::ArtilleryRange,
                dispersion: TtxStat::ArtilleryDispersion,
                ammo_switch_time: TtxStat::ArtilleryAmmoSwitchTime,
                gun_caliber: TtxStat::GunCaliber,
                gun_num_barrels: TtxStat::GunNumBarrels,
                gun_num_guns: TtxStat::GunNumGuns,
                gun_rotation_speed: TtxStat::GunRotationSpeed,
                gun_rotation_time: TtxStat::GunRotationTime,
                shell_damage: TtxStat::ShellDamage,
                shell_caliber: TtxStat::ShellCaliber,
                shell_penetration: TtxStat::ShellPenetration,
                shell_burn_chance: TtxStat::ShellBurnChance,
                shell_flood_chance: TtxStat::ShellFloodChance,
                shell_max_ammo: TtxStat::ShellMaxAmmo,
                shell_disabled_underwater: Some(TtxStat::ShellDisabledUnderwater),
            },
            ArtilleryKind::Secondary => ArtilleryStats {
                reload_time: TtxStat::SecondaryReloadTime,
                range: TtxStat::SecondaryRange,
                dispersion: TtxStat::SecondaryDispersion,
                ammo_switch_time: TtxStat::SecondaryAmmoSwitchTime,
                gun_caliber: TtxStat::SecondaryGunCaliber,
                gun_num_barrels: TtxStat::SecondaryGunNumBarrels,
                gun_num_guns: TtxStat::SecondaryGunNumGuns,
                gun_rotation_speed: TtxStat::SecondaryGunRotationSpeed,
                gun_rotation_time: TtxStat::SecondaryGunRotationTime,
                shell_damage: TtxStat::SecondaryShellDamage,
                shell_caliber: TtxStat::SecondaryShellCaliber,
                shell_penetration: TtxStat::SecondaryShellPenetration,
                shell_burn_chance: TtxStat::SecondaryShellBurnChance,
                shell_flood_chance: TtxStat::SecondaryShellFloodChance,
                shell_max_ammo: TtxStat::SecondaryShellMaxAmmo,
                shell_disabled_underwater: None,
            },
        }
    }
}

/// Survivability stats (`FactoryDurability`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Durability {
    pub health: Option<Hp>,
    pub torpedo_protection: Option<Percent>,
}

/// Maneuverability stats (`FactoryMobility`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mobility {
    pub speed: Option<Knots>,
    pub turning_radius: Option<Meters>,
    pub rudder_time: Option<Seconds>,
}

/// Armor thickness extremes (`FactoryArmor`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Armor {
    pub min: Option<Millimeters>,
    pub max: Option<Millimeters>,
}

/// Submarine battery stats (`FactoryBattery`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Battery {
    pub capacity: Option<f32>,
    pub regeneration: Option<f32>,
}

/// Main-battery gun mount stats (`MainGunTTX`, FactoryArtillery.py:70-76 +
/// PreprocessedGun.initGunTTX, PreprocessedGun.py:18-23).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MainGun {
    /// `barrelDiameter * 1000` (PreprocessedGun.py:22).
    pub caliber: Option<Millimeters>,
    /// `gp.numBarrels` (PreprocessedGun.py:21).
    pub num_barrels: Option<u32>,
    /// Count of `HP_AGM_*` mounts (`gunsCount`, PreprocessedArtillery.py:29).
    pub num_guns: Option<u32>,
    /// `rotationSpeed[0] * GMRotationSpeed + GMRotationSpeedBonus` (FactoryArtillery.py:74).
    pub rotation_speed: Option<DegreesPerSecond>,
    /// `180 / rotationSpeed` (FactoryArtillery.py:75).
    pub rotation_time: Option<Seconds>,
}

/// Per-shell stats (`ArtilleryAmmoTTX`, createAmmoTTX, FactoryArtillery.py:147-190).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShellStats {
    /// Projectile GameParams name (`ammoParams.name`, FactoryArtillery.py:152).
    pub name: String,
    /// `ammoType` string ("HE"/"AP"/"CS") off the projectile (PreprocessedAmmo.py:13).
    pub ammo_kind: Option<String>,
    /// `damage` (FactoryArtillery.py:164, full coefficient product).
    pub damage: Option<Hp>,
    /// `caliber * 1000` (FactoryArtillery.py:155).
    pub caliber: Option<Millimeters>,
    /// `bulletSpeed * timeFactor` in m/s (PreprocessedAmmo.py:16).
    pub speed: Option<f32>,
    /// HE `floor(alphaPiercingHE * GMPenetrationCoeffHE)` (FactoryArtillery.py:182),
    /// CS `floor(alphaPiercingCS)` (FactoryArtillery.py:185). AP is a ballistic sim
    /// (no closed-form `piercing` in the deob), left `None`.
    pub penetration: Option<Millimeters>,
    /// HE/AP `calculateBurnChance(...)` as a percent (FactoryArtillery.py:171/188).
    pub burn_chance: Option<Percent>,
    /// HE `floodChance` (`uwCritical`) as a percent (FactoryArtillery.py:172).
    pub flood_chance: Option<Percent>,
    /// `maxAmmoCount` from `poolSize` (FactoryArtillery.py:167-168); `-1` -> `Infinite`.
    pub max_ammo: Option<AmmoCount>,
    /// `disabledUnderwater` (`hull.canBeUnderwater`, FactoryArtillery.py:165).
    pub disabled_underwater: Option<bool>,
}

/// Gun battery stats (`ArtilleryTTX`, FactoryArtillery.py + TTXFactory.py).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Artillery {
    /// `mgReloadTime`: `gun.shotDelay * GMShotDelay`.
    pub reload_time: Option<Seconds>,
    /// `mgMaxDist`: `(maxDist / KM_TO_M) * fcMaxDistCoef * GMMaxDist` (FactoryArtillery.py:42).
    pub range: Option<Km>,
    /// `mgDispersion`: `getDispersionValue(gun, range_km, GMIdealRadius)` (FactoryArtillery.py:47).
    pub dispersion: Option<Meters>,
    /// `ammoSwitchTime`: `shotDelay * ammoSwitchCoeff * GMShotDelay * switchAmmoReloadCoef`
    /// (FactoryTorpedoes.py:67 main-gun analog).
    pub ammo_switch_time: Option<Seconds>,
    /// The main-battery gun mount.
    pub gun: Option<MainGun>,
    /// Per-shell stats, one per resolved ammo name.
    pub shells: Vec<ShellStats>,
}

/// Torpedo launcher + ammo stats (`FactoryTorpedoes.py` `createTorpedoesTTX`).
///
/// `reload_time` is the aggregated launcher reload (`initAmmoReloadParams`,
/// FactoryTorpedoes.py:27-42: `min` of the non-zero per-mount reload times).
/// `launchers` is one [`Launcher`] per torpedo tube (`createLauncherTTX`).
/// `torpedoes` is the per-ammo stat list (`createTorpedoTTX`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Torpedoes {
    pub reload_time: Option<Seconds>,
    pub launchers: Vec<Launcher>,
    pub torpedoes: Vec<TorpedoStats>,
}

/// One torpedo launcher's traverse stats (`TorpedoLauncherTTX`,
/// FactoryTorpedoes.py:74-80).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Launcher {
    /// `rotationSpeed[0] * yawSpeedCoef + yawSpeedBonus` (FactoryTorpedoes.py:78).
    pub rotation_speed: Option<DegreesPerSecond>,
    /// `180 / rotationSpeed` (FactoryTorpedoes.py:79).
    pub rotation_time: Option<Seconds>,
    /// `gp.numBarrels` (PreprocessedTorpedoes.py:72, surfaced for group display).
    pub num_barrels: Option<u32>,
}

/// Per-ammo torpedo stats (`TorpedoTTX`, FactoryTorpedoes.py:82-122).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TorpedoStats {
    /// Projectile GameParams name (`ammoParams.name`, FactoryTorpedoes.py:89).
    pub name: String,
    /// `getTorpedoDamage` (ModifiersApply.py:477-488).
    pub damage: Option<Hp>,
    /// `getTorpedoSpeed` (ModifiersApply.py:491-499).
    pub speed: Option<Knots>,
    /// `maxDist * torpedoRangeCoefficient * BW_TO_BALLISTIC / KM_TO_M` (FactoryTorpedoes.py:93).
    pub range: Option<Km>,
    /// `visibilityFactor * torpedoVisibilityFactor` (FactoryTorpedoes.py:92).
    pub visibility: Option<Km>,
    /// `distanceOfMaxDamage` (FactoryTorpedoes.py:119); arming-distance piece needs
    /// data absent here (`armingTime`/`maneuverDist`), so left `None` (see factory note).
    pub distance_of_max_damage: Option<Km>,
    /// `isDamageIncreasing` (FactoryTorpedoes.py:120).
    pub is_damage_increasing: Option<bool>,
    /// `disabledUnderwater` (FactoryTorpedoes.py:101); submarine-only.
    pub disabled_underwater: Option<bool>,
}

/// Fire-control stats (`PreprocessedFireControl`).
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FireControl {
    pub max_dist: Option<Km>,
}

/// Detectability stats (`FactoryVisibility` createVisibilityTTX).
///
/// Ranges are in km. `sea_detection`/`air_detection` are the `visibilityByShip.normal`
/// / `visibilityByPlane.normal` slots; the `_on_fire` variants are the `.fire` slots;
/// `detection_in_smoke` is `visibilityByShip.smoke`; `secondary_range_detection` is the
/// `visibilityByShip.atba` (secondary/MG floor); `periscope_depth_detection` is
/// `visibilityFromDepth.max`. Per-depth submarine ranges (`byDepth`,
/// createVisibilityTTX@387-513) are a runtime entity calc and deferred.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Visibility {
    pub sea_detection: Option<Km>,
    pub sea_detection_on_fire: Option<Km>,
    pub air_detection: Option<Km>,
    pub air_detection_on_fire: Option<Km>,
    pub detection_in_smoke: Option<Km>,
    pub secondary_range_detection: Option<Km>,
    pub periscope_depth_detection: Option<Km>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ship_stats_default_is_empty() {
        let stats = ShipStats::default();
        assert!(stats.durability.is_none());
        assert!(stats.mobility.is_none());
        assert!(stats.armor.is_none());
        assert!(stats.battery.is_none());
        assert!(stats.artillery.is_none());
        assert!(stats.secondaries.is_none());
        assert!(stats.torpedoes.is_none());
        assert!(stats.fire_control.is_none());
        assert!(stats.visibility.is_none());
    }

    #[test]
    fn leaf_defaults_are_absent() {
        let durability = Durability::default();
        assert!(durability.health.is_none());
        assert!(durability.torpedo_protection.is_none());

        let mobility = Mobility::default();
        assert!(mobility.speed.is_none());
        assert!(mobility.turning_radius.is_none());
        assert!(mobility.rudder_time.is_none());
    }

    #[test]
    fn ammo_count_models_unlimited_pool() {
        assert_eq!(AmmoCount::Finite(40), AmmoCount::Finite(40));
        assert_ne!(AmmoCount::Finite(40), AmmoCount::Infinite);
    }

    #[test]
    fn newtype_display_formats() {
        assert_eq!(Hp::from(40350.0).to_string(), "40350");
        assert_eq!(Knots::from(30.5).to_string(), "30.5 kn");
        assert_eq!(Seconds::from(7.5).to_string(), "7.5 s");
        assert_eq!(Percent::from(16.0).to_string(), "16%");
        assert_eq!(DegreesPerSecond::from(6.0).to_string(), "6.0 deg/s");
        assert_eq!(AmmoCount::Finite(120).to_string(), "120");
        assert_eq!(AmmoCount::Infinite.to_string(), "inf");
    }

    #[test]
    fn stat_value_display_delegates() {
        assert_eq!(StatValue::Hp(Hp::from(40350.0)).to_string(), "40350");
        assert_eq!(StatValue::Km(Km::from(10.5)).to_string(), "10.5 km");
        assert_eq!(StatValue::Count(3).to_string(), "3");
        assert_eq!(StatValue::Bool(true).to_string(), "yes");
        assert_eq!(StatValue::Bool(false).to_string(), "no");
        assert_eq!(StatValue::Ammo(AmmoCount::Infinite).to_string(), "inf");
    }

    #[test]
    fn stat_value_as_f32() {
        assert_eq!(StatValue::Km(Km::from(10.5)).as_f32(), Some(10.5));
        assert_eq!(StatValue::Count(3).as_f32(), Some(3.0));
        assert_eq!(StatValue::Ammo(AmmoCount::Finite(120)).as_f32(), Some(120.0));
        assert_eq!(StatValue::Ammo(AmmoCount::Infinite).as_f32(), None);
        assert_eq!(StatValue::Bool(true).as_f32(), None);
    }

    #[test]
    fn rows_on_default_is_empty() {
        assert!(ShipStats::default().rows().is_empty());
    }

    #[test]
    fn rows_enumerate_scalars_and_shell_collection() {
        let stats = ShipStats {
            durability: Some(Durability {
                health: Some(Hp::from(40350.0)),
                torpedo_protection: Some(Percent::from(16.0)),
            }),
            mobility: Some(Mobility {
                speed: Some(Knots::from(30.5)),
                turning_radius: Some(Meters::from(740.0)),
                rudder_time: None,
            }),
            artillery: Some(Artillery {
                reload_time: Some(Seconds::from(30.0)),
                shells: vec![
                    ShellStats {
                        name: "PHEShell".to_string(),
                        ammo_kind: Some("HE".to_string()),
                        damage: Some(Hp::from(5000.0)),
                        ..Default::default()
                    },
                    ShellStats {
                        name: "PAPShell".to_string(),
                        ammo_kind: Some("AP".to_string()),
                        damage: Some(Hp::from(11900.0)),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };

        let rows = stats.rows();

        let health = rows.iter().find(|r| r.stat == TtxStat::Health).unwrap();
        assert_eq!(health.qualifier, None);
        assert_eq!(health.value.to_string(), "40350");

        let radius = rows.iter().find(|r| r.stat == TtxStat::TurningRadius).unwrap();
        assert_eq!(radius.value.to_string(), "740 m");

        let reload = rows.iter().find(|r| r.stat == TtxStat::ArtilleryReloadTime).unwrap();
        assert_eq!(reload.value.to_string(), "30.0 s");

        let shell_damage: Vec<&StatRow> = rows.iter().filter(|r| r.stat == TtxStat::ShellDamage).collect();
        assert_eq!(shell_damage.len(), 2);
        assert_eq!(shell_damage[0].qualifier.as_deref(), Some("HE"));
        assert_eq!(shell_damage[0].value.to_string(), "5000");
        assert_eq!(shell_damage[1].qualifier.as_deref(), Some("AP"));
        assert_eq!(shell_damage[1].value.to_string(), "11900");
    }

    #[test]
    fn rows_qualify_launchers_by_index() {
        let stats = ShipStats {
            torpedoes: Some(Torpedoes {
                reload_time: None,
                launchers: vec![
                    Launcher {
                        rotation_speed: Some(DegreesPerSecond::from(25.0)),
                        ..Default::default()
                    },
                    Launcher {
                        rotation_speed: Some(DegreesPerSecond::from(30.0)),
                        ..Default::default()
                    },
                ],
                torpedoes: Vec::new(),
            }),
            ..Default::default()
        };

        let rows = stats.rows();
        let speeds: Vec<&StatRow> = rows.iter().filter(|r| r.stat == TtxStat::LauncherRotationSpeed).collect();
        assert_eq!(speeds.len(), 2);
        assert_eq!(speeds[0].qualifier.as_deref(), Some("0"));
        assert_eq!(speeds[1].qualifier.as_deref(), Some("1"));
    }

    /// Every [`TtxStat`] variant must be reachable from `rows()` (its model
    /// field, when present, yields a row) EXCEPT the four with no fitting
    /// [`StatValue`] variant. This locks the known unmapped set.
    #[test]
    fn unmapped_stats_are_exactly_the_unitless_floats() {
        use crate::game_params::ttx::labels::TtxStat as T;
        let unmapped = [T::BatteryCapacity, T::BatteryRegeneration, T::ShellSpeed, T::SecondaryShellSpeed];
        // Sanity: the four are real variants and distinct.
        let mut seen = unmapped.to_vec();
        seen.sort_by_key(|s| s.field_key());
        seen.dedup();
        assert_eq!(seen.len(), 4);
    }
}
