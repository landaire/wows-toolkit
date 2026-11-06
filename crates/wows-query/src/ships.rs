use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::game_types::GameParamId;
use wowsunpack::Rc;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Credits;
use wowsunpack::game_params::types::Experience;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::armament::Armament;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretation;
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::names::NameIndex;
use crate::names::NameKind;
use crate::names::ResolutionMethod;
use crate::names::search_key;
use crate::text::clean;

const FIND_SHIPS_LIMIT: usize = 40;

/// Full names of ships whose in-game name is abbreviated, and community
/// abbreviations, each with the in-game English name. Every target is
/// checked against the data by the `alias_tables_resolve_to_their_targets` test.
pub(crate) const SHIP_ALIASES: &[(&str, &str)] = &[
    ("Gro\u{df}er Kurf\u{fc}rst", "G. Kurf\u{fc}rst"),
    ("GK", "G. Kurf\u{fc}rst"),
    ("Friedrich der Gro\u{df}e", "F. der Grosse"),
    ("FdG", "F. der Grosse"),
    ("DM", "Des Moines"),
    ("Dmitri Donskoi", "Dm. Donskoi"),
    ("Dmitry Donskoy", "Dm. Donskoi"),
    ("Dmitri Pozharsky", "D. Pozharsky"),
    ("Dmitry Pozharsky", "D. Pozharsky"),
    ("Attilio Regolo", "A. Regolo"),
    ("Francesco Ferruccio", "F. Ferruccio"),
    ("Forrest Sherman", "F. Sherman"),
    ("Prinz Eitel Friedrich", "P. E. Friedrich"),
];

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

/// A ship tier, 1 to 11. Arguments also accept "supership" for tier 11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Tier(#[schemars(range(min = 1, max = 11))] pub u32);

impl<'de> Deserialize<'de> for Tier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(TierVisitor)
    }
}

struct TierVisitor;

impl serde::de::Visitor<'_> for TierVisitor {
    type Value = Tier;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a tier from 1 to 11 or \"supership\"")
    }

    fn visit_u64<E>(self, value: u64) -> Result<Tier, E>
    where
        E: serde::de::Error,
    {
        u32::try_from(value).map(Tier).map_err(|_| E::invalid_value(serde::de::Unexpected::Unsigned(value), &self))
    }

    fn visit_str<E>(self, value: &str) -> Result<Tier, E>
    where
        E: serde::de::Error,
    {
        if ["supership", "superships"].iter().any(|name| value.eq_ignore_ascii_case(name)) {
            return Ok(Tier(11));
        }

        Err(E::invalid_value(serde::de::Unexpected::Str(value), &self))
    }
}

/// A ship class. Arguments also accept the community abbreviations (DD, CA, CL, BB, CV, SS) and capitalized names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShipClass {
    #[serde(alias = "DD", alias = "dd", alias = "Destroyer")]
    Destroyer,
    #[serde(alias = "CA", alias = "ca", alias = "CL", alias = "cl", alias = "Cruiser")]
    Cruiser,
    #[serde(alias = "BB", alias = "bb", alias = "Battleship")]
    Battleship,
    #[serde(alias = "CV", alias = "cv", alias = "carrier", alias = "Carrier", alias = "AircraftCarrier")]
    AircraftCarrier,
    #[serde(alias = "SS", alias = "ss", alias = "sub", alias = "Sub", alias = "Submarine")]
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

/// How many ships a list holds, in total and per class.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ShipCounts {
    pub total: usize,
    pub by_class: BTreeMap<ShipClass, usize>,

    /// Ships in closed testing among `total`.
    #[serde(skip_serializing_if = "is_zero")]
    pub test: usize,
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

impl ShipCounts {
    pub(crate) fn from_ships<'a>(ships: impl IntoIterator<Item = &'a Ship>) -> Self {
        let mut counts = Self::default();
        for ship in ships {
            counts.total += 1;
            *counts.by_class.entry(ship.class).or_default() += 1;
            if ship.status() == ShipStatus::Test {
                counts.test += 1;
            }
        }

        counts
    }
}

/// A ship indexed by its English name, including ships in closed testing.
pub(crate) struct Ship {
    pub(crate) param: Rc<Param>,
    pub(crate) name: String,
    pub(crate) search_key: String,
    pub(crate) class: ShipClass,
    pub(crate) tier: Tier,

    /// A released ship has the same name: this is a test copy of it, listed
    /// only with event variants.
    pub(crate) copies_released: bool,
}

impl Ship {
    pub(crate) fn from_param(provider: &GameMetadataProvider, param: &Rc<Param>) -> Option<Self> {
        let vehicle = param.vehicle()?;
        let class = ShipClass::from_species(*param.species()?.known()?)?;
        let name = clean(&provider.localized_name_from_param(param)?);

        Some(Self {
            param: Rc::clone(param),
            search_key: search_key(&name),
            name,
            class,
            tier: Tier(vehicle.level()),
            copies_released: false,
        })
    }

    /// Marks test ships that copy a released ship of the same name, such as
    /// "[Conqueror]".
    pub(crate) fn mark_test_copies(ships: &mut [Ship]) {
        let released: std::collections::BTreeSet<String> =
            ships.iter().filter(|ship| ship.is_regular()).map(|ship| ship.search_key.clone()).collect();
        for ship in ships.iter_mut() {
            ship.copies_released = ship.status() == ShipStatus::Test && released.contains(&ship.search_key);
        }
    }

    pub(crate) fn group(&self) -> &str {
        self.param.vehicle().map_or("", |vehicle| vehicle.group())
    }

    pub(crate) fn is_regular(&self) -> bool {
        REGULAR_GROUPS.contains(&self.group())
    }

    pub(crate) fn status(&self) -> ShipStatus {
        let test = self.group() == TEST_GROUP || self.param.vehicle().is_some_and(|vehicle| vehicle.is_test_ship());
        if test { ShipStatus::Test } else { ShipStatus::Released }
    }

    pub(crate) fn summary(&self) -> ShipSummary {
        let status = self.status();
        ShipSummary {
            name: self.name.clone(),
            index: self.param.index().to_owned(),
            tier: self.tier,
            class: self.class,
            nation: self.param.nation().to_owned(),
            group: self.group().to_owned(),
            status,
            status_note: (status == ShipStatus::Test).then_some(TEST_SHIP_NOTE),
        }
    }

    /// "T10 Des Moines (cruiser, USA)", for compact lists; test ships end in
    /// ", test".
    pub(crate) fn label(&self) -> String {
        let test = if self.status() == ShipStatus::Test { ", test" } else { "" };
        format!("T{} {} ({}, {}{test})", self.tier.0, self.name, self.class.label(), self.param.nation())
    }
}

/// GameParams group of ships in closed testing.
const TEST_GROUP: &str = "demoWithoutStats";

const TEST_SHIP_NOTE: &str = "test ship, values from the current client may change before release";

/// Whether a ship is released or in closed testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShipStatus {
    Released,
    Test,
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

    pub status: ShipStatus,

    /// Present for test ships.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_note: Option<&'static str>,
}

/// Arguments for `find_ships`. Every filter is optional.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindShipsArgs {
    /// Ship name or part of it, e.g. "moines". A name that names one ship exactly, by an alias such as "DM", by its initials or by a former name lists that ship first. Omit to list by the other filters.
    pub name: Option<String>,

    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,
    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA", "Germany", "Japan", "Russia", "United_Kingdom".
    pub nation: Option<String>,

    /// Include event, clan-battle and retired variants. Defaults to false.
    pub include_event_variants: Option<bool>,

    /// Leave out ships in closed testing. Defaults to false: they are listed with status "test". A test copy of a released ship is listed only with event variants.
    pub exclude_test_ships: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindShipsOutput {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub total: usize,

    /// Ships in closed testing among `total`.
    #[serde(skip_serializing_if = "is_zero")]
    pub test: usize,

    pub ships: Vec<FoundShip>,

    /// Matches left out of `ships` to keep the answer short.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FoundShip {
    #[serde(flatten)]
    pub ship: ShipSummary,

    /// How the name filter matched; absent without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_match: Option<NameMatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NameMatch {
    /// The name names this ship, as single-ship tools read it.
    Named,
    /// The ship's name contains the name.
    Partial,
}

/// Arguments naming one ship.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipArgs {
    /// Ship name as the user wrote it, e.g. "Des Moines", or a GameParams index such as "PASC020".
    pub ship: String,
}

/// Arguments for `ship_overview`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipOverviewArgs {
    /// Ship name as the user wrote it, e.g. "Des Moines", or a GameParams index such as "PASC020".
    pub ship: String,

    /// Modules whose armament to report instead of stock, e.g. ["Hull B"]. Omit for stock.
    pub modules: Option<Vec<String>>,
}

/// A ship resolved from a player-written name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedShip {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,
}

/// Price, research position, availability and armament of one ship.
#[derive(Debug, Clone, Serialize)]
pub struct ShipOverview {
    /// Names read as something they do not spell exactly.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    /// Port price in credits. Absent when the ship is not sold for credits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_credits: Option<Credits>,

    /// How the ship is obtained when it is not researched or sold in the port.
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

    /// Armament of the stock modules, or of the modules given.
    pub armament: Armament,
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

/// Ship filters shared by the list queries. Test ships are included unless
/// excluded; a test copy of a released ship counts as a variant.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShipFilter {
    pub(crate) tiers: Option<TierRange>,
    pub(crate) class: Option<ShipClass>,
    pub(crate) nation: Option<String>,
    pub(crate) include_irregular: bool,
    pub(crate) exclude_test: bool,
}

impl ShipFilter {
    pub(crate) fn matches(&self, ship: &Ship) -> bool {
        let listed = match ship.status() {
            ShipStatus::Test => !self.exclude_test && (!ship.copies_released || self.include_irregular),
            ShipStatus::Released => self.include_irregular || ship.is_regular(),
        };

        listed
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
            exclude_test: args.exclude_test_ships.unwrap_or(false),
        };

        // A name single-ship tools resolve without guessing leads the list,
        // so "DM" lists Des Moines before Dm. Donskoi.
        let mut interpretations = Interpretations::default();
        let named = match args.name.as_deref() {
            Some(name) => self.named_ships(name, &filter, &mut interpretations),
            None => Vec::new(),
        };

        let mut partial: Vec<&Ship> = self
            .ships
            .iter()
            .filter(|ship| needle.as_ref().is_none_or(|needle| ship.search_key.contains(needle.as_str())))
            .filter(|ship| filter.matches(ship))
            .filter(|ship| !named.iter().any(|named| named.param.id() == ship.param.id()))
            .collect();
        partial.sort_by(|left, right| right.tier.cmp(&left.tier).then_with(|| left.name.cmp(&right.name)));

        let partial_match = needle.as_ref().map(|_| NameMatch::Partial);
        let ships: Vec<(&Ship, Option<NameMatch>)> = named
            .iter()
            .map(|ship| (*ship, Some(NameMatch::Named)))
            .chain(partial.iter().map(|ship| (*ship, partial_match)))
            .collect();

        let total = ships.len();
        let test = ships.iter().filter(|(ship, _)| ship.status() == ShipStatus::Test).count();
        let listed: Vec<FoundShip> = ships
            .iter()
            .take(FIND_SHIPS_LIMIT)
            .map(|(ship, name_match)| FoundShip { ship: ship.summary(), name_match: *name_match })
            .collect();
        let omitted = (total > listed.len()).then(|| total - listed.len());

        FindShipsOutput { interpretations, total, test, ships: listed, omitted }
    }

    /// The playable ship with this GameParams id, e.g. from a replay.
    pub fn ship_by_id(&self, id: GameParamId) -> Option<ShipSummary> {
        self.ships.iter().find(|ship| ship.param.id() == id).map(Ship::summary)
    }

    /// Resolves a player-written ship name or GameParams index to one ship.
    pub fn ship_summary(&self, query: &str) -> Result<ResolvedShip, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(query, &mut interpretations)?.summary();

        Ok(ResolvedShip { interpretations, ship })
    }

    /// Price, research links, availability and armament of one ship.
    pub fn ship_overview(&self, args: &ShipOverviewArgs) -> Result<ShipOverview, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let Some(vehicle) = ship.param.vehicle() else {
            return Err(QueryError::ShipNotFound { query: args.ship.clone(), suggestions: Vec::new() });
        };
        let modules = self.resolve_modules(ship, args.modules.as_deref().unwrap_or_default())?;
        // Stock modules always fit; checking them could only lose the price data.
        if !modules.chosen.is_empty() {
            self.check_hull_fit(ship, &modules)?;
        }
        let armament = self.armament(ship, &modules.selection);
        let research = vehicle.research();

        // Retired copies keep their research links; only owned-ship groups
        // and ships in testing are listed.
        let names = |params: &[String]| -> Vec<String> {
            params
                .iter()
                .filter(|name| {
                    self.ships.iter().any(|other| {
                        other.param.name() == name.as_str()
                            && (other.is_regular() || (other.status() == ShipStatus::Test && !other.copies_released))
                    })
                })
                .map(|name| self.ship_display_name(name))
                .collect()
        };
        let researched_from =
            self.research_predecessors.get(ship.param.name()).map(|from| names(from)).unwrap_or_default();

        let variants = self
            .ships
            .iter()
            .filter(|other| other.search_key == ship.search_key && other.param.id() != ship.param.id())
            .map(Ship::summary)
            .collect();

        Ok(ShipOverview {
            interpretations,
            ship: ship.summary(),
            cost_credits: research.cost_credits(),
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
            armament,
        })
    }

    /// The ship `query` names. A ship name shared by event or retired copies
    /// resolves to the regular variant. "Super <ship>" names the tier 11 ship
    /// a tier 10 ship leads to.
    pub(crate) fn resolve_ship(&self, query: &str, interpretations: &mut Interpretations) -> Result<&Ship, QueryError> {
        if let Some(supership) = self.resolve_supership(query, interpretations) {
            return supership;
        }

        self.resolve_named_ship(query, interpretations)
    }

    fn resolve_named_ship(&self, query: &str, interpretations: &mut Interpretations) -> Result<&Ship, QueryError> {
        match self.ship_names.lookup(query) {
            Lookup::Found(resolved, targets) => {
                let ships: Vec<&Ship> = targets.iter().map(|index| &self.ships[*index]).collect();
                let ship = preferred(&ships)
                    .ok_or_else(|| QueryError::ShipNotFound { query: query.to_owned(), suggestions: Vec::new() })?;
                interpretations.note(query, &resolved, NameKind::Ship);
                Ok(ship)
            }
            Lookup::Ambiguous(candidates) => Err(QueryError::AmbiguousShip { query: query.to_owned(), candidates }),
            Lookup::NotFound(suggestions) => Err(QueryError::ShipNotFound { query: query.to_owned(), suggestions }),
        }
    }

    /// The tier 11 research successor of the tier 10 ship a "super <ship>"
    /// query names. `None` when `query` is not of that form, or names no ship
    /// or one not of tier 10, so the query is read as a plain name.
    fn resolve_supership(
        &self,
        query: &str,
        interpretations: &mut Interpretations,
    ) -> Option<Result<&Ship, QueryError>> {
        let base_query = supership_base(query)?;
        let mut base_interpretations = Interpretations::default();
        let base = match self.resolve_named_ship(base_query, &mut base_interpretations) {
            Ok(base) => base,
            Err(error @ QueryError::AmbiguousShip { .. }) => return Some(Err(error)),
            Err(_) => return None,
        };
        if base.tier != Tier(10) {
            return None;
        }

        let next = base.param.vehicle().map(|vehicle| vehicle.research().next_ships()).unwrap_or_default();
        let successors: Vec<&Ship> = self
            .ships
            .iter()
            .filter(|ship| ship.tier == Tier(11) && next.iter().any(|name| name == ship.param.name()))
            .collect();
        let Some(successor) = preferred(&successors) else {
            return Some(Err(QueryError::NoSupership { query: query.to_owned(), ship: base.name.clone() }));
        };

        interpretations.append(base_interpretations);
        interpretations.push(Interpretation {
            input: query.to_owned(),
            resolved: successor.name.clone(),
            kind: NameKind::Ship,
            method: ResolutionMethod::Supership,
        });

        Some(Ok(successor))
    }

    /// The ships `name` names as the single-ship tools read it, without
    /// partial names or misspellings except in the ship a "super <ship>" name
    /// leads from, that pass `filter`.
    fn named_ships(&self, name: &str, filter: &ShipFilter, interpretations: &mut Interpretations) -> Vec<&Ship> {
        let mut supership_interpretations = Interpretations::default();
        if let Some(Ok(ship)) = self.resolve_supership(name, &mut supership_interpretations) {
            if !filter.matches(ship) {
                return Vec::new();
            }

            interpretations.append(supership_interpretations);
            return vec![ship];
        }

        match self.ship_names.lookup(name) {
            Lookup::Found(resolved, targets)
                if !matches!(resolved.method, Some(ResolutionMethod::Partial | ResolutionMethod::Fuzzy)) =>
            {
                let named: Vec<&Ship> =
                    targets.iter().map(|index| &self.ships[*index]).filter(|ship| filter.matches(ship)).collect();
                if !named.is_empty() {
                    interpretations.note(name, &resolved, NameKind::Ship);
                }
                named
            }
            _ => Vec::new(),
        }
    }

    /// English name of a ship param, or the param name when it has none.
    pub(crate) fn ship_display_name(&self, param_name: &str) -> String {
        self.provider()
            .game_param_by_name(param_name)
            .and_then(|param| self.provider().localized_name_from_param(&param))
            .map_or_else(|| param_name.to_owned(), |name| clean(&name))
    }
}

/// The ship name in "super <name>" or "super-<name>", any case.
fn supership_base(query: &str) -> Option<&str> {
    let query = query.trim();
    let (word, rest) = query.split_at_checked(5)?;
    let base = rest.strip_prefix([' ', '-'])?.trim();

    (word.eq_ignore_ascii_case("super") && !base.is_empty()).then_some(base)
}

/// The regular variant when one exists, else the first.
fn preferred<'a>(ships: &[&'a Ship]) -> Option<&'a Ship> {
    ships.iter().find(|ship| ship.is_regular()).or_else(|| ships.first()).copied()
}

/// Groups of ships obtained outside the port. The prices are server side and
/// absent from game data.
fn acquisition_note(group: &str) -> Option<&'static str> {
    match group {
        "ultimate" => Some(
            "Sold in the Armory for a resource such as coal, steel or research bureau points, not for doubloons. Game data does not hold that price.",
        ),
        "specialUnsellable" => Some(
            "Obtained from the Armory, events, containers or bundles, not as a doubloon store item. Game data does not hold that price.",
        ),
        "disabled" => Some("Not obtainable."),
        _ => None,
    }
}

/// Ship names, GameParams indexes and names, and [`SHIP_ALIASES`].
pub(crate) fn ship_names(ships: &[Ship]) -> NameIndex<usize> {
    let mut names = NameIndex::default();
    for (index, ship) in ships.iter().enumerate() {
        names.add_name(&ship.name, index, !ship.is_regular());
        names.add_id(ship.param.index(), &ship.name, index);
        names.add_id(ship.param.name(), &ship.name, index);
    }
    for (alias, name) in SHIP_ALIASES {
        names.add_alias(alias, name, ResolutionMethod::ExactAlias);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matchmaking_range_is_clamped() {
        assert_eq!(TierRange::around(Tier(1)), TierRange { min: Tier(1), max: Tier(3) });
        assert_eq!(TierRange::around(Tier(10)), TierRange { min: Tier(8), max: Tier(11) });
        assert_eq!(TierRange::around(Tier(11)), TierRange { min: Tier(9), max: Tier(11) });
    }

    #[test]
    fn class_arguments_accept_community_abbreviations() {
        let parsed: Vec<String> = ["DD", "cl", "BB", "CV", "SS", "sub", "Cruiser", "aircraft_carrier"]
            .into_iter()
            .map(|name| {
                format!("{name} -> {:?}", serde_json::from_value::<ShipClass>(serde_json::Value::from(name)).unwrap())
            })
            .collect();

        insta::assert_snapshot!(parsed.join("\n"), @"
        DD -> Destroyer
        cl -> Cruiser
        BB -> Battleship
        CV -> AircraftCarrier
        SS -> Submarine
        sub -> Submarine
        Cruiser -> Cruiser
        aircraft_carrier -> AircraftCarrier
        ");
    }

    #[test]
    fn tier_arguments_accept_supership() {
        let parsed: Vec<String> =
            [json!(10), json!("supership"), json!("Superships"), json!("super"), json!(-1), json!(true)]
                .into_iter()
                .map(|value| match serde_json::from_value::<Tier>(value.clone()) {
                    Ok(tier) => format!("{value} -> {}", tier.0),
                    Err(error) => format!("{value} -> {error}"),
                })
                .collect();

        insta::assert_snapshot!(parsed.join("\n"), @r#"
        10 -> 10
        "supership" -> 11
        "Superships" -> 11
        "super" -> invalid value: string "super", expected a tier from 1 to 11 or "supership"
        -1 -> invalid type: integer `-1`, expected a tier from 1 to 11 or "supership"
        true -> invalid type: boolean `true`, expected a tier from 1 to 11 or "supership"
        "#);
    }

    #[test]
    fn supership_queries_name_their_base_ship() {
        let bases: Vec<String> =
            ["Super Colombo", "super-montana", "  SUPER  Des Moines ", "Superb", "super", "super-"]
                .into_iter()
                .map(|query| format!("{query:?} -> {:?}", supership_base(query)))
                .collect();

        insta::assert_snapshot!(bases.join("\n"), @r#"
        "Super Colombo" -> Some("Colombo")
        "super-montana" -> Some("montana")
        "  SUPER  Des Moines " -> Some("Des Moines")
        "Superb" -> None
        "super" -> None
        "super-" -> None
        "#);
    }
}
