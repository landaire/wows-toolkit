//! Tech tree lines: research paths ending at a tier 10 ship, with the
//! research XP and purchase credits of their ships in a tier range.

use std::collections::BTreeSet;
use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wowsunpack::game_params::types::Credits;
use wowsunpack::game_params::types::Experience;

use crate::data::GameData;
use crate::error::QueryError;
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

    /// Lowest tier whose ships are costed. Defaults to 8.
    #[schemars(range(min = 1, max = 10))]
    pub min_tier: Option<Tier>,

    /// Highest tier whose ships are costed. Defaults to 10.
    #[schemars(range(min = 1, max = 10))]
    pub max_tier: Option<Tier>,

    /// Order of the lines, cheapest first. Defaults to research_xp.
    pub sort_by: Option<LineOrder>,

    /// Most lines to list. Defaults to 20.
    pub limit: Option<usize>,
}

/// Order of tech tree lines, cheapest first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineOrder {
    /// Total research XP, then total purchase credits.
    #[default]
    ResearchXp,

    /// Total purchase credits, then total research XP.
    PurchaseCredits,
}

#[derive(Debug, Clone, Serialize)]
pub struct TechTreeLinesOutput {
    /// Tiers whose ships are costed.
    pub tiers: TierRange,

    pub sort_by: LineOrder,

    /// Lines matching the filters.
    pub total: usize,

    pub lines: Vec<TechTreeLine>,

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

    /// Ship names from tier 1 to tier 10.
    pub path: Vec<String>,

    /// The highest-tier ship this line shares with another line. The line's
    /// own ships follow it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branches_from: Option<String>,

    /// The line's ships in the tier range.
    pub ships: Vec<LineShip>,

    /// Sums over `ships`.
    pub total_research_xp: Experience,
    pub total_purchase_credits: Credits,
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
    /// purchase credits of their ships in a tier range, cheapest first.
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
        let mut lines: Vec<TechTreeLine> = paths
            .iter()
            .filter(|path| filter.matches(&self.ships[path.end()]))
            .map(|path| self.tech_tree_line(path, tiers, &lines_through))
            .collect();

        let sort_by = args.sort_by.unwrap_or_default();
        lines.sort_by(|left, right| {
            let xp = left.total_research_xp.cmp(&right.total_research_xp);
            let credits = left.total_purchase_credits.cmp(&right.total_purchase_credits);
            let costs = match sort_by {
                LineOrder::ResearchXp => xp.then(credits),
                LineOrder::PurchaseCredits => credits.then(xp),
            };

            costs.then_with(|| left.tier_10_ship.cmp(&right.tier_10_ship)).then_with(|| left.path.cmp(&right.path))
        });

        let total = lines.len();
        lines.truncate(args.limit.unwrap_or(DEFAULT_LIMIT));
        let omitted = (total > lines.len()).then(|| total - lines.len());

        Ok(TechTreeLinesOutput { tiers, sort_by, total, lines, omitted })
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

        TechTreeLine {
            tier_10_ship: end.name.clone(),
            nation: end.param.nation().to_owned(),
            class: end.class,
            path: path.ships.iter().map(|ship| self.ships[*ship].name.clone()).collect(),
            branches_from,
            ships,
            total_research_xp,
            total_purchase_credits,
        }
    }
}

fn is_tech_tree(ship: &Ship) -> bool {
    ship.status() == ShipStatus::Released && TECH_TREE_GROUPS.contains(&ship.group())
}
