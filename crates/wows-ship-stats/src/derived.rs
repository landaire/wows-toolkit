//! Stats computed from the port stat card that the card itself does not list.

use std::fmt;

use serde::Serialize;
use wows_core::units;
use wows_core::units::DamagePerMinute;
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
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Meters;
use wowsunpack::game_params::types::Param;

use crate::mounts::aiming_mounts;
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
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::MainBatteryBarrels | Self::TorpedoTubes => Unit::Count,
            Self::MainBatterySalvoDamage => Unit::Hp,
            Self::MainBatteryDamagePerMinute => Unit::HpPerMinute,
            Self::MainBatteryDispersionAtStockRange
            | Self::MainBatteryVerticalDispersionAtStockRange
            | Self::SecondaryDispersionAtStockRange
            | Self::SecondaryVerticalDispersionAtStockRange => Unit::Meters,
            Self::ConsumableAllChargesTime => Unit::Seconds,
        }
    }

    pub fn direction(self) -> Option<Direction> {
        match self {
            Self::MainBatteryBarrels
            | Self::MainBatterySalvoDamage
            | Self::MainBatteryDamagePerMinute
            | Self::TorpedoTubes => Some(Direction::Higher),
            Self::MainBatteryDispersionAtStockRange
            | Self::MainBatteryVerticalDispersionAtStockRange
            | Self::SecondaryDispersionAtStockRange
            | Self::SecondaryVerticalDispersionAtStockRange => Some(Direction::Lower),

            // More charges and longer action raise it, and so does a longer reload.
            Self::ConsumableAllChargesTime => None,
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
        }
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
}

impl DerivedValue {
    pub fn as_f32(self) -> f32 {
        match self {
            Self::Count(count) => count as f32,
            Self::Hp(hp) => hp.value(),
            Self::DamagePerMinute(rate) => rate.value(),
            Self::Meters(meters) => meters.value(),
            Self::Seconds(seconds) => seconds.value(),
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
    pub stock_ranges: StockRanges,
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
