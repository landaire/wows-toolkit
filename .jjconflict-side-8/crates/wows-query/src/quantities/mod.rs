//! Quantities the toolkit computes, each listed with its unit, what it
//! applies to, parameters and sources, and valued for named ships or as
//! statistics over a group of ships. `quantity_search` finds definitions by
//! the words of a question; `quantity_values` computes them.

mod ballistics;
mod factors;
mod stats;
mod torpedoes;
mod underwater;

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use serde::de::DeserializeOwned;
use serde_json::Map;
use serde_json::Value;

use crate::armor_assessment::parallel_map;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::names::search_key;
use crate::ship_variants::listing_order;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipFilter;
use crate::ships::ShipStatus;
use crate::ships::Tier;
use crate::ships::TierRange;

/// Arguments for `quantity_search`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuantitySearchArgs {
    /// Words of the question, e.g. "torpedo travel time", "torpedo spotted", "shell flight time", "submarine splash damage".
    pub query: String,

    /// Definitions listed. Defaults to 8.
    pub limit: Option<QuantityLimit>,
}

/// Number of definitions `quantity_search` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct QuantityLimit(#[schemars(range(min = 1, max = 20))] pub usize);

/// Arguments for `quantity_values`. Give ships, group, or both.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuantityValuesArgs {
    /// Quantities by name from quantity_search, each with its params, e.g. {"name": "torpedo.travel_time", "params": {"distance_km": 10}}.
    #[schemars(length(min = 1, max = 6))]
    pub quantities: Vec<QuantityRequest>,

    /// Ships to value, by name, e.g. ["Shimakaze"].
    pub ships: Option<Vec<String>>,

    /// Every ship of one class, optionally of a tier range and nation, summarized per quantity.
    pub group: Option<QuantityGroup>,
}

/// One quantity to value.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuantityRequest {
    /// Name as quantity_search lists it, e.g. "torpedo.travel_time".
    pub name: String,

    /// Parameters as quantity_search lists them, e.g. {"distance_km": 10}.
    pub params: Option<Map<String, Value>>,
}

/// A group of ships: one class, optionally narrowed by tier and nation.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuantityGroup {
    pub class: ShipClass,

    /// One tier. Use min_tier and max_tier for a range.
    pub tier: Option<Tier>,
    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,

    /// Nation as GameParams spells it, e.g. "USA".
    pub nation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuantitySearchOutput {
    pub query: String,
    pub quantities: Vec<QuantityDefinition>,

    /// Further definitions that match some of the words.
    #[serde(skip_serializing_if = "is_zero")]
    pub omitted: usize,

    pub note: &'static str,
}

/// A quantity the toolkit computes.
#[derive(Debug, Clone, Serialize)]
pub struct QuantityDefinition {
    /// Name to pass to quantity_values.
    pub name: String,
    pub unit: &'static str,
    pub applies_to: AppliesTo,
    pub description: String,
    pub parameters: Vec<Parameter>,

    /// GameParams fields and client scripts the value comes from.
    pub sources: Vec<Source>,

    #[serde(skip)]
    evaluator: Evaluator,
}

/// What one value of a quantity belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppliesTo {
    /// One value per ship, or per row of the ship such as an ammo type.
    Ship,

    /// Each shell of an ammo type and battery.
    Shell,

    /// Each torpedo a ship can carry, stock and on alternative modules.
    TorpedoOption,

    /// Each shell and depth charge of a ship.
    ShellOrDepthCharge,
}

/// A parameter of a quantity.
#[derive(Debug, Clone, Serialize)]
pub struct Parameter {
    pub name: &'static str,

    #[serde(rename = "type")]
    pub kind: ParameterType,

    pub required: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<&'static str>,

    pub description: &'static str,
}

/// The values a parameter takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterType {
    /// A number in a unit, e.g. km.
    Number { unit: &'static str },

    /// One of a fixed list.
    Choice { options: &'static [&'static str] },

    /// A ship class: destroyer, cruiser, battleship, aircraft_carrier or submarine.
    ShipClass,

    /// Free text.
    Text,
}

impl Serialize for ParameterType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let text = match self {
            Self::Number { unit } => format!("number ({unit})"),
            Self::Choice { options } => format!("one of: {}", options.join(", ")),
            Self::ShipClass => "ship class: destroyer, cruiser, battleship, aircraft_carrier or submarine".to_owned(),
            Self::Text => "text".to_owned(),
        };

        serializer.serialize_str(&text)
    }
}

/// Where a quantity's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A GameParams field, e.g. "Projectile.distanceOfDamage".
    GameParams(&'static str),

    /// A client script and what in it reads the value, e.g.
    /// "VehicleUtils.py canHit".
    ClientScript(&'static str),

    /// A toolkit computation, e.g. "ship_comparison per-ship scoring".
    Computation(&'static str),
}

impl Serialize for Source {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let text = match self {
            Self::GameParams(field) => format!("GameParams {field}"),
            Self::ClientScript(script) => format!("client script {script}"),
            Self::Computation(what) => format!("computation: {what}"),
        };

        serializer.serialize_str(&text)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct QuantityValuesOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ships: Vec<ShipQuantities>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupQuantities>,

    pub note: &'static str,
}

/// The values of every requested quantity for one ship.
#[derive(Debug, Clone, Serialize)]
pub struct ShipQuantities {
    /// "T10 Shimakaze (destroyer, Japan)".
    pub ship: String,
    pub quantities: Vec<QuantityResult>,
}

/// One quantity's values for one ship.
#[derive(Debug, Clone, Serialize)]
pub struct QuantityResult {
    pub quantity: String,
    pub unit: &'static str,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<QuantityValue>,

    /// Why there are no values, or what every value assumes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One value, labeled by what it belongs to and the build it is on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuantityValue {
    /// The torpedo, shell, depth charge or row, e.g. "Type 93 mod. 3 (modules: Type 93 mod. 3)".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,

    /// "base" (stock modules, no captain skills or upgrades) or the best-fit build with what it takes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,

    /// Absent when the quantity has no value here; note says why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Statistics of each quantity over a group of ships.
#[derive(Debug, Clone, Serialize)]
pub struct GroupQuantities {
    /// "tier 8-10 cruisers".
    pub group: String,

    /// Ships in the group, test ships included.
    pub ships: usize,

    /// Ships in closed testing among `ships`.
    #[serde(skip_serializing_if = "is_zero")]
    pub test_ships: usize,

    pub quantities: Vec<GroupSummary>,
}

/// One quantity over a group, each ship counted once.
#[derive(Debug, Clone, Serialize)]
pub struct GroupSummary {
    pub quantity: String,
    pub unit: &'static str,

    /// Which value of each ship is counted.
    pub counts: String,

    #[serde(flatten)]
    pub summary: Summary,

    /// Ships without a value: they lack what the quantity measures, or it
    /// has no value at the parameters given.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub without_value: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub by_tier: Vec<TierSummary>,
}

/// Statistics over the ships with a value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub counted: usize,

    /// Ships in closed testing among `counted`.
    #[serde(skip_serializing_if = "is_zero")]
    pub counted_test: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub median: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub lowest: Option<Extreme>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub highest: Option<Extreme>,
}

/// The ship with the lowest or highest value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Extreme {
    pub ship: String,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TierSummary {
    pub tier: Tier,

    #[serde(flatten)]
    pub summary: Summary,
}

/// Decimals a quantity's group mean and median are rounded to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Precision {
    Hundredths,

    /// For coefficients stated in thousandths, such as a 0.165 splash share.
    Thousandths,
}

impl Precision {
    fn round(self, value: f32) -> f32 {
        let scale = match self {
            Self::Hundredths => 100.0,
            Self::Thousandths => 1000.0,
        };

        ((f64::from(value) * scale).round() / scale) as f32
    }
}

/// The computation behind a definition.
#[derive(Debug, Clone)]
enum Evaluator {
    Stat(stats::StatEvaluator),
    Factor(factors::FactorEvaluator),
    Ballistic(ballistics::BallisticQuantity),
    Torpedo(torpedoes::TorpedoQuantity),
    Underwater(underwater::UnderwaterQuantity),
}

impl Evaluator {
    fn prepare(&self, game: &GameData, params: &Map<String, Value>) -> Result<Prepared, QueryError> {
        match self {
            Self::Stat(stat) => stat.prepare(params).map(Prepared::Stat),
            Self::Factor(factor) => factor.prepare(params).map(Prepared::Factor),
            Self::Ballistic(quantity) => quantity.prepare(params).map(Prepared::Ballistic),
            Self::Torpedo(quantity) => quantity.prepare(game, params).map(Prepared::Torpedo),
            Self::Underwater(quantity) => quantity.prepare(params).map(Prepared::Underwater),
        }
    }
}

/// A quantity with its parameters read.
enum Prepared {
    Stat(stats::PreparedStat),
    Factor(factors::PreparedFactor),
    Ballistic(ballistics::PreparedBallistic),
    Torpedo(torpedoes::PreparedTorpedo),
    Underwater(underwater::PreparedUnderwater),
}

impl Prepared {
    /// Which value of each ship a group counts.
    fn counts(&self) -> String {
        match self {
            Self::Stat(stat) => stat.counts(),
            Self::Factor(factor) => factor.counts(),
            Self::Ballistic(quantity) => quantity.counts(),
            Self::Torpedo(quantity) => quantity.counts(),
            Self::Underwater(quantity) => quantity.counts(),
        }
    }

    fn precision(&self) -> Precision {
        match self {
            Self::Underwater(_) => Precision::Thousandths,
            Self::Stat(_) | Self::Factor(_) | Self::Ballistic(_) | Self::Torpedo(_) => Precision::Hundredths,
        }
    }

    /// Checks that a group can count one value per ship.
    fn check_group(&self) -> Result<(), QueryError> {
        match self {
            Self::Ballistic(quantity) => quantity.check_group(),
            Self::Underwater(quantity) => quantity.check_group(),
            Self::Stat(_) | Self::Factor(_) | Self::Torpedo(_) => Ok(()),
        }
    }

    /// The quantity for each of `ships`. A failure on one ship is that
    /// ship's note.
    fn evaluate(&self, game: &GameData, ships: &[&Ship]) -> Vec<ShipEvaluation> {
        let one = |ship: &&Ship| {
            let evaluated = match self {
                Self::Stat(stat) => stat.evaluate(game, ship),
                Self::Factor(_) => unreachable!("factors are valued for every ship at once"),
                Self::Ballistic(quantity) => quantity.evaluate(game, ship),
                Self::Torpedo(quantity) => quantity.evaluate(game, ship),
                Self::Underwater(quantity) => quantity.evaluate(game, ship),
            };
            evaluated.unwrap_or_else(|error| ShipEvaluation::failed(&error))
        };

        match self {
            Self::Factor(factor) => factor.evaluate(game, ships),
            _ => parallel_map(ships, one),
        }
    }
}

/// One quantity for one ship.
pub(crate) struct ShipEvaluation {
    values: Vec<QuantityValue>,
    note: Option<String>,

    /// The value a group counts for the ship.
    counted: Option<f32>,
}

impl ShipEvaluation {
    /// `counted` and the values as shown, rounded by the family.
    fn new(values: Vec<QuantityValue>, note: Option<String>, counted: Option<f32>) -> Self {
        Self { values, note, counted }
    }

    /// No value, for the reason `note` gives.
    fn absent(note: String) -> Self {
        Self { values: Vec::new(), note: Some(note), counted: None }
    }

    fn failed(error: &QueryError) -> Self {
        Self::absent(format!("could not be computed: {error}"))
    }
}

impl QuantityValue {
    fn new(option: Option<String>, build: Option<String>, value: Option<f32>, note: Option<String>) -> Self {
        Self { option, build, value, note }
    }
}

fn is_finite_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn group_tiers(group: &QuantityGroup) -> Result<Option<TierRange>, QueryError> {
    if group.tier.is_some() && (group.min_tier.is_some() || group.max_tier.is_some()) {
        return Err(QueryError::InvalidArguments { reason: "tier cannot be combined with min_tier or max_tier" });
    }

    let tiers = group
        .tier
        .map(|tier| TierRange { min: tier, max: tier })
        .or_else(|| TierRange::from_bounds(group.min_tier, group.max_tier));
    if tiers.is_some_and(|tiers| tiers.min > tiers.max) {
        return Err(QueryError::InvalidArguments { reason: "min_tier must not exceed max_tier" });
    }

    Ok(tiers)
}

/// Parameters as `P`, rejecting unknown names.
fn read_params<P>(quantity: &str, params: &Map<String, Value>) -> Result<P, QueryError>
where
    P: DeserializeOwned,
{
    serde_json::from_value(Value::Object(params.clone()))
        .map_err(|source| QueryError::QuantityParameters { quantity: quantity.to_owned(), source })
}

impl GameData {
    /// Quantity definitions matching the words of `args.query`, best first.
    pub fn quantity_search(&self, args: &QuantitySearchArgs) -> Result<QuantitySearchOutput, QueryError> {
        const DEFAULT_LIMIT: QuantityLimit = QuantityLimit(8);
        const MAX_LIMIT: usize = 20;
        const NOTE: &str = "Call quantity_values with a name and its params: it computes the values from game data. Speeds are game knots: ships and torpedoes move 2.6854 m/s per game knot on the map, not 0.5144 m/s per nautical knot; values already apply this.";

        let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit.0) {
            return Err(QueryError::InvalidArguments { reason: "limit must be from 1 to 20" });
        }
        let words = query_words(&args.query);
        if words.is_empty() {
            return Err(QueryError::InvalidArguments { reason: "query needs at least one word" });
        }

        let mut matches: Vec<(Match, QuantityDefinition)> = self
            .quantity_definitions()
            .into_iter()
            .filter_map(|definition| {
                let found = Match::of(&words, &definition);
                (found.words > 0).then_some((found, definition))
            })
            .collect();
        matches.sort_by(|(left, left_definition), (right, right_definition)| {
            right
                .words
                .cmp(&left.words)
                .then_with(|| right.score.cmp(&left.score))
                .then_with(|| left_definition.name.cmp(&right_definition.name))
        });

        let omitted = matches.len().saturating_sub(limit.0);
        let quantities = matches.into_iter().take(limit.0).map(|(_, definition)| definition).collect();

        Ok(QuantitySearchOutput { query: args.query.clone(), quantities, omitted, note: NOTE })
    }

    /// Values of the requested quantities for named ships and statistics
    /// over a group.
    pub fn quantity_values(&self, args: &QuantityValuesArgs) -> Result<QuantityValuesOutput, QueryError> {
        const MAX_QUANTITIES: usize = 6;
        const NOTE: &str = "Values are computed from game data and final: speeds are game knots (2.6854 m/s each on the map), and arming distance, damage falloff, ballistics, skills and upgrades are applied. Quote them; do not convert or estimate.";

        if args.quantities.is_empty() || args.quantities.len() > MAX_QUANTITIES {
            return Err(QueryError::InvalidArguments { reason: "give 1 to 6 quantities" });
        }
        let named_ships = args.ships.as_ref().is_some_and(|ships| !ships.is_empty());
        if !named_ships && args.group.is_none() {
            return Err(QueryError::InvalidArguments { reason: "give ships, group, or both" });
        }

        let definitions = self.quantity_definitions();
        let empty = Map::new();
        let mut requested: Vec<(&QuantityDefinition, Prepared)> = Vec::new();
        for request in &args.quantities {
            let definition = find_definition(&definitions, &request.name)?;
            let prepared = definition.evaluator.prepare(self, request.params.as_ref().unwrap_or(&empty))?;
            if args.group.is_some() {
                prepared.check_group()?;
            }
            requested.push((definition, prepared));
        }

        let mut interpretations = Interpretations::default();
        let mut named: Vec<&Ship> = Vec::new();
        for name in args.ships.iter().flatten() {
            let ship = self.resolve_ship(name, &mut interpretations)?;
            if !named.iter().any(|other| other.param.index() == ship.param.index()) {
                named.push(ship);
            }
        }

        let ships = if named.is_empty() {
            Vec::new()
        } else {
            let evaluated: Vec<Vec<ShipEvaluation>> =
                requested.iter().map(|(_, prepared)| prepared.evaluate(self, &named)).collect();
            named
                .iter()
                .enumerate()
                .map(|(index, ship)| ShipQuantities {
                    ship: ship.label(),
                    quantities: requested
                        .iter()
                        .zip(&evaluated)
                        .map(|((definition, _), evaluations)| {
                            let evaluation = &evaluations[index];
                            QuantityResult {
                                quantity: definition.name.clone(),
                                unit: definition.unit,
                                values: evaluation.values.clone(),
                                note: evaluation.note.clone(),
                            }
                        })
                        .collect(),
                })
                .collect()
        };

        let group = match &args.group {
            Some(group) => Some(self.group_quantities(group, &requested)?),
            None => None,
        };

        Ok(QuantityValuesOutput { interpretations, ships, group, note: NOTE })
    }

    /// Every quantity definition, family by family.
    fn quantity_definitions(&self) -> Vec<QuantityDefinition> {
        let mut definitions = Vec::new();
        definitions.extend(torpedoes::definitions());
        definitions.extend(ballistics::definitions());
        definitions.extend(underwater::definitions());
        definitions.extend(factors::definitions());
        definitions.extend(stats::definitions(self));

        definitions
    }

    fn group_quantities(
        &self,
        group: &QuantityGroup,
        requested: &[(&QuantityDefinition, Prepared)],
    ) -> Result<GroupQuantities, QueryError> {
        let tiers = group_tiers(group)?;

        // As ship_comparison groups ships: test ships in, event copies and
        // replicas out.
        let filter = ShipFilter {
            tiers,
            class: Some(group.class),
            nation: group.nation.clone(),
            include_irregular: false,
            exclude_test: false,
            include_replicas: false,
        };
        let mut ships: Vec<&Ship> = self.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| listing_order(left, right));

        let quantities = requested
            .iter()
            .map(|(definition, prepared)| {
                let evaluations = prepared.evaluate(self, &ships);
                let counted: Vec<(&Ship, f32)> = ships
                    .iter()
                    .zip(&evaluations)
                    .filter_map(|(ship, evaluation)| evaluation.counted.map(|value| (*ship, value)))
                    .collect();
                let without_value = ships
                    .iter()
                    .zip(&evaluations)
                    .filter(|(_, evaluation)| evaluation.counted.is_none())
                    .map(|(ship, _)| ship.name.clone())
                    .collect();

                let mut by_tier: BTreeMap<Tier, Vec<(&Ship, f32)>> = BTreeMap::new();
                for (ship, value) in &counted {
                    by_tier.entry(ship.tier).or_default().push((ship, *value));
                }
                let by_tier = if by_tier.len() > 1 {
                    by_tier
                        .into_iter()
                        .map(|(tier, values)| TierSummary { tier, summary: summarize(&values, prepared.precision()) })
                        .collect()
                } else {
                    Vec::new()
                };

                GroupSummary {
                    quantity: definition.name.clone(),
                    unit: definition.unit,
                    counts: prepared.counts(),
                    summary: summarize(&counted, prepared.precision()),
                    without_value,
                    by_tier,
                }
            })
            .collect();

        let label = match tiers {
            Some(TierRange { min, max }) if min == max => format!("tier {}", min.0),
            Some(TierRange { min, max }) => format!("tier {}-{}", min.0, max.0),
            None => "every tier".to_owned(),
        };
        let nation = group.nation.as_deref().map(|nation| format!(" {nation}")).unwrap_or_default();

        Ok(GroupQuantities {
            group: format!("{label}{nation} {}s", group.class.label().replace('_', " ")),
            ships: ships.len(),
            test_ships: ships.iter().filter(|ship| ship.status() == ShipStatus::Test).count(),
            quantities,
        })
    }
}

/// How well a definition matches the query words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Match {
    /// Query words found anywhere in the definition.
    words: usize,

    /// Per word found: 3 in the name, 2 in the description, 1 in the sources.
    score: usize,
}

impl Match {
    fn of(words: &[String], definition: &QuantityDefinition) -> Self {
        let name = query_words(&definition.name.replace(['.', '_'], " "));
        let description = query_words(&definition.description);
        let sources: Vec<String> = definition
            .sources
            .iter()
            .flat_map(|source| {
                let text = match source {
                    Source::GameParams(text) | Source::ClientScript(text) | Source::Computation(text) => text,
                };
                query_words(text)
            })
            .collect();

        let mut found = Self { words: 0, score: 0 };
        for word in words {
            let weight = if contains_word(&name, word) {
                3
            } else if contains_word(&description, word) {
                2
            } else if contains_word(&sources, word) {
                1
            } else {
                continue;
            };
            found.words += 1;
            found.score += weight;
        }

        found
    }
}

/// The definition named `name`, or the error naming the closest ones.
fn find_definition<'d>(
    definitions: &'d [QuantityDefinition],
    name: &str,
) -> Result<&'d QuantityDefinition, QueryError> {
    let key = search_key(name);
    if let Some(definition) = definitions.iter().find(|definition| search_key(&definition.name) == key) {
        return Ok(definition);
    }

    let words = query_words(&name.replace(['.', '_'], " "));
    let mut near: Vec<(Match, &QuantityDefinition)> = definitions
        .iter()
        .map(|definition| (Match::of(&words, definition), definition))
        .filter(|(found, _)| found.words > 0)
        .collect();
    near.sort_by(|(left, _), (right, _)| right.words.cmp(&left.words).then_with(|| right.score.cmp(&left.score)));

    Err(QueryError::QuantityNotFound {
        query: name.to_owned(),
        suggestions: near.into_iter().take(5).map(|(_, definition)| definition.name.clone()).collect(),
    })
}

/// Lower-case words of `text` without the words every question has.
fn query_words(text: &str) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "a", "an", "and", "are", "at", "by", "can", "do", "does", "for", "from", "how", "in", "is", "it", "its", "of",
        "on", "or", "the", "to", "what", "when", "which", "with",
    ];

    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .filter(|word| !STOP_WORDS.contains(&word.as_str()))
        .collect()
}

/// Whether `words` holds `word` or another form of it: one of the two starts
/// with the other and the shorter has at least 4 letters ("torpedo" and
/// "torpedoes", "spot" and "spotted").
fn contains_word(words: &[String], word: &str) -> bool {
    const SHORTEST_STEM: usize = 4;

    words.iter().any(|candidate| {
        let (shorter, longer) =
            if candidate.len() <= word.len() { (candidate.as_str(), word) } else { (word, candidate.as_str()) };
        shorter == longer || (shorter.len() >= SHORTEST_STEM && longer.starts_with(shorter))
    })
}

/// Count, mean, median and extremes of `values`, ships in listing order.
fn summarize(values: &[(&Ship, f32)], precision: Precision) -> Summary {
    let counted = values.len();
    let counted_test = values.iter().filter(|(ship, _)| ship.status() == ShipStatus::Test).count();
    if values.is_empty() {
        return Summary { counted, counted_test, mean: None, median: None, lowest: None, highest: None };
    }

    let total: f64 = values.iter().map(|(_, value)| f64::from(*value)).sum();
    let mean = (total / counted as f64) as f32;

    let mut sorted: Vec<f32> = values.iter().map(|(_, value)| *value).collect();
    sorted.sort_by(f32::total_cmp);
    let middle = counted / 2;
    let median = if counted.is_multiple_of(2) { (sorted[middle - 1] + sorted[middle]) / 2.0 } else { sorted[middle] };

    // The first ship in listing order wins a tie.
    let extreme = |better: fn(f32, f32) -> bool| {
        values
            .iter()
            .copied()
            .reduce(|kept, candidate| if better(candidate.1, kept.1) { candidate } else { kept })
            .map(|(ship, value)| Extreme { ship: ship.label(), value })
    };

    Summary {
        counted,
        counted_test,
        mean: Some(precision.round(mean)),
        median: Some(precision.round(median)),
        lowest: extreme(|candidate, kept| candidate < kept),
        highest: extreme(|candidate, kept| candidate > kept),
    }
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_forms_match_on_a_shared_stem() {
        let words = query_words("Distance at which enemy ships spot a torpedo");

        assert!(contains_word(&words, "torpedoes"));
        assert!(contains_word(&words, "spotted"));
        assert!(!contains_word(&words, "he"));
        assert_eq!(query_words("How long does a torpedo take?"), ["long", "torpedo", "take"]);
    }

    #[test]
    fn range_inputs_require_finite_positive_values() {
        assert!(is_finite_positive(1.0));
        assert!(!is_finite_positive(0.0));
        assert!(!is_finite_positive(-1.0));
        assert!(!is_finite_positive(f32::NAN));
        assert!(!is_finite_positive(f32::INFINITY));
        assert!(!is_finite_positive(f32::NEG_INFINITY));
    }

    #[test]
    fn group_rejects_tier_with_a_bound() {
        let group = QuantityGroup {
            class: ShipClass::Cruiser,
            tier: Some(Tier(8)),
            min_tier: Some(Tier(7)),
            max_tier: None,
            nation: None,
        };

        assert!(matches!(group_tiers(&group), Err(QueryError::InvalidArguments { .. })));
    }
}
