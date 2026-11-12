//! AP shells that depart from their class's standard: the angles at which
//! they ricochet, the fuse time and the plate thickness that arms the fuse.

use std::collections::BTreeMap;
use std::collections::HashSet;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Millimeters;
use wows_core::units::Seconds;

use crate::data::GameData;
use crate::error::QueryError;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::RicochetAngles;
use crate::shells::Shell;
use crate::ship_variants::listing_order;
use crate::ships::Shares;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;

const DEFAULT_LIMIT: usize = 50;

/// A fuse threshold this close to the standard threshold for the caliber is
/// standard: the game rounds caliber / ratio to whole millimeters, and some
/// shells land a millimeter off.
const THRESHOLD_TOLERANCE_MM: f32 = 1.0;

/// A fuse at most this share of the class standard is short, as players mean
/// it: British light cruiser AP at 0.005 s against 0.033 s. Fuses between
/// this and the standard, such as 0.025 s, are slightly short.
const SHORT_FUSE_SHARE: f32 = 0.5;

/// Arguments for `ap_shell_traits`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApShellTraitsArgs {
    /// The departure from the class standard to list.
    #[serde(rename = "trait")]
    pub shell_trait: ApShellTrait,

    pub class: Option<ShipClass>,
    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,

    /// Nation as GameParams spells it, e.g. "USA", "United_Kingdom".
    pub nation: Option<String>,

    /// Battery whose AP shells are read. Defaults to main.
    pub battery: Option<Battery>,

    /// Most ships to list. Defaults to 50.
    pub limit: Option<usize>,
}

/// How an AP shell can depart from the standard of its ship's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApShellTrait {
    /// Ricochets start at a steeper angle, always-ricochet at a steeper
    /// angle, or both, and neither is shallower: "improved penetration
    /// angles", the shell ricochets less.
    ImprovedRicochet,

    /// Fuse time at most half the class standard: detonates much sooner
    /// after penetrating, so far fewer overpenetrations on thin armor.
    ShortFuse,

    /// Fuse time below the class standard but more than half of it, such as
    /// 0.025 s against 0.033 s.
    SlightlyShortFuse,

    /// Fuse time longer than the class standard: travels farther inside the
    /// ship before detonating, so more overpenetrations on thin armor.
    LongFuse,

    /// Arms on thinner plates than the standard for its caliber, so fewer
    /// hits pass through thin plating without detonating.
    LowFuseThreshold,

    /// Needs thicker plates than the standard for its caliber to arm.
    HighFuseThreshold,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApShellTraitsOutput {
    #[serde(rename = "trait")]
    pub shell_trait: ApShellTrait,
    pub battery: Battery,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<TierRange>,

    /// The standard of each class, from every released ship of every tier
    /// and nation.
    pub standards: Vec<ClassStandard>,

    /// The arming threshold most AP shells have: caliber divided by this.
    /// Absent when no shell of the battery lists a threshold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_threshold_caliber_ratio: Option<u32>,

    /// Ships matching the filters whose battery fires AP.
    pub considered: ShipCounts,

    /// Ships among those considered with at least one AP shell with the
    /// trait; listed in `ships`.
    pub qualifying: ShipCounts,

    #[serde(flatten)]
    pub shares: Shares,

    /// Largest departure from the standard first (the shortest fuse for
    /// short_fuse, the steepest angles for improved_ricochet), then highest
    /// tier first, tech tree ships before others of a tier.
    pub ships: Vec<TraitShip>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,

    pub rule: &'static str,
}

/// The values most AP shells of one class share.
#[derive(Debug, Clone, Serialize)]
pub struct ClassStandard {
    pub class: ShipClass,
    pub ricochet: RicochetAngles,
    pub fuse_time_s: Seconds,

    /// Distinct AP shells the standard is drawn from.
    pub shells: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraitShip {
    /// "T10 Des Moines (cruiser, USA)".
    pub ship: String,
    pub shells: Vec<TraitShell>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraitShell {
    pub name: String,
    pub caliber_mm: Millimeters,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ricochet: Option<RicochetAngles>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_time_s: Option<Seconds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_threshold_mm: Option<Millimeters>,

    /// The trait's value beside the standard, e.g. "ricochets from 60 deg,
    /// always at 67.5 deg; cruiser standard 45 / 60 deg".
    pub versus_standard: String,
}

/// The standards every trait is judged against.
struct Standards {
    by_class: BTreeMap<ShipClass, ClassStandard>,
    threshold_ratio: Option<u32>,
}

impl Standards {
    /// Whether `shell` of a `class` ship has `shell_trait`, with the value
    /// beside the standard when it does.
    fn judge(&self, class: ShipClass, shell: &Shell, shell_trait: ApShellTrait) -> Option<String> {
        let standard = self.by_class.get(&class)?;
        let class_name = class.label().replace('_', " ");
        match shell_trait {
            ApShellTrait::ImprovedRicochet => {
                let angles = shell.ricochet?;
                let base = standard.ricochet;
                let improved = angles != base
                    && angles.from_deg.value() >= base.from_deg.value()
                    && angles.always_from_deg.value() >= base.always_from_deg.value();
                improved.then(|| {
                    format!(
                        "ricochets from {} deg, always at {} deg; {class_name} standard {} / {} deg",
                        angles.from_deg.value(),
                        angles.always_from_deg.value(),
                        base.from_deg.value(),
                        base.always_from_deg.value()
                    )
                })
            }
            ApShellTrait::ShortFuse | ApShellTrait::SlightlyShortFuse | ApShellTrait::LongFuse => {
                let fuse = shell.fuse_time_s?.value();
                let base = standard.fuse_time_s.value();
                // In whole microseconds, so 0.005 s is exactly half of 0.01 s.
                let micros = |seconds: f32| (seconds * 1e6).round() as u32;
                let fuse_us = micros(fuse);
                let base_us = micros(base);
                let half_us = micros(base * SHORT_FUSE_SHARE);
                let departs = match shell_trait {
                    ApShellTrait::ShortFuse => fuse_us <= half_us,
                    ApShellTrait::SlightlyShortFuse => fuse_us > half_us && fuse_us < base_us,
                    _ => fuse_us > base_us,
                };
                departs.then(|| format!("fuse {fuse} s; {class_name} standard {base} s"))
            }
            ApShellTrait::LowFuseThreshold | ApShellTrait::HighFuseThreshold => {
                let threshold = shell.fuse_threshold_mm?.value();
                let ratio = self.threshold_ratio?;
                let base = shell.caliber_mm.value() / ratio as f32;
                let departs = if shell_trait == ApShellTrait::LowFuseThreshold {
                    threshold < base - THRESHOLD_TOLERANCE_MM
                } else {
                    threshold > base + THRESHOLD_TOLERANCE_MM
                };
                departs.then(|| {
                    format!(
                        "arms on plates of {threshold} mm or more; standard for {} mm is {} mm (caliber / {})",
                        shell.caliber_mm.value(),
                        base.round(),
                        ratio
                    )
                })
            }
        }
    }
}

impl ApShellTrait {
    /// How far `shell` departs in this trait's direction, lower for a larger
    /// departure, to list the most marked ships first.
    fn departure(self, shell: &TraitShell) -> f32 {
        let threshold_share = shell.fuse_threshold_mm.map(|threshold| threshold.value() / shell.caliber_mm.value());
        match self {
            Self::ImprovedRicochet => shell
                .ricochet
                .map_or(0.0, |angles| -(angles.from_deg.value() * 1000.0 + angles.always_from_deg.value())),
            Self::ShortFuse | Self::SlightlyShortFuse => shell.fuse_time_s.map_or(f32::MAX, |fuse| fuse.value()),
            Self::LongFuse => shell.fuse_time_s.map_or(0.0, |fuse| -fuse.value()),
            Self::LowFuseThreshold => threshold_share.unwrap_or(f32::MAX),
            Self::HighFuseThreshold => threshold_share.map_or(0.0, |share| -share),
        }
    }
}

impl GameData {
    /// Ships whose AP shells of one battery depart from their class's
    /// standard in the way `trait` names.
    pub fn ap_shell_traits(&self, args: &ApShellTraitsArgs) -> Result<ApShellTraitsOutput, QueryError> {
        let battery = args.battery.unwrap_or(Battery::Main);
        let tiers = TierRange::from_bounds(args.min_tier, args.max_tier);
        let standards = self.ap_standards(battery);

        let filter = ShipFilter {
            tiers,
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: false,
            exclude_test: true,
            include_replicas: false,
        };
        let mut considered: Vec<&Ship> = Vec::new();
        let mut qualifying: Vec<(&Ship, Vec<TraitShell>)> = Vec::new();
        for ship in self.ships.iter().filter(|ship| filter.matches(ship)) {
            let shells: Vec<Shell> = self
                .shells(ship)
                .into_iter()
                .filter(|shell| shell.battery == battery && shell.ammo == Ammo::Ap)
                .collect();
            if shells.is_empty() {
                continue;
            }
            considered.push(ship);

            let with_trait: Vec<TraitShell> = shells
                .into_iter()
                .filter_map(|shell| {
                    let versus_standard = standards.judge(ship.class, &shell, args.shell_trait)?;
                    Some(TraitShell {
                        name: shell.name,
                        caliber_mm: shell.caliber_mm,
                        ricochet: shell.ricochet,
                        fuse_time_s: shell.fuse_time_s,
                        fuse_threshold_mm: shell.fuse_threshold_mm,
                        versus_standard,
                    })
                })
                .collect();
            if !with_trait.is_empty() {
                qualifying.push((ship, with_trait));
            }
        }
        let departure = |shells: &[TraitShell]| {
            shells.iter().map(|shell| args.shell_trait.departure(shell)).fold(f32::MAX, f32::min)
        };
        qualifying.sort_by(|left, right| {
            departure(&left.1).total_cmp(&departure(&right.1)).then_with(|| listing_order(left.0, right.0))
        });

        let considered_counts = ShipCounts::from_ships(considered.iter().copied());
        let qualifying_counts = ShipCounts::from_ships(qualifying.iter().map(|(ship, _)| *ship));
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
        let omitted = (qualifying.len() > limit).then(|| qualifying.len() - limit);
        let ships =
            qualifying.into_iter().take(limit).map(|(ship, shells)| TraitShip { ship: ship.label(), shells }).collect();

        Ok(ApShellTraitsOutput {
            shell_trait: args.shell_trait,
            battery,
            tiers,
            standards: standards.by_class.into_values().collect(),
            fuse_threshold_caliber_ratio: standards.threshold_ratio,
            shares: Shares::new(&qualifying_counts.by_class, &considered_counts),
            considered: considered_counts,
            qualifying: qualifying_counts,
            ships,
            omitted,
            rule: "Each class's standard ricochet angles and fuse time are the values most of the distinct AP shells of that battery on the class's released ships have, across every tier and nation; ties go to the lower value. The standard fuse threshold is the caliber divided by the ratio most AP shells have, within 1 mm. improved_ricochet: neither ricochet angle is below the standard and one is above it. short_fuse: fuse time at most half the class standard; slightly_short_fuse: below the standard but more than half of it; long_fuse: above the standard. low_fuse_threshold and high_fuse_threshold: arming threshold more than 1 mm below or above the standard for the caliber. Every gun module's shells are read. Every listed shell is AP by game data; names such as SAP-42 or P.Spr.Gr. are AP designations, not SAP.",
        })
    }

    /// The standard ricochet angles and fuse time of each class and the
    /// common arming threshold ratio, over the distinct AP shells of
    /// `battery` on released ships.
    fn ap_standards(&self, battery: Battery) -> Standards {
        let filter = ShipFilter { exclude_test: true, ..ShipFilter::default() };
        let mut seen: HashSet<(ShipClass, String)> = HashSet::new();
        // Angles in tenths of a degree and fuse times in microseconds, so
        // the tallies key on integers.
        let mut ricochets: BTreeMap<ShipClass, BTreeMap<(u32, u32), usize>> = BTreeMap::new();
        let mut fuses: BTreeMap<ShipClass, BTreeMap<u32, usize>> = BTreeMap::new();
        let mut shell_counts: BTreeMap<ShipClass, usize> = BTreeMap::new();
        let mut ratios: BTreeMap<u32, usize> = BTreeMap::new();
        for ship in self.ships.iter().filter(|ship| filter.matches(ship)) {
            for shell in self.shells(ship) {
                if shell.battery != battery || shell.ammo != Ammo::Ap || !seen.insert((ship.class, shell.param.clone()))
                {
                    continue;
                }
                *shell_counts.entry(ship.class).or_default() += 1;
                if let Some(angles) = shell.ricochet {
                    let key = (tenths(angles.from_deg.value()), tenths(angles.always_from_deg.value()));
                    *ricochets.entry(ship.class).or_default().entry(key).or_default() += 1;
                }
                if let Some(fuse) = shell.fuse_time_s {
                    *fuses.entry(ship.class).or_default().entry((fuse.value() * 1e6).round() as u32).or_default() += 1;
                }
                if let Some(threshold) = shell.fuse_threshold_mm.filter(|threshold| threshold.value() > 0.0) {
                    *ratios.entry((shell.caliber_mm.value() / threshold.value()).round() as u32).or_default() += 1;
                }
            }
        }

        let by_class = shell_counts
            .into_iter()
            .filter_map(|(class, shells)| {
                let (from, always) = mode(ricochets.get(&class)?)?;
                let fuse = mode(fuses.get(&class)?)?;
                Some((
                    class,
                    ClassStandard {
                        class,
                        ricochet: RicochetAngles {
                            from_deg: Degrees::new(from as f32 / 10.0),
                            always_from_deg: Degrees::new(always as f32 / 10.0),
                        },
                        fuse_time_s: Seconds::new(fuse as f32 / 1e6),
                        shells,
                    },
                ))
            })
            .collect();

        Standards { by_class, threshold_ratio: mode(&ratios) }
    }
}

fn tenths(degrees: f32) -> u32 {
    (degrees * 10.0).round() as u32
}

/// The most common key; ties go to the lowest, as keys iterate in order.
fn mode<K: Copy + Ord>(tally: &BTreeMap<K, usize>) -> Option<K> {
    tally.iter().rev().max_by_key(|(_, count)| **count).map(|(key, _)| *key)
}
