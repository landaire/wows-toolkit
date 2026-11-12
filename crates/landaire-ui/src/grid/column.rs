use std::fmt;
use std::rc::Rc;

use serde::Deserialize;
use serde::Serialize;
use zgui::prelude::AnyView;

/// What names a column, in its layout and in a sort.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColumnId(String);

impl ColumnId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ColumnId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

impl fmt::Display for ColumnId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A column's width in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColumnWidth(pub f32);

impl ColumnWidth {
    /// The narrowest a column is by default. Below this its content is unreadable and its header
    /// is gone.
    pub const MIN: Self = Self(28.0);

    pub(crate) fn css(self) -> String {
        format!("{}px", self.0)
    }
}

/// How wide a column is before anyone resizes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnSizing {
    Fixed(ColumnWidth),
    /// As wide as its widest cell or its header, measured as laid out. A column only grows while
    /// the rows stay the same, so scrolling a virtualized grid does not make it jump back and forth.
    Fit,
}

/// Which edge of its column a cell's content sits against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColumnAlign {
    #[default]
    Start,
    Center,
    /// For figures, so their last digits line up.
    End,
}

impl ColumnAlign {
    pub(crate) const fn attribute(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
        }
    }
}

/// Builds a column's header.
pub type HeaderView = Rc<dyn Fn() -> AnyView>;
/// Builds one row's cell in a column.
pub type CellView<T> = Rc<dyn Fn(&T) -> AnyView>;
/// One row's value in a column as plain text, for copying.
pub type CellText<T> = Rc<dyn Fn(&T) -> String>;

/// One column of a [`Grid`](crate::Grid): what it is called, what its cells show and what a
/// reader may do with it.
pub struct GridColumn<T> {
    id: ColumnId,
    label: String,
    header: Option<HeaderView>,
    cell: CellView<T>,
    text: Option<CellText<T>>,

    sizing: ColumnSizing,
    min_width: ColumnWidth,
    align: ColumnAlign,

    resizable: bool,
    reorderable: bool,
    pinnable: bool,
    sortable: bool,
}

impl<T> GridColumn<T> {
    /// A column whose header is `label` and whose cells `cell` builds. It is fitted to its content,
    /// resizable, reorderable and pinnable, and does not sort.
    pub fn new<V>(id: impl Into<ColumnId>, label: impl Into<String>, cell: impl Fn(&T) -> V + 'static) -> Self
    where
        V: zgui::prelude::IntoView + 'static,
    {
        Self {
            id: id.into(),
            label: label.into(),
            header: None,
            cell: Rc::new(move |row: &T| AnyView::new(cell(row))),
            text: None,
            sizing: ColumnSizing::Fit,
            min_width: ColumnWidth::MIN,
            align: ColumnAlign::Start,
            resizable: true,
            reorderable: true,
            pinnable: true,
            sortable: false,
        }
    }

    /// Shows `header` in place of the label. The label still names the column for a screen reader
    /// and in the header's menu.
    #[must_use]
    pub fn header<V>(mut self, header: impl Fn() -> V + 'static) -> Self
    where
        V: zgui::prelude::IntoView + 'static,
    {
        self.header = Some(Rc::new(move || AnyView::new(header())));
        self
    }

    /// What a copied row holds for this column. A column without it is left out of a copy.
    #[must_use]
    pub fn text(mut self, text: impl Fn(&T) -> String + 'static) -> Self {
        self.text = Some(Rc::new(text));
        self
    }

    #[must_use]
    pub fn sizing(mut self, sizing: ColumnSizing) -> Self {
        self.sizing = sizing;
        self
    }

    #[must_use]
    pub fn min_width(mut self, width: ColumnWidth) -> Self {
        self.min_width = width;
        self
    }

    #[must_use]
    pub fn align(mut self, align: ColumnAlign) -> Self {
        self.align = align;
        self
    }

    #[must_use]
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    #[must_use]
    pub fn reorderable(mut self, reorderable: bool) -> Self {
        self.reorderable = reorderable;
        self
    }

    #[must_use]
    pub fn pinnable(mut self, pinnable: bool) -> Self {
        self.pinnable = pinnable;
        self
    }

    /// Makes a press on the header ask for a sort by this column.
    #[must_use]
    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    pub fn id(&self) -> &ColumnId {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn is_resizable(&self) -> bool {
        self.resizable
    }

    pub fn is_reorderable(&self) -> bool {
        self.reorderable
    }

    pub fn is_pinnable(&self) -> bool {
        self.pinnable
    }

    pub fn is_sortable(&self) -> bool {
        self.sortable
    }

    pub(crate) fn header_view(&self) -> AnyView {
        match &self.header {
            Some(header) => header(),
            None => AnyView::new(self.label.clone()),
        }
    }

    pub(crate) fn cell_view(&self, row: &T) -> AnyView {
        (self.cell)(row)
    }

    pub(crate) fn cell_text(&self, row: &T) -> Option<String> {
        self.text.as_ref().map(|text| text(row))
    }

    pub(crate) fn sizing_value(&self) -> ColumnSizing {
        self.sizing
    }

    pub(crate) fn min_width_value(&self) -> ColumnWidth {
        self.min_width
    }

    pub(crate) fn align_value(&self) -> ColumnAlign {
        self.align
    }

    /// `width` held to this column's minimum.
    pub(crate) fn clamp(&self, width: f32) -> ColumnWidth {
        ColumnWidth(width.max(self.min_width.0))
    }
}

impl<T> Clone for GridColumn<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            label: self.label.clone(),
            header: self.header.clone(),
            cell: Rc::clone(&self.cell),
            text: self.text.clone(),
            sizing: self.sizing,
            min_width: self.min_width,
            align: self.align,
            resizable: self.resizable,
            reorderable: self.reorderable,
            pinnable: self.pinnable,
            sortable: self.sortable,
        }
    }
}

impl<T> fmt::Debug for GridColumn<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GridColumn")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("sizing", &self.sizing)
            .field("min_width", &self.min_width)
            .finish_non_exhaustive()
    }
}
