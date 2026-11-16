//! Anti-aircraft defense: the auras of a ship's AA, secondary and main
//! battery components, their flak bursts and the priority sector, with the
//! build's modifiers and Defensive AA Fire applied as the client applies them.
//!
//! Aura damage model, from the client scripts:
//! - Every component (`airDefense`, `atba`, `artillery`) may carry auras: dicts
//!   with a `type` of `near`, `medium` or `far` (`Components/AirDefense.py:51-61`).
//!   Each aura covers `minDistance` to `maxDistance` (ballistic meters,
//!   `AirDefense.py:355-356`), so a plane close to the ship is inside every
//!   aura at once.
//! - Continuous damage: `areaDamage` every `areaDamagePeriod` seconds, times
//!   `AAAuraDamage * AAAuraDamageAbsolute` (`calculateAirDefenseAreaDamage`,
//!   `Modifiers/ModifiersApply.py:74-75`) and, with Defensive AA Fire active on
//!   an aura with `enableBarrage`, the consumable's `areaDamageMultiplier` times
//!   `airDefenseBarrageAreaDamageCoef` (`AirDefense.py:247-251`,
//!   `getAuraAreaDamageMultiplier`, `ModifiersApply.py:719-728`). The expected
//!   damage weighs it by `hitChance` (`calculateAverageAuraDPS`,
//!   `ModifiersApply.py:755`). The port card sums auras of one type rounded per
//!   aura, averages their hit chance and takes the longest reach
//!   (`createAirDefenseTTX`, `mab132439/ttx/FactoryAirDefense.py:33-40`).
//! - Flak: an aura with `bubbleDamage` fires salvos of `innerBubbleCount` bursts
//!   placed in the planes' path and `outerBubbleCount` around them, between its
//!   `minDistance` and `maxDistance`. A burst deals `bubbleDamage` per server
//!   tick, 7 ticks per second (`TICKS_PER_SECOND`, `m28c422b5.py:62`), which the
//!   port card shows as the burst damage (`PreprocessedAirDefense.py:115-120`).
//!   `AABubbleDamage` multiplies far auras only
//!   (`calculateAirDefenseAuraBubbleDamage`, `ModifiersApply.py:78-82`);
//!   `AAExtraBubbles` adds inner bursts to the one main flak aura
//!   (`calculateAirDefenseAuraBubblesCount`, `ModifiersApply.py:89-96`,
//!   `AirDefense.py:82-103`); Defensive AA Fire multiplies by
//!   `bubbleDamageMultiplier * airDefenseBarrageBubbleDamageCoef`
//!   (`getAuraBubbleDamageMultiplier`, `ModifiersApply.py:733-738`). The client
//!   spawns `bursts * hitChance` bursts every `BUBBLE_UPDATE_INTERVAL` seconds
//!   (`m979f9c73/AirDefense.py:1068`, `AirPlanes/AntiAirBubble.py:3`).
//! - Priority sector: `prioritySectorPhases` of `(duration, interpolation,
//!   startDamage, strengthFrom, strengthTo)`; the sector strength multiplies the
//!   damage in the chosen sector (`Components/AntiAirManualFire.py:15-117`), and
//!   the first phase's `startDamage` is an instant share of the squadron's
//!   health (`createAirDefenseTTX`, `FactoryAirDefense.py:61-65`).

use std::collections::BTreeMap;

use wows_core::units::DamagePerSecond;

use crate::game_params::keys::ComponentType;
use crate::game_params::ttx::model::Hp;
use crate::game_params::ttx::model::Seconds;
use crate::game_params::ttx::modifiers::ModifierBundle;
use crate::game_params::ttx::selection::ShipUpgradeSelection;
use crate::game_params::types::AbilityCategory;
use crate::game_params::types::BigWorldDistance;
use crate::game_params::types::Km;
use crate::game_params::types::Meters;
use crate::game_params::types::Millimeters;
use crate::game_params::types::Param;

/// `consumableType` of Defensive AA Fire.
const DEFENSIVE_AA_FIRE: &str = "airDefenseDisp";

/// Seconds between flak spawn rounds (`BUBBLE_UPDATE_INTERVAL`,
/// `AirPlanes/AntiAirBubble.py:3`).
pub fn bubble_update_interval() -> Seconds {
    Seconds::from(2.0186)
}

/// Distance band of an aura, its GameParams `type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum AuraBand {
    /// `near`.
    Short,
    /// `medium`.
    Medium,
    /// `far`.
    Long,
}

impl AuraBand {
    pub const ALL: [Self; 3] = [Self::Short, Self::Medium, Self::Long];

    /// The band of a GameParams aura `type`; `None` for other types.
    pub fn from_game_str(kind: &str) -> Option<Self> {
        match kind {
            "near" => Some(Self::Short),
            "medium" => Some(Self::Medium),
            "far" => Some(Self::Long),
            _ => None,
        }
    }

    /// Lowercase key, e.g. `short`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Medium => "medium",
            Self::Long => "long",
        }
    }

    /// e.g. `short-range`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Short => "short-range",
            Self::Medium => "medium-range",
            Self::Long => "long-range",
        }
    }
}

/// Chance an aura's damage or a flak burst lands, `hitChance`, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct HitChance(pub f32);

/// Number of gun mounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct MountCount(pub u32);

/// Number of flak bursts in one salvo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct FlakBurstCount(pub u32);

/// One gun mount feeding an aura: an `HP_*` gun dict of the aura's component
/// named in the aura's `guns`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AntiAirMount {
    /// Hardpoint, e.g. `HP_AGA_1`.
    pub hardpoint: String,
    /// Gun param `name`, e.g. `PAGA003_20mm_Oerlikon_MK4`.
    pub gun: String,
    /// `barrelDiameter` in millimeters.
    pub caliber: Option<Millimeters>,
    /// `numBarrels`.
    pub barrels: Option<u32>,
}

/// Flak fields of an aura with `bubbleDamage`.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct FlakFields {
    /// `bubbleDamage`: damage per server tick inside a burst.
    pub damage_per_tick: Hp,
    /// `innerBubbleCount`: bursts placed in the planes' path.
    pub inner: FlakBurstCount,
    /// `outerBubbleCount`: bursts placed around them.
    pub outer: FlakBurstCount,
    /// `bubbleRadius`, world units of 30 m as the client draws the burst.
    pub radius: BigWorldDistance,
    /// `innerBubbleZone`: depth of the zone inner bursts are placed in.
    pub inner_zone: BigWorldDistance,
    /// `halfOuterBubbleZone`: half width of the zone outer bursts are placed in.
    pub half_outer_zone: BigWorldDistance,
    /// `bubbleDuration`.
    pub duration: Seconds,
}

/// One aura dict of a component, GameParams values.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AntiAirAura {
    /// Key of the aura dict in its component, e.g. `AuraFar_Bubbles`.
    pub name: String,
    pub band: AuraBand,
    /// `areaDamage`, dealt every `area_damage_period`. Zero on flak-only auras.
    pub area_damage: Hp,
    /// `areaDamagePeriod`.
    pub area_damage_period: Seconds,
    /// `round(areaDamage / areaDamagePeriod)`, rounded from the GameParams
    /// doubles as the client does (`FactoryAirDefense.py:37`); single
    /// precision lands some auras half a point low.
    pub area_dps: DamagePerSecond,
    /// `hitChance`.
    pub hit_chance: HitChance,
    /// `minDistance`, ballistic meters.
    pub min_distance: Meters,
    /// `maxDistance`, ballistic meters.
    pub max_distance: Meters,
    /// `enableBarrage`: Defensive AA Fire boosts it.
    pub enable_barrage: bool,
    /// `None` without `bubbleDamage`.
    pub flak: Option<FlakFields>,
    /// Mounts of the aura's `guns`, in GameParams order.
    pub mounts: Vec<AntiAirMount>,
}

/// One phase of `prioritySectorPhases`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct PrioritySectorPhase {
    pub duration: Seconds,
    /// Instant damage on activation, a share of the squadron's health.
    pub start_damage: f32,
    /// Sector damage multiplier at the start and at the end of the phase.
    pub strength_from: f32,
    pub strength_to: f32,
}

/// The auras of one component, and its priority sector when it is the AA
/// component.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AntiAirComponent {
    /// Auras against squadrons (`targetType` 0), in GameParams order.
    pub auras: Vec<AntiAirAura>,
    /// `prioritySectorCooldown`.
    pub priority_sector_cooldown: Option<Seconds>,
    /// `prioritySectorPhases`; empty when the component has none.
    pub priority_sector_phases: Vec<PrioritySectorPhase>,
}

/// Defensive AA Fire multipliers (`ConsumableAirDefenseDispLogic`), from the
/// consumable's `logic`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefensiveAaFire {
    /// `areaDamageMultiplier`.
    pub area_damage_multiplier: f32,
    /// `bubbleDamageMultiplier`.
    pub bubble_damage_multiplier: f32,
}

impl DefensiveAaFire {
    /// The multipliers of a Defensive AA Fire variant; `None` for another
    /// consumable or one lacking either field.
    pub fn from_category(category: &AbilityCategory) -> Option<Self> {
        if category.consumable_type_raw() != DEFENSIVE_AA_FIRE {
            return None;
        }
        let fields = category.effect_fields();

        Some(Self {
            area_damage_multiplier: *fields.get("areaDamageMultiplier")?,
            bubble_damage_multiplier: *fields.get("bubbleDamageMultiplier")?,
        })
    }
}

/// One aura's continuous damage under a build.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuraDamage {
    pub dps: DamagePerSecond,
    pub hit_chance: HitChance,
    pub min_range: Km,
    pub max_range: Km,
}

impl AuraDamage {
    /// `dps * hit_chance`.
    pub fn expected_dps(&self) -> DamagePerSecond {
        DamagePerSecond::new(self.dps.value() * self.hit_chance.0)
    }
}

/// Mounts of one gun feeding a band.
#[derive(Clone, Debug, PartialEq)]
pub struct MountGroup {
    /// Gun param name.
    pub gun: String,
    pub caliber: Option<Millimeters>,
    pub barrels_per_mount: Option<u32>,
    pub count: MountCount,
}

/// Every aura of one band under a build.
#[derive(Clone, Debug, PartialEq)]
pub struct BandDefense {
    pub band: AuraBand,
    /// Auras with continuous damage, in component order.
    pub auras: Vec<AuraDamage>,
    /// Mounts of those auras, by gun, largest caliber first.
    pub mounts: Vec<MountGroup>,
}

impl BandDefense {
    /// Damage per second of every aura of the band, as the port card sums it.
    pub fn dps(&self) -> DamagePerSecond {
        DamagePerSecond::new(self.auras.iter().map(|aura| aura.dps.value()).sum())
    }

    /// Mean hit chance of the auras, as the port card shows it.
    pub fn hit_chance(&self) -> HitChance {
        let total: f32 = self.auras.iter().map(|aura| aura.hit_chance.0).sum();

        HitChance(total / self.auras.len() as f32)
    }

    /// Reach of the farthest aura.
    pub fn range(&self) -> Km {
        Km::from(self.auras.iter().map(|aura| aura.max_range.value()).fold(0.0, f32::max))
    }

    pub fn mount_count(&self) -> MountCount {
        MountCount(self.mounts.iter().map(|group| group.count.0).sum())
    }
}

/// One flak aura under a build.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakAura {
    pub band: AuraBand,
    /// Bursts of one salvo, with `AAExtraBubbles` when it is the main flak aura.
    pub bursts: FlakBurstCount,
    pub inner: FlakBurstCount,
    pub outer: FlakBurstCount,
    /// Damage of one burst: `bubbleDamage * TICKS_PER_SECOND` times modifiers.
    pub damage: Hp,
    pub hit_chance: HitChance,
    pub min_range: Km,
    pub max_range: Km,
    pub radius: BigWorldDistance,
    pub inner_zone: BigWorldDistance,
    pub half_outer_zone: BigWorldDistance,
}

/// The flak of a build, as the port card shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakDefense {
    /// Flak auras in component order.
    pub auras: Vec<FlakAura>,
}

impl FlakDefense {
    /// Bursts of every flak aura (`numBubbles`, `FactoryAirDefense.py:52`).
    pub fn bursts(&self) -> FlakBurstCount {
        FlakBurstCount(self.auras.iter().map(|aura| aura.bursts.0).sum())
    }

    /// Mean burst damage (`FactoryAirDefense.py:54`).
    pub fn damage(&self) -> Hp {
        let total: f32 = self.auras.iter().map(|aura| aura.damage.value()).sum();

        Hp::from(total / self.auras.len() as f32)
    }

    /// Nearest flak reach (`FactoryAirDefense.py:51`).
    pub fn min_range(&self) -> Km {
        Km::from(self.auras.iter().map(|aura| aura.min_range.value()).fold(f32::INFINITY, f32::min))
    }

    /// Shortest far edge of the flak auras (`FactoryAirDefense.py:50`).
    pub fn max_range(&self) -> Km {
        Km::from(self.auras.iter().map(|aura| aura.max_range.value()).fold(f32::INFINITY, f32::min))
    }

    /// Highest hit chance (`FactoryAirDefense.py:49`).
    pub fn hit_chance(&self) -> HitChance {
        HitChance(self.auras.iter().map(|aura| aura.hit_chance.0).fold(0.0, f32::max))
    }
}

/// The priority sector under a build (`FactoryAirDefense.py:61-65`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrioritySector {
    /// Instant damage on activation, a share of the squadron's health:
    /// `startDamage * prioritySectorStartPhaseStrengthCoefficient +
    /// prioritySectorStartPhaseStrengthBonus / 100`.
    pub instant_damage: f32,
    /// Peak damage multiplier in the sector: the strongest phase value times
    /// `prioritySectorStrengthCoefficient` plus
    /// `prioritySectorStrengthBonus / 100`.
    pub strength: f32,
    /// Phase durations times `prioritySectorPhaseDurationCoefficient`.
    pub duration: Seconds,
    /// `prioritySectorCooldown * prioritySectorCooldownMultiplier`.
    pub cooldown: Option<Seconds>,
}

/// AA defense of one module selection under one build.
#[derive(Clone, Debug, PartialEq)]
pub struct AntiAirDefense {
    /// Bands with continuous damage, short to long.
    pub bands: Vec<BandDefense>,
    pub flak: Option<FlakDefense>,
    pub priority_sector: Option<PrioritySector>,
}

impl AntiAirDefense {
    pub fn band(&self, band: AuraBand) -> Option<&BandDefense> {
        self.bands.iter().find(|defense| defense.band == band)
    }
}

/// Components of `selection` that carry auras: the hull's AA and secondary
/// components and the main battery component the hull mounts, without
/// repeats.
pub fn anti_air_components<'a>(ship: &'a Param, selection: &ShipUpgradeSelection) -> Vec<&'a AntiAirComponent> {
    let Some(vehicle) = ship.vehicle() else {
        return Vec::new();
    };
    let Some(components) = vehicle.ttx_components() else {
        return Vec::new();
    };
    let Some(hull) = selection.hull.as_deref().and_then(|name| vehicle.hull_upgrade(name)) else {
        return Vec::new();
    };

    let artillery = match selection.artillery.as_deref() {
        Some(upgrade) => components.component_on_hull(upgrade, hull, ComponentType::Artillery),
        None => hull.component_name(ComponentType::Artillery),
    };
    let mut names: Vec<&str> = Vec::new();
    for name in [hull.component_name(ComponentType::AirDefense), hull.component_name(ComponentType::Atba), artillery]
        .into_iter()
        .flatten()
    {
        if !names.contains(&name) {
            names.push(name);
        }
    }

    names.into_iter().filter_map(|name| components.anti_air.get(name)).collect()
}

/// AA defense of `selection` under `modifiers`, with Defensive AA Fire active
/// when `barrage` is given. `None` when no selected component has an aura.
pub fn anti_air(
    ship: &Param,
    selection: &ShipUpgradeSelection,
    modifiers: &ModifierBundle,
    barrage: Option<DefensiveAaFire>,
) -> Option<AntiAirDefense> {
    let components = anti_air_components(ship, selection);
    let auras: Vec<&AntiAirAura> = components.iter().flat_map(|component| &component.auras).collect();
    if auras.is_empty() {
        return None;
    }

    let aura_damage = modifiers.coef("AAAuraDamage") * modifiers.coef("AAAuraDamageAbsolute");
    let barrage_area = |aura: &AntiAirAura| match barrage {
        Some(barrage) if aura.enable_barrage => {
            barrage.area_damage_multiplier * modifiers.coef("airDefenseBarrageAreaDamageCoef")
        }
        _ => 1.0,
    };
    let barrage_bubble = |aura: &AntiAirAura| match barrage {
        Some(barrage) if aura.enable_barrage => {
            barrage.bubble_damage_multiplier * modifiers.coef("airDefenseBarrageBubbleDamageCoef")
        }
        _ => 1.0,
    };

    let mut bands = Vec::new();
    for band in AuraBand::ALL {
        let of_band: Vec<&AntiAirAura> =
            auras.iter().copied().filter(|aura| aura.band == band && aura.area_dps.value() > 0.0).collect();
        if of_band.is_empty() {
            continue;
        }

        let damage = of_band
            .iter()
            .map(|aura| AuraDamage {
                dps: DamagePerSecond::new(aura.area_dps.value() * aura_damage * barrage_area(aura)),
                hit_chance: aura.hit_chance,
                min_range: aura.min_distance.to_km(),
                max_range: aura.max_distance.to_km(),
            })
            .collect();
        bands.push(BandDefense { band, auras: damage, mounts: mount_groups(&of_band) });
    }

    Some(AntiAirDefense {
        bands,
        flak: flak(&auras, modifiers, &barrage_bubble),
        priority_sector: components.iter().find_map(|component| priority_sector(component, modifiers)),
    })
}

/// The flak auras of `auras`. Extra bursts go to the main flak aura: the most
/// damage per salvo without Defensive AA Fire, then the farthest reach, then
/// the largest mount caliber, the later aura on a tie (`AirDefense.py:82-103`).
fn flak(
    auras: &[&AntiAirAura],
    modifiers: &ModifierBundle,
    barrage_bubble: &dyn Fn(&AntiAirAura) -> f32,
) -> Option<FlakDefense> {
    /// `TICKS_PER_SECOND` (`m28c422b5.py:62`).
    const TICKS_PER_SECOND: f32 = 7.0;

    let (mut flak, ranks): (Vec<FlakAura>, Vec<[f32; 3]>) = auras
        .iter()
        .filter_map(|aura| {
            let fields = aura.flak.as_ref().filter(|fields| fields.damage_per_tick.value() > 0.0)?;
            let bursts = fields.inner.0 + fields.outer.0;
            if bursts == 0 {
                return None;
            }
            let far = if aura.band == AuraBand::Long { modifiers.coef("AABubbleDamage") } else { 1.0 };
            let damage = fields.damage_per_tick.value() * TICKS_PER_SECOND * far;
            let caliber =
                aura.mounts.iter().filter_map(|mount| mount.caliber.map(|caliber| caliber.value())).fold(0.0, f32::max);
            let rank = [damage * bursts as f32, aura.max_distance.value(), caliber];

            let flak = FlakAura {
                band: aura.band,
                bursts: FlakBurstCount(bursts),
                inner: fields.inner,
                outer: fields.outer,
                damage: Hp::from(damage * barrage_bubble(aura)),
                hit_chance: aura.hit_chance,
                min_range: aura.min_distance.to_km(),
                max_range: aura.max_distance.to_km(),
                radius: fields.radius,
                inner_zone: fields.inner_zone,
                half_outer_zone: fields.half_outer_zone,
            };
            Some((flak, rank))
        })
        .unzip();
    let main = (0..flak.len()).reduce(|best, index| {
        let order = ranks[index]
            .iter()
            .zip(&ranks[best])
            .fold(std::cmp::Ordering::Equal, |order, (left, right)| order.then(left.total_cmp(right)));
        if order.is_ge() { index } else { best }
    })?;

    // A fractional or negative bonus is not a burst count.
    let extra = modifiers.bonus("AAExtraBubbles").round().max(0.0) as u32;
    flak[main].inner.0 += extra;
    flak[main].bursts.0 += extra;

    Some(FlakDefense { auras: flak })
}

fn priority_sector(component: &AntiAirComponent, modifiers: &ModifierBundle) -> Option<PrioritySector> {
    let first = component.priority_sector_phases.first()?;
    let peak = component.priority_sector_phases.iter().map(|phase| phase.strength_to).fold(f32::NEG_INFINITY, f32::max);
    let duration: f32 = component.priority_sector_phases.iter().map(|phase| phase.duration.value()).sum();

    Some(PrioritySector {
        instant_damage: (first.start_damage * modifiers.coef("prioritySectorStartPhaseStrengthCoefficient")
            + modifiers.bonus("prioritySectorStartPhaseStrengthBonus") / 100.0)
            .max(0.0),
        strength: peak * modifiers.coef("prioritySectorStrengthCoefficient")
            + modifiers.bonus("prioritySectorStrengthBonus") / 100.0,
        duration: Seconds::from(duration * modifiers.coef("prioritySectorPhaseDurationCoefficient")),
        cooldown: component
            .priority_sector_cooldown
            .map(|cooldown| Seconds::from(cooldown.value() * modifiers.coef("prioritySectorCooldownMultiplier"))),
    })
}

/// Mounts of `auras` by gun, largest caliber first. A mount feeding two
/// auras of the band counts once.
fn mount_groups(auras: &[&AntiAirAura]) -> Vec<MountGroup> {
    let mut seen: Vec<&str> = Vec::new();
    let mut groups: BTreeMap<&str, MountGroup> = BTreeMap::new();
    for mount in auras.iter().flat_map(|aura| &aura.mounts) {
        if seen.contains(&mount.hardpoint.as_str()) {
            continue;
        }
        seen.push(&mount.hardpoint);

        groups
            .entry(&mount.gun)
            .or_insert_with(|| MountGroup {
                gun: mount.gun.clone(),
                caliber: mount.caliber,
                barrels_per_mount: mount.barrels,
                count: MountCount(0),
            })
            .count
            .0 += 1;
    }

    let mut groups: Vec<MountGroup> = groups.into_values().collect();
    groups.sort_by(|left, right| {
        let caliber = |group: &MountGroup| group.caliber.map_or(0.0, |caliber| caliber.value());
        caliber(right).total_cmp(&caliber(left)).then(left.gun.cmp(&right.gun))
    });

    groups
}
