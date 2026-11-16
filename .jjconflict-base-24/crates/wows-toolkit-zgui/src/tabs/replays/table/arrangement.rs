//! The reader's column order, pins and widths, kept under the settings rows the GPUI front end
//! uses, and their translation to and from the grid's [`ColumnLayout`].

use std::cell::RefCell;

use landaire_ui::ColumnId;
use landaire_ui::ColumnLayout;
use landaire_ui::ColumnWidth;
use wows_toolkit_replay_data::columns::ColumnArrangement;
use wows_toolkit_replay_data::columns::Placement;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::columns::Section;
use wows_toolkit_replay_data::columns::StoredArrangement;

/// The settings row the resized widths are kept in, one entry per column in `ReplayColumn::ALL`
/// order.
pub const COLUMN_WIDTHS_KEY: &str = "replay_column_widths_v2";

/// The settings row the column order and the pinned set are kept in, by column name.
pub const COLUMN_ARRANGEMENT_KEY: &str = "replay_column_arrangement";

/// Every column's place and resized width, whether it is shown or not, so a column the filters
/// bring back returns to where it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Columns {
    pub arrangement: ColumnArrangement,
    /// Indexed by `ReplayColumn as usize`. `None` for a column on its fitted width.
    pub widths: [Option<ColumnWidth>; ReplayColumn::ALL.len()],
}

impl Columns {
    pub fn new() -> Self {
        Self { arrangement: ColumnArrangement::new(), widths: [None; ReplayColumn::ALL.len()] }
    }

    /// The rows as the settings database holds them. Nothing stored is the default arrangement,
    /// never changed.
    pub fn from_stored(arrangement: Option<&StoredArrangement>, widths: Option<&[Option<f32>]>) -> Self {
        let mut columns = Self::new();
        if let Some(arrangement) = arrangement {
            columns.arrangement = ColumnArrangement::from_stored(arrangement);
        }
        for (slot, width) in columns.widths.iter_mut().zip(widths.into_iter().flatten()) {
            // A width narrower than the grip would leave a column that cannot be grabbed to widen.
            *slot = width.map(|width| ColumnWidth(width.max(ColumnWidth::MIN.0)));
        }

        columns
    }

    pub fn stored_widths(&self) -> Vec<Option<f32>> {
        self.widths.iter().map(|width| width.map(|width| width.0)).collect()
    }

    /// The grid's layout of the `shown` columns.
    pub fn layout(&self, shown: &[ReplayColumn]) -> ColumnLayout {
        let drawn = self.arrangement.shown(shown);
        let mut layout = ColumnLayout::new(drawn.iter().map(column_id));
        for col in &drawn.pinned {
            layout.pin(&column_id(*col));
        }
        for col in drawn.iter() {
            layout.set_width(&column_id(col), self.widths[col as usize]);
        }

        layout
    }

    /// These columns with the `shown` ones as the grid's `layout` has them, and the hidden ones
    /// where they were.
    pub fn with_layout(&self, layout: &ColumnLayout, shown: &[ReplayColumn]) -> Self {
        let mut next = self.clone();
        let mut pinned = 0;
        let mut scrolling = 0;
        for state in layout.columns() {
            let Some(col) = ReplayColumn::from_storage_key(state.id().as_str()) else { continue };

            let placement = if state.is_pinned() {
                pinned += 1;
                Placement { section: Section::Pinned, index: pinned - 1 }
            } else {
                scrolling += 1;
                Placement { section: Section::Scrolling, index: scrolling - 1 }
            };
            next.arrangement.place(col, placement, shown);
            next.widths[col as usize] = state.width();
        }

        next
    }
}

impl Default for Columns {
    fn default() -> Self {
        Self::new()
    }
}

/// The grid's id for `col`: its storage name, which is stable across versions.
pub fn column_id(col: ReplayColumn) -> ColumnId {
    ColumnId::new(col.storage_key())
}

thread_local! {
    /// The columns last read from or written to the settings database, so a table opened later
    /// starts from them rather than reading the database again.
    static KNOWN: RefCell<Option<Columns>> = const { RefCell::new(None) };
}

pub fn known() -> Option<Columns> {
    KNOWN.with(|known| known.borrow().clone())
}

pub fn remember(columns: &Columns) {
    KNOWN.with(|known| *known.borrow_mut() = Some(columns.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: [ReplayColumn; 4] =
        [ReplayColumn::Name, ReplayColumn::ShipName, ReplayColumn::Kills, ReplayColumn::ActualDamage];

    #[test]
    fn the_default_layout_pins_the_player_and_the_ship() {
        let layout = Columns::new().layout(&SHOWN);

        assert_eq!(layout.pinned_count(), 2);
        assert!(layout.is_pinned(&column_id(ReplayColumn::Name)));
        assert!(layout.is_pinned(&column_id(ReplayColumn::ShipName)));
    }

    #[test]
    fn a_moved_column_keeps_the_hidden_columns_in_their_places() {
        let columns = Columns::new();
        let mut layout = columns.layout(&SHOWN);
        layout.place(
            &column_id(ReplayColumn::ActualDamage),
            landaire_ui::Placement { section: landaire_ui::Section::Scrolling, index: 0 },
        );
        layout.set_width(&column_id(ReplayColumn::Kills), Some(ColumnWidth(90.0)));

        let next = columns.with_layout(&layout, &SHOWN);

        let drawn = next.arrangement.shown(&SHOWN);
        assert_eq!(drawn.scrolling, vec![ReplayColumn::ActualDamage, ReplayColumn::Kills]);
        assert_eq!(next.widths[ReplayColumn::Kills as usize], Some(ColumnWidth(90.0)));
        let everything = next.arrangement.shown(&ReplayColumn::ALL);
        assert_eq!(everything.iter().count(), ReplayColumn::ALL.len(), "every column keeps a place");
    }

    #[test]
    fn a_stored_width_reads_back_onto_its_column_and_no_narrower_than_the_grip() {
        let mut widths = vec![None; ReplayColumn::ALL.len()];
        widths[ReplayColumn::Kills as usize] = Some(4.0);
        widths[ReplayColumn::Hits as usize] = Some(120.0);

        let columns = Columns::from_stored(None, Some(&widths));

        assert_eq!(columns.widths[ReplayColumn::Kills as usize], Some(ColumnWidth::MIN));
        assert_eq!(columns.widths[ReplayColumn::Hits as usize], Some(ColumnWidth(120.0)));
        assert_eq!(columns.stored_widths()[ReplayColumn::Hits as usize], Some(120.0));
    }

    #[test]
    fn an_unpinned_column_reads_back_unpinned() {
        let columns = Columns::new();
        let mut layout = columns.layout(&SHOWN);
        layout.unpin(&column_id(ReplayColumn::ShipName));

        let stored = columns.with_layout(&layout, &SHOWN).arrangement.to_stored();
        let read = Columns::from_stored(Some(&stored), None);

        assert!(!read.arrangement.is_pinned(ReplayColumn::ShipName));
        assert!(read.arrangement.is_pinned(ReplayColumn::Name));
    }
}
