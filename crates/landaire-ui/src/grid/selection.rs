use std::collections::HashSet;
use std::hash::Hash;

/// How many rows a reader may select at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridSelectionMode {
    None,
    #[default]
    Single,
    /// Ctrl adds or removes one row, shift selects a run of rows.
    Multiple,
}

/// The rows a reader has selected, by key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection<K>
where
    K: Eq + Hash,
{
    keys: HashSet<K>,
    /// Where a shift-selected run starts.
    anchor: Option<K>,
}

impl<K> Selection<K>
where
    K: Clone + Eq + Hash,
{
    pub fn new() -> Self {
        Self { keys: HashSet::new(), anchor: None }
    }

    pub fn contains(&self, key: &K) -> bool {
        self.keys.contains(key)
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.keys.iter()
    }

    /// Selects `key` alone.
    pub fn select_only(&mut self, key: K) {
        self.keys.clear();
        self.keys.insert(key.clone());
        self.anchor = Some(key);
    }

    /// Adds `key` to the selection, or takes it out if it is in it.
    pub fn toggle(&mut self, key: K) {
        if !self.keys.remove(&key) {
            self.keys.insert(key.clone());
        }
        self.anchor = Some(key);
    }

    /// Selects the run of rows from the anchor to `key` in `order`, alone. With no anchor, or one
    /// no longer in `order`, it selects `key` alone.
    pub fn select_run(&mut self, key: K, order: &[K]) {
        let anchor = self.anchor.as_ref().and_then(|anchor| order.iter().position(|other| other == anchor));
        let (Some(from), Some(to)) = (anchor, order.iter().position(|other| *other == key)) else {
            self.select_only(key);
            return;
        };

        let run = if from <= to { from..=to } else { to..=from };
        self.keys = order[run].iter().cloned().collect();
    }

    /// Selects every row in `order`, keeping the anchor.
    pub fn select_all(&mut self, order: &[K]) {
        self.keys = order.iter().cloned().collect();
    }

    pub fn clear(&mut self) {
        self.keys.clear();
        self.anchor = None;
    }
}

impl<K> Default for Selection<K>
where
    K: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_reaches_from_the_anchor_either_way() {
        let order = [1, 2, 3, 4, 5];
        let mut selection = Selection::new();
        selection.select_only(3);

        selection.select_run(5, &order);
        assert!([3, 4, 5].iter().all(|key| selection.contains(key)));
        assert_eq!(selection.len(), 3);

        selection.select_run(1, &order);
        assert!([1, 2, 3].iter().all(|key| selection.contains(key)));
        assert_eq!(selection.len(), 3);
    }
}
