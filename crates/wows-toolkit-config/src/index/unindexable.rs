//! The files a parse could not read, remembered so they are not retried every
//! launch.
//!
//! Shared, because the ledger is one row in the settings table and both front
//! ends walk the same directory: a replay the egui app gave up on is one this one
//! gives up on too, and neither should re-read it a hundred times.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use sqlx::sqlite::SqlitePool;

/// Files that panicked or hard-errored, keyed by path and modification time, so
/// a replaced file recovers on its own.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Unindexable {
    entries: HashSet<(String, i64)>,
}

impl Unindexable {
    const SETTING_KEY: &'static str = "replay_unindexable";

    /// The key a file is remembered under: its path and the time it was last
    /// written. A file replaced since is a different key, so it is tried again.
    fn key(path: &Path) -> Option<(String, i64)> {
        let mtime = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_secs() as i64)?;
        Some((path.to_string_lossy().to_string(), mtime))
    }

    pub fn contains(&self, path: &Path) -> bool {
        Self::key(path).is_some_and(|key| self.entries.contains(&key))
    }

    /// Records the file as unreadable. `true` when this is new, so a caller knows
    /// the ledger is worth writing.
    pub fn insert(&mut self, path: &Path) -> bool {
        match Self::key(path) {
            Some(key) => self.entries.insert(key),
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What is stored, or an empty ledger. An unreadable row is an empty ledger
    /// too: a file that is really unreadable will be recorded again.
    pub async fn load(pool: &SqlitePool) -> Self {
        crate::queries::get_setting::<Self>(pool, Self::SETTING_KEY).await.unwrap_or_default()
    }

    pub async fn save(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        crate::queries::set_setting(pool, Self::SETTING_KEY, self).await
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    /// The ledger is keyed by what the file was, not only where it is: a replay
    /// replaced at the same path is read again rather than skipped forever.
    #[tokio::test]
    async fn a_replaced_file_is_no_longer_remembered() {
        let dir = tempfile::tempdir().expect("a temp directory");
        let path = dir.path().join("broken.wowsreplay");
        std::fs::write(&path, b"first").expect("the file is written");

        let mut ledger = Unindexable::default();
        assert!(ledger.insert(&path), "the first record is new");
        assert!(!ledger.insert(&path), "and the second is not");
        assert!(ledger.contains(&path));

        // Written again, an hour later as far as the filesystem is concerned.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        std::fs::write(&path, b"second").expect("the file is rewritten");
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(later))
            .expect("the modification time is set");
        assert!(!ledger.contains(&path), "a replaced file is tried again");
    }

    /// It round-trips through the row both apps read.
    #[tokio::test]
    async fn the_ledger_round_trips() {
        let pool = crate::test_pool().await;
        let dir = tempfile::tempdir().expect("a temp directory");
        let path = dir.path().join("broken.wowsreplay");
        std::fs::write(&path, b"x").expect("the file is written");

        let mut ledger = Unindexable::default();
        ledger.insert(&path);
        ledger.save(&pool).await.expect("the ledger is written");

        let read = Unindexable::load(&pool).await;
        assert!(read.contains(&path));
        assert_eq!(read.len(), 1);
    }
}
