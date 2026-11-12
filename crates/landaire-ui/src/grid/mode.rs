use std::rc::Rc;

use zgui::prelude::AnyView;
use zgui::prelude::IntoView;

use crate::surface::RowHeight;

/// Builds the region under an expanded row.
pub type RowDetail<T> = Rc<dyn Fn(&T) -> AnyView>;

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
    Natural { detail: Option<RowDetail<T>> },
    /// Only the rows in view are built, each `height` tall. For row counts in the thousands.
    Fixed {
        height: RowHeight,
        /// How many rows beyond each edge of the view are built, so a fast scroll shows rows
        /// rather than a gap.
        overscan: usize,
    },
}

impl<T> RowMode<T> {
    pub fn natural() -> Self {
        Self::Natural { detail: None }
    }

    /// Every row built, each opening `detail` under itself when expanded.
    pub fn with_detail<V>(detail: impl Fn(&T) -> V + 'static) -> Self
    where
        V: IntoView + 'static,
    {
        Self::Natural { detail: Some(Rc::new(move |row: &T| AnyView::new(detail(row)))) }
    }

    pub fn fixed(height: RowHeight) -> Self {
        Self::Fixed { height, overscan: 4 }
    }
}
