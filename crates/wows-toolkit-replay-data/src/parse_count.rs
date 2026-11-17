//! How many times each replay was parsed, for a test to check that what reads a replay reads it
//! once.
//!
//! Keyed by path and held process-wide: the parse runs on a worker thread, and other tests parse
//! other files at the same time.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

static PARSES: Mutex<Option<HashMap<PathBuf, usize>>> = Mutex::new(None);

/// How many parses of `path` have started.
pub fn parses_of(path: &Path) -> usize {
    PARSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|parses| parses.get(path))
        .copied()
        .unwrap_or(0)
}

/// Counts a parse of `path` starting.
pub(crate) fn note(path: &Path) {
    *PARSES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_default()
        .entry(path.to_path_buf())
        .or_default() += 1;
}
