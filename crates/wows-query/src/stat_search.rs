//! Fleet search by stat conditions. A ship qualifies with its stock modules,
//! or with the fewest modules, upgrades and captain skills that make every
//! condition hold.

use std::cell::OnceCell;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_ship_stats::Derivation;
use wows_ship_stats::DerivedRow;
use wows_ship_stats::DerivedStat;
use wows_ship_stats::Direction;
use wows_ship_stats::StockRanges;
use wows_ship_stats::Triggers;
use wows_ship_stats::Unit;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::skill_grid_data::skill_grid;
use wowsunpack::game_params::ttx::InputId;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::StatKey;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::labels::stat_display_label;
use wowsunpack::game_params::ttx::model::AmmoCount;
use wowsunpack::game_params::ttx::model::StatRow;
use wowsunpack::game_params::ttx::model::StatValue;
use wowsunpack::game_params::ttx::provenance::ShipStatsProvenance;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::SkillPointCost;

use crate::armor::ThicknessSpan;
use crate::armor::plates;
use crate::armor::zone_spans;
use crate::data::GameData;
use crate::error::QueryError;
use crate::fuzzy;
use crate::fuzzy::Fuzzy;
use crate::loadout::ResolvedLoadout;
use crate::loadout::build_failed;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::names::search_key;
use crate::ships::Shares;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;
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

    /// Only the row with this qualifier: ammo type for shells ("HE", "AP", "CS"), torpedo, secondary mount or consumable name. Omit to accept any row.
    pub qualifier: Option<String>,
}

/// Arguments for `stat_search`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatSearchArgs {
    /// Conditions that must all hold.
    pub conditions: Vec<StatCondition>,

    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,
    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA".
    pub nation: Option<String>,

    /// Include event, clan-battle and retired variants. Defaults to false.
    pub include_event_variants: Option<bool>,

    /// Leave out ships in closed testing. Defaults to false: they are searched and counted under `test`. A test copy of a released ship is searched only with event variants.
    pub exclude_test_ships: Option<bool>,

    /// Search premium replicas of other ships (e.g. "BA Montana" of Montana, Xuan Wu of Iowa) as separate ships. Defaults to false: they are left out of the results and counts and named in their original's entry, unless a nation filter leaves out the original.
    pub include_replicas: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatSearchOutput {
    /// The conditions as understood, e.g. "artillery.reload_time < 5 s".
    pub conditions: Vec<String>,

    pub rule: &'static str,

    /// Ships matching the tier, class and nation filters.
    pub considered: ShipCounts,

    pub qualifying: QualifyingCounts,

    #[serde(flatten)]
    pub shares: Shares,

    /// Fewest requirements first, then highest tier.
    pub ships: Vec<StatSearchHit>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,

    /// Ships that are not counted as qualifying because their search could
    /// not finish.
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

        stats
    }

    fn name(self) -> String {
        match self {
            Self::Card(stat) => stat.field_key().to_owned(),
            Self::Armor { zone, bound: Bound::Min } => format!("armor.{}.min", zone.key),
            Self::Armor { zone, bound: Bound::Max } => format!("armor.{}.max", zone.key),
            Self::Derived(stat) => stat.field_key().to_owned(),
        }
    }

    fn unit(self) -> Unit {
        match self {
            Self::Card(stat) => wows_ship_stats::stat::unit(stat),
            Self::Armor { .. } => Unit::Millimeters,
            Self::Derived(stat) => stat.unit(),
        }
    }

    fn label(self, game: &GameData) -> String {
        match self {
            Self::Card(stat) => clean(&stat_display_label(stat, game.provider())),
            Self::Armor { zone, bound: Bound::Min } => {
                format!("Thinnest {} plate above the waterline", zone.key.replace('_', " "))
            }
            Self::Armor { zone, bound: Bound::Max } => {
                format!("Thickest {} plate above the waterline", zone.key.replace('_', " "))
            }
            Self::Derived(stat) => stat.label().to_owned(),
        }
    }

    /// What one row of a multi-row stat stands for.
    fn item(self) -> Option<&'static str> {
        let key = match self {
            Self::Card(TtxStat::SecondaryRange) | Self::Armor { .. } => return None,
            Self::Card(stat) => stat.field_key(),
            Self::Derived(stat) => stat.field_key(),
        };

        if key.starts_with("artillery.shells.") {
            Some("per ammo type")
        } else if key.starts_with("secondary.") {
            Some("per secondary mount")
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

    /// Internal names of conditional skills counted as triggered.
    triggered: Vec<String>,
}

impl View {
    fn readings(&self, condition: &Condition, armor: &BTreeMap<&'static str, ThicknessSpan>) -> Vec<Reading> {
        match condition.stat {
            SearchStat::Card(stat) => self
                .rows
                .iter()
                .filter(|row| row.stat == stat)
                .filter(|row| {
                    condition.qualifier.as_deref().is_none_or(|wanted| {
                        row.qualifier.as_deref().is_some_and(|qualifier| qualifier.eq_ignore_ascii_case(wanted.trim()))
                    })
                })
                .filter_map(|row| {
                    let value = match row.value {
                        StatValue::Ammo(AmmoCount::Infinite) => f32::INFINITY,
                        value => value.as_f32()?,
                    };
                    Some(Reading { qualifier: row.qualifier.clone(), value })
                })
                .collect(),
            SearchStat::Armor { zone, bound } => armor
                .get(zone.zone)
                .map(|span| {
                    let mm = match bound {
                        Bound::Min => span.thinnest(),
                        Bound::Max => span.thickest(),
                    };
                    Reading { qualifier: None, value: mm.value() }
                })
                .into_iter()
                .collect(),
            SearchStat::Derived(stat) => self
                .derived
                .iter()
                .filter(|row| row.stat == stat)
                .filter(|row| {
                    condition.qualifier.as_deref().is_none_or(|wanted| {
                        row.qualifier.as_deref().is_some_and(|qualifier| qualifier.eq_ignore_ascii_case(wanted.trim()))
                    })
                })
                .map(|row| Reading { qualifier: row.qualifier.clone(), value: row.value.as_f32() })
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

    fn describe(&self, game: &GameData, triggered: &[String]) -> String {
        match self {
            Item::Module(module) => format!("{}: {}", module.kind.label(), module.name),
            Item::Upgrade(upgrade) => format!("upgrade: {} (slot {})", upgrade.name, upgrade.port_slot()),
            Item::Skill { skill, points } => {
                let trigger = if triggered.iter().any(|name| name == skill.internal_name().as_str()) {
                    ", trigger counted as met"
                } else {
                    ""
                };
                format!("skill: {} ({} points{trigger})", game.skill_name(skill), points.get())
            }
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
    /// modules, upgrades and captain skills.
    pub fn stat_search(&self, args: &StatSearchArgs) -> Result<StatSearchOutput, QueryError> {
        if args.conditions.is_empty() {
            return Err(QueryError::InvalidArguments { reason: "give at least one condition" });
        }
        let conditions: Vec<Condition> =
            args.conditions.iter().map(resolve_condition).collect::<Result<_, QueryError>>()?;

        let filter = ShipFilter {
            tiers: TierRange::from_bounds(args.min_tier, args.max_tier),
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: args.include_event_variants.unwrap_or(false),
            exclude_test: args.exclude_test_ships.unwrap_or(false),
            include_replicas: args.include_replicas.unwrap_or(false),
        };
        let mut ships: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| right.tier.cmp(&left.tier).then_with(|| left.name.cmp(&right.name)));

        let mut hits: Vec<(&Ship, StatSearchHit)> = Vec::new();
        let mut undetermined = Vec::new();
        let mut searched_without_skills = Vec::new();
        let mut qualifier_matched = vec![false; conditions.len()];
        for &ship in &ships {
            let search = ShipSearch::new(self, ship, &conditions);
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
                Outcome::Stock { values } => hits.push((
                    ship,
                    StatSearchHit {
                        ship: ship.listing_label(),
                        stock: true,
                        requires: Vec::new(),
                        values,
                        unchecked: None,
                    },
                )),
                Outcome::Requires { requires, values, unchecked } => hits.push((
                    ship,
                    StatSearchHit { ship: ship.listing_label(), stock: false, requires, values, unchecked },
                )),
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
            left.requires
                .len()
                .cmp(&right.requires.len())
                .then_with(|| right_ship.tier.cmp(&left_ship.tier))
                .then_with(|| left_ship.name.cmp(&right_ship.name))
        });

        let stock = hits.iter().filter(|(_, hit)| hit.stock).count();
        let counts = ShipCounts::from_ships(hits.iter().map(|(ship, _)| *ship));
        let qualifying = QualifyingCounts {
            total: counts.total,
            by_class: counts.by_class,
            stock,
            with_requirements: hits.len() - stock,
            test: counts.test,
        };
        const HIT_LIMIT: usize = 40;
        let listed: Vec<StatSearchHit> = hits.into_iter().take(HIT_LIMIT).map(|(_, hit)| hit).collect();
        let omitted = (qualifying.total > listed.len()).then(|| qualifying.total - listed.len());

        let considered = ShipCounts::from_ships(ships.iter().copied());

        Ok(StatSearchOutput {
            conditions: conditions.iter().map(Condition::describe).collect(),
            rule: "A ship qualifies with stock modules, or with the fewest modules, upgrades (one per slot) and captain skills (21 points, each skill needing one on every cheaper tier) that make every condition hold. Conditional skills count as triggered; health-scaled effects use full health.",
            shares: Shares::new(&qualifying.by_class, &considered),
            considered,
            qualifying,
            ships: listed,
            omitted,
            undetermined,
            searched_without_skills,
            unmatched_qualifiers,
        })
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

        for item in loadout_items.iter().filter(|item| relevant.contains(&input_id(item))) {
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
            if condition.holds(&view.readings(condition, &self.armor)) {
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
                                let requires =
                                    items.iter().map(|item| item.describe(self.game, &view.triggered)).collect();
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
            self.conditions.iter().map(|condition| view.readings(condition, &self.armor)).collect();
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
                let qualifier =
                    reading.qualifier.as_deref().map(|qualifier| format!(" ({qualifier})")).unwrap_or_default();
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
        let derived = Derivation {
            ship: &self.ship.param,
            selection: &selection,
            stats: &stats,
            modifiers: effects.build.modifiers.bundle(),
            stock_ranges: self.stock_ranges()?,
        }
        .rows();

        Ok(View { rows: stats.rows(), derived, triggered: effects.active_conditions })
    }

    fn stock_ranges(&self) -> Result<StockRanges, QueryError> {
        if let Some(ranges) = self.stock_ranges.get() {
            return Ok(*ranges);
        }

        let ranges = self.game.stock_ranges(self.ship)?;

        Ok(*self.stock_ranges.get_or_init(|| ranges))
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
        loadout.triggers = Triggers::All;
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
            SearchStat::Armor { .. } => continue,
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
fn next_combination(indices: &mut [usize], count: usize) -> bool {
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

fn format_value(value: f32, unit: Unit) -> String {
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
            StatValue::MetersPerSecond(_) => Unit::MetersPerSecond,
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
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
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
