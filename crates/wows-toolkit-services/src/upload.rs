//! Contributing a finished battle to ShipBuilds, as the data-sharing setting
//! asks.
//!
//! The rules -- which battles are eligible, which payload each mode sends, and
//! how long a raw upload waits for the battle's results -- are
//! `wows_toolkit_viewmodel::upload`. What is here is the HTTP and the ledger
//! read: the `sent_replays` table, so a replay is sent once however it was
//! read.
//!
//! Every function is a future for the tokio runtime: the store and the client
//! both need its reactor.

use std::path::Path;
use std::path::PathBuf;

use sqlx::sqlite::SqlitePool;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::upload::RAW_REPLAYS_URL;
use wows_toolkit_viewmodel::upload::RawUploadDeadline;
use wows_toolkit_viewmodel::upload::ReplayUploadAction;
use wows_toolkit_viewmodel::upload::ReplayUploadSkipReason;
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
    /// Nothing to send, because of what the replay holds: ShipBuilds does not
    /// take its game type, or Replays mode cannot rule out a test ship. The same
    /// bytes always decide the same, so a sweep need not read them again.
    Ineligible,
    /// Already in the ledger.
    AlreadySent,
    /// Nothing sent yet: the battle has no results and the grace has not lapsed.
    /// A read at `deadline` sends it whether or not the results have arrived.
    Waiting { deadline: RawUploadDeadline },
    /// The service could not be reached or did not accept it. Not recorded, so
    /// it is tried again.
    Failed(String),
}

/// Whether the sent-replay ledger decides what is skipped.
///
/// A bulk pass is normally over what has not been sent, but a reader told the
/// service lost what they sent needs a pass that sends it all again.
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

/// Where each kind of payload is posted.
struct Endpoints<'a> {
    builds: &'a str,
    raw_replays: &'a str,
}

const SHIP_BUILDS: Endpoints<'static> = Endpoints { builds: SHIP_BUILDS_URL, raw_replays: RAW_REPLAYS_URL };

/// When this replay was first seen, which is what the grace window is measured
/// from.
///
/// Recorded on the first sight, so later passes agree about how long a battle
/// has been waiting for its results.
/// A battle first seen under one of `other_paths` keeps that anchor. Without the
/// anchor the window cannot be measured, so a caller tries again rather than
/// sending.
pub async fn first_seen(
    pool: &SqlitePool,
    path: &Path,
    other_paths: &[PathBuf],
) -> Result<jiff::Timestamp, sqlx::Error> {
    let mut first_sight = jiff::Timestamp::now();
    for other in other_paths {
        if let Some(anchor) = wows_toolkit_config::queries::raw_upload_anchor(pool, &other.to_string_lossy()).await? {
            first_sight = first_sight.min(anchor);
        }
    }

    wows_toolkit_config::queries::raw_upload_first_seen(pool, &path.to_string_lossy(), first_sight).await
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
    send_to(candidate, mode, ledger, sender, SHIP_BUILDS).await
}

async fn send_to(
    candidate: Candidate,
    mode: DataSharingMode,
    ledger: LedgerUse,
    sender: Sender<'_>,
    endpoints: Endpoints<'_>,
) -> Outcome {
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
            match reason {
                ReplayUploadSkipReason::IneligibleGameType | ReplayUploadSkipReason::PossibleTestShip => {
                    Outcome::Ineligible
                }
                ReplayUploadSkipReason::SharingDisabled => Outcome::Skipped,
            }
        }
        ReplayUploadAction::AwaitResults { deadline } => {
            tracing::debug!(path = %path.display(), until = %deadline.0, "sharing: waiting for the battle's results");
            Outcome::Waiting { deadline }
        }
        ReplayUploadAction::BuildData => {
            if shareable.builds.is_empty() {
                return Outcome::Skipped;
            }
            for payload in &shareable.builds {
                if let Err(err) = deliver(client.post(endpoints.builds).json(payload)).await {
                    return Outcome::Failed(err);
                }
            }
            record(sender.pool, &recorded).await
        }
        ReplayUploadAction::RawReplay => {
            let bytes = match tokio::fs::read(&path).await {
                Ok(bytes) => bytes,
                Err(err) => return Outcome::Failed(err.to_string()),
            };
            let request = client
                .post(endpoints.raw_replays)
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes);
            match deliver(request).await {
                Ok(()) => record(sender.pool, &recorded).await,
                Err(err) => Outcome::Failed(err),
            }
        }
    }
}

/// Posts one request. Only a success status counts as delivered: a rejection
/// or a redirect leaves the replay unrecorded, so a later read tries again.
async fn deliver(request: reqwest::RequestBuilder) -> Result<(), String> {
    let response = request.send().await.map_err(|err| err.to_string())?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("ShipBuilds answered {status}"));
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::io::Write;
    use std::net::TcpListener;

    use wows_toolkit_viewmodel::upload::ResultsScan;

    use super::*;

    const AGENT: UserAgent = UserAgent::new("wows-toolkit-services-test");

    /// A server answering one request with each of `statuses` in turn.
    fn status_server(statuses: &[u16]) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let statuses = statuses.to_vec();
        let handle = std::thread::spawn(move || {
            for status in statuses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                let reason = if (200..300).contains(&status) { "OK" } else { "Error" };
                write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
        });

        (format!("http://{address}"), handle)
    }

    /// Sends a finished random battle as a raw replay to a server answering
    /// `status`, and returns the outcome and whether the ledger recorded it.
    async fn send_raw_against(status: u16) -> (Outcome, bool) {
        let pool = wows_toolkit_config::test_pool().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("20261116_120000_PASB110-Yamato_19_OC_prey.wowsreplay");
        std::fs::write(&path, b"replay").unwrap();
        let candidate = Candidate {
            path: path.clone(),
            shareable: Shareable {
                game_type: "RandomBattle".to_owned(),
                // The game type alone decides eligibility.
                version: Default::default(),
                self_confirmed_non_test: true,
                results: ResultsScan::Present,
                builds: Vec::new(),
            },
            first_seen: jiff::Timestamp::now(),
        };
        let (url, server) = status_server(&[status]);
        let endpoints = Endpoints { builds: &url, raw_replays: &url };
        let sender = Sender { pool: &pool, agent: AGENT, proxy: "" };

        let outcome = send_to(candidate, DataSharingMode::Replays, LedgerUse::Consult, sender, endpoints).await;

        server.join().unwrap();
        let recorded = wows_toolkit_config::queries::sent_replay_exists(&pool, &path.to_string_lossy()).await.unwrap();

        (outcome, recorded)
    }

    #[tokio::test]
    async fn a_success_status_is_recorded_as_sent() {
        for status in [200, 204] {
            assert_eq!(send_raw_against(status).await, (Outcome::Sent, true), "HTTP {status}");
        }
    }

    #[tokio::test]
    async fn any_other_status_is_a_failure_that_is_not_recorded() {
        for status in [302, 400, 500] {
            let (outcome, recorded) = send_raw_against(status).await;

            assert!(matches!(outcome, Outcome::Failed(_)), "HTTP {status}: {outcome:?}");
            assert!(!recorded, "HTTP {status}");
        }
    }
}
