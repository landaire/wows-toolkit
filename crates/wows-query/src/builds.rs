//! Stats as players mean them: on the build they would run.
//!
//! Each build-dependent stat reports its `base` (stock modules, no captain
//! skills or upgrades), its `best` value with the modules, skills and upgrades
//! assumed, and `alternatives` such as the same build without one of them.
//!
//! The choice follows the build analysis of shipbuilds.com
//! (`crates/core/src/analysis/rules.rs` of the wowsbuilds repository): a skill
//! or upgrade counts for a stat only when its GameParams modifiers change that
//! stat for this ship's class, battery and calibre, which the stat engine
//! decides by applying them; skills come from the class's skill tree, upgrades
//! must fit the ship and one is taken per slot. Two of its rules decide when a
//! skill that does change a stat is still not part of the best-fit build: Inertia
//! Fuse for HE Shells only when it lifts a shell over an armor threshold it
//! misses without it (`ifhe`, see [`crate::shells`]), and extra consumable
//! charges only when they can be used within a match
//! (`unburnable_consumable_charge`). Conditional skills count only as
//! alternatives, each with only its own trigger met.
//!
//! The client lets a captain learn a skill only with one learned on every
//! cheaper tier (`CommonCrew.hasLearnedSkillOnTier`), so a build that takes a
//! skill also takes, for each cheaper tier it leaves empty, the skill of that
//! tier that leaves the stat best, listed under `prerequisites`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::rc::Rc;

use serde::Serialize;
use serde::Serializer;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_core::units::Seconds;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::model::ShipStats;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::CrewSkill;
use wowsunpack::game_params::types::SkillPointCost;

use crate::data::GameData;
use crate::error::QueryError;
use crate::loadout::AppliedSkill;
use crate::loadout::AppliedUpgrade;
use crate::loadout::ResolvedLoadout;
use crate::loadout::Triggers;
use crate::modules::ModuleKind;
use crate::ships::Ship;
use crate::stat_search::skills_fit;
use crate::upgrades::Upgrade;

/// Two stat values closer than this are equal.
const EPSILON: f32 = 1e-4;

/// A stat value with a unit, comparable as a number.
pub trait StatUnit: Copy + Serialize {
    /// Unit symbol reported next to the value.
    const UNIT: &'static str;

    fn amount(self) -> f32;
}

impl StatUnit for Km {
    const UNIT: &'static str = "km";

    fn amount(self) -> f32 {
        self.value()
    }
}

impl StatUnit for Meters {
    const UNIT: &'static str = "m";

    fn amount(self) -> f32 {
        self.value()
    }
}

impl StatUnit for Millimeters {
    const UNIT: &'static str = "mm";

    fn amount(self) -> f32 {
        self.value()
    }
}

impl StatUnit for Seconds {
    const UNIT: &'static str = "s";

    fn amount(self) -> f32 {
        self.value()
    }
}

/// Number of charges, or unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charges {
    Limited(u32),
    Unlimited,
}

impl Serialize for Charges {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Limited(count) => serializer.serialize_u32(*count),
            Self::Unlimited => serializer.serialize_str("unlimited"),
        }
    }
}

impl StatUnit for Charges {
    const UNIT: &'static str = "charges";

    fn amount(self) -> f32 {
        match self {
            // Precision is not a concern for charge counts.
            #[allow(clippy::cast_precision_loss)]
            Self::Limited(count) => count as f32,
            Self::Unlimited => f32::INFINITY,
        }
    }
}

/// One build-dependent stat.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildStat<V> {
    /// What the value measures, e.g. "detectability by ships".
    pub stat: String,
    pub unit: &'static str,
    pub builds: Builds<V>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Builds<V> {
    /// Stock modules, no captain skills or upgrades.
    pub base: V,

    /// The best-fit build for this stat.
    pub best: BuildValue<V>,

    /// Other builds players run, and builds that change the stat further at a
    /// cost the best-fit build does not pay.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<BuildAlternative<V>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildValue<V> {
    pub value: V,

    /// Modules, captain skills and upgrades assumed. Empty when nothing
    /// changes the stat.
    pub assumed: Vec<String>,

    /// Captain skills taken only because a skill in `assumed` needs one on
    /// every cheaper tier, each the skill of its tier that leaves the stat
    /// best.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BuildAlternative<V> {
    /// e.g. "without Concealment Expert".
    pub label: String,
    pub value: V,
    pub assumed: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Which way a stat improves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Better {
    Lower,
    Higher,
}

impl Better {
    /// Whether `candidate` improves on `current`.
    pub(crate) fn improves(self, candidate: f32, current: f32) -> bool {
        match self {
            Self::Lower => candidate < current - EPSILON,
            Self::Higher => candidate > current + EPSILON,
        }
    }
}

/// A captain skill of the ship's class tree or an upgrade that fits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Item {
    /// Index into the indexed skills, and its point cost in the class tree.
    Skill { index: usize, points: SkillPointCost },

    /// Index into the indexed upgrades.
    Upgrade(usize),
}

/// A ship's stat card and modifiers under one build.
pub(crate) struct Outcome {
    pub(crate) stats: ShipStats,
    pub(crate) bundle: ModifierBundle,
}

/// Why the best-fit build leaves out an item that changes the stat.
pub(crate) struct Exclusion {
    pub(crate) item: Item,
    pub(crate) note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OutcomeKey {
    selection: usize,
    items: Vec<Item>,

    /// Index into the indexed skills of the skill whose triggers are met.
    triggered: Option<usize>,
}

/// The items of one build: those taken for the stat, and the skills their
/// tiers need.
struct Build {
    items: Vec<Item>,
    prerequisites: Vec<Item>,
}

impl Build {
    fn all(&self) -> Vec<Item> {
        self.items.iter().chain(&self.prerequisites).copied().collect()
    }
}

/// The builds of one ship, with the outcomes computed so far.
pub(crate) struct Planner<'a> {
    game: &'a GameData,
    ship: &'a Ship,

    /// Module combinations that fit, stock first, each with the names of
    /// its modules that are not stock.
    selections: Vec<(ShipUpgradeSelection, Vec<String>)>,

    items: Vec<Item>,

    /// Skills with a trigger condition, as alternatives with it met.
    triggered: Vec<Item>,

    outcomes: RefCell<HashMap<OutcomeKey, Rc<Outcome>>>,
}

/// The value of a stat under one build, `None` when the ship lacks it.
pub(crate) type Extract<'f, V> = &'f dyn Fn(&Outcome) -> Option<V>;

/// A stat to plan for.
pub(crate) struct StatPlan<'f, V> {
    pub(crate) label: String,
    pub(crate) better: Better,
    pub(crate) extract: Extract<'f, V>,

    /// Whether module choices may change the stat.
    pub(crate) modules: bool,

    /// Whether to list conditional skills with their trigger met. The
    /// stat engine applies triggered modifiers to every consumable alike, so
    /// consumable stats leave them out.
    pub(crate) triggered: bool,

    pub(crate) exclude: Option<Exclude<'f, V>>,
}

/// Items the best-fit build leaves out although they change the stat, given
/// the value of each item alone.
pub(crate) type Exclude<'f, V> = &'f dyn Fn(&Planner<'_>, &[(Item, V)]) -> Result<Vec<Exclusion>, QueryError>;

impl<'f, V> StatPlan<'f, V> {
    /// A stat module choices and conditional skills may change.
    pub(crate) fn new(label: String, better: Better, extract: Extract<'f, V>) -> Self {
        Self { label, better, extract, modules: true, triggered: true, exclude: None }
    }

    /// For consumable stats: the ship's modules do not change them.
    pub(crate) fn for_consumable(mut self) -> Self {
        self.modules = false;
        self.triggered = false;
        self
    }
}

impl<'a> Planner<'a> {
    pub(crate) fn new(game: &'a GameData, ship: &'a Ship) -> Self {
        let skill_items = game.skills.iter().enumerate().filter_map(|(index, skill)| {
            game.skill_points(skill, ship.class)
                .map(|points| (Item::Skill { index, points: SkillPointCost::new(points) }, skill))
        });
        let mut items = Vec::new();
        let mut triggered = Vec::new();
        for (item, skill) in skill_items {
            if has_trigger(skill) {
                triggered.push(item);
            }
            items.push(item);
        }
        items.extend(
            game.upgrades
                .iter()
                .enumerate()
                .filter(|(_, upgrade)| upgrade.fits(ship))
                .map(|(index, _)| Item::Upgrade(index)),
        );

        Self {
            game,
            ship,
            selections: game.module_combinations(ship),
            items,
            triggered,
            outcomes: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn ship(&self) -> &Ship {
        self.ship
    }

    /// Items whose English names are in `names`.
    pub(crate) fn items_named(&self, names: &[String]) -> Vec<Item> {
        self.items.iter().copied().filter(|item| names.contains(&self.name(*item))).collect()
    }

    /// English name of an item.
    pub(crate) fn name(&self, item: Item) -> String {
        match item {
            Item::Skill { index, .. } => self.game.skill_name(&self.game.skills[index]),
            Item::Upgrade(index) => self.game.upgrades[index].name.clone(),
        }
    }

    fn upgrade(&self, item: Item) -> Option<&Upgrade> {
        match item {
            Item::Upgrade(index) => Some(&self.game.upgrades[index]),
            Item::Skill { .. } => None,
        }
    }

    /// The outcome of the stock modules and no items.
    pub(crate) fn stock(&self) -> Result<Rc<Outcome>, QueryError> {
        self.outcome(0, &[], None)
    }

    /// The outcome of the stock modules with `items`.
    pub(crate) fn with_items(&self, items: &[Item]) -> Result<Rc<Outcome>, QueryError> {
        self.outcome(0, items, None)
    }

    /// The outcome of `items` on module combination `selection`, with the
    /// triggers of the skill at index `triggered` met.
    fn outcome(&self, selection: usize, items: &[Item], triggered: Option<usize>) -> Result<Rc<Outcome>, QueryError> {
        let mut sorted = items.to_vec();
        sorted.sort_unstable();
        let key = OutcomeKey { selection, items: sorted, triggered };
        if let Some(outcome) = self.outcomes.borrow().get(&key) {
            return Ok(Rc::clone(outcome));
        }

        let loadout = self.loadout(&key.items, triggered);
        let effects = self.game.effective_modifiers(self.ship, &loadout)?;
        let stats = effects.modifiers.stats(
            &self.ship.param,
            &self.selections[selection].0,
            self.ship.tier.0,
            self.game.provider(),
        );
        let outcome = Rc::new(Outcome { stats, bundle: effects.modifiers.bundle().clone() });
        self.outcomes.borrow_mut().insert(key, Rc::clone(&outcome));
        Ok(outcome)
    }

    fn loadout(&self, items: &[Item], triggered: Option<usize>) -> ResolvedLoadout {
        let mut loadout = self.game.stock_loadout();
        if let Some(index) = triggered {
            loadout.triggers = Triggers::Skill(self.game.skills[index].internal_name().clone());
        }
        for &item in items {
            match item {
                Item::Skill { index, points } => {
                    let skill = &self.game.skills[index];
                    let name = self.game.skill_name(skill);
                    loadout.labels.insert(skill.internal_name().as_str().to_owned(), name.clone());
                    loadout.applied_skills.push(AppliedSkill { name, points: points.get() });
                    loadout.skills.push(skill.clone());
                }
                Item::Upgrade(index) => {
                    let upgrade = &self.game.upgrades[index];
                    loadout.labels.insert(upgrade.param.name().to_owned(), upgrade.name.clone());
                    loadout
                        .modernizations
                        .push((upgrade.param.name().to_owned(), upgrade.modernization().modifiers().to_vec()));
                    loadout
                        .applied_upgrades
                        .push(AppliedUpgrade { name: upgrade.name.clone(), slot: upgrade.port_slot() });
                }
            }
        }
        loadout
    }

    /// Base, best-fit build and alternatives of one stat. `None` when the
    /// ship lacks the stat.
    pub(crate) fn plan<V: StatUnit>(&self, plan: &StatPlan<'_, V>) -> Result<Option<BuildStat<V>>, QueryError> {
        let value = |outcome: &Outcome| (plan.extract)(outcome);
        let Some(base) = value(&*self.stock()?) else {
            return Ok(None);
        };

        let mut selection = 0;
        let mut start = base;
        if plan.modules {
            for index in 1..self.selections.len() {
                if let Some(candidate) = value(&*self.outcome(index, &[], None)?)
                    && plan.better.improves(candidate.amount(), start.amount())
                {
                    selection = index;
                    start = candidate;
                }
            }
        }

        let mut singles: Vec<(Item, V)> = Vec::new();
        for &item in &self.items {
            if let Some(candidate) = value(&*self.outcome(selection, &[item], None)?)
                && plan.better.improves(candidate.amount(), start.amount())
            {
                singles.push((item, candidate));
            }
        }
        let exclusions = match plan.exclude {
            Some(exclude) => exclude(self, &singles)?,
            None => Vec::new(),
        };
        let excluded: Vec<Item> = exclusions.iter().map(|exclusion| exclusion.item).collect();
        let chosen = self.complete(selection, self.choose(&singles, &excluded, plan.better), plan, &excluded)?;

        let best_value = value(&*self.outcome(selection, &chosen.all(), None)?).unwrap_or(start);
        let modules = &self.selections[selection].1;
        let names = |items: &[Item]| -> Vec<String> { items.iter().map(|item| self.name(*item)).collect() };
        let assumed = |build: &Build| -> Vec<String> { modules.iter().cloned().chain(names(&build.items)).collect() };
        let alternative = |label: String, value: V, build: &Build, note: Option<String>| BuildAlternative {
            label,
            value,
            assumed: assumed(build),
            prerequisites: names(&build.prerequisites),
            note,
        };

        let mut alternatives = Vec::new();
        for &item in &chosen.items {
            let rest = chosen.items.iter().copied().filter(|other| *other != item).collect();
            let avoid: Vec<Item> = excluded.iter().copied().chain([item]).collect();
            let without = self.complete(selection, rest, plan, &avoid)?;
            if let Some(candidate) = value(&*self.outcome(selection, &without.all(), None)?)
                && (candidate.amount() - best_value.amount()).abs() > EPSILON
            {
                alternatives.push(alternative(format!("without {}", self.name(item)), candidate, &without, None));
            }
        }
        for exclusion in &exclusions {
            let Some(items) = self.with_item(&chosen.items, exclusion.item) else {
                continue;
            };
            let with = self.complete(selection, items, plan, &excluded)?;
            if let Some(candidate) = value(&*self.outcome(selection, &with.all(), None)?) {
                let label = format!("with {}", self.name(exclusion.item));
                alternatives.push(alternative(label, candidate, &with, Some(exclusion.note.clone())));
            }
        }
        let triggered: &[Item] = if plan.triggered { &self.triggered } else { &[] };
        for &item in triggered {
            let Item::Skill { index, .. } = item else {
                continue;
            };
            // Skip a skill whose trigger leaves the stat as it is.
            let idle = value(&*self.outcome(selection, &[item], None)?);
            let active = value(&*self.outcome(selection, &[item], Some(index))?);
            if idle.zip(active).is_none_or(|(idle, active)| (idle.amount() - active.amount()).abs() <= EPSILON) {
                continue;
            }

            let Some(items) = self.with_item(&chosen.items, item) else {
                continue;
            };
            let with = self.complete(selection, items, plan, &excluded)?;
            if let Some(candidate) = value(&*self.outcome(selection, &with.all(), Some(index))?)
                && plan.better.improves(candidate.amount(), best_value.amount())
            {
                let label = format!("with {} active", self.name(item));
                alternatives.push(alternative(label, candidate, &with, self.trigger_sentence(item)));
            }
        }

        let best =
            BuildValue { value: best_value, assumed: assumed(&chosen), prerequisites: names(&chosen.prerequisites) };
        Ok(Some(BuildStat { stat: plan.label.clone(), unit: V::UNIT, builds: Builds { base, best, alternatives } }))
    }

    /// Improving skills that fit the captain's points with a skill on every
    /// cheaper tier, most improving first, and the most improving upgrade of
    /// each slot.
    fn choose<V: StatUnit>(&self, singles: &[(Item, V)], excluded: &[Item], better: Better) -> Vec<Item> {
        let gain = |value: V| match better {
            Better::Lower => -value.amount(),
            Better::Higher => value.amount(),
        };
        let mut ranked: Vec<&(Item, V)> = singles.iter().filter(|(item, _)| !excluded.contains(item)).collect();
        ranked.sort_by(|left, right| gain(right.1).total_cmp(&gain(left.1)).then(left.0.cmp(&right.0)));

        let mut chosen = Vec::new();
        let mut costs: Vec<SkillPointCost> = Vec::new();
        let mut slots: BTreeMap<u8, Item> = BTreeMap::new();
        for (item, _) in ranked {
            match *item {
                Item::Skill { points, .. } => {
                    costs.push(points);
                    if skills_fit(&costs) {
                        chosen.push(*item);
                    } else {
                        costs.pop();
                    }
                }
                Item::Upgrade(index) => {
                    slots.entry(self.game.upgrades[index].slot).or_insert(*item);
                }
            }
        }
        chosen.extend(slots.into_values());
        chosen
    }

    /// `items` with a skill on every cheaper tier its skills leave empty: of
    /// each such tier, the skill outside `avoid` that leaves the stat best,
    /// the first in skill order on a tie.
    fn complete<V: StatUnit>(
        &self,
        selection: usize,
        items: Vec<Item>,
        plan: &StatPlan<'_, V>,
        avoid: &[Item],
    ) -> Result<Build, QueryError> {
        let mut build = Build { items, prerequisites: Vec::new() };
        let costs = skill_costs(&build.items);
        let Some(highest) = costs.iter().max().copied() else {
            return Ok(build);
        };

        for tier in 1..highest.get() {
            let cost = SkillPointCost::new(tier);
            if costs.contains(&cost) {
                continue;
            }

            let mut with = build.all();
            let mut best: Option<(Item, f32)> = None;
            for &candidate in &self.items {
                let Item::Skill { points, .. } = candidate else {
                    continue;
                };
                if points != cost || avoid.contains(&candidate) {
                    continue;
                }
                with.push(candidate);
                let outcome = self.outcome(selection, &with, None)?;
                with.pop();
                let Some(value) = (plan.extract)(&outcome) else {
                    continue;
                };
                if best.is_none_or(|(_, current)| plan.better.improves(value.amount(), current)) {
                    best = Some((candidate, value.amount()));
                }
            }
            if let Some((skill, _)) = best {
                build.prerequisites.push(skill);
            }
        }

        Ok(build)
    }

    /// `items` with `item` added in place of an upgrade in its slot, or
    /// `None` when its skills exceed the captain's points.
    fn with_item(&self, items: &[Item], item: Item) -> Option<Vec<Item>> {
        let slot = self.upgrade(item).map(|upgrade| upgrade.slot);
        let mut with: Vec<Item> = items
            .iter()
            .copied()
            .filter(|other| *other != item)
            .filter(|other| slot.is_none() || self.upgrade(*other).map(|upgrade| upgrade.slot) != slot)
            .collect();
        with.push(item);

        skills_fit(&skill_costs(&with)).then_some(with)
    }

    fn trigger_sentence(&self, item: Item) -> Option<String> {
        let Item::Skill { index, .. } = item else {
            return None;
        };
        self.game.skill_trigger_sentence(&self.game.skills[index])
    }
}

fn skill_costs(items: &[Item]) -> Vec<SkillPointCost> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Skill { points, .. } => Some(*points),
            Item::Upgrade(_) => None,
        })
        .collect()
}

fn has_trigger(skill: &CrewSkill) -> bool {
    skill.logic_trigger().and_then(|trigger| trigger.trigger_type()).is_some_and(|kind| !kind.is_empty())
}

/// Two decimals, enough for every port value.
pub(crate) fn round(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

impl GameData {
    /// Module combinations of `ship` whose guns and torpedoes fit the hull,
    /// stock first. Every combination is kept: a ship has at most a few
    /// modules of each kind.
    fn module_combinations(&self, ship: &Ship) -> Vec<(ShipUpgradeSelection, Vec<String>)> {
        let stock = ShipUpgradeSelection::stock(&ship.param);
        let modules = self.ship_modules(ship);
        let mut combinations: Vec<(ShipUpgradeSelection, Vec<String>)> = vec![(stock, Vec::new())];
        for kind in ModuleKind::ALL {
            let options: Vec<_> = modules.iter().filter(|module| module.kind == kind && !module.stock).collect();
            let mut extended = Vec::new();
            for (selection, names) in &combinations {
                for module in &options {
                    let mut selection = selection.clone();
                    module.kind.set(&mut selection, module.param.clone());
                    let mut names = names.clone();
                    names.push(module.name.clone());
                    extended.push((selection, names));
                }
            }
            combinations.extend(extended);
        }

        combinations.retain(|(selection, _)| self.hull_fits(ship, selection));
        combinations
    }
}
