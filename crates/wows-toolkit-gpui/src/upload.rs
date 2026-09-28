//! Contributing a finished battle to ShipBuilds, as the data-sharing setting
//! asks.
//!
//! The rules -- which battles are eligible, which payload each mode sends, and
//! how long a raw upload waits for the battle's results -- are
//! `wows_toolkit_viewmodel::upload`, shared with the egui app so both decide the
//! same thing about the same replay. What is here is this app's own HTTP and the
//! ledger read: the `sent_replays` table both apps write, so a replay is sent
//! once however it was read.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::upload::RAW_REPLAYS_URL;
use wows_toolkit_viewmodel::upload::ReplayUploadAction;
use wows_toolkit_viewmodel::upload::ResultsScan;
use wows_toolkit_viewmodel::upload::SHIP_BUILDS_URL;
use wows_toolkit_viewmodel::upload::build_tracker::BuildTrackerPayload;
use wows_toolkit_viewmodel::upload::decide_upload_action;
use wows_toolkit_viewmodel::upload::is_eligible_game_type;
use wows_toolkit_viewmodel::upload::raw_replay_snapshot_state;

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

/// What a decision needs to know about the parsed battle.
///
/// Taken as plain data rather than the report itself: the decision is shared and
/// must not depend on either app's own types.
pub struct Shareable {
    /// `ReplayMeta::gameType`, which says whether the service takes this battle.
    pub game_type: String,
    pub version: wowsunpack::data::Version,
    /// Whether the recording player's ship is positively known not to be a test
    /// ship. Uncertainty is `false`, which keeps the file off `/api/replays`.
    pub self_confirmed_non_test: bool,
    /// Whether the packet stream carried an end-of-battle marker.
    pub results: ResultsScan,
    /// The per-player build payloads, for the build-data mode. Empty is nothing
    /// to send.
    pub builds: Vec<BuildTrackerPayload>,
}

/// One replay's contribution: the file, and what the parse found in it.
pub struct Candidate {
    pub path: PathBuf,
    pub shareable: Shareable,
    /// When this replay was first seen, which anchors the grace window.
    pub first_seen: jiff::Timestamp,
}

/// Sends what `mode` asks for, if anything, and records what was sent.
///
/// Runs on the caller's task rather than blocking a draw: the read of the file
/// and the request both take as long as the network does.
pub fn contribute(
    candidate: Candidate,
    mode: DataSharingMode,
    pool: sqlx::sqlite::SqlitePool,
    proxy: String,
    cx: &App,
) -> Task<Outcome> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return Outcome::Failed("no runtime to send on".to_owned()) };
        runtime.block_on(async move { send(candidate, mode, &pool, &proxy).await })
    })
}

async fn send(candidate: Candidate, mode: DataSharingMode, pool: &sqlx::sqlite::SqlitePool, proxy: &str) -> Outcome {
    let path = candidate.path.clone();
    let recorded = path.to_string_lossy().into_owned();

    // The ledger first: a replay already contributed is not read again, whichever
    // app read it the first time.
    match wows_toolkit_config::queries::sent_replay_exists(pool, &recorded).await {
        Ok(true) => return Outcome::AlreadySent,
        Ok(false) => {}
        // A ledger that cannot be read is not a reason to send again: a second
        // copy of a replay is worse for the service than a missing one.
        Err(err) => return Outcome::Failed(format!("the sent-replay ledger could not be read: {err}")),
    }

    let shareable = &candidate.shareable;
    let eligible = is_eligible_game_type(&shareable.game_type, shareable.version);
    let snapshot = raw_replay_snapshot_state(shareable.results, candidate.first_seen, jiff::Timestamp::now());
    let action = decide_upload_action(mode, eligible, shareable.self_confirmed_non_test, snapshot);

    let client = match crate::http::client(proxy, reqwest::redirect::Policy::none()) {
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
            record(pool, &recorded).await
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
                Ok(_) => record(pool, &recorded).await,
                Err(err) => Outcome::Failed(err.to_string()),
            }
        }
    }
}

/// Records the replay as contributed, so nothing sends it twice.
async fn record(pool: &sqlx::sqlite::SqlitePool, path: &str) -> Outcome {
    match wows_toolkit_config::queries::insert_sent_replay(pool, path).await {
        Ok(()) => Outcome::Sent,
        // Sent but not recorded: say so, because the next read will send it
        // again and the service will see it twice.
        Err(err) => Outcome::Failed(format!("sent, but the ledger was not written: {err}")),
    }
}

/// Whether the recording player's ship is positively not a test ship.
///
/// Any doubt is `false`: an unknown ship keeps the whole file off the service,
/// which is the rule `decide_upload_action` documents.
pub fn self_is_not_a_test_ship(report: &wows_battle_world::report::BattleReport) -> bool {
    report
        .players()
        .iter()
        .find(|player| player.relation().is_self())
        .and_then(|player| player.vehicle().vehicle())
        .map(|vehicle| !vehicle.is_test_ship())
        .unwrap_or(false)
}
