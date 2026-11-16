//! Replays whose contents mean ShipBuilds will never take them, remembered so a
//! startup sweep does not read them again every launch.
//!
//! Shared, because the ledger is one row in the settings table and both front
//! ends sweep the same directory.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sqlx::sqlite::SqlitePool;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct ReplayContentDigest([u8; 32]);

impl ReplayContentDigest {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self(sha2::Sha256::digest(bytes).into())
    }
}

/// A replay file as it was when read: its path and a digest of its bytes, so a
/// file rewritten since is a different identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReplayFileIdentity {
    path: String,
    content_digest: ReplayContentDigest,
}

impl ReplayFileIdentity {
    /// `None` when the file cannot be read.
    pub fn from_path(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        Some(Self::from_bytes(path, &bytes))
    }

    pub fn from_bytes(path: &Path, bytes: &[u8]) -> Self {
        Self { path: path.to_string_lossy().into_owned(), content_digest: ReplayContentDigest::from_bytes(bytes) }
    }
}

/// Identities whose replay contents make a ShipBuilds upload ineligible
/// whatever the sharing mode.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StableUploadSkips {
    entries: HashSet<ReplayFileIdentity>,
}

impl StableUploadSkips {
    const SETTING_KEY: &'static str = "replay_stable_upload_skips";

    /// Whether the file at `path`, as it is now, was recorded. The file is only
    /// read when its path was recorded.
    pub fn contains(&self, path: &Path) -> bool {
        let path_text = path.to_string_lossy();
        if !self.entries.iter().any(|identity| identity.path == path_text) {
            return false;
        }
        ReplayFileIdentity::from_path(path).is_some_and(|identity| self.entries.contains(&identity))
    }

    /// Records the identity, replacing any older one for the same path. `true`
    /// when this is new, so a caller knows the ledger is worth writing.
    pub fn insert(&mut self, identity: ReplayFileIdentity) -> bool {
        if self.entries.contains(&identity) {
            return false;
        }
        self.entries.retain(|existing| existing.path != identity.path);
        self.entries.insert(identity)
    }

    /// What is stored, or an empty ledger. A missing row is expected before the
    /// first skip, and an unreadable one only costs a re-read of each replay.
    pub async fn load(pool: &SqlitePool) -> Self {
        crate::queries::get_setting::<Self>(pool, Self::SETTING_KEY).await.unwrap_or_default()
    }

    pub async fn save(&self, pool: &SqlitePool) -> Result<(), sqlx::Error> {
        crate::queries::set_setting(pool, Self::SETTING_KEY, self).await
    }
}
