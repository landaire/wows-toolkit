//! Directory watches a test drives itself.
//!
//! The test scheduler refuses wakeups from notify's own thread, so once a
//! test calls [`install`], a listing on that thread registers its watch here
//! and the test reports what changed with [`report`].

use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use futures::channel::mpsc::UnboundedSender;

use super::browser_view::DirChange;

#[derive(Default)]
struct Watches {
    next_id: u64,
    open: Vec<OpenWatch>,
}

struct OpenWatch {
    id: u64,
    dir: PathBuf,
    changes: UnboundedSender<DirChange>,
}

thread_local! {
    static WATCHES: RefCell<Option<Watches>> = const { RefCell::new(None) };
}

/// One listing's watch. Dropping it ends the watch, as dropping a filesystem
/// watcher does.
pub(crate) struct Registration {
    id: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // The thread's storage may already be gone when the test ends.
        let _ = WATCHES.try_with(|watches| {
            if let Some(watches) = watches.borrow_mut().as_mut() {
                watches.open.retain(|watch| watch.id != self.id);
            }
        });
    }
}

/// Routes every watch started on this thread from now on through [`report`].
pub(crate) fn install() {
    WATCHES.with(|watches| *watches.borrow_mut() = Some(Watches::default()));
}

/// Registers a watch on `dir`, when this thread has [`install`]ed simulated
/// watches.
pub(super) fn register(dir: &Path, changes: &UnboundedSender<DirChange>) -> Option<Registration> {
    WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let watches = watches.as_mut()?;
        let id = watches.next_id;
        watches.next_id += 1;
        watches.open.push(OpenWatch { id, dir: dir.to_path_buf(), changes: changes.clone() });

        Some(Registration { id })
    })
}

/// How many watches are open on `dir`.
pub(crate) fn watching(dir: &Path) -> usize {
    WATCHES.with(|watches| {
        watches.borrow().as_ref().map_or(0, |watches| watches.open.iter().filter(|watch| watch.dir == dir).count())
    })
}

/// Reports `change` to every watch open on `dir`, and says how many it reached.
pub(crate) fn report(dir: &Path, change: DirChange) -> usize {
    WATCHES.with(|watches| {
        let watches = watches.borrow();
        let Some(watches) = watches.as_ref() else { return 0 };

        watches
            .open
            .iter()
            .filter(|watch| watch.dir == dir)
            .filter(|watch| watch.changes.unbounded_send(change.clone()).is_ok())
            .count()
    })
}
