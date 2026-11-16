use std::rc::Rc;

use zgui::prelude::AnyView;
use zgui::prelude::IntoView;

use crate::grid::column::ColumnId;
use crate::surface::RowHeight;

/// A column's part of an expanded row.
type ColumnDetail<T> = Rc<dyn Fn(&T, &ColumnId) -> Option<AnyView>>;

/// What an expanded row shows under its line.
pub enum RowDetail<T> {
    /// One region under the row, as wide as the view.
    Spanning(Rc<dyn Fn(&T) -> AnyView>),
    /// A part under each column's own cell, laid out in that column's track. A column widens to its
    /// widest part while any shows, closing ones included, and returns to its width after. A column
    /// resized while a part shows holds the width set until none does, and a pinned column widens no
    /// further than the pinned share of the view. A part wider than its column is cut off with a
    /// fade. `None` leaves the column's part empty.
    Columns(ColumnDetail<T>),
}

impl<T> Clone for RowDetail<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Spanning(detail) => Self::Spanning(Rc::clone(detail)),
            Self::Columns(detail) => Self::Columns(Rc::clone(detail)),
        }
    }
}

/// The name a row's mark goes by, for the application's styles to colour. A row with none has no
/// mark.
pub type RowMark<T> = Rc<dyn Fn(&T) -> Option<&'static str>>;

/// Whether a row opens a run, given the row before it.
type RunOpens<T> = Rc<dyn Fn(Option<&T>, &T) -> bool>;

/// How rows fall into runs that each open under a heading spanning the grid.
///
/// A run is a stretch of adjacent rows in the same group, so the application orders its rows to
/// keep each group together. A row in no group opens no run, which lets an application drop the
/// headings for an order that scatters its groups.
pub struct RowGroups<T> {
    opens: RunOpens<T>,
    heading: Rc<dyn Fn(&T) -> AnyView>,
}

impl<T> Clone for RowGroups<T> {
    fn clone(&self) -> Self {
        Self { opens: Rc::clone(&self.opens), heading: Rc::clone(&self.heading) }
    }
}

impl<T: 'static> RowGroups<T> {
    /// Rows grouped by `group`, each run headed by `heading` built from its first row.
    pub fn new<G, V>(group: impl Fn(&T) -> Option<G> + 'static, heading: impl Fn(&T) -> V + 'static) -> Self
    where
        G: PartialEq,
        V: IntoView + 'static,
    {
        Self {
            opens: Rc::new(move |previous: Option<&T>, row: &T| match group(row) {
                Some(own) => previous.is_none_or(|previous| group(previous).as_ref() != Some(&own)),
                None => false,
            }),
            heading: Rc::new(move |row: &T| AnyView::new(heading(row))),
        }
    }

    /// Whether `row` opens a run, given the row before it.
    pub(crate) fn starts(&self, previous: Option<&T>, row: &T) -> bool {
        (self.opens)(previous, row)
    }

    pub(crate) fn heading(&self, row: &T) -> AnyView {
        (self.heading)(row)
    }
}

/// How a grid builds and sizes its rows.
#[non_exhaustive]
pub enum RowMode<T> {
    /// Every row is built, each as tall as its content, and a row may open a detail region under
    /// itself. For row counts in the hundreds.
    Natural { detail: Option<RowDetail<T>>, groups: Option<RowGroups<T>> },
    /// Only the rows in view are built, each `height` tall. For row counts in the thousands.
    Fixed {
        height: RowHeight,
        /// How many rows beyond each edge of the view are built, so a fast scroll shows rows
        /// rather than a gap.
        overscan: usize,
    },
    /// Only the rows in view are built, each as tall as its content and its open detail region.
    /// A row stands at `estimate` until it has been built and measured. For row counts in the
    /// thousands whose rows differ in height.
    Measured {
        estimate: RowHeight,
        /// How many rows beyond each edge of the view are built.
        overscan: usize,
        detail: Option<RowDetail<T>>,
    },
}

impl<T: 'static> RowMode<T> {
    pub fn natural() -> Self {
        Self::Natural { detail: None, groups: None }
    }

    /// Every row built, each opening `detail` under itself when expanded.
    pub fn with_detail<V>(detail: impl Fn(&T) -> V + 'static) -> Self
    where
        V: IntoView + 'static,
    {
        Self::Natural {
            detail: Some(RowDetail::Spanning(Rc::new(move |row: &T| AnyView::new(detail(row))))),
            groups: None,
        }
    }

    /// Every row built, each opening under each column the part `detail` builds for it.
    pub fn with_column_detail<V>(detail: impl Fn(&T, &ColumnId) -> Option<V> + 'static) -> Self
    where
        V: IntoView + 'static,
    {
        let detail = move |row: &T, column: &ColumnId| detail(row, column).map(AnyView::new);
        Self::Natural { detail: Some(RowDetail::Columns(Rc::new(detail))), groups: None }
    }

    pub fn fixed(height: RowHeight) -> Self {
        Self::Fixed { height, overscan: 4 }
    }

    /// Rows built in view and measured, each standing at `estimate` until it is.
    pub fn measured(estimate: RowHeight) -> Self {
        Self::Measured { estimate, overscan: 4, detail: None }
    }

    /// The same mode with its rows run under `groups`' headings. Only a grid that builds every
    /// row draws headings.
    pub fn grouped(self, groups: RowGroups<T>) -> Self {
        match self {
            Self::Natural { detail, .. } => Self::Natural { detail, groups: Some(groups) },
            virtualized @ (Self::Fixed { .. } | Self::Measured { .. }) => virtualized,
        }
    }

    pub(crate) fn into_parts(self) -> RowParts<T> {
        match self {
            Self::Natural { detail, groups } => RowParts { detail, groups, building: Building::All },
            Self::Fixed { height, overscan } => {
                RowParts { detail: None, groups: None, building: Building::Fixed { height, overscan } }
            }
            Self::Measured { estimate, overscan, detail } => {
                RowParts { detail, groups: None, building: Building::Measured { estimate, overscan } }
            }
        }
    }
}

/// A [`RowMode`] taken apart for building the grid.
pub(crate) struct RowParts<T> {
    pub(crate) detail: Option<RowDetail<T>>,
    pub(crate) groups: Option<RowGroups<T>>,
    pub(crate) building: Building,
}

/// Which rows a grid builds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Building {
    All,
    Fixed { height: RowHeight, overscan: usize },
    Measured { estimate: RowHeight, overscan: usize },
}

#[cfg(test)]
mod tests {
    use super::RowGroups;

    fn openers(groups: &RowGroups<Option<u32>>, rows: &[Option<u32>]) -> Vec<usize> {
        (0..rows.len())
            .filter(|&index| groups.starts(index.checked_sub(1).map(|previous| &rows[previous]), &rows[index]))
            .collect()
    }

    #[test]
    fn each_run_opens_once_and_a_row_in_no_group_opens_none() {
        let groups = RowGroups::new(|row: &Option<u32>| *row, |_: &Option<u32>| ());

        assert_eq!(openers(&groups, &[Some(1), Some(1), Some(2), Some(1)]), vec![0, 2, 3]);
        assert_eq!(openers(&groups, &[None, None, Some(3)]), vec![2]);
    }
}
