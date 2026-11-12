//! What narrows a tree in the options rail: the rail's own query and the
//! query typed into the tree's own field. A row is kept when every typed
//! query matches it, or when a row under it is kept.

use std::ops::Range;

use gpui_kit::SharedString;
use gpui_kit::StyledText;

use crate::text_filter::FilterQuery;
use crate::text_filter::MarkedLabel;
use crate::text_filter::highlighted_text;

/// A filter a checkbox row's label is drawn and kept by.
pub(crate) trait RowFilter {
    fn is_active(&self) -> bool;

    /// `text` marked where it matched; `None` when the row is not kept.
    fn row_label(&self, text: String) -> Option<StyledText>;
}

impl RowFilter for FilterQuery {
    fn is_active(&self) -> bool {
        FilterQuery::is_active(self)
    }

    fn row_label(&self, text: String) -> Option<StyledText> {
        MarkedLabel::label(self, text)
    }
}

/// The rail's query and a tree's own, applied together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TreeFilter {
    rail: FilterQuery,
    tree: FilterQuery,
}

impl TreeFilter {
    pub(crate) fn new(rail: FilterQuery, tree: FilterQuery) -> Self {
        Self { rail, tree }
    }

    /// Whether a group's box acts only on the rows listed under it. The
    /// rail's query narrows what a box acts on, as it does across the rail;
    /// the tree's own field only narrows what is shown.
    pub(crate) fn narrows(&self) -> bool {
        self.rail.is_active()
    }

    /// The filter for the rows under a row reading `text`: a query that
    /// matched the row keeps everything under it.
    pub(crate) fn below(&self, text: &str) -> Self {
        let keep = |query: &FilterQuery| if query.keeps(text) { FilterQuery::default() } else { query.clone() };

        Self { rail: keep(&self.rail), tree: keep(&self.tree) }
    }

    /// The rail's query alone, which is what decides the rows a group's box
    /// acts on.
    pub(crate) fn rail_only(&self) -> Self {
        Self { rail: self.rail.clone(), tree: FilterQuery::default() }
    }

    /// Whether every typed query matches `text`.
    pub(crate) fn keeps(&self, text: &str) -> bool {
        self.ranges(text).is_some()
    }

    /// `text` marked by each typed query that matched it, for a row kept by
    /// the rows under it.
    pub(crate) fn marked(&self, text: String) -> StyledText {
        let text = SharedString::from(text);
        let ranges = [&self.rail, &self.tree].into_iter().filter_map(|query| query.ranges(&text)).flatten().collect();

        highlighted_text(text, merge(ranges))
    }

    /// The byte ranges of `text` every typed query matched, merged; `None`
    /// unless every typed query matches it.
    fn ranges(&self, text: &str) -> Option<Vec<Range<usize>>> {
        let mut ranges = self.rail.ranges(text)?;
        ranges.extend(self.tree.ranges(text)?);

        Some(merge(ranges))
    }
}

impl RowFilter for TreeFilter {
    fn is_active(&self) -> bool {
        self.rail.is_active() || self.tree.is_active()
    }

    fn row_label(&self, text: String) -> Option<StyledText> {
        let text = SharedString::from(text);
        let ranges = self.ranges(&text)?;

        Some(highlighted_text(text, ranges))
    }
}

/// How many rows of a tree `query` matches by name, over the whole tree:
/// each group's label and the labels of the rows under it.
pub(crate) fn match_count(query: &FilterQuery, groups: &[(String, Vec<String>)]) -> usize {
    groups
        .iter()
        .flat_map(|(group, rows)| std::iter::once(group).chain(rows))
        .filter(|label| query.keeps(label))
        .count()
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

    fn filter(rail: &str, tree: &str) -> TreeFilter {
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

    /// A query a row matched keeps everything under it; one it did not still
    /// narrows the rows below.
    #[test]
    fn below_a_row_only_the_queries_it_did_not_match_still_narrow() {
        let under_bow = filter("bow", "belt").below("Bow");

        assert!(!RowFilter::is_active(&filter("bow", "bow").below("Bow")));
        assert_eq!(under_bow, filter("", "belt"));
        assert!(under_bow.ranges("Belt").is_some());
        assert!(under_bow.ranges("Deck").is_none());
    }

    #[test]
    fn both_queries_marks_are_drawn_joined() {
        assert_eq!(filter("bow", "belt").ranges("Bow belt"), Some(vec![0..3, 4..8]));
        assert_eq!(merge(vec![4..6, 0..2, 1..3, 3..4]), vec![0..6]);
    }

    /// Only the rail's query narrows what a box acts on in the tree: with the
    /// rail keeping the whole Hull group, its box acts on every mesh in it
    /// whatever the tree's field shows.
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
