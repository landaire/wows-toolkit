//! The extraction queue.
//!
//! Queueing is a decision about what the user asked for, not a rendering
//! concern, so both front ends share it: the same de-duplication, the same
//! rule about what "extract everything listed" means.

use wowsunpack::vfs::VfsPath;

use super::listing::ListingEntry;

/// Entries waiting to be extracted, in the order they were queued.
///
/// De-duplicates by VFS path: queueing the same entry twice would otherwise
/// extract and overwrite the same file twice.
#[derive(Debug, Clone, Default)]
pub struct ExtractQueue {
    entries: Vec<VfsPath>,
}

impl ExtractQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[VfsPath] {
        &self.entries
    }

    pub fn contains(&self, path: &VfsPath) -> bool {
        self.entries.iter().any(|queued| queued.as_str() == path.as_str())
    }

    /// Queues `path` unless it is already there. Returns whether it was added,
    /// so a caller can tell a fresh queue from a repeat click.
    pub fn push(&mut self, path: VfsPath) -> bool {
        if self.contains(&path) {
            return false;
        }
        self.entries.push(path);
        true
    }

    /// Queues the files of a listing.
    ///
    /// Directory rows are skipped rather than queued whole: a listing showing
    /// twelve rows at "/res" would otherwise extract the entire install, which
    /// is not what "extract what is listed" says. A directory reaches the
    /// queue only when the user queues that row on purpose.
    pub fn push_listed_files(&mut self, entries: &[ListingEntry]) -> usize {
        entries.iter().filter(|entry| !entry.is_dir).filter(|entry| self.push(entry.path.clone())).count()
    }

    /// Removes one entry by path, for the queue popover's per-row remove.
    pub fn remove(&mut self, path: &VfsPath) {
        self.entries.retain(|queued| queued.as_str() != path.as_str());
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Takes the queue, leaving it empty, to hand to an extraction run.
    pub fn take(&mut self) -> Vec<VfsPath> {
        std::mem::take(&mut self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::super::listing::directory_entries;
    use super::*;
    use std::io::Write as _;
    use wowsunpack::vfs::MemoryFS;

    fn fixture() -> VfsPath {
        let root: VfsPath = MemoryFS::new().into();
        root.join("res/content").unwrap().create_dir_all().unwrap();
        for (path, body) in [("res/content/a.xml", "<a/>"), ("res/one.txt", "1"), ("res/two.txt", "2")] {
            root.join(path).unwrap().create_file().unwrap().write_all(body.as_bytes()).unwrap();
        }
        root
    }

    #[test]
    fn queueing_the_same_path_twice_adds_it_once() {
        let root = fixture();
        let file = root.join("res/one.txt").unwrap();
        let mut queue = ExtractQueue::new();

        assert!(queue.push(file.clone()), "the first queue is accepted");
        assert!(!queue.push(file.clone()), "the repeat is refused");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn queueing_a_listing_takes_its_files_and_leaves_its_directories() {
        let root = fixture();
        let listed = directory_entries(&root, "/res");
        assert!(listed.iter().any(|entry| entry.is_dir), "the fixture lists a directory");

        let mut queue = ExtractQueue::new();
        let added = queue.push_listed_files(&listed);

        assert_eq!(added, 2, "one.txt and two.txt only");
        assert_eq!(queue.len(), 2);
        assert!(
            queue.entries().iter().all(|path| path.as_str().ends_with(".txt")),
            "the content directory is not queued whole"
        );
    }

    #[test]
    fn queueing_the_same_listing_twice_does_not_double_it() {
        let root = fixture();
        let listed = directory_entries(&root, "/res");
        let mut queue = ExtractQueue::new();

        queue.push_listed_files(&listed);
        let added_again = queue.push_listed_files(&listed);

        assert_eq!(added_again, 0);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn an_entry_can_be_removed_without_disturbing_the_rest() {
        let root = fixture();
        let mut queue = ExtractQueue::new();
        queue.push(root.join("res/one.txt").unwrap());
        queue.push(root.join("res/two.txt").unwrap());

        queue.remove(&root.join("res/one.txt").unwrap());

        assert_eq!(queue.len(), 1);
        assert!(queue.entries()[0].as_str().ends_with("two.txt"));
    }

    #[test]
    fn taking_the_queue_empties_it() {
        let root = fixture();
        let mut queue = ExtractQueue::new();
        queue.push(root.join("res/one.txt").unwrap());

        let taken = queue.take();

        assert_eq!(taken.len(), 1);
        assert!(queue.is_empty());
    }
}
