//! Stats computed from the port stat card that the card itself does not list.

use std::fmt;

use serde::Serialize;
use wows_core::units;
use wows_core::units::DamagePerMinute;
use wows_core::units::FiresPerMinute;
use wows_core::units::Percent;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::constants::DispersionEllipse;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::model::AmmoCount;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::model::Seconds;
use wowsunpack::game_params::ttx::model::ShellStats;
use wowsunpack::game_params::ttx::model::ShipStats;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::orchestration::ArtilleryDispersion;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Meters;
use wowsunpack::game_params::types::Param;

use crate::bearing::MountCoverage;
use crate::fires::FireArmament;
use crate::fires::FireInputs;
use crate::fires::secondary_fires_by_bearing;
use crate::fires::total_fires_all_mounts;
use crate::fires::total_fires_by_bearing;
use crate::mounts::aiming_mounts;
use crate::resistance::FireResistance;
use crate::resistance::Flooding;
use crate::stat::Direction;
use crate::stat::Unit;

/// A stat derived from the port stat card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedStat {
    /// Barrels of every main battery mount together.
    MainBatteryBarrels,

    /// Damage of one main battery salvo, per ammo type: shell damage times
    /// barrels.
    MainBatterySalvoDamage,

    /// Main battery damage per minute at full reload, per ammo type: salvo
    /// damage times salvos per minute.
    MainBatteryDamagePerMinute,

    /// Main battery dispersion with the build applied, measured at the stock
    /// ship's firing range. The horizontal value ignores range changes; the
    /// vertical one still follows the build's range, which scales its
    /// distance ratio, and both stop at a range shorter than stock.
    MainBatteryDispersionAtStockRange,
    MainBatteryVerticalDispersionAtStockRange,

    /// The same per secondary mount, at the stock secondary range.
    SecondaryDispersionAtStockRange,
    SecondaryVerticalDispersionAtStockRange,

    /// Torpedo tubes of every launcher together.
    TorpedoTubes,

    /// Time to use every charge of a consumable back to back, per
    /// consumable: charges times action time plus reload.
    ConsumableAllChargesTime,

    /// Theoretical fire starts per minute against one target at the bearing
    /// where the most start: the main battery's best ammo type and every
    /// secondary group from the mounts that bear there, the best squadron of
    /// the stock plane modules and the best air strike aircraft.
    FiresPerMinuteTotal,

    /// The same with every gun mount firing at once: a ceiling no single
    /// target sees.
    FiresPerMinuteTotalAllMounts,

    /// Theoretical fire starts per minute against one target at its best
    /// bearing, per main battery ammo type.
    FiresPerMinuteMainBattery,

    /// Theoretical fire starts per minute against one target at its best
    /// bearing, every secondary group together.
    FiresPerMinuteSecondary,

    /// Theoretical fire starts per minute against one target at the group's
    /// own best bearing, per secondary group.
    FiresPerMinuteSecondaryGroup,

    /// Every secondary mount firing at once: a ceiling no single target sees.
    FiresPerMinuteSecondaryAllMounts,

    /// Theoretical fire starts per minute, per squadron or air strike
    /// aircraft.
    FiresPerMinuteAircraft,

    /// Chance one bomb, rocket or depth charge starts a fire, per aircraft.
    AircraftFireChance,

    /// How much less likely shells are to set the ship on fire: 100 minus
    /// the hull's fire-node probability times the build's `burnProb`, in
    /// percent.
    FireChanceReduction,

    /// Fires that can burn at once.
    FireZones,
    FireDuration,

    /// Percent of maximum health one fire burns per second.
    FireDamagePerSecond,

    /// Health one fire burns over its whole duration.
    FireDamage,

    FloodDuration,

    /// Percent of maximum health one flood takes per second.
    FloodDamagePerSecond,

    /// Health one flood takes over its whole duration.
    FloodDamage,
}

impl DerivedStat {
    pub const ALL: &'static [DerivedStat] = &[
        DerivedStat::MainBatteryBarrels,
        DerivedStat::MainBatterySalvoDamage,
        DerivedStat::MainBatteryDamagePerMinute,
        DerivedStat::MainBatteryDispersionAtStockRange,
        DerivedStat::MainBatteryVerticalDispersionAtStockRange,
        DerivedStat::SecondaryDispersionAtStockRange,
        DerivedStat::SecondaryVerticalDispersionAtStockRange,
        DerivedStat::TorpedoTubes,
        DerivedStat::ConsumableAllChargesTime,
        DerivedStat::FiresPerMinuteTotal,
        DerivedStat::FiresPerMinuteTotalAllMounts,
        DerivedStat::FiresPerMinuteMainBattery,
        DerivedStat::FiresPerMinuteSecondary,
        DerivedStat::FiresPerMinuteSecondaryGroup,
        DerivedStat::FiresPerMinuteSecondaryAllMounts,
        DerivedStat::FiresPerMinuteAircraft,
        DerivedStat::AircraftFireChance,
        DerivedStat::FireChanceReduction,
        DerivedStat::FireZones,
        DerivedStat::FireDuration,
        DerivedStat::FireDamagePerSecond,
        DerivedStat::FireDamage,
        DerivedStat::FloodDuration,
        DerivedStat::FloodDamagePerSecond,
        DerivedStat::FloodDamage,
    ];

    /// Dotted name in the namespace of `TtxStat::field_key`.
    pub fn field_key(self) -> &'static str {
        match self {
            Self::MainBatteryBarrels => "artillery.barrels",
            Self::MainBatterySalvoDamage => "artillery.shells.salvo_damage",
            Self::MainBatteryDamagePerMinute => "artillery.shells.damage_per_minute",
            Self::MainBatteryDispersionAtStockRange => "artillery.dispersion_at_stock_range",
            Self::MainBatteryVerticalDispersionAtStockRange => "artillery.dispersion_vertical_at_stock_range",
            Self::SecondaryDispersionAtStockRange => "secondary.dispersion_at_stock_range",
            Self::SecondaryVerticalDispersionAtStockRange => "secondary.dispersion_vertical_at_stock_range",
            Self::TorpedoTubes => "torpedoes.tubes",
            Self::ConsumableAllChargesTime => "consumable.all_charges_time",
            Self::FiresPerMinuteTotal => "fires_per_minute.total",
            Self::FiresPerMinuteTotalAllMounts => "fires_per_minute.total_all_mounts",
            Self::FiresPerMinuteMainBattery => "fires_per_minute.main_battery",
            Self::FiresPerMinuteSecondary => "fires_per_minute.secondary",
            Self::FiresPerMinuteSecondaryGroup => "fires_per_minute.secondary_group",
            Self::FiresPerMinuteSecondaryAllMounts => "fires_per_minute.secondary_all_mounts",
            Self::FiresPerMinuteAircraft => "fires_per_minute.aircraft",
            Self::AircraftFireChance => "aircraft.fire_chance",
            Self::FireChanceReduction => "durability.fire_chance_reduction",
            Self::FireZones => "durability.fire_zones",
            Self::FireDuration => "durability.fire_duration",
            Self::FireDamagePerSecond => "durability.fire_damage_per_second",
            Self::FireDamage => "durability.fire_damage",
            Self::FloodDuration => "durability.flood_duration",
            Self::FloodDamagePerSecond => "durability.flood_damage_per_second",
            Self::FloodDamage => "durability.flood_damage",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MainBatteryBarrels => "Main battery barrels, all mounts",
            Self::MainBatterySalvoDamage => "Main battery salvo damage",
            Self::MainBatteryDamagePerMinute => "Main battery damage per minute",
            Self::MainBatteryDispersionAtStockRange => "Main battery horizontal dispersion at stock firing range",
            Self::MainBatteryVerticalDispersionAtStockRange => "Main battery vertical dispersion at stock firing range",
            Self::SecondaryDispersionAtStockRange => "Secondary horizontal dispersion at stock secondary range",
            Self::SecondaryVerticalDispersionAtStockRange => "Secondary vertical dispersion at stock secondary range",
            Self::TorpedoTubes => "Torpedo tubes, all launchers",
            Self::ConsumableAllChargesTime => "Time to use every charge",
            Self::FiresPerMinuteTotal => {
                "Fire starts per minute against one target at its best bearing, all armament, every projectile hitting"
            }
            Self::FiresPerMinuteTotalAllMounts => {
                "Fire starts per minute with every gun mount firing at once, all armament: a ceiling no single target sees"
            }
            Self::FiresPerMinuteMainBattery => {
                "Main battery fire starts per minute against one target at its best bearing, every shell hitting"
            }
            Self::FiresPerMinuteSecondary => {
                "Secondary fire starts per minute against one target at its best bearing, every group, every shell hitting"
            }
            Self::FiresPerMinuteSecondaryGroup => {
                "Secondary fire starts per minute of one gun group against one target at its best bearing, every shell hitting"
            }
            Self::FiresPerMinuteSecondaryAllMounts => {
                "Secondary fire starts per minute with every mount firing at once: a ceiling no single target sees"
            }
            Self::FiresPerMinuteAircraft => "Aircraft fire starts per minute, all ordnance hitting",
            Self::AircraftFireChance => "Fire chance of one aircraft bomb, rocket or depth charge",
            Self::FireChanceReduction => "Fire chance reduction against shells hitting the ship",
            Self::FireZones => "Fires that can burn at once",
            Self::FireDuration => "Fire duration",
            Self::FireDamagePerSecond => "Fire damage per second, percent of maximum health",
            Self::FireDamage => "Damage of one fire over its whole duration",
            Self::FloodDuration => "Flooding duration",
            Self::FloodDamagePerSecond => "Flooding damage per second, percent of maximum health",
            Self::FloodDamage => "Damage of one flood over its whole duration",
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::MainBatteryBarrels | Self::TorpedoTubes | Self::FireZones => Unit::Count,
            Self::MainBatterySalvoDamage | Self::FireDamage | Self::FloodDamage => Unit::Hp,
            Self::MainBatteryDamagePerMinute => Unit::HpPerMinute,
            Self::MainBatteryDispersionAtStockRange
            | Self::MainBatteryVerticalDispersionAtStockRange
            | Self::SecondaryDispersionAtStockRange
            | Self::SecondaryVerticalDispersionAtStockRange => Unit::Meters,
            Self::ConsumableAllChargesTime | Self::FireDuration | Self::FloodDuration => Unit::Seconds,
            Self::FiresPerMinuteTotal
            | Self::FiresPerMinuteTotalAllMounts
            | Self::FiresPerMinuteMainBattery
            | Self::FiresPerMinuteSecondary
            | Self::FiresPerMinuteSecondaryGroup
            | Self::FiresPerMinuteSecondaryAllMounts
            | Self::FiresPerMinuteAircraft => Unit::FiresPerMinute,
            Self::AircraftFireChance
            | Self::FireChanceReduction
            | Self::FireDamagePerSecond
            | Self::FloodDamagePerSecond => Unit::Percent,
        }
    }

    pub fn direction(self) -> Option<Direction> {
        match self {
            Self::MainBatteryBarrels
            | Self::MainBatterySalvoDamage
            | Self::MainBatteryDamagePerMinute
            | Self::TorpedoTubes
            | Self::FireChanceReduction => Some(Direction::Higher),
            Self::MainBatteryDispersionAtStockRange
            | Self::MainBatteryVerticalDispersionAtStockRange
            | Self::SecondaryDispersionAtStockRange
            | Self::SecondaryVerticalDispersionAtStockRange
            | Self::FireZones
            | Self::FireDuration
            | Self::FireDamagePerSecond
            | Self::FireDamage
            | Self::FloodDuration
            | Self::FloodDamagePerSecond
            | Self::FloodDamage => Some(Direction::Lower),

            // More charges and longer action raise it, and so does a longer reload.
            Self::ConsumableAllChargesTime => None,
            Self::FiresPerMinuteTotal
            | Self::FiresPerMinuteTotalAllMounts
            | Self::FiresPerMinuteMainBattery
            | Self::FiresPerMinuteSecondary
            | Self::FiresPerMinuteSecondaryGroup
            | Self::FiresPerMinuteSecondaryAllMounts
            | Self::FiresPerMinuteAircraft
            | Self::AircraftFireChance => Some(Direction::Higher),
        }
    }

    /// The card stats it is computed from, so the inputs that move them
    /// move it.
    pub fn sources(self) -> &'static [TtxStat] {
        match self {
            Self::MainBatteryBarrels => &[TtxStat::GunNumBarrels, TtxStat::GunNumGuns],
            Self::MainBatterySalvoDamage => &[TtxStat::ShellDamage, TtxStat::GunNumBarrels, TtxStat::GunNumGuns],
            Self::MainBatteryDamagePerMinute => {
                &[TtxStat::ShellDamage, TtxStat::GunNumBarrels, TtxStat::GunNumGuns, TtxStat::ArtilleryReloadTime]
            }
            Self::MainBatteryDispersionAtStockRange => &[TtxStat::ArtilleryDispersion],
            Self::MainBatteryVerticalDispersionAtStockRange => &[TtxStat::ArtilleryDispersionVertical],
            Self::SecondaryDispersionAtStockRange => &[TtxStat::SecondaryDispersion],
            Self::SecondaryVerticalDispersionAtStockRange => &[TtxStat::SecondaryDispersionVertical],
            Self::TorpedoTubes => &[TtxStat::LauncherNumBarrels],
            Self::ConsumableAllChargesTime => {
                &[TtxStat::ConsumableCharges, TtxStat::ConsumableWorkTime, TtxStat::ConsumableReloadTime]
            }
            Self::FiresPerMinuteTotal | Self::FiresPerMinuteTotalAllMounts => &[
                TtxStat::ShellBurnChance,
                TtxStat::ArtilleryReloadTime,
                TtxStat::SecondaryShellBurnChance,
                TtxStat::SecondaryReloadTime,
            ],
            Self::FiresPerMinuteMainBattery => &[TtxStat::ShellBurnChance, TtxStat::ArtilleryReloadTime],
            Self::FiresPerMinuteSecondary
            | Self::FiresPerMinuteSecondaryGroup
            | Self::FiresPerMinuteSecondaryAllMounts => {
                &[TtxStat::SecondaryShellBurnChance, TtxStat::SecondaryReloadTime]
            }

            // The card lists no aircraft stat.
            Self::FiresPerMinuteAircraft | Self::AircraftFireChance => &[],
            Self::FireDamage | Self::FloodDamage => &[TtxStat::Health],
            Self::FireChanceReduction
            | Self::FireZones
            | Self::FireDuration
            | Self::FireDamagePerSecond
            | Self::FloodDuration
            | Self::FloodDamagePerSecond => &[],
        }
    }

    /// Modifiers it reads from the build besides the card stats, so the
    /// skills and upgrades carrying them move it.
    pub fn modifiers(self) -> &'static [&'static str] {
        match self {
            Self::FireChanceReduction => &["burnProb"],
            Self::FireDuration => &["hlCritTimeCoeff", "burnTime"],
            Self::FireDamagePerSecond => &["vulnerabilityBurn"],
            Self::FireDamage => &["hlCritTimeCoeff", "burnTime", "vulnerabilityBurn"],
            Self::FloodDuration => &["hlCritTimeCoeff", "floodTime"],
            Self::FloodDamagePerSecond => &["vulnerabilityFlood"],
            Self::FloodDamage => &["hlCritTimeCoeff", "floodTime", "vulnerabilityFlood"],
            Self::MainBatteryBarrels
            | Self::MainBatterySalvoDamage
            | Self::MainBatteryDamagePerMinute
            | Self::MainBatteryDispersionAtStockRange
            | Self::MainBatteryVerticalDispersionAtStockRange
            | Self::SecondaryDispersionAtStockRange
            | Self::SecondaryVerticalDispersionAtStockRange
            | Self::TorpedoTubes
            | Self::ConsumableAllChargesTime
            | Self::FiresPerMinuteTotal
            | Self::FiresPerMinuteTotalAllMounts
            | Self::FiresPerMinuteMainBattery
            | Self::FiresPerMinuteSecondary
            | Self::FiresPerMinuteSecondaryGroup
            | Self::FiresPerMinuteSecondaryAllMounts
            | Self::FiresPerMinuteAircraft
            | Self::AircraftFireChance
            | Self::FireZones => &[],
        }
    }

    /// Whether `sources` covers every input that moves it. Aircraft
    /// ordnance reads modifiers the card records under no stat.
    pub fn fully_traced(self) -> bool {
        !matches!(
            self,
            Self::FiresPerMinuteTotal
                | Self::FiresPerMinuteTotalAllMounts
                | Self::FiresPerMinuteAircraft
                | Self::AircraftFireChance
        )
    }

    /// Whether it counts only the gun mounts bearing on one target, and so
    /// needs [`Derivation::coverage`].
    pub fn reads_mount_bearings(self) -> bool {
        !self.bearing_armaments().is_empty()
    }

    /// The gun armaments it counts against one target.
    pub fn bearing_armaments(self) -> &'static [FireArmament] {
        match self {
            Self::FiresPerMinuteTotal => &[FireArmament::MainBattery, FireArmament::Secondary],
            Self::FiresPerMinuteMainBattery => &[FireArmament::MainBattery],
            Self::FiresPerMinuteSecondary | Self::FiresPerMinuteSecondaryGroup => &[FireArmament::Secondary],
            _ => &[],
        }
    }

    /// Whether it is one of the fire start stats, whose ranking lists each
    /// armament's share.
    pub fn is_fire_starts(self) -> bool {
        matches!(
            self,
            Self::FiresPerMinuteTotal
                | Self::FiresPerMinuteTotalAllMounts
                | Self::FiresPerMinuteMainBattery
                | Self::FiresPerMinuteSecondary
                | Self::FiresPerMinuteSecondaryGroup
                | Self::FiresPerMinuteSecondaryAllMounts
                | Self::FiresPerMinuteAircraft
        )
    }
}

/// A derived stat value in its unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "unit", content = "value", rename_all = "snake_case")]
pub enum DerivedValue {
    Count(u32),
    Hp(Hp),
    DamagePerMinute(DamagePerMinute),
    Meters(Meters),
    Seconds(Seconds),
    FiresPerMinute(FiresPerMinute),
    Percent(Percent),
}

impl DerivedValue {
    pub fn as_f32(self) -> f32 {
        match self {
            Self::Count(count) => count as f32,
            Self::Hp(hp) => hp.value(),
            Self::DamagePerMinute(rate) => rate.value(),
            Self::Meters(meters) => meters.value(),
            Self::Seconds(seconds) => seconds.value(),
            Self::FiresPerMinute(rate) => rate.value(),
            Self::Percent(percent) => percent.value(),
        }
    }
}

impl fmt::Display for DerivedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(count) => write!(f, "{count}"),
            Self::Hp(hp) => write!(f, "{hp}"),
            Self::DamagePerMinute(rate) => write!(f, "{:.0} hp/min", rate.value()),
            Self::Meters(meters) => write!(f, "{:.0} m", meters.value()),
            Self::Seconds(seconds) => write!(f, "{seconds}"),
            Self::FiresPerMinute(rate) => write!(f, "{:.2} fires/min", rate.value()),
            Self::Percent(percent) => write!(f, "{:.2}%", percent.value()),
        }
    }
}

/// One derived stat value; `qualifier` names the ammo type, secondary mount
/// or consumable it belongs to, as on the matching card rows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DerivedRow {
    pub stat: DerivedStat,
    pub qualifier: Option<String>,
    pub value: DerivedValue,
}

/// Firing ranges of the stock ship, the distances the `AtStockRange`
/// dispersions are measured at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StockRanges {
    pub main_battery: Option<Km>,
    pub secondary: Option<Km>,
}

impl StockRanges {
    /// The ranges on the stock card: stock modules, no skills or upgrades.
    pub fn from_stock_card(stock: &ShipStats) -> Self {
        Self {
            main_battery: stock.artillery.as_ref().and_then(|artillery| artillery.range),
            secondary: stock.secondaries.as_ref().map(|secondaries| secondaries.range),
        }
    }
}

/// The inputs a derived stat reads besides the card.
pub struct Derivation<'a> {
    pub ship: &'a Param,
    pub selection: &'a ShipUpgradeSelection,
    pub stats: &'a ShipStats,

    /// The modifiers the card was computed with.
    pub modifiers: &'a ModifierBundle,

    /// Fire Prevention Expert is learned (`BuildEffects::merges_fire_zones`).
    pub merges_fire_zones: bool,
    pub stock_ranges: StockRanges,
    pub provider: &'a dyn GameParamProvider,

    /// Which gun mounts bear at each bearing. Without it the one-target fire
    /// stats are left out and only the all-mount ceilings remain.
    pub coverage: Option<&'a MountCoverage>,
}

impl Derivation<'_> {
    /// Every derived stat the ship has, in `DerivedStat::ALL` order.
    pub fn rows(&self) -> Vec<DerivedRow> {
        let mut rows = Vec::new();

        let barrels = self.main_battery_barrels();
        if let Some(barrels) = barrels {
            rows.push(DerivedRow {
                stat: DerivedStat::MainBatteryBarrels,
                qualifier: None,
                value: DerivedValue::Count(barrels),
            });
        }

        let salvos = self.main_battery_salvos();
        for (qualifier, salvo) in &salvos {
            rows.push(DerivedRow {
                stat: DerivedStat::MainBatterySalvoDamage,
                qualifier: Some(qualifier.clone()),
                value: DerivedValue::Hp(*salvo),
            });
        }

        let reload = self
            .stats
            .artillery
            .as_ref()
            .and_then(|artillery| artillery.reload_time)
            .filter(|reload| reload.value() > 0.0);
        if let Some(reload) = reload {
            for (qualifier, salvo) in &salvos {
                rows.push(DerivedRow {
                    stat: DerivedStat::MainBatteryDamagePerMinute,
                    qualifier: Some(qualifier.clone()),
                    value: DerivedValue::DamagePerMinute(DamagePerMinute::from_reload(
                        salvo.value(),
                        units::Seconds::new(reload.value()),
                    )),
                });
            }
        }

        if let Some(ellipse) = self.main_battery_dispersion_at_stock_range() {
            rows.push(DerivedRow {
                stat: DerivedStat::MainBatteryDispersionAtStockRange,
                qualifier: None,
                value: DerivedValue::Meters(ellipse.horizontal.to_meters()),
            });
            rows.push(DerivedRow {
                stat: DerivedStat::MainBatteryVerticalDispersionAtStockRange,
                qualifier: None,
                value: DerivedValue::Meters(ellipse.vertical.to_meters()),
            });
        }

        for (label, ellipse) in self.secondary_dispersion_at_stock_range() {
            rows.push(DerivedRow {
                stat: DerivedStat::SecondaryDispersionAtStockRange,
                qualifier: Some(label.clone()),
                value: DerivedValue::Meters(ellipse.horizontal.to_meters()),
            });
            rows.push(DerivedRow {
                stat: DerivedStat::SecondaryVerticalDispersionAtStockRange,
                qualifier: Some(label),
                value: DerivedValue::Meters(ellipse.vertical.to_meters()),
            });
        }

        if let Some(torpedoes) = &self.stats.torpedoes {
            let tubes: u32 = torpedoes.launchers.iter().filter_map(|launcher| launcher.num_barrels).sum();
            if tubes > 0 {
                rows.push(DerivedRow {
                    stat: DerivedStat::TorpedoTubes,
                    qualifier: None,
                    value: DerivedValue::Count(tubes),
                });
            }
        }

        for card in &self.stats.consumables {
            let consumable = &card.stats;
            let (AmmoCount::Finite(charges), Some(work_time)) = (consumable.charges, consumable.work_time) else {
                continue;
            };
            let cycle = work_time.value() + consumable.reload_time.value();
            rows.push(DerivedRow {
                stat: DerivedStat::ConsumableAllChargesTime,
                qualifier: Some(card.label.clone()),
                value: DerivedValue::Seconds(Seconds::from(charges as f32 * cycle)),
            });
        }

        let fires = FireInputs {
            ship: self.ship,
            selection: self.selection,
            stats: self.stats,
            modifiers: self.modifiers,
            provider: self.provider,
        }
        .sources();
        if let Some(total) = total_fires_all_mounts(&fires) {
            rows.push(DerivedRow {
                stat: DerivedStat::FiresPerMinuteTotalAllMounts,
                qualifier: None,
                value: DerivedValue::FiresPerMinute(total),
            });
        }
        // Aircraft fire starts do not depend on the bearing, so a ship
        // with no gun that starts fires needs no mount facings.
        let no_mounts = MountCoverage::default();
        let coverage = self.coverage.or_else(|| {
            fires
                .iter()
                .all(|source| matches!(source.armament, FireArmament::Squadron | FireArmament::AirStrike))
                .then_some(&no_mounts)
        });
        if let Some(coverage) = coverage {
            if let Some(total) = total_fires_by_bearing(&fires, coverage) {
                rows.push(DerivedRow {
                    stat: DerivedStat::FiresPerMinuteTotal,
                    qualifier: None,
                    value: DerivedValue::FiresPerMinute(total.at(total.best())),
                });
            }
            if let Some(secondaries) = secondary_fires_by_bearing(&fires, coverage) {
                rows.push(DerivedRow {
                    stat: DerivedStat::FiresPerMinuteSecondary,
                    qualifier: None,
                    value: DerivedValue::FiresPerMinute(secondaries.at(secondaries.best())),
                });
            }
            for source in &fires {
                let stat = match source.armament {
                    FireArmament::MainBattery => DerivedStat::FiresPerMinuteMainBattery,
                    FireArmament::Secondary => DerivedStat::FiresPerMinuteSecondaryGroup,
                    // Aircraft do not depend on the bearing.
                    FireArmament::Squadron | FireArmament::AirStrike => continue,
                };
                let by_bearing = source.by_bearing(coverage);
                rows.push(DerivedRow {
                    stat,
                    qualifier: Some(source.qualifier.clone()),
                    value: DerivedValue::FiresPerMinute(by_bearing.at(by_bearing.best())),
                });
            }
        }
        let secondary_ceiling: Vec<FiresPerMinute> = fires
            .iter()
            .filter(|source| source.armament == FireArmament::Secondary)
            .map(|source| source.fires_per_minute)
            .collect();
        if !secondary_ceiling.is_empty() {
            rows.push(DerivedRow {
                stat: DerivedStat::FiresPerMinuteSecondaryAllMounts,
                qualifier: None,
                value: DerivedValue::FiresPerMinute(secondary_ceiling.into_iter().sum()),
            });
        }
        for source in
            fires.iter().filter(|source| matches!(source.armament, FireArmament::Squadron | FireArmament::AirStrike))
        {
            rows.push(DerivedRow {
                stat: DerivedStat::FiresPerMinuteAircraft,
                qualifier: Some(source.qualifier.clone()),
                value: DerivedValue::FiresPerMinute(source.fires_per_minute),
            });
        }
        for source in
            fires.iter().filter(|source| matches!(source.armament, FireArmament::Squadron | FireArmament::AirStrike))
        {
            rows.push(DerivedRow {
                stat: DerivedStat::AircraftFireChance,
                qualifier: Some(source.qualifier.clone()),
                value: DerivedValue::Percent(source.fire_chance),
            });
        }

        rows.extend(self.fire_and_flood_rows());

        rows
    }

    /// Fire resistance, fire and flooding of the selected hull with the build
    /// applied.
    fn fire_and_flood_rows(&self) -> Vec<DerivedRow> {
        let Some(hull) = self
            .ship
            .vehicle()
            .and_then(|vehicle| vehicle.ttx_components())
            .and_then(|components| components.hull(self.selection.hull.as_deref()?))
        else {
            return Vec::new();
        };
        let health = self.stats.durability.as_ref().and_then(|durability| durability.health);
        let row = |stat, value| DerivedRow { stat, qualifier: None, value };
        let percent = |value: f32| DerivedValue::Percent(units::Percent::new((value * 100.0).round() / 100.0));
        let damage = |percent_per_sec: f32, duration: Seconds| {
            health.map(|health| Hp::from(health.value() * percent_per_sec / 100.0 * duration.value()))
        };

        let mut rows = Vec::new();
        if let Some(fire) = FireResistance::new(hull, self.modifiers, self.merges_fire_zones) {
            rows.push(row(DerivedStat::FireChanceReduction, percent(100.0 * (1.0 - fire.chance_factor()))));
            rows.push(row(DerivedStat::FireZones, DerivedValue::Count(fire.zones)));
            rows.push(row(DerivedStat::FireDuration, DerivedValue::Seconds(fire.duration)));
            rows.push(row(DerivedStat::FireDamagePerSecond, percent(fire.damage_per_sec.value())));
            if let Some(damage) = damage(fire.damage_per_sec.value(), fire.duration) {
                rows.push(row(DerivedStat::FireDamage, DerivedValue::Hp(damage)));
            }
        }
        if let Some(flooding) = Flooding::new(hull, self.modifiers) {
            rows.push(row(DerivedStat::FloodDuration, DerivedValue::Seconds(flooding.duration)));
            rows.push(row(DerivedStat::FloodDamagePerSecond, percent(flooding.damage_per_sec.value())));
            if let Some(damage) = damage(flooding.damage_per_sec.value(), flooding.duration) {
                rows.push(row(DerivedStat::FloodDamage, DerivedValue::Hp(damage)));
            }
        }

        rows
    }

    /// Damage of one salvo of every main battery mount, per ammo type as the
    /// card's shell rows name it.
    fn main_battery_salvos(&self) -> Vec<(String, Hp)> {
        let Some(artillery) = self.stats.artillery.as_ref() else {
            return Vec::new();
        };
        let mounts = aiming_mounts(self.ship, self.selection, ComponentType::Artillery);

        let mut kinds: Vec<(String, Vec<&ShellStats>)> = Vec::new();
        for (idx, shell) in artillery.shells.iter().enumerate() {
            let qualifier = shell_qualifier(shell, idx);
            match kinds.iter_mut().find(|(kind, _)| *kind == qualifier) {
                Some((_, shells)) => shells.push(shell),
                None => kinds.push((qualifier, vec![shell])),
            }
        }

        kinds
            .into_iter()
            .filter_map(|(kind, shells)| {
                let salvo: f32 =
                    mounts.iter().map(|(mount, barrels)| mount_salvo(mount.ammo(), *barrels, &shells).value()).sum();
                (salvo > 0.0).then(|| (kind, Hp::from(salvo)))
            })
            .collect()
    }

    /// Barrels of every main battery mount of the selected guns.
    fn main_battery_barrels(&self) -> Option<u32> {
        let components = self.ship.vehicle()?.ttx_components()?;
        let artillery = components.artillery(self.selection.artillery.as_deref()?)?;
        if artillery.guns.is_empty() {
            return None;
        }

        artillery.guns.iter().map(|gun| gun.num_barrels.map(|barrels| barrels.round() as u32)).sum()
    }

    fn main_battery_dispersion_at_stock_range(&self) -> Option<DispersionEllipse> {
        let stock_range = self.stock_ranges.main_battery?;
        let max_range = self.stats.artillery.as_ref()?.range?;
        let components = self.ship.vehicle()?.ttx_components()?;
        let artillery = components.artillery(self.selection.artillery.as_deref()?)?;
        let curve = artillery.guns.first()?.dispersion_curve()?;
        let profile = ArtilleryDispersion { curve, max_range, ideal_radius_coef: self.modifiers.coef("GMIdealRadius") };

        Some(profile.ellipse_at(stock_range))
    }

    /// Per secondary mount, by its card label. The guns of a mount share one
    /// dispersion curve.
    fn secondary_dispersion_at_stock_range(&self) -> Vec<(String, DispersionEllipse)> {
        let (Some(stock_range), Some(battery)) = (self.stock_ranges.secondary, self.stats.secondaries.as_ref()) else {
            return Vec::new();
        };
        let Some(atba) = self
            .ship
            .vehicle()
            .and_then(|vehicle| vehicle.ttx_components())
            .and_then(|components| components.secondaries(self.selection.hull.as_deref()?))
        else {
            return Vec::new();
        };
        let ideal_radius_coef = self.modifiers.coef("GSIdealRadius");

        battery
            .mounts
            .iter()
            .filter_map(|mount| {
                let gun = atba.guns.iter().find(|gun| mount.hardpoints.contains(&gun.hardpoint))?;
                let curve = gun.stats.dispersion_curve()?;
                let profile = ArtilleryDispersion { curve, max_range: battery.range, ideal_radius_coef };
                Some((mount.label.clone(), profile.ellipse_at(stock_range)))
            })
            .collect()
    }
}

/// Damage of one salvo of a mount that lists `ammo` and has `barrels`: each
/// barrel fires the first of `shells` the mount lists. Zero when it lists
/// none of them.
pub fn mount_salvo(ammo: &[String], barrels: u32, shells: &[&ShellStats]) -> Hp {
    let damage = ammo
        .iter()
        .find_map(|name| shells.iter().find(|shell| shell.name == *name).and_then(|shell| shell.damage))
        .map_or(0.0, |damage| damage.value());

    Hp::from(barrels as f32 * damage)
}

/// The qualifier the card gives a shell row: ammo type, else projectile name,
/// else index (`ShipStats::rows`).
fn shell_qualifier(shell: &ShellStats, idx: usize) -> String {
    shell
        .ammo_kind
        .clone()
        .or_else(|| (!shell.name.is_empty()).then(|| shell.name.clone()))
        .unwrap_or_else(|| idx.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(name: &str, damage: Option<f32>) -> ShellStats {
        ShellStats { name: name.to_owned(), damage: damage.map(Hp::from), ..ShellStats::default() }
    }

    #[test]
    fn a_mount_fires_the_first_listed_shell_with_damage() {
        let he = shell("PAPA001_HE", Some(2100.0));
        let alternative = shell("PAPA002_HE", Some(1800.0));
        let unrated = shell("PAPA003_HE", None);
        let ammo = ["PAPA003_HE", "PAPA002_HE", "PAPA001_HE"].map(str::to_owned);

        assert_eq!(mount_salvo(&ammo, 3, &[&he, &alternative, &unrated]), Hp::from(5400.0));
        assert_eq!(mount_salvo(&ammo[..1], 3, &[&he, &unrated]), Hp::from(0.0));
        assert_eq!(mount_salvo(&ammo, 2, &[]), Hp::from(0.0));
    }
}
