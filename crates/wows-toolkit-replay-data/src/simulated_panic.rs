//! Replay reads a test makes panic, standing in for faults in the decoders, in the loader and in
//! the live match poll.
//!
//! Keyed by path and held process-wide: the read runs on a worker thread, and other tests read
//! other files at the same time.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

/// Where a simulated fault is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    /// In the parse, which contains it.
    Parse,
    /// In the loader around the parse, which stops the loader's thread.
    Loader,
    /// In the live match poll of a replays directory, which stops the poll's thread.
    LivePoll,
}

static FAULTS: Mutex<Vec<(PathBuf, Fault)>> = Mutex::new(Vec::new());

/// Makes every later parse of `path` panic.
pub fn panic_on_parse(path: &Path) {
    ask(path, Fault::Parse);
}

/// Makes the replay loader's thread panic, outside the parse, whenever it reads `path`.
pub fn stop_loader_on(path: &Path) {
    ask(path, Fault::Loader);
}

/// Makes the next live match poll of the replays directory `dir` stop its thread.
pub fn stop_live_poll_once(dir: &Path) {
    ask(dir, Fault::LivePoll);
}

fn ask(path: &Path, fault: Fault) {
    FAULTS.lock().unwrap_or_else(PoisonError::into_inner).push((path.to_path_buf(), fault));
}

fn asked(path: &Path, fault: Fault) -> bool {
    FAULTS.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|asked| asked.0 == path && asked.1 == fault)
}

/// Panics when a test asked parses of `path` to.
pub(crate) fn panic_if_asked(path: &Path) {
    if asked(path, Fault::Parse) {
        panic!("a simulated decoder fault in {}", path.display());
    }
}

/// Panics when a test asked the loader to stop on `path`.
pub(crate) fn stop_loader_if_asked(path: &Path) {
    if asked(path, Fault::Loader) {
        panic!("a simulated loader fault in {}", path.display());
    }
}

/// Panics, once, when a test asked the live match poll of `dir` to stop.
pub(crate) fn stop_live_poll_if_asked(dir: &Path) {
    let asked = {
        let mut faults = FAULTS.lock().unwrap_or_else(PoisonError::into_inner);
        let before = faults.len();
        faults.retain(|asked| !(asked.0 == dir && asked.1 == Fault::LivePoll));
        faults.len() != before
    };
    if asked {
        panic!("a simulated live match poll fault in {}", dir.display());
    }
}
