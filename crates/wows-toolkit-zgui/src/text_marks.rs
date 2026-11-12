//! Where a typed filter matched a row's text, as the byte ranges a `MarkedText` draws marked.

use std::ops::Range;
use std::rc::Rc;

use wows_toolkit_viewmodel::text_filter::FilterQuery;

/// Byte ranges of a text a query matched.
pub type Marks = Rc<[Range<usize>]>;

/// Where `query` matched `text`; none for an inactive query or a text it did not match.
pub fn marks(query: &FilterQuery, text: &str) -> Marks {
    query.ranges(text).map(Marks::from).unwrap_or_default()
}

/// Where `query` matched `part`, as ranges of `label`, which holds it. For a row filtered on part
/// of what it says, such as a ship's name.
pub fn marks_within(query: &FilterQuery, label: &str, part: &str) -> Marks {
    let Some(offset) = label.find(part) else { return Marks::default() };

    query.ranges(part).unwrap_or_default().into_iter().map(|range| range.start + offset..range.end + offset).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ship row says its tier before its name; the marks land on the name within it.
    #[test]
    fn marks_within_shift_onto_the_part() {
        let query = FilterQuery::new("ymt");

        assert_eq!(*marks_within(&query, "X Yamato", "Yamato"), [2..3, 4..5, 6..7]);
        assert!(marks_within(&FilterQuery::default(), "X Yamato", "Yamato").is_empty());
        assert!(marks_within(&query, "X Yamato", "Hayate").is_empty(), "a part the label lacks marks nothing");
    }
}
