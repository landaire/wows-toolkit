//! Contributing one replay as the data-sharing setting asks, for each path that does: a battle
//! that lands, one held for its results, the startup sweep and the palette's bulk pass.

use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::index::unindexable::Unindexable;
use wows_toolkit_config::upload_skips::ReplayFileIdentity;
use wows_toolkit_config::upload_skips::StableUploadSkips;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::parse_replay;
use wows_toolkit_replay_data::replay_folder::ReplayFolder;
use wows_toolkit_services::upload;
use wows_toolkit_services::upload::Candidate;
use wows_toolkit_services::upload::LedgerUse;
use wows_toolkit_services::upload::Outcome;
use wows_toolkit_services::upload::Sender;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use zgui::reactive::LocalStorage;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;

use crate::long_job::long_job;
use crate::services::Services;
use crate::services::TokioError;
use crate::tabs::replays::browser::proxy_url;

/// How many times the grace anchor is read before the replay is left for a later pass.
const ANCHOR_ATTEMPTS: u32 = 3;
/// The wait between reads of the grace anchor.
const ANCHOR_RETRY: Duration = Duration::from_secs(5);

/// Why a replay was not put to the service.
#[derive(Debug, thiserror::Error)]
pub enum ContributeError {
    #[error("the replay did not parse: {0}")]
    Unparsed(#[source] ReplayLoadError),
    /// Without the anchor the grace window cannot be measured, and sending early could send a
    /// battle still in progress.
    #[error("the first-seen anchor could not be read: {0}")]
    NoAnchor(#[source] AnchorError),
    #[error("the send did not complete: {0}")]
    Tokio(#[source] TokioError),
}

#[derive(Debug, thiserror::Error)]
pub enum AnchorError {
    #[error(transparent)]
    Store(#[from] sqlx::Error),
    #[error(transparent)]
    Tokio(#[from] TokioError),
}

/// What a contribution needs: the setting, the store with the ledger, the game data to read
/// replays with and the client settings.
#[derive(Clone)]
pub struct Contributor {
    services: Services,
    pool: SqlitePool,
    game_data: GameDataCache,
    proxy: String,
}

impl Contributor {
    /// `None` while sharing is off, or the settings, store or game data are not ready.
    pub fn new(services: Services) -> Option<Self> {
        let contributor = Self {
            services,
            pool: services.settings.pool()?,
            game_data: services.game_data.cache_untracked()?,
            proxy: proxy_url(services),
        };
        contributor.mode()?;
        Some(contributor)
    }

    /// What the setting shares now, `None` while it shares nothing. Read again before each send,
    /// so a reader who turns sharing off stops a sweep already running.
    pub fn mode(&self) -> Option<DataSharingMode> {
        self.services
            .settings
            .read_untracked(|settings| settings.data_sharing)
            .filter(|mode| *mode != DataSharingMode::Off)
    }

    /// Reads `path` and sends what the setting asks for, if anything.
    pub async fn contribute(&self, path: PathBuf, ledger: LedgerUse) -> Result<Outcome, ContributeError> {
        // The ledger and the anchor may name the battle by another of its paths: another spelling
        // of the install, or the copy the game replaced when it rewrote the file under an overlay.
        let other_paths = self.other_paths(&path).await;
        if ledger == LedgerUse::Consult && self.ledger_has(other_paths.clone()).await {
            return Ok(Outcome::AlreadySent);
        }
        let (reading, cache) = (path.clone(), self.game_data.clone());
        let parsed = long_job(move || parse_replay(&reading, &cache, None)).await.map_err(ContributeError::Unparsed)?;

        // The anchor both apps keep, recorded on the first sight of this replay: the grace window
        // for its results runs from there.
        let first_seen = self.first_seen(&path, &other_paths).await.map_err(ContributeError::NoAnchor)?;
        let Some(mode) = self.mode() else { return Ok(Outcome::Skipped) };
        let candidate = Candidate { path, shareable: parsed.shareable, first_seen };
        let (pool, proxy) = (self.pool.clone(), self.proxy.clone());
        self.services
            .on_tokio(async move {
                let sender = Sender { pool: &pool, agent: crate::http::USER_AGENT, proxy: &proxy };
                upload::send(candidate, mode, ledger, sender).await
            })
            .await
            .map_err(ContributeError::Tokio)
    }

    /// Whether the ledger already has `path`, or the battle under another of its paths in the
    /// install. A ledger that cannot be read answers `false`, and the send asks again.
    pub async fn already_contributed(&self, path: &Path) -> bool {
        self.ledger_has(vec![path.to_path_buf()]).await || self.ledger_has(self.other_paths(path).await).await
    }

    /// See [`ReplayFolder::other_paths`]. Empty for a replay outside the install.
    async fn other_paths(&self, path: &Path) -> Vec<PathBuf> {
        let Some(wows_dir) = self.services.settings.read_untracked(|settings| settings.wows_dir.clone()) else {
            return Vec::new();
        };
        if wows_dir.is_empty() {
            return Vec::new();
        }
        let named = path.to_path_buf();

        long_job(move || ReplayFolder::from_app_install(Path::new(&wows_dir)).other_paths(&named)).await
    }

    async fn ledger_has(&self, paths: Vec<PathBuf>) -> bool {
        if paths.is_empty() {
            return false;
        }
        let pool = self.pool.clone();
        self.services
            .on_tokio(async move {
                for path in &paths {
                    if upload::already_contributed(&pool, path).await {
                        return true;
                    }
                }
                false
            })
            .await
            .unwrap_or(false)
    }

    /// Records that `paths`, as they are now, will never be taken, so the startup sweep does not
    /// read them again. The ledger is written once for all of them.
    pub async fn remember_ineligible(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let identities =
            long_job(move || paths.iter().filter_map(|path| ReplayFileIdentity::from_path(path)).collect::<Vec<_>>())
                .await;
        let pool = self.pool.clone();
        let saved = self
            .services
            .on_tokio(async move {
                let mut skips = StableUploadSkips::load(&pool).await;
                let mut grew = false;
                for identity in identities {
                    grew |= skips.insert(identity);
                }
                if grew { skips.save(&pool).await } else { Ok(()) }
            })
            .await;
        match saved {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(%err, "sharing: the ineligible replay was not recorded"),
            Err(err) => tracing::warn!(%err, "sharing: the ineligible replay was not recorded"),
        }
    }

    /// The replays in `folder` the startup sweep reads: every battle the game wrote that is neither
    /// in the ledger, nor unreadable, nor known never to be taken.
    pub async fn unsent_in(&self, folder: ReplayFolder) -> Vec<PathBuf> {
        let pool = self.pool.clone();
        let ledgers = self
            .services
            .on_tokio(async move {
                let sent = wows_toolkit_config::queries::get_all_sent_replays(&pool).await?;
                Ok::<_, sqlx::Error>((sent, Unindexable::load(&pool).await, StableUploadSkips::load(&pool).await))
            })
            .await;
        let (sent, unindexable, skips) = match ledgers {
            Ok(Ok(ledgers)) => ledgers,
            // A sweep that cannot tell what was sent would send it all again.
            Ok(Err(err)) => {
                tracing::warn!(%err, "sharing: the sent-replay ledger could not be read, so nothing is swept");
                return Vec::new();
            }
            Err(err) => {
                tracing::warn!(%err, "sharing: the sent-replay ledger could not be read, so nothing is swept");
                return Vec::new();
            }
        };
        let sent: HashSet<String> = sent.into_iter().collect();
        long_job(move || unsent_of(&folder, &sent, &unindexable, &skips)).await
    }

    async fn first_seen(&self, path: &Path, other_paths: &[PathBuf]) -> Result<jiff::Timestamp, AnchorError> {
        let mut attempt = 1;
        loop {
            let (pool, named, others) = (self.pool.clone(), path.to_path_buf(), other_paths.to_vec());
            let read = self.services.on_tokio(async move { upload::first_seen(&pool, &named, &others).await }).await;
            let err = match read {
                Ok(Ok(first_seen)) => return Ok(first_seen),
                Ok(Err(err)) => AnchorError::Store(err),
                Err(err) => AnchorError::Tokio(err),
            };
            if attempt == ANCHOR_ATTEMPTS {
                return Err(err);
            }
            tracing::debug!(path = %path.display(), %err, attempt, "sharing: the first-seen anchor was not read");
            attempt += 1;
            // A runtime that is not there will not be there after the wait either.
            if self.services.on_tokio(tokio::time::sleep(ANCHOR_RETRY)).await.is_err() {
                return Err(err);
            }
        }
    }
}

/// The replays being read for sharing now, by any of the paths that contribute. A second read of
/// one waits for the first, so two cannot both find the ledger empty and send it twice. Keyed by
/// file name: under an install overlay one battle has a path in each layer.
#[derive(Clone, Copy)]
pub struct SharingReads {
    reading: StoredValue<HashMap<OsString, Reading>, LocalStorage>,
    /// Reads a replay again that was asked for while another read held it.
    again: StoredValue<Option<ReadAgain>, LocalStorage>,
}

type ReadAgain = Rc<dyn Fn(PathBuf)>;

/// Whether the replay was asked for again while it was being read, and at which path.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Reading {
    Once,
    Again(PathBuf),
}

impl SharingReads {
    pub fn new() -> Self {
        Self { reading: StoredValue::new_local(HashMap::new()), again: StoredValue::new_local(None) }
    }

    /// Sets what reads a replay again once the read that held it ends.
    pub fn on_again(self, read: impl Fn(PathBuf) + 'static) {
        self.again.set_value(Some(Rc::new(read)));
    }

    /// Marks `path` as being read. `false` when it, or its copy in another overlay layer, already
    /// is, which asks for it to be read again once that read ends, since the file may have
    /// changed after it was read.
    pub fn begin(self, path: &Path) -> bool {
        let key = reading_key(path);
        self.reading
            .try_update_value(|reading| match reading.get_mut(&key) {
                Some(state) => {
                    *state = Reading::Again(path.to_path_buf());
                    false
                }
                None => {
                    reading.insert(key, Reading::Once);
                    true
                }
            })
            .unwrap_or(false)
    }

    /// Ends the read of `path`, and reads it again if that was asked for meanwhile.
    pub fn finish(self, path: &Path) {
        let key = reading_key(path);
        let state = self.reading.try_update_value(|reading| reading.remove(&key)).flatten();
        let Some(Reading::Again(asked)) = state else { return };
        // Taken out first: the read starts with `begin`, which updates `reading`.
        if let Some(read) = self.again.try_get_value().flatten() {
            read(asked);
        }
    }
}

fn reading_key(path: &Path) -> OsString {
    path.file_name().map_or_else(|| path.as_os_str().to_owned(), ToOwned::to_owned)
}

impl Default for SharingReads {
    fn default() -> Self {
        Self::new()
    }
}

/// The `battles` that are neither in the ledger, nor unreadable, nor known never to be taken.
pub fn unsent(
    battles: Vec<PathBuf>,
    sent: &HashSet<String>,
    unindexable: &Unindexable,
    skips: &StableUploadSkips,
) -> Vec<PathBuf> {
    battles
        .into_iter()
        .filter(|path| {
            !sent.contains(path.to_string_lossy().as_ref()) && !unindexable.contains(path) && !skips.contains(path)
        })
        .collect()
}

/// The battles of `folder` that [`unsent`] keeps and that were not sent under another of their
/// paths either: another spelling of the install, or the copy a battle replaced under an overlay.
pub fn unsent_of(
    folder: &ReplayFolder,
    sent: &HashSet<String>,
    unindexable: &Unindexable,
    skips: &StableUploadSkips,
) -> Vec<PathBuf> {
    unsent(battles_of(folder), sent, unindexable, skips)
        .into_iter()
        .filter(|path| !folder.other_paths(path).iter().any(|other| sent.contains(other.to_string_lossy().as_ref())))
        .collect()
}

/// The finished battles directly in `dir`. The live battle's file is rewritten all match long and
/// is never one.
pub fn battles_in(dir: &Path) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(dir = %dir.display(), %err, "sharing: the replays directory could not be read");
            return Vec::new();
        }
    };
    let mut battles: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "wowsreplay")
                && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
        })
        .collect();
    battles.sort();
    battles
}

/// The finished battles of `folder`, at the real paths they are read from. A plain directory is
/// read as [`battles_in`] reads it.
pub fn battles_of(folder: &ReplayFolder) -> Vec<PathBuf> {
    if !folder.is_overlaid() {
        return battles_in(folder.path());
    }

    let mut battles = folder.replay_files();
    battles.sort();

    battles
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory holding a file per name, each with its name as its contents.
    fn dir_with(names: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        let root = dir.path().to_path_buf();
        for name in names {
            std::fs::write(root.join(name), name.as_bytes()).expect("the file is written");
        }
        (dir, root)
    }

    #[test]
    fn only_finished_battles_directly_in_the_directory_are_swept() {
        let (_dir, root) = dir_with(&["b.wowsreplay", "a.wowsreplay", "temp.wowsreplay", "notes.txt"]);
        std::fs::create_dir(root.join("nested")).expect("the directory is made");
        std::fs::write(root.join("nested").join("c.wowsreplay"), b"c").expect("the file is written");

        assert_eq!(battles_in(&root), vec![root.join("a.wowsreplay"), root.join("b.wowsreplay")]);
    }

    #[test]
    fn the_sweep_skips_what_was_sent_what_is_unreadable_and_what_will_never_be_taken() {
        let (_dir, root) =
            dir_with(&["sent.wowsreplay", "unreadable.wowsreplay", "ineligible.wowsreplay", "unsent.wowsreplay"]);
        let sent = HashSet::from([root.join("sent.wowsreplay").to_string_lossy().into_owned()]);
        let mut unindexable = Unindexable::default();
        unindexable.insert(&root.join("unreadable.wowsreplay"));
        let mut skips = StableUploadSkips::default();
        let ineligible = root.join("ineligible.wowsreplay");
        skips.insert(ReplayFileIdentity::from_path(&ineligible).expect("the file is read"));

        assert_eq!(unsent(battles_in(&root), &sent, &unindexable, &skips), vec![root.join("unsent.wowsreplay")]);
    }

    #[test]
    fn an_ineligible_replay_written_again_is_swept_again() {
        let (_dir, root) = dir_with(&["rewritten.wowsreplay"]);
        let rewritten = root.join("rewritten.wowsreplay");
        let mut skips = StableUploadSkips::default();
        skips.insert(ReplayFileIdentity::from_path(&rewritten).expect("the file is read"));
        std::fs::write(&rewritten, b"the battle's results").expect("the file is written again");

        let unsent = unsent(battles_in(&root), &HashSet::new(), &Unindexable::default(), &skips);
        assert_eq!(unsent, vec![rewritten]);
    }

    /// Once the launcher runs an install named through a link from an overlay, the sweep sends
    /// again neither a battle the ledger holds under the name the install was given nor one it
    /// holds under the install's resolved name. A battle only the overlay holds is sent.
    #[cfg(unix)]
    #[test]
    fn an_install_named_through_a_link_is_not_sent_again_once_overlaid() {
        let tmp = tempfile::tempdir().expect("a temporary directory can be made");
        let base = std::fs::canonicalize(tmp.path()).expect("the directory resolves");
        let real = base.join("real");
        std::fs::create_dir_all(real.join("install/replays")).expect("the install is made");
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("the link is made");
        let install = link.join("install");
        let replays = install.join("replays");
        for name in ["named.wowsreplay", "resolved.wowsreplay", "unsent.wowsreplay"] {
            std::fs::write(replays.join(name), name.as_bytes()).expect("the file is written");
        }
        let dirs = wgc_launch::RuntimeDirs::new(&link.join("app"));
        let state = dirs.install_state(&std::fs::canonicalize(&install).expect("the install resolves"));
        let upper = state.upper().join("replays");
        std::fs::create_dir_all(&upper).expect("the upper directory is made");
        std::fs::write(state.backend_record(), "kernel-bwrap").expect("the backend is recorded");
        std::fs::write(upper.join("upper.wowsreplay"), b"upper").expect("the file is written");
        let sent = HashSet::from([
            replays.join("named.wowsreplay").to_string_lossy().into_owned(),
            real.join("install/replays/resolved.wowsreplay").to_string_lossy().into_owned(),
        ]);

        let folder = ReplayFolder::from_install(&install, Some(&dirs));
        let mut unsent = unsent_of(&folder, &sent, &Unindexable::default(), &StableUploadSkips::default());
        unsent.sort();

        let mut expected = vec![replays.join("unsent.wowsreplay"), upper.join("upper.wowsreplay")];
        expected.sort();
        assert_eq!(unsent, expected);
    }

    /// A battle keeps the earliest anchor recorded under any of its paths, though the path it is
    /// read by already has a later one: its grace window runs from its first sight.
    #[test]
    fn the_anchor_is_the_first_sight_under_any_path() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime starts");
        let pool = runtime.block_on(wows_toolkit_config::test_pool());
        let (lower, upper) = (PathBuf::from("/mnt/wows/replays/a.wowsreplay"), PathBuf::from("/upper/a.wowsreplay"));
        let at = |second| jiff::Timestamp::from_second(second).expect("the time is in range");
        let record = |path: &Path, second| {
            let path = path.to_string_lossy().into_owned();
            runtime.block_on(wows_toolkit_config::queries::raw_upload_first_seen(&pool, &path, at(second)))
        };
        record(&lower, 1_000).expect("the anchor is recorded");
        record(&upper, 2_000).expect("the anchor is recorded");

        let anchor = runtime.block_on(upload::first_seen(&pool, &upper, &[lower])).expect("the anchor is read");

        assert_eq!(anchor, at(1_000));
    }
}
