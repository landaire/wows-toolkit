use std::rc::Rc;

use zgui::prelude::AnyView;
use zgui::prelude::IntoView;

use crate::surface::RowHeight;

/// Builds the region under an expanded row.
pub type RowDetail<T> = Rc<dyn Fn(&T) -> AnyView>;

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
///
/// A variable-height virtualized mode belongs here as a further variant carrying an estimated
/// height and a [`RowDetail`]: its window comes from the virtualizer instead of from
/// [`RowMode::Fixed`]'s arithmetic, and the rows, their detail regions and their reveal stay as
/// they are.
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
        Self::Natural { detail: Some(Rc::new(move |row: &T| AnyView::new(detail(row)))), groups: None }
    }

    pub fn fixed(height: RowHeight) -> Self {
        Self::Fixed { height, overscan: 4 }
    }

    /// The same mode with its rows run under `groups`' headings. A fixed-height grid places every
    /// row by arithmetic on one height and so draws no headings.
    pub fn grouped(self, groups: RowGroups<T>) -> Self {
        match self {
            Self::Natural { detail, .. } => Self::Natural { detail, groups: Some(groups) },
            fixed @ Self::Fixed { .. } => fixed,
        }
    }
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
