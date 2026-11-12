//! Drawing a typed-text filter's matches. The matching itself is
//! `wows_toolkit_viewmodel::text_filter`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::FontWeight;
use gpui_kit::HighlightStyle;
use gpui_kit::Hsla;
use gpui_kit::SharedString;
use gpui_kit::StyledText;

pub(crate) use wows_toolkit_viewmodel::text_filter::FilterKey;
pub(crate) use wows_toolkit_viewmodel::text_filter::FilterQuery;
pub(crate) use wows_toolkit_viewmodel::text_filter::MatchScore;

/// Each text a query was asked about, with the byte ranges matched in it, or
/// `None` where it did not match.
type MatchCache = HashMap<String, Option<Rc<[Range<usize>]>>>;

/// How many texts a query remembers. A held query can outlive the rows it
/// was drawn over, so past this the remembered texts are forgotten and the
/// rows still drawn are matched again.
const MATCH_CACHE_LIMIT: usize = 8192;

/// A query with the matches it has found in each text it was asked about,
/// so that a row drawn again for the same query is not matched again. Built
/// when the query changes and kept with what it filtered; clones share the
/// matches.
#[derive(Clone, Default)]
pub(crate) struct MarkedQuery {
    query: FilterQuery,
    /// `None` for an inactive query, which matches every text without
    /// marking any.
    matches: Option<Rc<RefCell<MatchCache>>>,
}

/// The query typed into a field, kept while the text is unchanged so that a
/// view building its query as it draws keeps its matches between frames.
#[derive(Default)]
pub(crate) struct TypedQuery {
    held: RefCell<Option<(String, MarkedQuery)>>,
}

/// A row label marked where a [`MarkedQuery`] matched it.
pub(crate) trait MarkedLabel {
    /// `text` with its matched characters marked; `None` when it does not
    /// match.
    fn label(&self, text: impl Into<SharedString>) -> Option<StyledText>;

    /// `text` marked where it matched, and unmarked when it did not: for a
    /// row kept by another of its texts.
    fn marked(&self, text: impl Into<SharedString>) -> StyledText;
}

impl MarkedQuery {
    pub(crate) fn new(typed: &str) -> Self {
        Self::from(FilterQuery::new(typed))
    }

    /// The byte ranges of `text` the query matched, as
    /// [`FilterQuery::ranges`] finds them, matched once per text.
    pub(crate) fn ranges(&self, text: &str) -> Option<Rc<[Range<usize>]>> {
        let Some(matches) = &self.matches else { return Some(no_ranges()) };

        if let Some(found) = matches.borrow().get(text) {
            return found.clone();
        }

        let found: Option<Rc<[Range<usize>]>> = self.query.ranges(text).map(Rc::from);
        let mut matches = matches.borrow_mut();
        if matches.len() >= MATCH_CACHE_LIMIT {
            matches.clear();
        }
        matches.insert(text.to_owned(), found.clone());

        found
    }

    /// Whether `text` matches.
    pub(crate) fn keeps(&self, text: &str) -> bool {
        self.ranges(text).is_some()
    }

    pub(crate) fn query(&self) -> &FilterQuery {
        &self.query
    }
}

impl From<FilterQuery> for MarkedQuery {
    fn from(query: FilterQuery) -> Self {
        let matches = query.is_active().then(Rc::default);

        Self { query, matches }
    }
}

impl Deref for MarkedQuery {
    type Target = FilterQuery;

    fn deref(&self) -> &FilterQuery {
        &self.query
    }
}

impl PartialEq for MarkedQuery {
    fn eq(&self, other: &Self) -> bool {
        self.query == other.query
    }
}

impl std::fmt::Debug for MarkedQuery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.query.fmt(formatter)
    }
}

impl MarkedLabel for MarkedQuery {
    fn label(&self, text: impl Into<SharedString>) -> Option<StyledText> {
        let text: SharedString = text.into();
        let ranges = self.ranges(&text)?;

        Some(highlighted_text(text, &ranges))
    }

    fn marked(&self, text: impl Into<SharedString>) -> StyledText {
        let text: SharedString = text.into();
        // A text that does not match draws unmarked; the row is kept for
        // another reason.
        let ranges = self.ranges(&text).unwrap_or_else(no_ranges);

        highlighted_text(text, &ranges)
    }
}

impl TypedQuery {
    /// The query for `typed`: the one held when the text is unchanged, and
    /// otherwise a new one held in its place.
    pub(crate) fn of(&self, typed: &str) -> MarkedQuery {
        let mut held = self.held.borrow_mut();
        if let Some((text, query)) = held.as_ref()
            && text == typed
        {
            return query.clone();
        }

        let query = MarkedQuery::new(typed);
        *held = Some((typed.to_owned(), query.clone()));

        query
    }
}

/// No marks, shared rather than allocated for each row that has none.
pub(crate) fn no_ranges() -> Rc<[Range<usize>]> {
    thread_local! {
        static NONE: Rc<[Range<usize>]> = Rc::from([]);
    }

    NONE.with(Rc::clone)
}

/// `text` with `ranges`, byte ranges on character boundaries, marked as a
/// filter marks its matches.
pub(crate) fn highlighted_text(text: impl Into<SharedString>, ranges: &[Range<usize>]) -> StyledText {
    let text: SharedString = text.into();
    #[cfg(test)]
    record_marks(&text, ranges);

    let style = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        background_color: Some(match_mark()),
        ..Default::default()
    };

    StyledText::new(text).with_highlights(ranges.iter().map(|range| (range.clone(), style)))
}

/// What a filter match is marked with.
pub(crate) fn match_mark() -> Hsla {
    Hsla::from(gpui_kit::rgb(crate::theme::semantic().notice)).opacity(0.3)
}

#[cfg(test)]
thread_local! {
    /// Every marked label built since the last [`take_marks`], in order.
    static MARKS: std::cell::RefCell<Vec<(String, Vec<Range<usize>>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn record_marks(text: &str, ranges: &[Range<usize>]) {
    MARKS.with(|marks| marks.borrow_mut().push((text.to_string(), ranges.to_vec())));
}

/// The marked labels rows built since the last call: what a test checks a
/// filtered row drew, as text and marked byte ranges.
#[cfg(test)]
pub(crate) fn take_marks() -> Vec<(String, Vec<Range<usize>>)> {
    MARKS.with(|marks| std::mem::take(&mut *marks.borrow_mut()))
}

/// The ranges the last marked label built for `text` drew, since the last
/// [`take_marks`].
#[cfg(test)]
pub(crate) fn marks_for(text: &str) -> Option<Vec<Range<usize>>> {
    MARKS.with(|marks| marks.borrow().iter().rev().find(|(drawn, _)| drawn == text).map(|(_, ranges)| ranges.clone()))
}

#[cfg(test)]
mod tests {
    use super::MATCH_CACHE_LIMIT;
    use super::MarkedQuery;

    /// A held query drawn over ever new rows forgets what it remembered
    /// rather than growing, and still matches what it is asked about.
    #[test]
    fn a_query_remembers_a_bounded_number_of_texts() {
        let query = MarkedQuery::new("a");
        for row in 0..MATCH_CACHE_LIMIT + 10 {
            assert!(query.keeps(&format!("a{row}")));
        }
        let remembered = query.matches.as_ref().expect("an active query remembers").borrow().len();

        assert!(remembered <= MATCH_CACHE_LIMIT, "{remembered}");
        assert!(!query.keeps("b"));
    }
}
