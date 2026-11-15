//! `gameparams_search`: find GameParams entries by any name a reader may
//! use, ranked, for `gameparams_query` selections.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::names::Lookup;
use crate::names::NameKind;
use crate::names::ResolutionMethod;
use crate::names::search_key;
use crate::raw_params::EntryPosition;
use crate::raw_params::MatchMethod;
use crate::raw_params::MatchScore;
use crate::raw_params::RawParamEntry;
use crate::raw_params::RawParams;
use crate::raw_params::SearchHit;
use crate::raw_params::TermField;
use crate::raw_params::kind_rank;

/// Arguments for `gameparams_search`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameParamsSearchArgs {
    /// A param name (PASC020_Des_Moines_1948), index (PASC020), numeric id,
    /// English name ("Des Moines", "Damage Control Party"), community name
    /// ("DM", "GK") or a part or misspelling of one. At most 100 characters.
    #[schemars(length(min = 1, max = 100))]
    pub query: String,

    /// Only entries of this `typeinfo.type`, e.g. Ship, Modernization,
    /// Ability, Crew, Exterior, Projectile. Text matches without regard to
    /// case.
    #[serde(rename = "type")]
    pub kind: Option<String>,

    /// Most entries listed, 1 to 50, default 10.
    #[schemars(range(min = 1, max = 50))]
    pub limit: Option<u32>,
}

/// Entries matching a search, best first.
#[derive(Debug, Clone, Serialize)]
pub struct GameParamsSearchOutput {
    /// Community names read as the names they stand for.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    /// Entries that matched, how many.
    pub matched: usize,
    pub results: Vec<ParamMatch>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// One entry and how it matched.
#[derive(Debug, Clone, Serialize)]
pub struct ParamMatch {
    pub param: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// `typeinfo.type`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub species: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<u32>,
    /// English name, for entries that have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The name that matched, as stored.
    pub matched: String,
    pub field: MatchField,
    pub method: SearchMethod,
    /// Higher is better: 1000 for an exact name, 950 for a community name,
    /// then whole words, prefix, part, misspelling and scattered letters.
    pub score: MatchScore,
}

/// Which name of an entry matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum MatchField {
    Term(TermField),
    /// A name the ship, upgrade, consumable and commander tools resolve.
    Community(CommunityField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunityField {
    CommunityName,
}

/// How a name matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SearchMethod {
    Term(MatchMethod),
    Name(ResolutionMethod),
}

impl GameData {
    /// GameParams entries named by `args.query`, ranked.
    pub fn gameparams_search(&self, args: &GameParamsSearchArgs) -> Result<GameParamsSearchOutput, QueryError> {
        const DEFAULT_LIMIT: u32 = 10;
        const MAX_LIMIT: u32 = 50;
        // Misspelling checks cost the query length times each name's length.
        const MAX_QUERY_CHARS: usize = 100;

        let len = args.query.chars().count();
        if len > MAX_QUERY_CHARS {
            return Err(QueryError::SearchQueryTooLong { len, max: MAX_QUERY_CHARS });
        }

        let store = self.raw_params()?;
        let accept = |entry: &RawParamEntry<'_>| {
            args.kind.as_deref().is_none_or(|kind| entry.kind().is_some_and(|found| found.eq_ignore_ascii_case(kind)))
        };

        let mut interpretations = Interpretations::default();
        let mut best: HashMap<EntryPosition, Ranked<'_>> = HashMap::new();
        for hit in store.search(&args.query, accept) {
            best.insert(hit.entry.position(), Ranked::from(hit));
        }
        for named in self.community_matches(&store, &args.query, &mut interpretations) {
            if !accept(&named.entry) {
                continue;
            }
            match best.entry(named.entry.position()) {
                Entry::Occupied(mut current) => {
                    if named.rank() > current.get().rank() {
                        current.insert(named);
                    }
                }
                Entry::Vacant(slot) => {
                    slot.insert(named);
                }
            }
        }

        let mut ranked: Vec<Ranked<'_>> = best.into_values().collect();
        ranked.sort_by_key(|ranked| (Reverse(ranked.rank()), kind_rank(ranked.entry.kind()), ranked.entry.position()));
        let matched = ranked.len();
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
        let results: Vec<ParamMatch> = ranked.into_iter().take(limit).map(|ranked| self.param_match(ranked)).collect();
        let hint = results.is_empty().then(|| {
            "no entry matches; try fewer words, the English name or the GameParams index, or leave out type".to_owned()
        });

        Ok(GameParamsSearchOutput { interpretations, matched, results, hint })
    }

    /// Entries named by `query` as the single-item tools read names:
    /// aliases, initialisms, former names, misspellings, superships and
    /// variants of ships, and names of upgrades, consumables and commanders.
    fn community_matches<'a>(
        &self,
        store: &'a RawParams,
        query: &str,
        interpretations: &mut Interpretations,
    ) -> Vec<Ranked<'a>> {
        let mut found = Vec::new();
        let mut add = |entry: Option<RawParamEntry<'a>>, method: Option<ResolutionMethod>, name: &str| {
            let Some(entry) = entry else {
                return;
            };
            let (method, score) = match method {
                // An exact English name, which the display name terms match too.
                None => (SearchMethod::Term(MatchMethod::Exact), MatchScore(1000)),
                Some(method) => (SearchMethod::Name(method), community_score(method)),
            };
            found.push(Ranked {
                entry,
                field: MatchField::Community(CommunityField::CommunityName),
                text: name.to_owned(),
                method,
                score,
                spelled: false,
            });
        };

        // Ships resolve as the ship tools read them; the last reading, such
        // as the tier 11 ship of "super Colombo", says how.
        let mut read = Interpretations::default();
        if let Ok(ship) = self.resolve_ship(query, &mut read) {
            let method = read.iter().filter(|interpretation| interpretation.kind == NameKind::Ship).last();
            add(store.find(ship.param.name()), method.map(|interpretation| interpretation.method), &ship.name);
            interpretations.append(read);
        }
        if let Lookup::Found(resolved, upgrades) = self.upgrade_names.lookup(query) {
            interpretations.note(query, &resolved, NameKind::Upgrade);
            for upgrade in upgrades {
                add(store.find(self.upgrades[upgrade].param.name()), resolved.method, resolved.name);
            }
        }
        if let Lookup::Found(resolved, commanders) = self.commander_names.lookup(query) {
            interpretations.note(query, &resolved, NameKind::Commander);
            for commander in commanders {
                add(store.find(&self.commanders[commander].index), resolved.method, resolved.name);
            }
        }
        // Consumables resolve to a type shared by many ability entries; the
        // entries are those displaying the resolved name.
        if let Lookup::Found(resolved, _) = self.consumable_names.lookup(query) {
            interpretations.note(query, &resolved, NameKind::Consumable);
            let key = search_key(resolved.name);
            for term in store.terms().filter(|term| term.field == TermField::DisplayName && term.key == key) {
                let entry = store.entry(term.position);
                if entry.is_some_and(|entry| entry.kind() == Some("Ability")) {
                    add(entry, resolved.method, resolved.name);
                }
            }
        }

        found
    }

    fn param_match(&self, ranked: Ranked<'_>) -> ParamMatch {
        let entry = ranked.entry;

        ParamMatch {
            param: entry.name().to_owned(),
            index: entry.index().map(str::to_owned),
            id: entry.id().map(|id| id.raw()),
            kind: entry.kind().map(str::to_owned),
            species: entry.species().map(str::to_owned),
            nation: entry.nation().map(str::to_owned),
            tier: entry.level(),
            name: self.param_display_name(entry.name()),
            matched: ranked.text,
            field: ranked.field,
            method: ranked.method,
            score: ranked.score,
        }
    }
}

/// A match before it is described.
struct Ranked<'a> {
    entry: RawParamEntry<'a>,
    field: MatchField,
    text: String,
    method: SearchMethod,
    score: MatchScore,
    /// The query spells the name exactly; wins ties.
    spelled: bool,
}

impl Ranked<'_> {
    fn rank(&self) -> (MatchScore, bool) {
        (self.score, self.spelled)
    }
}

impl<'a> From<SearchHit<'a>> for Ranked<'a> {
    fn from(hit: SearchHit<'a>) -> Self {
        Self {
            entry: hit.entry,
            field: MatchField::Term(hit.field),
            text: hit.text.to_owned(),
            method: SearchMethod::Term(hit.method),
            score: hit.score,
            spelled: hit.spelled,
        }
    }
}

/// Scores of community names: a curated name is as good as exact, while
/// partial names and misspellings rank with their store counterparts.
fn community_score(method: ResolutionMethod) -> MatchScore {
    MatchScore(match method {
        ResolutionMethod::ExactAlias
        | ResolutionMethod::Initialism
        | ResolutionMethod::Historical
        | ResolutionMethod::Supership
        | ResolutionMethod::Variant => 950,
        ResolutionMethod::Partial => 650,
        ResolutionMethod::Fuzzy => 550,
    })
}
