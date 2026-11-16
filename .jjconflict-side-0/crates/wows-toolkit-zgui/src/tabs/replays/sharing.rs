//! Contributing one replay as the data-sharing setting asks, for each path that does: a battle
//! that lands, one held for its results, the startup sweep and the palette's bulk pass.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::index::unindexable::Unindexable;
use wows_toolkit_config::upload_skips::ReplayFileIdentity;
use wows_toolkit_config::upload_skips::StableUploadSkips;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::parse_replay;
use wows_toolkit_services::upload;
use wows_toolkit_services::upload::Candidate;
use wows_toolkit_services::upload::LedgerUse;
use wows_toolkit_services::upload::Outcome;
use wows_toolkit_services::upload::Sender;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use zgui::task::blocking;

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
        self.services.settings.read_untracked(|settings| settings.data_sharing).filter(|mode| *mode != DataSharingMode::Off)
    }

    /// Reads `path` and sends what the setting asks for, if anything.
    pub async fn contribute(&self, path: PathBuf, ledger: LedgerUse) -> Result<Outcome, ContributeError> {
        let (reading, cache) = (path.clone(), self.game_data.clone());
        let parsed = blocking(move || parse_replay(&reading, &cache, None)).await.map_err(ContributeError::Unparsed)?;

        // The anchor both apps keep, recorded on the first sight of this replay: the grace window
        // for its results runs from there.
        let first_seen = self.first_seen(&path).await.map_err(ContributeError::NoAnchor)?;
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

    /// Whether the ledger already has `path`. A ledger that cannot be read answers `false`, and
    /// the send asks again.
    pub async fn already_contributed(&self, path: &Path) -> bool {
        let (pool, named) = (self.pool.clone(), path.to_path_buf());
        self.services.on_tokio(async move { upload::already_contributed(&pool, &named).await }).await.unwrap_or(false)
    }

    /// Records that `paths`, as they are now, will never be taken, so the startup sweep does not
    /// read them again. The ledger is written once for all of them.
    pub async fn remember_ineligible(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let identities =
            blocking(move || paths.iter().filter_map(|path| ReplayFileIdentity::from_path(path)).collect::<Vec<_>>())
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

    /// The replays in `dir` the startup sweep reads: every battle the game wrote that is neither
    /// in the ledger, nor unreadable, nor known never to be taken.
    pub async fn unsent_in(&self, dir: PathBuf) -> Vec<PathBuf> {
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
        blocking(move || unsent(battles_in(&dir), &sent, &unindexable, &skips)).await
    }

    async fn first_seen(&self, path: &Path) -> Result<jiff::Timestamp, AnchorError> {
        let mut attempt = 1;
        loop {
            let (pool, named) = (self.pool.clone(), path.to_path_buf());
            let read = self.services.on_tokio(async move { upload::first_seen(&pool, &named).await }).await;
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
}
