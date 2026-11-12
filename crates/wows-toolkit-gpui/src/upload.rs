//! Schedules `wows_toolkit_services::upload` on the shared tokio runtime, off
//! the draw.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;
use wows_toolkit_services::upload::Candidate;
use wows_toolkit_services::upload::LedgerUse;
use wows_toolkit_services::upload::Outcome;
use wows_toolkit_services::upload::Sender;
use wows_toolkit_viewmodel::settings::DataSharingMode;

/// When this replay was first seen (`upload::first_seen`).
pub fn first_seen(path: PathBuf, pool: sqlx::sqlite::SqlitePool, cx: &App) -> Task<jiff::Timestamp> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        // Without a runtime the window starts now, which holds the upload
        // rather than sending a battle that may still be in progress.
        let Some(runtime) = runtime else { return jiff::Timestamp::now() };

        runtime.block_on(wows_toolkit_services::upload::first_seen(&pool, &path))
    })
}

/// Whether this replay is already in the ledger (`upload::already_contributed`).
pub fn already_contributed(path: PathBuf, pool: sqlx::sqlite::SqlitePool, cx: &App) -> Task<bool> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return false };

        runtime.block_on(wows_toolkit_services::upload::already_contributed(&pool, &path))
    })
}

/// Sends what `mode` asks for, if anything, and records what was sent.
///
/// Runs on the caller's task rather than blocking a draw: the read of the file
/// and the request both take as long as the network does.
pub fn contribute(
    candidate: Candidate,
    mode: DataSharingMode,
    ledger: LedgerUse,
    pool: sqlx::sqlite::SqlitePool,
    proxy: String,
    cx: &App,
) -> Task<Outcome> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return Outcome::Failed("no runtime to send on".to_owned()) };

        let sender = Sender { pool: &pool, agent: crate::http::USER_AGENT, proxy: &proxy };
        runtime.block_on(wows_toolkit_services::upload::send(candidate, mode, ledger, sender))
    })
}
