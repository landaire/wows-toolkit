//! Directory watches a test drives itself.
//!
//! The test scheduler refuses wakeups from notify's own thread, so once a
//! test calls [`install`], a listing on that thread registers its watch here
//! and the test reports what changed with [`report`] and [`overflow`], and
//! how the watch stops with [`fail`], [`crash`] and [`exit`].

use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use crate::replay_dir::DirChange;
use crate::replay_dir::WatchSink;
use crate::replay_dir::WatchStopCause;

#[derive(Default)]
struct Watches {
    next_id: u64,
    open: Vec<OpenWatch>,
    /// Every directory a watch was asked for, once per asking.
    asked: Vec<PathBuf>,
    /// Directories a watch is refused on.
    refused: Vec<PathBuf>,
}

struct OpenWatch {
    id: u64,
    dir: PathBuf,
    sink: WatchSink,
}

thread_local! {
    static WATCHES: RefCell<Option<Watches>> = const { RefCell::new(None) };
}

/// One listing's watch. Dropping it ends the watch, as dropping a filesystem
/// watcher does.
pub struct Registration {
    id: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // Taken out before it is dropped: the sink is not dropped while the
        // watches are borrowed.
        let closed = WATCHES
            .try_with(|watches| {
                let mut watches = watches.borrow_mut();
                let watches = watches.as_mut()?;
                let at = watches.open.iter().position(|watch| watch.id == self.id)?;
                Some(watches.open.remove(at))
            })
            .ok()
            .flatten();
        drop(closed);
    }
}

/// Routes every watch started on this thread from now on through [`report`].
pub fn install() {
    WATCHES.with(|watches| *watches.borrow_mut() = Some(Watches::default()));
}

/// Whether this thread has [`install`]ed simulated watches.
pub fn is_installed() -> bool {
    WATCHES.with(|watches| watches.borrow().is_some())
}

/// Registers a watch on `dir`, unless watches on it are [`refuse`]d.
pub fn register(dir: &Path, sink: WatchSink) -> Result<Registration, WatchStopCause> {
    WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let watches = watches.as_mut().expect("simulated watches are installed");
        watches.asked.push(dir.to_path_buf());
        if watches.refused.iter().any(|refused| refused == dir) {
            return Err(WatchStopCause::NotWatchable { error: "refused by the test".to_owned() });
        }

        let id = watches.next_id;
        watches.next_id += 1;
        watches.open.push(OpenWatch { id, dir: dir.to_path_buf(), sink });

        Ok(Registration { id })
    })
}

/// How many watches are open on `dir`.
pub fn watching(dir: &Path) -> usize {
    WATCHES.with(|watches| {
        watches.borrow().as_ref().map_or(0, |watches| watches.open.iter().filter(|watch| watch.dir == dir).count())
    })
}

/// How many times a watch on `dir` has been asked for, whether or not it
/// started.
pub fn asked(dir: &Path) -> usize {
    WATCHES.with(|watches| {
        watches.borrow().as_ref().map_or(0, |watches| watches.asked.iter().filter(|asked| *asked == dir).count())
    })
}

/// Refuses every watch asked for on `dir` from now on, or stops refusing them.
pub fn refuse(dir: &Path, refused: bool) {
    WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let watches = watches.as_mut().expect("simulated watches are installed");
        watches.refused.retain(|held| held != dir);
        if refused {
            watches.refused.push(dir.to_path_buf());
        }
    });
}

/// Reports `change` to every watch open on `dir`, and says how many it reached.
pub fn report(dir: &Path, change: DirChange) -> usize {
    WATCHES.with(|watches| {
        let watches = watches.borrow();
        let Some(watches) = watches.as_ref() else { return 0 };

        watches.open.iter().filter(|watch| watch.dir == dir).filter(|watch| watch.sink.change(change.clone())).count()
    })
}

/// Reports an error from the watcher to every watch open on `dir`, as notify
/// does from its own thread.
pub fn fail(dir: &Path, error: &str) {
    WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let Some(watches) = watches.as_mut() else { return };
        for watch in watches.open.iter_mut().filter(|watch| watch.dir == dir) {
            watch.sink.handle(Err(notify::Error::generic(error)));
        }
    });
}

/// Tells every watch open on `dir` that the watcher dropped events, as notify
/// does when the kernel's queue overflows.
pub fn overflow(dir: &Path) {
    WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let Some(watches) = watches.as_mut() else { return };
        for watch in watches.open.iter_mut().filter(|watch| watch.dir == dir) {
            let lost = notify::Event::new(notify::EventKind::Other).set_flag(notify::event::Flag::Rescan);
            watch.sink.handle(Ok(lost));
        }
    });
}

/// Panics on every watch open on `dir`, as the watcher's thread does when it
/// dies: each sink is dropped by the unwinding.
pub fn crash(dir: &Path, message: &'static str) {
    for sink in take(dir) {
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _held = sink;
            std::panic::panic_any(message);
        }));
        assert!(unwound.is_err(), "the watcher's thread panicked");
    }
}

/// Ends every watch open on `dir` without a word, as a watcher's thread that
/// returns does.
pub fn exit(dir: &Path) {
    drop(take(dir));
}

/// Takes the sinks of the watches open on `dir` out of the open watches.
fn take(dir: &Path) -> Vec<WatchSink> {
    WATCHES.with(|watches| {
        let mut watches = watches.borrow_mut();
        let Some(watches) = watches.as_mut() else { return Vec::new() };
        let (taken, kept): (Vec<OpenWatch>, Vec<OpenWatch>) =
            std::mem::take(&mut watches.open).into_iter().partition(|watch| watch.dir == dir);
        watches.open = kept;

        taken.into_iter().map(|watch| watch.sink).collect()
    })
}
