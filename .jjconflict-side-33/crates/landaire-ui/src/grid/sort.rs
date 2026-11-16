use serde::Deserialize;
use serde::Serialize;

use crate::grid::column::ColumnId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub(crate) const fn a11y(self) -> zgui::vocab::SortDirection {
        match self {
            Self::Ascending => zgui::vocab::SortDirection::Ascending,
            Self::Descending => zgui::vocab::SortDirection::Descending,
        }
    }
}

/// The column a grid's rows are sorted by, and which way.
///
/// The application sorts its rows. A grid shows this on the header and asks for the next one
/// when a header is pressed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnSort {
    pub column: ColumnId,
    pub direction: SortDirection,
}

impl ColumnSort {
    /// The sort a press on `column`'s header asks for after `current`: ascending first, then
    /// descending, then unsorted.
    pub fn after_press(current: Option<&Self>, column: &ColumnId) -> Option<Self> {
        let direction = match current {
            Some(sort) if sort.column == *column => match sort.direction {
                SortDirection::Ascending => Some(SortDirection::Descending),
                SortDirection::Descending => None,
            },
            Some(_) | None => Some(SortDirection::Ascending),
        };

        direction.map(|direction| Self { column: column.clone(), direction })
    }
}
