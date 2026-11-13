//! Replay parses a test makes panic, standing in for a fault in the decoders.
//!
//! Keyed by path and held process-wide: the parse runs on a worker thread, and other tests parse
//! other files at the same time.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

static PANICKING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Makes every later parse of `path` panic.
pub fn panic_on_parse(path: &Path) {
    PANICKING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(path.to_path_buf());
}

/// Panics when a test asked parses of `path` to.
pub(crate) fn panic_if_asked(path: &Path) {
    let asked = PANICKING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|asked| asked == path);
    if asked {
        panic!("a simulated decoder fault in {}", path.display());
    }
}
