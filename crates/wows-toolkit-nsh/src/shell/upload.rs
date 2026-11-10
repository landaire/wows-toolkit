//! Contributing a finished battle, as the data-sharing setting asks.
//!
//! The rules (which battles are eligible, what each mode sends, how long a raw
//! upload waits for results) and the sending are
//! `wows_toolkit_viewmodel::upload`, shared with the other front ends so all of
//! them decide the same thing about the same replay and keep one ledger.

use std::path::PathBuf;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::PropEnum;
use nsh_core::prelude::background;
use nsh_core::prelude::blocking;
use nsh_core::prelude::module;
use wows_toolkit_viewmodel::upload::contribute::Candidate;
use wows_toolkit_viewmodel::upload::contribute::LedgerUse;
use wows_toolkit_viewmodel::upload::contribute::Outcome;
use wows_toolkit_viewmodel::upload::shareable::Shareable;

use crate::shell::error::SettingsError;
use crate::shell::error::UploadError;
use crate::shell::settings::parse_sharing;
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

/// Sends what the data-sharing setting asks for about the replay at `path`,
/// whose parse found `shareable`, and records what was sent.
///
/// The mode and proxy are read from the shared settings at the moment of the
/// send, so a change made in any front end applies to the next battle.
pub async fn contribute(path: PathBuf, shareable: Shareable, ledger: LedgerUse) -> Result<Outcome, SettingsError> {
    let pool = crate::shell::settings::open().await?;
    let settings = crate::shell::settings::load_from(&pool).await?;
    let mode = parse_sharing(&settings.data_sharing)?;

    let redirects = reqwest::redirect::Policy::none();
    let client = match wows_toolkit_viewmodel::http::client(USER_AGENT, &settings.proxy_url, redirects) {
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
        let outcome = background(contribute_path(PathBuf::from(path), ledger.usage())).await.map_err(UploadError::Task)??;

        Ok(Contribution::from_outcome(outcome))
    }
}
