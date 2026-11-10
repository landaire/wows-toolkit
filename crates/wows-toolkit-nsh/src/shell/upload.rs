//! Contributing a finished battle, as the data-sharing setting asks.
//!
//! The rules (which battles are eligible, what each mode sends, how long a raw
//! upload waits for results) and the sending are
//! `wows_toolkit_viewmodel::upload`, shared with the other front ends so all of
//! them decide the same thing about the same replay and keep one ledger.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::PoisonError;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use nsh_core::prelude::background;
use nsh_core::prelude::blocking;
use nsh_core::prelude::module;
use tokio::sync::Notify;
use wows_toolkit_viewmodel::upload::contribute::Candidate;
use wows_toolkit_viewmodel::upload::contribute::LedgerUse;
use wows_toolkit_viewmodel::upload::contribute::Outcome;
use wows_toolkit_viewmodel::upload::shareable::Shareable;

use crate::shell::error::SettingsError;
use crate::shell::error::UploadError;
use crate::shell::update::USER_AGENT;

/// Whether the sent-replay ledger decides what is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum Ledger {
    /// Skip what the ledger says was already sent, which is every ordinary send.
    Consult,
    /// Send it whether the ledger has it or not, for a reader told the service
    /// lost what they sent.
    Ignore,
}

impl Ledger {
    fn usage(self) -> LedgerUse {
        match self {
            Self::Consult => LedgerUse::Consult,
            Self::Ignore => LedgerUse::Ignore,
        }
    }
}

/// What one replay's contribution came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum ContributionOutcome {
    Sent,
    Skipped,
    AlreadySent,
    /// The battle has no results yet and its grace has not lapsed; the next
    /// read decides again.
    Waiting,
    Failed,
}

/// One replay's contribution, and why it failed when it did.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct Contribution {
    pub outcome: ContributionOutcome,
    pub reason: Option<String>,
}

impl Contribution {
    fn from_outcome(outcome: Outcome) -> Self {
        let (outcome, reason) = match outcome {
            Outcome::Sent => (ContributionOutcome::Sent, None),
            Outcome::Skipped => (ContributionOutcome::Skipped, None),
            Outcome::AlreadySent => (ContributionOutcome::AlreadySent, None),
            Outcome::Waiting => (ContributionOutcome::Waiting, None),
            Outcome::Failed(reason) => (ContributionOutcome::Failed, Some(reason)),
        };

        Self { outcome, reason }
    }
}

/// The replays being contributed in this process: the automatic contribution
/// of a landed battle and a contribute-all pass may reach the same replay at
/// once, and both would find it missing from the ledger and send it.
struct Claims {
    held: Mutex<BTreeSet<PathBuf>>,
    released: Notify,
}

impl Claims {
    fn new() -> Self {
        Self { held: Mutex::new(BTreeSet::new()), released: Notify::new() }
    }

    /// Holds `path` until the claim drops, first waiting out whoever holds it.
    /// The waiter then reads the ledger the holder wrote, or sends itself when
    /// the holder failed.
    async fn claim(&self, path: &Path) -> Claim<'_> {
        loop {
            // Enabled before the check, so a release between the check and the
            // wait still wakes it.
            let mut released = std::pin::pin!(self.released.notified());
            released.as_mut().enable();
            if self.held.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_path_buf()) {
                return Claim { claims: self, path: path.to_path_buf() };
            }
            released.await;
        }
    }
}

/// One replay held by a contribution, released when it drops.
struct Claim<'a> {
    claims: &'a Claims,
    path: PathBuf,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.claims.held.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.path);
        self.claims.released.notify_waiters();
    }
}

static CLAIMS: LazyLock<Claims> = LazyLock::new(Claims::new);

/// Sends what the data-sharing setting asks for about the replay at `path`,
/// whose parse found `shareable`, and records what was sent.
///
/// The mode and proxy are read from the shared settings at the moment of the
/// send, so a change made in any front end applies to the next battle.
///
/// A second call for a replay already being contributed waits for the first.
pub async fn contribute(path: PathBuf, shareable: Shareable, ledger: LedgerUse) -> Result<Outcome, SettingsError> {
    let _claim = CLAIMS.claim(&path).await;
    let pool = crate::shell::settings::open().await?;
    let mode = crate::shell::settings::sharing_mode(&pool).await?;
    let proxy_url = crate::shell::settings::manual_proxy(&pool).await?;

    let redirects = reqwest::redirect::Policy::none();
    let client =
        match wows_toolkit_viewmodel::http::client(USER_AGENT, proxy_url.as_deref().unwrap_or_default(), redirects) {
            Ok(client) => client,
            Err(err) => return Ok(Outcome::Failed(err.to_string())),
        };
    let first_seen = wows_toolkit_viewmodel::upload::contribute::first_seen(&pool, &path).await;
    let candidate = Candidate { path, shareable, first_seen };

    Ok(wows_toolkit_viewmodel::upload::contribute::send(candidate, mode, ledger, &pool, &client).await)
}

/// Reads a replay and contributes it.
async fn contribute_path(path: PathBuf, ledger: LedgerUse) -> Result<Outcome, UploadError> {
    // A replay already contributed is not read at all: the read is seconds of
    // work, and the send would be refused.
    if ledger == LedgerUse::Consult {
        let pool = crate::shell::settings::open().await?;
        if wows_toolkit_viewmodel::upload::contribute::already_contributed(&pool, &path).await {
            return Ok(Outcome::AlreadySent);
        }
    }

    let parsing = path.clone();
    let parsed = blocking(move || crate::game_data::parse_replay(&parsing)).await.map_err(UploadError::Task)??;

    Ok(contribute(path, parsed.shareable, ledger).await?)
}

/// Contributing battles to ShipBuilds, as the data-sharing setting asks.
#[module]
pub struct Uploads;

#[module]
impl Uploads {
    /// The replays in the game's replay directory, newest first: what a
    /// contribute-all pass goes over.
    pub fn list_replays() -> Vec<String> {
        let Some(dir) = crate::game_data::replays_dir() else {
            return Vec::new();
        };

        wows_toolkit_replay_data::replay_index::replays_under(&dir)
            .into_iter()
            .map(|path| path.display().to_string())
            .collect()
    }

    /// Reads the replay at `path` and contributes it, as the setting asks.
    pub async fn contribute_replay(path: String, ledger: Ledger) -> Result<Contribution, UploadError> {
        let outcome =
            background(contribute_path(PathBuf::from(path), ledger.usage())).await.map_err(UploadError::Task)??;

        Ok(Contribution::from_outcome(outcome))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_time().build().expect("a test runtime starts")
    }

    #[test]
    fn a_replay_being_contributed_is_claimed_again_only_once_released() {
        runtime().block_on(async {
            let claims = Claims::new();
            let path = Path::new("/replays/battle.wowsreplay");
            let first = claims.claim(path).await;

            let second = tokio::time::timeout(Duration::from_millis(50), claims.claim(path)).await;
            assert!(second.is_err(), "a held replay waits for its holder");

            // A failed send releases the replay as a finished one does.
            drop(first);
            let third = tokio::time::timeout(Duration::from_secs(5), claims.claim(path)).await;
            assert!(third.is_ok(), "a released replay is claimed again");
        });
    }

    #[test]
    fn a_waiting_claim_proceeds_when_the_holder_releases() {
        runtime().block_on(async {
            let claims = Claims::new();
            let path = Path::new("/replays/battle.wowsreplay");
            let first = claims.claim(path).await;

            let waiter = claims.claim(path);
            let release = async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                drop(first);
            };
            let (claimed, ()) = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(waiter, release) })
                .await
                .expect("the waiter is woken by the release");
            assert_eq!(claimed.path, path);
        });
    }

    #[test]
    fn different_replays_are_claimed_independently() {
        runtime().block_on(async {
            let claims = Claims::new();
            let _first = claims.claim(Path::new("/replays/a.wowsreplay")).await;

            let other =
                tokio::time::timeout(Duration::from_millis(50), claims.claim(Path::new("/replays/b.wowsreplay"))).await;
            assert!(other.is_ok());
        });
    }
}
