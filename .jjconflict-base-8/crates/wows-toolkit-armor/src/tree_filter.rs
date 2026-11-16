//! What narrows a tree in the options rail: the rail's own query and the query typed into the
//! tree's own field. A row is kept when every typed query matches it, or when a row under it is
//! kept.

use std::ops::Range;
use std::rc::Rc;

use wows_toolkit_viewmodel::text_filter::FilterQuery;

/// A typed query as a tree filter reads it.
pub trait TextQuery: Clone + Default {
    fn is_active(&self) -> bool;

    /// The byte ranges of `text` the query matched; `None` when it does not match. An inactive
    /// query matches every text and marks nothing.
    fn ranges(&self, text: &str) -> Option<Rc<[Range<usize>]>>;

    fn keeps(&self, text: &str) -> bool {
        self.ranges(text).is_some()
    }
}

impl TextQuery for FilterQuery {
    fn is_active(&self) -> bool {
        FilterQuery::is_active(self)
    }

    fn ranges(&self, text: &str) -> Option<Rc<[Range<usize>]>> {
        FilterQuery::ranges(self, text).map(Rc::from)
    }
}

/// The rail's query and a tree's own, applied together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeFilter<Q> {
    rail: Q,
    tree: Q,
}

impl<Q> TreeFilter<Q>
where
    Q: TextQuery,
{
    pub fn new(rail: Q, tree: Q) -> Self {
        Self { rail, tree }
    }

    pub fn is_active(&self) -> bool {
        self.rail.is_active() || self.tree.is_active()
    }

    /// Whether a group's box acts only on the rows listed under it. The rail's query narrows
    /// what a box acts on, as it does across the rail; the tree's own field only narrows what is
    /// shown.
    pub fn narrows(&self) -> bool {
        self.rail.is_active()
    }

    /// The filter for the rows under a row reading `text`: a query that matched the row keeps
    /// everything under it.
    pub fn below(&self, text: &str) -> Self {
        let keep = |query: &Q| if query.keeps(text) { Q::default() } else { query.clone() };

        Self { rail: keep(&self.rail), tree: keep(&self.tree) }
    }

    /// The rail's query alone, which is what decides the rows a group's box acts on.
    pub fn rail_only(&self) -> Self {
        Self { rail: self.rail.clone(), tree: Q::default() }
    }

    /// Whether every typed query matches `text`.
    pub fn keeps(&self, text: &str) -> bool {
        self.ranges(text).is_some()
    }

    /// The byte ranges of `text` every typed query matched, merged; `None` unless every typed
    /// query matches it.
    pub fn ranges(&self, text: &str) -> Option<Rc<[Range<usize>]>> {
        Some(joined(self.rail.ranges(text)?, self.tree.ranges(text)?))
    }

    /// `text`'s ranges marked by each typed query that matched it, for a row kept by the rows
    /// under it.
    pub fn marks(&self, text: &str) -> Rc<[Range<usize>]> {
        joined(
            self.rail.ranges(text).unwrap_or_else(|| Rc::from([])),
            self.tree.ranges(text).unwrap_or_else(|| Rc::from([])),
        )
    }
}

/// How many rows of a tree `query` matches by name, over the whole tree: each group's label and
/// the labels of the rows under it.
pub fn match_count<Q>(query: &Q, groups: &[(String, Vec<String>)]) -> usize
where
    Q: TextQuery,
{
    groups
        .iter()
        .flat_map(|(group, rows)| std::iter::once(group).chain(rows))
        .filter(|label| query.keeps(label))
        .count()
}

/// Both queries' marks, joined only when both marked something.
fn joined(rail: Rc<[Range<usize>]>, tree: Rc<[Range<usize>]>) -> Rc<[Range<usize>]> {
    if tree.is_empty() {
        return rail;
    }
    if rail.is_empty() {
        return tree;
    }

    merge(rail.iter().chain(tree.iter()).cloned().collect()).into()
}

/// `ranges` sorted, with those that overlap or touch joined.
fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(rail: &str, tree: &str) -> TreeFilter<FilterQuery> {
        TreeFilter::new(FilterQuery::new(rail), FilterQuery::new(tree))
    }

    #[test]
    fn a_row_is_kept_only_when_every_typed_query_matches_it() {
        assert!(filter("", "").ranges("Bow").is_some());
        assert!(filter("bow", "").ranges("Bow belt").is_some());
        assert!(filter("bow", "belt").ranges("Bow belt").is_some());
        assert!(filter("bow", "deck").ranges("Bow belt").is_none());
        assert!(filter("", "deck").ranges("Bow belt").is_none());
    }

    /// A query a row matched keeps everything under it; one it did not still narrows the rows
    /// below.
    #[test]
    fn below_a_row_only_the_queries_it_did_not_match_still_narrow() {
        let under_bow = filter("bow", "belt").below("Bow");

        assert!(!filter("bow", "bow").below("Bow").is_active());
        assert_eq!(under_bow, filter("", "belt"));
        assert!(under_bow.ranges("Belt").is_some());
        assert!(under_bow.ranges("Deck").is_none());
    }

    #[test]
    fn both_queries_marks_are_drawn_joined() {
        assert_eq!(filter("bow", "belt").ranges("Bow belt").as_deref(), Some([0..3, 4..8].as_slice()));
        let marked: Vec<Range<usize>> = std::iter::once(0..3).collect();
        assert_eq!(
            filter("bow", "deck").marks("Bow belt").to_vec(),
            marked,
            "a row kept by its rows marks what matched"
        );
        assert_eq!(merge(vec![4..6, 0..2, 1..3, 3..4]), vec![0..6]);
    }

    /// Only the rail's query narrows what a box acts on in the tree: with the rail keeping the
    /// whole Hull group, its box acts on every mesh in it whatever the tree's field shows.
    #[test]
    fn only_the_rails_query_narrows_what_a_box_acts_on() {
        assert!(filter("bow", "").narrows());
        assert!(!filter("", "bow").narrows());

        let acted_on = filter("hull", "aft").rail_only().below("Hull");
        assert!(acted_on.keeps("Hull_Fore") && acted_on.keeps("Hull_Aft"));
        let acted_on = filter("fore", "aft").rail_only().below("Hull");
        assert!(acted_on.keeps("Hull_Fore") && !acted_on.keeps("Hull_Aft"));
    }

    #[test]
    fn the_count_takes_groups_and_rows_by_name() {
        let groups = vec![
            ("Bow".to_string(), vec!["Bow belt".to_string(), "Deck".to_string()]),
            ("Stern".to_string(), vec!["Stern deck".to_string()]),
        ];

        assert_eq!(match_count(&FilterQuery::new("bow"), &groups), 2);
        assert_eq!(match_count(&FilterQuery::new("deck"), &groups), 2);
        assert_eq!(match_count(&FilterQuery::new("zzz"), &groups), 0);
    }
}
