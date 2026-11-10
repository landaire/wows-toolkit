//! Contributing a finished battle to ShipBuilds, as the data-sharing setting
//! asks: the HTTP and the ledger read around the decision in `crate::upload`.
//!
//! The ledger is the `sent_replays` table both apps write, so a replay is sent
//! once however it was read.

use std::path::Path;
use std::path::PathBuf;

use sqlx::sqlite::SqlitePool;

use crate::settings::DataSharingMode;
use crate::upload::RAW_REPLAYS_URL;
use crate::upload::ReplayUploadAction;
use crate::upload::SHIP_BUILDS_URL;
use crate::upload::decide_upload_action;
use crate::upload::is_eligible_game_type;
use crate::upload::raw_replay_snapshot_state;
use crate::upload::shareable::Shareable;

/// What one replay's contribution came to.
#[derive(Debug)]
pub enum Outcome {
    /// Sent, and recorded in the ledger so it is not sent again.
    Sent,
    /// Nothing to send, for a reason that will not change.
    Skipped,
    /// Already in the ledger.
    AlreadySent,
    /// Nothing sent yet: the battle has no results and the grace has not lapsed.
    /// The next read of this replay decides again.
    Waiting,
    /// The service could not be reached. Not recorded, so it is tried again.
    Failed(ContributeError),
}

/// Why a contribution did not complete.
#[derive(Debug, thiserror::Error)]
pub enum ContributeError {
    #[error("no runtime to send on")]
    NoRuntime,
    #[error(transparent)]
    Client(#[from] crate::http::HttpError),
    #[error("the sent-replay ledger could not be read: {0}")]
    LedgerRead(#[source] sqlx::Error),
    #[error(transparent)]
    ReadReplay(std::io::Error),
    #[error(transparent)]
    Post(reqwest::Error),
    /// Sent but not recorded: the next read sends it again and the service
    /// sees it twice.
    #[error("sent, but the ledger was not written: {0}")]
    LedgerWrite(#[source] sqlx::Error),
}

/// Whether the sent-replay ledger decides what is skipped.
///
/// The egui app's `SendReplayCachePolicy`: a bulk pass is normally over what has
/// not been sent, but a reader told the service lost what they sent needs a pass
/// that sends it all again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerUse {
    /// Skip what the ledger says was already sent, which is every ordinary send.
    Consult,
    /// Send it whether the ledger has it or not.
    Ignore,
}

/// One replay's contribution: the file, and what the parse found in it.
pub struct Candidate {
    pub path: PathBuf,
    pub shareable: Shareable,
    /// When this replay was first seen, which anchors the grace window.
    pub first_seen: jiff::Timestamp,
}

/// When this replay was first seen, which is what the grace window is measured
/// from.
///
/// Recorded on the first sight, in the row the egui app's own uploader keeps, so
/// the two apps agree about how long a battle has been waiting for its results.
pub async fn first_seen(pool: &SqlitePool, path: &Path) -> jiff::Timestamp {
    let now = jiff::Timestamp::now();
    let recorded = path.to_string_lossy();

    wows_toolkit_config::queries::raw_upload_first_seen(pool, &recorded, now).await.unwrap_or_else(|err| {
        // A window that starts now holds the upload rather than sending a
        // battle that may still be in progress.
        tracing::warn!(%err, "sharing: the first-seen anchor could not be read");
        now
    })
}

/// Whether this replay is already in the ledger.
///
/// Asked before a bulk pass reads a replay at all: the read is seconds of work,
/// and a replay already contributed needs none of it.
pub async fn already_contributed(pool: &SqlitePool, path: &Path) -> bool {
    let recorded = path.to_string_lossy();

    // A ledger that cannot be read here costs only the read: `send` consults it
    // again and reports the failure there.
    wows_toolkit_config::queries::sent_replay_exists(pool, &recorded).await.unwrap_or(false)
}

/// Sends what `mode` asks for, if anything, and records what was sent.
///
/// The caller builds `client` with `reqwest::redirect::Policy::none()`.
pub async fn send(
    candidate: Candidate,
    mode: DataSharingMode,
    ledger: LedgerUse,
    pool: &SqlitePool,
    client: &reqwest::Client,
) -> Outcome {
    let path = candidate.path.clone();
    let recorded = path.to_string_lossy().into_owned();

    // The ledger first: a replay already contributed is not read again, whichever
    // app read it the first time.
    if ledger == LedgerUse::Consult {
        match wows_toolkit_config::queries::sent_replay_exists(pool, &recorded).await {
            Ok(true) => return Outcome::AlreadySent,
            Ok(false) => {}
            // A ledger that cannot be read is not a reason to send again: a second
            // copy of a replay is worse for the service than a missing one.
            Err(err) => return Outcome::Failed(ContributeError::LedgerRead(err)),
        }
    }

    let shareable = &candidate.shareable;
    let eligible = is_eligible_game_type(&shareable.game_type, shareable.version);
    let snapshot = raw_replay_snapshot_state(shareable.results, candidate.first_seen, jiff::Timestamp::now());
    let action = decide_upload_action(mode, eligible, shareable.self_confirmed_non_test, snapshot);

    match action {
        ReplayUploadAction::Skip(reason) => {
            tracing::debug!(path = %path.display(), ?reason, "sharing: nothing to send");
            Outcome::Skipped
        }
        ReplayUploadAction::AwaitResults { deadline } => {
            tracing::debug!(path = %path.display(), until = %deadline.0, "sharing: waiting for the battle's results");
            Outcome::Waiting
        }
        ReplayUploadAction::BuildData => {
            if shareable.builds.is_empty() {
                return Outcome::Skipped;
            }
            for payload in &shareable.builds {
                if let Err(err) = client.post(SHIP_BUILDS_URL).json(payload).send().await {
                    return Outcome::Failed(ContributeError::Post(err));
                }
            }
            record(pool, &recorded).await
        }
        ReplayUploadAction::RawReplay => {
            let bytes = match tokio::fs::read(&path).await {
                Ok(bytes) => bytes,
                Err(err) => return Outcome::Failed(ContributeError::ReadReplay(err)),
            };
            let sent = client
                .post(RAW_REPLAYS_URL)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes)
                .send()
                .await;
            match sent {
                Ok(_) => record(pool, &recorded).await,
                Err(err) => Outcome::Failed(ContributeError::Post(err)),
            }
        }
    }
}

/// Records the replay as contributed, so nothing sends it twice.
async fn record(pool: &SqlitePool, path: &str) -> Outcome {
    match wows_toolkit_config::queries::insert_sent_replay(pool, path).await {
        Ok(()) => Outcome::Sent,
        Err(err) => Outcome::Failed(ContributeError::LedgerWrite(err)),
    }
}
