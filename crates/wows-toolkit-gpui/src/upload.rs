//! Contributing a finished battle to ShipBuilds, as the data-sharing setting
//! asks.
//!
//! The rules and the sending are `wows_toolkit_viewmodel::upload`, shared with
//! the egui app so both decide the same thing about the same replay. What is
//! here runs them off the draw thread.

use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::upload::contribute::Candidate;
use wows_toolkit_viewmodel::upload::contribute::ContributeError;
use wows_toolkit_viewmodel::upload::contribute::LedgerUse;
use wows_toolkit_viewmodel::upload::contribute::Outcome;

/// When this replay was first seen; see
/// `wows_toolkit_viewmodel::upload::contribute::first_seen`.
pub fn first_seen(path: PathBuf, pool: sqlx::sqlite::SqlitePool, cx: &App) -> Task<jiff::Timestamp> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return jiff::Timestamp::now() };
        runtime.block_on(async move { wows_toolkit_viewmodel::upload::contribute::first_seen(&pool, &path).await })
    })
}

/// Whether this replay is already in the ledger.
pub fn already_contributed(path: PathBuf, pool: sqlx::sqlite::SqlitePool, cx: &App) -> Task<bool> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return false };
        runtime.block_on(
            async move { wows_toolkit_viewmodel::upload::contribute::already_contributed(&pool, &path).await },
        )
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
        let Some(runtime) = runtime else { return Outcome::Failed(ContributeError::NoRuntime) };
        let client = match crate::http::client(&proxy, reqwest::redirect::Policy::none()) {
            Ok(client) => client,
            Err(err) => return Outcome::Failed(ContributeError::Client(err)),
        };
        runtime.block_on(async move {
            wows_toolkit_viewmodel::upload::contribute::send(candidate, mode, ledger, &pool, &client).await
        })
    })
}
