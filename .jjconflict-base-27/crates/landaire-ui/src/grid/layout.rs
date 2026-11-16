use serde::Deserialize;
use serde::Serialize;

use crate::grid::column::ColumnId;
use crate::grid::column::ColumnWidth;

/// How wide the pinned zone is while a column is carried and nothing is pinned, so there is
/// somewhere to drop a column to pin it.
pub(crate) const EMPTY_PIN_ZONE: f32 = 24.0;

/// The order, pins and widths a reader gave a grid's columns, for an application to keep between
/// sessions.
///
/// Pinned columns always come first. A width is present only for a column a reader resized.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredLayout")]
pub struct ColumnLayout {
    columns: Vec<ColumnState>,
}

/// One column's place in a [`ColumnLayout`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnState {
    id: ColumnId,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<ColumnWidth>,
}

impl ColumnState {
    pub fn id(&self) -> &ColumnId {
        &self.id
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// The width a reader resized the column to, if they did.
    pub fn width(&self) -> Option<ColumnWidth> {
        self.width
    }
}

/// The two runs of columns: the pinned ones held at the leading edge, and the ones that scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Pinned,
    Scrolling,
}

/// Where a moved column lands: its index among the other columns of `section`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Placement {
    pub section: Section,
    pub index: usize,
}

impl ColumnLayout {
    /// Every column in `ids` order, none pinned or resized.
    pub fn new(ids: impl IntoIterator<Item = ColumnId>) -> Self {
        let columns = ids.into_iter().map(|id| ColumnState { id, pinned: false, width: None }).collect();

        Self { columns }
    }

    pub fn columns(&self) -> &[ColumnState] {
        &self.columns
    }

    /// Every column id in the order the columns are shown.
    pub fn ids(&self) -> impl Iterator<Item = &ColumnId> {
        self.columns.iter().map(|column| &column.id)
    }

    pub fn pinned_count(&self) -> usize {
        self.columns.iter().take_while(|column| column.pinned).count()
    }

    pub fn is_pinned(&self, id: &ColumnId) -> bool {
        self.state(id).is_some_and(|column| column.pinned)
    }

    pub fn width(&self, id: &ColumnId) -> Option<ColumnWidth> {
        self.state(id).and_then(|column| column.width)
    }

    /// Sets the width `id` was resized to, or with `None` gives it back its own sizing.
    pub fn set_width(&mut self, id: &ColumnId, width: Option<ColumnWidth>) {
        if let Some(column) = self.columns.iter_mut().find(|column| column.id == *id) {
            column.width = width;
        }
    }

    /// Pins `id` after the pinned columns.
    pub fn pin(&mut self, id: &ColumnId) {
        let index = self.pinned_count() - usize::from(self.is_pinned(id));
        self.place(id, Placement { section: Section::Pinned, index });
    }

    /// Unpins `id` to the start of the scrolling columns, beside the pinned ones.
    pub fn unpin(&mut self, id: &ColumnId) {
        self.place(id, Placement { section: Section::Scrolling, index: 0 });
    }

    /// Moves `id` to `placement`, pinning or unpinning it as the section says.
    pub fn place(&mut self, id: &ColumnId, placement: Placement) {
        let Some(from) = self.position(id) else {
            return;
        };

        let mut column = self.columns.remove(from);
        column.pinned = placement.section == Section::Pinned;

        let pinned = self.pinned_count();
        let at = match placement.section {
            Section::Pinned => placement.index.min(pinned),
            Section::Scrolling => (pinned + placement.index).min(self.columns.len()),
        };
        self.columns.insert(at, column);
    }

    /// This layout over the columns a grid has now: a column it names that no longer exists is
    /// dropped, and a column it does not name goes after the column it follows in `known`.
    ///
    /// A stored layout may come from a version of the application with another set of columns,
    /// and the reader's order for the columns both share is still theirs.
    pub fn reconciled(&self, known: &[ColumnId]) -> Self {
        let mut columns: Vec<ColumnState> =
            self.columns.iter().filter(|column| known.contains(&column.id)).cloned().collect();

        for (index, id) in known.iter().enumerate() {
            if columns.iter().any(|column| column.id == *id) {
                continue;
            }

            let after = known[..index]
                .iter()
                .rev()
                .find_map(|previous| columns.iter().position(|column| column.id == *previous));
            let pinned = columns.iter().take_while(|column| column.pinned).count();
            let at = after.map_or(pinned, |after| (after + 1).max(pinned));
            columns.insert(at, ColumnState { id: id.clone(), pinned: false, width: None });
        }

        Self { columns }
    }

    pub(crate) fn position(&self, id: &ColumnId) -> Option<usize> {
        self.columns.iter().position(|column| column.id == *id)
    }

    fn state(&self, id: &ColumnId) -> Option<&ColumnState> {
        self.columns.iter().find(|column| column.id == *id)
    }
}

/// What a [`ColumnLayout`] is read from: any order, which is put right on the way in.
#[derive(Deserialize)]
struct StoredLayout {
    columns: Vec<ColumnState>,
}

impl From<StoredLayout> for ColumnLayout {
    fn from(stored: StoredLayout) -> Self {
        let mut columns: Vec<ColumnState> = Vec::with_capacity(stored.columns.len());
        for column in stored.columns {
            if !columns.iter().any(|kept| kept.id == column.id) {
                columns.push(column);
            }
        }
        // Stable, so each section keeps its stored order.
        columns.sort_by_key(|column| !column.pinned);

        Self { columns }
    }
}

/// Where a carried column would land with the pointer `pointer` CSS pixels from the grid's leading
/// edge: before the first column of the section under the pointer whose middle lies beyond it.
///
/// `scroll` is how far the scrolling columns are scrolled, `width` gives each column's width as
/// drawn, and `may_pin` says whether the carried column may be pinned.
pub(crate) fn placement_at(
    layout: &ColumnLayout,
    carried: &ColumnId,
    pointer: f32,
    scroll: f32,
    width: impl Fn(&ColumnId) -> f32,
    may_pin: bool,
) -> Placement {
    // Measured against the zone as drawn, so a column taken into it stays pinned until it is
    // taken back out past the zone's edge.
    let zone = pin_zone(layout, &width);
    let section = if may_pin && pointer < zone { Section::Pinned } else { Section::Scrolling };

    let others: Vec<&ColumnState> = layout.columns.iter().filter(|column| column.id != *carried).collect();
    let pinned_edge: f32 = others.iter().filter(|column| column.pinned).map(|column| width(&column.id)).sum();

    let (mut start, shift) = match section {
        Section::Pinned => (0.0, 0.0),
        Section::Scrolling => (pinned_edge, scroll),
    };
    let mut index = 0;
    for column in others.iter().filter(|column| column.pinned == (section == Section::Pinned)) {
        let column_width = width(&column.id);
        if start - shift + column_width / 2.0 >= pointer {
            break;
        }
        start += column_width;
        index += 1;
    }

    Placement { section, index }
}

/// How far the pinned zone reaches from the leading edge.
pub(crate) fn pin_zone(layout: &ColumnLayout, width: impl Fn(&ColumnId) -> f32) -> f32 {
    let pinned: f32 = layout.columns.iter().filter(|column| column.pinned).map(|column| width(&column.id)).sum();
    if pinned > 0.0 { pinned } else { EMPTY_PIN_ZONE }
}

/// Where each column is drawn from the grid's leading edge with the scrolling ones scrolled by
/// `scroll`: the pinned ones hold still.
pub(crate) fn drawn_starts(
    layout: &ColumnLayout,
    scroll: f32,
    width: impl Fn(&ColumnId) -> f32,
) -> Vec<(ColumnId, f32)> {
    let mut start = 0.0;
    let mut starts = Vec::with_capacity(layout.columns.len());
    for column in &layout.columns {
        let shift = if column.pinned { 0.0 } else { scroll };
        starts.push((column.id.clone(), start - shift));
        start += width(&column.id);
    }

    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(layout: &ColumnLayout) -> Vec<&str> {
        layout.ids().map(ColumnId::as_str).collect()
    }

    fn layout(names: &[&str]) -> ColumnLayout {
        ColumnLayout::new(names.iter().map(|name| ColumnId::from(*name)))
    }

    #[test]
    fn a_pinned_column_moves_ahead_of_the_scrolling_ones_and_back_beside_them() {
        let mut layout = layout(&["a", "b", "c", "d"]);

        layout.pin(&"c".into());
        layout.pin(&"a".into());
        assert_eq!(ids(&layout), ["c", "a", "b", "d"]);
        assert_eq!(layout.pinned_count(), 2);

        layout.unpin(&"c".into());
        assert_eq!(ids(&layout), ["a", "c", "b", "d"]);
        assert_eq!(layout.pinned_count(), 1);
    }

    #[test]
    fn the_pointer_lands_a_column_before_the_first_middle_beyond_it() {
        let layout = layout(&["a", "b", "c", "d"]);
        let width = |_: &ColumnId| 100.0;

        // Past the middle of `c` (which starts at 100 with `a` taken out), short of `d`'s.
        let placement = placement_at(&layout, &"a".into(), 160.0, 0.0, width, true);
        assert_eq!(placement, Placement { section: Section::Scrolling, index: 2 });

        // The same point with the columns scrolled 100 to the left is over `d`'s middle.
        let placement = placement_at(&layout, &"a".into(), 160.0, 100.0, width, true);
        assert_eq!(placement, Placement { section: Section::Scrolling, index: 3 });

        // Within the empty pin zone.
        let placement = placement_at(&layout, &"c".into(), 10.0, 0.0, width, true);
        assert_eq!(placement, Placement { section: Section::Pinned, index: 0 });
        let placement = placement_at(&layout, &"c".into(), 10.0, 0.0, width, false);
        assert_eq!(placement, Placement { section: Section::Scrolling, index: 0 });
    }

    #[test]
    fn a_stored_layout_keeps_its_order_for_the_columns_that_remain() {
        let mut stored = layout(&["a", "b", "c"]);
        stored.place(&"c".into(), Placement { section: Section::Scrolling, index: 0 });
        stored.pin(&"b".into());
        assert_eq!(ids(&stored), ["b", "c", "a"]);

        let known: Vec<ColumnId> = ["a", "new", "c", "b"].into_iter().map(ColumnId::from).collect();
        let reconciled = stored.reconciled(&known);

        assert_eq!(ids(&reconciled), ["b", "c", "a", "new"]);
        assert!(reconciled.is_pinned(&"b".into()));
    }

    #[test]
    fn a_rearranged_layout_reads_back_as_written() {
        let mut written = layout(&["a", "b", "c", "d"]);
        written.place(&"d".into(), Placement { section: Section::Scrolling, index: 0 });
        written.pin(&"c".into());
        written.set_width(&"a".into(), Some(ColumnWidth(132.5)));

        let text = serde_json::to_string_pretty(&written).expect("a layout writes");
        let read: ColumnLayout = serde_json::from_str(&text).expect("a layout reads");

        insta::assert_snapshot!(text);
        assert_eq!(read, written);
    }

    #[test]
    fn a_stored_layout_out_of_order_is_read_with_its_pinned_columns_first() {
        let read: ColumnLayout = serde_json::from_str(
            r#"{"columns":[{"id":"a"},{"id":"b","pinned":true},{"id":"a"},{"id":"c","width":90.0}]}"#,
        )
        .expect("a layout");

        assert_eq!(ids(&read), ["b", "a", "c"]);
        assert_eq!(read.width(&"c".into()), Some(ColumnWidth(90.0)));
    }
}
