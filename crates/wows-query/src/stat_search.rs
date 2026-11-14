//! Fleet search by stat conditions. A ship qualifies with its stock modules,
//! or with the fewest modules, upgrades and captain skills that make every
//! condition hold.

use std::cell::OnceCell;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::FiresPerMinute;
use wows_ship_stats::Derivation;
use wows_ship_stats::DerivedRow;
use wows_ship_stats::DerivedStat;
use wows_ship_stats::Direction;
use wows_ship_stats::FireBreakdown;
use wows_ship_stats::FireInputs;
use wows_ship_stats::MountCoverage;
use wows_ship_stats::StockRanges;
use wows_ship_stats::Unit;
use wows_ship_stats::fire_breakdown;
use wows_ship_stats::resistance::merges_fire_zones;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::skill_grid_data::skill_grid;
use wowsunpack::game_params::ttx::InputId;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::StatKey;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::labels::stat_display_label;
use wowsunpack::game_params::ttx::model::AmmoCount;
use wowsunpack::game_params::ttx::model::Knots;
use wowsunpack::game_params::ttx::model::StatRow;
use wowsunpack::game_params::ttx::model::StatValue;
use wowsunpack::game_params::ttx::provenance::ShipStatsProvenance;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::SkillPointCost;
use wowsunpack::game_params::types::Species;

use crate::armament::AswWeapons;
use crate::armor::ThicknessSpan;
use crate::armor::plates;
use crate::armor::zone_spans;
use crate::builds;
use crate::builds::BestFit;
use crate::builds::Better;
use crate::builds::BuildValue;
use crate::builds::Builds;
use crate::builds::Extract;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::fuzzy;
use crate::fuzzy::Fuzzy;
use crate::loadout::ResolvedLoadout;
use crate::loadout::build_failed;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::names::Interpretations;
use crate::names::search_key;
use crate::shells::Ammo;
use crate::shells::Battery;
use crate::shells::RicochetAngles;
use crate::shells::ricochet_angles;
use crate::shells::shell_ammo;
use crate::ship_variants::listing_order;
use crate::ships::Shares;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;
use crate::speed::SpeedDepth;
use crate::speed::names_condition;
use crate::text::clean;
use crate::upgrades::Upgrade;

/// How a stat value compares to a condition's threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Comparison {
    #[serde(rename = "<")]
    Less,
    #[serde(rename = "<=")]
    AtMost,
    #[serde(rename = ">")]
    Greater,
    #[serde(rename = ">=")]
    AtLeast,
    #[serde(rename = "=")]
    Equal,
    #[serde(rename = "!=")]
    NotEqual,
}

impl Comparison {
    fn symbol(self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::AtMost => "<=",
            Self::Greater => ">",
            Self::AtLeast => ">=",
            Self::Equal => "=",
            Self::NotEqual => "!=",
        }
    }

    /// Compares the value as `format_value` prints it, to two decimals, so a
    /// listed value always meets a threshold copied from it. The tolerance
    /// absorbs float noise only.
    fn holds(self, value: f32, threshold: f32) -> bool {
        let value = if value.is_finite() { shown(value) } else { value };
        let tolerance = 1e-6 * threshold.abs().max(1.0);
        let equal = (value - threshold).abs() <= tolerance;

        match self {
            Self::Less => value < threshold - tolerance,
            Self::AtMost => value <= threshold + tolerance,
            Self::Greater => value > threshold + tolerance,
            Self::AtLeast => value >= threshold - tolerance,
            Self::Equal => equal,
            Self::NotEqual => !equal,
        }
    }

    /// The direction a value must move to approach the threshold, `None` for
    /// equality tests, where any change may help.
    fn direction(self) -> Option<Direction> {
        match self {
            Self::Less | Self::AtMost => Some(Direction::Lower),
            Self::Greater | Self::AtLeast => Some(Direction::Higher),
            Self::Equal | Self::NotEqual => None,
        }
    }
}

/// One condition on a stat.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatCondition {
    /// Stat name from stat_names, e.g. "artillery.reload_time" or "armor.citadel.max".
    pub stat: String,

    pub op: Comparison,

    /// Threshold in the unit stat_names lists for the stat.
    pub value: f32,

    /// Only the row with this qualifier: ammo type for shells ("HE", "AP", "SAP"), torpedo name as values show it, secondary mount or consumable name. Omit to accept any row.
    pub qualifier: Option<String>,
}

/// Arguments for `stat_search`. Give conditions, rank, or both.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatSearchArgs {
    /// Conditions that must all hold. Optional when rank is given.
    #[serde(default)]
    pub conditions: Vec<StatCondition>,

    /// Order ships by one stat on each ship's best-fit build, for "best", "lowest", "highest" or "most" questions. With conditions, ranks only the ships that meet them.
    pub rank: Option<StatRank>,

    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,
    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA".
    pub nation: Option<String>,

    /// Include ships players do not own: old versions, clan and event rentals, co-op bot copies, event battle ships, removed and unavailable copies. Defaults to false.
    pub include_event_variants: Option<bool>,

    /// Leave out ships in closed testing. Defaults to false: they are searched and counted under `test`. A test copy of a released ship is searched only with event variants.
    pub exclude_test_ships: Option<bool>,

    /// Search premium replicas of other ships (e.g. "BA Montana" of Montana, Xuan Wu of Iowa) as separate ships. Defaults to false: they are left out of the results and counts and named in their original's entry, unless a nation filter leaves out the original.
    pub include_replicas: Option<bool>,
}

/// A stat to order ships by.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatRank {
    /// Stat name from stat_names, e.g. "visibility.sea_detection".
    pub stat: String,

    /// "asc": lowest first. "desc": highest first.
    pub order: RankOrder,

    /// Only the row with this qualifier, as for conditions. Omit to rank each ship by its lowest or highest row, as build picks.
    pub qualifier: Option<String>,

    /// Ships listed. Defaults to 10.
    pub limit: Option<RankLimit>,

    /// The build each ship is ranked on. Omit it to rank each ship on the build players run for the stat, the one that makes it better whatever the order: lower detection, reload, dispersion and flight time; higher damage, range and health. So "desc" on visibility.sea_detection lists the worst concealment as players build their ships. "base": stock hull and modules, no captain skills, no upgrades, for base values such as base detection. "lowest" or "highest": the build that lowers or raises the stat most, e.g. how bad a stat can get. A stat with no better direction ranks on the build that moves it furthest in the order.
    pub build: Option<RankBuild>,

    /// For speed stats only (mobility.speed, underwater_mobility.periscope_speed, underwater_mobility.speed): rank each ship on its top speed while a condition holds. "all" ranks each ship on its fastest combination of everything that can hold at once: speed boost consumable active, conditional skills whose triggers can hold together (e.g. Swift in Silence, but not with Dazzle: one needs the ship undetected, the other detected), signals mounted and the special ability active; the row's build names every item. Any other value names one condition as in the ship_stats speed alternatives, e.g. "Engine Boost", "Sierra Mike", "Swift in Silence", "Engine Boost and Sierra Mike". Ships without the condition rank on their always-on best build and their entries carry no condition. Omit it to rank on the always-on best build, without conditional skills, signals, consumables or special abilities. Leave build out with it.
    pub condition: Option<String>,
}

/// The build each ranked ship is valued on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RankBuild {
    /// Stock hull and modules, or the first modules that have the stat
    /// when stock lacks it; no captain skills or upgrades.
    Base,

    /// The modules, captain skills and upgrades that lower the stat most.
    Lowest,

    /// The modules, captain skills and upgrades that raise the stat most.
    Highest,
}

impl RankBuild {
    /// The build to rank on when none is given: the one that improves
    /// `stat`, or with no better direction the one that moves it furthest
    /// in `order`.
    fn default_for(stat: SearchStat, order: RankOrder) -> (Self, BuildBasis) {
        match stat.better() {
            Some(Direction::Lower) => (Self::Lowest, BuildBasis::Usual),
            Some(Direction::Higher) => (Self::Highest, BuildBasis::Usual),
            None => {
                let build = match order {
                    RankOrder::Asc => Self::Lowest,
                    RankOrder::Desc => Self::Highest,
                };
                (build, BuildBasis::Order)
            }
        }
    }
}

/// Why a ranking is on its build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildBasis {
    /// The request named it.
    Given,

    /// The build that improves the stat, which players run.
    Usual,

    /// The stat has no better direction; the build follows the order.
    Order,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RankOrder {
    Asc,
    Desc,
}

impl RankOrder {
    fn better(self) -> Better {
        match self {
            Self::Asc => Better::Lower,
            Self::Desc => Better::Higher,
        }
    }
}

/// Number of ranked ships to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct RankLimit(#[schemars(range(min = 1, max = 25))] pub usize);

#[derive(Debug, Clone, Serialize)]
pub struct StatSearchOutput {
    /// The conditions as understood, e.g. "artillery.reload_time < 5 s".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,

    /// How ships qualify; given with conditions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<&'static str>,

    /// Ships matching the tier, class and nation filters.
    pub considered: ShipCounts,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifying: Option<QualifyingCounts>,

    #[serde(flatten)]
    pub shares: Option<Shares>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking: Option<Ranking>,

    /// Without rank: fewest requirements first, then owned ships before test ships and variants, highest tier first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ships: Option<Vec<StatSearchHit>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,

    /// Ships that are not counted as qualifying or ranked because their
    /// search could not finish.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub undetermined: Vec<UndeterminedShip>,

    /// Ships searched without captain skills because no skill tree is known
    /// for their class in this game version.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub searched_without_skills: Vec<String>,

    /// Conditions whose qualifier names no row of any considered ship in
    /// any configuration searched, so the condition never held.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmatched_qualifiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndeterminedShip {
    pub ship: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualifyingCounts {
    pub total: usize,
    pub by_class: BTreeMap<ShipClass, usize>,

    /// Qualify with stock modules, no upgrades and no skills.
    pub stock: usize,

    /// Qualify only with the listed modules, upgrades or skills.
    pub with_requirements: usize,

    /// Ships in closed testing among `total`.
    #[serde(skip_serializing_if = "is_zero")]
    pub test: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatSearchHit {
    /// "T10 Des Moines (cruiser, USA)".
    pub ship: String,

    #[serde(flatten)]
    pub meets: ConditionMatch,
}

/// How one ship meets every condition.
#[derive(Debug, Clone, Serialize)]
pub struct ConditionMatch {
    /// True when stock modules meet every condition without upgrades or skills.
    pub stock: bool,

    /// The fewest modules, upgrades and skills that meet every condition,
    /// e.g. "upgrade: Main Battery Modification 3".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,

    /// Per condition, the value that meets it, e.g. "artillery.reload_time: 4.5 s".
    pub values: Vec<String>,

    /// Set when some configurations could not be computed, so a smaller set
    /// of requirements may exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchecked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ranking {
    /// The stat, order and build as understood, e.g. "visibility.sea_detection (km), lowest first, on the build that lowers it most".
    pub stat: String,

    pub rule: String,

    /// What the stat does not measure, for nominal armor thickness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caution: Option<&'static str>,

    /// Ships with a value of the stat.
    pub ranked: usize,

    pub ships: Vec<RankedShip>,

    /// Ships left out by the limit that share the value of the last one listed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tied_beyond_limit: Vec<String>,

    /// Ranked ships left out by the limit. They have the stat; ships without
    /// it are in `lacking`, not here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,

    /// Ships that lack the stat, or the qualifier's row, with every module
    /// combination: they are not ranked and not among `omitted`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lacking: Option<LackingShips>,

    /// Context the stat needs, e.g. which tiers meet submarines for
    /// anti-submarine stats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Ships matching the filters that lack a ranked stat, answering "which
/// ships have no X".
#[derive(Debug, Clone, Serialize)]
pub struct LackingShips {
    pub total: usize,
    pub by_tier: BTreeMap<Tier, usize>,
    pub by_class: BTreeMap<ShipClass, usize>,

    /// "T4 Isokaze (destroyer, Japan)", highest tier first.
    pub ships: Vec<String>,

    /// Lacking ships left out of `ships` by the list cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_listed: Option<usize>,
}

impl LackingShips {
    /// `ships` in any order; `None` when there are none.
    fn new(mut ships: Vec<&Ship>) -> Option<Self> {
        /// Lacking ships named; the counts cover every one.
        const LISTED: usize = 60;

        if ships.is_empty() {
            return None;
        }
        ships.sort_by(|left, right| listing_order(left, right));
        let mut by_tier = BTreeMap::new();
        let mut by_class = BTreeMap::new();
        for ship in &ships {
            *by_tier.entry(ship.tier).or_default() += 1;
            *by_class.entry(ship.class).or_default() += 1;
        }

        Some(Self {
            total: ships.len(),
            by_tier,
            by_class,
            not_listed: (ships.len() > LISTED).then(|| ships.len() - LISTED),
            ships: ships.iter().take(LISTED).map(|ship| ship.listing_label()).collect(),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedShip {
    /// Position from 1; ships with equal values share it.
    pub rank: usize,

    /// "T10 Des Moines (cruiser, USA)".
    pub ship: String,

    /// On the build the ranking names: the base value, or the build that
    /// lowers or raises the stat most.
    pub value: String,

    /// Stock hull and modules, or the first modules that have the stat
    /// when stock lacks it; no captain skills or upgrades.
    pub base: String,

    /// Modules, captain skills and upgrades of the build value is on.
    /// Empty for a base ranking and when nothing changes the stat.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<String>,

    /// Captain skills taken only to fill cheaper skill tiers; not conditions
    /// of the build. Not in the tool output.
    #[serde(skip)]
    pub tier_fillers: Vec<String>,

    /// Another ranked ship has the same value.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tied: bool,

    /// With conditions: how the ship meets them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meets: Option<ConditionMatch>,

    /// For the fire start stats: fires against one target at the bearing
    /// where the ranked build starts the most, in total and per armament.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fires_by_armament: Option<FireBreakdown>,

    /// With rank.condition: the speed alternative the value is on, e.g.
    /// "with Engine Boost active". Absent when the ship lacks the condition
    /// and is ranked on its always-on best build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    /// The row the value comes from when the stat has several, e.g. "main
    /// battery 483 mm HE".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
}

/// Arguments for `stat_names`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatNamesArgs {}

#[derive(Debug, Clone, Serialize)]
pub struct StatNamesOutput {
    pub count: usize,

    /// "name (unit, per item): label".
    pub stats: Vec<String>,

    pub note: &'static str,
}

/// A stat stat_search can search, as the quantity catalog lists it.
pub(crate) struct StatDefinition {
    pub(crate) name: String,
    pub(crate) unit: &'static str,
    pub(crate) label: String,

    /// What one row stands for when the stat has several, e.g. "per ammo type".
    pub(crate) item: Option<&'static str>,

    /// Which way the stat improves; `None` when no build changes it for the better.
    pub(crate) better: Option<Direction>,
}

/// A stat valued on one build per ship, as stat_search ranks it.
pub(crate) struct StatQuantity {
    rank: Rank,
}

impl StatQuantity {
    /// The stat named `stat`, on `build`, or without one on the build that
    /// improves it (the base value for a stat with no better direction).
    /// `qualifier` picks one row as in stat_search.
    pub(crate) fn new(stat: &str, qualifier: Option<String>, build: Option<RankBuild>) -> Result<Self, QueryError> {
        let stat = resolve_stat(stat)?;
        let (build, basis) = match (build, stat.better()) {
            (Some(build), _) => (build, BuildBasis::Given),
            (None, Some(Direction::Lower)) => (RankBuild::Lowest, BuildBasis::Usual),
            (None, Some(Direction::Higher)) => (RankBuild::Highest, BuildBasis::Usual),
            (None, None) => (RankBuild::Base, BuildBasis::Given),
        };
        // A base value of a stat with several rows takes the lowest row.
        let order = match build {
            RankBuild::Highest => RankOrder::Desc,
            RankBuild::Base | RankBuild::Lowest => RankOrder::Asc,
        };

        Ok(Self { rank: Rank { stat, qualifier, order, build, basis, limit: RankLimit(1), condition: None } })
    }

    /// What the value is on, e.g. the best-fit build.
    pub(crate) fn build_label(&self) -> &'static str {
        match (self.rank.build, self.rank.basis) {
            (RankBuild::Base, _) => "base value: stock modules, no captain skills or upgrades",
            (RankBuild::Lowest, BuildBasis::Usual) | (RankBuild::Highest, BuildBasis::Usual) => {
                "best-fit build: the modules, captain skills and upgrades players take for this stat"
            }
            (RankBuild::Lowest, _) => "the build that lowers it most",
            (RankBuild::Highest, _) => "the build that raises it most",
        }
    }
}

/// One ship's value of a [`StatQuantity`].
pub(crate) struct StatReading {
    /// On the quantity's build, rounded as stat_search shows it.
    pub(crate) value: f32,

    /// Stock modules, no captain skills or upgrades.
    pub(crate) base: f32,

    /// Modules, captain skills and upgrades of the build `value` is on.
    pub(crate) build: Vec<String>,

    /// The row `value` comes from when the stat has several.
    pub(crate) row: Option<String>,

    /// The row `base` comes from when the stat has several.
    pub(crate) base_row: Option<String>,
}

/// A stat a condition can test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchStat {
    Card(TtxStat),

    /// Thinnest or thickest nonzero plate of a hull zone above the waterline.
    Armor {
        zone: ArmorZone,
        bound: Bound,
    },

    /// A stat computed from the card.
    Derived(DerivedStat),

    /// Ricochet angle of the selected guns' main battery shells of one ammo type.
    Ricochet {
        ammo: Ammo,
        bound: RicochetBound,
    },

    /// An anti-submarine weapon value of the selected hull.
    Asw(AswStat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArmorZone {
    /// Name segment, e.g. "citadel".
    key: &'static str,

    /// Zone as `zone_from_material_name` names it.
    zone: &'static str,
}

const ARMOR_ZONES: [ArmorZone; 9] = [
    ArmorZone { key: "bow", zone: "Bow" },
    ArmorZone { key: "stern", zone: "Stern" },
    ArmorZone { key: "citadel", zone: "Citadel" },
    ArmorZone { key: "casemate", zone: "Casemate" },
    ArmorZone { key: "superstructure", zone: "Superstructure" },
    ArmorZone { key: "turret", zone: "Turret" },
    ArmorZone { key: "steering_gear", zone: "SteeringGear" },
    ArmorZone { key: "torpedo_protection", zone: "TorpedoProtection" },
    ArmorZone { key: "hull", zone: "Hull" },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    Min,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RicochetBound {
    /// Ricochets start.
    Start,

    /// Every hit that does not overmatch the plate ricochets.
    Always,
}

impl RicochetBound {
    fn angle(self, angles: &RicochetAngles) -> f32 {
        let angle = match self {
            Self::Start => angles.from_deg,
            Self::Always => angles.always_from_deg,
        };

        angle.value()
    }
}

/// Anti-submarine weapon values, base values without skills or upgrades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AswStat {
    DepthChargeDamage,
    DepthChargeSplashRadius,
    DepthChargePerAttack,
    DepthChargeDamagePerAttack,
    DepthChargeCharges,
    DepthChargeReload,
    AirStrikeDamage,
    AirStrikeSplashRadius,
    AirStrikePerStrike,
    AirStrikeDamagePerStrike,
    AirStrikeCharges,
    AirStrikeReload,
    AirStrikeRange,
    BestDamagePerAttack,
}

/// Row names of the two anti-submarine weapons.
const SHIP_DEPTH_CHARGES: &str = "ship-mounted depth charges";
const DEPTH_CHARGE_AIR_STRIKE: &str = "depth charge air strike";

impl AswStat {
    const ALL: [Self; 14] = [
        Self::DepthChargeDamage,
        Self::DepthChargeSplashRadius,
        Self::DepthChargePerAttack,
        Self::DepthChargeDamagePerAttack,
        Self::DepthChargeCharges,
        Self::DepthChargeReload,
        Self::AirStrikeDamage,
        Self::AirStrikeSplashRadius,
        Self::AirStrikePerStrike,
        Self::AirStrikeDamagePerStrike,
        Self::AirStrikeCharges,
        Self::AirStrikeReload,
        Self::AirStrikeRange,
        Self::BestDamagePerAttack,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::DepthChargeDamage => "asw.depth_charge.damage",
            Self::DepthChargeSplashRadius => "asw.depth_charge.splash_radius",
            Self::DepthChargePerAttack => "asw.depth_charge.per_attack",
            Self::DepthChargeDamagePerAttack => "asw.depth_charge.damage_per_attack",
            Self::DepthChargeCharges => "asw.depth_charge.charges",
            Self::DepthChargeReload => "asw.depth_charge.reload",
            Self::AirStrikeDamage => "asw.airstrike.damage",
            Self::AirStrikeSplashRadius => "asw.airstrike.splash_radius",
            Self::AirStrikePerStrike => "asw.airstrike.per_strike",
            Self::AirStrikeDamagePerStrike => "asw.airstrike.damage_per_strike",
            Self::AirStrikeCharges => "asw.airstrike.charges",
            Self::AirStrikeReload => "asw.airstrike.reload",
            Self::AirStrikeRange => "asw.airstrike.range",
            Self::BestDamagePerAttack => "asw.best_damage_per_attack",
        }
    }

    fn unit(self) -> Unit {
        match self {
            Self::DepthChargeDamage
            | Self::DepthChargeDamagePerAttack
            | Self::AirStrikeDamage
            | Self::AirStrikeDamagePerStrike
            | Self::BestDamagePerAttack => Unit::Hp,
            Self::DepthChargeSplashRadius | Self::AirStrikeSplashRadius => Unit::Meters,
            Self::DepthChargePerAttack
            | Self::DepthChargeCharges
            | Self::AirStrikePerStrike
            | Self::AirStrikeCharges => Unit::Count,
            Self::DepthChargeReload | Self::AirStrikeReload => Unit::Seconds,
            Self::AirStrikeRange => Unit::Km,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::DepthChargeDamage => "Damage of one ship-mounted depth charge",
            Self::DepthChargeSplashRadius => {
                "Blast radius of one ship-mounted depth charge, in the game world's meters (the scale of ship models; full damage in the inner part, a third beyond)"
            }
            Self::DepthChargePerAttack => "Ship-mounted depth charges dropped in one attack, every launcher together",
            Self::DepthChargeDamagePerAttack => "Damage of every ship-mounted depth charge of one attack if all hit",
            Self::DepthChargeCharges => "Ship-mounted depth charge attacks before a reload",
            Self::DepthChargeReload => "Reload of one ship-mounted depth charge attack",
            Self::AirStrikeDamage => "Damage of one depth charge of the depth charge air strike",
            Self::AirStrikeSplashRadius => {
                "Blast radius of one air strike depth charge, in the game world's meters (the scale of ship models)"
            }
            Self::AirStrikePerStrike => "Depth charges one air strike drops, every plane of it together",
            Self::AirStrikeDamagePerStrike => "Damage of every depth charge of one air strike if all hit",
            Self::AirStrikeCharges => "Depth charge air strikes before a reload",
            Self::AirStrikeReload => "Reload of one depth charge air strike",
            Self::AirStrikeRange => "Farthest point a depth charge air strike can be called on",
            Self::BestDamagePerAttack => {
                "Damage of one attack with the ship's stronger anti-submarine weapon, ship-mounted depth charges or depth charge air strike, if every depth charge hits; the row names the weapon"
            }
        }
    }

    /// The value of each weapon row `weapons` gives this stat, with the
    /// weapon's name.
    fn readings(self, weapons: &AswWeapons) -> Vec<(&'static str, f32)> {
        let launchers = weapons.launchers.as_ref();
        let strike = weapons.air_strike.as_ref();
        let launcher_damage = launchers.and_then(|launchers| {
            Some(launchers.damage_per_depth_charge?.value() * launchers.depth_charges_per_attack? as f32)
        });
        let strike_damage = strike.and_then(|strike| {
            Some(strike.damage_per_depth_charge?.value() * strike.depth_charges_per_strike()? as f32)
        });
        let from_launchers = |value: Option<f32>| value.map(|value| (SHIP_DEPTH_CHARGES, value));
        let from_strike = |value: Option<f32>| value.map(|value| (DEPTH_CHARGE_AIR_STRIKE, value));

        let reading = match self {
            Self::DepthChargeDamage => {
                from_launchers(launchers.and_then(|launchers| launchers.damage_per_depth_charge).map(|hp| hp.value()))
            }
            Self::DepthChargeSplashRadius => {
                from_launchers(launchers.and_then(|launchers| launchers.splash_radius_m).map(|radius| radius.value()))
            }
            Self::DepthChargePerAttack => from_launchers(
                launchers.and_then(|launchers| launchers.depth_charges_per_attack).map(|count| count as f32),
            ),
            Self::DepthChargeDamagePerAttack => from_launchers(launcher_damage),
            Self::DepthChargeCharges => {
                from_launchers(launchers.and_then(|launchers| launchers.charges).map(|count| count as f32))
            }
            Self::DepthChargeReload => {
                from_launchers(launchers.and_then(|launchers| launchers.reload_s).map(|reload| reload.value()))
            }
            Self::AirStrikeDamage => {
                from_strike(strike.and_then(|strike| strike.damage_per_depth_charge).map(|hp| hp.value()))
            }
            Self::AirStrikeSplashRadius => {
                from_strike(strike.and_then(|strike| strike.splash_radius_m).map(|radius| radius.value()))
            }
            Self::AirStrikePerStrike => {
                from_strike(strike.and_then(|strike| strike.depth_charges_per_strike()).map(|count| count as f32))
            }
            Self::AirStrikeDamagePerStrike => from_strike(strike_damage),
            Self::AirStrikeCharges => from_strike(strike.and_then(|strike| strike.charges).map(|count| count as f32)),
            Self::AirStrikeReload => {
                from_strike(strike.and_then(|strike| strike.reload_s).map(|reload| reload.value()))
            }
            Self::AirStrikeRange => from_strike(strike.and_then(|strike| strike.range_km).map(|range| range.value())),
            Self::BestDamagePerAttack => {
                // The stronger weapon only, so a ship counts once.
                match (from_launchers(launcher_damage), from_strike(strike_damage)) {
                    (Some(ship), Some(air)) => Some(if air.1 > ship.1 { air } else { ship }),
                    (ship, air) => ship.or(air),
                }
            }
        };

        reading.into_iter().collect()
    }
}

impl SearchStat {
    fn all() -> Vec<Self> {
        let mut stats: Vec<Self> = TtxStat::ALL
            .iter()
            .copied()
            .filter(|stat| wows_ship_stats::stat::unit(*stat) != Unit::Flag)
            .map(Self::Card)
            .collect();
        stats.extend(DerivedStat::ALL.iter().copied().map(Self::Derived));
        for zone in ARMOR_ZONES {
            stats.push(Self::Armor { zone, bound: Bound::Min });
            stats.push(Self::Armor { zone, bound: Bound::Max });
        }
        for ammo in [Ammo::Ap, Ammo::Sap] {
            stats.push(Self::Ricochet { ammo, bound: RicochetBound::Start });
            stats.push(Self::Ricochet { ammo, bound: RicochetBound::Always });
        }
        stats.extend(AswStat::ALL.map(Self::Asw));

        stats
    }

    fn name(self) -> String {
        match self {
            Self::Card(stat) => stat.field_key().to_owned(),
            Self::Armor { zone, bound: Bound::Min } => format!("armor.{}.min", zone.key),
            Self::Armor { zone, bound: Bound::Max } => format!("armor.{}.max", zone.key),
            Self::Derived(stat) => stat.field_key().to_owned(),
            Self::Ricochet { ammo, bound: RicochetBound::Start } => {
                format!("artillery.{}.ricochet_start", ammo.label().to_ascii_lowercase())
            }
            Self::Ricochet { ammo, bound: RicochetBound::Always } => {
                format!("artillery.{}.ricochet_always", ammo.label().to_ascii_lowercase())
            }
            Self::Asw(stat) => stat.key().to_owned(),
        }
    }

    fn unit(self) -> Unit {
        match self {
            Self::Card(stat) => wows_ship_stats::stat::unit(stat),
            Self::Armor { .. } => Unit::Millimeters,
            Self::Derived(stat) => stat.unit(),
            Self::Ricochet { .. } => Unit::Degrees,
            Self::Asw(stat) => stat.unit(),
        }
    }

    /// Which way the stat improves. `None` for stats with no better
    /// direction and for those builds do not change.
    fn better(self) -> Option<Direction> {
        match self {
            Self::Card(stat) => wows_ship_stats::stat::direction(stat),
            Self::Derived(stat) => stat.direction(),
            // Plate thickness is the same on every build.
            Self::Armor { .. } => None,
            // A later ricochet suits the shooter and an earlier one the
            // target; neither is better for the ship ranked.
            Self::Ricochet { .. } => None,
            // Read without skills or upgrades.
            Self::Asw(_) => None,
        }
    }

    fn label(self, game: &GameData) -> String {
        const NOMINAL: &str = "nominal plate thickness, not protection; see armor_assessment";

        match self {
            Self::Card(stat @ (TtxStat::ArmorMin | TtxStat::ArmorMax)) => {
                format!("{} ({NOMINAL})", clean(&stat_display_label(stat, game.provider())))
            }
            Self::Card(TtxStat::ConsumableMaxCapacity) => "Capacity of a time-based consumable, one switched on and off that draws on a pool of seconds of use per battle instead of charges (e.g. 20 de Julio's Main Battery Reload Booster); unlimited ranks above every number. Only ships with a time-based consumable have a row, so a condition >= 0 lists them all".to_owned(),
            Self::Card(TtxStat::SubmergedSpeed) => "Submerged speed: a submarine's top speed fully underwater, at maximum depth. This is how fast a submarine moves underwater; mobility.speed is its speed on the surface".to_owned(),
            Self::Card(TtxStat::PeriscopeSpeed) => "A submarine's top speed at periscope depth, just below the surface; equal to its surface speed (mobility.speed) on almost every submarine, so not its underwater speed (underwater_mobility.speed)".to_owned(),
            Self::Card(stat) => clean(&stat_display_label(stat, game.provider())),
            Self::Armor { zone, bound: Bound::Min } => {
                format!("Thinnest {} plate above the waterline ({NOMINAL})", zone.key.replace('_', " "))
            }
            Self::Armor { zone, bound: Bound::Max } => {
                format!("Thickest {} plate above the waterline ({NOMINAL})", zone.key.replace('_', " "))
            }
            Self::Derived(stat) => stat.label().to_owned(),
            Self::Ricochet { ammo, bound: RicochetBound::Start } => format!(
                "Main battery {} shell impact angle from the plate normal where ricochets start, from game data",
                ammo.label()
            ),
            Self::Ricochet { ammo, bound: RicochetBound::Always } => format!(
                "Main battery {} shell impact angle from the plate normal where every hit ricochets, from game data",
                ammo.label()
            ),
            Self::Asw(stat) => format!("{}; base value, without skills or upgrades", stat.label()),
        }
    }

    /// What a ranking by the stat does not tell.
    fn caution(self) -> Option<&'static str> {
        match self {
            Self::Armor { .. } | Self::Card(TtxStat::ArmorMin | TtxStat::ArmorMax) => Some(
                "Nominal plate thickness ranks plates, not protection: the belt, bulge and decks in front of a zone decide what reaches it. For how well ships are protected, use armor_assessment or ship_comparison (citadel and penetration rates).",
            ),
            Self::Card(_) | Self::Derived(_) | Self::Ricochet { .. } | Self::Asw(_) => None,
        }
    }

    /// What one row of a multi-row stat stands for.
    fn item(self) -> Option<&'static str> {
        let key = match self {
            Self::Asw(AswStat::BestDamagePerAttack) => return Some("per weapon, the stronger one"),
            Self::Card(TtxStat::SecondaryRange) | Self::Armor { .. } | Self::Ricochet { .. } | Self::Asw(_) => {
                return None;
            }
            Self::Card(stat) => stat.field_key(),
            Self::Derived(stat) => stat.field_key(),
        };

        if key.starts_with("artillery.shells.") || key == "fires_per_minute.main_battery" {
            Some("per ammo type")
        } else if key.starts_with("secondary.") || key == "fires_per_minute.secondary_group" {
            Some("per secondary mount")
        } else if key.starts_with("aircraft.") || key == "fires_per_minute.aircraft" {
            Some("per aircraft, by GameParams name")
        } else if key.starts_with("torpedoes.launchers.") {
            Some("per launcher")
        } else if key.starts_with("torpedoes.torpedoes.") {
            Some("per torpedo")
        } else if key.starts_with("consumable.") {
            Some("per consumable")
        } else {
            None
        }
    }

    /// Captain skills, upgrades and modules never change it.
    fn is_fixed(self) -> bool {
        matches!(self, Self::Armor { .. })
    }

    /// Modules do not change it. Ranked as ship_consumables plans consumable
    /// stats, except that every extra charge counts.
    fn is_consumable(self) -> bool {
        matches!(self, Self::Card(stat) if stat.field_key().starts_with("consumable."))
    }

    /// The depth of a top speed stat; `None` for other stats.
    fn speed_depth(self) -> Option<SpeedDepth> {
        match self {
            Self::Card(TtxStat::Speed) => Some(SpeedDepth::Surface),
            Self::Card(TtxStat::PeriscopeSpeed) => Some(SpeedDepth::Periscope),
            Self::Card(TtxStat::SubmergedSpeed) => Some(SpeedDepth::Submerged),
            _ => None,
        }
    }

    /// The battery a shell stat describes, named with each value so main and
    /// secondary battery rows are told apart.
    fn shell_battery(self) -> Option<Battery> {
        let key = match self {
            Self::Ricochet { .. } => return Some(Battery::Main),
            Self::Armor { .. } | Self::Asw(_) => return None,
            Self::Card(stat) => stat.field_key(),
            Self::Derived(stat) => stat.field_key(),
        };

        if key.starts_with("artillery.shells.") {
            Some(Battery::Main)
        } else if key.starts_with("secondary.shells.") {
            Some(Battery::Secondary)
        } else {
            None
        }
    }
}

/// A condition resolved against the stat list.
#[derive(Debug, Clone)]
struct Condition {
    stat: SearchStat,
    op: Comparison,
    threshold: f32,
    qualifier: Option<String>,
}

impl Condition {
    fn describe(&self) -> String {
        let qualifier = self.qualifier.as_deref().map(|qualifier| format!(" ({qualifier})")).unwrap_or_default();

        format!(
            "{}{qualifier} {} {}",
            self.stat.name(),
            self.op.symbol(),
            format_value(self.threshold, self.stat.unit())
        )
    }

    fn holds(&self, readings: &[Reading]) -> bool {
        readings.iter().any(|reading| self.op.holds(reading.value, self.threshold))
    }

    /// The reading nearest to meeting the condition.
    fn best<'a>(&self, readings: &'a [Reading]) -> Option<&'a Reading> {
        if let Some(reading) = readings.iter().find(|reading| self.op.holds(reading.value, self.threshold)) {
            return Some(reading);
        }

        match self.op.direction() {
            Some(Direction::Lower) => readings.iter().min_by(|left, right| left.value.total_cmp(&right.value)),
            Some(Direction::Higher) => readings.iter().max_by(|left, right| left.value.total_cmp(&right.value)),
            None => readings.first(),
        }
    }

    /// Whether some row of `candidate` is nearer to meeting the condition than
    /// the same row of `base`, or is new. Rows pair by qualifier, so a row that
    /// only meets the threshold with several items counts for each of them.
    fn improves(&self, base: &[Reading], candidate: &[Reading]) -> bool {
        let Some(direction) = self.op.direction() else {
            return base != candidate;
        };

        candidate.iter().any(|after| {
            let Some(before) = base.iter().find(|before| before.qualifier == after.qualifier) else {
                return true;
            };
            match direction {
                Direction::Lower => after.value < before.value,
                Direction::Higher => after.value > before.value,
            }
        })
    }
}

/// One row of a stat in one configuration.
#[derive(Debug, Clone, PartialEq)]
struct Reading {
    qualifier: Option<String>,
    value: f32,
}

/// The stats of one ship configuration that conditions read.
struct View {
    rows: Vec<StatRow>,
    derived: Vec<DerivedRow>,

    /// Main battery shells of the selected guns.
    ricochet: Vec<(Ammo, RicochetAngles)>,

    /// Anti-submarine weapons of the selected hull, when a condition reads them.
    asw: Option<AswWeapons>,
}

impl View {
    fn readings(
        &self,
        condition: &Condition,
        armor: &BTreeMap<&'static str, ThicknessSpan>,
        game: &GameData,
    ) -> Vec<Reading> {
        match condition.stat {
            SearchStat::Card(stat) => card_values(game, &self.rows, stat, condition.qualifier.as_deref())
                .map(|(row, value)| Reading { qualifier: row.qualifier.clone(), value })
                .collect(),
            SearchStat::Armor { zone, bound } => {
                armor_value(armor, zone, bound).map(|value| Reading { qualifier: None, value }).into_iter().collect()
            }
            SearchStat::Derived(stat) => derived_rows(game, &self.derived, stat, condition.qualifier.as_deref())
                .map(|row| Reading { qualifier: row.qualifier.clone(), value: row.value.as_f32() })
                .collect(),
            SearchStat::Ricochet { ammo, bound } => {
                let angles =
                    self.ricochet.iter().filter(|(shell_ammo, _)| *shell_ammo == ammo).map(|(_, angles)| angles);
                let mut readings: Vec<Reading> = Vec::new();
                for angles in angles {
                    let reading = Reading { qualifier: None, value: bound.angle(angles) };
                    if !readings.contains(&reading) {
                        readings.push(reading);
                    }
                }

                readings
            }
            SearchStat::Asw(stat) => self
                .asw
                .as_ref()
                .map(|weapons| stat.readings(weapons))
                .unwrap_or_default()
                .into_iter()
                .filter(|(weapon, _)| qualifier_matches(game, Some(weapon), condition.qualifier.as_deref()))
                .map(|(weapon, value)| Reading { qualifier: Some(weapon.to_owned()), value })
                .collect(),
        }
    }
}

/// A module, upgrade or captain skill a ship can take.
#[derive(Clone, Copy)]
enum Item<'a> {
    Module(&'a Module),
    Upgrade(&'a Upgrade),
    Skill { skill: &'a CrewSkill, points: SkillPointCost },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ItemGroup {
    Module,
    Upgrade,
    Skill,
}

/// Sort key of an item: group, then module kind, upgrade slot or skill
/// point cost, then param name.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ItemOrder<'a> {
    group: ItemGroup,
    rank: u8,
    name: &'a str,
}

impl Item<'_> {
    /// Modules, then upgrades by slot, then skills by point cost.
    fn order(&self) -> ItemOrder<'_> {
        match self {
            Item::Module(module) => {
                ItemOrder { group: ItemGroup::Module, rank: module.kind as u8, name: module.param.as_str() }
            }
            Item::Upgrade(upgrade) => {
                ItemOrder { group: ItemGroup::Upgrade, rank: upgrade.slot, name: upgrade.param.name() }
            }
            Item::Skill { skill, points } => {
                ItemOrder { group: ItemGroup::Skill, rank: points.get(), name: skill.internal_name().as_str() }
            }
        }
    }

    fn same(&self, other: &Item<'_>) -> bool {
        self.order() == other.order()
    }

    fn describe(&self, game: &GameData) -> String {
        match self {
            Item::Module(module) => format!("{}: {}", module.kind.label(), module.name),
            Item::Upgrade(upgrade) if upgrade.is_unique() => {
                format!("unique upgrade: {} (slot {})", upgrade.name, upgrade.port_slot())
            }
            Item::Upgrade(upgrade) => format!("upgrade: {} (slot {})", upgrade.name, upgrade.port_slot()),
            Item::Skill { skill, points } => format!("skill: {} ({} points)", game.skill_name(skill), points.get()),
        }
    }
}

/// The result of searching one ship.
enum Outcome {
    Stock { values: Vec<String> },
    Requires { requires: Vec<String>, values: Vec<String>, unchecked: Option<String> },
    Impossible,
    Undetermined { reason: String },
}

/// Configurations whose stats could not be computed during one ship's search.
#[derive(Default)]
struct Failures {
    count: usize,
    first: Option<String>,
}

impl Failures {
    fn describe(&self) -> Option<String> {
        let first = self.first.as_deref()?;
        Some(format!("{} stat computations failed, the first because: {first}", self.count))
    }
}

/// Everything one ship can take, and the stock configuration's stats.
struct ShipSearch<'a> {
    game: &'a GameData,
    ship: &'a Ship,
    conditions: &'a [Condition],
    stock_selection: ShipUpgradeSelection,
    armor: BTreeMap<&'static str, ThicknessSpan>,
    modules: Vec<Module>,
    has_skill_grid: bool,

    /// A configuration that fails to compute is skipped instead of failing
    /// the ship; these are reported with the outcome.
    failures: RefCell<Failures>,

    /// Per condition, whether any configuration read had a row for it.
    rows_seen: RefCell<Vec<bool>>,

    /// Computed on first use.
    stock_ranges: OnceCell<StockRanges>,

    /// Which gun mounts bear at each bearing, per module combination read,
    /// when a condition counts only bearing mounts.
    coverages: RefCell<Vec<(ShipUpgradeSelection, Option<Rc<MountCoverage>>)>>,
}

impl GameData {
    /// Every stat `stat_search` accepts, with unit and label.
    pub fn stat_names(&self, _args: &StatNamesArgs) -> StatNamesOutput {
        let stats: Vec<String> = SearchStat::all()
            .into_iter()
            .map(|stat| {
                let item = stat.item().map(|item| format!(", {item}")).unwrap_or_default();
                format!("{} ({}{item}): {}", stat.name(), stat.unit().name(), stat.label(self))
            })
            .collect();

        StatNamesOutput {
            count: stats.len(),
            stats,
            note: "Compare values in the listed unit. A stat with \"per\" items has one row per item; a condition holds when any row does, or the row named by qualifier. Armor stats count plates above the waterline in the ship's one armor table; modules do not change them.",
        }
    }

    /// Ships whose stats meet every condition, stock or with the fewest
    /// modules, upgrades and captain skills, or ships in order of one stat on
    /// their best-fit builds.
    pub fn stat_search(&self, args: &StatSearchArgs) -> Result<StatSearchOutput, QueryError> {
        const CONDITION_RULE: &str = "A ship qualifies with stock modules, or with the fewest modules, upgrades (one per slot) and captain skills (21 points, each skill needing one on every cheaper tier) that make every condition hold. Conditional skills count with their triggers unmet, as on the always-on build; health-scaled effects use full health.";

        let mut conditions: Vec<Condition> =
            args.conditions.iter().map(resolve_condition).collect::<Result<_, QueryError>>()?;
        let mut rank = args.rank.as_ref().map(resolve_rank).transpose()?;
        for condition in &mut conditions {
            condition.qualifier = self.consumable_qualifier(condition.stat, condition.qualifier.take());
        }
        if let Some(rank) = &mut rank {
            rank.qualifier = self.consumable_qualifier(rank.stat, rank.qualifier.take());
        }
        if conditions.is_empty() && rank.is_none() {
            return Err(QueryError::InvalidArguments { reason: "give at least one condition, or rank" });
        }

        let filter = ShipFilter {
            tiers: TierRange::from_bounds(args.min_tier, args.max_tier),
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: args.include_event_variants.unwrap_or(false),
            exclude_test: args.exclude_test_ships.unwrap_or(false),
            include_replicas: args.include_replicas.unwrap_or(false),
        };
        let mut ships: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| listing_order(left, right));
        let considered = ShipCounts::from_ships(ships.iter().copied());

        let mut undetermined = Vec::new();
        let ConditionSearch { hits, searched_without_skills, unmatched_qualifiers } = if conditions.is_empty() {
            ConditionSearch::default()
        } else {
            self.condition_search(&ships, &conditions, &mut undetermined)
        };
        let qualifying = (!conditions.is_empty()).then(|| qualifying_counts(&hits));
        let shares = qualifying.as_ref().map(|qualifying| Shares::new(&qualifying.by_class, &considered));

        let (ranking, listed, omitted) = match &rank {
            Some(rank) => {
                let candidates: Vec<(&Ship, Option<ConditionMatch>)> = if conditions.is_empty() {
                    ships.iter().map(|ship| (*ship, None)).collect()
                } else {
                    hits.into_iter().map(|(ship, meets)| (ship, Some(meets))).collect()
                };
                (Some(self.rank_ships(candidates, rank, &mut undetermined)), None, None)
            }
            None => {
                const HIT_LIMIT: usize = 40;

                let total = hits.len();
                let listed: Vec<StatSearchHit> = hits
                    .into_iter()
                    .take(HIT_LIMIT)
                    .map(|(ship, meets)| StatSearchHit { ship: ship.listing_label(), meets })
                    .collect();
                let omitted = (total > listed.len()).then(|| total - listed.len());
                (None, Some(listed), omitted)
            }
        };

        Ok(StatSearchOutput {
            conditions: conditions.iter().map(Condition::describe).collect(),
            rule: (!conditions.is_empty()).then_some(CONDITION_RULE),
            considered,
            qualifying,
            shares,
            ranking,
            ships: listed,
            omitted,
            undetermined,
            searched_without_skills,
            unmatched_qualifiers,
        })
    }

    /// `qualifier` of a consumable stat as the consumable type rows are keyed
    /// by, when it names one consumable as players write it ("Surveillance
    /// Radar", "ATWC"); otherwise as given, such as a type itself.
    fn consumable_qualifier(&self, stat: SearchStat, qualifier: Option<String>) -> Option<String> {
        let qualifier = qualifier?;
        if !stat.name().starts_with("consumable.") {
            return Some(qualifier);
        }

        match self.resolve_consumable(&qualifier, &mut Interpretations::default()) {
            Ok(types) if types.len() == 1 => types.into_iter().next(),
            _ => Some(qualifier),
        }
    }

    /// Ships that meet every condition, fewest requirements first, then in
    /// [`listing_order`].
    fn condition_search<'s>(
        &'s self,
        ships: &[&'s Ship],
        conditions: &[Condition],
        undetermined: &mut Vec<UndeterminedShip>,
    ) -> ConditionSearch<'s> {
        let mut hits: Vec<(&Ship, ConditionMatch)> = Vec::new();
        let mut searched_without_skills = Vec::new();
        let mut qualifier_matched = vec![false; conditions.len()];
        for &ship in ships {
            let search = ShipSearch::new(self, ship, conditions);
            let outcome = match search.stock_readings() {
                Ok(stock) => search.run(&stock),
                Err(error) => Outcome::Undetermined { reason: format!("stock stats could not be computed: {error}") },
            };
            for (matched, seen) in qualifier_matched.iter_mut().zip(search.rows_seen.borrow().iter()) {
                *matched |= seen;
            }
            if !search.has_skill_grid && !matches!(outcome, Outcome::Stock { .. }) {
                searched_without_skills.push(ship.listing_label());
            }
            match outcome {
                Outcome::Stock { values } => {
                    hits.push((ship, ConditionMatch { stock: true, requires: Vec::new(), values, unchecked: None }))
                }
                Outcome::Requires { requires, values, unchecked } => {
                    hits.push((ship, ConditionMatch { stock: false, requires, values, unchecked }))
                }
                Outcome::Impossible => {}
                Outcome::Undetermined { reason } => {
                    undetermined.push(UndeterminedShip { ship: ship.listing_label(), reason })
                }
            }
        }

        let unmatched_qualifiers = if ships.is_empty() {
            Vec::new()
        } else {
            conditions
                .iter()
                .zip(&qualifier_matched)
                .filter(|(condition, matched)| condition.qualifier.is_some() && !**matched)
                .map(|(condition, _)| condition.describe())
                .collect()
        };

        hits.sort_by(|(left_ship, left), (right_ship, right)| {
            left.requires.len().cmp(&right.requires.len()).then_with(|| listing_order(left_ship, right_ship))
        });

        ConditionSearch { hits, searched_without_skills, unmatched_qualifiers }
    }

    /// `candidates` in rank order, each on the build `rank` names for the
    /// rank stat, then in [`listing_order`].
    fn rank_ships(
        &self,
        candidates: Vec<(&Ship, Option<ConditionMatch>)>,
        rank: &Rank,
        undetermined: &mut Vec<UndeterminedShip>,
    ) -> Ranking {
        let row_names = RowNames::default();
        let mut entries: Vec<RankEntry<'_>> = Vec::new();
        let mut lacking: Vec<&Ship> = Vec::new();
        for (ship, meets) in candidates {
            match self.rank_builds(ship, rank, &row_names) {
                Ok(Some(RankedBuilds { builds, fires_by_armament, condition })) => {
                    entries.push(RankEntry { ship, builds, fires_by_armament, condition, meets })
                }
                Ok(None) => lacking.push(ship),
                Err(error) => undetermined.push(UndeterminedShip {
                    ship: ship.listing_label(),
                    reason: format!("{} could not be computed: {error}", rank.stat.name()),
                }),
            }
        }

        entries.sort_by(|left, right| {
            let (left_value, right_value) = (rank.ranked(&left.builds).value, rank.ranked(&right.builds).value);
            let order = match rank.order {
                RankOrder::Asc => left_value.total_cmp(&right_value),
                RankOrder::Desc => right_value.total_cmp(&left_value),
            };
            order.then_with(|| listing_order(left.ship, right.ship))
        });

        let limit = rank.limit.0;
        let printed = |index: usize| entries.get(index).map(|entry| shown(rank.ranked(&entry.builds).value));
        let tied_beyond_limit = (limit..entries.len())
            .filter(|&index| printed(index) == printed(limit - 1))
            .map(|index| entries[index].ship.listing_label())
            .collect();

        // Competition ranks: a ship tied with the one before it shares its rank.
        let mut placements: Vec<(usize, bool)> = Vec::new();
        for index in 0..entries.len().min(limit) {
            let value = printed(index);
            let after_tie = index > 0 && printed(index - 1) == value;
            let position = match placements.last() {
                Some(&(previous, _)) if after_tie => previous,
                _ => index + 1,
            };
            placements.push((position, after_tie || printed(index + 1) == value));
        }

        let ranked = entries.len();
        let lowest_ranked_tier = entries.iter().map(|entry| entry.ship.tier).min();
        let unit = rank.stat.unit();
        let ships: Vec<RankedShip> = entries
            .into_iter()
            .zip(placements)
            .map(|(RankEntry { ship, builds, fires_by_armament, condition, meets }, (position, tied))| {
                let value = rank.ranked(&builds);
                let (build, tier_fillers) = match rank.build {
                    RankBuild::Base => (Vec::new(), Vec::new()),
                    RankBuild::Lowest | RankBuild::Highest => (builds.best.assumed, builds.best.tier_fillers),
                };

                RankedShip {
                    rank: position,
                    ship: ship.listing_label(),
                    value: format_value(value.value, unit),
                    base: format_value(builds.base.value, unit),
                    build,
                    tier_fillers,
                    tied,
                    meets,
                    fires_by_armament,
                    condition,
                    row: value.row.and_then(|row| row_names.label(self, ship, rank.stat, row)),
                }
            })
            .collect();
        let omitted = (ranked > ships.len()).then(|| ranked - ships.len());

        Ranking {
            stat: rank.describe(),
            rule: rank.rule(),
            caution: rank.stat.caution(),
            ranked,
            ships,
            tied_beyond_limit,
            omitted,
            lacking: LackingShips::new(lacking),
            note: matches!(rank.stat, SearchStat::Asw(_)).then(|| self.submarine_note(lowest_ranked_tier)),
        }
    }

    /// Which submarines exist and from which tier the searched ships carry
    /// anti-submarine weapons, for asw.* rankings.
    fn submarine_note(&self, lowest_ranked_tier: Option<Tier>) -> String {
        let filter = ShipFilter { class: Some(ShipClass::Submarine), exclude_test: true, ..ShipFilter::default() };
        let tiers: BTreeSet<Tier> =
            self.ships.iter().filter(|ship| filter.matches(ship)).map(|ship| ship.tier).collect();
        let listed: Vec<String> = tiers.iter().map(|tier| tier.0.to_string()).collect();
        let submarines = if listed.is_empty() {
            "No released submarines are in this game data.".to_owned()
        } else {
            format!("Released submarines are of tiers {}.", listed.join(", "))
        };
        let armed = match lowest_ranked_tier {
            Some(tier) => {
                format!(" Of the searched ships, those with anti-submarine weapons start at tier {}.", tier.0)
            }
            None => String::new(),
        };

        format!(
            "{submarines}{armed} Most battleships and cruisers of tiers V to X received anti-submarine weapons when submarines entered Random Battles in update 0.10.9 (https://blog.worldofwarships.com/blog/225); few ships below tier V have them."
        )
    }

    /// Base and best-fit build value of the rank stat, `None` when the ship
    /// lacks it.
    fn rank_builds(&self, ship: &Ship, rank: &Rank, row_names: &RowNames) -> Result<Option<RankedBuilds>, QueryError> {
        if let Some(condition) = &rank.condition {
            return self.conditional_speed_builds(ship, condition);
        }

        let plain = |builds: Option<Builds<Amount>>| {
            builds.map(|builds| RankedBuilds { builds, fires_by_armament: None, condition: None })
        };
        match rank.stat {
            SearchStat::Card(stat) => {
                let planner = Planner::new(self, ship);
                let extract = |outcome: &builds::Outcome| {
                    let rows = outcome.stats.rows();
                    let caliber = main_battery_caliber(rank.stat, &rows);
                    let values = card_values(self, &rows, stat, rank.qualifier.as_deref())
                        .map(|(row, value)| Amount { value, row: row_names.id(row.qualifier.as_deref(), caliber) });
                    extreme(rank.direction(), values)
                };

                planned_builds(&planner, rank, &extract).map(plain)
            }
            SearchStat::Derived(stat) => {
                let stock_ranges = self.stock_ranges(ship)?;
                let planner = Planner::new(self, ship);
                let selections: Vec<&ShipUpgradeSelection> = planner.selections().collect();
                let coverages = if stat.reads_mount_bearings() || stat.is_fire_starts() {
                    self.mount_coverages(ship, planner.selections())?
                } else {
                    Vec::new()
                };
                let coverage = |selection: usize| coverages.get(selection).and_then(|coverage| coverage.as_deref());
                let extract = |outcome: &builds::Outcome| {
                    let rows = Derivation {
                        ship: &ship.param,
                        selection: selections[outcome.selection],
                        stats: &outcome.stats,
                        modifiers: &outcome.bundle,
                        merges_fire_zones: outcome.merges_fire_zones,
                        stock_ranges,
                        provider: self.provider(),
                        coverage: if stat.reads_mount_bearings() { coverage(outcome.selection) } else { None },
                    }
                    .rows();
                    let caliber = (rank.stat.shell_battery() == Some(Battery::Main))
                        .then(|| main_battery_caliber(rank.stat, &outcome.stats.rows()))
                        .flatten();
                    let values = derived_rows(self, &rows, stat, rank.qualifier.as_deref()).map(|row| Amount {
                        value: row.value.as_f32(),
                        row: row_names.id(row.qualifier.as_deref(), caliber),
                    });
                    extreme(rank.direction(), values)
                };

                let fire_sources = |outcome: &builds::Outcome| {
                    FireInputs {
                        ship: &ship.param,
                        selection: selections[outcome.selection],
                        stats: &outcome.stats,
                        modifiers: &outcome.bundle,
                        provider: self.provider(),
                    }
                    .sources()
                };

                let Some(plan) = rank_plan(&planner, rank, &extract)? else {
                    // One-target fire starts and torpedo rates need the mount facings.
                    if stat.reads_mount_bearings() && coverages.iter().all(Option::is_none) {
                        let bare = planner.bare(0)?;
                        let armaments = stat.bearing_armaments();
                        let sources = fire_sources(&bare);
                        let fires = sources.iter().any(|source| armaments.contains(&source.armament));
                        // Submarine tubes never get one-target figures, and a
                        // ship whose ceiling row is absent lacks the stat.
                        let submarine =
                            ship.param.species().and_then(|species| species.known()) == Some(&Species::Submarine);
                        let launches = !submarine
                            && stat.launcher_ceiling().is_some_and(|ceiling| {
                                Derivation {
                                    ship: &ship.param,
                                    selection: selections[0],
                                    stats: &bare.stats,
                                    modifiers: &bare.bundle,
                                    merges_fire_zones: bare.merges_fire_zones,
                                    stock_ranges,
                                    provider: self.provider(),
                                    coverage: None,
                                }
                                .rows()
                                .iter()
                                .any(|row| row.stat == ceiling)
                            });
                        if fires || launches {
                            return Err(QueryError::HullFacingsUnavailable { ship: ship.name.clone() });
                        }
                    }
                    return Ok(None);
                };
                if !stat.is_fire_starts() {
                    return planner.best(&plan).map(plain);
                }

                let Some(BestFit { builds, outcome: best }) = planner.best_fit(&plan)? else {
                    return Ok(None);
                };
                let ranked = match rank.build {
                    RankBuild::Base => planner.bare(plan.base_selection)?,
                    RankBuild::Lowest | RankBuild::Highest => best,
                };
                let sources = fire_sources(&ranked);
                let fires_by_armament =
                    coverage(ranked.selection).and_then(|coverage| fire_breakdown(&sources, coverage)).map(rounded);

                Ok(Some(RankedBuilds { builds, fires_by_armament, condition: None }))
            }
            SearchStat::Armor { zone, bound } => {
                let value = armor_value(&zone_spans(&plates(ship)), zone, bound);
                Ok(plain(value.map(|value| {
                    let amount = Amount { value, row: None };
                    let best = BuildValue { value: amount, assumed: Vec::new(), tier_fillers: Vec::new() };
                    Builds { base: amount, best, alternatives: Vec::new() }
                })))
            }
            SearchStat::Ricochet { ammo, bound } => {
                let planner = Planner::new(self, ship);
                let selections: Vec<&ShipUpgradeSelection> = planner.selections().collect();
                let extract = |outcome: &builds::Outcome| {
                    let shells = main_battery_ricochet(self, ship, selections[outcome.selection]);
                    let angles = shells.iter().filter(|(shell_ammo, _)| *shell_ammo == ammo);
                    let values = angles.map(|(_, angles)| Amount { value: bound.angle(angles), row: None });
                    extreme(rank.direction(), values)
                };

                planned_builds(&planner, rank, &extract).map(plain)
            }
            SearchStat::Asw(stat) => {
                let planner = Planner::new(self, ship);
                let selections: Vec<&ShipUpgradeSelection> = planner.selections().collect();
                let extract = |outcome: &builds::Outcome| {
                    let weapons = self.asw_weapons(ship, selections[outcome.selection].hull.as_deref());
                    let values = stat
                        .readings(&weapons)
                        .into_iter()
                        .filter(|(weapon, _)| qualifier_matches(self, Some(weapon), rank.qualifier.as_deref()))
                        .map(|(weapon, value)| Amount { value, row: row_names.id(Some(weapon), None) });
                    extreme(rank.direction(), values)
                };

                planned_builds(&planner, rank, &extract).map(plain)
            }
        }
    }

    /// Base and top speed with `condition` held, or the always-on best-fit
    /// build when the ship lacks it. `None` when the ship lacks the speed.
    fn conditional_speed_builds(
        &self,
        ship: &Ship,
        condition: &RankCondition,
    ) -> Result<Option<RankedBuilds>, QueryError> {
        let planner = Planner::new(self, ship);
        let Some(stat) = self.speed_build(&planner, condition.depth)? else {
            return Ok(None);
        };

        let builds = stat.builds;
        let amount = |value: Knots| Amount { value: value.value(), row: None };
        let held = match &condition.kind {
            SpeedCondition::Fastest => self.fastest_speed(&planner, condition.depth)?,
            SpeedCondition::Named(name) => {
                builds.alternatives.into_iter().find(|alternative| names_condition(&alternative.label, name))
            }
        };
        let (best, label) = match held {
            Some(alternative) => (
                BuildValue {
                    value: amount(alternative.value),
                    assumed: alternative.assumed,
                    tier_fillers: alternative.tier_fillers,
                },
                Some(alternative.label),
            ),
            None => (
                BuildValue {
                    value: amount(builds.best.value),
                    assumed: builds.best.assumed,
                    tier_fillers: builds.best.tier_fillers,
                },
                None,
            ),
        };

        Ok(Some(RankedBuilds {
            builds: Builds { base: amount(builds.base), best, alternatives: Vec::new() },
            fires_by_armament: None,
            condition: label,
        }))
    }

    /// `ship`'s value of `stat`, from the same builds stat_search ranks by.
    /// `None` when the ship lacks the stat.
    pub(crate) fn stat_reading(&self, ship: &Ship, stat: &StatQuantity) -> Result<Option<StatReading>, QueryError> {
        let row_names = RowNames::default();
        let Some(RankedBuilds { builds, .. }) = self.rank_builds(ship, &stat.rank, &row_names)? else {
            return Ok(None);
        };

        let ranked = stat.rank.ranked(&builds);
        let build = match stat.rank.build {
            RankBuild::Base => Vec::new(),
            RankBuild::Lowest | RankBuild::Highest => builds.best.assumed,
        };

        Ok(Some(StatReading {
            value: shown(ranked.value),
            base: shown(builds.base.value),
            build,
            row: ranked.row.and_then(|row| row_names.label(self, ship, stat.rank.stat, row)),
            base_row: builds.base.row.and_then(|row| row_names.label(self, ship, stat.rank.stat, row)),
        }))
    }
}

impl<'a> ShipSearch<'a> {
    fn new(game: &'a GameData, ship: &'a Ship, conditions: &'a [Condition]) -> Self {
        Self {
            game,
            ship,
            conditions,
            stock_selection: ShipUpgradeSelection::stock(&ship.param),
            armor: zone_spans(&plates(ship)),
            modules: game.ship_modules(ship).into_iter().filter(|module| !module.stock).collect(),
            has_skill_grid: skill_grid(&game.version(), ship.class.species()).is_some(),
            failures: RefCell::default(),
            rows_seen: RefCell::new(vec![false; conditions.len()]),
            stock_ranges: OnceCell::new(),
            coverages: RefCell::default(),
        }
    }

    fn stock_readings(&self) -> Result<Vec<Vec<Reading>>, QueryError> {
        Ok(self.readings(&self.view(&[])?))
    }

    fn run(&self, stock: &[Vec<Reading>]) -> Outcome {
        if self.all_hold(stock) {
            return Outcome::Stock { values: self.values(stock) };
        }

        let failing: Vec<usize> =
            (0..self.conditions.len()).filter(|&index| !self.conditions[index].holds(&stock[index])).collect();
        if failing.iter().any(|&index| self.conditions[index].stat.is_fixed()) {
            return Outcome::Impossible;
        }

        let outcome = match self.choices(&failing, stock) {
            Some(choices) => self.enumerate(&failing, &choices),
            None => Outcome::Impossible,
        };
        match (outcome, self.failures.borrow().describe()) {
            (Outcome::Impossible, Some(reason)) => Outcome::Undetermined { reason },
            (Outcome::Requires { requires, values, .. }, unchecked @ Some(_)) => {
                Outcome::Requires { requires, values, unchecked }
            }
            (outcome, _) => outcome,
        }
    }

    /// The readings of a configuration, or `None` after recording why its
    /// stats could not be computed.
    fn try_readings(&self, items: &[Item<'_>]) -> Option<Vec<Vec<Reading>>> {
        self.recorded(self.view(items)).map(|view| self.readings(&view))
    }

    fn recorded<T>(&self, result: Result<T, QueryError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                let mut failures = self.failures.borrow_mut();
                failures.count += 1;
                failures.first.get_or_insert_with(|| error.to_string());
                None
            }
        }
    }

    /// Items that move a failing condition toward its threshold, each with
    /// the conditions it moves, plus hulls that mount helpful guns or
    /// torpedoes the stock hull cannot. `None` when some failing condition
    /// cannot be met even by stacking every such item.
    fn choices(&self, failing: &[usize], stock: &[Vec<Reading>]) -> Option<Vec<Choice<'_>>> {
        let mut choices: Vec<Choice<'_>> = Vec::new();

        // An item that restores a condition another item breaks is needed even
        // when that condition holds on stock.
        let every: Vec<usize> = (0..self.conditions.len()).collect();

        // Each module is tried on the stock configuration, with a hull that
        // mounts it when the stock hull does not.
        let mut bases: Vec<(Vec<Item<'_>>, Vec<Vec<Reading>>)> = vec![(Vec::new(), stock.to_vec())];
        for module in &self.modules {
            // A hull, or a module on a hull other than the stock one, also
            // needs guns and torpedoes that hull takes.
            let hull = match module.kind {
                ModuleKind::Hull => Some(module),
                _ if !self.mounts_on_stock_hull(module) => self.enabling_hulls(module).first().copied(),
                _ => None,
            };
            let mut base = vec![Item::Module(module)];
            if let Some(hull) = hull {
                if !std::ptr::eq(hull, module) {
                    base.push(Item::Module(hull));
                }
                for kind in [ModuleKind::Guns, ModuleKind::Torpedoes] {
                    if let Some(companion) = self.companions(hull, module.kind).find(|companion| companion.kind == kind)
                    {
                        base.push(Item::Module(companion));
                    }
                }
            }
            let Some(readings) = self.try_readings(&base) else {
                continue;
            };
            let helps = self.helped(&every, stock, &readings);
            if helps.is_empty() {
                continue;
            }

            // A helpful module also listed as an enabler is kept as helpful.
            choices.retain(|choice| !choice.item.same(&Item::Module(module)));
            choices.push(Choice { item: Item::Module(module), helps, enables: false });
            let enablers: Vec<&Module> = if module.kind == ModuleKind::Hull {
                self.companions(module, ModuleKind::Hull).collect()
            } else if !self.mounts_on_stock_hull(module) {
                self.enabling_hulls(module)
                    .into_iter()
                    .flat_map(|hull| std::iter::once(hull).chain(self.companions(hull, module.kind)))
                    .collect()
            } else {
                Vec::new()
            };
            for enabler in enablers {
                if !choices.iter().any(|choice| choice.item.same(&Item::Module(enabler))) {
                    choices.push(Choice { item: Item::Module(enabler), helps: Vec::new(), enables: true });
                }
            }
            bases.push((base, readings));
        }

        let loadout_items = self.loadout_items();
        let mut relevant: HashSet<InputId> = HashSet::new();
        for (base, _) in &bases {
            let mut everything = base.clone();
            everything.extend(loadout_items.iter().copied());
            if let Some(provenance) = self.recorded(self.explain(&everything)) {
                relevant.extend(relevant_inputs(&provenance, self.conditions.iter()));
            }
        }
        // Stats read straight from build modifiers have no card provenance;
        // the items carrying those modifiers move them.
        for item in &loadout_items {
            if self.conditions.iter().any(|condition| carries_modifier(item, &condition.stat)) {
                relevant.insert(input_id(item));
            }
        }

        // A derived stat moved by inputs no card stat records, such as
        // aircraft fire starts, cannot narrow the items by provenance.
        let untraced = self
            .conditions
            .iter()
            .any(|condition| matches!(condition.stat, SearchStat::Derived(stat) if !stat.fully_traced()));
        for item in loadout_items.iter().filter(|item| untraced || relevant.contains(&input_id(item))) {
            let mut helps: Vec<usize> = Vec::new();
            for (base, before) in &bases {
                let mut with = base.clone();
                with.push(*item);
                let Some(after) = self.try_readings(&with) else {
                    continue;
                };
                for index in self.helped(&every, before, &after) {
                    if !helps.contains(&index) {
                        helps.push(index);
                    }
                }
            }
            if !helps.is_empty() {
                choices.push(Choice { item: *item, helps, enables: false });
            }
        }

        if !failing.iter().all(|&index| self.reachable(index, &choices)) {
            return None;
        }
        choices.sort_by(|left, right| left.item.order().cmp(&right.item.order()));

        Some(choices)
    }

    /// Whether condition `index` holds with every choice that moves it
    /// applied at once, for some choice of one module per kind. Stacking
    /// ignores slot and point limits, so failing here rules the ship out.
    fn reachable(&self, index: usize, choices: &[Choice<'_>]) -> bool {
        let condition = &self.conditions[index];
        if condition.op.direction().is_none() {
            return true;
        }

        let helping: Vec<&Choice<'_>> = choices.iter().filter(|choice| choice.helps.contains(&index)).collect();
        let stacked: Vec<Item<'_>> =
            helping.iter().filter(|choice| !matches!(choice.item, Item::Module(_))).map(|choice| choice.item).collect();

        let mut choices: BTreeMap<ModuleKind, Vec<&Module>> = BTreeMap::new();
        for choice in &helping {
            if let Item::Module(module) = choice.item {
                choices.entry(module.kind).or_default().push(module);
            }
        }
        let mut assignments: Vec<Vec<&Module>> = vec![Vec::new()];
        for modules in choices.values() {
            let mut next = Vec::new();
            for assignment in &assignments {
                next.push(assignment.clone());
                for module in modules {
                    let mut extended = assignment.clone();
                    extended.push(*module);
                    next.push(extended);
                }
            }
            assignments = next;
        }

        for assignment in assignments {
            let mut items: Vec<Item<'_>> = assignment.into_iter().map(Item::Module).collect();
            items.extend(stacked.iter().copied());
            let Some(view) = self.recorded(self.view(&items)) else {
                continue;
            };
            if condition.holds(&view.readings(condition, &self.armor, self.game)) {
                return true;
            }
        }

        false
    }

    /// Tries choice sets by size, then in choice order; the first set
    /// meeting every condition has the fewest items.
    fn enumerate(&self, failing: &[usize], choices: &[Choice<'_>]) -> Outcome {
        /// Sets examined per ship, valid or not, before it is reported as
        /// undetermined.
        const SET_LIMIT: usize = 1 << 20;
        /// Configurations computed per ship before it is reported as
        /// undetermined.
        const EVALUATION_LIMIT: usize = 4096;

        let mut examined = 0;
        let mut evaluations = 0;
        for size in 1..=choices.len() {
            let mut indices: Vec<usize> = (0..size).collect();
            loop {
                examined += 1;
                if examined > SET_LIMIT {
                    return Outcome::Undetermined {
                        reason: format!("more than {SET_LIMIT} sets of requirements would need checking"),
                    };
                }

                let chosen: Vec<&Choice<'_>> = indices.iter().map(|&index| &choices[index]).collect();
                if self.worth_trying(failing, &chosen) {
                    let items: Vec<Item<'_>> = chosen.iter().map(|choice| choice.item).collect();
                    if self.is_valid(&items) {
                        evaluations += 1;
                        if evaluations > EVALUATION_LIMIT {
                            return Outcome::Undetermined {
                                reason: format!("more than {EVALUATION_LIMIT} configurations would need checking"),
                            };
                        }

                        if let Some(view) = self.recorded(self.view(&items)) {
                            let readings = self.readings(&view);
                            if self.all_hold(&readings) {
                                let requires = items.iter().map(|item| item.describe(self.game)).collect();
                                return Outcome::Requires { requires, values: self.values(&readings), unchecked: None };
                            }
                        }
                    }
                }

                if !next_combination(&mut indices, choices.len()) {
                    break;
                }
            }
        }

        Outcome::Impossible
    }

    /// A minimal set moves every failing condition, holds no hull that mounts
    /// nothing it chose, and holds no companion guns or torpedoes without a
    /// chosen hull that needs them.
    fn worth_trying(&self, failing: &[usize], chosen: &[&Choice<'_>]) -> bool {
        let covers = failing.iter().all(|index| chosen.iter().any(|choice| choice.helps.contains(index)));
        let hulls = || {
            chosen.iter().filter_map(|choice| match choice.item {
                Item::Module(module) if module.kind == ModuleKind::Hull => Some(module),
                _ => None,
            })
        };
        let enablers_used = chosen.iter().filter(|choice| choice.enables).all(|enabler| {
            let Item::Module(enabler) = enabler.item else {
                return true;
            };
            if enabler.kind != ModuleKind::Hull {
                return hulls()
                    .any(|hull| self.companions(hull, ModuleKind::Hull).any(|module| std::ptr::eq(module, enabler)));
            }
            chosen.iter().any(|choice| match choice.item {
                Item::Module(module) => {
                    !choice.enables && !self.mounts_on_stock_hull(module) && self.mounts_on(&enabler.param, module)
                }
                _ => false,
            })
        });

        covers && enablers_used
    }

    /// One module per kind, mounted on the chosen hull; one upgrade per slot;
    /// skills within the captain's points.
    fn is_valid(&self, items: &[Item<'_>]) -> bool {
        let mut kinds: Vec<ModuleKind> = Vec::new();
        let mut slots: Vec<u8> = Vec::new();
        let mut skill_points: Vec<SkillPointCost> = Vec::new();
        for item in items {
            match item {
                Item::Module(module) => {
                    if kinds.contains(&module.kind) {
                        return false;
                    }
                    kinds.push(module.kind);
                }
                Item::Upgrade(upgrade) => {
                    if slots.contains(&upgrade.slot) {
                        return false;
                    }
                    slots.push(upgrade.slot);
                }
                Item::Skill { points, .. } => skill_points.push(*points),
            }
        }

        let hull = items
            .iter()
            .find_map(|item| match item {
                Item::Module(module) if module.kind == ModuleKind::Hull => Some(module.param.as_str()),
                _ => None,
            })
            .or(self.stock_selection.hull.as_deref());
        // Guns and torpedoes must fit the chosen hull; the stock pair is
        // taken as valid.
        let (selection, _) = self.configuration(items);
        let stock_hull = self.stock_selection.hull.as_deref();
        let mounted = hull.is_none_or(|hull| {
            [
                (ModuleKind::Guns, selection.artillery.as_deref(), self.stock_selection.artillery.as_deref()),
                (ModuleKind::Torpedoes, selection.torpedoes.as_deref(), self.stock_selection.torpedoes.as_deref()),
            ]
            .into_iter()
            .all(|(kind, param, stock)| {
                param.is_none_or(|param| {
                    (Some(hull) == stock_hull && Some(param) == stock) || self.mounts_param_on(hull, kind, param)
                })
            })
        });

        mounted && skills_fit(&skill_points)
    }

    /// Indices of failing conditions that `after` moves toward their threshold.
    fn helped(&self, failing: &[usize], before: &[Vec<Reading>], after: &[Vec<Reading>]) -> Vec<usize> {
        failing
            .iter()
            .copied()
            .filter(|&index| self.conditions[index].improves(&before[index], &after[index]))
            .collect()
    }

    fn all_hold(&self, readings: &[Vec<Reading>]) -> bool {
        self.conditions.iter().zip(readings).all(|(condition, readings)| condition.holds(readings))
    }

    fn readings(&self, view: &View) -> Vec<Vec<Reading>> {
        let readings: Vec<Vec<Reading>> =
            self.conditions.iter().map(|condition| view.readings(condition, &self.armor, self.game)).collect();
        for (seen, readings) in self.rows_seen.borrow_mut().iter_mut().zip(&readings) {
            *seen |= !readings.is_empty();
        }
        readings
    }

    /// Per condition, the reading that meets it.
    fn values(&self, readings: &[Vec<Reading>]) -> Vec<String> {
        self.conditions
            .iter()
            .zip(readings)
            .filter_map(|(condition, readings)| {
                let reading = condition.best(readings)?;
                let qualifier = row_label(self.game, self.ship, condition.stat, None, reading.qualifier.as_deref())
                    .map(|label| format!(" ({label})"))
                    .unwrap_or_default();
                Some(format!(
                    "{}{qualifier}: {}",
                    condition.stat.name(),
                    format_value(reading.value, condition.stat.unit())
                ))
            })
            .collect()
    }

    /// Every upgrade that fits and every skill of the class tree.
    fn loadout_items(&self) -> Vec<Item<'a>> {
        let mut items: Vec<Item<'a>> =
            self.game.upgrades.iter().filter(|upgrade| upgrade.fits(self.ship)).map(Item::Upgrade).collect();

        for slot in skill_grid(&self.game.version(), self.ship.class.species()).unwrap_or_default() {
            let Some(skill) = self.game.skills.iter().find(|skill| skill.internal_name().as_str() == slot.skill) else {
                continue;
            };
            items.push(Item::Skill { skill, points: SkillPointCost::from_grid_row(slot.tier) });
        }

        items
    }

    fn view(&self, items: &[Item<'_>]) -> Result<View, QueryError> {
        let (selection, loadout) = self.configuration(items);
        let effects = self.game.effective_modifiers(self.ship, &loadout)?;
        let stats =
            effects.build.stats(&self.ship.param, &selection, self.game.provider()).map_err(build_failed(self.ship))?;
        let coverage = self.coverage(&selection)?;
        let derived = Derivation {
            ship: &self.ship.param,
            selection: &selection,
            stats: &stats,
            modifiers: effects.build.modifiers.bundle(),
            merges_fire_zones: effects.build.merges_fire_zones,
            stock_ranges: self.stock_ranges()?,
            provider: self.game.provider(),
            coverage: coverage.as_deref(),
        }
        .rows();

        // Shells are looked up only when a condition reads them.
        let ricochet = if self.conditions.iter().any(|condition| matches!(condition.stat, SearchStat::Ricochet { .. }))
        {
            main_battery_ricochet(self.game, self.ship, &selection)
        } else {
            Vec::new()
        };

        let asw = self
            .conditions
            .iter()
            .any(|condition| matches!(condition.stat, SearchStat::Asw(_)))
            .then(|| self.game.asw_weapons(self.ship, selection.hull.as_deref()));

        Ok(View { rows: stats.rows(), derived, ricochet, asw })
    }

    fn stock_ranges(&self) -> Result<StockRanges, QueryError> {
        if let Some(ranges) = self.stock_ranges.get() {
            return Ok(*ranges);
        }

        let ranges = self.game.stock_ranges(self.ship)?;

        Ok(*self.stock_ranges.get_or_init(|| ranges))
    }

    /// Mount coverage of `selection`, read only when a condition counts the
    /// mounts bearing on one target.
    fn coverage(&self, selection: &ShipUpgradeSelection) -> Result<Option<Rc<MountCoverage>>, QueryError> {
        let needed = self
            .conditions
            .iter()
            .any(|condition| matches!(condition.stat, SearchStat::Derived(stat) if stat.reads_mount_bearings()));
        if !needed {
            return Ok(None);
        }
        if let Some((_, coverage)) = self.coverages.borrow().iter().find(|(known, _)| known == selection) {
            return Ok(coverage.as_ref().map(Rc::clone));
        }

        let coverage = self.game.mount_coverages(self.ship, std::iter::once(selection))?.pop().flatten();
        self.coverages.borrow_mut().push((selection.clone(), coverage.as_ref().map(Rc::clone)));

        Ok(coverage)
    }

    fn explain(&self, items: &[Item<'_>]) -> Result<ShipStatsProvenance, QueryError> {
        let (selection, loadout) = self.configuration(items);
        let effects = self.game.effective_modifiers(self.ship, &loadout)?;
        let card =
            effects.build.card(&self.ship.param, &selection, self.game.provider()).map_err(build_failed(self.ship))?;

        Ok(card.provenance)
    }

    fn configuration(&self, items: &[Item<'_>]) -> (ShipUpgradeSelection, ResolvedLoadout) {
        let mut selection = self.stock_selection.clone();
        let mut loadout = self.game.stock_loadout();
        for item in items {
            match item {
                Item::Module(module) => {
                    let param = Some(module.param.clone());
                    match module.kind {
                        ModuleKind::Hull => selection.hull = param,
                        ModuleKind::Guns => selection.artillery = param,
                        ModuleKind::Torpedoes => selection.torpedoes = param,
                        ModuleKind::Engine => selection.engine = param,
                        ModuleKind::FireControl => selection.fire_control = param,
                    }
                }
                Item::Upgrade(upgrade) => loadout
                    .modernizations
                    .push((upgrade.param.name().to_owned(), upgrade.modernization().modifiers().to_vec())),
                Item::Skill { skill, .. } => loadout.skills.push((*skill).clone()),
            }
        }

        (selection, loadout)
    }

    fn mounts_on_stock_hull(&self, module: &Module) -> bool {
        self.stock_selection.hull.as_deref().is_none_or(|hull| self.mounts_on(hull, module))
    }

    /// Hulls, other than the stock one, that mount `module`.
    fn enabling_hulls(&self, module: &Module) -> Vec<&Module> {
        self.modules
            .iter()
            .filter(|hull| hull.kind == ModuleKind::Hull && self.mounts_on(&hull.param, module))
            .collect()
    }

    fn mounts_on(&self, hull: &str, module: &Module) -> bool {
        self.mounts_param_on(hull, module.kind, &module.param)
    }

    /// Guns or torpedoes, other than `except`, that `hull` needs because it
    /// does not take the stock ones.
    fn companions<'s>(&'s self, hull: &'s Module, except: ModuleKind) -> impl Iterator<Item = &'s Module> + 's {
        [(ModuleKind::Guns, &self.stock_selection.artillery), (ModuleKind::Torpedoes, &self.stock_selection.torpedoes)]
            .into_iter()
            .filter(move |(kind, stock)| {
                *kind != except
                    && stock.as_deref().is_some_and(|stock| !self.mounts_param_on(&hull.param, *kind, stock))
            })
            .flat_map(move |(kind, _)| {
                self.modules.iter().filter(move |module| module.kind == kind && self.mounts_on(&hull.param, module))
            })
    }

    /// Whether the hull upgrade `hull` names a component the `param` upgrade
    /// lists. Data without component links places no limit.
    fn mounts_param_on(&self, hull: &str, kind: ModuleKind, param: &str) -> bool {
        let component_type = match kind {
            ModuleKind::Guns => ComponentType::Artillery,
            ModuleKind::Torpedoes => ComponentType::Torpedoes,
            ModuleKind::Hull | ModuleKind::Engine | ModuleKind::FireControl => return true,
        };
        let Some(vehicle) = self.ship.param.vehicle() else {
            return true;
        };
        let Some(components) = vehicle.ttx_components() else {
            return true;
        };
        if components.upgrade_components(param).is_empty() {
            return true;
        }
        let Some(config) = vehicle.hull_upgrade(hull) else {
            return true;
        };

        components.component_on_hull(param, config, component_type).is_some()
    }
}

/// An item worth trying for one ship.
struct Choice<'a> {
    item: Item<'a>,

    /// Failing conditions the item moves toward their threshold.
    helps: Vec<usize>,

    /// A hull that only serves to mount a helpful module.
    enables: bool,
}

/// Ships meeting every condition, with what the search learned on the way.
#[derive(Default)]
struct ConditionSearch<'a> {
    hits: Vec<(&'a Ship, ConditionMatch)>,
    searched_without_skills: Vec<String>,
    unmatched_qualifiers: Vec<String>,
}

/// A rank request resolved against the stat list.
struct Rank {
    stat: SearchStat,
    qualifier: Option<String>,
    order: RankOrder,
    build: RankBuild,
    basis: BuildBasis,
    limit: RankLimit,
    condition: Option<RankCondition>,
}

/// A condition a speed ranking holds, with the depth of the speed.
struct RankCondition {
    depth: SpeedDepth,
    kind: SpeedCondition,
}

/// Which conditions a speed ranking holds.
enum SpeedCondition {
    /// Every condition that can hold at once ("all").
    Fastest,

    /// One condition as the request names it, matched against speed
    /// alternative labels.
    Named(String),
}

impl Rank {
    fn describe(&self) -> String {
        let rows: Vec<&str> =
            self.stat.shell_battery().map(Battery::label).into_iter().chain(self.qualifier.as_deref()).collect();
        let rows = if rows.is_empty() { String::new() } else { format!(" ({})", rows.join(" ")) };
        let order = match self.order {
            RankOrder::Asc => "lowest first",
            RankOrder::Desc => "highest first",
        };
        let build = match self.build {
            RankBuild::Base => "on the base value",
            RankBuild::Lowest => "on the build that lowers it most",
            RankBuild::Highest => "on the build that raises it most",
        };
        let basis = match self.basis {
            BuildBasis::Given => "",
            BuildBasis::Usual => ", the usual build",
            BuildBasis::Order => ", as the order runs",
        };

        let condition = match &self.condition {
            Some(RankCondition { kind: SpeedCondition::Fastest, .. }) => {
                ", with every condition that can hold at once".to_owned()
            }
            Some(RankCondition { kind: SpeedCondition::Named(name), .. }) => format!(", with {name} held"),
            None => String::new(),
        };

        format!("{}{rows} ({}), {order}, {build}{basis}{condition}", self.stat.name(), self.stat.unit().name())
    }

    fn rule(&self) -> String {
        const BASE_RULE: &str = "Each ship is ranked by its base value: stock hull and modules (or the first modules that have the stat when stock lacks it), no captain skills, no upgrades. Ships are ordered by exact value; ships whose values print the same share a rank.";
        const LOWEST_RULE: &str = "Each ship is ranked by its value on the build that lowers the stat most, as ship_concealment and ship_stats report best builds: the module combination, captain skills (21 points, each skill needing one on every cheaper tier) and upgrades (one per slot) that lower the stat most. Conditional skills count with their triggers unmet; every extra consumable charge counts. With conditions, the build is chosen for the rank stat alone. Ships are ordered by exact value; ships whose values print the same share a rank.";
        const HIGHEST_RULE: &str = "Each ship is ranked by its value on the build that raises the stat most, as ship_concealment and ship_stats report best builds: the module combination, captain skills (21 points, each skill needing one on every cheaper tier) and upgrades (one per slot) that raise the stat most. Conditional skills count with their triggers unmet; every extra consumable charge counts. With conditions, the build is chosen for the rank stat alone. Ships are ordered by exact value; ships whose values print the same share a rank.";
        const USUAL: &str =
            " That is the usual build, the one players run for the stat, whichever order the ships are listed in.";
        const ORDER: &str = " The stat is no better higher than lower, so the build is the one that moves it furthest in the rank order.";

        let rule = match self.build {
            RankBuild::Base => BASE_RULE,
            RankBuild::Lowest => LOWEST_RULE,
            RankBuild::Highest => HIGHEST_RULE,
        };
        let basis = match self.basis {
            BuildBasis::Given => "",
            BuildBasis::Usual => USUAL,
            BuildBasis::Order => ORDER,
        };

        let condition = match &self.condition {
            Some(RankCondition { kind: SpeedCondition::Fastest, .. }) => " With the condition \"all\": each ship is ranked on its fastest combination of everything that can hold at once on top of that build: its strongest speed boost consumable active, every speed signal mounted, its special ability active when it raises speed, and the conditional skills whose triggers can hold at the same moment (a skill needing the ship undetected never stacks with one needing it detected, such as Swift in Silence with Dazzle), within 21 skill points. Its entry names the combination under condition and every item under build. A ship with nothing to add is ranked on its always-on best build and its entry has no condition. Commander talents are not counted.".to_owned(),
            Some(RankCondition { kind: SpeedCondition::Named(name), .. }) => format!(
                " With the condition \"{name}\": each ship that has it is ranked on its ship_stats speed alternative with it held, on top of that build; its entry names the alternative under condition. A ship without it is ranked on its always-on best build and its entry has no condition."
            ),
            None => String::new(),
        };

        format!("{rule}{basis}{condition}")
    }

    /// Which way the build moves the stat, and which row of a ship with
    /// several the ship is ranked by. A base ranking takes the row first
    /// in the order.
    fn direction(&self) -> RankOrder {
        match self.build {
            RankBuild::Base => self.order,
            RankBuild::Lowest => RankOrder::Asc,
            RankBuild::Highest => RankOrder::Desc,
        }
    }

    /// The value of `builds` the ship is ranked by.
    fn ranked(&self, builds: &Builds<Amount>) -> Amount {
        match self.build {
            RankBuild::Base => builds.base,
            RankBuild::Lowest | RankBuild::Highest => builds.best.value,
        }
    }
}

/// One ranked ship.
struct RankEntry<'a> {
    ship: &'a Ship,
    builds: Builds<Amount>,
    fires_by_armament: Option<FireBreakdown>,
    condition: Option<String>,
    meets: Option<ConditionMatch>,
}

/// The rank stat's builds on one ship, with each armament's share for the
/// fire start stats.
struct RankedBuilds {
    builds: Builds<Amount>,
    fires_by_armament: Option<FireBreakdown>,

    /// The speed alternative the best value is on, for a ranking with a
    /// condition.
    condition: Option<String>,
}

/// A stat value in its search `Unit`, for the build planner, with the row it
/// comes from.
#[derive(Debug, Clone, Copy)]
struct Amount {
    value: f32,
    row: Option<RowId>,
}

impl StatAmount for Amount {
    fn amount(self) -> f32 {
        self.value
    }
}

/// Index of a row in [`RowNames`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowId(usize);

/// A ranked row: its qualifier and, for main battery shell stats, the gun
/// caliber in mm.
#[derive(Debug, Clone, PartialEq)]
struct RowKey {
    qualifier: String,
    caliber: Option<f32>,
}

/// Rows seen while ranking, so an [`Amount`] names its row and stays `Copy`
/// as the planner requires.
#[derive(Default)]
struct RowNames(RefCell<Vec<RowKey>>);

impl RowNames {
    fn id(&self, qualifier: Option<&str>, caliber: Option<f32>) -> Option<RowId> {
        let qualifier = qualifier?;
        let mut keys = self.0.borrow_mut();
        let found = keys.iter().position(|key| key.qualifier == qualifier && key.caliber == caliber);
        let index = match found {
            Some(index) => index,
            None => {
                keys.push(RowKey { qualifier: qualifier.to_owned(), caliber });
                keys.len() - 1
            }
        };

        Some(RowId(index))
    }

    fn label(&self, game: &GameData, ship: &Ship, stat: SearchStat, id: RowId) -> Option<String> {
        let key = &self.0.borrow()[id.0];

        row_label(game, ship, stat, key.caliber, Some(&key.qualifier))
    }
}

/// Every stat stat_search can search.
pub(crate) fn stat_definitions(game: &GameData) -> Vec<StatDefinition> {
    SearchStat::all()
        .into_iter()
        .map(|stat| StatDefinition {
            name: stat.name(),
            unit: stat.unit().name(),
            label: stat.label(game),
            item: stat.item(),
            better: stat.better(),
        })
        .collect()
}

fn resolve_rank(rank: &StatRank) -> Result<Rank, QueryError> {
    const DEFAULT_LIMIT: RankLimit = RankLimit(10);
    const MAX_LIMIT: RankLimit = RankLimit(25);

    let limit = rank.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT.0).contains(&limit.0) {
        return Err(QueryError::RankLimitOutOfRange { limit: limit.0, max: MAX_LIMIT.0 });
    }

    let stat = resolve_stat(&rank.stat)?;
    let (build, basis) = match rank.build {
        Some(build) => (build, BuildBasis::Given),
        None => RankBuild::default_for(stat, rank.order),
    };
    let condition = match &rank.condition {
        Some(name) => {
            let Some(depth) = stat.speed_depth() else {
                return Err(QueryError::RankConditionNotSpeed { stat: stat.name() });
            };
            if rank.build.is_some() {
                return Err(QueryError::RankConditionWithBuild);
            }
            let kind = if name.trim().eq_ignore_ascii_case("all") {
                SpeedCondition::Fastest
            } else {
                SpeedCondition::Named(name.clone())
            };
            Some(RankCondition { depth, kind })
        }
        None => None,
    };

    Ok(Rank { stat, qualifier: rank.qualifier.clone(), order: rank.order, build, basis, limit, condition })
}

fn qualifying_counts(hits: &[(&Ship, ConditionMatch)]) -> QualifyingCounts {
    let stock = hits.iter().filter(|(_, meets)| meets.stock).count();
    let counts = ShipCounts::from_ships(hits.iter().map(|(ship, _)| *ship));

    QualifyingCounts {
        total: counts.total,
        by_class: counts.by_class,
        stock,
        with_requirements: hits.len() - stock,
        test: counts.test,
    }
}

fn resolve_condition(condition: &StatCondition) -> Result<Condition, QueryError> {
    Ok(Condition {
        stat: resolve_stat(&condition.stat)?,
        op: condition.op,
        threshold: condition.value,
        qualifier: condition.qualifier.clone(),
    })
}

fn resolve_stat(query: &str) -> Result<SearchStat, QueryError> {
    let key = search_key(query);
    let stats = SearchStat::all();
    let names: Vec<(String, String)> = stats.iter().map(|stat| (search_key(&stat.name()), stat.name())).collect();

    if let Some(index) = names.iter().position(|(stat_key, _)| *stat_key == key) {
        return Ok(stats[index]);
    }
    if let Some(stat) = TtxStat::ALL.iter().find(|stat| search_key(stat.field_key()) == key) {
        return Err(QueryError::StatNotNumeric { stat: stat.field_key() });
    }
    if let Some((stat, ammo)) = per_ammo_stat(query, &names) {
        return Err(QueryError::StatNameNamesAmmo { query: query.to_owned(), stat, ammo });
    }

    match fuzzy::closest(&key, names.iter().map(|(key, name)| fuzzy::Candidate::primary(key, name))) {
        Fuzzy::Unique(name) => {
            let index =
                names.iter().position(|(_, stat_name)| stat_name == name).expect("closest returns a listed name");
            Ok(stats[index])
        }
        Fuzzy::Suggestions(suggestions) if !suggestions.is_empty() => Err(QueryError::StatNotFound {
            query: query.to_owned(),
            suggestions: suggestions.into_iter().map(str::to_owned).collect(),
        }),
        // A GameParams key such as "artillery.max_dist" is too far from any
        // stat name; offer the stats of its group instead.
        Fuzzy::Suggestions(_) => {
            let group = query.trim().split_once('.').map(|(group, _)| format!("{}.", group.to_ascii_lowercase()));
            let suggestions = group
                .map(|group| {
                    names.iter().map(|(_, name)| name).filter(|name| name.starts_with(&group)).cloned().collect()
                })
                .unwrap_or_default();
            Err(QueryError::StatNotFound { query: query.to_owned(), suggestions })
        }
    }
}

/// The per-ammo stat and ammo type a query such as "artillery.he.penetration"
/// means: a shell stat with the ammo type in place of "shells".
fn per_ammo_stat(query: &str, names: &[(String, String)]) -> Option<(String, Ammo)> {
    let query = query.trim().to_ascii_lowercase();
    let mut segments = query.splitn(3, '.');
    let (group, ammo, field) = (segments.next()?, segments.next()?, segments.next()?);
    let ammo = match ammo {
        "ap" => Ammo::Ap,
        "he" => Ammo::He,
        "sap" | "cs" => Ammo::Sap,
        _ => return None,
    };
    let key = search_key(&format!("{group}.shells.{field}"));

    names.iter().find(|(stat_key, _)| *stat_key == key).map(|(_, name)| (name.clone(), ammo))
}

/// Loadout inputs behind the failing conditions' stats, following derived
/// stats to the stats they derive from.
fn relevant_inputs<'c>(
    provenance: &ShipStatsProvenance,
    failing: impl Iterator<Item = &'c Condition>,
) -> HashSet<InputId> {
    let by_key: HashMap<StatKey, usize> = provenance
        .attributions
        .iter()
        .enumerate()
        .map(|(index, attribution)| {
            (StatKey { stat: attribution.stat, qualifier: attribution.qualifier.clone() }, index)
        })
        .collect();

    let mut pending: Vec<usize> = Vec::new();
    for condition in failing {
        let stats: &[TtxStat] = match &condition.stat {
            SearchStat::Card(stat) => std::slice::from_ref(stat),
            SearchStat::Derived(stat) => stat.sources(),
            SearchStat::Armor { .. } | SearchStat::Ricochet { .. } | SearchStat::Asw(_) => continue,
        };
        pending.extend(
            provenance
                .attributions
                .iter()
                .enumerate()
                .filter(|(_, attribution)| stats.contains(&attribution.stat))
                .map(|(index, _)| index),
        );
    }

    let mut visited: HashSet<usize> = HashSet::new();
    let mut inputs = HashSet::new();
    while let Some(index) = pending.pop() {
        if !visited.insert(index) {
            continue;
        }

        let attribution = &provenance.attributions[index];
        inputs.extend(attribution.steps.iter().map(|step| step.input.clone()));
        pending.extend(attribution.derived_from.iter().filter_map(|key| by_key.get(key).copied()));
    }

    inputs
}

fn input_id(item: &Item<'_>) -> InputId {
    match item {
        Item::Module(module) => InputId::Module {
            slot: match module.kind {
                ModuleKind::Hull => wowsunpack::game_params::ttx::ModuleSlot::Hull,
                ModuleKind::Guns => wowsunpack::game_params::ttx::ModuleSlot::Artillery,
                ModuleKind::Torpedoes => wowsunpack::game_params::ttx::ModuleSlot::Torpedoes,
                ModuleKind::Engine => wowsunpack::game_params::ttx::ModuleSlot::Engine,
                ModuleKind::FireControl => wowsunpack::game_params::ttx::ModuleSlot::FireControl,
            },
            name: module.param.clone(),
        },
        Item::Upgrade(upgrade) => InputId::Upgrade { name: upgrade.param.name().to_owned() },
        Item::Skill { skill, .. } => InputId::Skill { name: skill.internal_name().clone() },
    }
}

/// Whether `item` carries a build modifier the derived `stat` reads, or is
/// Fire Prevention Expert for the fire zone count.
fn carries_modifier(item: &Item<'_>, stat: &SearchStat) -> bool {
    let SearchStat::Derived(stat) = *stat else {
        return false;
    };
    let modifiers = match item {
        Item::Module(_) => return false,
        Item::Upgrade(upgrade) => upgrade.modernization().modifiers(),
        Item::Skill { skill, .. } => {
            if stat == DerivedStat::FireZones && merges_fire_zones(skill) {
                return true;
            }
            skill.modifiers().map_or(&[][..], Vec::as_slice)
        }
    };

    modifiers.iter().any(|modifier| stat.modifiers().contains(&modifier.name()))
}

/// Ammo type and ricochet angles of each main battery shell of the selected guns.
fn main_battery_ricochet(
    game: &GameData,
    ship: &Ship,
    selection: &ShipUpgradeSelection,
) -> Vec<(Ammo, RicochetAngles)> {
    let Some(artillery) = ship
        .param
        .vehicle()
        .and_then(|vehicle| vehicle.ttx_components())
        .and_then(|components| components.artillery(selection.artillery.as_deref()?))
    else {
        return Vec::new();
    };

    let mut names: Vec<&str> = artillery.guns.iter().flat_map(|gun| gun.ammo.iter().map(String::as_str)).collect();
    names.sort_unstable();
    names.dedup();

    names
        .into_iter()
        .filter_map(|name| {
            let param = game.provider().game_param_by_name(name)?;
            let projectile = param.projectile()?;
            let ammo = shell_ammo(projectile)?;
            Some((ammo, ricochet_angles(ammo, projectile)?))
        })
        .collect()
}

/// A skill needs a learned skill on the tier below it
/// (`CommonCrew.hasLearnedSkillOnTier`), so each cheaper tier left empty costs
/// its cheapest skill on top.
pub(crate) fn skills_fit(points: &[SkillPointCost]) -> bool {
    /// `SKILL_POINTS.MAX_FREE_SKILL_POINTS` in the client.
    const MAX_SKILL_POINTS: u32 = 21;

    let Some(highest) = points.iter().map(|points| points.get()).max() else {
        return true;
    };

    let chosen: u32 = points.iter().map(|points| u32::from(points.get())).sum();
    let filler: u32 = (1..highest).filter(|&tier| !points.contains(&SkillPointCost::new(tier))).map(u32::from).sum();

    chosen + filler <= MAX_SKILL_POINTS
}

/// Advances `indices` to the next combination of `count` items in
/// lexicographic order; false when there is none.
pub(crate) fn next_combination(indices: &mut [usize], count: usize) -> bool {
    let size = indices.len();
    let Some(position) = (0..size).rev().find(|&position| indices[position] < count - size + position) else {
        return false;
    };

    indices[position] += 1;
    for next in position + 1..size {
        indices[next] = indices[next - 1] + 1;
    }

    true
}

/// `value` rounded as `{:.2}` prints it. An f32 times 100 is exact in f64,
/// and formatting rounds the exact decimal half to even.
fn shown(value: f32) -> f32 {
    ((f64::from(value) * 100.0).round_ties_even() / 100.0) as f32
}

pub(crate) fn format_value(value: f32, unit: Unit) -> String {
    // Only unlimited charges are infinite; see `card_values`.
    if value.is_infinite() {
        return "unlimited".to_owned();
    }

    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');

    format!("{text}{}", unit.suffix())
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// The rows of `stat` with their values, only the row named `qualifier`
/// when given.
fn card_values<'r>(
    game: &'r GameData,
    rows: &'r [StatRow],
    stat: TtxStat,
    qualifier: Option<&'r str>,
) -> impl Iterator<Item = (&'r StatRow, f32)> + 'r {
    rows.iter()
        .filter(move |row| row.stat == stat && qualifier_matches(game, row.qualifier.as_deref(), qualifier))
        .filter_map(|row| {
            let value = match row.value {
                // Infinity sorts unlimited charges above every count.
                StatValue::Ammo(AmmoCount::Infinite) => f32::INFINITY,
                value => value.as_f32()?,
            };
            Some((row, value))
        })
}

/// The derived rows of `stat`, only the row named `qualifier` when given.
fn derived_rows<'r>(
    game: &'r GameData,
    rows: &'r [DerivedRow],
    stat: DerivedStat,
    qualifier: Option<&'r str>,
) -> impl Iterator<Item = &'r DerivedRow> + 'r {
    rows.iter().filter(move |row| row.stat == stat && qualifier_matches(game, row.qualifier.as_deref(), qualifier))
}

/// Whether `row` is the row `wanted` names, a projectile by GameParams name
/// or English name, or a shell by ammo type ("SAP" for "CS"). No wanted
/// qualifier accepts any row.
fn qualifier_matches(game: &GameData, row: Option<&str>, wanted: Option<&str>) -> bool {
    let Some(wanted) = wanted.map(str::trim) else {
        return true;
    };

    row.is_some_and(|row| {
        row.eq_ignore_ascii_case(wanted)
            || game.qualifier_name(row).eq_ignore_ascii_case(wanted)
            || AmmoType::from_game_str(row).display_name().eq_ignore_ascii_case(wanted)
    })
}

fn armor_value(armor: &BTreeMap<&'static str, ThicknessSpan>, zone: ArmorZone, bound: Bound) -> Option<f32> {
    let span = armor.get(zone.zone)?;
    let mm = match bound {
        Bound::Min => span.thinnest(),
        Bound::Max => span.thickest(),
    };

    Some(mm.value())
}

/// The lowest ("asc") or highest ("desc") of a ship's rows.
fn extreme(order: RankOrder, values: impl Iterator<Item = Amount>) -> Option<Amount> {
    match order {
        RankOrder::Asc => values.min_by(|left, right| left.value.total_cmp(&right.value)),
        RankOrder::Desc => values.max_by(|left, right| left.value.total_cmp(&right.value)),
    }
}

/// What a row of `stat` on `ship` stands for, e.g. "main battery 483 mm HE"
/// or "Main Battery Reload Booster, type artilleryBoosters"; `None` when
/// neither battery nor row needs naming.
fn row_label(
    game: &GameData,
    ship: &Ship,
    stat: SearchStat,
    caliber: Option<f32>,
    qualifier: Option<&str>,
) -> Option<String> {
    let battery = stat.shell_battery().map(Battery::label).map(str::to_owned);
    let caliber = caliber.map(|caliber| format_value(caliber, Unit::Millimeters));
    let row = qualifier.map(|qualifier| match battery {
        // Shell rows are keyed by GameParams ammo type, "CS" for SAP.
        Some(_) => AmmoType::from_game_str(qualifier).display_name().to_owned(),
        // Consumable rows are keyed by consumable type, which qualifiers
        // accept; the English name says which consumable it is.
        None if stat.name().starts_with("consumable.") => match game.consumable_row_name(ship, qualifier) {
            Some(name) => format!("{name}, type {qualifier}"),
            None => qualifier.to_owned(),
        },
        None => game.qualifier_name(qualifier),
    });
    let parts: Vec<String> = battery.into_iter().chain(caliber).chain(row).collect();

    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Caliber in mm of the main battery guns when `stat` is a main battery shell
/// stat, so a ranked row names the shell.
fn main_battery_caliber(stat: SearchStat, rows: &[StatRow]) -> Option<f32> {
    if stat.shell_battery() != Some(Battery::Main) {
        return None;
    }

    rows.iter().find(|row| row.stat == TtxStat::GunCaliber).and_then(|row| row.value.as_f32())
}

/// Base and best-fit build of a stat the planner computes. A stat the stock
/// modules lack, such as torpedoes a later hull mounts, is ranked from the
/// first module combination that has it.
fn planned_builds(
    planner: &Planner<'_>,
    rank: &Rank,
    extract: Extract<'_, Amount>,
) -> Result<Option<Builds<Amount>>, QueryError> {
    match rank_plan(planner, rank, extract)? {
        Some(plan) => planner.best(&plan),
        None => Ok(None),
    }
}

/// The plan that ranks a ship by `rank`, starting from the first module
/// combination that has the stat. `None` when none has it.
fn rank_plan<'f>(
    planner: &Planner<'_>,
    rank: &Rank,
    extract: Extract<'f, Amount>,
) -> Result<Option<StatPlan<'f, Amount>>, QueryError> {
    let mut base_selection = None;
    for selection in 0..planner.selections().count() {
        if extract(&*planner.bare(selection)?).is_some() {
            base_selection = Some(selection);
            break;
        }
    }
    let Some(base_selection) = base_selection else {
        return Ok(None);
    };

    let plan = StatPlan::new(rank.stat.name(), rank.direction().better(), extract).on_selection(base_selection);

    Ok(Some(if rank.stat.is_consumable() { plan.for_consumable() } else { plan }))
}

/// `breakdown` with its rates rounded for display.
fn rounded(breakdown: FireBreakdown) -> FireBreakdown {
    let round_fires = |fires: FiresPerMinute| FiresPerMinute::new(round(fires.value()));

    FireBreakdown {
        bearing: breakdown.bearing,
        total: round_fires(breakdown.total),
        main_battery: breakdown.main_battery.map(round_fires),
        secondary: breakdown.secondary.map(round_fires),
        aircraft: breakdown.aircraft.map(round_fires),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn card_units_match_every_stock_row() {
        let game = test_game();
        let mut checked = 0;
        for ship in &game.ships {
            let view = ShipSearch::new(&game, ship, &[]).view(&[]);
            let Ok(view) = view else {
                continue;
            };
            for row in &view.rows {
                assert_eq!(
                    unit_of_value(&row.value),
                    wows_ship_stats::stat::unit(row.stat),
                    "{:?} on {}",
                    row.stat,
                    ship.name
                );
                checked += 1;
            }
        }
        assert!(checked > 10_000, "only {checked} rows checked");
    }

    fn unit_of_value(value: &StatValue) -> Unit {
        match value {
            StatValue::Hp(_) => Unit::Hp,
            StatValue::Knots(_) => Unit::Knots,
            StatValue::Seconds(_) => Unit::Seconds,
            StatValue::Meters(_) => Unit::Meters,
            StatValue::MetersPerSecond(_) | StatValue::VerticalSpeed(_) => Unit::MetersPerSecond,
            StatValue::Km(_) => Unit::Km,
            StatValue::Millimeters(_) => Unit::Millimeters,
            StatValue::Percent(_) => Unit::Percent,
            StatValue::DegreesPerSecond(_) => Unit::DegreesPerSecond,
            StatValue::Count(_) | StatValue::Ammo(_) => Unit::Count,
            StatValue::Float(_) => Unit::Ratio,
            StatValue::Bool(_) => Unit::Flag,
        }
    }

    fn test_game() -> GameData {
        let root =
            std::path::PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
            .expect("dump root has a readable builds.toml");
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");

        GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
    }

    #[test]
    fn comparisons_use_the_printed_value() {
        let reload = 5.5_f32 * 0.95;
        assert_eq!(format_value(reload, Unit::Seconds), "5.22 s");
        assert!(Comparison::AtMost.holds(reload, 5.22));

        for hundredths in 0..100_000_u32 {
            let value = hundredths as f32 / 100.0 + 0.005;
            let printed: f32 = format!("{value:.2}").parse().expect("formatted f32 parses");
            assert_eq!(shown(value), printed, "{value}");
        }
    }

    #[test]
    fn comparisons_tolerate_rounding() {
        assert!(Comparison::AtMost.holds(4.50001, 4.5));
        assert!(!Comparison::Less.holds(4.50001, 4.5));
        assert!(Comparison::Equal.holds(12.0, 12.0));
        assert!(Comparison::NotEqual.holds(11.9, 12.0));
        assert!(Comparison::Greater.holds(f32::INFINITY, 1000.0));
    }

    #[test]
    fn skills_need_a_skill_on_every_cheaper_tier() {
        let fit = |points: &[u8]| skills_fit(&points.iter().copied().map(SkillPointCost::new).collect::<Vec<_>>());
        assert!(fit(&[4, 4, 4]));
        assert!(fit(&[4, 4, 4, 3, 3, 2, 1]));
        // 16 points plus the cheapest skill of tiers 1, 2 and 3.
        assert!(!fit(&[4, 4, 4, 4]));
        // 18 points plus tiers 1 and 3.
        assert!(!fit(&[4, 4, 4, 4, 2]));
    }

    #[test]
    fn combinations_are_lexicographic() {
        let mut indices = vec![0, 1];
        let mut seen = vec![indices.clone()];
        while next_combination(&mut indices, 4) {
            seen.push(indices.clone());
        }
        assert_eq!(seen, vec![vec![0, 1], vec![0, 2], vec![0, 3], vec![1, 2], vec![1, 3], vec![2, 3]]);
    }

    #[test]
    fn stat_names_resolve_by_field_key_or_words() {
        assert_eq!(resolve_stat("artillery.reload_time").ok(), Some(SearchStat::Card(TtxStat::ArtilleryReloadTime)));
        assert_eq!(resolve_stat("artillery reload time").ok(), Some(SearchStat::Card(TtxStat::ArtilleryReloadTime)));
        assert_eq!(
            resolve_stat("armor.citadel.max").ok(),
            Some(SearchStat::Armor { zone: ARMOR_ZONES[2], bound: Bound::Max })
        );
        assert!(matches!(
            resolve_stat("torpedoes.torpedoes.is_damage_increasing"),
            Err(QueryError::StatNotNumeric { .. })
        ));
    }

    #[test]
    fn gameparams_keys_suggest_the_stats_of_their_group() {
        let Err(QueryError::StatNotFound { suggestions, .. }) = resolve_stat("artillery.max_dist") else {
            panic!("artillery.max_dist is not a stat name");
        };
        assert!(suggestions.iter().any(|name| name == "artillery.range"), "{suggestions:?}");
        assert!(suggestions.iter().all(|name| name.starts_with("artillery.")), "{suggestions:?}");
    }
}
