//! Torpedo options of a class and tier group ranked by a weighted judgment
//! over every factor that decides how much damage they land, each factor
//! ranked within the group in the direction that is better for it.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wowsunpack::game_params::keys::ComponentType;

use crate::armor_assessment::class_plural;
use crate::armor_assessment::parallel_map;
use crate::armor_trace::join_and;
use crate::builds::Better;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::comparison::Amount;
use crate::comparison::FactorValues;
use crate::comparison::GroupRank;
use crate::comparison::MountArcs;
use crate::comparison::TorpedoFactor;
use crate::comparison::number;
use crate::comparison::round2;
use crate::comparison::tier_label;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::ship_variants::listing_order;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipCounts;
use crate::ships::ShipFilter;
use crate::ships::Tier;
use crate::ships::TierRange;

/// Arguments for `torpedo_rankings`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TorpedoRankingsArgs {
    /// A ship whose torpedoes to judge: lists only its options, ranked among its class and tier unless class or tiers are given.
    pub ship: Option<String>,

    /// destroyer, cruiser or battleship. Omit to rank the torpedoes of all three in one group. Cruisers and battleships carry torpedoes as close-range secondary weapons, so for questions about torpedoes in general give destroyer.
    pub class: Option<ShipClass>,

    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,

    /// Nation as GameParams spells it, e.g. "USA", "United_Kingdom". Lists only that nation's torpedoes, still ranked among every nation's. A premium replica of another ship ranks as its original and is not listed apart.
    pub nation: Option<String>,

    /// best (the default) or worst first.
    pub order: Option<TorpedoOrder>,

    /// Most torpedo options listed. Defaults to 10.
    pub limit: Option<usize>,

    /// Rank ships in closed testing too. Defaults to false.
    pub include_test: Option<bool>,
}

/// Which end of the ranking is listed first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TorpedoOrder {
    #[default]
    Best,
    Worst,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorpedoRankingsOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// e.g. "tier 10 destroyers".
    pub group: String,

    pub classes: Vec<ShipClass>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<TierRange>,

    /// Ships of the group that carry torpedoes.
    pub ships: ShipCounts,

    /// Torpedo options ranked: one per torpedo a ship can carry.
    pub options: usize,

    pub order: TorpedoOrder,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub nation: Option<String>,

    /// With `ship`: per option, its rank in the group in words, e.g. "F.
    /// Sherman's Mk17 ranks 33 of the 39 torpedo options of tier 10
    /// destroyers (score 36.4): better than 13% of the others."
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub verdicts: Vec<String>,

    pub entries: Vec<TorpedoEntry>,

    /// Options matching the filters that `limit` leaves out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,

    /// Per factor: its weight out of 100, which way is better, and why it
    /// counts.
    pub weights: Vec<String>,

    pub rule: String,
}

/// One torpedo option and the standings behind its score.
#[derive(Debug, Clone, Serialize)]
pub struct TorpedoEntry {
    /// From 1, the highest score first; equal scores share a rank.
    pub rank: usize,

    /// "T10 F. Sherman (destroyer, USA, premium)".
    pub ship: String,

    pub torpedo: String,

    /// Modules other than stock that carry the torpedo. Empty for the stock
    /// torpedo.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,

    pub score: TorpedoScore,

    /// The factors raising the score most, in words.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub strengths: Vec<String>,

    /// The factors lowering the score most, in words.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub weaknesses: Vec<String>,

    /// Per factor, "value unit (rank r/n, pNN)".
    pub factors: FactorValues,

    /// The ship's unique upgrade and the factors whose best-fit build
    /// takes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_upgrade: Option<String>,

    /// Standing per factor, for tests and hosts. Not in the tool output.
    #[serde(skip)]
    pub standings: Vec<TorpedoStanding>,
}

/// A score from 0 to 100, one decimal.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TorpedoScore(f32);

impl TorpedoScore {
    pub fn value(self) -> f32 {
        self.0
    }
}

/// One factor of a torpedo option and the standing the score uses.
#[derive(Debug, Clone, PartialEq)]
pub struct TorpedoStanding {
    pub factor: &'static str,
    pub value: f32,

    /// From 0 to 100, higher better: the share of the other options beaten,
    /// ties counting half; for targets, the share of classes hit.
    pub standing: f32,
}

/// One factor of a torpedo option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Factor {
    BroadsideDamagePerMinute,
    BroadsideAlpha,
    TorpedoesPerBroadside,
    Damage,
    DamageAtMaxRange,
    Reload,
    ReactionTime,
    Speed,
    Range,
    Detectability,
    FloodCoefficient,
    FiringArc,
    BowDeadZone,
    SternDeadZone,
    Targets,
}

impl Factor {
    const ALL: [Self; 15] = [
        Self::BroadsideDamagePerMinute,
        Self::BroadsideAlpha,
        Self::TorpedoesPerBroadside,
        Self::Damage,
        Self::DamageAtMaxRange,
        Self::Reload,
        Self::ReactionTime,
        Self::Speed,
        Self::Range,
        Self::Detectability,
        Self::FloodCoefficient,
        Self::FiringArc,
        Self::BowDeadZone,
        Self::SternDeadZone,
        Self::Targets,
    ];

    /// The weights are a judgment; they sum to 100.
    fn spec(self) -> FactorSpec {
        let (key, unit, better, weight, reason) = match self {
            Self::BroadsideDamagePerMinute => (
                "broadside_dpm",
                "HP/min",
                Better::Higher,
                12,
                "sustained damage against one target at the best bearing: tubes bearing, reload and damage together",
            ),
            Self::BroadsideAlpha => (
                "broadside_alpha",
                "HP",
                Better::Higher,
                10,
                "damage of one launch at one target, every torpedo hitting",
            ),
            Self::TorpedoesPerBroadside => (
                "torpedoes_per_broadside",
                "torpedoes",
                Better::Higher,
                8,
                "torpedoes in one spread at one target; a wider, denser spread is harder to dodge",
            ),
            Self::Damage => ("damage", "HP", Better::Higher, 8, "damage of each hit"),
            Self::DamageAtMaxRange => (
                "damage_at_max_range",
                "HP",
                Better::Higher,
                6,
                "damage of a hit at the end of the run; torpedoes with damage falloff lose it on long launches",
            ),
            Self::Reload => ("reload", "s", Better::Lower, 6, "how often the launchers fire"),
            Self::ReactionTime => (
                "reaction_time",
                "s",
                Better::Lower,
                10,
                "time a target has to turn away after spotting the torpedo, the main limit on hits",
            ),
            Self::Speed => ("speed", "kn", Better::Higher, 7, "time to the target, in which it can leave the spread"),
            Self::Range => {
                ("range", "km", Better::Higher, 7, "launch distance, which lets a launch come from beyond detection")
            }
            Self::Detectability => (
                "detectability",
                "km",
                Better::Lower,
                3,
                "distance at which ships spot the torpedo; most of its effect is already in reaction_time",
            ),
            Self::FloodCoefficient => {
                ("flood_coefficient", "x", Better::Higher, 3, "flooding chance of a hit, damage on top of the hit")
            }
            Self::FiringArc => (
                "firing_arc",
                "deg",
                Better::Higher,
                4,
                "degrees of one side, out of 179, at which a launcher fires; a narrow arc needs a precise turn to launch",
            ),
            Self::BowDeadZone => (
                "bow_dead_zone",
                "deg",
                Better::Lower,
                3,
                "angle off the bow inside which no launcher fires; a small one launches while closing bow-in",
            ),
            Self::SternDeadZone => (
                "stern_dead_zone",
                "deg",
                Better::Lower,
                3,
                "angle off the stern inside which no launcher fires; a small one launches while retreating",
            ),
            Self::Targets => (
                "targets",
                "% of classes",
                Better::Higher,
                10,
                "share of the five ship classes the torpedo can hit; the score loses this weight times the share it cannot hit, as deep-water torpedoes pass under destroyers and submarines",
            ),
        };

        FactorSpec { key, unit, better, weight, reason }
    }

    /// The factor of ship_comparison that measures the same.
    fn from_comparison(factor: TorpedoFactor) -> Self {
        match factor {
            TorpedoFactor::Speed => Self::Speed,
            TorpedoFactor::Range => Self::Range,
            TorpedoFactor::Damage => Self::Damage,
            TorpedoFactor::DamageAtMaxRange => Self::DamageAtMaxRange,
            TorpedoFactor::FloodCoefficient => Self::FloodCoefficient,
            TorpedoFactor::Detectability => Self::Detectability,
            TorpedoFactor::ReactionTime => Self::ReactionTime,
            TorpedoFactor::DamagePerMinute => Self::BroadsideDamagePerMinute,
        }
    }

    /// The standing a factor is judged against: the middle of the group,
    /// or for targets the torpedo that hits every class.
    fn norm(self) -> f32 {
        match self {
            Self::Targets => 100.0,
            _ => 50.0,
        }
    }

    /// The value in words, for a strength or a weakness.
    fn describe(self, option: &TorpedoValues, value: f32, strength: bool) -> String {
        let shown = number(value);
        match (self, strength) {
            (Self::BroadsideDamagePerMinute, true) => format!("high damage per minute at one target, {shown} HP/min"),
            (Self::BroadsideDamagePerMinute, false) => format!("low damage per minute at one target, {shown} HP/min"),
            (Self::BroadsideAlpha, true) => format!("heavy broadside launch, {shown} HP"),
            (Self::BroadsideAlpha, false) => format!("light broadside launch, {shown} HP"),
            (Self::TorpedoesPerBroadside, true) => format!("{shown} torpedoes per broadside"),
            (Self::TorpedoesPerBroadside, false) => format!("only {shown} torpedoes per broadside"),
            (Self::Damage, true) => format!("high damage per hit, {shown} HP"),
            (Self::Damage, false) => format!("low damage per hit, {shown} HP"),
            (Self::DamageAtMaxRange, true) => format!("high damage at the end of its run, {shown} HP"),
            (Self::DamageAtMaxRange, false) => match option.values.get(&Self::Damage) {
                Some(full) if *full > value => {
                    format!("damage falls to {shown} HP per hit at the end of its run, from {}", number(*full))
                }
                _ => format!("low damage at the end of its run, {shown} HP"),
            },
            (Self::Reload, true) => format!("fast reload, {shown} s"),
            (Self::Reload, false) => format!("slow reload, {shown} s"),
            (Self::ReactionTime, true) => format!("targets get only {shown} s to react after spotting it"),
            (Self::ReactionTime, false) => format!("targets get {shown} s to react after spotting it"),
            (Self::Speed, true) => format!("fast, {shown} kn"),
            (Self::Speed, false) => format!("slow, {shown} kn"),
            (Self::Range, true) => format!("long range, {shown} km"),
            (Self::Range, false) => format!("short range, {shown} km"),
            (Self::Detectability, true) => format!("spotted only from {shown} km"),
            (Self::Detectability, false) => format!("spotted from {shown} km"),
            (Self::FloodCoefficient, true) => format!("high flooding coefficient, {shown}x"),
            (Self::FloodCoefficient, false) => format!("low flooding coefficient, {shown}x"),
            (Self::FiringArc, true) => format!("launchers bear across {shown} deg of one side"),
            (Self::FiringArc, false) => {
                let bow = option.values.get(&Self::BowDeadZone);
                let stern = option.values.get(&Self::SternDeadZone);
                match (bow, stern) {
                    // One unbroken arc from the bow dead zone to the stern one.
                    (Some(from), Some(stern)) if 180.0 - stern - from + 1.0 == value => format!(
                        "launchers bear only from {} to {} deg off the bow",
                        number(*from),
                        number(180.0 - stern)
                    ),
                    _ => format!("launchers bear across only {shown} deg of one side"),
                }
            }
            (Self::BowDeadZone, true) => format!("launchers fire from {shown} deg off the bow"),
            (Self::BowDeadZone, false) => format!("launchers cannot fire within {shown} deg of the bow"),
            (Self::SternDeadZone, true) => format!("launchers fire from {shown} deg off the stern"),
            (Self::SternDeadZone, false) => format!("launchers cannot fire within {shown} deg of the stern"),
            (Self::Targets, _) => {
                let missed: Vec<&str> = ShipClass::ALL
                    .into_iter()
                    .filter(|class| option.hits_only.as_ref().is_some_and(|hits| !hits.contains(class)))
                    .map(class_plural)
                    .collect();
                format!("cannot hit {}", join_and(&missed))
            }
        }
    }
}

/// What a factor measures, which way it improves and its weight.
struct FactorSpec {
    key: &'static str,
    unit: &'static str,
    better: Better,

    /// Out of 100 over every factor.
    weight: u32,
    reason: &'static str,
}

/// The values of one torpedo option before ranks.
struct TorpedoValues<'a> {
    ship: &'a Ship,
    name: String,
    modules: Vec<String>,
    values: HashMap<Factor, f32>,

    /// The only classes the torpedo hits; `None` when it hits every class.
    hits_only: Option<Vec<ShipClass>>,

    /// "Name (unique upgrade): reload, speed".
    unique_upgrade: Option<String>,
}

/// How far one factor moves an option's score from the norm: its weight
/// times its standing's distance from [`Factor::norm`].
struct Pull {
    factor: Factor,
    value: f32,
    standing: f32,
    pull: f32,
}

/// The score of one torpedo option and what lies behind it, before its
/// rank is known.
struct Judgment {
    score: TorpedoScore,
    strengths: Vec<String>,
    weaknesses: Vec<String>,
    factors: FactorValues,
    standings: Vec<TorpedoStanding>,
}

impl Judgment {
    fn into_entry(self, rank: usize, option: &TorpedoValues<'_>) -> TorpedoEntry {
        TorpedoEntry {
            rank,
            ship: option.ship.label(),
            torpedo: option.name.clone(),
            modules: option.modules.clone(),
            score: self.score,
            strengths: self.strengths,
            weaknesses: self.weaknesses,
            factors: self.factors,
            unique_upgrade: option.unique_upgrade.clone(),
            standings: self.standings,
        }
    }
}

impl GameData {
    /// Torpedo options of a class and tier group, best or worst first, by
    /// a weighted score of their standings in the group.
    pub fn torpedo_rankings(&self, args: &TorpedoRankingsArgs) -> Result<TorpedoRankingsOutput, QueryError> {
        const DEFAULT_LIMIT: usize = 10;
        // Launchers aimed by bearing: submarine tubes aim by firing sectors.
        const RANKED_CLASSES: [ShipClass; 3] = [ShipClass::Destroyer, ShipClass::Cruiser, ShipClass::Battleship];

        let mut interpretations = Interpretations::default();
        let focus = match &args.ship {
            Some(query) => Some(self.resolve_ship(query, &mut interpretations)?),
            None => None,
        };
        let classes: Vec<ShipClass> = match args.class.or(focus.map(|ship| ship.class)) {
            None => RANKED_CLASSES.to_vec(),
            Some(class) if RANKED_CLASSES.contains(&class) => vec![class],
            Some(_) => {
                return Err(QueryError::InvalidArguments {
                    reason: "torpedo_rankings ranks destroyer, cruiser and battleship torpedoes: submarine torpedoes aim by firing sectors and home on targets, and carrier torpedoes are dropped by aircraft",
                });
            }
        };
        let tiers = match (focus, args.min_tier, args.max_tier) {
            (Some(ship), None, None) => TierRange::from_bounds(Some(ship.tier), Some(ship.tier)),
            (_, min, max) => TierRange::from_bounds(min, max),
        };
        if tiers.is_some_and(|tiers| tiers.min > tiers.max) {
            return Err(QueryError::InvalidArguments { reason: "min_tier must not exceed max_tier" });
        }
        let order = args.order.unwrap_or_default();

        let filter = ShipFilter {
            tiers,
            class: None,
            nation: None,
            include_irregular: false,
            exclude_test: !args.include_test.unwrap_or(false),
            include_replicas: false,
        };
        let mut group: Vec<&Ship> =
            self.ships.iter().filter(|ship| classes.contains(&ship.class) && filter.matches(ship)).collect();
        group.sort_by(|left, right| listing_order(left, right));

        let per_ship =
            parallel_map(&group, |ship| self.torpedo_values(ship)).into_iter().collect::<Result<Vec<_>, _>>()?;
        let options: Vec<TorpedoValues<'_>> = per_ship.into_iter().flatten().collect();
        let armed = ShipCounts::from_ships(
            group
                .iter()
                .copied()
                .filter(|ship| options.iter().any(|option| option.ship.param.index() == ship.param.index())),
        );

        if options.len() < 2 {
            return Err(QueryError::InvalidArguments {
                reason: "fewer than two torpedo options match; widen the tiers or classes to rank torpedoes",
            });
        }

        let mut judged: Vec<(Judgment, &TorpedoValues<'_>)> =
            options.iter().map(|option| (judge(option, &options), option)).collect();
        judged.sort_by(|(left, left_option), (right, right_option)| {
            right
                .score
                .value()
                .total_cmp(&left.score.value())
                .then_with(|| listing_order(left_option.ship, right_option.ship))
                .then_with(|| left_option.name.cmp(&right_option.name))
        });
        let scores: Vec<f32> = judged.iter().map(|(judgment, _)| judgment.score.value()).collect();
        let listing = ShipFilter { nation: args.nation.clone(), ..filter };
        let focused = |ship: &Ship| focus.is_none_or(|focus| focus.param.index() == ship.param.index());
        let mut listed: Vec<TorpedoEntry> = judged
            .into_iter()
            .filter(|(_, option)| listing.matches(option.ship) && focused(option.ship))
            .map(|(judgment, option)| {
                let rank = 1 + scores.iter().filter(|other| **other > judgment.score.value()).count();
                judgment.into_entry(rank, option)
            })
            .collect();
        if focus.is_some() && listed.is_empty() {
            return Err(QueryError::InvalidArguments {
                reason: "the ship carries no torpedoes in the group ranked; give its class and tier or omit them",
            });
        }
        if order == TorpedoOrder::Worst {
            listed.reverse();
        }
        let tier_text = tiers.map_or_else(|| "every tier".to_owned(), tier_label);
        let class_names: Vec<&str> = classes.iter().copied().map(class_plural).collect();
        let classes_text = join_and(&class_names);
        let group_text = format!("{tier_text} {classes_text}");
        let verdicts = match focus {
            Some(ship) => listed.iter().map(|entry| verdict(&ship.name, entry, &scores, &group_text)).collect(),
            None => Vec::new(),
        };
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
        let omitted = (listed.len() > limit).then(|| listed.len() - limit);
        let entries = listed.into_iter().take(limit).collect();

        Ok(TorpedoRankingsOutput {
            interpretations,
            group: group_text,
            classes,
            tiers,
            ships: armed,
            options: options.len(),
            order,
            nation: args.nation.clone(),
            verdicts,
            entries,
            omitted,
            weights: Factor::ALL
                .iter()
                .map(|factor| {
                    let spec = factor.spec();
                    let better = match spec.better {
                        Better::Lower => "lower",
                        Better::Higher => "higher",
                    };
                    format!("{} {} ({}, {better} is better): {}", spec.key, spec.weight, spec.unit, spec.reason)
                })
                .collect(),
            rule: rule(&classes_text),
        })
    }

    /// Every torpedo option of one ship with its factor values. Empty for a
    /// ship without torpedoes.
    fn torpedo_values<'a>(&self, ship: &'a Ship) -> Result<Vec<TorpedoValues<'a>>, QueryError> {
        let planner = Planner::new(self, ship);
        // Damage rates need a build search per weapon; skip ships that carry
        // no torpedoes before paying for it.
        if crate::torpedoes::torpedo_options(&planner)?.is_empty() {
            return Ok(Vec::new());
        }
        let rates = self.rates(&planner)?;
        let unique = self.upgrades.iter().find(|upgrade| upgrade.is_unique() && upgrade.fits(ship));
        let unique_label = unique.map(|upgrade| upgrade.build_label());

        let mut options = Vec::new();
        for torpedo in self.torpedo_scores(&planner, &rates, unique)? {
            let mut values: HashMap<Factor, f32> =
                torpedo.values.iter().map(|(factor, value)| (Factor::from_comparison(*factor), *value)).collect();
            let mut unique_used: Vec<Factor> = torpedo
                .values
                .keys()
                .filter(|factor| torpedo.unique_for(**factor).is_some())
                .map(|factor| Factor::from_comparison(*factor))
                .collect();

            let carries =
                |outcome: &Outcome| {
                    outcome.stats.torpedoes.as_ref().is_some_and(|torpedoes| {
                        torpedoes.torpedoes.iter().any(|carried| carried.name == torpedo.param)
                    })
                };
            let reload = |outcome: &Outcome| {
                if !carries(outcome) {
                    return None;
                }
                Some(Amount(round2(outcome.stats.torpedoes.as_ref()?.reload_time?.value())))
            };
            let plan =
                StatPlan::new("torpedo reload".to_owned(), Better::Lower, &reload).on_selection(torpedo.selection);
            if let Some(builds) = planner.best(&plan)? {
                values.insert(Factor::Reload, builds.best.value.0);
                if unique_label.as_ref().is_some_and(|label| builds.best.assumed.contains(label)) {
                    unique_used.push(Factor::Reload);
                }
            }

            // The damage rate is for one target only where the launcher
            // facings are known; otherwise it counts every launcher.
            let one_target = rates
                .torpedo_damage
                .iter()
                .any(|ammo| ammo.ammo == torpedo.name && ammo.rates.builds.best.value.bearing.is_some());
            if !one_target {
                values.remove(&Factor::BroadsideDamagePerMinute);
            }
            if let Some(arcs) = self.torpedo_arcs(ship, &planner, &carries)? {
                let broadside = arcs.broadside as f32;
                values.insert(Factor::TorpedoesPerBroadside, broadside);
                values.insert(Factor::FiringArc, arcs.side_arc as f32);
                if let Some(damage) = values.get(&Factor::Damage).copied() {
                    values.insert(Factor::BroadsideAlpha, (broadside * damage).round());
                }
                if let Some(angle) = arcs.any_from_bow {
                    values.insert(Factor::BowDeadZone, f32::from(angle));
                }
                if let Some(angle) = arcs.any_from_stern {
                    values.insert(Factor::SternDeadZone, f32::from(angle));
                }
            }

            let hits_only = self.torpedo_targets(&torpedo.param);
            let hit = hits_only.as_ref().map_or(ShipClass::ALL.len(), Vec::len);
            values.insert(Factor::Targets, round2(hit as f32 * 100.0 / ShipClass::ALL.len() as f32));

            let mut unique_keys: Vec<&str> = unique_used
                .into_iter()
                .filter(|factor| values.contains_key(factor))
                .map(|factor| factor.spec().key)
                .collect();
            unique_keys.sort_unstable();
            unique_keys.dedup();
            let unique_upgrade = match (&unique_label, unique_keys.is_empty()) {
                (Some(label), false) => Some(format!("{label}: {}", unique_keys.join(", "))),
                _ => None,
            };

            options.push(TorpedoValues {
                ship,
                name: torpedo.name,
                modules: torpedo.modules,
                values,
                hits_only,
                unique_upgrade,
            });
        }

        Ok(options)
    }

    /// How the launchers bear on the module combination carrying the
    /// torpedo with the most tubes on one broadside, then the smallest bow
    /// dead zone. `None` without the hull's launcher facings.
    fn torpedo_arcs(
        &self,
        ship: &Ship,
        planner: &Planner<'_>,
        carries: &dyn Fn(&Outcome) -> bool,
    ) -> Result<Option<MountArcs>, QueryError> {
        let mut best: Option<MountArcs> = None;
        for (index, selection) in planner.selections().enumerate() {
            if !carries(&*planner.bare(index)?) {
                continue;
            }
            let arcs = match self.selection_arcs(ship, selection, ComponentType::Torpedoes) {
                Ok(arcs) => arcs,
                Err(QueryError::HullFacingsUnavailable { .. }) => continue,
                Err(error) => return Err(error),
            };
            let Some(arcs) = arcs else {
                continue;
            };
            let dead_zone = |arcs: &MountArcs| arcs.any_from_bow.unwrap_or(u16::MAX);
            let better = best.is_none_or(|best| {
                arcs.broadside > best.broadside
                    || (arcs.broadside == best.broadside && dead_zone(&arcs) < dead_zone(&best))
            });
            if better {
                best = Some(arcs);
            }
        }

        Ok(best)
    }
}

/// The score of one torpedo option among `options` and the standings behind
/// it.
fn judge(option: &TorpedoValues<'_>, options: &[TorpedoValues<'_>]) -> Judgment {
    // Strengths and weaknesses named per entry.
    const NAMED_FACTORS: usize = 3;
    // Standings within this many points of the norm are not named.
    const NAMED_MARGIN: f32 = 10.0;
    // A firing_arc standing below this is among the narrowest of the group.
    const NARROW_ARC_BELOW: f32 = 25.0;

    let mut factors = Vec::new();
    let mut standings = Vec::new();
    let mut pulls: Vec<Pull> = Vec::new();
    let mut weighted = 0.0;
    let mut weights = 0.0;
    let mut penalty = 0.0;
    for factor in Factor::ALL {
        let Some(&value) = option.values.get(&factor) else {
            continue;
        };
        let spec = factor.spec();
        let weight = spec.weight as f32;
        let shown = match spec.unit {
            "x" => format!("{}x", number(value)),
            unit if unit.starts_with('%') => format!("{}{unit}", number(value)),
            unit => format!("{} {unit}", number(value)),
        };

        // Targets subtracts from the score rather than averaging into it, so
        // an option missing other factors gains nothing from hitting every
        // class.
        if factor == Factor::Targets {
            let classes = match &option.hits_only {
                Some(hits) => {
                    let names: Vec<&str> = hits.iter().map(|class| class.label()).collect();
                    format!("hits only {}", names.join(", "))
                }
                None => "hits every class".to_owned(),
            };
            factors.push((spec.key, format!("{shown} ({classes})")));
            penalty = weight * (100.0 - value) / 100.0;
            standings.push(TorpedoStanding { factor: spec.key, value, standing: value });
            pulls.push(Pull { factor, value, standing: value, pull: weight * (value - factor.norm()) });
            continue;
        }

        let pool = options.iter().filter_map(|other| other.values.get(&factor).copied());
        let rank = GroupRank::new(value, spec.better, pool);
        // A factor no other option has says nothing about this one.
        let Some(beaten) = rank.beaten_ties_half() else {
            factors.push((spec.key, format!("{shown} (the only option with it)")));
            continue;
        };
        let standing = beaten.value();
        factors.push((spec.key, format!("{shown} (rank {}/{}, p{})", rank.position, rank.count, standing.round())));
        weighted += weight * standing;
        weights += weight;
        standings.push(TorpedoStanding { factor: spec.key, value, standing });
        pulls.push(Pull { factor, value, standing, pull: weight * (standing - factor.norm()) });
    }
    // Every option of a group of two or more ranks on damage, so weights is
    // positive.
    let score = (weighted / weights - penalty).max(0.0);

    pulls.sort_by(|left, right| right.pull.total_cmp(&left.pull));
    // Firing close to the bow or stern is no strength when the launchers
    // bear only across one of the narrowest arcs of the group.
    let narrow_arc = pulls.iter().any(|pull| pull.factor == Factor::FiringArc && pull.standing < NARROW_ARC_BELOW);
    let strong = |pull: &&Pull| pull.standing - pull.factor.norm() > NAMED_MARGIN;
    let weak = |pull: &&Pull| pull.factor.norm() - pull.standing > NAMED_MARGIN;
    // broadside_alpha follows from torpedoes_per_broadside, and damage at the
    // end of the run equals damage without falloff; naming both would crowd
    // out another cause.
    let follows = |pull: &&Pull, named: &dyn Fn(&&Pull) -> bool| {
        let source = match pull.factor {
            Factor::BroadsideAlpha => Factor::TorpedoesPerBroadside,
            Factor::DamageAtMaxRange => Factor::Damage,
            _ => return false,
        };
        pulls.iter().any(|other| {
            other.factor == source
                && named(&other)
                && (pull.factor != Factor::DamageAtMaxRange || other.value == pull.value)
        })
    };
    let strengths = pulls
        .iter()
        .filter(strong)
        .filter(|pull| !(narrow_arc && matches!(pull.factor, Factor::BowDeadZone | Factor::SternDeadZone)))
        .filter(|pull| !follows(pull, &strong))
        .take(NAMED_FACTORS)
        .map(|pull| pull.factor.describe(option, pull.value, true))
        .collect();
    let weaknesses = pulls
        .iter()
        .rev()
        .filter(weak)
        .filter(|pull| !follows(pull, &weak))
        .take(NAMED_FACTORS)
        .map(|pull| pull.factor.describe(option, pull.value, false))
        .collect();

    Judgment {
        score: TorpedoScore((score * 10.0).round() / 10.0),
        strengths,
        weaknesses,
        factors: FactorValues(factors),
        standings,
    }
}

/// "F. Sherman's Mk17 ranks 33 of the 39 torpedo options of tier 10
/// destroyers (score 36.4): better than 13% of the others."
fn verdict(ship: &str, entry: &TorpedoEntry, scores: &[f32], group: &str) -> String {
    let score = entry.score.value();
    let others = scores.len().saturating_sub(1).max(1);
    let beaten = scores.iter().filter(|other| **other < score).count();
    let share = (beaten as f32 * 100.0 / others as f32).round();

    format!(
        "{ship}'s {} ranks {} of the {} torpedo options of {group} (score {score}): better than {share}% of the others.",
        entry.torpedo,
        entry.rank,
        scores.len()
    )
}

fn rule(classes: &str) -> String {
    format!(
        "Classes considered: {classes}. Each torpedo option (a torpedo a ship can carry; a ship with alternative torpedoes has one option per torpedo) ranks within the group on each factor. A factor's standing is pNN, the share of the other options it beats in the direction better for that factor, ties counting half. score = sum of weight x standing over the factors the option has other than targets / sum of those weights, minus the targets weight x the share of the five ship classes the torpedo cannot hit (4 for a deep-water torpedo hitting cruisers, battleships and carriers; 6 for one hitting only battleships and carriers), from 0 to 100, higher better. Lower is better for reload, reaction_time, detectability, bow_dead_zone and stern_dead_zone: a torpedo spotted only from 0.6 km is seen later than one spotted from 1.5 km, a strength. reaction_time = detectability / speed. Values are on each ship's best-fit build for each factor, so speed may assume Torpedo Acceleration while range does not. torpedoes_per_broadside, broadside_alpha (torpedoes_per_broadside x damage), firing_arc and the dead zones come from each launcher's firing arcs on the hull, on the module combination carrying the torpedo with the most tubes bearing at one bearing on either broadside; a centerline launcher counts on both sides. firing_arc counts the bearings of one side at which any launcher fires; the dead zones are the smallest angles off the bow and off the stern at which any launcher fires. broadside_dpm is the damage per minute of the launchers bearing on one target at the bearing where the most tubes bear, on its own best-fit build; it is left out where the launcher facings are unknown. Damage figures assume every torpedo hits. strengths and weaknesses name up to three factors each, those moving the score most, among standings more than 10 points above or below the middle of the group (p50; for targets, below 90); broadside_alpha is not named beside torpedoes_per_broadside, which it follows from, nor damage_at_max_range beside an equal damage, and the dead zones are not named strengths when firing_arc is in the narrowest quarter of the group. The score is a judgment from game data and these weights, not win rates or hit rates: say so when quoting it, and explain a rank from strengths and weaknesses."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_100() {
        let total: u32 = Factor::ALL.iter().map(|factor| factor.spec().weight).sum();

        assert_eq!(total, 100);
    }
}
