use std::collections::BTreeMap;
use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::game_types::GameParamId;
use wows_core::units::Percent;
use wowsunpack::Rc;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Credits;
use wowsunpack::game_params::types::Experience;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::armament::Armament;
use crate::builds::Planner;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::round_to;
use crate::firing_range::FiringRanges;
use crate::names::Interpretation;
use crate::names::Interpretations;
use crate::names::NameIndex;
use crate::names::NameKind;
use crate::names::ResolutionMethod;
use crate::names::search_key;
use crate::ship_variants::ShipCategory;
use crate::ship_variants::ShipRank;
use crate::ship_variants::family_key;
use crate::ship_variants::listing_order;
use crate::text::clean;

const FIND_SHIPS_LIMIT: usize = 40;

/// Full names of ships whose in-game name is abbreviated, and community
/// abbreviations, each with the in-game English name. Every target is
/// checked against the data by the `alias_tables_resolve_to_their_targets` test.
pub(crate) const SHIP_ALIASES: &[(&str, &str)] = &[
    ("Gro\u{df}er Kurf\u{fc}rst", "G. Kurf\u{fc}rst"),
    ("GK", "G. Kurf\u{fc}rst"),
    ("Grober", "G. Kurf\u{fc}rst"),
    ("Mega Grober", "Hannover"),
    ("Brik", "Vermont"),
    ("Brick", "Vermont"),
    ("PvO", "Prins van Oranje"),
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

/// Qualifying ships as a percent of the ships considered, to 0.1 percent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shares {
    /// Absent when no ship was considered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share_pct: Option<Percent>,

    /// Every considered class, 0 when none of it qualifies.
    pub share_pct_by_class: BTreeMap<ShipClass, Percent>,
}

impl Shares {
    pub(crate) fn new(qualifying: &BTreeMap<ShipClass, usize>, considered: &ShipCounts) -> Self {
        // Ship counts stay far below 2^24, so the f32 conversion is exact.
        let share = |part: usize, whole: usize| Percent::new(round_to(part as f32 * 100.0 / whole as f32, 10.0));
        let qualifying_total: usize = qualifying.values().sum();

        Self {
            share_pct: (considered.total > 0).then(|| share(qualifying_total, considered.total)),
            share_pct_by_class: considered
                .by_class
                .iter()
                .map(|(class, whole)| (*class, share(qualifying.get(class).copied().unwrap_or_default(), *whole)))
                .collect(),
        }
    }
}

/// A ship indexed by its English name, including ships in closed testing.
pub(crate) struct Ship {
    pub(crate) param: Rc<Param>,
    pub(crate) name: String,
    pub(crate) search_key: String,
    pub(crate) class: ShipClass,
    pub(crate) tier: Tier,
    pub(crate) category: ShipCategory,

    /// GameParams `group` of the vehicle, such as "special" or "upgradeable".
    pub(crate) group: String,

    /// Search key the ship shares with its variants; see [`family_key`].
    pub(crate) family_key: String,

    /// A released ship has the same name: this is a test copy of it, listed
    /// only with event variants.
    pub(crate) copies_released: bool,

    /// The owned ship this one replicates; listed only on request.
    pub(crate) replica_of: Option<ReplicaOf>,

    /// English names of the replicas of this ship, sorted.
    pub(crate) replicas: Vec<String>,
}

impl Ship {
    pub(crate) fn from_param(provider: &GameMetadataProvider, param: &Rc<Param>) -> Option<Self> {
        let vehicle = param.vehicle()?;
        let class = ShipClass::from_species(*param.species()?.known()?)?;
        let name = clean(&provider.localized_name_from_param(param)?);

        Some(Self {
            param: Rc::clone(param),
            search_key: search_key(&name),
            family_key: family_key(&name),
            category: ShipCategory::classify(vehicle, param.nation(), &name),
            name,
            class,
            tier: Tier(vehicle.level()),
            group: vehicle.group().to_owned(),
            copies_released: false,
            replica_of: None,
            replicas: Vec::new(),
        })
    }

    /// This ship with `param` in place of its own, such as the same ship with
    /// its main battery in its alternative fire mode.
    pub(crate) fn with_param(&self, param: Rc<Param>) -> Self {
        Self {
            param,
            name: self.name.clone(),
            search_key: self.search_key.clone(),
            class: self.class,
            tier: self.tier,
            category: self.category,
            group: self.group.clone(),
            family_key: self.family_key.clone(),
            copies_released: self.copies_released,
            replica_of: self.replica_of.clone(),
            replicas: self.replicas.clone(),
        }
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

    /// Links owned ships that GameParams marks as replicas (`originShipName`)
    /// to their owned original, such as "BA Montana" to Montana or Xuan Wu
    /// to Iowa. Clan, event and test replicas stay event variants.
    pub(crate) fn mark_replicas(ships: &mut [Ship]) {
        let by_param: HashMap<&str, usize> =
            ships.iter().enumerate().map(|(index, ship)| (ship.param.name(), index)).collect();
        let owned = |ship: &Ship| ship.is_regular() && ship.status() == ShipStatus::Released;
        let links: Vec<(usize, usize)> = ships
            .iter()
            .enumerate()
            .filter(|(_, ship)| owned(ship))
            .filter_map(|(index, ship)| {
                let origin = *by_param.get(ship.param.vehicle()?.origin_ship()?)?;

                (origin != index && owned(&ships[origin])).then_some((index, origin))
            })
            .collect();

        for (replica, origin) in links {
            let replica_of = ReplicaOf::new(&ships[replica], &ships[origin]);
            let name = ships[replica].name.clone();
            ships[replica].replica_of = Some(replica_of);
            ships[origin].replicas.push(name);
        }
        for ship in ships.iter_mut() {
            ship.replicas.sort();
        }
    }

    pub(crate) fn is_replica(&self) -> bool {
        self.replica_of.is_some()
    }

    pub(crate) fn group(&self) -> &str {
        &self.group
    }

    /// Where the ship stands among ships sharing its name.
    pub(crate) fn rank(&self) -> ShipRank {
        ShipRank::new(self.category, self.is_replica())
    }

    pub(crate) fn is_regular(&self) -> bool {
        self.category.is_owned()
    }

    pub(crate) fn status(&self) -> ShipStatus {
        if self.category == ShipCategory::Test { ShipStatus::Test } else { ShipStatus::Released }
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
            category: self.category,
            status,
            status_note: (status == ShipStatus::Test).then_some(TEST_SHIP_NOTE),
            replica_of: self.replica_of.clone(),
            replicas: self.replicas.clone(),
        }
    }

    /// "premium" or "test" for a ship not of the tech tree, which players
    /// get otherwise than by research. `None` for tech tree ships, and for
    /// rentals, bot copies and other variants their names and listing
    /// sections already set apart.
    pub(crate) fn obtained_note(&self) -> Option<&'static str> {
        match self.category {
            ShipCategory::Premium => Some("premium"),
            ShipCategory::Test => Some("test"),
            _ => None,
        }
    }

    /// "T10 Des Moines (cruiser, USA)", for compact lists; premium and test
    /// ships end in ", premium" or ", test" and replicas name their original.
    pub(crate) fn label(&self) -> String {
        let obtained = self.obtained_note().map(|note| format!(", {note}")).unwrap_or_default();
        let replica = self.replica_of.as_ref().map(|of| format!(", replica of {}", of.ship)).unwrap_or_default();
        format!("T{} {} ({}, {}{obtained}{replica})", self.tier.0, self.name, self.class.label(), self.param.nation())
    }

    /// "Schlieffen (co-op bot copy): T10 battleship, PGSB920", naming one of
    /// the ships that share a name.
    pub(crate) fn variant_label(&self) -> String {
        let category = match &self.replica_of {
            Some(of) => format!("{}, replica of {}", self.category.label(), of.ship),
            None => self.category.label().to_owned(),
        };

        format!("{} ({category}): T{} {}, {}", self.name, self.tier.0, self.class.label(), self.param.index())
    }

    /// [`Self::label`] followed by the replicas that lists leave out, e.g.
    /// "T10 Montana (battleship, USA) [replicas: BA Montana]".
    pub(crate) fn listing_label(&self) -> String {
        if self.replicas.is_empty() {
            return self.label();
        }

        format!("{} [replicas: {}]", self.label(), self.replicas.join(", "))
    }
}

const REPLICA_BENEFITS: &str = "premium: more credits, free commander transfer";

/// The original of a premium replica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplicaOf {
    /// English name of the original.
    pub ship: String,

    /// GameParams index of the original.
    pub index: String,

    /// Nation of the original as GameParams spells it.
    pub nation: String,

    /// Cosmetic series as GameParams spells it, e.g. "azurlane", "arpeggio", "ba", "decorative". Absent on many replicas, collaboration ones included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,

    /// "replica of Montana; premium: more credits, free commander transfer". The replica's own stats apply; they can differ from the original's.
    pub note: String,
}

impl ReplicaOf {
    fn new(replica: &Ship, origin: &Ship) -> Self {
        let series = replica.param.vehicle().and_then(|vehicle| vehicle.peculiarity()).map(str::to_owned);
        let note = if replica.param.nation() == origin.param.nation() {
            format!("replica of {}; {REPLICA_BENEFITS}", origin.name)
        } else {
            format!(
                "replica of {} ({}) in the {} tree; {REPLICA_BENEFITS}",
                origin.name,
                origin.param.nation(),
                replica.param.nation()
            )
        };

        Self {
            ship: origin.name.clone(),
            index: origin.param.index().to_owned(),
            nation: origin.param.nation().to_owned(),
            series,
            note,
        }
    }
}

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

    /// What the ship is to a player. A name several ships share reads as the
    /// first category: "tech_tree", "premium", "test", "experimental", "old",
    /// "clan_rental", "event_rental", "coop_bot", "event_battle", "removed",
    /// "unavailable", "unclassified".
    pub category: ShipCategory,

    pub status: ShipStatus,

    /// Present for test ships.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_note: Option<&'static str>,

    /// Present for a premium replica of another ship; its own stats apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replica_of: Option<ReplicaOf>,

    /// Premium replicas of this ship, e.g. "BA Montana" for Montana. Lists leave them out unless asked.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replicas: Vec<String>,
}

/// Arguments for `find_ships`. Every filter is optional.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindShipsArgs {
    /// Ship name or part of it, e.g. "moines". A name that names one ship exactly, by an alias such as "DM", by its initials or by a former name lists that ship first, followed by the ships sharing its name that the other filters admit. A leading tier, as in "T10 Fuyutsuki", lists only ships of that tier. A qualifier such as "old Essex", "[Zao]", "clan", "test" or "co-op" lists that variant first. Omit to list by the other filters.
    pub name: Option<String>,

    pub min_tier: Option<Tier>,
    pub max_tier: Option<Tier>,
    pub class: Option<ShipClass>,

    /// Nation as GameParams spells it, e.g. "USA", "Germany", "Japan", "Russia", "United_Kingdom".
    pub nation: Option<String>,

    /// Include ships players do not own: old versions, clan and event rentals, co-op bot copies, event battle ships, removed and unavailable copies. Defaults to false. They are listed after owned and test ships.
    pub include_event_variants: Option<bool>,

    /// Leave out ships in closed testing. Defaults to false: they are listed with status "test". A test copy of a released ship is listed only with event variants.
    pub exclude_test_ships: Option<bool>,

    /// List premium replicas of other ships as separate ships: collaboration and recolored versions such as "BA Montana" of Montana, and ports to another nation such as Xuan Wu of Iowa. Defaults to false: each is named in its original's `replicas`, and listed on its own only when a nation filter leaves out the original. A name that names a replica exactly lists it either way.
    pub include_replicas: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindShipsOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
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
    /// The ship shares the name of the named ship: an old version, a rental,
    /// a test or bot copy.
    Variant,
    /// The ship's name contains the name.
    Partial,
}

/// Arguments naming one ship.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipArgs {
    /// Ship name as the user wrote it, e.g. "Des Moines", or a GameParams index such as "PASC020". A name several ships share reads as the tech tree or premium ship; a qualifier, as in "old Essex", "[Zao]", "clan Zao", "test Vrijheid" or "Schlieffen co-op", or a leading tier, as in "T9 Essex", picks another.
    pub ship: String,
}

/// Arguments for `ship_overview`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipOverviewArgs {
    /// Ship name as the user wrote it, e.g. "Des Moines", or a GameParams index such as "PASC020". A name several ships share reads as the tech tree or premium ship; a qualifier, as in "old Essex", "[Zao]", "clan Zao", "test Vrijheid" or "Schlieffen co-op", or a leading tier, as in "T9 Essex", picks another.
    pub ship: String,

    /// Modules whose armament to report instead of stock, e.g. ["Hull B"]. Omit for stock.
    pub modules: Option<Vec<String>>,
}

/// A ship resolved from a player-written name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedShip {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,
}

/// Price, research position, availability and armament of one ship.
#[derive(Debug, Clone, Serialize)]
pub struct ShipOverview {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    /// Port purchase price in credits. Absent when the ship is not sold for credits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchase_credits: Option<Credits>,

    /// What the price and research fields leave out about how the ship is obtained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquisition_note: Option<&'static str>,

    /// Ship experience needed to research it. Absent for ships outside the
    /// research tree and for tier 11 ships, which cost no experience.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub research_xp: Option<Experience>,

    /// Ships whose research unlocks this one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub researched_from: Vec<String>,

    /// Ships this one unlocks for research.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unlocks: Vec<String>,

    /// Ship that must be researched before this one can be bought.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_ship: Option<String>,

    /// Number of upgrade slots. Absent when the ship's class is unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade_slots: Option<usize>,

    /// Other variants with the same name (event, clan, retired copies) and premium replicas of this ship.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<ShipSummary>,

    /// Main battery and secondary firing range: base on stock modules without
    /// upgrades or captain skills, best on the best-fit build naming the
    /// modules, upgrades and skills it assumes, and alternatives such as a
    /// range consumable or conditional skill active. Independent of `modules`.
    pub firing_range: FiringRanges,

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

    /// `tier` and the tiers either side of it, within 1..=11.
    pub(crate) fn adjacent(tier: Tier) -> Self {
        Self { min: Tier(tier.0.saturating_sub(1).max(1)), max: Tier((tier.0 + 1).min(11)) }
    }

    pub(crate) fn contains(self, tier: Tier) -> bool {
        (self.min..=self.max).contains(&tier)
    }
}

/// Ship filters shared by the list queries. Test ships are included unless
/// excluded; a test copy of a released ship counts as a variant. Premium
/// replicas are left out unless included or their original's nation is
/// filtered out.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShipFilter {
    pub(crate) tiers: Option<TierRange>,
    pub(crate) class: Option<ShipClass>,
    pub(crate) nation: Option<String>,
    pub(crate) include_irregular: bool,
    pub(crate) exclude_test: bool,
    pub(crate) include_replicas: bool,
}

impl ShipFilter {
    pub(crate) fn matches(&self, ship: &Ship) -> bool {
        let listed = match ship.status() {
            ShipStatus::Test => !self.exclude_test && (!ship.copies_released || self.include_irregular),
            ShipStatus::Released => self.include_irregular || ship.is_regular(),
        };

        listed
            && (self.include_replicas || ship.replica_of.as_ref().is_none_or(|of| !self.lists_original(of)))
            && self.tiers.is_none_or(|range| range.contains(ship.tier))
            && self.class.is_none_or(|class| class == ship.class)
            && self.nation.as_deref().is_none_or(|nation| nation_matches(nation, ship.param.nation()))
    }

    /// Whether `of`, the original of a replica, passes the nation filter, so
    /// lists name the replica with it. A replica shares its original's tier
    /// and class, so nation is the only filter that can split them.
    fn lists_original(&self, of: &ReplicaOf) -> bool {
        self.nation.as_deref().is_none_or(|nation| nation_matches(nation, &of.nation))
    }
}

/// Player names of nations besides the GameParams ones, e.g. "UK" for
/// "United_Kingdom", as search keys: lower case, letters and digits only.
const NATION_ALIASES: &[(&str, &str)] = &[
    ("uk", "United_Kingdom"),
    ("unitedkingdom", "United_Kingdom"),
    ("britain", "United_Kingdom"),
    ("greatbritain", "United_Kingdom"),
    ("british", "United_Kingdom"),
    ("royalnavy", "United_Kingdom"),
    ("rn", "United_Kingdom"),
    ("us", "USA"),
    ("usn", "USA"),
    ("america", "USA"),
    ("american", "USA"),
    ("unitedstates", "USA"),
    ("ussr", "Russia"),
    ("soviet", "Russia"),
    ("sovietunion", "Russia"),
    ("russian", "Russia"),
    ("german", "Germany"),
    ("kms", "Germany"),
    ("japanese", "Japan"),
    ("ijn", "Japan"),
    ("french", "France"),
    ("italian", "Italy"),
    ("spanish", "Spain"),
    ("dutch", "Netherlands"),
    ("panasian", "Pan_Asia"),
    ("panamerican", "Pan_America"),
    ("european", "Europe"),
];

/// Whether `wanted`, a nation as a player or model writes it, names
/// `actual`, a GameParams nation: "united kingdom", "UK" and "British" all
/// name "United_Kingdom".
pub(crate) fn nation_matches(wanted: &str, actual: &str) -> bool {
    let key = |name: &str| name.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    let wanted = key(wanted);
    let actual = key(actual);

    wanted == actual || NATION_ALIASES.iter().any(|(alias, nation)| *alias == wanted && key(nation) == actual)
}

impl GameData {
    /// Ships by name fragment, tier, class or nation.
    pub fn find_ships(&self, args: &FindShipsArgs) -> FindShipsOutput {
        // "T10 Fuyutsuki" lists ships of that tier by the rest of the name.
        let (name, name_tier) = match args.name.as_deref() {
            Some(name) => match tier_prefix(name) {
                Some((tier, rest)) => (Some(rest), Some(tier)),
                None => (Some(name), None),
            },
            None => (None, None),
        };
        let tiers = match (TierRange::from_bounds(args.min_tier, args.max_tier), name_tier) {
            (bounds, None) => bounds,
            (None, Some(tier)) => Some(TierRange { min: tier, max: tier }),
            (Some(bounds), Some(tier)) => Some(TierRange { min: bounds.min.max(tier), max: bounds.max.min(tier) }),
        };
        let needle = name.map(search_key);
        let filter = ShipFilter {
            tiers,
            class: args.class,
            nation: args.nation.clone(),
            include_irregular: args.include_event_variants.unwrap_or(false),
            exclude_test: args.exclude_test_ships.unwrap_or(false),
            include_replicas: args.include_replicas.unwrap_or(false),
        };

        // A name single-ship tools resolve without guessing leads the list,
        // so "DM" lists Des Moines before Dm. Donskoi.
        let mut interpretations = Interpretations::default();
        let named = match name {
            Some(name) => self.named_ships(name, name_tier, &filter, &mut interpretations),
            None => Vec::new(),
        };

        let mut partial: Vec<&Ship> = self
            .ships
            .iter()
            .filter(|ship| {
                needle.as_ref().is_none_or(|needle| {
                    ship.search_key.contains(needle.as_str())
                        || ship.replicas.iter().any(|replica| search_key(replica).contains(needle.as_str()))
                })
            })
            .filter(|ship| filter.matches(ship))
            .filter(|ship| !named.iter().any(|(named, _)| named.param.id() == ship.param.id()))
            .collect();
        partial.sort_by(|left, right| listing_order(left, right));

        let partial_match = needle.as_ref().map(|_| NameMatch::Partial);
        let ships: Vec<(&Ship, Option<NameMatch>)> = named
            .iter()
            .map(|(ship, name_match)| (*ship, Some(*name_match)))
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

    /// Premium replicas of owned ships, each naming its original, by tier
    /// and name.
    pub fn replicas(&self) -> Vec<ShipSummary> {
        let mut replicas: Vec<&Ship> = self.ships.iter().filter(|ship| ship.is_replica()).collect();
        replicas.sort_by(|left, right| listing_order(left, right));

        replicas.into_iter().map(Ship::summary).collect()
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
        let armament = self.armament(ship, &modules.selection)?;
        let firing_range = self.firing_ranges(&Planner::new(self, ship))?;
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

        let is_replica_of_ship =
            |other: &Ship| other.replica_of.as_ref().is_some_and(|of| of.index == ship.param.index());
        let mut variants: Vec<&Ship> = self
            .ships
            .iter()
            .filter(|other| {
                (other.family_key == ship.family_key || is_replica_of_ship(other))
                    && other.param.id() != ship.param.id()
            })
            .collect();
        variants.sort_by_key(|other| other.rank());
        let variants = variants.into_iter().map(Ship::summary).collect();

        Ok(ShipOverview {
            interpretations,
            ship: ship.summary(),
            purchase_credits: research.cost_credits().filter(|_| sold_for_credits(ship.group())),
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
            firing_range,
            armament,
        })
    }

    /// The ship `query` names. A name several ships share reads as the one
    /// [`GameData::choose_ship`] ranks first, and `interpretations` lists the
    /// others. "Super <ship>" names the tier 11 ship a tier 10 ship leads to.
    /// A leading tier, as in "T10 Fuyutsuki" or "Tier 9 ZH-1", must be the
    /// ship's.
    pub(crate) fn resolve_ship(&self, query: &str, interpretations: &mut Interpretations) -> Result<&Ship, QueryError> {
        let (tier, name) = match tier_prefix(query) {
            Some((tier, name)) => (Some(tier), name),
            None => (None, query),
        };

        if let Some(supership) = self.resolve_supership(name, interpretations) {
            let ship = supership?;
            if let Some(tier) = tier
                && ship.tier != tier
            {
                return Err(QueryError::TierMismatch {
                    query: query.to_owned(),
                    ship: ship.name.clone(),
                    tier,
                    actual: ship.tier,
                });
            }

            return Ok(ship);
        }

        let choice = self.choose_ship(name, tier).map_err(|error| match error {
            QueryError::TierMismatch { ship, tier, actual, .. } => {
                QueryError::TierMismatch { query: query.to_owned(), ship, tier, actual }
            }
            other => other,
        })?;
        choice.note(query, interpretations);

        Ok(choice.ship())
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
        let base = match self.choose_ship(base_query, None) {
            Ok(base) => base,
            Err(error @ QueryError::AmbiguousShip { .. }) => return Some(Err(error)),
            Err(_) => return None,
        };
        if base.ship().tier != Tier(10) {
            return None;
        }

        let next = base.ship().param.vehicle().map(|vehicle| vehicle.research().next_ships()).unwrap_or_default();
        let Some(successor) = self
            .ships
            .iter()
            .filter(|ship| ship.tier == Tier(11) && next.iter().any(|name| name == ship.param.name()))
            .min_by_key(|ship| ship.rank())
        else {
            return Some(Err(QueryError::NoSupership { query: query.to_owned(), ship: base.ship().name.clone() }));
        };

        base.note_reading(base_query, interpretations);
        interpretations.push(Interpretation {
            input: query.to_owned(),
            resolved: successor.name.clone(),
            kind: NameKind::Ship,
            method: ResolutionMethod::Supership,
            alternatives: Vec::new(),
        });

        Some(Ok(successor))
    }

    /// The ships `name` names as the single-ship tools read it, without
    /// partial names or misspellings except in the ship a "super <ship>" name
    /// leads from: that ship, then the ships sharing its name that pass
    /// `filter`. The named ship passes as a replica or variant, since the name
    /// asks for it.
    fn named_ships(
        &self,
        name: &str,
        tier: Option<Tier>,
        filter: &ShipFilter,
        interpretations: &mut Interpretations,
    ) -> Vec<(&Ship, NameMatch)> {
        let named_filter = ShipFilter { include_irregular: true, include_replicas: true, ..filter.clone() };

        let mut supership_interpretations = Interpretations::default();
        if let Some(Ok(ship)) = self.resolve_supership(name, &mut supership_interpretations) {
            if !named_filter.matches(ship) {
                return Vec::new();
            }

            interpretations.append(supership_interpretations);
            return vec![(ship, NameMatch::Named)];
        }

        let Ok(choice) = self.choose_ship(name, tier) else {
            return Vec::new();
        };
        if !choice.is_exact() || !named_filter.matches(choice.ship()) {
            return Vec::new();
        }

        choice.note(name, interpretations);
        let variants = choice.alternatives().iter().filter(|ship| filter.matches(ship));

        std::iter::once((choice.ship(), NameMatch::Named))
            .chain(variants.map(|ship| (*ship, NameMatch::Variant)))
            .collect()
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

/// The tier and ship name of "T10 Fuyutsuki", "t8 Edinburgh", "Tier 9 ZH-1",
/// "Tier9 ZH-1" or "Tier X Yamato". A space must end the tier, so "T61" stays
/// a name. Roman numerals follow only "Tier".
fn tier_prefix(query: &str) -> Option<(Tier, &str)> {
    const NUMERALS: [&str; 11] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI"];

    let query = query.trim();
    let strip = |prefix: &str| {
        query.get(..prefix.len()).filter(|head| head.eq_ignore_ascii_case(prefix)).map(|_| &query[prefix.len()..])
    };
    let (rest, roman) = match (strip("tier"), strip("t")) {
        (Some(rest), _) => (rest, true),
        (None, Some(rest)) => (rest, false),
        (None, None) => return None,
    };

    let (tier, name) = rest.trim_start().split_once(char::is_whitespace)?;
    let tier = match tier.parse::<u32>() {
        Ok(tier) => tier,
        Err(_) if roman => {
            NUMERALS.iter().zip(1..).find(|(numeral, _)| numeral.eq_ignore_ascii_case(tier)).map(|(_, tier)| tier)?
        }
        Err(_) => return None,
    };
    let name = name.trim();

    ((1..=11).contains(&tier) && !name.is_empty()).then_some((Tier(tier), name))
}

/// Whether ships of `group` are bought for credits. Ships of other groups may
/// carry a nominal `costCR` that is not a purchase price. A test ship of a
/// research line carries the price it will have on release.
fn sold_for_credits(group: &str) -> bool {
    matches!(group, "upgradeable" | "superShip" | "demoWithoutStats")
}

/// What the price and research fields leave out. How a ship outside the
/// research tree is sold, for which currency and at what price, is decided on
/// the server and changes with sales and events, so the note names none.
fn acquisition_note(group: &str) -> Option<&'static str> {
    match group {
        "upgradeable" | "start" | "demoWithoutStats" => None,
        "superShip" => Some(
            "Bought for credits, with no experience cost, once the ship in requires_ship is researched. The account must also own or have researched at least three tier X ships.",
        ),
        _ => Some("How the ship is obtained and its price are not in game data."),
    }
}

/// Ship names, GameParams indexes and names, and [`SHIP_ALIASES`].
pub(crate) fn ship_names(ships: &[Ship]) -> NameIndex<usize> {
    let mut names = NameIndex::default();
    for (index, ship) in ships.iter().enumerate() {
        names.add_name(&ship.name, index, !ship.is_regular() || ship.is_replica());
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
    fn nations_match_as_players_write_them() {
        for wanted in ["United_Kingdom", "united kingdom", "UK", "British", "Royal Navy"] {
            assert!(nation_matches(wanted, "United_Kingdom"), "{wanted}");
        }
        assert!(nation_matches("Pan Asia", "Pan_Asia"));
        assert!(nation_matches("USSR", "Russia"));
        assert!(!nation_matches("UK", "USA"));
        assert!(!nation_matches("Germany", "Japan"));
    }

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

    #[test]
    fn tier_prefixes_split_from_the_name() {
        let split: Vec<String> = [
            "T10 Fuyutsuki",
            "t8 Edinburgh",
            "Tier 9 ZH-1",
            "TIER10  Des Moines ",
            "Tier X Yamato",
            "tier xi Super Montana",
            "T 10 Gearing",
            "T X Yamato",
            "T61",
            "T-61",
            "Tirpitz",
            "Tiger '59",
            "T12 X",
            "T0 X",
            "T10",
        ]
        .into_iter()
        .map(|query| format!("{query:?} -> {:?}", tier_prefix(query).map(|(tier, name)| (tier.0, name))))
        .collect();

        insta::assert_snapshot!(split.join("\n"), @r#"
        "T10 Fuyutsuki" -> Some((10, "Fuyutsuki"))
        "t8 Edinburgh" -> Some((8, "Edinburgh"))
        "Tier 9 ZH-1" -> Some((9, "ZH-1"))
        "TIER10  Des Moines " -> Some((10, "Des Moines"))
        "Tier X Yamato" -> Some((10, "Yamato"))
        "tier xi Super Montana" -> Some((11, "Super Montana"))
        "T 10 Gearing" -> Some((10, "Gearing"))
        "T X Yamato" -> None
        "T61" -> None
        "T-61" -> None
        "Tirpitz" -> None
        "Tiger '59" -> None
        "T12 X" -> None
        "T0 X" -> None
        "T10" -> None
        "#);
    }
}
