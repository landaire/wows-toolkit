//! Unit and better direction of each port stat card field.

use serde::Serialize;
use wowsunpack::game_params::ttx::labels::TtxStat;

/// The unit a stat value is reported and compared in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Hp,
    HpPerMinute,
    Knots,
    Seconds,
    Meters,
    MetersPerSecond,
    Km,
    Millimeters,

    /// 0 to 100.
    Percent,
    DegreesPerSecond,
    Count,

    /// A dimensionless real, such as the submarine dive capacity pool.
    Ratio,

    /// Yes or no.
    Flag,
}

impl Unit {
    /// Suffix printed after a value; empty for plain numbers.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Hp => " hp",
            Self::HpPerMinute => " hp/min",
            Self::Knots => " kn",
            Self::Seconds => " s",
            Self::Meters => " m",
            Self::MetersPerSecond => " m/s",
            Self::Km => " km",
            Self::Millimeters => " mm",
            Self::Percent => "%",
            Self::DegreesPerSecond => " deg/s",
            Self::Count | Self::Ratio | Self::Flag => "",
        }
    }

    /// Name of the unit in a stat list.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hp => "hp",
            Self::HpPerMinute => "hp/min",
            Self::Knots => "kn",
            Self::Seconds => "s",
            Self::Meters => "m",
            Self::MetersPerSecond => "m/s",
            Self::Km => "km",
            Self::Millimeters => "mm",
            Self::Percent => "percent 0-100",
            Self::DegreesPerSecond => "deg/s",
            Self::Count => "count",
            Self::Ratio => "ratio",
            Self::Flag => "yes/no",
        }
    }
}

/// Which way a stat improves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Higher,
    Lower,
}

/// The unit `ShipStats::rows` reports `stat` in.
pub fn unit(stat: TtxStat) -> Unit {
    match stat {
        TtxStat::Health | TtxStat::ShellDamage | TtxStat::SecondaryShellDamage | TtxStat::TorpedoDamage => Unit::Hp,
        TtxStat::Speed | TtxStat::TorpedoSpeed => Unit::Knots,
        TtxStat::RudderTime
        | TtxStat::ArtilleryReloadTime
        | TtxStat::ArtilleryAmmoSwitchTime
        | TtxStat::GunRotationTime
        | TtxStat::SecondaryReloadTime
        | TtxStat::SecondaryGunRotationTime
        | TtxStat::TorpedoReloadTime
        | TtxStat::LauncherRotationTime
        | TtxStat::ConsumableReloadTime
        | TtxStat::ConsumableWorkTime
        | TtxStat::ConsumablePreparationTime
        | TtxStat::ConsumableSmokeLifetime
        | TtxStat::ConsumableCallFightersTimeDelay
        | TtxStat::ConsumableCallFightersTimeFromHeaven => Unit::Seconds,
        TtxStat::TurningRadius
        | TtxStat::ArtilleryDispersion
        | TtxStat::ArtilleryDispersionVertical
        | TtxStat::SecondaryDispersion
        | TtxStat::SecondaryDispersionVertical
        | TtxStat::ConsumableDetectionRadius => Unit::Meters,
        TtxStat::ShellSpeed | TtxStat::SecondaryShellSpeed => Unit::MetersPerSecond,
        TtxStat::ArtilleryRange
        | TtxStat::SecondaryRange
        | TtxStat::TorpedoRange
        | TtxStat::TorpedoVisibility
        | TtxStat::TorpedoDistanceOfMaxDamage
        | TtxStat::FireControlMaxDist
        | TtxStat::ConsumableSmokeRadius
        | TtxStat::ConsumableCallFightersRadius
        | TtxStat::SeaDetection
        | TtxStat::SeaDetectionOnFire
        | TtxStat::AirDetection
        | TtxStat::AirDetectionOnFire
        | TtxStat::DetectionInSmoke
        | TtxStat::MainGunRangeDetection
        | TtxStat::SecondaryRangeDetection
        | TtxStat::PeriscopeDepthDetection => Unit::Km,
        TtxStat::ArmorMin
        | TtxStat::ArmorMax
        | TtxStat::GunCaliber
        | TtxStat::ShellCaliber
        | TtxStat::ShellPenetration
        | TtxStat::SecondaryGunCaliber
        | TtxStat::SecondaryShellCaliber
        | TtxStat::SecondaryShellPenetration => Unit::Millimeters,
        TtxStat::TorpedoProtection
        | TtxStat::ShellBurnChance
        | TtxStat::ShellFloodChance
        | TtxStat::SecondaryShellBurnChance
        | TtxStat::SecondaryShellFloodChance => Unit::Percent,
        TtxStat::GunRotationSpeed | TtxStat::SecondaryGunRotationSpeed | TtxStat::LauncherRotationSpeed => {
            Unit::DegreesPerSecond
        }
        TtxStat::GunNumBarrels
        | TtxStat::GunNumGuns
        | TtxStat::SecondaryGunNumBarrels
        | TtxStat::SecondaryGunNumGuns
        | TtxStat::LauncherNumBarrels
        | TtxStat::ShellMaxAmmo
        | TtxStat::SecondaryShellMaxAmmo
        | TtxStat::ConsumableCharges => Unit::Count,
        TtxStat::BatteryCapacity
        | TtxStat::BatteryRegeneration
        | TtxStat::ConsumableMaxCapacity
        | TtxStat::ConsumableRegenerationHpSpeed
        | TtxStat::ConsumableFightersCount
        | TtxStat::ConsumablePlaneRegenerationRate => Unit::Ratio,
        TtxStat::ShellDisabledUnderwater | TtxStat::TorpedoIsDamageIncreasing | TtxStat::TorpedoDisabledUnderwater => {
            Unit::Flag
        }
    }
}

/// Which way `stat` improves; `None` for flags, calibers and distances
/// that are neither better nor worse.
pub fn direction(stat: TtxStat) -> Option<Direction> {
    match stat {
        TtxStat::ArtilleryReloadTime
        | TtxStat::SecondaryReloadTime
        | TtxStat::TorpedoReloadTime
        | TtxStat::ArtilleryDispersion
        | TtxStat::ArtilleryDispersionVertical
        | TtxStat::SecondaryDispersion
        | TtxStat::SecondaryDispersionVertical
        | TtxStat::ArtilleryAmmoSwitchTime
        | TtxStat::RudderTime
        | TtxStat::GunRotationTime
        | TtxStat::SecondaryGunRotationTime
        | TtxStat::LauncherRotationTime
        | TtxStat::TurningRadius
        | TtxStat::SeaDetection
        | TtxStat::SeaDetectionOnFire
        | TtxStat::AirDetection
        | TtxStat::AirDetectionOnFire
        | TtxStat::DetectionInSmoke
        | TtxStat::MainGunRangeDetection
        | TtxStat::SecondaryRangeDetection
        | TtxStat::PeriscopeDepthDetection
        | TtxStat::TorpedoVisibility
        | TtxStat::ConsumableReloadTime
        | TtxStat::ConsumablePreparationTime
        | TtxStat::ConsumableCallFightersTimeDelay
        | TtxStat::ConsumableCallFightersTimeFromHeaven => Some(Direction::Lower),
        TtxStat::ShellDisabledUnderwater
        | TtxStat::TorpedoIsDamageIncreasing
        | TtxStat::TorpedoDisabledUnderwater
        | TtxStat::GunCaliber
        | TtxStat::ShellCaliber
        | TtxStat::SecondaryGunCaliber
        | TtxStat::SecondaryShellCaliber
        | TtxStat::TorpedoDistanceOfMaxDamage => None,
        TtxStat::Health
        | TtxStat::TorpedoProtection
        | TtxStat::Speed
        | TtxStat::ArmorMin
        | TtxStat::ArmorMax
        | TtxStat::BatteryCapacity
        | TtxStat::BatteryRegeneration
        | TtxStat::ArtilleryRange
        | TtxStat::GunNumBarrels
        | TtxStat::GunNumGuns
        | TtxStat::GunRotationSpeed
        | TtxStat::ShellDamage
        | TtxStat::ShellSpeed
        | TtxStat::ShellPenetration
        | TtxStat::ShellBurnChance
        | TtxStat::ShellFloodChance
        | TtxStat::ShellMaxAmmo
        | TtxStat::SecondaryRange
        | TtxStat::SecondaryGunNumBarrels
        | TtxStat::SecondaryGunNumGuns
        | TtxStat::SecondaryGunRotationSpeed
        | TtxStat::SecondaryShellDamage
        | TtxStat::SecondaryShellSpeed
        | TtxStat::SecondaryShellPenetration
        | TtxStat::SecondaryShellBurnChance
        | TtxStat::SecondaryShellFloodChance
        | TtxStat::SecondaryShellMaxAmmo
        | TtxStat::LauncherRotationSpeed
        | TtxStat::LauncherNumBarrels
        | TtxStat::TorpedoDamage
        | TtxStat::TorpedoSpeed
        | TtxStat::TorpedoRange
        | TtxStat::FireControlMaxDist
        | TtxStat::ConsumableWorkTime
        | TtxStat::ConsumableCharges
        | TtxStat::ConsumableMaxCapacity
        | TtxStat::ConsumableDetectionRadius
        | TtxStat::ConsumableRegenerationHpSpeed
        | TtxStat::ConsumableSmokeRadius
        | TtxStat::ConsumableSmokeLifetime
        | TtxStat::ConsumableFightersCount
        | TtxStat::ConsumableCallFightersRadius
        | TtxStat::ConsumablePlaneRegenerationRate => Some(Direction::Higher),
    }
}
