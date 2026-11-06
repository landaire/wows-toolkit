use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::game_types::GameParamId;
use wowsunpack::Rc;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Credits;
use wowsunpack::game_params::types::Doubloons;
use wowsunpack::game_params::types::Experience;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::data::GameData;
use crate::error::QueryError;

const FIND_SHIPS_LIMIT: usize = 40;

/// Groups of ships players can own. Event, clan-battle, test and retired
/// variants share display names with these and are listed only on request.
const REGULAR_GROUPS: &[&str] = &[
    "start",
    "upgradeable",
    "upgradeableExclusive",
    "upgradeableUltimate",
    "premium",
    "special",
    "specialUnsellable",
    "ultimate",
    "earlyAccess",
    "superShip",
];

/// A ship tier, 1 to 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Tier(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShipClass {
    Destroyer,
    Cruiser,
    Battleship,
    AircraftCarrier,
    Submarine,
}

impl ShipClass {
    pub fn from_species(species: Species) -> Option<Self> {
        match species {
            Species::Destroyer => Some(Self::Destroyer),
            Species::Cruiser => Some(Self::Cruiser),
            Species::Battleship => Some(Self::Battleship),
            Species::AirCarrier => Some(Self::AircraftCarrier),
            Species::Submarine => Some(Self::Submarine),
            _ => None,
        }
    }

    pub fn species(self) -> Species {
        match self {
            Self::Destroyer => Species::Destroyer,
            Self::Cruiser => Species::Cruiser,
            Self::Battleship => Species::Battleship,
            Self::AircraftCarrier => Species::AirCarrier,
            Self::Submarine => Species::Submarine,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Destroyer => "destroyer",
            Self::Cruiser => "cruiser",
            Self::Battleship => "battleship",
            Self::AircraftCarrier => "aircraft_carrier",
            Self::Submarine => "submarine",
        }
    }

    pub const ALL: [ShipClass; 5] =
        [Self::Destroyer, Self::Cruiser, Self::Battleship, Self::AircraftCarrier, Self::Submarine];
}

/// A playable ship indexed by its English name.
pub(crate) struct Ship {
    pub(crate) param: Rc<Param>,
    pub(crate) name: String,
    pub(crate) search_key: String,
    pub(crate) class: ShipClass,
    pub(crate) tier: Tier,
}

impl Ship {
    pub(crate) fn from_param(provider: &GameMetadataProvider, param: &Rc<Param>) -> Option<Self> {
        let vehicle = param.vehicle()?;
        if vehicle.is_test_ship() {
            return None;
        }

        let class = ShipClass::from_species(*param.species()?.known()?)?;
        let name = provider.localized_name_from_param(param)?;

        Some(Self { param: Rc::clone(param), search_key: search_key(&name), name, class, tier: Tier(vehicle.level()) })
    }

    pub(crate) fn group(&self) -> &str {
        self.param.vehicle().map_or("", |vehicle| vehicle.group())
    }

    pub(crate) fn is_regular(&self) -> bool {
        REGULAR_GROUPS.contains(&self.group())
    }

    pub(crate) fn summary(&self) -> ShipSummary {
        ShipSummary {
            name: self.name.clone(),
            index: self.param.index().to_owned(),
            tier: self.tier,
            class: self.class,
            nation: self.param.nation().to_owned(),
            group: self.group().to_owned(),
        }
    }

    /// "T10 Des Moines (cruiser, USA)", for compact lists.
    pub(crate) fn label(&self) -> String {
        format!("T{} {} ({}, {})", self.tier.0, self.name, self.class.label(), self.param.nation())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShipSummary {
    pub name: String,

    /// GameParams index, e.g. "PASC020". Unique per ship.
    pub index: String,

    pub tier: Tier,
    pub class: ShipClass,

    /// Nation as GameParams spells it, e.g. "USA", "Germany", "United_Kingdom".
    pub nation: String,

    /// GameParams group: "upgradeable" (tech tree), "premium", "special",
    /// "ultimate", "superShip", "start"; event and retired variants use others.
    pub group: String,
}

/// Arguments for `find_ships`. Every filter is optional.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct FindShipsArgs {
    /// Part of the ship's name, e.g. "moines". Omit to list by the other filters.
    pub name: Option<String>,

    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,
    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA", "Germany", "Japan", "Russia", "United_Kingdom".
    pub nation: Option<String>,

    /// Include event, clan-battle and retired variants. Defaults to false.
    pub include_event_variants: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindShipsOutput {
    pub total: usize,
    pub ships: Vec<ShipSummary>,

    /// Matches left out of `ships` to keep the answer short.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,
}

/// Arguments naming one ship.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ShipArgs {
    /// Ship name as the user wrote it, e.g. "Des Moines", or a GameParams index such as "PASC020".
    pub ship: String,
}

/// Price, research position and availability of one ship.
#[derive(Debug, Clone, Serialize)]
pub struct ShipOverview {
    pub ship: ShipSummary,

    /// Port price in credits. Absent when the ship is not sold for credits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_credits: Option<Credits>,

    /// GameParams `costGold`. The doubloon store price for ships sold for
    /// doubloons; see `acquisition_note` for ships it does not price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_params_doubloon_value: Option<Doubloons>,

    /// How the ship is obtained when `game_params_doubloon_value` is not its price.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquisition_note: Option<&'static str>,

    /// Ship experience needed to research it. Absent for ships outside the research tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub research_xp: Option<Experience>,

    /// Ships whose research unlocks this one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub researched_from: Vec<String>,

    /// Ships this one unlocks for research.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unlocks: Vec<String>,

    /// Ship that must be owned before this one can be bought.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_ship: Option<String>,

    /// Number of upgrade slots.
    pub upgrade_slots: usize,

    /// Other variants with the same name (event, clan, retired copies).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<ShipSummary>,
}

/// An inclusive tier range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TierRange {
    pub min: Tier,
    pub max: Tier,
}

impl TierRange {
    pub(crate) fn from_bounds(min: Option<Tier>, max: Option<Tier>) -> Option<Self> {
        if min.is_none() && max.is_none() {
            return None;
        }

        Some(Self { min: min.unwrap_or(Tier(1)), max: max.unwrap_or(Tier(11)) })
    }

    /// Matchmaking spread around `tier`: two tiers either way, within 1..=11.
    pub(crate) fn around(tier: Tier) -> Self {
        Self { min: Tier(tier.0.saturating_sub(2).max(1)), max: Tier((tier.0 + 2).min(11)) }
    }

    pub(crate) fn contains(self, tier: Tier) -> bool {
        (self.min..=self.max).contains(&tier)
    }
}

/// Ship filters shared by the list queries.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShipFilter {
    pub(crate) tiers: Option<TierRange>,
    pub(crate) class: Option<ShipClass>,
    pub(crate) nation: Option<String>,
    pub(crate) include_irregular: bool,
}

impl ShipFilter {
    pub(crate) fn matches(&self, ship: &Ship) -> bool {
        (self.include_irregular || ship.is_regular())
            && self.tiers.is_none_or(|range| range.contains(ship.tier))
            && self.class.is_none_or(|class| class == ship.class)
            && self.nation.as_deref().is_none_or(|nation| ship.param.nation().eq_ignore_ascii_case(nation))
    }
}

impl GameData {
    /// Ships by name fragment, tier, class or nation.
    pub fn find_ships(&self, args: &FindShipsArgs) -> FindShipsOutput {
        let needle = args.name.as_deref().map(search_key);
        let filter = ShipFilter {
            tiers: TierRange::from_bounds(args.min_tier, args.max_tier),
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: args.include_event_variants.unwrap_or(false),
        };

        let mut ships: Vec<&Ship> = self
            .ships
            .iter()
            .filter(|ship| needle.as_ref().is_none_or(|needle| ship.search_key.contains(needle.as_str())))
            .filter(|ship| filter.matches(ship))
            .collect();
        ships.sort_by(|left, right| right.tier.cmp(&left.tier).then_with(|| left.name.cmp(&right.name)));

        let total = ships.len();
        let listed: Vec<ShipSummary> = ships.iter().take(FIND_SHIPS_LIMIT).map(|ship| ship.summary()).collect();
        let omitted = (total > listed.len()).then(|| total - listed.len());

        FindShipsOutput { total, ships: listed, omitted }
    }

    /// The playable ship with this GameParams id, e.g. from a replay.
    pub fn ship_by_id(&self, id: GameParamId) -> Option<ShipSummary> {
        self.ships.iter().find(|ship| ship.param.id() == id).map(Ship::summary)
    }

    /// Resolves a player-written ship name or GameParams index to one ship.
    pub fn ship_summary(&self, query: &str) -> Result<ShipSummary, QueryError> {
        self.resolve_ship(query).map(Ship::summary)
    }

    /// Price, research links and availability of one ship.
    pub fn ship_overview(&self, args: &ShipArgs) -> Result<ShipOverview, QueryError> {
        let ship = self.resolve_ship(&args.ship)?;
        let Some(vehicle) = ship.param.vehicle() else {
            return Err(QueryError::ShipNotFound { query: args.ship.clone() });
        };
        let research = vehicle.research();

        // Retired copies keep their research links; only owned-ship groups are listed.
        let names = |params: &[String]| -> Vec<String> {
            params
                .iter()
                .filter(|name| self.ships.iter().any(|other| other.param.name() == name.as_str() && other.is_regular()))
                .map(|name| self.ship_display_name(name))
                .collect()
        };
        let researched_from =
            self.research_predecessors.get(ship.param.name()).map(|from| names(from)).unwrap_or_default();

        let variants = self
            .ships
            .iter()
            .filter(|other| other.name == ship.name && other.param.id() != ship.param.id())
            .map(Ship::summary)
            .collect();

        Ok(ShipOverview {
            ship: ship.summary(),
            cost_credits: research.cost_credits(),
            game_params_doubloon_value: research.cost_doubloons(),
            acquisition_note: acquisition_note(ship.group()),
            research_xp: research.cost_xp(),
            researched_from,
            unlocks: names(research.next_ships()),
            requires_ship: research.required_ship().map(|name| self.ship_display_name(name)),
            upgrade_slots: wowsunpack::game_params::types::modernization_slot_count(
                self.provider().params(),
                &ship.param,
            ),
            variants,
        })
    }

    pub(crate) fn resolve_ship(&self, query: &str) -> Result<&Ship, QueryError> {
        let key = search_key(query);

        let exact: Vec<&Ship> = self.ships.iter().filter(|ship| ship.search_key == key).collect();
        if let Some(ship) = preferred(&exact) {
            return Ok(ship);
        }

        let by_index = self.ships.iter().find(|ship| {
            ship.param.index().eq_ignore_ascii_case(query.trim())
                || ship.param.name().eq_ignore_ascii_case(query.trim())
        });
        if let Some(ship) = by_index {
            return Ok(ship);
        }

        let partial: Vec<&Ship> = self.ships.iter().filter(|ship| ship.search_key.contains(key.as_str())).collect();
        let distinct: BTreeMap<&str, &Ship> = partial.iter().map(|ship| (ship.name.as_str(), *ship)).collect();
        match distinct.len() {
            0 => Err(QueryError::ShipNotFound { query: query.to_owned() }),
            1 => preferred(&partial).ok_or_else(|| QueryError::ShipNotFound { query: query.to_owned() }),
            _ => Err(QueryError::AmbiguousShip {
                query: query.to_owned(),
                candidates: distinct.keys().take(15).map(|name| (*name).to_owned()).collect(),
            }),
        }
    }

    /// English name of a ship param, or the param name when it has none.
    pub(crate) fn ship_display_name(&self, param_name: &str) -> String {
        self.provider()
            .game_param_by_name(param_name)
            .and_then(|param| self.provider().localized_name_from_param(&param))
            .unwrap_or_else(|| param_name.to_owned())
    }
}

/// The regular variant when one exists, else the first.
fn preferred<'a>(ships: &[&'a Ship]) -> Option<&'a Ship> {
    ships.iter().find(|ship| ship.is_regular()).or_else(|| ships.first()).copied()
}

/// Groups whose GameParams doubloon value is not how the ship is obtained.
/// The real currencies are server side and absent from game data.
fn acquisition_note(group: &str) -> Option<&'static str> {
    match group {
        "ultimate" => Some(
            "Sold in the Armory for a resource such as coal, steel or research bureau points, not for doubloons. Game data does not hold that price.",
        ),
        "specialUnsellable" => Some(
            "Obtained from the Armory, events, containers or bundles, not as a doubloon store item. Game data does not hold that price.",
        ),
        "disabled" => Some("Not obtainable. The doubloon value is a placeholder."),
        _ => None,
    }
}

/// Lowercase ASCII with punctuation folded to single spaces, so names with
/// diacritics match their plain spelling.
pub(crate) fn search_key(name: &str) -> String {
    let ascii = deunicode::deunicode(name).to_lowercase();

    ascii
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_key_folds_diacritics_and_punctuation() {
        assert_eq!(search_key("Gro\u{df}er Kurf\u{fc}rst"), "grosser kurfurst");
        assert_eq!(search_key("ARP My\u{14d}k\u{14d}"), "arp myoko");
        assert_eq!(search_key("Des-Moines "), "des moines");
    }

    #[test]
    fn matchmaking_range_is_clamped() {
        assert_eq!(TierRange::around(Tier(1)), TierRange { min: Tier(1), max: Tier(3) });
        assert_eq!(TierRange::around(Tier(10)), TierRange { min: Tier(8), max: Tier(11) });
        assert_eq!(TierRange::around(Tier(11)), TierRange { min: Tier(9), max: Tier(11) });
    }
}
