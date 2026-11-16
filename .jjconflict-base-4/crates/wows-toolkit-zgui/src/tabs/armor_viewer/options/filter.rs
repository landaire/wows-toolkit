//! The options rail's filter as the rows read it: whether a row is kept, and where its label is
//! marked.

use std::rc::Rc;

use landaire_ui::MarkedTextProps;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use zgui::view;
use zgui::view::AnyView;

use crate::text_marks::Marks;
use crate::text_marks::marks;

/// What the rail's filter field asks for. Inactive keeps every row unmarked.
#[derive(Clone, Default)]
pub struct RailFilter(Rc<FilterQuery>);

/// A label as a filter marked it.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkedLabel {
    pub text: String,
    pub marks: Marks,
}

impl MarkedLabel {
    /// `text` with nothing marked.
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), marks: Marks::default() }
    }

    pub fn view(&self) -> AnyView {
        let (text, marks) = (self.text.clone(), Rc::clone(&self.marks));
        AnyView::new(view! { MarkedText(text = text, ranges = marks) })
    }
}

impl RailFilter {
    pub fn new(typed: &str) -> Self {
        Self(Rc::new(FilterQuery::new(typed)))
    }

    pub fn is_active(&self) -> bool {
        self.0.is_active()
    }

    pub fn keeps(&self, text: &str) -> bool {
        self.0.keeps(text)
    }

    /// `text` marked where the filter matched it, or `None` when it filters `text` out.
    pub fn label(&self, text: impl Into<String>) -> Option<MarkedLabel> {
        let text = text.into();
        if !self.keeps(&text) {
            return None;
        }
        let marks = marks(&self.0, &text);
        Some(MarkedLabel { text, marks })
    }

    /// `text` marked where the filter matched it, kept whatever the filter says.
    pub fn marked(&self, text: impl Into<String>) -> MarkedLabel {
        let text = text.into();
        let marks = marks(&self.0, &text);
        MarkedLabel { text, marks }
    }

    /// What the rows under a heading are filtered by: nothing once the heading itself matched, so
    /// a match on it keeps everything under it.
    pub fn within(&self, heading: &str) -> Self {
        if self.is_active() && self.keeps(heading) { Self::default() } else { self.clone() }
    }

    /// Whether the filter keeps any of the texts the translation `keys` name.
    pub fn keeps_any_key(&self, keys: &[&str]) -> bool {
        !self.is_active() || keys.iter().any(|key| self.keeps(&rust_i18n::t!(*key)))
    }

    pub fn query(&self) -> &FilterQuery {
        &self.0
    }
}
