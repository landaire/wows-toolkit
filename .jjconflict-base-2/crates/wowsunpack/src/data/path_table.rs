//! Paths held as one buffer rather than one allocation apiece.
//!
//! A build addresses hundreds of thousands of paths, and a refcounted handle
//! per path costs a header, a malloc block and the allocator's per-block
//! rounding on top of the bytes themselves. [`PathTable`] holds the bytes end
//! to end and addresses each path by [`PathId`]; [`PathSet`] adds the lookup
//! from path text back to its id.

use std::num::NonZeroU32;

use rustc_hash::FxBuildHasher;
use std::hash::BuildHasher;

/// Where one path sits in a [`PathTable`].
///
/// A position, not a key: an id read against a table that did not issue it
/// names whatever that table holds at the same position. One is never handed
/// across tables, and a table that other code addresses by position exposes an
/// index of its own rather than passing this on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathId(u32);

impl PathId {
    /// Position in the issuing table, which is also the position of anything
    /// stored parallel to it.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// An append-only list of paths, stored as one buffer.
#[derive(Debug, Clone)]
pub struct PathTable {
    bytes: String,
    /// Byte offset of each path, with a final entry for the end of the last,
    /// so path `i` is `bytes[starts[i]..starts[i + 1]]`.
    starts: Vec<u32>,
}

impl Default for PathTable {
    fn default() -> Self {
        Self { bytes: String::new(), starts: vec![0] }
    }
}

impl PathTable {
    /// Room for `paths` paths of `bytes` total length, so a whole build's
    /// worth lands in two allocations that never grow.
    pub fn with_capacity(paths: usize, bytes: usize) -> Self {
        let mut starts = Vec::with_capacity(paths + 1);
        starts.push(0);
        Self { bytes: String::with_capacity(bytes), starts }
    }

    pub fn len(&self) -> usize {
        self.starts.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Appends `path`, whether or not an equal one is already held.
    pub fn push(&mut self, path: &str) -> PathId {
        let id = PathId(u32::try_from(self.len()).expect("a build addresses far fewer than 4 billion paths"));
        self.bytes.push_str(path);
        let end = u32::try_from(self.bytes.len()).expect("a build's paths are far shorter than 4 GiB in total");
        self.starts.push(end);
        id
    }

    /// The path `id` names. Panics past the end of the table.
    pub fn get(&self, id: PathId) -> &str {
        let at = id.index();
        &self.bytes[self.starts[at] as usize..self.starts[at + 1] as usize]
    }

    /// Every path, in the order they were appended.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &str> {
        (0..self.len()).map(|at| self.get(PathId(at as u32)))
    }

    /// Every id the table holds now. Borrows nothing, so a caller can hold the
    /// iterator while appending, but the ids it yields are fixed when it is
    /// made: a walk that must cover its own appends uses [`Self::id_at`].
    pub fn ids(&self) -> impl ExactSizeIterator<Item = PathId> + use<> {
        (0..self.len() as u32).map(PathId)
    }

    /// The id issued `index` pushes in, or `None` past the end. Reads the
    /// length afresh, so a walk over rising indices covers what it appends.
    pub fn id_at(&self, index: usize) -> Option<PathId> {
        (index < self.len()).then_some(PathId(index as u32))
    }

    /// Releases the growth slack once no more paths will be appended.
    pub fn shrink_to_fit(&mut self) {
        self.bytes.shrink_to_fit();
        self.starts.shrink_to_fit();
    }
}

/// A [`PathTable`] that can also be searched by path text.
///
/// Open-addressed rather than a `HashMap` because the keys live in the table
/// rather than in the map, which no std map can express.
#[derive(Debug, Clone)]
pub struct PathSet {
    paths: PathTable,
    /// Power-of-two slot table. A slot holds its path's id biased by one, so
    /// an empty slot is `None` without widening the slot past four bytes.
    slots: Vec<Option<NonZeroU32>>,
}

/// Slots per stored path. Two keeps the table under half full, which holds
/// the average probe to a little over one; the growth test below is what
/// enforces it, so the two move together.
const SLOTS_PER_PATH: usize = 2;

impl Default for PathSet {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

impl PathSet {
    /// Room for `paths` paths before the slot table has to grow.
    pub fn with_capacity(paths: usize) -> Self {
        Self { paths: PathTable::default(), slots: vec![None; slot_count(paths)] }
    }

    /// Room for `paths` paths of `bytes` total length.
    pub fn with_byte_capacity(paths: usize, bytes: usize) -> Self {
        Self { paths: PathTable::with_capacity(paths, bytes), slots: vec![None; slot_count(paths)] }
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn get(&self, id: PathId) -> &str {
        self.paths.get(id)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &str> {
        self.paths.iter()
    }

    pub fn ids(&self) -> impl ExactSizeIterator<Item = PathId> + use<> {
        self.paths.ids()
    }

    pub fn id_at(&self, index: usize) -> Option<PathId> {
        self.paths.id_at(index)
    }

    /// The id already held for `path`.
    pub fn id_of(&self, path: &str) -> Option<PathId> {
        self.probe(path).ok()
    }

    /// The id of `path`, appending it if it is not held yet.
    pub fn intern(&mut self, path: &str) -> PathId {
        let free = match self.probe(path) {
            Ok(id) => return id,
            Err(free) => free,
        };

        if (self.paths.len() + 1) * SLOTS_PER_PATH > self.slots.len() {
            // Growing moves every path, so the slot the probe found is gone
            // and the new path is placed from scratch.
            self.grow();
            let id = self.paths.push(path);
            self.place(id);
            return id;
        }

        let id = self.paths.push(path);
        self.slots[free] = Some(bias(id));
        id
    }

    /// Releases the path buffer's growth slack. The slot table keeps its size,
    /// since that is what bounds the probe length.
    pub fn shrink_to_fit(&mut self) {
        self.paths.shrink_to_fit();
    }

    fn mask(&self) -> usize {
        self.slots.len() - 1
    }

    /// Slots are indexed by the low bits of the hash, which `FxHasher` fills
    /// by rotating the accumulator before it returns.
    fn probe_start(&self, path: &str) -> usize {
        FxBuildHasher.hash_one(path) as usize & self.mask()
    }

    /// The id held for `path`, or the free slot the search ended on. The slot
    /// table is never more than half full, so the search always reaches one.
    fn probe(&self, path: &str) -> Result<PathId, usize> {
        let mut at = self.probe_start(path);
        loop {
            match self.slots[at] {
                None => return Err(at),
                Some(held) => {
                    let id = unbias(held);
                    if self.paths.get(id) == path {
                        return Ok(id);
                    }
                }
            }
            at = (at + 1) & self.mask();
        }
    }

    /// Puts `id` in the first free slot from its hash. The caller has already
    /// established that the path is not held, so no equality check is needed.
    fn place(&mut self, id: PathId) {
        let mut at = self.probe_start(self.paths.get(id));
        while self.slots[at].is_some() {
            at = (at + 1) & self.mask();
        }
        self.slots[at] = Some(bias(id));
    }

    /// Doubles the slot table and re-places what it already holds. Called
    /// before the path that triggered it is appended, so every id is placed
    /// exactly once.
    fn grow(&mut self) {
        self.slots = vec![None; self.slots.len() * 2];
        for id in self.paths.ids() {
            self.place(id);
        }
    }
}

/// The smallest power of two that holds `paths` at the load factor, and never
/// zero: the mask arithmetic needs at least one slot.
fn slot_count(paths: usize) -> usize {
    ((paths + 1) * SLOTS_PER_PATH).next_power_of_two()
}

fn bias(id: PathId) -> NonZeroU32 {
    NonZeroU32::new(id.0 + 1).expect("one more than an id is never zero")
}

fn unbias(slot: NonZeroU32) -> PathId {
    PathId(slot.get() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_hands_back_what_was_pushed_in_order() {
        let mut table = PathTable::default();
        let a = table.push("/res/a.xml");
        let b = table.push("/res/b.xml");
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(a), "/res/a.xml");
        assert_eq!(table.get(b), "/res/b.xml");
        assert_eq!(table.iter().collect::<Vec<_>>(), vec!["/res/a.xml", "/res/b.xml"]);
        assert_eq!(a.index(), 0);
        assert_eq!(b.index(), 1);
    }

    #[test]
    fn an_empty_table_holds_nothing() {
        let table = PathTable::default();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert_eq!(table.iter().count(), 0);
    }

    #[test]
    fn a_table_holds_the_empty_path_apart_from_the_paths_beside_it() {
        let mut table = PathTable::default();
        let empty = table.push("");
        let root = table.push("/");
        assert_eq!(table.get(empty), "");
        assert_eq!(table.get(root), "/");
    }

    #[test]
    fn interning_the_same_path_twice_yields_the_same_id() {
        let mut set = PathSet::default();
        let first = set.intern("/res/content/a.xml");
        let second = set.intern("/res/content/a.xml");
        assert_eq!(first, second);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn a_path_that_was_never_interned_has_no_id() {
        let mut set = PathSet::default();
        set.intern("/res/a.xml");
        assert_eq!(set.id_of("/res/b.xml"), None);
        assert_eq!(set.id_of(""), None);
    }

    #[test]
    fn every_path_is_still_found_after_the_slot_table_has_grown_several_times() {
        // Enough to force the table past its starting size repeatedly, with
        // paths whose shared prefix makes a weak hash collide.
        let paths: Vec<String> = (0..5000).map(|at| format!("/res/content/gameparams/{at}.xml")).collect();
        let mut set = PathSet::with_capacity(4);
        let ids: Vec<PathId> = paths.iter().map(|path| set.intern(path)).collect();

        assert_eq!(set.len(), paths.len());
        for (path, id) in paths.iter().zip(&ids) {
            assert_eq!(set.id_of(path), Some(*id), "{path} is still found");
            assert_eq!(set.get(*id), path.as_str());
        }
        assert_eq!(set.id_of("/res/content/gameparams/5000.xml"), None);
    }

    #[test]
    fn a_set_sized_for_its_paths_never_grows_its_slot_table() {
        let paths: Vec<String> = (0..1000).map(|at| format!("/res/{at}")).collect();
        let mut set = PathSet::with_capacity(paths.len());
        let slots = set.slots.len();
        for path in &paths {
            set.intern(path);
        }
        assert_eq!(set.slots.len(), slots, "the slot table was sized for them up front");
        assert_eq!(set.len(), paths.len());
    }

    #[test]
    fn a_preallocated_set_and_a_grown_one_agree() {
        let paths: Vec<String> = (0..500).map(|at| format!("/res/{at}")).collect();
        let mut grown = PathSet::with_capacity(0);
        let mut sized = PathSet::with_byte_capacity(paths.len(), 8 * paths.len());
        for path in &paths {
            assert_eq!(grown.intern(path), sized.intern(path));
        }
        assert_eq!(grown.iter().collect::<Vec<_>>(), sized.iter().collect::<Vec<_>>());
    }
}
