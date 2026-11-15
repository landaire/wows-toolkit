//! Ranked name search over a store's entries: param names, indexes, ids and
//! localized display names, tolerating partial names, initials-like
//! subsequences and misspellings.
//!
//! Every name is indexed when the store is built, as a search key (see
//! [`crate::names::search_key`]). A query is ranked against each key by the
//! first method that matches, best first: exact, whole words, prefix,
//! substring, misspelling within an edit budget, then fuzzy subsequence
//! (the matcher wowsbuilds' ship switcher uses). Each entry keeps its best
//! match.

use std::cmp::Reverse;
use std::collections::HashMap;

use nucleo_matcher::Matcher;
use nucleo_matcher::Utf32Str;
use nucleo_matcher::pattern::AtomKind;
use nucleo_matcher::pattern::CaseMatching;
use nucleo_matcher::pattern::Normalization;
use nucleo_matcher::pattern::Pattern;
use schemars::JsonSchema;
use serde::Serialize;

use super::EntryPosition;
use super::RawParamEntry;
use super::RawParams;
use super::Tables;
use super::format::Section;
use super::format::StringId;
use super::format::TERM_WORDS;
use crate::fuzzy;
use crate::names::search_key;

/// Which name of an entry matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TermField {
    /// Param name, e.g. `PASC020_Des_Moines_1948`.
    Name = 0,
    /// Short index, e.g. `PASC020`.
    Index = 1,
    /// Name in the store's localization, e.g. `Des Moines`.
    DisplayName = 2,
    /// Numeric id.
    Id = 3,
}

impl TermField {
    pub(crate) fn from_word(word: u32) -> Option<Self> {
        Some(match word {
            0 => Self::Name,
            1 => Self::Index,
            2 => Self::DisplayName,
            3 => Self::Id,
            _ => return None,
        })
    }
}

/// How a name matched, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchMethod {
    /// The whole name, ignoring case and punctuation.
    Exact,
    /// One or more whole words of the name, e.g. "moines".
    Words,
    /// The start of the name.
    Prefix,
    /// A part of the name.
    Substring,
    /// The name or a run of its words, misspelled by a few letters.
    Misspelling,
    /// The query's letters in order, e.g. "dsmoins".
    Subsequence,
}

/// Rank of a match: higher is better. Methods occupy bands, exact at 1000;
/// within a band a closer match scores higher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct MatchScore(pub u32);

/// One entry a query matched, by its best-matching name.
#[derive(Clone, Copy)]
pub struct SearchHit<'a> {
    pub entry: RawParamEntry<'a>,
    pub field: TermField,
    /// The name as stored.
    pub text: &'a str,
    pub method: MatchMethod,
    pub score: MatchScore,
    /// The query spells the name exactly, case and punctuation included.
    /// Wins ties between names that differ only in those, such as
    /// "[Des Moines]".
    pub spelled: bool,
}

impl SearchHit<'_> {
    fn rank(&self) -> (MatchScore, bool) {
        (self.score, self.spelled)
    }
}

/// One indexed name of an entry.
#[derive(Clone, Copy)]
pub(crate) struct Term<'a> {
    pub(crate) position: EntryPosition,
    pub(crate) field: TermField,
    pub(crate) key: &'a str,
    text: StringId,
    tables: Tables<'a>,
}

impl<'a> Term<'a> {
    /// The name as stored.
    pub(crate) fn text(&self) -> &'a str {
        self.tables.string(self.text).expect("term strings are checked when the store opens")
    }
}

impl RawParams {
    /// Entries matching `query`, best first, each by its best name. `accept`
    /// filters entries, such as by type.
    pub fn search<'a>(&'a self, query: &str, accept: impl Fn(&RawParamEntry<'a>) -> bool) -> Vec<SearchHit<'a>> {
        let key = search_key(query);
        let mut best: HashMap<EntryPosition, SearchHit<'a>> = HashMap::new();
        let mut offer = |hit: SearchHit<'a>| {
            if !accept(&hit.entry) {
                return;
            }
            best.entry(hit.entry.position())
                .and_modify(|current| {
                    if hit.rank() > current.rank() {
                        *current = hit;
                    }
                })
                .or_insert(hit);
        };

        // An exact name, index or id, whatever its case. An id match shows
        // the param name.
        let trimmed = query.trim();
        if let Some(entry) = self.find(query) {
            let (field, text) = match entry.index() {
                Some(index) if index.eq_ignore_ascii_case(trimmed) => (TermField::Index, index),
                _ if entry.name().eq_ignore_ascii_case(trimmed) => (TermField::Name, entry.name()),
                _ => (TermField::Id, entry.name()),
            };
            let spelled = text == trimmed;
            offer(SearchHit { entry, field, text, method: MatchMethod::Exact, score: SCORE_EXACT, spelled });
        }

        if key.is_empty() {
            return finish(best);
        }

        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pattern = Pattern::new(&key, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
        let mut buffer = Vec::new();
        let perfect = pattern.score(Utf32Str::new(&key, &mut buffer), &mut matcher);
        let typo_budget = fuzzy::edit_budget(&key);

        for term in self.terms() {
            let Some((method, score)) = rank(term, &key, typo_budget, perfect, &pattern, &mut matcher) else {
                continue;
            };
            let Some(entry) = self.entry(term.position) else {
                continue;
            };
            let text = term.text();
            let spelled = text.trim() == trimmed;
            offer(SearchHit { entry, field: term.field, text, method, score: MatchScore(score), spelled });
        }

        finish(best)
    }

    /// Every indexed name, in entry order.
    pub(crate) fn terms(&self) -> impl Iterator<Item = Term<'_>> {
        let tables = self.tables();

        (0..tables.count(Section::Terms)).map(move |index| {
            let word = |offset| tables.word(Section::Terms, index * TERM_WORDS + offset).expect("index below count");
            let string =
                |offset| tables.string(StringId(word(offset))).expect("term strings are checked when the store opens");

            Term {
                position: EntryPosition(word(0)),
                field: TermField::from_word(word(1)).expect("term fields are checked when the store opens"),
                key: string(3),
                text: StringId(word(2)),
                tables,
            }
        })
    }
}

const SCORE_EXACT: MatchScore = MatchScore(1000);

/// The method and score of the best way `term` matches the search key
/// `query`, if any.
fn rank(
    term: Term<'_>,
    query: &str,
    typo_budget: Option<usize>,
    perfect: Option<u32>,
    pattern: &Pattern,
    matcher: &mut Matcher,
) -> Option<(MatchMethod, u32)> {
    // Characters beyond the query's, so a closer name ranks higher.
    let extra = |key: &str| (key.len().saturating_sub(query.len())).min(99) as u32;
    let key = term.key;

    if key == query {
        return Some((MatchMethod::Exact, SCORE_EXACT.0));
    }
    // Indexes match only whole or by their start: "PASC0".
    if term.field == TermField::Index {
        return key.starts_with(query).then(|| (MatchMethod::Prefix, 700 - extra(key)));
    }
    if contains_words(key, query) {
        return Some((MatchMethod::Words, 800 - extra(key)));
    }
    if key.starts_with(query) {
        return Some((MatchMethod::Prefix, 700 - extra(key)));
    }
    if key.contains(query) {
        return Some((MatchMethod::Substring, 600 - extra(key)));
    }

    // Misspellings of display names only: param names are identifiers, and
    // their display name carries the words people type.
    if term.field == TermField::DisplayName
        && let Some(budget) = typo_budget
        && let Some(nearness) = fuzzy::nearness(query, key, budget)
    {
        let partial = if nearness.partial { 50 } else { 0 };
        return Some((MatchMethod::Misspelling, 500 - 100 * nearness.edits as u32 - partial));
    }

    // Subsequences need three letters; shorter ones match almost anything.
    if query.len() < 3 {
        return None;
    }
    let perfect = perfect?;
    let score = pattern.score(Utf32Str::Ascii(key.as_bytes()), matcher)?;
    // Scattered letters score far below a run of them; half of a perfect
    // match keeps abbreviations and drops coincidences. Scores are kept to
    // tenths of a perfect match, since finer steps follow letter positions
    // more than what the reader meant.
    let tenths = (10 * score / perfect.max(1)).min(10);

    (tenths >= 5).then(|| (MatchMethod::Subsequence, 100 + 50 * (tenths - 5)))
}

/// Whether `query` is a run of whole words of `key`; both are search keys.
fn contains_words(key: &str, query: &str) -> bool {
    key.match_indices(query).any(|(start, _)| {
        let end = start + query.len();
        (start == 0 || key.as_bytes()[start - 1] == b' ') && (end == key.len() || key.as_bytes()[end] == b' ')
    })
}

/// Hits best first; equal ranks by [`kind_rank`], then in GameParams order.
fn finish(best: HashMap<EntryPosition, SearchHit<'_>>) -> Vec<SearchHit<'_>> {
    let mut hits: Vec<SearchHit<'_>> = best.into_values().collect();
    hits.sort_by_key(|hit| (Reverse(hit.rank()), kind_rank(hit.entry.kind()), hit.entry.position()));

    hits
}

/// Order of entries with equal scores: ships, then upgrades, consumables and
/// commanders, then the rest. Readers look these up far more often than the
/// entries that describe their parts.
pub(crate) fn kind_rank(kind: Option<&str>) -> u8 {
    match kind {
        Some("Ship") => 0,
        Some("Modernization" | "Ability" | "Crew") => 1,
        _ => 2,
    }
}
