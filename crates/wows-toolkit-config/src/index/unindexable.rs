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

    /// The changes made since the load. A save applies them over what is stored by then, so it
    /// keeps what another writer recorded meanwhile.
    #[serde(skip)]
    inserted: HashSet<(String, i64)>,
    #[serde(skip)]
    removed: HashSet<(String, i64)>,
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
        let Some(key) = Self::key(path) else {
            return false;
        };

        self.removed.remove(&key);
        self.inserted.insert(key.clone());
        self.entries.insert(key)
    }

    /// Forgets the file as unreadable, for one that has since been read.
    /// `true` when it was recorded, so a caller knows the ledger is worth
    /// writing.
    pub fn remove(&mut self, path: &Path) -> bool {
        let Some(key) = Self::key(path) else {
            return false;
        };

        self.inserted.remove(&key);
        self.removed.insert(key.clone());
        self.entries.remove(&key)
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

    /// Writes this ledger's changes since its load or last save over the stored one. Both front
    /// ends, the index passes and sharing each hold their own copy, and a save of the whole copy
    /// would drop what the others recorded. The changes are spent once written, so a later save
    /// does not apply them again over what the others recorded since.
    pub async fn save(&mut self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        // Immediate, so no writer lands between the read and the write.
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let stored: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?1")
            .bind(Self::SETTING_KEY)
            .fetch_optional(&mut *tx)
            .await?;
        // A row that does not read is replaced, as `load` reads it as empty.
        let mut entries =
            stored.and_then(|(json,)| serde_json::from_str::<Self>(&json).ok()).unwrap_or_default().entries;
        entries.retain(|key| !self.removed.contains(key));
        entries.extend(self.inserted.iter().cloned());

        let merged = Self { entries, ..Self::default() };
        let json = serde_json::to_string(&merged).map_err(|err| sqlx::Error::Protocol(err.to_string()))?;
        sqlx::query("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2")
            .bind(Self::SETTING_KEY)
            .bind(json)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        self.inserted.clear();
        self.removed.clear();
        Ok(())
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

    /// A file read since it was recorded is forgotten, so later passes read
    /// it rather than skip it as unreadable.
    #[test]
    fn a_file_that_has_since_been_read_is_forgotten() {
        let dir = tempfile::tempdir().expect("a temp directory");
        let path = dir.path().join("fixed.wowsreplay");
        std::fs::write(&path, b"x").expect("the file is written");
        let mut ledger = Unindexable::default();
        ledger.insert(&path);

        assert!(ledger.remove(&path), "a recorded file is removed");
        assert!(!ledger.remove(&path), "and only once");
        assert!(!ledger.contains(&path));
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

    /// Two copies loaded together and saved one after the other each keep what the other wrote.
    #[tokio::test]
    async fn a_save_keeps_what_another_copy_recorded() {
        let pool = crate::test_pool().await;
        let dir = tempfile::tempdir().expect("a temp directory");
        let [kept, fixed, found] = ["kept", "fixed", "found"].map(|name| {
            let path = dir.path().join(format!("{name}.wowsreplay"));
            std::fs::write(&path, b"x").expect("the file is written");
            path
        });
        let mut first = Unindexable::default();
        first.insert(&kept);
        first.insert(&fixed);
        first.save(&pool).await.expect("the ledger is written");

        let mut indexer = Unindexable::load(&pool).await;
        let mut sharing = Unindexable::load(&pool).await;
        indexer.remove(&fixed);
        indexer.save(&pool).await.expect("the indexer's copy is written");
        sharing.insert(&found);
        sharing.save(&pool).await.expect("the sharing copy is written");

        let read = Unindexable::load(&pool).await;
        assert!(read.contains(&kept));
        assert!(read.contains(&found), "the second save keeps its own record");
        assert!(!read.contains(&fixed), "and does not put back what the first one forgot");
    }

    /// A copy saved again after another copy changed the ledger does not undo that change.
    #[tokio::test]
    async fn a_second_save_does_not_apply_spent_changes() {
        let pool = crate::test_pool().await;
        let dir = tempfile::tempdir().expect("a temp directory");
        let [fixed, found] = ["fixed", "found"].map(|name| {
            let path = dir.path().join(format!("{name}.wowsreplay"));
            std::fs::write(&path, b"x").expect("the file is written");
            path
        });
        let mut egui = Unindexable::default();
        egui.insert(&fixed);
        egui.save(&pool).await.expect("the egui copy is written");

        let mut zgui = Unindexable::load(&pool).await;
        zgui.remove(&fixed);
        zgui.save(&pool).await.expect("the zgui copy is written");

        egui.insert(&found);
        egui.save(&pool).await.expect("the egui copy is written again");

        let read = Unindexable::load(&pool).await;
        assert!(read.contains(&found), "the new record is kept");
        assert!(!read.contains(&fixed), "and the record the other copy forgot stays forgotten");
    }
}
