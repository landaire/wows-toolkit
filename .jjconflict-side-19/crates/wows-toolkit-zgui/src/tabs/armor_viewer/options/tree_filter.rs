//! What narrows a tree in the options rail: the rail's own query and the query typed into the
//! tree's own field. A row is kept when every typed query matches it, or when a row under it is
//! kept. Also the checkbox row every section draws, kept or dropped by such a filter.

use std::ops::Range;
use std::rc::Rc;

use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::prelude::Binding;

use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;

const SHEET: &str = "wt-armor-rows";

const CSS: &str = css!(
    ".wt-armor-check {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .wt-armor-check__label { font-size: 13px; cursor: pointer; min-width: 0; }
    .wt-armor-check__label[data-disabled] { cursor: default; color: var(--wt-color-text-dim); }"
);

/// A filter a checkbox row's label is drawn and kept by.
pub trait RowFilter {
    fn is_active(&self) -> bool;

    /// `text` marked where it matched; `None` when the row is not kept.
    fn row_label(&self, text: String) -> Option<MarkedLabel>;
}

impl RowFilter for RailFilter {
    fn is_active(&self) -> bool {
        RailFilter::is_active(self)
    }

    fn row_label(&self, text: String) -> Option<MarkedLabel> {
        self.label(text)
    }
}

/// The rail's query and a tree's own, applied together.
#[derive(Clone, Default)]
pub struct TreeFilter {
    rail: RailFilter,
    tree: RailFilter,
}

impl TreeFilter {
    pub fn new(rail: RailFilter, tree: RailFilter) -> Self {
        Self { rail, tree }
    }

    /// Whether a group's box acts only on the rows listed under it. The rail's query narrows what
    /// a box acts on, as it does across the rail; the tree's own field only narrows what is shown.
    pub fn narrows(&self) -> bool {
        self.rail.is_active()
    }

    /// The filter for the rows under a row reading `text`: a query that matched the row keeps
    /// everything under it.
    pub fn below(&self, text: &str) -> Self {
        let keep = |query: &RailFilter| if query.keeps(text) { RailFilter::default() } else { query.clone() };

        Self { rail: keep(&self.rail), tree: keep(&self.tree) }
    }

    /// The rail's query alone, which is what decides the rows a group's box acts on.
    pub fn rail_only(&self) -> Self {
        Self { rail: self.rail.clone(), tree: RailFilter::default() }
    }

    /// Whether every typed query matches `text`.
    pub fn keeps(&self, text: &str) -> bool {
        self.ranges(text).is_some()
    }

    /// `text` marked by each typed query that matched it, for a row kept by the rows under it.
    pub fn marked(&self, text: String) -> MarkedLabel {
        let rail = self.rail.query().ranges(&text).unwrap_or_default();
        let tree = self.tree.query().ranges(&text).unwrap_or_default();
        let marks = joined(rail, tree).into();

        MarkedLabel { text, marks }
    }

    /// The byte ranges of `text` every typed query matched, merged; `None` unless every typed
    /// query matches it.
    fn ranges(&self, text: &str) -> Option<Vec<Range<usize>>> {
        Some(joined(self.rail.query().ranges(text)?, self.tree.query().ranges(text)?))
    }
}

impl RowFilter for TreeFilter {
    fn is_active(&self) -> bool {
        self.rail.is_active() || self.tree.is_active()
    }

    fn row_label(&self, text: String) -> Option<MarkedLabel> {
        let marks = self.ranges(&text)?.into();

        Some(MarkedLabel { text, marks })
    }
}

/// How many rows of a tree `query` matches by name, over the whole tree: each group's label and
/// the labels of the rows under it.
pub fn match_count(query: &RailFilter, groups: &[(String, Vec<String>)]) -> usize {
    groups
        .iter()
        .flat_map(|(group, rows)| std::iter::once(group).chain(rows))
        .filter(|label| query.keeps(label))
        .count()
}

/// A checkbox kept only when `filter` keeps its label, which is drawn beside it marked where the
/// filter matched. A click on the label toggles the box as the box's own label would.
pub fn filtered_checkbox(
    test_id: String,
    label: String,
    checked: Signal<bool, LocalStorage>,
    disabled: impl Into<Signal<bool, LocalStorage>>,
    filter: &impl RowFilter,
    on_change: impl Fn(bool) + 'static,
) -> Option<AnyView> {
    let marked = filter.row_label(label.clone())?;
    install_stylesheet(SHEET, CSS);
    let disabled: Signal<bool, LocalStorage> = disabled.into();
    let on_change = Rc::new(on_change);
    let from_box = Rc::clone(&on_change);
    let state = Signal::derive_local(move || if checked.get() { Checked::Yes } else { Checked::No });
    let label_id = format!("{test_id}-label");

    Some(AnyView::new(view! {
        row(class = "wt-armor-check") {
            Checkbox(
                checked = Binding::controlled(state, move |next: Checked| from_box(next.is_checked())),
                disabled = disabled,
                attr:data-testid = test_id,
                a11y:label = label
            )
            text(
                class = "wt-armor-check__label",
                attr:data-testid = label_id,
                attr:data-disabled = move || disabled.get().then(String::new),
                on:click = move |_| {
                    if !disabled.get_untracked() {
                        on_change(!checked.get_untracked());
                    }
                }
            ) {{marked.view()}}
        }
    }))
}

/// Both queries' marks, joined only when both marked something.
fn joined(rail: Vec<Range<usize>>, tree: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if tree.is_empty() {
        return rail;
    }
    if rail.is_empty() {
        return tree;
    }

    merge(rail.into_iter().chain(tree).collect())
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
        TreeFilter::new(RailFilter::new(rail), RailFilter::new(tree))
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

        assert!(!RowFilter::is_active(&filter("bow", "bow").below("Bow")));
        assert!(!under_bow.rail.is_active());
        assert!(under_bow.ranges("Belt").is_some());
        assert!(under_bow.ranges("Deck").is_none());
    }

    #[test]
    fn both_queries_marks_are_drawn_joined() {
        assert_eq!(filter("bow", "belt").ranges("Bow belt"), Some(vec![0..3, 4..8]));
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

        assert_eq!(match_count(&RailFilter::new("bow"), &groups), 2);
        assert_eq!(match_count(&RailFilter::new("deck"), &groups), 2);
        assert_eq!(match_count(&RailFilter::new("zzz"), &groups), 0);
    }
}
