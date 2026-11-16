//! Typo-tolerant name lookup over search keys, used after exact, prefix and
//! substring matching find nothing.

use std::collections::BTreeMap;

/// The result of matching a misspelled name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fuzzy<'a> {
    /// One name is closest and close enough to be the one meant.
    Unique(&'a str),

    /// Names worth suggesting, closest first. Empty when nothing is near.
    Suggestions(Vec<&'a str>),
}

/// One name a query may match.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Candidate<'a> {
    /// Search key: lowercase ASCII words separated by single spaces.
    pub(crate) key: &'a str,

    pub(crate) name: &'a str,

    /// Loses ties to non-secondary names; event and retired ship copies.
    pub(crate) secondary: bool,
}

impl<'a> Candidate<'a> {
    pub(crate) fn primary(key: &'a str, name: &'a str) -> Self {
        Self { key, name, secondary: false }
    }
}

/// How near a candidate is: edits first, then whether only some of its words
/// matched, then whether it is secondary. "Kitakaze" beats "STAR Kitakaze"
/// for "kitakazee".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Nearness {
    edits: usize,
    partial: bool,
    secondary: bool,
}

/// Matches `query` (a search key) against candidates. A name matches when its
/// whole key, or a run of as many of its words as the query has, is within an
/// edit budget scaled by the query length. Words with digits must match
/// exactly: "modification 2" never resolves to "modification 1".
pub(crate) fn closest<'a>(query: &str, candidates: impl IntoIterator<Item = Candidate<'a>>) -> Fuzzy<'a> {
    const SUGGESTION_LIMIT: usize = 5;

    let Some(budget) = edit_budget(query) else {
        return Fuzzy::Suggestions(Vec::new());
    };

    // Several entries can share a name (event variants of one ship); each
    // name keeps its nearest key.
    let mut nearest: BTreeMap<&'a str, Nearness> = BTreeMap::new();
    for candidate in candidates {
        let nearness = key_nearness(query, candidate.key, budget, candidate.secondary);
        nearest.entry(candidate.name).and_modify(|best| *best = (*best).min(nearness)).or_insert(nearness);
    }

    let mut scored: Vec<(Nearness, &'a str)> = nearest.into_iter().map(|(name, nearness)| (nearness, name)).collect();
    scored.sort_unstable();

    if let [(best, name), rest @ ..] = scored.as_slice()
        && best.edits <= budget
        && rest.first().is_none_or(|(next, _)| next > best)
    {
        return Fuzzy::Unique(name);
    }

    Fuzzy::Suggestions(
        scored
            .iter()
            .take_while(|(nearness, _)| nearness.edits <= budget + 1 && nearness.edits * 2 < query.len())
            .take(SUGGESTION_LIMIT)
            .map(|(_, name)| *name)
            .collect(),
    )
}

/// Edits a query may differ by and still name one thing. Queries of three
/// characters or fewer must match exactly.
fn edit_budget(query: &str) -> Option<usize> {
    match query.len() {
        0..=3 => None,
        4..=7 => Some(1),
        8..=11 => Some(2),
        _ => Some(3),
    }
}

/// The nearer of the whole key and any run of the key's words as long as the
/// query.
fn key_nearness(query: &str, key: &str, budget: usize, secondary: bool) -> Nearness {
    let measure = |text: &str, partial: bool| {
        let edits = edit_distance(query, text);
        Nearness {
            // A digit mismatch stays out of the unique-match budget but may
            // still be suggested.
            edits: if digit_words_match(query, text) { edits } else { edits.max(budget + 1) },
            partial,
            secondary,
        }
    };
    let whole = measure(key, false);

    let query_words = query.split(' ').count();
    let starts: Vec<usize> = std::iter::once(0).chain(key.match_indices(' ').map(|(index, _)| index + 1)).collect();
    if starts.len() <= query_words {
        return whole;
    }

    (0..=starts.len() - query_words)
        .map(|first| {
            let end = starts.get(first + query_words).map_or(key.len(), |next| next - 1);
            measure(&key[starts[first]..end], true)
        })
        .fold(whole, Nearness::min)
}

/// Whether both texts have the same words containing digits, in order.
fn digit_words_match(left: &str, right: &str) -> bool {
    fn digit_words(text: &str) -> impl Iterator<Item = &str> {
        text.split(' ').filter(|word| word.bytes().any(|byte| byte.is_ascii_digit()))
    }

    digit_words(left).eq(digit_words(right))
}

/// Optimal string alignment distance over bytes (keys are ASCII):
/// insertions, deletions, substitutions and transpositions of adjacent
/// characters each cost one.
fn edit_distance(left: &str, right: &str) -> usize {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let width = right.len() + 1;

    let mut rows = vec![0usize; (left.len() + 1) * width];
    for (column, cell) in rows.iter_mut().take(width).enumerate() {
        *cell = column;
    }

    for row in 1..=left.len() {
        rows[row * width] = row;
        for column in 1..width {
            let cost = usize::from(left[row - 1] != right[column - 1]);
            let mut best = (rows[(row - 1) * width + column] + 1)
                .min(rows[row * width + column - 1] + 1)
                .min(rows[(row - 1) * width + column - 1] + cost);
            if row > 1 && column > 1 && left[row - 1] == right[column - 2] && left[row - 2] == right[column - 1] {
                best = best.min(rows[(row - 2) * width + column - 2] + 1);
            }
            rows[row * width + column] = best;
        }
    }

    rows[left.len() * width + right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Iterator<Item = Candidate<'a>> {
        pairs.iter().map(|(key, name)| Candidate::primary(key, name))
    }

    #[test]
    fn distance_counts_transpositions_once() {
        assert_eq!(edit_distance("sekryu", "sekiryu"), 1);
        assert_eq!(edit_distance("kurfurts", "kurfurst"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("yamato", "yamato"), 0);
    }

    #[test]
    fn closest_requires_a_unique_best_within_budget() {
        let names = [("des moines", "Des Moines"), ("grosser kurfurst", "Grosser Kurfurst"), ("sekiryu", "Sekiryu")];

        assert_eq!(closest("des moins", primary(&names)), Fuzzy::Unique("Des Moines"));
        assert_eq!(closest("kurfurts", primary(&names)), Fuzzy::Unique("Grosser Kurfurst"));
        assert_eq!(closest("sekryu", primary(&names)), Fuzzy::Unique("Sekiryu"));
        assert_eq!(closest("sek", primary(&names)), Fuzzy::Suggestions(Vec::new()));

        let tied = [("hood", "Hood"), ("hook", "Hook")];
        assert_eq!(closest("hoox", primary(&tied)), Fuzzy::Suggestions(vec!["Hood", "Hook"]));
    }

    #[test]
    fn whole_name_beats_a_word_of_a_longer_name() {
        let names = [("star kitakaze", "STAR Kitakaze"), ("kitakaze", "Kitakaze")];

        assert_eq!(closest("kitakazee", primary(&names)), Fuzzy::Unique("Kitakaze"));
    }

    #[test]
    fn short_query_two_edits_away_is_neither_matched_nor_suggested() {
        let names = [("hood", "Hood")];

        assert_eq!(closest("hoxx", primary(&names)), Fuzzy::Suggestions(Vec::new()));
        assert_eq!(closest("hodo", primary(&names)), Fuzzy::Unique("Hood"));
    }

    #[test]
    fn digit_words_never_resolve_to_a_different_number() {
        let names = [("concealment system modification 1", "Concealment System Modification 1")];

        assert_eq!(
            closest("concealment system modification 2", primary(&names)),
            Fuzzy::Suggestions(vec!["Concealment System Modification 1"]),
        );
        assert_eq!(
            closest("concealment sytem modification 1", primary(&names)),
            Fuzzy::Unique("Concealment System Modification 1"),
        );
    }

    #[test]
    fn secondary_names_lose_only_ties() {
        let names = [
            Candidate { key: "smolensk", name: "Smolensk", secondary: false },
            Candidate { key: "smolensk", name: "[Smolensk]", secondary: true },
            Candidate { key: "tashkent", name: "Tashkent", secondary: false },
            Candidate { key: "tashkent b", name: "Tashkent B", secondary: true },
        ];

        assert_eq!(closest("smolenks", names), Fuzzy::Unique("Smolensk"));
        assert_eq!(closest("taskhent b", names), Fuzzy::Unique("Tashkent B"));
    }

    #[test]
    fn multi_word_window_matches_inside_a_longer_name() {
        let names = [("admiral graf spee", "Admiral Graf Spee")];

        assert_eq!(closest("graf spe", primary(&names)), Fuzzy::Unique("Admiral Graf Spee"));
    }

    #[test]
    fn suggestions_stop_at_five() {
        let names = [("abcde", "A"), ("abcdf", "B"), ("abcdg", "C"), ("abcdh", "D"), ("abcdi", "E"), ("abcdj", "F")];

        assert_eq!(closest("abcdx", primary(&names)), Fuzzy::Suggestions(vec!["A", "B", "C", "D", "E"]));
    }
}
