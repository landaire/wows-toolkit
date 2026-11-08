//! Tech tree lines: research paths ending at a tier 10 ship, with the
//! research XP and purchase credits of their ships in a tier range and, on
//! request, the cost of a Research Bureau reset per Research Point.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::ops::AddAssign;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wowsunpack::game_params::types::Credits;
use wowsunpack::game_params::types::Experience;

use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipFilter;
use crate::ships::ShipStatus;
use crate::ships::Tier;
use crate::ships::TierRange;

/// GameParams groups of researchable ships: the tier 1 starting ships and
/// the ships researched from them.
const TECH_TREE_GROUPS: &[&str] = &["start", "upgradeable"];

/// Tier of the ships lines end at. Tier 11 ships follow tier 10 ships in the
/// research tree but are not part of a line.
const LINE_END: Tier = Tier(10);

const DEFAULT_LIMIT: usize = 20;

/// Arguments for `tech_tree_lines`. Every filter is optional.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TechTreeLinesArgs {
    /// Nation as GameParams spells it, e.g. "USA", "Germany", "Japan", "Russia", "United_Kingdom".
    pub nation: Option<String>,

    /// Class of the tier 10 ship the line ends at.
    pub class: Option<ShipClass>,

    /// The tier 10 ship the line ends at, by name or common alias, e.g. "Harugumo".
    pub tier_10_ship: Option<String>,

    /// Lowest tier whose ships are costed. Defaults to 8.
    #[schemars(range(min = 1, max = 10))]
    pub min_tier: Option<Tier>,

    /// Highest tier whose ships are costed. Defaults to 10.
    #[schemars(range(min = 1, max = 10))]
    pub max_tier: Option<Tier>,

    /// Order of the lines. Defaults to nation.
    pub sort_by: Option<LineOrder>,

    /// Give each line its Research Bureau reset: Research Points and the cost per point. Always given when sort_by is credits_per_point or xp_per_point.
    #[serde(default)]
    pub include_reset: bool,

    /// Most lines to list. Defaults to 20.
    pub limit: Option<usize>,
}

/// Order of tech tree lines. Every order breaks remaining ties by the tier 10
/// ship's name, then by the path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineOrder {
    /// Nation, then the tier 10 ship's name.
    #[default]
    Nation,

    /// Cheapest first by reset purchase credits per Research Point, then reset XP per point.
    CreditsPerPoint,

    /// Cheapest first by reset research XP per Research Point, then reset credits per point.
    XpPerPoint,

    /// Cheapest first by the mean of the line's ranks by credits_per_point and by xp_per_point.
    ResetCost,

    /// Cheapest first by total research XP in the tier range, then total purchase credits.
    ResearchXp,

    /// Cheapest first by total purchase credits in the tier range, then total research XP.
    PurchaseCredits,
}

impl LineOrder {
    /// Orders by the Research Bureau reset, which lines then carry.
    fn by_reset(self) -> bool {
        matches!(self, Self::CreditsPerPoint | Self::XpPerPoint | Self::ResetCost)
    }
}

/// How `LineOrder::ResetCost` ranks, stated in the output beside it.
const RESET_COST_RANKING: &str = "reset_cost.mean_rank is the mean of the line's rank by credits_per_point and its rank by xp_per_point among all lines matching the filters, listed or omitted (1 is cheapest; equal values share a rank). Lines go from the lowest mean_rank; ties go by credits_per_point, then xp_per_point.";

#[derive(Debug, Clone, Serialize)]
pub struct TechTreeLinesOutput {
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// Tiers whose ships are costed.
    pub tiers: TierRange,

    pub sort_by: LineOrder,

    /// Lines matching the filters.
    pub total: usize,

    pub lines: Vec<TechTreeLine>,

    /// How sort_by ranks when it combines measures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking: Option<&'static str>,

    /// The reset_cost order in words, when sort_by is reset_cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ResetCostSummary>,

    /// Lines left out of `lines` by the limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,
}

/// The research path from a tier 1 ship to one tier 10 ship.
#[derive(Debug, Clone, Serialize)]
pub struct TechTreeLine {
    /// The tier 10 ship the line ends at; it names the line.
    pub tier_10_ship: String,

    pub nation: String,
    pub class: ShipClass,

    /// Ships from tier 1 to tier 10, each with its tier and class.
    pub path: Vec<PathShip>,

    /// The first path ship of the line's class, when ships of other classes
    /// precede it: those are only the research route to the line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_starts_at: Option<String>,

    /// The highest-tier ship this line shares with another line. The line's
    /// own ships follow it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches_from: Option<String>,

    /// The line's ships in the tier range.
    pub ships: Vec<LineShip>,

    /// Sums over `ships`.
    pub total_research_xp: Experience,
    pub total_purchase_credits: Credits,

    /// Present when the reset was asked for or orders the lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset: Option<LineReset>,
}

/// One ship of a line's research path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PathShip {
    pub name: String,
    pub tier: Tier,
    pub class: ShipClass,
}

/// What a Research Bureau reset of the line pays and what regrinding it
/// costs. Module research and purchases are excluded: the game data does not
/// carry their prices.
#[derive(Debug, Clone, Serialize)]
pub struct LineReset {
    /// Research Points the line pays once each of its tier 5 to 10 ships is
    /// researched again and wins a battle or earns 300 base XP in one battle,
    /// before season multipliers.
    pub research_points: ResearchPoints,

    /// The ships that pay Research Points.
    pub points_by_ship: Vec<ShipPoints>,

    /// Research XP and purchase credits of every ship from tier 2 to 10: the
    /// ships the reset takes away.
    pub regrind: Cost,

    /// Research XP and purchase credits of the ships in `points_by_ship`.
    pub paying_ships: Cost,

    /// `regrind` credits and XP per Research Point.
    pub credits_per_point: CostPerPoint<Credits>,
    pub xp_per_point: CostPerPoint<Experience>,

    /// The line's ranks by both costs per point, when sort_by is reset_cost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_cost: Option<ResetCostRank>,

    /// Credits the reset pays for the line's ships when the player owns every
    /// ship from tier 2 to 10: half their purchase price.
    pub sale_credits: Credits,

    /// `regrind` credits less `sale_credits`.
    pub net_credits_if_owned: Credits,
}

/// A line's place by both reset costs per Research Point among the lines
/// matching the filters. See `RESET_COST_RANKING`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ResetCostRank {
    pub credits_per_point_rank: LineRank,
    pub xp_per_point_rank: LineRank,

    /// Mean of the two ranks: whole or half.
    pub mean_rank: f32,
}

/// Position of a line in a ranking, 1 cheapest. Equal values share a rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct LineRank(pub usize);

impl LineRank {
    /// One more than the number of `values` below `value`.
    fn of<T: Ord>(value: &T, values: impl Iterator<Item = T>) -> Self {
        Self(1 + values.filter(|other| other < value).count())
    }
}

/// The reset_cost ranking stated as sentences, so the order and each line's
/// two ranks are read rather than inferred.
#[derive(Debug, Clone, Serialize)]
pub struct ResetCostSummary {
    /// One sentence per listed line, in order: "1. Grozovoi (Russia
    /// destroyer): 2nd cheapest in credits per point (5,001.3), 3rd cheapest
    /// in XP per point (60.1); mean rank 2.5".
    pub lines: Vec<String>,

    /// The lines ranked 1 by credits per point among all lines considered.
    pub best_in_credits: String,

    /// The lines ranked 1 by XP per point among all lines considered.
    pub best_in_xp: String,
}

impl ResetCostSummary {
    /// `lines` in reset_cost order, every line considered; the first
    /// `listed` are listed.
    fn new(lines: &[TechTreeLine], listed: usize) -> Self {
        struct Ranked<'a> {
            line: &'a TechTreeLine,
            reset: &'a LineReset,
            rank: ResetCostRank,
        }

        let ranked: Vec<Ranked<'_>> = lines
            .iter()
            .filter_map(|line| {
                let reset = line.reset.as_ref()?;
                Some(Ranked { line, reset, rank: reset.reset_cost? })
            })
            .collect();
        let name = |line: &TechTreeLine| {
            format!("{} ({} {})", line.tier_10_ship, line.nation, line.class.label().replace('_', " "))
        };
        let sentences = ranked
            .iter()
            .take(listed)
            .enumerate()
            .map(|(index, Ranked { line, reset, rank })| {
                format!(
                    "{}. {}: {} in credits per point ({}), {} in XP per point ({}); mean rank {}",
                    index + 1,
                    name(line),
                    cheapest(rank.credits_per_point_rank),
                    one_decimal(reset.credits_per_point.value()),
                    cheapest(rank.xp_per_point_rank),
                    one_decimal(reset.xp_per_point.value()),
                    rank.mean_rank
                )
            })
            .collect();
        let best = |rank_of: fn(&ResetCostRank) -> LineRank, value_of: fn(&LineReset) -> f64, unit: &str| {
            let firsts: Vec<String> = ranked
                .iter()
                .filter(|ranked| rank_of(&ranked.rank) == LineRank(1))
                .map(|ranked| format!("{}, {} {unit}", name(ranked.line), one_decimal(value_of(ranked.reset))))
                .collect();
            format!("{}: the lowest of the {} lines considered", firsts.join(" and "), ranked.len())
        };

        Self {
            lines: sentences,
            best_in_credits: best(
                |rank| rank.credits_per_point_rank,
                |reset| reset.credits_per_point.value(),
                "credits per point",
            ),
            best_in_xp: best(|rank| rank.xp_per_point_rank, |reset| reset.xp_per_point.value(), "XP per point"),
        }
    }
}

/// "cheapest" for rank 1, else "2nd cheapest", "3rd cheapest", ...
fn cheapest(rank: LineRank) -> String {
    let suffix = match (rank.0 % 10, rank.0 % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };

    if rank.0 == 1 { "cheapest".to_owned() } else { format!("{}{suffix} cheapest", rank.0) }
}

/// One decimal with thousands separators: "5,001.3".
fn one_decimal(value: f64) -> String {
    let text = format!("{value:.1}");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, "0"));
    let digits = whole.trim_start_matches('-');
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let sign = if whole.starts_with('-') { "-" } else { "" };

    format!("{sign}{grouped}.{fraction}")
}

/// Research Points, the Research Bureau currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ResearchPoints(pub u64);

impl ResearchPoints {
    /// Points a reset puts on a researched ship of `tier`. GameParams and the
    /// client scripts do not carry them; the server sends the reward. Sources:
    /// <https://worldofwarships.com/en/news/game-updates/research-bureau/> (2019-08-16),
    /// <https://worldofwarships.com/en/news/game-updates/update-099/> (2020-09-29),
    /// <https://worldofwarships.com/en/news/game-updates/research-bureau-update/> (2020-10-13),
    /// <https://worldofwarships.com/en/news/game-updates/update-0911/> (2020-12-01).
    /// Cross-checked against the per-line totals on
    /// <https://wiki.worldofwarships.com/Ship:Research_Bureau>.
    fn for_reset_ship(tier: Tier) -> Option<Self> {
        match tier.0 {
            5 => Some(Self(200)),
            6 | 7 => Some(Self(500)),
            8 | 9 => Some(Self(1_500)),
            10 => Some(Self(6_000)),
            _ => None,
        }
    }
}

/// A ship that pays Research Points after a reset.
#[derive(Debug, Clone, Serialize)]
pub struct ShipPoints {
    pub name: String,
    pub tier: Tier,
    pub points: ResearchPoints,
}

/// Research XP and purchase credits of a set of ships.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Cost {
    pub research_xp: Experience,
    pub purchase_credits: Credits,
}

impl Cost {
    pub fn new(research_xp: Experience, purchase_credits: Credits) -> Self {
        Self { research_xp, purchase_credits }
    }
}

impl AddAssign for Cost {
    fn add_assign(&mut self, other: Self) {
        self.research_xp = Experience::new(self.research_xp.value() + other.research_xp.value());
        self.purchase_credits = Credits::new(self.purchase_credits.value() + other.purchase_credits.value());
    }
}

/// An amount a `CostPerPoint` divides.
pub trait PointCost: Copy {
    fn amount(self) -> u64;
}

impl PointCost for Credits {
    fn amount(self) -> u64 {
        self.value()
    }
}

impl PointCost for Experience {
    fn amount(self) -> u64 {
        self.value()
    }
}

/// A cost divided by Research Points. Compares exactly; serializes rounded
/// to one decimal place.
#[derive(Debug, Clone, Copy)]
pub struct CostPerPoint<C> {
    cost: C,
    points: ResearchPoints,
}

impl<C> CostPerPoint<C>
where
    C: PointCost,
{
    pub fn value(self) -> f64 {
        self.cost.amount() as f64 / self.points.0 as f64
    }
}

impl<C> PartialEq for CostPerPoint<C>
where
    C: PointCost,
{
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<C> Eq for CostPerPoint<C> where C: PointCost {}

impl<C> PartialOrd for CostPerPoint<C>
where
    C: PointCost,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<C> Ord for CostPerPoint<C>
where
    C: PointCost,
{
    fn cmp(&self, other: &Self) -> Ordering {
        let left = u128::from(self.cost.amount()) * u128::from(other.points.0);
        let right = u128::from(other.cost.amount()) * u128::from(self.points.0);

        left.cmp(&right)
    }
}

impl<C> Serialize for CostPerPoint<C>
where
    C: PointCost,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f64((self.value() * 10.0).round() / 10.0)
    }
}

/// A ship of a line in the tier range.
#[derive(Debug, Clone, Serialize)]
pub struct LineShip {
    pub name: String,
    pub tier: Tier,

    /// Ship experience to research it from the previous ship. Absent for a
    /// ship that needs no research.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub research_xp: Option<Experience>,

    /// Port price in credits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchase_credits: Option<Credits>,

    /// Tier 10 ships of the other lines through this ship.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shared_with: Vec<String>,
}

/// A line as indexes into `GameData::ships`, from tier 1 to tier 10.
struct LinePath {
    ships: Vec<usize>,
}

impl LinePath {
    fn end(&self) -> usize {
        self.ships[self.ships.len() - 1]
    }
}

impl GameData {
    /// Research paths ending at a tier 10 ship, with the research XP and
    /// purchase credits of their ships in a tier range and, when asked for or
    /// sorted by, the value of a Research Bureau reset. By default by nation.
    pub fn tech_tree_lines(&self, args: &TechTreeLinesArgs) -> Result<TechTreeLinesOutput, QueryError> {
        let tiers = TierRange { min: args.min_tier.unwrap_or(Tier(8)), max: args.max_tier.unwrap_or(LINE_END) };
        if tiers.max > LINE_END {
            return Err(QueryError::InvalidArguments {
                reason: "max_tier is at most 10; tier 11 ships are not part of tech tree lines",
            });
        }
        if tiers.min > tiers.max {
            return Err(QueryError::InvalidArguments { reason: "min_tier must not exceed max_tier" });
        }

        let paths = self.line_paths();

        let mut interpretations = Interpretations::default();
        let end = match args.tier_10_ship.as_deref() {
            Some(name) => {
                let ship = self.resolve_ship(name, &mut interpretations)?;
                if !paths.iter().any(|path| std::ptr::eq(&self.ships[path.end()], ship)) {
                    return Err(QueryError::NotLineEnd { ship: ship.name.clone() });
                }
                Some(ship)
            }
            None => None,
        };

        // Every line counts toward sharing, including those the filters leave out.
        let mut lines_through: HashMap<usize, BTreeSet<usize>> = HashMap::new();
        for path in &paths {
            for ship in &path.ships {
                lines_through.entry(*ship).or_default().insert(path.end());
            }
        }

        let filter = ShipFilter {
            tiers: None,
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: false,
            exclude_test: true,
            include_replicas: false,
        };
        let sort_by = args.sort_by.unwrap_or_default();
        let with_reset = args.include_reset || sort_by.by_reset();
        let mut lines: Vec<TechTreeLine> = paths
            .iter()
            .filter(|path| filter.matches(&self.ships[path.end()]))
            .filter(|path| end.is_none_or(|end| std::ptr::eq(&self.ships[path.end()], end)))
            .map(|path| self.tech_tree_line(path, tiers, &lines_through, with_reset))
            .collect();

        if sort_by == LineOrder::ResetCost {
            let resets = || lines.iter().filter_map(|line| line.reset.as_ref());
            let credits: Vec<CostPerPoint<Credits>> = resets().map(|reset| reset.credits_per_point).collect();
            let xp: Vec<CostPerPoint<Experience>> = resets().map(|reset| reset.xp_per_point).collect();
            for reset in lines.iter_mut().filter_map(|line| line.reset.as_mut()) {
                let credits_per_point_rank = LineRank::of(&reset.credits_per_point, credits.iter().copied());
                let xp_per_point_rank = LineRank::of(&reset.xp_per_point, xp.iter().copied());
                reset.reset_cost = Some(ResetCostRank {
                    credits_per_point_rank,
                    xp_per_point_rank,
                    mean_rank: (credits_per_point_rank.0 + xp_per_point_rank.0) as f32 / 2.0,
                });
            }
        }

        lines.sort_by(|left, right| {
            let xp = left.total_research_xp.cmp(&right.total_research_xp);
            let credits = left.total_purchase_credits.cmp(&right.total_purchase_credits);
            let per_point = |line: &TechTreeLine| line.reset.as_ref().map(|reset| reset.credits_per_point);
            let credits_per_point = per_point(left).cmp(&per_point(right));
            let xp_per_point = |line: &TechTreeLine| line.reset.as_ref().map(|reset| reset.xp_per_point);
            let xp_per_point = xp_per_point(left).cmp(&xp_per_point(right));
            // The sum orders as the mean does, without comparing floats.
            let rank_sum = |line: &TechTreeLine| {
                line.reset
                    .as_ref()
                    .and_then(|reset| reset.reset_cost)
                    .map(|rank| rank.credits_per_point_rank.0 + rank.xp_per_point_rank.0)
            };
            let order = match sort_by {
                LineOrder::Nation => left.nation.cmp(&right.nation),
                LineOrder::CreditsPerPoint => credits_per_point.then(xp_per_point),
                LineOrder::XpPerPoint => xp_per_point.then(credits_per_point),
                LineOrder::ResetCost => rank_sum(left).cmp(&rank_sum(right)).then(credits_per_point).then(xp_per_point),
                LineOrder::ResearchXp => xp.then(credits),
                LineOrder::PurchaseCredits => credits.then(xp),
            };

            order.then_with(|| left.tier_10_ship.cmp(&right.tier_10_ship)).then_with(|| left.path.cmp(&right.path))
        });

        let total = lines.len();
        let listed = args.limit.unwrap_or(DEFAULT_LIMIT).min(total);
        // The best lines are named among every line considered, before the
        // limit drops any.
        let summary = (sort_by == LineOrder::ResetCost).then(|| ResetCostSummary::new(&lines, listed));
        lines.truncate(listed);
        let omitted = (total > lines.len()).then(|| total - lines.len());

        let ranking = (sort_by == LineOrder::ResetCost).then_some(RESET_COST_RANKING);

        Ok(TechTreeLinesOutput { interpretations, tiers, sort_by, total, lines, ranking, summary, omitted })
    }

    /// Every research path from a tier 1 ship to a released tier 10 tech
    /// tree ship. A tier 10 ship reached by several paths has one line per path.
    fn line_paths(&self) -> Vec<LinePath> {
        let tech_tree: HashMap<&str, usize> = self
            .ships
            .iter()
            .enumerate()
            .filter(|(_, ship)| is_tech_tree(ship))
            .map(|(index, ship)| (ship.param.name(), index))
            .collect();

        let mut ends: Vec<usize> =
            tech_tree.values().copied().filter(|index| self.ships[*index].tier == LINE_END).collect();
        ends.sort_unstable();

        // Walks back from each tier 10 ship; partial paths run from the
        // tier 10 ship down.
        let mut paths = Vec::new();
        let mut partial: Vec<Vec<usize>> = ends.into_iter().map(|end| vec![end]).collect();
        while let Some(mut path) = partial.pop() {
            let first = &self.ships[path[path.len() - 1]];
            let from: Vec<usize> = self
                .research_predecessors
                .get(first.param.name())
                .map(|from| {
                    from.iter()
                        .filter_map(|name| tech_tree.get(name.as_str()).copied())
                        .filter(|ship| !path.contains(ship))
                        .collect()
                })
                .unwrap_or_default();
            if from.is_empty() {
                path.reverse();
                paths.push(LinePath { ships: path });
                continue;
            }

            for ship in from {
                let mut longer = path.clone();
                longer.push(ship);
                partial.push(longer);
            }
        }

        paths
    }

    fn tech_tree_line(
        &self,
        path: &LinePath,
        tiers: TierRange,
        lines_through: &HashMap<usize, BTreeSet<usize>>,
        with_reset: bool,
    ) -> TechTreeLine {
        let end = &self.ships[path.end()];
        let shares =
            |ship: &usize| lines_through.get(ship).is_some_and(|ends| ends.iter().any(|other| *other != path.end()));

        let branches_from =
            path.ships.iter().rev().find(|ship| shares(ship)).map(|ship| self.ships[*ship].name.clone());

        let ships: Vec<LineShip> = path
            .ships
            .iter()
            .filter(|ship| tiers.contains(self.ships[**ship].tier))
            .map(|index| {
                let ship = &self.ships[*index];
                let research = ship.param.vehicle().map(|vehicle| vehicle.research());
                let mut shared_with: Vec<String> = lines_through
                    .get(index)
                    .into_iter()
                    .flatten()
                    .filter(|other| **other != path.end())
                    .map(|other| self.ships[*other].name.clone())
                    .collect();
                shared_with.sort_unstable();
                shared_with.dedup();

                LineShip {
                    name: ship.name.clone(),
                    tier: ship.tier,
                    research_xp: research.and_then(|research| research.cost_xp()),
                    purchase_credits: research.and_then(|research| research.cost_credits()),
                    shared_with,
                }
            })
            .collect();

        let total_research_xp =
            Experience::new(ships.iter().filter_map(|ship| ship.research_xp).map(Experience::value).sum());
        let total_purchase_credits =
            Credits::new(ships.iter().filter_map(|ship| ship.purchase_credits).map(Credits::value).sum());

        let route: Vec<PathShip> = path
            .ships
            .iter()
            .map(|index| {
                let ship = &self.ships[*index];
                PathShip { name: ship.name.clone(), tier: ship.tier, class: ship.class }
            })
            .collect();
        let class_starts_at = match route.iter().position(|ship| ship.class == end.class) {
            Some(0) | None => None,
            Some(first) => Some(route[first].name.clone()),
        };

        TechTreeLine {
            tier_10_ship: end.name.clone(),
            nation: end.param.nation().to_owned(),
            class: end.class,
            path: route,
            class_starts_at,
            branches_from,
            ships,
            total_research_xp,
            total_purchase_credits,
            reset: with_reset.then(|| self.line_reset(path)),
        }
    }

    /// The Research Bureau reset of a line: the Research Points its ships pay
    /// and the cost of researching and buying its tier 2 to 10 ships again.
    fn line_reset(&self, path: &LinePath) -> LineReset {
        // A reset rolls a line back to its tier 1 ship; every later ship is
        // researched and bought again.
        const RESET_FROM: Tier = Tier(2);

        let mut points_by_ship = Vec::new();
        let mut regrind = Cost::new(Experience::new(0), Credits::new(0));
        let mut paying_ships = regrind;
        let mut sale_credits = 0;

        for index in &path.ships {
            let ship = &self.ships[*index];
            if ship.tier < RESET_FROM {
                continue;
            }

            // A missing price is a ship that costs nothing in that currency.
            let research = ship.param.vehicle().map(|vehicle| vehicle.research());
            let cost = Cost::new(
                research.and_then(|research| research.cost_xp()).unwrap_or(Experience::new(0)),
                research.and_then(|research| research.cost_credits()).unwrap_or(Credits::new(0)),
            );
            regrind += cost;
            sale_credits += cost.purchase_credits.value() / 2;

            let Some(points) = ResearchPoints::for_reset_ship(ship.tier) else {
                continue;
            };
            paying_ships += cost;
            points_by_ship.push(ShipPoints { name: ship.name.clone(), tier: ship.tier, points });
        }

        // Nonzero: every line ends at a tier 10 ship.
        let research_points = ResearchPoints(points_by_ship.iter().map(|ship| ship.points.0).sum());

        LineReset {
            research_points,
            points_by_ship,
            regrind,
            paying_ships,
            credits_per_point: CostPerPoint { cost: regrind.purchase_credits, points: research_points },
            xp_per_point: CostPerPoint { cost: regrind.research_xp, points: research_points },
            reset_cost: None,
            sale_credits: Credits::new(sale_credits),
            net_credits_if_owned: Credits::new(regrind.purchase_credits.value() - sale_credits),
        }
    }
}

fn is_tech_tree(ship: &Ship) -> bool {
    ship.status() == ShipStatus::Released && TECH_TREE_GROUPS.contains(&ship.group())
}
