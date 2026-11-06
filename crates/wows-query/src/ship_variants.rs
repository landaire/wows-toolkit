//! What each ship param is to a player, and the order in which the ships
//! sharing a name are read: "Essex" is the tech tree carrier, "old Essex" and
//! "Essex (old)" the carrier from before the rework.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::Serialize;
use wowsunpack::game_params::types::Vehicle;

use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretation;
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::names::NameKind;
use crate::names::ResolutionMethod;
use crate::names::search_key;
use crate::ships::Ship;
use crate::ships::Tier;

/// GameParams nation of the ships of event battle modes, such as the
/// Halloween and space battle ships.
const EVENT_NATION: &str = "Events";

/// What a ship param is to a player, from its GameParams group, nation and
/// English name. Ships sharing a name are read in this order: ships players
/// own and take into random battles; a ship in testing, which is what the name
/// will mean once released; the version from before a rework, which players
/// remember and compare; the rental and bot copies of a regular ship, which
/// share its stats; then ships no player can take into battle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShipCategory {
    /// Groups "start", "upgradeable", "upgradeableExclusive",
    /// "upgradeableUltimate" and "superShip".
    TechTree,

    /// Sold, awarded or exchanged for a resource: groups "premium", "special",
    /// "specialUnsellable", "ultimate" and "earlyAccess".
    Premium,

    /// In closed testing: groups starting "demo".
    Test,

    /// Group "experimental".
    Experimental,

    /// A version from before a rework: group "preserved", or a name ending
    /// "(old)" such as "Satsuma (old)".
    Old,

    /// Rented for clan battles, named in brackets such as "[Zao]": group
    /// "clan".
    ClanRental,

    /// Rented for an event or operation, such as "[Cleveland]": group "event".
    EventRental,

    /// The copy bots sail in co-op battles: group "coopOnly".
    CoopBot,

    /// A ship of an event battle mode: nation "Events".
    EventBattle,

    /// Group "disabled".
    Removed,

    /// Not obtainable, such as the tier 11 "+" copies: group "unavailable".
    Unavailable,

    /// A group none of the above name.
    Unclassified,
}

impl ShipCategory {
    pub(crate) fn classify(vehicle: &Vehicle, nation: &str, name: &str) -> Self {
        if vehicle.is_test_ship() {
            return Self::Test;
        }

        match vehicle.group() {
            "start" | "upgradeable" | "upgradeableExclusive" | "upgradeableUltimate" | "superShip" => {
                return Self::TechTree;
            }
            "premium" | "special" | "specialUnsellable" | "ultimate" | "earlyAccess" => return Self::Premium,
            // Classified below.
            _ => {}
        }

        if nation == EVENT_NATION {
            return Self::EventBattle;
        }

        if vehicle.group() == "preserved" || is_old_name(name) {
            return Self::Old;
        }

        match vehicle.group() {
            "experimental" => Self::Experimental,
            "clan" => Self::ClanRental,
            "event" => Self::EventRental,
            "coopOnly" => Self::CoopBot,
            "disabled" => Self::Removed,
            "unavailable" => Self::Unavailable,
            _ => Self::Unclassified,
        }
    }

    /// Ships players own and take into random battles.
    pub(crate) fn is_owned(self) -> bool {
        matches!(self, Self::TechTree | Self::Premium)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TechTree => "tech tree ship",
            Self::Premium => "premium or special ship",
            Self::Test => "test ship",
            Self::Experimental => "experimental ship",
            Self::Old => "old version",
            Self::ClanRental => "clan battle rental",
            Self::EventRental => "event rental",
            Self::CoopBot => "co-op bot copy",
            Self::EventBattle => "event battle ship",
            Self::Removed => "removed ship",
            Self::Unavailable => "unavailable copy",
            Self::Unclassified => "unclassified ship",
        }
    }

    /// Lists show owned ships together, then each other category.
    fn listing_section(self) -> Self {
        if self.is_owned() { Self::TechTree } else { self }
    }
}

/// Order of ships sharing a name: by category, originals before replicas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ShipRank {
    category: ShipCategory,
    replica: bool,
}

impl ShipRank {
    pub(crate) fn new(category: ShipCategory, replica: bool) -> Self {
        Self { category, replica }
    }
}

/// A word in a ship query asking for a kind of variant, as "old" in
/// "old Essex".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Qualifier {
    Old,
    Clan,
    Rental,
    Event,
    Operations,
    Test,
    Removed,
}

impl Qualifier {
    /// Search keys of the words, each matched whole at either end of a query.
    const WORDS: &[(&str, Self)] = &[
        ("old", Self::Old),
        ("legacy", Self::Old),
        ("preserved", Self::Old),
        ("clan", Self::Clan),
        ("cb", Self::Clan),
        ("rental", Self::Rental),
        ("event", Self::Event),
        ("operation", Self::Operations),
        ("operations", Self::Operations),
        ("ops", Self::Operations),
        ("coop", Self::Operations),
        ("co op", Self::Operations),
        ("bot", Self::Operations),
        ("test", Self::Test),
        ("testing", Self::Test),
        ("experimental", Self::Test),
        ("removed", Self::Removed),
        ("retired", Self::Removed),
    ];

    fn accepts(self, category: ShipCategory) -> bool {
        match self {
            Self::Old => matches!(category, ShipCategory::Old | ShipCategory::Removed),
            Self::Clan => category == ShipCategory::ClanRental,
            Self::Rental => {
                matches!(category, ShipCategory::ClanRental | ShipCategory::EventRental | ShipCategory::CoopBot)
            }
            Self::Event => matches!(category, ShipCategory::EventRental | ShipCategory::EventBattle),
            Self::Operations => matches!(category, ShipCategory::EventRental | ShipCategory::CoopBot),
            Self::Test => matches!(category, ShipCategory::Test | ShipCategory::Experimental),
            Self::Removed => matches!(category, ShipCategory::Removed | ShipCategory::Unavailable),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Old => "old version",
            Self::Clan => "clan battle rental",
            Self::Rental => "rental copy",
            Self::Event => "event version",
            Self::Operations => "co-op or operations copy",
            Self::Test => "test version",
            Self::Removed => "removed version",
        }
    }
}

/// A ship query split into a name and the qualifiers around it: "old Essex",
/// "Essex (old)", "clan Zao" or "[Zao]", whose brackets ask for a rental.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Qualified {
    /// Search key of the name.
    name: String,
    qualifiers: Vec<Qualifier>,
}

impl Qualified {
    /// `None` when `query` has no qualifier or nothing besides.
    pub(crate) fn parse(query: &str) -> Option<Self> {
        let trimmed = query.trim();
        let mut qualifiers = Vec::new();
        let inner = match trimmed.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            Some(inner) => {
                qualifiers.push(Qualifier::Rental);
                inner
            }
            None => trimmed,
        };

        let mut name = search_key(inner);
        while let Some((rest, qualifier)) = strip_qualifier(&name) {
            qualifiers.push(qualifier);
            name = rest;
        }

        (!qualifiers.is_empty() && !name.is_empty()).then_some(Self { name, qualifiers })
    }
}

/// The ships a query may name: the pick, then the others.
pub(crate) struct ShipChoice<'a> {
    /// Never empty.
    ranked: Vec<&'a Ship>,

    /// How the name was read before ships sharing it were ranked.
    lookup_method: Option<ResolutionMethod>,

    /// The query gives the pick's index, or spells its name and no other
    /// ship has that name, so the pick needs no note.
    unambiguous: bool,
}

impl<'a> ShipChoice<'a> {
    pub(crate) fn ship(&self) -> &'a Ship {
        self.ranked[0]
    }

    /// The other ships the query may name, in [`ShipRank`] order.
    pub(crate) fn alternatives(&self) -> &[&'a Ship] {
        &self.ranked[1..]
    }

    /// The name was read whole, not as part of a name or a misspelling.
    pub(crate) fn is_exact(&self) -> bool {
        !is_guess(self.lookup_method)
    }

    /// Records how `input` was read when it was not an exact unique name,
    /// with the other ships it may name.
    pub(crate) fn note(&self, input: &str, interpretations: &mut Interpretations) {
        let Some(method) = self.lookup_method.or((!self.unambiguous).then_some(ResolutionMethod::Variant)) else {
            return;
        };

        interpretations.push(Interpretation {
            input: input.to_owned(),
            resolved: self.ship().name.clone(),
            kind: NameKind::Ship,
            method,
            alternatives: self.alternatives().iter().map(|ship| ship.variant_label()).collect(),
        });
    }

    /// Records how `input` was read when it was not an exact name, leaving
    /// out the ships sharing it: "Yamato" in "Super Yamato" names no
    /// clan rental.
    pub(crate) fn note_reading(&self, input: &str, interpretations: &mut Interpretations) {
        let Some(method) = self.lookup_method else {
            return;
        };

        interpretations.push(Interpretation {
            input: input.to_owned(),
            resolved: self.ship().name.clone(),
            kind: NameKind::Ship,
            method,
            alternatives: Vec::new(),
        });
    }
}

impl GameData {
    /// The ships `query` may name: the ships its name spells, and the ships
    /// sharing their name but for brackets or "(old)". The pick is a
    /// GameParams index or name the query gives, else the best of ships the
    /// query's qualifiers ask for, ships of `tier`, ships the name spells
    /// exactly, and the [`ShipRank`]. The others follow by [`ShipRank`].
    pub(crate) fn choose_ship(&self, query: &str, tier: Option<Tier>) -> Result<ShipChoice<'_>, QueryError> {
        let qualified = Qualified::parse(query);
        let (resolved_name, lookup_method, targets, qualifiers) = match self.ship_names.lookup(query) {
            Lookup::Found(resolved, targets) if !is_guess(resolved.method) => {
                // "Test Sub 1" is a name, not a test version of "Sub 1".
                let qualifiers = match &qualified {
                    Some(qualified) if family_key(resolved.name) == qualified.name => qualified.qualifiers.clone(),
                    _ => Vec::new(),
                };
                (resolved.name, resolved.method, targets, qualifiers)
            }
            whole => {
                let rest = qualified.as_ref().map(|qualified| (self.ship_names.lookup(&qualified.name), qualified));
                match (rest, whole) {
                    // A guess at the rest stands only where the whole query
                    // names nothing: "test sub" is part of "Test Sub 1".
                    (Some((Lookup::Found(resolved, targets), qualified)), whole)
                        if !is_guess(resolved.method) || !matches!(whole, Lookup::Found(..)) =>
                    {
                        (resolved.name, resolved.method, targets, qualified.qualifiers.clone())
                    }
                    (_, Lookup::Found(resolved, targets)) => (resolved.name, resolved.method, targets, Vec::new()),
                    (_, Lookup::Ambiguous(candidates)) => {
                        return Err(QueryError::AmbiguousShip { query: query.to_owned(), candidates });
                    }
                    (_, Lookup::NotFound(suggestions)) => {
                        return Err(QueryError::ShipNotFound { query: query.to_owned(), suggestions });
                    }
                }
            }
        };

        let named: Vec<&Ship> = targets.iter().map(|index| &self.ships[*index]).collect();
        let identifier = query.trim();
        let pinned = named
            .iter()
            .find(|ship| {
                ship.param.index().eq_ignore_ascii_case(identifier) || ship.param.name().eq_ignore_ascii_case(identifier)
            })
            .map(|ship| ship.param.id());
        let families: BTreeSet<&str> = named.iter().map(|ship| ship.family_key.as_str()).collect();

        let mut ranked: Vec<&Ship> = named.clone();
        ranked.extend(
            self.ships
                .iter()
                .filter(|ship| families.contains(ship.family_key.as_str()))
                .filter(|ship| !named.iter().any(|named| named.param.id() == ship.param.id())),
        );
        let is_named = |ship: &Ship| named.iter().any(|named| named.param.id() == ship.param.id());
        let key = |ship: &Ship| ChoiceKey {
            unpinned: pinned.is_some_and(|id| id != ship.param.id()),
            unqualified: !qualifiers.iter().all(|qualifier| qualifier.accepts(ship.category)),
            other_tier: tier.is_some_and(|tier| tier != ship.tier),
            // A qualifier reaches past the exact spelling: "old Essex" is not
            // the removed "Essex".
            unnamed: qualifiers.is_empty() && !is_named(ship),
            rank: ship.rank(),
        };
        let by_name = |left: &&Ship, right: &&Ship| {
            left.name.cmp(&right.name).then_with(|| left.param.index().cmp(right.param.index()))
        };
        ranked.sort_by(|left, right| key(left).cmp(&key(right)).then_with(|| by_name(left, right)));
        let Some((&mut ship, alternatives)) = ranked.split_first_mut() else {
            return Err(QueryError::ShipNotFound { query: query.to_owned(), suggestions: vec![resolved_name.to_owned()] });
        };
        alternatives.sort_by(|left, right| left.rank().cmp(&right.rank()).then_with(|| by_name(left, right)));

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
        if pinned.is_none()
            && let Some(qualifier) = qualifiers.iter().find(|qualifier| !qualifier.accepts(ship.category))
        {
            return Err(QueryError::NoShipVariant {
                query: query.to_owned(),
                ship: ship.name.clone(),
                variant: qualifier.label(),
            });
        }

        let unambiguous = pinned.is_some()
            || (ranked.len() == 1 && qualifiers.is_empty())
            || (search_key(query) == ship.search_key && ranked[1..].iter().all(|other| other.name != ship.name));

        Ok(ShipChoice { ranked, lookup_method, unambiguous })
    }
}

/// How a candidate fits a query, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ChoiceKey {
    unpinned: bool,
    unqualified: bool,
    other_tier: bool,
    unnamed: bool,
    rank: ShipRank,
}

/// Order of ship lists: owned ships first, then the other categories in
/// [`ShipCategory`] order; within each, higher tiers first, then by name,
/// with a ship's variants after it.
pub(crate) fn listing_order(left: &Ship, right: &Ship) -> Ordering {
    left.category
        .listing_section()
        .cmp(&right.category.listing_section())
        .then_with(|| right.tier.cmp(&left.tier))
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.rank().cmp(&right.rank()))
        .then_with(|| left.param.index().cmp(right.param.index()))
}

/// Search key shared by a ship and its variants: "Essex (old)" and "[Zao]"
/// are keyed as "Essex" and "Zao" are.
pub(crate) fn family_key(name: &str) -> String {
    let key = search_key(name);
    if is_old_name(name)
        && let Some(base) = key.strip_suffix(" old")
    {
        return base.to_owned();
    }

    key
}

fn is_old_name(name: &str) -> bool {
    name.trim_end().to_ascii_lowercase().ends_with("(old)")
}

/// A partial name or a misspelling, which a qualifier must not stand on.
fn is_guess(method: Option<ResolutionMethod>) -> bool {
    matches!(method, Some(ResolutionMethod::Partial | ResolutionMethod::Fuzzy))
}

/// `key` without one qualifier word at its start or end.
fn strip_qualifier(key: &str) -> Option<(String, Qualifier)> {
    Qualifier::WORDS.iter().find_map(|(word, qualifier)| {
        let rest = key
            .strip_prefix(word)
            .and_then(|rest| rest.strip_prefix(' '))
            .or_else(|| key.strip_suffix(word).and_then(|rest| rest.strip_suffix(' ')))?;

        Some((rest.to_owned(), *qualifier))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifiers_split_from_the_name() {
        let parsed: Vec<String> = [
            "old Essex",
            "Essex (old)",
            "Kiev (OLD)",
            "[Zao]",
            "clan Zao",
            "Schlieffen co-op",
            "operations Schlieffen",
            "test Vrijheid",
            "Essex",
            "old",
            "Oldenburg",
            "Test Sub 1",
        ]
        .into_iter()
        .map(|query| format!("{query:?} -> {:?}", Qualified::parse(query)))
        .collect();

        insta::assert_snapshot!(parsed.join("\n"), @r#"
        "old Essex" -> Some(Qualified { name: "essex", qualifiers: [Old] })
        "Essex (old)" -> Some(Qualified { name: "essex", qualifiers: [Old] })
        "Kiev (OLD)" -> Some(Qualified { name: "kiev", qualifiers: [Old] })
        "[Zao]" -> Some(Qualified { name: "zao", qualifiers: [Rental] })
        "clan Zao" -> Some(Qualified { name: "zao", qualifiers: [Clan] })
        "Schlieffen co-op" -> Some(Qualified { name: "schlieffen", qualifiers: [Operations] })
        "operations Schlieffen" -> Some(Qualified { name: "schlieffen", qualifiers: [Operations] })
        "test Vrijheid" -> Some(Qualified { name: "vrijheid", qualifiers: [Test] })
        "Essex" -> None
        "old" -> None
        "Oldenburg" -> None
        "Test Sub 1" -> Some(Qualified { name: "sub 1", qualifiers: [Test] })
        "#);
    }

    #[test]
    fn family_keys_drop_variant_marks() {
        let keys: Vec<String> = ["Essex (old)", "[Za\u{14d}]", "Clausewitz +", "Kiev (OLD)", "Tone 2", "Oldendorf"]
            .into_iter()
            .map(|name| ascii(&format!("{name} -> {}", family_key(name))))
            .collect();

        insta::assert_snapshot!(keys.join("\n"), @r"
        Essex (old) -> essex
        [Za\u{14d}] -> zao
        Clausewitz + -> clausewitz
        Kiev (OLD) -> kiev
        Tone 2 -> tone 2
        Oldendorf -> oldendorf
        ");
    }

    /// `text` with letters outside ASCII escaped, so snapshots in this file
    /// stay ASCII.
    fn ascii(text: &str) -> String {
        text.chars()
            .map(|character| {
                if character.is_ascii() { character.to_string() } else { character.escape_unicode().to_string() }
            })
            .collect()
    }

    fn game() -> GameData {
        let root =
            std::path::PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");

        GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
    }

    /// Ships per category, with the groups and nations each category holds.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn categories_of_the_indexed_ships() {
        let game = game();
        let mut categories: std::collections::BTreeMap<ShipCategory, std::collections::BTreeMap<String, usize>> =
            std::collections::BTreeMap::new();
        for ship in &game.ships {
            let source = match ship.param.nation() {
                EVENT_NATION => format!("{} ({EVENT_NATION})", ship.group()),
                _ => ship.group().to_owned(),
            };
            *categories.entry(ship.category).or_default().entry(source).or_default() += 1;
        }
        let lines: Vec<String> = categories
            .iter()
            .map(|(category, sources)| {
                let total: usize = sources.values().sum();
                let sources: Vec<String> = sources.iter().map(|(source, count)| format!("{source} {count}")).collect();
                format!("{category:?} {total}: {}", sources.join(", "))
            })
            .collect();

        insta::assert_snapshot!(lines.join("\n"), @"
        TechTree 418: start 13, superShip 23, upgradeable 382
        Premium 536: earlyAccess 8, premium 14, special 386, specialUnsellable 15, ultimate 113
        Test 52: demoWithoutStats 21, demoWithoutStatsPrem 31
        Experimental 6: experimental 6
        Old 44: disabled 2, preserved 42
        ClanRental 13: clan 13
        EventRental 7: event 7
        CoopBot 1: coopOnly 1
        EventBattle 105: disabled (Events) 96, event (Events) 1, unavailable (Events) 8
        Removed 30: disabled 30
        Unavailable 6: unavailable 6
        ");
    }

    /// Every name or search key several ships share, read as a query: the
    /// pick, then the alternatives. An owned ship wins whenever one shares
    /// the name.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn shared_names_read_as_an_owned_ship() {
        let game = game();
        let mut families: std::collections::BTreeMap<&str, Vec<&Ship>> = std::collections::BTreeMap::new();
        for ship in &game.ships {
            families.entry(ship.family_key.as_str()).or_default().push(ship);
        }

        let mut lines = Vec::new();
        for (key, members) in families.iter().filter(|(_, members)| members.len() > 1) {
            let mut queries: BTreeSet<&str> = BTreeSet::from([*key]);
            for ship in members {
                if members.iter().filter(|other| other.name == ship.name).count() > 1 {
                    queries.insert(ship.name.as_str());
                }
            }

            for query in queries {
                let choice = game.choose_ship(query, None).expect("a shared name resolves");
                let pick = choice.ship();
                let mut sharing = members.iter().filter(|ship| ship.name == query || ship.search_key == query);
                assert!(
                    pick.category.is_owned() || !sharing.any(|ship| ship.category.is_owned()),
                    "{query:?} reads as {}",
                    pick.variant_label()
                );

                let alternatives: Vec<String> = choice.alternatives().iter().map(|ship| ship.variant_label()).collect();
                lines.push(ascii(&format!("{query} -> {}; {}", pick.variant_label(), alternatives.join("; "))));
            }
        }

        insta::assert_snapshot!(lines.join("\n"), @r"
        ARP Kirishima -> ARP Kirishima (premium or special ship, replica of Kong\u{14d}): T5 battleship, PJSB706; ARP Kirishima (removed ship): T5 battleship, PJSB799
        arp kirishima -> ARP Kirishima (premium or special ship, replica of Kong\u{14d}): T5 battleship, PJSB706; ARP Kirishima (removed ship): T5 battleship, PJSB799
        audacious -> Audacious (tech tree ship): T10 aircraft_carrier, PBSA210; [Audacious] (clan battle rental): T10 aircraft_carrier, PBSA910
        Aurora -> Aurora (premium or special ship): T3 cruiser, PRSC001; Aurora (event battle ship): T7 cruiser, PXSC007
        aurora -> Aurora (premium or special ship): T3 cruiser, PRSC001; Aurora (event battle ship): T7 cruiser, PXSC007
        balao -> Balao (tech tree ship): T10 submarine, PASS210; Balao (old) (old version): T10 submarine, PASS110
        baltimore -> Baltimore (tech tree ship): T8 cruiser, PASC108; Baltimore (old) (old version): T9 cruiser, PASC017
        Brennus -> Brennus (premium or special ship): T10 cruiser, PFSC710; Brennus (removed ship): T10 cruiser, PFSC810
        brennus -> Brennus (premium or special ship): T10 cruiser, PFSC710; Brennus (removed ship): T10 cruiser, PFSC810
        Buffalo -> Buffalo (tech tree ship): T9 cruiser, PASC109; Buffalo (removed ship): T10 cruiser, PASC019
        buffalo -> Buffalo (tech tree ship): T9 cruiser, PASC109; Buffalo (removed ship): T10 cruiser, PASC019
        cachalot -> Cachalot (tech tree ship): T6 submarine, PASS206; Cachalot (old) (old version): T6 submarine, PASS106
        clausewitz -> Clausewitz (tech tree ship): T11 cruiser, PGSC111; Clausewitz + (unavailable copy): T11 cruiser, PGSC891
        cleveland -> Cleveland (tech tree ship): T8 cruiser, PASC208; Cleveland (old) (old version): T6 cruiser, PASC007; [Cleveland] (event rental): T8 cruiser, PASC998
        conqueror -> Conqueror (tech tree ship): T10 battleship, PBSB110; [Conqueror] (clan battle rental): T10 battleship, PBSB910
        enterprise -> Enterprise (premium or special ship): T8 aircraft_carrier, PASA518; Enterprise (old) (old version): T8 aircraft_carrier, PASA508
        Essex -> Essex (tech tree ship): T10 aircraft_carrier, PASA020; Essex (old) (old version): T9 aircraft_carrier, PASA013; Essex (removed ship): T10 aircraft_carrier, PASA910
        essex -> Essex (tech tree ship): T10 aircraft_carrier, PASA020; Essex (old) (old version): T9 aircraft_carrier, PASA013; Essex (removed ship): T10 aircraft_carrier, PASA910
        fubuki -> Fubuki (tech tree ship): T6 destroyer, PJSD106; Fubuki (old) (old version): T8 destroyer, PJSD007
        g kurfurst -> G. Kurf\u{fc}rst (premium or special ship): T10 battleship, PGSB110; [G. Kurf\u{fc}rst] (clan battle rental): T10 battleship, PGSB910
        gearing -> Gearing (tech tree ship): T10 destroyer, PASD013; [Gearing] (clan battle rental): T10 destroyer, PASD913
        gnevny -> Gnevny (tech tree ship): T6 destroyer, PRSD206; Gnevny (OLD) (old version): T5 destroyer, PRSD105
        graf zeppelin -> Graf Zeppelin (premium or special ship): T8 aircraft_carrier, PGSA518; Graf Zeppelin (old) (old version): T8 aircraft_carrier, PGSA508
        grozovoi -> Grozovoi (tech tree ship): T10 destroyer, PRSD210; [Grozovoi] (clan battle rental): T10 destroyer, PRSD910
        hakuryu -> Hakury\u{16b} (tech tree ship): T10 aircraft_carrier, PJSA110; Hakury\u{16b} (old) (old version): T10 aircraft_carrier, PJSA017; [Hakury\u{16b}] (clan battle rental): T10 aircraft_carrier, PJSA917
        hannover -> Hannover (tech tree ship): T11 battleship, PGSB111; Hannover (old) (old version): T11 battleship, PGSB999
        hatsuharu -> Hatsuharu (tech tree ship): T6 destroyer, PJSD206; Hatsuharu (old) (old version): T7 destroyer, PJSD006
        Hector -> Hector (premium or special ship): T9 cruiser, PUSC509; Hector (event battle ship): T10 destroyer, PXSD103
        hector -> Hector (premium or special ship): T9 cruiser, PUSC509; Hector (event battle ship): T10 destroyer, PXSD103
        hindenburg -> Hindenburg (tech tree ship): T10 cruiser, PGSC110; [Hindenburg] (clan battle rental): T10 cruiser, PGSC910
        hosho -> H\u{14d}sh\u{14d} (tech tree ship): T4 aircraft_carrier, PJSA104; H\u{14d}sh\u{14d} (old) (old version): T4 aircraft_carrier, PJSA002
        independence -> Independence (tech tree ship): T6 aircraft_carrier, PASA026; Independence (old) (old version): T6 aircraft_carrier, PASA006
        j humphreys -> J. Humphreys (tech tree ship): T11 destroyer, PASD111; J. Humphreys + (unavailable copy): T11 destroyer, PASD891
        jacksonville -> Jacksonville (tech tree ship): T11 cruiser, PASC011; Jacksonville + (unavailable copy): T11 cruiser, PASC891
        kaga -> Kaga (premium or special ship): T8 aircraft_carrier, PJSA518; Kaga (old) (old version): T7 aircraft_carrier, PJSA507
        kagero -> Kager\u{14d} (tech tree ship): T8 destroyer, PJSD208; Kager\u{14d} (old) (old version): T9 destroyer, PJSD010
        kiev -> Kiev (tech tree ship): T8 destroyer, PRSD308; Kiev (OLD) (old version): T7 destroyer, PRSD109
        kirov -> Kirov (premium or special ship): T5 cruiser, PRSC525; Kirov (OLD) (old version): T5 cruiser, PRSC105
        Kitakami -> Kitakami (premium or special ship): T10 cruiser, PJSC610; Kitakami (removed ship): T10 cruiser, PJSC014
        kitakami -> Kitakami (premium or special ship): T10 cruiser, PJSC610; Kitakami (removed ship): T10 cruiser, PJSC014
        la galissonniere -> La Galissonni\u{e8}re (tech tree ship): T6 cruiser, PFSC106; [La Galissonni\u{e8}re] (event rental): T6 cruiser, PFSC896
        langley -> Langley (tech tree ship): T4 aircraft_carrier, PASA104; Langley (old) (old version): T4 aircraft_carrier, PASA004
        lexington -> Lexington (tech tree ship): T8 aircraft_carrier, PASA108; Lexington (old) (old version): T8 aircraft_carrier, PASA012
        maine -> Maine (tech tree ship): T11 battleship, PASB111; Maine + (unavailable copy): T11 battleship, PASB891
        midway -> Midway (tech tree ship): T10 aircraft_carrier, PASA110; Midway (old) (old version): T10 aircraft_carrier, PASA015; [Midway] (clan battle rental): T10 aircraft_carrier, PASA915
        montana -> Montana (tech tree ship): T10 battleship, PASB017; [Montana] (clan battle rental): T10 battleship, PASB917
        moskva -> Moskva (premium or special ship): T10 cruiser, PRSC110; [Moskva] (clan battle rental): T10 cruiser, PRSC910
        mutsuki -> Mutsuki (tech tree ship): T5 destroyer, PJSD105; Mutsuki (old) (old version): T6 destroyer, PJSD005
        new orleans -> New Orleans (tech tree ship): T7 cruiser, PASC107; New Orleans (old) (old version): T8 cruiser, PASC014
        ognevoi -> Ognevoi (tech tree ship): T8 destroyer, PRSD208; Ognevoi (OLD) (old version): T6 destroyer, PRSD106
        pensacola -> Pensacola (tech tree ship): T6 cruiser, PASC106; Pensacola (old) (old version): T7 cruiser, PASC012
        Ranger -> Ranger (tech tree ship): T6 aircraft_carrier, PASA106; Ranger (old) (old version): T7 aircraft_carrier, PASA010; Ranger (event battle ship): T6 aircraft_carrier, PXSA807
        ranger -> Ranger (tech tree ship): T6 aircraft_carrier, PASA106; Ranger (old) (old version): T7 aircraft_carrier, PASA010; Ranger (event battle ship): T6 aircraft_carrier, PXSA807
        richelieu -> Richelieu (tech tree ship): T8 battleship, PFSB108; [Richelieu] (event rental): T8 battleship, PFSB898
        ryujo -> Ry\u{16b}j\u{14d} (tech tree ship): T6 aircraft_carrier, PJSA106; Ry\u{16b}j\u{14d} (old) (old version): T6 aircraft_carrier, PJSA009
        saipan -> Saipan (premium or special ship): T8 aircraft_carrier, PASA528; Saipan (old) (old version): T7 aircraft_carrier, PASA507
        salmon -> Salmon (tech tree ship): T8 submarine, PASS208; Salmon (old) (old version): T8 submarine, PASS108
        satsuma -> Satsuma (tech tree ship): T11 battleship, PJSB111; Satsuma (old) (old version): T11 battleship, PJSB999; Satsuma + (unavailable copy): T11 battleship, PJSB891
        Schlieffen -> Schlieffen (tech tree ship): T10 battleship, PGSB210; Schlieffen (co-op bot copy): T10 battleship, PGSB920
        schlieffen -> Schlieffen (tech tree ship): T10 battleship, PGSB210; Schlieffen (co-op bot copy): T10 battleship, PGSB920
        shimakaze -> Shimakaze (tech tree ship): T10 destroyer, PJSD012; [Shimakaze] (clan battle rental): T10 destroyer, PJSD912
        shokaku -> Sh\u{14d}kaku (tech tree ship): T8 aircraft_carrier, PJSA108; Sh\u{14d}kaku (old) (old version): T8 aircraft_carrier, PJSA012
        taiho -> Taih\u{14d} (removed ship): T10 aircraft_carrier, PJSA910; Taih\u{14d} (old) (old version): T9 aircraft_carrier, PJSA015
        tashkent -> Tashkent (tech tree ship): T9 destroyer, PRSD409; Tashkent (OLD) (old version): T8 destroyer, PRSD108
        u 190 -> U-190 (tech tree ship): T8 submarine, PGSS208; U-190 (old) (old version): T8 submarine, PGSS108
        u 2501 -> U-2501 (tech tree ship): T10 submarine, PGSS210; U-2501 (old) (old version): T10 submarine, PGSS110
        u 69 -> U-69 (tech tree ship): T6 submarine, PGSS206; U-69 (old) (old version): T6 submarine, PGSS106
        Valkyrie -> Valkyrie (tech tree ship): T3 destroyer, PBSD103; Valkyrie (event battle ship): T9 cruiser, PXSC101
        valkyrie -> Valkyrie (tech tree ship): T3 destroyer, PBSD103; Valkyrie (event battle ship): T9 cruiser, PXSC101
        Vrijheid -> Vrijheid (premium or special ship): T8 cruiser, PHSC708; Vrijheid (test ship): T10 cruiser, PHSC990
        vrijheid -> Vrijheid (premium or special ship): T8 cruiser, PHSC708; Vrijheid (test ship): T10 cruiser, PHSC990
        Worcester -> Worcester (tech tree ship): T10 cruiser, PASC210; Worcester (removed ship): T10 cruiser, PASC016
        worcester -> Worcester (tech tree ship): T10 cruiser, PASC210; Worcester (removed ship): T10 cruiser, PASC016
        yamagiri -> Yamagiri (tech tree ship): T11 destroyer, PJSD111; Yamagiri + (unavailable copy): T11 destroyer, PJSD891
        yamato -> Yamato (tech tree ship): T10 battleship, PJSB018; [Yamato] (clan battle rental): T10 battleship, PJSB918
        zao -> Za\u{14d} (tech tree ship): T10 cruiser, PJSC034; [Za\u{14d}] (clan battle rental): T10 cruiser, PJSC934
        ");
    }

    /// A GameParams index or name names its own ship, whichever ships share
    /// its English name.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn every_ship_resolves_by_index_and_param_name() {
        let game = game();
        for ship in &game.ships {
            for identifier in [ship.param.index(), ship.param.name()] {
                let mut interpretations = Interpretations::default();
                let resolved = game.resolve_ship(identifier, &mut interpretations).expect("an identifier resolves");
                assert_eq!(resolved.param.id(), ship.param.id(), "{identifier} names {}", resolved.variant_label());
            }
        }
    }
}
