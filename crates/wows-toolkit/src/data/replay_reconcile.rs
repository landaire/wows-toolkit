//! Panic-isolated, two-ledger reconciliation primitives shared by the startup
//! pass and the on-demand "Index all replays" command.

use std::collections::HashSet;
use std::panic::UnwindSafe;
use std::panic::catch_unwind;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome {
    Skipped,
    Indexed,
    Failed,
}

/// Process one replay file. Skips when both ledgers are already satisfied.
/// Otherwise runs `parse_and_index` inside `catch_unwind` so a parser panic on
/// one file cannot abort the pass.
pub fn reconcile_one<F>(path: &Path, indexed: bool, sent: bool, parse_and_index: F) -> FileOutcome
where
    F: FnOnce() -> Result<(), ()> + UnwindSafe,
{
    if indexed && sent {
        return FileOutcome::Skipped;
    }
    match catch_unwind(parse_and_index) {
        Ok(Ok(())) => FileOutcome::Indexed,
        Ok(Err(())) => {
            warn!("failed to index replay {}", path.display());
            FileOutcome::Failed
        }
        Err(_) => {
            warn!("panic while indexing replay {} (skipped)", path.display());
            FileOutcome::Failed
        }
    }
}

/// Persistent set of files that panicked or hard-errored, keyed by path + mtime,
/// so they are not retried every launch. Serialized as JSON in the settings table.
#[derive(Default, Serialize, Deserialize)]
pub struct Unindexable {
    entries: HashSet<(String, i64)>,
}

impl Unindexable {
    fn key(path: &Path) -> Option<(String, i64)> {
        let mtime = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)?;
        Some((path.to_string_lossy().to_string(), mtime))
    }

    pub fn contains(&self, path: &Path) -> bool {
        Self::key(path).map(|k| self.entries.contains(&k)).unwrap_or(false)
    }

    pub fn insert(&mut self, path: &Path) {
        if let Some(k) = Self::key(path) {
            self.entries.insert(k);
        }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries.iter().map(|(p, _)| PathBuf::from(p)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::AssertUnwindSafe;
    use std::path::Path;

    #[test]
    fn skips_when_both_ledgers_satisfied() {
        let mut called = false;
        let out = reconcile_one(
            Path::new("a"),
            true,
            true,
            AssertUnwindSafe(|| {
                called = true;
                Ok(())
            }),
        );
        assert_eq!(out, FileOutcome::Skipped);
        assert!(!called, "must not parse when already indexed and sent");
    }

    #[test]
    fn a_panicking_parse_is_isolated_and_reported_failed() {
        let out = reconcile_one(
            Path::new("b"),
            false,
            false,
            AssertUnwindSafe(|| -> Result<(), ()> {
                panic!("boom");
            }),
        );
        assert_eq!(out, FileOutcome::Failed);
    }

    #[test]
    fn a_good_parse_after_a_bad_one_still_indexes() {
        let bad = reconcile_one(Path::new("b"), false, false, AssertUnwindSafe(|| -> Result<(), ()> { panic!("x") }));
        let good = reconcile_one(Path::new("c"), false, false, AssertUnwindSafe(|| Ok(())));
        assert_eq!(bad, FileOutcome::Failed);
        assert_eq!(good, FileOutcome::Indexed);
    }
}
