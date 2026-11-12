//! Drawing a typed-text filter's matches. The matching itself is
//! `wows_toolkit_viewmodel::text_filter`.

use std::ops::Range;

use gpui_kit::FontWeight;
use gpui_kit::HighlightStyle;
use gpui_kit::Hsla;
use gpui_kit::SharedString;
use gpui_kit::StyledText;

pub(crate) use wows_toolkit_viewmodel::text_filter::FilterKey;
pub(crate) use wows_toolkit_viewmodel::text_filter::FilterQuery;
pub(crate) use wows_toolkit_viewmodel::text_filter::MatchScore;

/// A row label marked where a [`FilterQuery`] matched it.
pub(crate) trait MarkedLabel {
    /// `text` with its matched characters marked; `None` when it does not
    /// match.
    fn label(&self, text: impl Into<SharedString>) -> Option<StyledText>;

    /// `text` marked where it matched, and unmarked when it did not: for a
    /// row kept by another of its texts.
    fn marked(&self, text: impl Into<SharedString>) -> StyledText;
}

impl MarkedLabel for FilterQuery {
    fn label(&self, text: impl Into<SharedString>) -> Option<StyledText> {
        let text: SharedString = text.into();
        let ranges = self.ranges(&text)?;

        Some(highlighted_text(text, ranges))
    }

    fn marked(&self, text: impl Into<SharedString>) -> StyledText {
        let text: SharedString = text.into();
        // A text that does not match draws unmarked; the row is kept for
        // another reason.
        let ranges = self.ranges(&text).unwrap_or_default();

        highlighted_text(text, ranges)
    }
}

/// `text` with `ranges`, byte ranges on character boundaries, marked as a
/// filter marks its matches.
pub(crate) fn highlighted_text(text: impl Into<SharedString>, ranges: Vec<Range<usize>>) -> StyledText {
    let text: SharedString = text.into();
    #[cfg(test)]
    record_marks(&text, &ranges);

    let style = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        background_color: Some(match_mark()),
        ..Default::default()
    };

    StyledText::new(text).with_highlights(ranges.into_iter().map(|range| (range, style)))
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
