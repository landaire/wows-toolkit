//! Contributing a finished battle to ShipBuilds, as the data-sharing setting
//! asks.
//!
//! The rules -- which battles are eligible, which payload each mode sends, and
//! how long a raw upload waits for the battle's results -- are
//! `wows_toolkit_viewmodel::upload`, shared with the egui app so both decide the
//! same thing about the same replay. What is here is the HTTP and the ledger
//! read: the `sent_replays` table both apps write, so a replay is sent once
//! however it was read.
//!
//! Every function is a future for the tokio runtime: the store and the client
//! both need its reactor.

use std::path::Path;
use std::path::PathBuf;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::upload::RAW_REPLAYS_URL;
use wows_toolkit_viewmodel::upload::ReplayUploadAction;
use wows_toolkit_viewmodel::upload::SHIP_BUILDS_URL;
use wows_toolkit_viewmodel::upload::Shareable;
use wows_toolkit_viewmodel::upload::decide_upload_action;
use wows_toolkit_viewmodel::upload::is_eligible_game_type;
use wows_toolkit_viewmodel::upload::raw_replay_snapshot_state;

use crate::http::UserAgent;

/// What one replay's contribution came to.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    Failed(String),
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

/// Where a contribution goes out from: the store holding the ledger, and the
/// client settings.
pub struct Sender<'a> {
    pub pool: &'a SqlitePool,
    pub agent: UserAgent,
    /// The saved proxy setting, empty for the machine's own configuration.
    pub proxy: &'a str,
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
/// and a replay already contributed needs none of it. A ledger that cannot be
/// read answers `false`, and [`send`] asks again before anything goes out.
pub async fn already_contributed(pool: &SqlitePool, path: &Path) -> bool {
    let recorded = path.to_string_lossy();

    wows_toolkit_config::queries::sent_replay_exists(pool, &recorded).await.unwrap_or(false)
}

/// Sends what `mode` asks for, if anything, and records what was sent.
pub async fn send(candidate: Candidate, mode: DataSharingMode, ledger: LedgerUse, sender: Sender<'_>) -> Outcome {
    let path = candidate.path.clone();
    let recorded = path.to_string_lossy().into_owned();

    // The ledger first: a replay already contributed is not read again, whichever
    // app read it the first time.
    if ledger == LedgerUse::Consult {
        match wows_toolkit_config::queries::sent_replay_exists(sender.pool, &recorded).await {
            Ok(true) => return Outcome::AlreadySent,
            Ok(false) => {}
            // A ledger that cannot be read is not a reason to send again: a second
            // copy of a replay is worse for the service than a missing one.
            Err(err) => return Outcome::Failed(format!("the sent-replay ledger could not be read: {err}")),
        }
    }

    let shareable = &candidate.shareable;
    let eligible = is_eligible_game_type(&shareable.game_type, shareable.version);
    let snapshot = raw_replay_snapshot_state(shareable.results, candidate.first_seen, jiff::Timestamp::now());
    let action = decide_upload_action(mode, eligible, shareable.self_confirmed_non_test, snapshot);

    let client = match crate::http::client(sender.agent, sender.proxy, reqwest::redirect::Policy::none()) {
        Ok(client) => client,
        Err(err) => return Outcome::Failed(err.to_string()),
    };

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
                    return Outcome::Failed(err.to_string());
                }
            }
            record(sender.pool, &recorded).await
        }
        ReplayUploadAction::RawReplay => {
            let bytes = match tokio::fs::read(&path).await {
                Ok(bytes) => bytes,
                Err(err) => return Outcome::Failed(err.to_string()),
            };
            let sent = client
                .post(RAW_REPLAYS_URL)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes)
                .send()
                .await;
            match sent {
                Ok(_) => record(sender.pool, &recorded).await,
                Err(err) => Outcome::Failed(err.to_string()),
            }
        }
    }
}

/// Records the replay as contributed, so nothing sends it twice.
async fn record(pool: &SqlitePool, path: &str) -> Outcome {
    match wows_toolkit_config::queries::insert_sent_replay(pool, path).await {
        Ok(()) => Outcome::Sent,
        // Sent but not recorded: say so, because the next read will send it
        // again and the service will see it twice.
        Err(err) => Outcome::Failed(format!("sent, but the ledger was not written: {err}")),
    }
}
