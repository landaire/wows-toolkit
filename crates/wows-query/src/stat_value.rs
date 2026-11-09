//! Which stat of a ship a quoted number is, and which build gives it.
//!
//! The search reads every port card and derived stat of the ship on its
//! module combinations, alone and with up to [`MAX_ITEMS`] captain skills and
//! upgrades. Each item's effect on a stat is first measured alone; a
//! combination is computed in full only when the effects, multiplied or
//! added, bring the stat near the number, so the search stays bounded.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_ship_stats::Derivation;
use wows_ship_stats::DerivedStat;
use wows_ship_stats::StockRanges;
use wows_ship_stats::Unit;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::labels::stat_display_label;
use wowsunpack::game_params::ttx::render_attributions;
use wowsunpack::game_params::types::SkillPointCost;

use crate::builds::Item;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::stat_search::format_value;
use crate::stat_search::skills_fit;
use crate::stats::changes;
use crate::stats::derived_changes;
use crate::stats::stat_label;
use crate::text::clean;

/// Skills and upgrades combined at most, besides modules.
const MAX_ITEMS: usize = 3;

/// Combinations computed in full per search, after the estimate.
const MAX_CHECKS: usize = 600;

/// Matches listed, simplest first.
const MAX_MATCHES: usize = 8;

/// Builds listed per matching stat.
const BUILDS_PER_STAT: usize = 3;

/// Nearest values listed when nothing matches.
const NEAREST: usize = 5;

/// An estimate this close, relative to the number, is computed in full.
const ESTIMATE_WINDOW: f32 = 0.03;

/// Arguments for `explain_stat_value`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExplainStatValueArgs {
    /// Ship name, e.g. "Shimakaze".
    pub ship: String,

    /// The number to explain, as quoted, e.g. 21400 or 18.36. Its decimals set the rounding a match may have.
    pub value: f64,

    /// Unit of the number when the quote gives one. Omit to try every stat.
    pub unit: Option<ValueUnit>,

    /// Words naming the stat when the quote says which, e.g. "hp", "range", "rudder", "reload". Omit to try every stat.
    pub stat: Option<String>,
}

/// Unit of a quoted number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum ValueUnit {
    #[serde(rename = "hp")]
    Hp,
    #[serde(rename = "km")]
    Km,
    #[serde(rename = "m")]
    Meters,
    #[serde(rename = "s")]
    Seconds,
    #[serde(rename = "kn")]
    Knots,
    #[serde(rename = "%")]
    Percent,
    #[serde(rename = "mm")]
    Millimeters,
    #[serde(rename = "deg")]
    Degrees,
    #[serde(rename = "deg/s")]
    DegreesPerSecond,
    #[serde(rename = "m/s")]
    MetersPerSecond,
    #[serde(rename = "hp/min")]
    HpPerMinute,
}

impl ValueUnit {
    fn unit(self) -> Unit {
        match self {
            Self::Hp => Unit::Hp,
            Self::Km => Unit::Km,
            Self::Meters => Unit::Meters,
            Self::Seconds => Unit::Seconds,
            Self::Knots => Unit::Knots,
            Self::Percent => Unit::Percent,
            Self::Millimeters => Unit::Millimeters,
            Self::Degrees => Unit::Degrees,
            Self::DegreesPerSecond => Unit::DegreesPerSecond,
            Self::MetersPerSecond => Unit::MetersPerSecond,
            Self::HpPerMinute => Unit::HpPerMinute,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainStatValueOutput {
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: String,

    /// The number as understood, with its unit when given.
    pub value: String,

    pub rule: &'static str,

    /// Stats and builds that give the number, simplest build first.
    pub matches: Vec<ValueMatch>,

    /// With no match: the stats whose values come nearest, each on the build
    /// that comes nearest.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub nearest: Vec<NearValue>,

    /// Limits that cut the search short, or a stat hint that matched nothing.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValueMatch {
    /// Stat name as stat_search spells it, e.g. "survivability.hp".
    pub stat: String,

    /// What the stat measures, with its row, e.g. "Hit points".
    pub label: String,

    pub value: String,

    /// Stock modules, no captain skills or upgrades.
    pub base: String,

    /// Modules, captain skills and upgrades the value needs; empty for the
    /// base value. A skill marked "active" counts with its trigger met.
    pub build: Vec<String>,

    /// What each part of the build adds, e.g. "Survivability Expert +350 hp
    /// per tier (+3500)".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub because: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearValue {
    pub stat: String,
    pub label: String,
    pub value: String,
    pub build: Vec<String>,

    /// Value minus the number.
    pub difference: String,
}

/// Which stat a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StatId {
    Card(TtxStat),
    Derived(DerivedStat),
}

impl StatId {
    fn name(self) -> &'static str {
        match self {
            Self::Card(stat) => stat.field_key(),
            Self::Derived(stat) => stat.field_key(),
        }
    }

    fn unit(self) -> Unit {
        match self {
            Self::Card(stat) => wows_ship_stats::stat::unit(stat),
            Self::Derived(stat) => stat.unit(),
        }
    }

    fn label(self, game: &GameData) -> String {
        match self {
            Self::Card(stat) => clean(&stat_display_label(stat, game.provider())),
            Self::Derived(stat) => stat.label().to_owned(),
        }
    }
}

/// One row of a stat: per ammo type, consumable, launcher and so on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RowKey {
    stat: StatId,
    qualifier: Option<String>,
}

type Readings = HashMap<RowKey, f32>;

/// A skill or upgrade, or a skill with its trigger met.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Part {
    item: Item,
    active: bool,
}

/// Modules and items a value is read on.
#[derive(Debug, Clone, PartialEq)]
struct Build {
    selection: usize,
    parts: Vec<Part>,

    /// Skills taken only so every cheaper tier holds one; they leave the
    /// matched row as it is.
    fillers: Vec<Item>,
}

impl Build {
    fn items(&self) -> Vec<Item> {
        self.parts.iter().map(|part| part.item).chain(self.fillers.iter().copied()).collect()
    }

    fn triggered(&self) -> Option<Item> {
        self.parts.iter().find(|part| part.active).map(|part| part.item)
    }
}

/// The number to match, with the rounding its decimals allow.
#[derive(Debug, Clone, Copy)]
struct Target {
    value: f32,
    tolerance: f32,
}

impl Target {
    fn new(value: f64) -> Self {
        let text = value.to_string();
        let decimals = text.split_once('.').map_or(0, |(_, fraction)| fraction.len());
        let tolerance = 0.5 * 10f64.powi(-i32::try_from(decimals).unwrap_or(i32::MAX));

        Self { value: value as f32, tolerance: tolerance as f32 + 1e-4 }
    }

    /// The number read in `unit`: a quote in meters of a km stat, or a share
    /// quoted as a fraction of a percent stat.
    fn in_unit(self, unit: Unit, quoted: Option<Unit>) -> Option<Self> {
        let scaled = |factor: f32| Self { value: self.value * factor, tolerance: self.tolerance * factor };
        match (quoted, unit) {
            (None, _) => Some(self),
            (Some(quoted), unit) if quoted == unit => Some(self),
            (Some(Unit::Meters), Unit::Km) => Some(scaled(0.001)),
            (Some(Unit::Km), Unit::Meters) => Some(scaled(1000.0)),
            _ => None,
        }
    }

    fn matches(self, value: f32) -> bool {
        (value - self.value).abs() <= self.tolerance
    }

    fn near(self, estimate: f32) -> bool {
        (estimate - self.value).abs() <= self.tolerance.max(ESTIMATE_WINDOW * self.value.abs())
    }
}

/// One ship's search: its planner, the rows wanted and what is measured.
struct Search<'a> {
    game: &'a GameData,
    ship: &'a Ship,
    planner: Planner<'a>,
    stock_ranges: StockRanges,

    /// Rows the stat hint and unit allow, with the number in their unit.
    targets: HashMap<RowKey, Target>,

    /// Per module combination, the readings with no items.
    bare: Vec<Readings>,

    /// Per module combination, each part's readings alone, filled on demand.
    singles: Vec<Option<Vec<(Part, Readings)>>>,

    parts: Vec<Part>,
    checks: usize,

    /// Prefix for every build of this search, e.g. the special ability.
    assumption: Option<String>,
}

/// A build found to give a row the number, or to come nearest.
struct Found {
    row: RowKey,
    build: Build,
    value: f32,
    assumption: Option<String>,
}

impl<'a> Search<'a> {
    fn new(
        game: &'a GameData,
        ship: &'a Ship,
        planner: Planner<'a>,
        assumption: Option<String>,
    ) -> Result<Self, QueryError> {
        let parts = planner
            .items()
            .iter()
            .map(|&item| Part { item, active: false })
            .chain(planner.triggered_items().iter().map(|&item| Part { item, active: true }))
            .collect();
        let selections = planner.selections().count();

        Ok(Self {
            game,
            ship,
            stock_ranges: game.stock_ranges(ship)?,
            planner,
            targets: HashMap::new(),
            bare: Vec::new(),
            singles: vec![None; selections],
            parts,
            checks: 0,
            assumption,
        })
    }

    /// Every numeric row of `outcome`, derived rows read without mount
    /// facings.
    fn read(&self, outcome: &Outcome) -> Readings {
        let mut readings = Readings::new();
        for row in outcome.stats.rows() {
            if let Some(value) = row.value.as_f32() {
                readings.insert(RowKey { stat: StatId::Card(row.stat), qualifier: row.qualifier }, value);
            }
        }
        let derived = Derivation {
            ship: &self.ship.param,
            selection: self.planner.selection(outcome.selection),
            stats: &outcome.stats,
            modifiers: &outcome.bundle,
            merges_fire_zones: outcome.merges_fire_zones,
            stock_ranges: self.stock_ranges,
            provider: self.game.provider(),
            coverage: None,
        }
        .rows();
        for row in derived {
            readings.insert(RowKey { stat: StatId::Derived(row.stat), qualifier: row.qualifier }, row.value.as_f32());
        }
        readings.retain(|_, value| value.is_finite());

        readings
    }

    fn outcome(&self, build: &Build) -> Result<Rc<Outcome>, QueryError> {
        self.planner.with_items_triggered(build.selection, &build.items(), build.triggered())
    }

    /// The readings of each part alone on `selection`.
    fn singles(&mut self, selection: usize) -> Result<&[(Part, Readings)], QueryError> {
        if self.singles[selection].is_none() {
            let mut singles = Vec::new();
            for &part in &self.parts {
                let build = Build { selection, parts: vec![part], fillers: Vec::new() };
                let readings = self.read(&*self.outcome(&build)?);
                singles.push((part, readings));
            }
            self.singles[selection] = Some(singles);
        }

        Ok(self.singles[selection].as_deref().unwrap_or_default())
    }

    /// The first module combination with `row`, where item effects on it are
    /// measured.
    fn home(&self, row: &RowKey) -> Option<usize> {
        self.bare.iter().position(|readings| readings.contains_key(row))
    }

    /// Whether module combination `selection` changes `row` from stock, so a
    /// build on it is not a build on stock modules with modules named for
    /// nothing.
    fn module_matters(&self, selection: usize, row: &RowKey) -> bool {
        let stock = self.bare[0].get(row);
        let value = self.bare[selection].get(row);

        selection == 0 || stock.zip(value).is_none_or(|(stock, value)| (stock - value).abs() > 1e-4)
    }

    /// Parts that change `row`, each with its value alone on the row's home
    /// combination.
    fn movers(&mut self, row: &RowKey) -> Result<Vec<(Part, f32)>, QueryError> {
        let Some(home) = self.home(row) else {
            return Ok(Vec::new());
        };
        let base = self.bare[home][row];

        Ok(self
            .singles(home)?
            .iter()
            .filter_map(|(part, readings)| readings.get(row).map(|value| (*part, *value)))
            .filter(|(_, value)| (value - base).abs() > 1e-4)
            .collect())
    }

    /// `parts` valid together: one per item, one active skill, one upgrade
    /// per slot, and skills within the captain's points once fillers take
    /// every cheaper tier. Fillers leave `row` as it is when a skill of the
    /// tier can.
    fn complete(&self, selection: usize, parts: &[Part], movers: &[(Part, f32)]) -> Option<Build> {
        let mut slots: Vec<u8> = Vec::new();
        let mut costs: Vec<SkillPointCost> = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if parts[..index].iter().any(|other| other.item == part.item) {
                return None;
            }
            if let Some(slot) = self.planner.upgrade_slot(part.item) {
                if slots.contains(&slot) {
                    return None;
                }
                slots.push(slot);
            }
            if let Item::Skill { points, .. } = part.item {
                costs.push(points);
            }
        }
        if parts.iter().filter(|part| part.active).count() > 1 {
            return None;
        }

        let mut fillers = Vec::new();
        let highest = costs.iter().max().map_or(0, |cost| cost.get());
        for tier in 1..highest {
            let cost = SkillPointCost::new(tier);
            if costs.contains(&cost) {
                continue;
            }
            let of_tier = self.planner.items().iter().copied().filter(|item| {
                matches!(item, Item::Skill { points, .. } if *points == cost)
                    && !parts.iter().any(|part| part.item == *item)
            });
            let moves = |item: &Item| movers.iter().any(|(part, _)| !part.active && part.item == *item);
            let mut candidates: Vec<Item> = of_tier.collect();
            candidates.sort_by_key(|item| moves(item));
            let filler = candidates.first().copied()?;
            fillers.push(filler);
            costs.push(cost);
        }

        skills_fit(&costs).then(|| Build { selection, parts: parts.to_vec(), fillers })
    }

    /// Builds that give each wanted row the number, fewest parts first; a
    /// row matched with fewer parts is not searched with more.
    fn run(&mut self, notes: &mut Vec<String>) -> Result<Vec<Found>, QueryError> {
        for selection in 0..self.singles.len() {
            let build = Build { selection, parts: Vec::new(), fillers: Vec::new() };
            let readings = self.read(&*self.outcome(&build)?);
            self.bare.push(readings);
        }

        let mut rows: Vec<RowKey> = self.targets.keys().cloned().collect();
        rows.sort_by(|left, right| (left.stat.name(), &left.qualifier).cmp(&(right.stat.name(), &right.qualifier)));

        let mut selections: Vec<usize> = (0..self.bare.len()).collect();
        selections.sort_by_key(|&selection| self.planner.selection_modules(selection).len());

        let mut found: Vec<Found> = Vec::new();
        for &selection in &selections {
            for row in &rows {
                if let Some(&value) = self.bare[selection].get(row)
                    && self.module_matters(selection, row)
                    && self.targets[row].matches(value)
                    && found.iter().filter(|found| found.row == *row).count() < BUILDS_PER_STAT
                {
                    let build = Build { selection, parts: Vec::new(), fillers: Vec::new() };
                    found.push(Found { row: row.clone(), build, value, assumption: self.assumption.clone() });
                }
            }
        }

        for size in 1..=MAX_ITEMS {
            let open: Vec<RowKey> =
                rows.iter().filter(|row| !found.iter().any(|found| found.row == **row)).cloned().collect();
            let mut candidates: Vec<(usize, f32, RowKey, Build)> = Vec::new();
            for row in &open {
                let movers = self.movers(row)?;
                let Some(home) = self.home(row) else {
                    continue;
                };
                let home_base = self.bare[home][row];
                let target = self.targets[row];
                let mut indices: Vec<usize> = (0..size).collect();
                if movers.len() < size {
                    continue;
                }
                loop {
                    let chosen: Vec<(Part, f32)> = indices.iter().map(|&index| movers[index]).collect();
                    for &selection in &selections {
                        let Some(&base) = self.bare[selection].get(row) else {
                            continue;
                        };
                        if !self.module_matters(selection, row) {
                            continue;
                        }
                        let product = chosen.iter().fold(base, |value, (_, alone)| value * ratio(*alone, home_base));
                        let sum = chosen.iter().fold(base, |value, (_, alone)| value + (alone - home_base));
                        if !(target.near(product) || target.near(sum)) {
                            continue;
                        }
                        let parts: Vec<Part> = chosen.iter().map(|(part, _)| *part).collect();
                        if let Some(build) = self.complete(selection, &parts, &movers) {
                            let miss = (product - target.value).abs().min((sum - target.value).abs());
                            let cost = self.planner.selection_modules(selection).len() + size;
                            candidates.push((cost, miss, row.clone(), build));
                        }
                    }
                    if !crate::stat_search::next_combination(&mut indices, movers.len()) {
                        break;
                    }
                }
            }

            candidates.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.total_cmp(&right.1)));
            for (_, _, row, build) in candidates {
                if self.checks >= MAX_CHECKS {
                    notes.push(format!(
                        "The search stopped after computing {MAX_CHECKS} builds in full; a build with more items may also give the number."
                    ));
                    return Ok(found);
                }
                if found.iter().filter(|found| found.row == row).count() >= BUILDS_PER_STAT {
                    continue;
                }
                self.checks += 1;
                let readings = self.read(&*self.outcome(&build)?);
                if let Some(&value) = readings.get(&row)
                    && self.targets[&row].matches(value)
                {
                    found.push(Found { row, build, value, assumption: self.assumption.clone() });
                }
            }
        }

        Ok(found)
    }

    /// The build of each wanted row whose value comes nearest the number,
    /// among the builds read.
    fn nearest(&mut self) -> Result<Vec<Found>, QueryError> {
        let mut homes: Vec<usize> = self.targets.keys().filter_map(|row| self.home(row)).collect();
        homes.sort_unstable();
        homes.dedup();
        for &home in &homes {
            self.singles(home)?;
        }

        let bare = self.bare.iter().enumerate().flat_map(|(selection, readings)| {
            readings
                .iter()
                .map(move |(row, value)| (row, Build { selection, parts: Vec::new(), fillers: Vec::new() }, *value))
        });
        let singles = homes.iter().flat_map(|&home| {
            self.singles[home].iter().flatten().flat_map(move |(part, readings)| {
                readings.iter().map(move |(row, value)| {
                    (row, Build { selection: home, parts: vec![*part], fillers: Vec::new() }, *value)
                })
            })
        });

        let mut nearest: HashMap<RowKey, Found> = HashMap::new();
        for (row, build, value) in bare.chain(singles) {
            let Some(target) = self.targets.get(row) else {
                continue;
            };
            let miss = (value - target.value).abs();
            if nearest.get(row).is_none_or(|current| (current.value - target.value).abs() > miss) {
                let found = Found { row: row.clone(), build, value, assumption: self.assumption.clone() };
                nearest.insert(row.clone(), found);
            }
        }

        Ok(nearest.into_values().collect())
    }

    fn build_names(&self, build: &Build) -> Vec<String> {
        let mut names: Vec<String> = self.assumption.iter().cloned().collect();
        names.extend(self.planner.selection_modules(build.selection).iter().cloned());
        names.extend(build.parts.iter().map(|part| {
            let label = self.planner.label(part.item);
            if part.active { format!("{label} (active)") } else { label }
        }));

        names
    }

    /// Each module, skill and upgrade of `build` with what it adds to `row`.
    fn because(&self, row: &RowKey, build: &Build) -> Result<Vec<String>, QueryError> {
        let unit = row.stat.unit();
        let mut because = Vec::new();
        if build.selection != 0 {
            let base = self.bare[0].get(row);
            let with = self.bare[build.selection].get(row);
            if let (Some(base), Some(with)) = (base, with) {
                because.push(format!(
                    "{}: {} -> {}",
                    self.planner.selection_modules(build.selection).join(", "),
                    format_value(*base, unit),
                    format_value(*with, unit),
                ));
            }
        }
        if build.parts.is_empty() {
            return Ok(because);
        }

        let loadout = self.planner.loadout_of(&build.items(), build.triggered());
        let selection = self.planner.selection(build.selection);
        let baseline = self.game.card_with(self.ship, &self.game.stock_loadout(), selection)?;
        let card = self.game.card_with(self.ship, &loadout, selection)?;
        let rows = card.stats.rows();
        let attributions = render_attributions(&card.provenance, &rows, self.game.provider(), self.game.provider());
        let label = stat_label(&self.label_text(row.stat), row.qualifier.as_deref(), self.game);
        let mut moved = match row.stat {
            StatId::Card(stat) => {
                changes(&baseline.stats, &card.stats, &attributions, &loadout, self.game, |candidate| candidate == stat)
            }
            StatId::Derived(_) => {
                derived_changes(&baseline.derived, &card.derived, &attributions, &loadout, self.game, self.ship)
            }
        };
        moved.retain(|change| change.stat == label);
        because.extend(moved.into_iter().flat_map(|change| change.because));

        Ok(because)
    }

    /// The label the stat card renders for `stat`, which change lists use.
    fn label_text(&self, stat: StatId) -> String {
        match stat {
            StatId::Card(stat) => stat_display_label(stat, self.game.provider()),
            StatId::Derived(stat) => stat.label().to_owned(),
        }
    }

    fn row_label(&self, row: &RowKey) -> String {
        let label = row.stat.label(self.game);
        match row.qualifier.as_deref() {
            Some(qualifier) if row.stat.name().starts_with("consumable.") => {
                let name = self.game.consumable_row_name(self.ship, qualifier).unwrap_or_else(|| qualifier.to_owned());
                format!("{label} ({name})")
            }
            Some(qualifier) => format!("{label} ({})", self.game.qualifier_name(qualifier)),
            None => label,
        }
    }
}

/// `value` as a share of `base`; 1 when the base is zero.
fn ratio(value: f32, base: f32) -> f32 {
    if base.abs() <= f32::EPSILON { 1.0 } else { value / base }
}

/// Whether `row` is the stat `words` name: every word appears in its name
/// or label.
fn named(row: &RowKey, words: &[String], game: &GameData) -> bool {
    let text = format!("{} {}", row.stat.name(), row.stat.label(game)).to_ascii_lowercase().replace(['_', '.'], " ");
    words.iter().all(|word| text.contains(word.as_str()))
}

impl GameData {
    /// Which stats of a ship a quoted number can be, and the builds that
    /// give it.
    pub fn explain_stat_value(&self, args: &ExplainStatValueArgs) -> Result<ExplainStatValueOutput, QueryError> {
        const RULE: &str = "A match is a stat of this ship whose value rounds to the number at its decimals, on the stock modules or another module combination, alone or with up to three captain skills (21 points, a skill on every cheaper tier) and upgrades (one per slot); conditional skills count with their trigger met only where marked active, and the ship's special ability only where named. Fewer assumptions rank first. Signals, camouflage and commander talents are not searched.";

        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let target = Target::new(args.value);
        let quoted = args.unit.map(ValueUnit::unit);
        let words: Vec<String> = args
            .stat
            .as_deref()
            .map(|stat| stat.to_ascii_lowercase().split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();
        let mut notes = Vec::new();

        let ability = match self.active_ability(ship) {
            Ok(ability) => Some(ability),
            Err(QueryError::NoSpecialAbility { .. }) => None,
            Err(error) => return Err(error),
        };
        let mut searches = vec![Search::new(self, ship, Planner::new(self, ship), None)?];
        if let Some(ability) = &ability {
            let switched = ability.switched.as_ref().unwrap_or(ship);
            let planner = Planner::new(self, switched).with_ability(ability.modifiers.clone(), ability.burst);
            searches.push(Search::new(self, switched, planner, Some(ability.names.join(", ")))?);
        }

        let mut found: Vec<(usize, Found)> = Vec::new();
        for (index, search) in searches.iter_mut().enumerate() {
            let mut rows: HashSet<RowKey> = HashSet::new();
            for selection in 0..search.singles.len() {
                rows.extend(search.read(&*search.planner.with_items_on(selection, &[])?).into_keys());
            }
            let named_rows: Vec<RowKey> = rows.iter().filter(|row| named(row, &words, self)).cloned().collect();
            let rows: Vec<RowKey> = if named_rows.is_empty() {
                if index == 0 && !words.is_empty() {
                    notes.push(format!("No stat matches {:?}; every stat was searched.", words.join(" ")));
                }
                rows.into_iter().collect()
            } else {
                named_rows
            };
            search.targets = rows
                .into_iter()
                .filter_map(|row| target.in_unit(row.stat.unit(), quoted).map(|target| (row, target)))
                .collect();
            found.extend(search.run(&mut notes)?.into_iter().map(|found| (index, found)));
        }

        // A build that matches without the special ability does not need it.
        let without_ability: Vec<(RowKey, Build)> = found
            .iter()
            .filter(|(index, _)| *index == 0)
            .map(|(_, found)| (found.row.clone(), found.build.clone()))
            .collect();
        found.retain(|(index, found)| {
            *index == 0 || !without_ability.iter().any(|(row, build)| *row == found.row && *build == found.build)
        });

        let cost = |(index, found): &(usize, Found)| {
            searches[*index].planner.selection_modules(found.build.selection).len()
                + found.build.parts.len()
                + found.build.parts.iter().filter(|part| part.active).count()
                + usize::from(found.assumption.is_some())
        };
        found.sort_by(|left, right| {
            cost(left)
                .cmp(&cost(right))
                .then(
                    matches!(left.1.row.stat, StatId::Derived(_)).cmp(&matches!(right.1.row.stat, StatId::Derived(_))),
                )
                .then(left.1.row.stat.name().cmp(right.1.row.stat.name()))
        });
        found.truncate(MAX_MATCHES);

        let mut matches = Vec::new();
        for (index, found) in &found {
            let search = &searches[*index];
            let unit = found.row.stat.unit();
            let base = search.bare[0]
                .get(&found.row)
                .or_else(|| search.home(&found.row).map(|home| &search.bare[home][&found.row]));
            matches.push(ValueMatch {
                stat: found.row.stat.name().to_owned(),
                label: search.row_label(&found.row),
                value: format_value(found.value, unit),
                base: base.map(|base| format_value(*base, unit)).unwrap_or_default(),
                build: search.build_names(&found.build),
                because: search.because(&found.row, &found.build)?,
            });
        }

        let mut nearest = Vec::new();
        if matches.is_empty() {
            let mut near: Vec<(usize, Found)> = Vec::new();
            for (index, search) in searches.iter_mut().enumerate() {
                near.extend(search.nearest()?.into_iter().map(|found| (index, found)));
            }
            let miss = |(index, found): &(usize, Found)| {
                let target = searches[*index].targets[&found.row].value;
                (found.value - target).abs() / target.abs().max(f32::EPSILON)
            };
            near.sort_by(|left, right| miss(left).total_cmp(&miss(right)));
            for (index, found) in near.into_iter().take(NEAREST) {
                let search = &searches[index];
                let unit = found.row.stat.unit();
                let target = search.targets[&found.row].value;
                nearest.push(NearValue {
                    stat: found.row.stat.name().to_owned(),
                    label: search.row_label(&found.row),
                    value: format_value(found.value, unit),
                    build: search.build_names(&found.build),
                    difference: format!("{:+}{}", crate::builds::round(found.value - target), unit.suffix()),
                });
            }
        }

        let value = match quoted {
            Some(unit) => format!("{}{}", args.value, unit.suffix()),
            None => args.value.to_string(),
        };

        Ok(ExplainStatValueOutput {
            interpretations,
            ship: ship.listing_label(),
            value,
            rule: RULE,
            matches,
            nearest,
            notes,
        })
    }
}
