//! The rows a tree shows, in order: every top-level node and, below each expanded node, its
//! children.

use std::collections::HashSet;
use std::ops::RangeInclusive;

use zgui::geom::CssPx;

use crate::surface::RowHeight;
use crate::surface::RowIndex;
use crate::surface::TreeDepth;
use crate::tree::model::TreeModel;

/// Whether a node can be expanded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Branch,
    Leaf,
}

/// Where a node sits among its siblings, which a screen reader announces: the rows around a
/// virtualized row are not built, so it cannot count them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SiblingPlace {
    /// From 1.
    pub position: usize,
    pub count: usize,
}

/// One visible row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FlatRow<K> {
    pub key: K,
    pub depth: TreeDepth,
    pub kind: NodeKind,
    pub place: SiblingPlace,
}

/// An expanded branch drawn at the top of the scrollport while its descendants scroll past it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PinnedRow {
    pub index: RowIndex,
    /// Where the row is drawn, from the top of the first row.
    pub top: CssPx,
}

/// The rows an expanded branch spans.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BranchSpan {
    index: RowIndex,
    /// The index just past the branch's last visible descendant.
    end: usize,
}

/// The expanded branches that show descendants, by depth, each depth in row order.
///
/// Built once per change of the rows, so finding the pinned branches as the tree scrolls costs a
/// search per pinned depth rather than a walk over the rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OpenBranches {
    by_depth: Vec<Vec<BranchSpan>>,
}

impl OpenBranches {
    /// The branches pinned to the top of a scrollport scrolled `offset` down rows of `height`,
    /// outermost first, at most `room` of them.
    ///
    /// Each branch whose row is above the pin line while one of its descendants is at it is
    /// pinned, the outermost at the top of the scrollport and each deeper one below the one that
    /// holds it. A branch whose last descendant is passing under its pinned row is pushed up with
    /// that row.
    pub fn pinned(&self, offset: CssPx, height: RowHeight, room: usize) -> Vec<PinnedRow> {
        let mut pinned = Vec::new();
        if height.0 <= 0.0 {
            return pinned;
        }

        let mut pin_line = offset.0.max(0.0);
        while pinned.len() < room {
            let Some(level) = self.by_depth.get(pinned.len()) else {
                break;
            };
            let at_line = (pin_line / height.0) as usize;
            // The row at the line is inside the last branch of this depth that starts at or above
            // it, or inside none of them.
            let starts_above = level.partition_point(|span| span.index.0 <= at_line);
            let Some(span) = starts_above.checked_sub(1).map(|at| level[at]) else {
                break;
            };
            let row_top = span.index.0 as f32 * height.0;
            if at_line >= span.end || row_top >= pin_line {
                break;
            }

            let top = pin_line.min(span.end as f32 * height.0 - height.0);
            pinned.push(PinnedRow { index: span.index, top: CssPx(top) });
            pin_line = top + height.0;
        }

        pinned
    }
}

/// The visible rows of a tree.
///
/// Expanding or collapsing one node splices that node's visible descendants in or out, so the
/// cost is the size of the subtree that changed, not of the tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FlatRows<K> {
    rows: Vec<FlatRow<K>>,
}

impl<K> FlatRows<K>
where
    K: Clone + Eq + std::hash::Hash,
{
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn build<M>(model: &M, expanded: &HashSet<K>) -> Self
    where
        M: TreeModel<Key = K>,
    {
        let mut rows = Vec::new();
        walk(model, expanded, model.roots(), TreeDepth(0), &mut rows);

        Self { rows }
    }

    /// Brings the rows from what `before` expanded to what `after` expands.
    pub fn follow<M>(&mut self, model: &M, before: &HashSet<K>, after: &HashSet<K>)
    where
        M: TreeModel<Key = K>,
    {
        // Each change is a scan for the node's row. Past this many, one walk of the tree is
        // cheaper than finding them one by one.
        const REBUILD_AFTER: usize = 32;

        let closed: Vec<&K> = before.difference(after).collect();
        let opened: Vec<&K> = after.difference(before).collect();
        if closed.len() + opened.len() > REBUILD_AFTER {
            *self = Self::build(model, after);
            return;
        }

        // A node and its ancestor can change together. Whichever is met first, the second finds
        // its rows already gone, or already present, and leaves them.
        for key in closed {
            if let Some(index) = self.position(key) {
                self.collapse(index);
            }
        }
        for key in opened {
            if let Some(index) = self.position(key) {
                self.expand(model, after, index);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn get(&self, index: RowIndex) -> Option<&FlatRow<K>> {
        self.rows.get(index.0)
    }

    pub fn position(&self, key: &K) -> Option<RowIndex> {
        self.rows.iter().position(|row| row.key == *key).map(RowIndex)
    }

    /// The row of the node directly above the row at `index`.
    pub fn parent(&self, index: RowIndex) -> Option<RowIndex> {
        let depth = self.rows.get(index.0)?.depth;

        self.rows[..index.0].iter().rposition(|row| row.depth < depth).map(RowIndex)
    }

    /// Whether the row at `index` has its children showing below it.
    pub fn is_open(&self, index: RowIndex) -> bool {
        match (self.rows.get(index.0), self.rows.get(index.0 + 1)) {
            (Some(row), Some(next)) => next.depth > row.depth,
            _ => false,
        }
    }

    /// The keys of the rows from one index to another, inclusive, in either order.
    pub fn keys_between(&self, ends: RangeInclusive<RowIndex>) -> impl Iterator<Item = &K> {
        let (from, to) = (ends.start().0, ends.end().0);
        let last = self.rows.len().saturating_sub(1);
        let span = from.min(to).min(last)..=from.max(to).min(last);
        let rows = if self.rows.is_empty() { &self.rows[..] } else { &self.rows[span] };

        rows.iter().map(|row| &row.key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.rows.iter().map(|row| &row.key)
    }

    /// The expanded branches that show descendants, with the rows each spans.
    pub fn open_branches(&self) -> OpenBranches {
        let mut by_depth: Vec<Vec<BranchSpan>> = Vec::new();
        // The depth and the slot in `by_depth` of each branch whose last descendant is not yet
        // found, innermost last.
        let mut unclosed: Vec<(TreeDepth, usize)> = Vec::new();

        for (at, row) in self.rows.iter().enumerate() {
            while let Some(&(depth, slot)) = unclosed.last() {
                if depth < row.depth {
                    break;
                }
                by_depth[depth.0][slot].end = at;
                unclosed.pop();
            }

            if !self.is_open(RowIndex(at)) {
                continue;
            }
            if by_depth.len() <= row.depth.0 {
                by_depth.resize_with(row.depth.0 + 1, Vec::new);
            }
            let level = &mut by_depth[row.depth.0];
            level.push(BranchSpan { index: RowIndex(at), end: self.rows.len() });
            unclosed.push((row.depth, level.len() - 1));
        }

        OpenBranches { by_depth }
    }

    fn expand<M>(&mut self, model: &M, expanded: &HashSet<K>, index: RowIndex)
    where
        M: TreeModel<Key = K>,
    {
        let row = &self.rows[index.0];
        if row.kind == NodeKind::Leaf || self.is_open(index) {
            return;
        }

        let depth = TreeDepth(row.depth.0 + 1);
        let mut inserted = Vec::new();
        walk(model, expanded, model.children(&row.key), depth, &mut inserted);
        let at = index.0 + 1;
        self.rows.splice(at..at, inserted);
    }

    fn collapse(&mut self, index: RowIndex) {
        let depth = self.rows[index.0].depth;
        let start = index.0 + 1;
        let below = self.rows[start..].iter().take_while(|row| row.depth > depth).count();
        self.rows.drain(start..start + below);
    }
}

/// The siblings of one level still to be walked.
struct Level<'model, K> {
    keys: &'model [K],
    depth: TreeDepth,
    /// How many siblings the level had before any were walked.
    count: usize,
}

/// Appends `level` at `depth` to `out` in reading order, each expanded branch followed by its
/// own visible rows.
fn walk<'model, M>(
    model: &'model M,
    expanded: &HashSet<M::Key>,
    level: &'model [M::Key],
    depth: TreeDepth,
    out: &mut Vec<FlatRow<M::Key>>,
) where
    M: TreeModel,
{
    let mut pending = vec![Level { keys: level, depth, count: level.len() }];
    while let Some(Level { keys, depth, count }) = pending.pop() {
        let Some((key, rest)) = keys.split_first() else {
            continue;
        };
        pending.push(Level { keys: rest, depth, count });

        let kind = if model.is_branch(key) { NodeKind::Branch } else { NodeKind::Leaf };
        let place = SiblingPlace { position: count - rest.len(), count };
        out.push(FlatRow { key: key.clone(), depth, kind, place });
        if kind == NodeKind::Branch && expanded.contains(key) {
            let children = model.children(key);
            pending.push(Level { keys: children, depth: TreeDepth(depth.0 + 1), count: children.len() });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A tree written as `(key, parent)` pairs in order.
    struct Nodes {
        roots: Vec<u32>,
        children: HashMap<u32, Vec<u32>>,
        parents: HashMap<u32, u32>,
    }

    impl Nodes {
        fn new(pairs: &[(u32, Option<u32>)]) -> Self {
            let mut nodes = Self { roots: Vec::new(), children: HashMap::new(), parents: HashMap::new() };
            for &(key, parent) in pairs {
                match parent {
                    Some(parent) => {
                        nodes.children.entry(parent).or_default().push(key);
                        nodes.parents.insert(key, parent);
                    }
                    None => nodes.roots.push(key),
                }
            }

            nodes
        }
    }

    impl TreeModel for Nodes {
        type Key = u32;

        fn roots(&self) -> &[u32] {
            &self.roots
        }

        fn children(&self, key: &u32) -> &[u32] {
            self.children.get(key).map_or(&[], Vec::as_slice)
        }

        fn parent(&self, key: &u32) -> Option<u32> {
            self.parents.get(key).copied()
        }
    }

    fn sample() -> Nodes {
        Nodes::new(&[
            (1, None),
            (10, Some(1)),
            (100, Some(10)),
            (101, Some(10)),
            (11, Some(1)),
            (2, None),
            (20, Some(2)),
            (3, None),
        ])
    }

    fn shown(rows: &FlatRows<u32>) -> Vec<(u32, usize)> {
        rows.rows.iter().map(|row| (row.key, row.depth.0)).collect()
    }

    #[test]
    fn collapsed_nodes_hide_their_descendants() {
        let model = sample();
        let expanded = HashSet::from([1, 100]);

        let rows = FlatRows::build(&model, &expanded);

        // 10 is closed, so 100 being open shows nothing.
        assert_eq!(shown(&rows), vec![(1, 0), (10, 1), (11, 1), (2, 0), (3, 0)]);
    }

    #[test]
    fn following_a_change_matches_a_fresh_walk() {
        let model = sample();
        let steps: [&[u32]; 6] = [&[1], &[1, 10], &[1, 10, 2], &[10, 2], &[1, 10, 2], &[]];

        let mut before = HashSet::new();
        let mut rows = FlatRows::build(&model, &before);
        for step in steps {
            let after: HashSet<u32> = step.iter().copied().collect();
            rows.follow(&model, &before, &after);
            assert_eq!(shown(&rows), shown(&FlatRows::build(&model, &after)), "after expanding {step:?}");
            before = after;
        }
    }

    #[test]
    fn a_parent_and_child_opened_together_show_the_child_rows_once() {
        let model = sample();
        let mut rows = FlatRows::build(&model, &HashSet::new());

        let after = HashSet::from([1, 10]);
        rows.follow(&model, &HashSet::new(), &after);

        assert_eq!(shown(&rows), vec![(1, 0), (10, 1), (100, 2), (101, 2), (11, 1), (2, 0), (3, 0)]);
    }

    #[test]
    fn parent_row_is_the_nearest_shallower_row_above() {
        let model = sample();
        let rows = FlatRows::build(&model, &HashSet::from([1, 10]));

        assert_eq!(rows.parent(RowIndex(3)), Some(RowIndex(1)), "101 is below 10");
        assert_eq!(rows.parent(RowIndex(4)), Some(RowIndex(0)), "11 is below 1");
        assert_eq!(rows.parent(RowIndex(5)), None, "2 is a top-level node");
    }

    #[test]
    fn branches_pin_while_their_descendants_scroll_past() {
        let model = sample();
        // Ten pixels a row: 1 [10 [100, 101], 11], 2 [20], 3.
        let branches = FlatRows::build(&model, &HashSet::from([1, 10, 2])).open_branches();
        let pinned_at = |offset: f32, room: usize| -> Vec<(usize, f32)> {
            let pinned = branches.pinned(CssPx(offset), RowHeight(10.0), room);
            pinned.into_iter().map(|pin| (pin.index.0, pin.top.0)).collect()
        };

        assert_eq!(pinned_at(0.0, 9), vec![], "no row is above the top");
        assert_eq!(pinned_at(15.0, 9), vec![(0, 15.0), (1, 25.0)], "1 holds 10, which holds 100");
        assert_eq!(pinned_at(22.0, 9), vec![(0, 22.0), (1, 30.0)], "10 is pushed up by its last row");
        assert_eq!(pinned_at(45.0, 9), vec![(0, 40.0)], "1 is pushed up by 11");
        assert_eq!(pinned_at(50.0, 9), vec![], "2 is at the top in its own place");
        assert_eq!(pinned_at(55.0, 9), vec![(5, 55.0)], "2 pins over 20");
        assert_eq!(pinned_at(75.0, 9), vec![], "3 is a leaf");
        assert_eq!(pinned_at(15.0, 1), vec![(0, 15.0)], "a port with room for one pins only 1");
    }
}
