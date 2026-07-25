//! Panic-isolated, two-ledger reconciliation primitives shared by the startup
//! pass and the on-demand "Index all replays" command.

use std::collections::HashSet;
use std::panic::UnwindSafe;
use std::panic::catch_unwind;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use tracing::warn;

/// Result of a single background parse attempt, distinguishing genuinely
/// un-processable files from retryable conditions so the caller can blacklist
/// only the former.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseOutcome {
    /// Parsed successfully and the upload completed (or was not required).
    ParsedAndSent,
    /// Parsed successfully (and indexed) but the upload hit a transient error.
    /// Left unsent so the upload is retried next launch.
    ParsedNotSent,
    /// A retryable non-parse condition: no game data for this build yet.
    /// Left unsent and unindexed; retried next launch.
    Transient,
    /// The replay is malformed / unparseable after the retries. Blacklist it.
    HardFailure,
}

/// Reconciliation decision for one replay file, consumed by the startup scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome {
    /// Both ledgers already satisfied; the parse closure was not run.
    Skipped,
    /// Parsed successfully. `sent` is true when the upload also completed.
    Parsed { sent: bool },
    /// A retryable condition; leave the file for a later launch, do not blacklist.
    Transient,
    /// A hard parse failure or a panic; record in the persistent blacklist.
    HardFailure,
}

/// Process one replay file. Skips when both ledgers are already satisfied.
/// Otherwise runs `parse_and_index` inside `catch_unwind` so a parser panic on
/// one file cannot abort the pass. A panic is mapped to [`FileOutcome::HardFailure`]
/// exactly like a hard parse failure.
pub fn reconcile_one<F>(path: &Path, indexed: bool, sent: bool, parse_and_index: F) -> FileOutcome
where
    F: FnOnce() -> ParseOutcome + UnwindSafe,
{
    if indexed && sent {
        return FileOutcome::Skipped;
    }
    match catch_unwind(parse_and_index) {
        Ok(ParseOutcome::ParsedAndSent) => FileOutcome::Parsed { sent: true },
        Ok(ParseOutcome::ParsedNotSent) => FileOutcome::Parsed { sent: false },
        Ok(ParseOutcome::Transient) => FileOutcome::Transient,
        Ok(ParseOutcome::HardFailure) => {
            warn!("failed to parse replay {} (blacklisted)", path.display());
            FileOutcome::HardFailure
        }
        Err(_) => {
            warn!("panic while parsing replay {} (blacklisted)", path.display());
            FileOutcome::HardFailure
        }
    }
}

/// Persistent set of files that panicked or hard-errored, keyed by path + mtime,
/// so they are not retried every launch. A replaced file (new mtime) recovers.
/// Serialized as JSON in the settings table under `replay_unindexable`.
#[derive(Default, Serialize, Deserialize)]
pub struct Unindexable {
    entries: HashSet<(String, i64)>,
}

impl Unindexable {
    const SETTING_KEY: &'static str = "replay_unindexable";

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

    /// Record the file as un-processable. Returns true when this is a new entry
    /// (so the caller knows the set is dirty and needs persisting).
    pub fn insert(&mut self, path: &Path) -> bool {
        match Self::key(path) {
            Some(k) => self.entries.insert(k),
            None => false,
        }
    }

    /// Load the persisted blacklist from the settings table, or an empty set.
    pub async fn load(pool: &sqlx::SqlitePool) -> Self {
        crate::db::queries::get_setting::<Self>(pool, Self::SETTING_KEY).await.unwrap_or_default()
    }

    /// Persist the blacklist to the settings table.
    pub async fn save(&self, pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
        crate::db::queries::set_setting(pool, Self::SETTING_KEY, self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::panic::AssertUnwindSafe;
    use std::path::Path;
    use std::time::Duration;
    use std::time::SystemTime;

    #[test]
    fn skips_when_both_ledgers_satisfied() {
        let mut called = false;
        let out = reconcile_one(
            Path::new("a"),
            true,
            true,
            AssertUnwindSafe(|| {
                called = true;
                ParseOutcome::ParsedAndSent
            }),
        );
        assert_eq!(out, FileOutcome::Skipped);
        assert!(!called, "must not parse when already indexed and sent");
    }

    #[test]
    fn a_panicking_parse_is_isolated_and_reported_as_hard_failure() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out = reconcile_one(Path::new("b"), false, false, AssertUnwindSafe(|| -> ParseOutcome { panic!("boom") }));
        std::panic::set_hook(prev);
        assert_eq!(out, FileOutcome::HardFailure);
    }

    #[test]
    fn a_transient_condition_is_not_a_hard_failure() {
        let out = reconcile_one(Path::new("t"), false, false, AssertUnwindSafe(|| ParseOutcome::Transient));
        assert_eq!(out, FileOutcome::Transient);
    }

    #[test]
    fn a_parsed_but_unsent_replay_is_not_blacklisted() {
        let out = reconcile_one(Path::new("p"), false, false, AssertUnwindSafe(|| ParseOutcome::ParsedNotSent));
        assert_eq!(out, FileOutcome::Parsed { sent: false });
    }

    #[test]
    fn a_good_parse_after_a_bad_one_still_parses() {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let bad = reconcile_one(Path::new("b"), false, false, AssertUnwindSafe(|| -> ParseOutcome { panic!("x") }));
        std::panic::set_hook(prev);
        let good = reconcile_one(Path::new("c"), false, false, AssertUnwindSafe(|| ParseOutcome::ParsedAndSent));
        assert_eq!(bad, FileOutcome::HardFailure);
        assert_eq!(good, FileOutcome::Parsed { sent: true });
    }

    #[test]
    fn unindexable_contains_insert_roundtrip_is_mtime_keyed() {
        let path = std::env::temp_dir().join(format!("wt_unindexable_test_{}.tmp", std::process::id()));
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"x").unwrap();
        }

        let mut u = Unindexable::default();
        assert!(!u.contains(&path), "fresh set contains nothing");
        assert!(u.insert(&path), "first insert reports a new entry");
        assert!(!u.insert(&path), "second insert of the same key is not new");
        assert!(u.contains(&path), "just-inserted path must be contained");

        // A replaced file (newer mtime) must not match the stored key.
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(10)).unwrap();
        drop(f);
        assert!(!u.contains(&path), "mtime change must invalidate the blacklist entry");

        // A missing file has no key and is never contained.
        let _ = std::fs::remove_file(&path);
        assert!(!u.contains(&path), "missing file is never contained");
    }
}
