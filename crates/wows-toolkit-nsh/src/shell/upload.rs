//! Contributing a finished battle, as the data-sharing setting asks.
//!
//! The rules (which battles are eligible, what each mode sends, how long a raw
//! upload waits for results) and the sending are
//! `wows_toolkit_viewmodel::upload`, shared with the other front ends so all of
//! them decide the same thing about the same replay and keep one ledger.

use std::path::PathBuf;

use wows_toolkit_viewmodel::upload::contribute::Candidate;
use wows_toolkit_viewmodel::upload::contribute::LedgerUse;
use wows_toolkit_viewmodel::upload::contribute::Outcome;
use wows_toolkit_viewmodel::upload::shareable::Shareable;

use crate::shell::error::SettingsError;
use crate::shell::settings::parse_sharing;
use crate::shell::update::USER_AGENT;

/// Sends what the data-sharing setting asks for about the replay at `path`,
/// whose parse found `shareable`, and records what was sent.
///
/// The mode and proxy are read from the shared settings at the moment of the
/// send, so a change made in any front end applies to the next battle.
pub async fn contribute(path: PathBuf, shareable: Shareable, ledger: LedgerUse) -> Result<Outcome, SettingsError> {
    let pool = crate::shell::settings::open().await?;
    let settings = crate::shell::settings::load_from(&pool).await?;
    let mode = parse_sharing(&settings.data_sharing)?;

    let client = match wows_toolkit_viewmodel::http::client(USER_AGENT, &settings.proxy_url, reqwest::redirect::Policy::none())
    {
        Ok(client) => client,
        Err(err) => return Ok(Outcome::Failed(err.to_string())),
    };
    let first_seen = wows_toolkit_viewmodel::upload::contribute::first_seen(&pool, &path).await;
    let candidate = Candidate { path, shareable, first_seen };

    Ok(wows_toolkit_viewmodel::upload::contribute::send(candidate, mode, ledger, &pool, &client).await)
}
