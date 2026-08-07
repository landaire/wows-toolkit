//! Panic-isolated, two-ledger reconciliation primitives shared by the startup
//! pass and the on-demand "Index all replays" command.

use std::collections::HashSet;
use std::panic::UnwindSafe;
use std::panic::catch_unwind;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use tracing::warn;

use crate::data::settings::DataSharingMode;

/// Result of a single background parse attempt, distinguishing genuinely
/// un-processable files from retryable conditions so the caller can blacklist
/// only the former.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    /// Parsed successfully and the upload completed.
    ParsedAndSent,
    /// Parsed successfully, but replay contents make it permanently ineligible.
    ParsedAndStableSkipped { identity: Option<ReplayFileIdentity> },
    /// Parsed successfully while sharing was disabled.
    ParsedAndDeferred,
    /// Parsed successfully (and indexed) but the upload hit a transient error.
    /// Left unsent so the upload is retried next launch.
    ParsedNotSent,
    /// A retryable non-parse condition: no game data for this build yet.
    /// Left unsent and unindexed; retried next launch.
    Transient,
    /// The replay is malformed / unparseable after the retries. Blacklist it.
    HardFailure,
}

/// Upload disposition retained after a successful replay parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedUploadDisposition {
    Sent,
    StableSkipped { identity: Option<ReplayFileIdentity> },
    Deferred,
    Retryable,
}

/// Whether upload work is still needed before startup reconciliation can skip
/// an already-indexed replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadReconciliation {
    Pending,
    Satisfied,
}

pub fn startup_upload_reconciliation(
    mode: DataSharingMode,
    sent: bool,
    stable_ineligible: bool,
) -> UploadReconciliation {
    if sent || stable_ineligible || !mode.shares_anything() {
        UploadReconciliation::Satisfied
    } else {
        UploadReconciliation::Pending
    }
}

/// Reconciliation decision for one replay file, consumed by the startup scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOutcome {
    /// Both ledgers already satisfied; the parse closure was not run.
    Skipped,
    /// Parsed successfully with the upload disposition preserved.
    Parsed { upload: ParsedUploadDisposition },
    /// A retryable condition; leave the file for a later launch, do not blacklist.
    Transient,
    /// A hard parse failure or a panic; record in the persistent blacklist.
    HardFailure,
}

/// Process one replay file. Skips when both ledgers are already satisfied.
/// Otherwise runs `parse_and_index` inside `catch_unwind` so a parser panic on
/// one file cannot abort the pass. A panic is mapped to [`FileOutcome::HardFailure`]
/// exactly like a hard parse failure.
pub fn reconcile_one<F>(path: &Path, indexed: bool, upload: UploadReconciliation, parse_and_index: F) -> FileOutcome
where
    F: FnOnce() -> ParseOutcome + UnwindSafe,
{
    if indexed && upload == UploadReconciliation::Satisfied {
        return FileOutcome::Skipped;
    }
    match catch_unwind(parse_and_index) {
        Ok(ParseOutcome::ParsedAndSent) => FileOutcome::Parsed { upload: ParsedUploadDisposition::Sent },
        Ok(ParseOutcome::ParsedAndStableSkipped { identity }) => {
            FileOutcome::Parsed { upload: ParsedUploadDisposition::StableSkipped { identity } }
        }
        Ok(ParseOutcome::ParsedAndDeferred) => FileOutcome::Parsed { upload: ParsedUploadDisposition::Deferred },
        Ok(ParseOutcome::ParsedNotSent) => FileOutcome::Parsed { upload: ParsedUploadDisposition::Retryable },
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct ReplayContentDigest([u8; 32]);

impl ReplayContentDigest {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self(sha2::Sha256::digest(bytes).into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReplayFileIdentity {
    path: String,
    content_digest: ReplayContentDigest,
}

impl ReplayFileIdentity {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        Some(Self::from_bytes(path, &bytes))
    }

    pub(crate) fn from_bytes(path: &Path, bytes: &[u8]) -> Self {
        Self { path: path.to_string_lossy().into_owned(), content_digest: ReplayContentDigest::from_bytes(bytes) }
    }
}

/// Persisted identities whose replay contents make ShipBuilds upload
/// ineligible regardless of the user's sharing mode.
#[derive(Default, Serialize, Deserialize)]
pub struct StableUploadSkips {
    entries: HashSet<ReplayFileIdentity>,
}

impl StableUploadSkips {
    const SETTING_KEY: &'static str = "replay_stable_upload_skips";

    pub fn contains(&self, path: &Path) -> bool {
        let path_text = path.to_string_lossy();
        if !self.entries.iter().any(|identity| identity.path == path_text) {
            return false;
        }
        ReplayFileIdentity::from_path(path).is_some_and(|identity| self.entries.contains(&identity))
    }

    pub(crate) fn insert(&mut self, identity: ReplayFileIdentity) -> bool {
        if self.entries.contains(&identity) {
            return false;
        }
        self.entries.retain(|existing| existing.path != identity.path);
        self.entries.insert(identity)
    }

    pub async fn load(pool: &sqlx::SqlitePool) -> Self {
        // Missing state is expected before the first stable skip. Malformed
        // state is already logged and safely causes the replay to be retried.
        crate::db::queries::get_setting(pool, Self::SETTING_KEY).await.unwrap_or_default()
    }

    pub async fn save(&self, pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
        crate::db::queries::set_setting(pool, Self::SETTING_KEY, self).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupFileOutcome {
    pub file: FileOutcome,
    pub stable_skip_changed: bool,
}

pub fn reconcile_startup_one<F>(
    path: &Path,
    indexed: bool,
    sent: bool,
    mode: DataSharingMode,
    stable_upload_skips: &mut StableUploadSkips,
    parse_and_index: F,
) -> StartupFileOutcome
where
    F: FnOnce() -> ParseOutcome + UnwindSafe,
{
    let upload = startup_upload_reconciliation(mode, sent, stable_upload_skips.contains(path));
    let file = reconcile_one(path, indexed, upload, parse_and_index);
    let stable_skip_changed = match &file {
        FileOutcome::Parsed { upload: ParsedUploadDisposition::StableSkipped { identity: Some(identity) } } => {
            stable_upload_skips.insert(identity.clone())
        }
        _ => false,
    };
    StartupFileOutcome { file, stable_skip_changed }
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
    use crate::data::settings::DataSharingMode;
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
            UploadReconciliation::Satisfied,
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
        let out = crate::test_utils::with_silenced_panic_hook(|| {
            reconcile_one(
                Path::new("b"),
                false,
                UploadReconciliation::Pending,
                AssertUnwindSafe(|| -> ParseOutcome { panic!("boom") }),
            )
        });
        assert_eq!(out, FileOutcome::HardFailure);
    }

    #[test]
    fn a_transient_condition_is_not_a_hard_failure() {
        let out = reconcile_one(
            Path::new("t"),
            false,
            UploadReconciliation::Pending,
            AssertUnwindSafe(|| ParseOutcome::Transient),
        );
        assert_eq!(out, FileOutcome::Transient);
    }

    #[test]
    fn a_parsed_but_unsent_replay_is_not_blacklisted() {
        let out = reconcile_one(
            Path::new("p"),
            false,
            UploadReconciliation::Pending,
            AssertUnwindSafe(|| ParseOutcome::ParsedNotSent),
        );
        assert_eq!(out, FileOutcome::Parsed { upload: ParsedUploadDisposition::Retryable });
    }

    #[test]
    fn a_parsed_replay_with_a_stable_skip_is_not_marked_sent() {
        let out = reconcile_one(
            Path::new("p"),
            false,
            UploadReconciliation::Pending,
            AssertUnwindSafe(|| ParseOutcome::ParsedAndStableSkipped { identity: None }),
        );
        assert_eq!(out, FileOutcome::Parsed { upload: ParsedUploadDisposition::StableSkipped { identity: None } });
    }

    #[test]
    fn an_indexed_replay_is_satisfied_while_sharing_is_off_without_being_sent() {
        let sent = false;
        let mut stable_upload_skips = StableUploadSkips::default();
        let mut parsed = false;

        let out = reconcile_startup_one(
            Path::new("indexed.wowsreplay"),
            true,
            sent,
            DataSharingMode::Off,
            &mut stable_upload_skips,
            AssertUnwindSafe(|| {
                parsed = true;
                ParseOutcome::ParsedAndSent
            }),
        );

        assert_eq!(out.file, FileOutcome::Skipped);
        assert!(!out.stable_skip_changed);
        assert!(!parsed);
        assert!(!sent, "reconciliation satisfaction must not change upload history");
    }

    #[test]
    fn a_stably_ineligible_replay_is_not_reparsed_on_the_next_launch() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let pool = runtime
            .block_on(sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:"))
            .unwrap();
        runtime
            .block_on(
                sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL)")
                    .execute(&pool),
            )
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ineligible.wowsreplay");
        std::fs::write(&path, b"stable identity").unwrap();
        let parsed_identity = ReplayFileIdentity::from_path(&path);
        let sent_replays = HashSet::<String>::new();

        let mut first_launch = StableUploadSkips::default();
        let first = reconcile_startup_one(
            &path,
            false,
            false,
            DataSharingMode::BuildData,
            &mut first_launch,
            AssertUnwindSafe(|| ParseOutcome::ParsedAndStableSkipped { identity: parsed_identity.clone() }),
        );
        assert_eq!(
            first.file,
            FileOutcome::Parsed { upload: ParsedUploadDisposition::StableSkipped { identity: parsed_identity } }
        );
        assert!(first.stable_skip_changed);

        runtime.block_on(first_launch.save(&pool)).unwrap();
        drop(first_launch);

        let second_launch = runtime.block_on(StableUploadSkips::load(&pool));
        let mut second_launch = second_launch;
        let mut reparsed = false;
        let second = reconcile_startup_one(
            &path,
            true,
            sent_replays.contains(path.to_string_lossy().as_ref()),
            DataSharingMode::BuildData,
            &mut second_launch,
            AssertUnwindSafe(|| {
                reparsed = true;
                ParseOutcome::ParsedAndSent
            }),
        );

        assert_eq!(second.file, FileOutcome::Skipped);
        assert!(!second.stable_skip_changed);
        assert!(!reparsed);
        assert!(sent_replays.is_empty(), "stable ineligibility is not upload success");
    }

    #[test]
    fn a_stable_skip_does_not_suppress_a_replacement_that_was_not_parsed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("replaced.wowsreplay");
        std::fs::write(&path, b"parsed snapshot").unwrap();
        let parsed_identity = ReplayFileIdentity::from_path(&path).unwrap();
        let parsed_modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(&path).unwrap();
        file.write_all(b"latest snapshot").unwrap();
        file.set_modified(parsed_modified).unwrap();
        drop(file);

        let mut stable_upload_skips = StableUploadSkips::default();
        assert!(stable_upload_skips.insert(parsed_identity));
        let mut parsed_replacement = false;
        let outcome = reconcile_startup_one(
            &path,
            true,
            false,
            DataSharingMode::BuildData,
            &mut stable_upload_skips,
            AssertUnwindSafe(|| {
                parsed_replacement = true;
                ParseOutcome::ParsedAndSent
            }),
        );

        assert_eq!(outcome.file, FileOutcome::Parsed { upload: ParsedUploadDisposition::Sent });
        assert!(!outcome.stable_skip_changed);
        assert!(parsed_replacement);
        assert!(!stable_upload_skips.contains(&path), "replacement bytes must invalidate the stable skip");
    }

    #[test]
    fn a_good_parse_after_a_bad_one_still_parses() {
        let bad = crate::test_utils::with_silenced_panic_hook(|| {
            reconcile_one(
                Path::new("b"),
                false,
                UploadReconciliation::Pending,
                AssertUnwindSafe(|| -> ParseOutcome { panic!("x") }),
            )
        });
        let good = reconcile_one(
            Path::new("c"),
            false,
            UploadReconciliation::Pending,
            AssertUnwindSafe(|| ParseOutcome::ParsedAndSent),
        );
        assert_eq!(bad, FileOutcome::HardFailure);
        assert_eq!(good, FileOutcome::Parsed { upload: ParsedUploadDisposition::Sent });
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
