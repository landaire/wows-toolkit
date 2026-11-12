//! The options rail's tree filter ([`wows_toolkit_armor::tree_filter`]) over the rail's marked
//! queries, drawing the labels it keeps.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::SharedString;
use gpui_kit::StyledText;

pub(crate) use wows_toolkit_armor::tree_filter::match_count;
use wows_toolkit_armor::tree_filter::TextQuery;

use crate::text_filter::MarkedLabel;
use crate::text_filter::MarkedQuery;
use crate::text_filter::highlighted_text;

/// The rail's query and a tree's own, applied together.
pub(crate) type TreeFilter = wows_toolkit_armor::tree_filter::TreeFilter<MarkedQuery>;

impl TextQuery for MarkedQuery {
    fn is_active(&self) -> bool {
        self.query().is_active()
    }

    fn ranges(&self, text: &str) -> Option<Rc<[Range<usize>]>> {
        MarkedQuery::ranges(self, text)
    }
}

/// A filter a checkbox row's label is drawn and kept by.
pub(crate) trait RowFilter {
    fn is_active(&self) -> bool;

    /// `text` marked where it matched; `None` when the row is not kept.
    fn row_label(&self, text: String) -> Option<StyledText>;
}

impl RowFilter for MarkedQuery {
    fn is_active(&self) -> bool {
        self.query().is_active()
    }

    fn row_label(&self, text: String) -> Option<StyledText> {
        MarkedLabel::label(self, text)
    }
}

impl RowFilter for TreeFilter {
    fn is_active(&self) -> bool {
        TreeFilter::is_active(self)
    }

    fn row_label(&self, text: String) -> Option<StyledText> {
        let text = SharedString::from(text);
        let ranges = self.ranges(&text)?;

        Some(highlighted_text(text, &ranges))
    }
}

/// A tree filter's label for a row kept by the rows under it.
pub(crate) trait MarkedTreeLabel {
    /// `text` marked by each typed query that matched it.
    fn marked(&self, text: String) -> StyledText;
}

impl MarkedTreeLabel for TreeFilter {
    fn marked(&self, text: String) -> StyledText {
        let text = SharedString::from(text);
        let ranges = self.marks(&text);

        highlighted_text(text, &ranges)
    }
}
