//! Keeping the result mappings replays are read through current: the newest
//! published mapping for the installed build, each listed build's own, a
//! refetch for a battle read through one that does not fit, and the reader's
//! own file. The work is `wows_toolkit_replay_data::constants`; what is here
//! is what one session remembers about it.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use nsh_core::prelude::JsObject;
use wows_toolkit_replay_data::constants::Cached;
use wows_toolkit_replay_data::constants::Checked;
use wows_toolkit_replay_data::constants::Fetched;
use wows_toolkit_viewmodel::bug_report::ConstantsFetchStatus;
use wows_toolkit_viewmodel::settings::keys;

use crate::game_data::GameDataState;
use crate::replay_inspector::error::InspectorError;
use crate::replay_inspector::settings;

/// A build a listing names, with the version its replays give, which is what
/// resolves a mapping across servers when the build numbers differ.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ConstantsBuild {
    pub build: u32,
    pub version: Option<String>,
}

/// Builds this session has had an answer about. A build whose fetch failed is
/// left out, so a later listing asks again.
static ASKED: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());

/// Builds a battle was read through an unfitting mapping for, each recovered
/// once a session.
static MISMATCHED: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());

/// Whether the newest published mapping was asked about this session.
static LATEST_CHECKED: AtomicBool = AtomicBool::new(false);

/// Fetches the mapping of every build in `builds` that has none on disk and
/// was not asked about this session. Returns how many were written. Blocking.
pub fn fetch_missing(builds: Vec<ConstantsBuild>) -> u32 {
    fetch(builds.into_iter().map(|wanted| (wanted.build, wanted.version)).collect(), Cached::Keep)
}

/// Fetches `build`'s mapping again over the one a battle was read through
/// that does not fit it, and forgets the newest-mapping commit so the next
/// check fetches that again too. `0` when this build was already recovered
/// this session. Blocking.
pub fn recover(build: u32, version: Option<String>) -> Result<u32, InspectorError> {
    if !lock(&MISMATCHED).insert(build) {
        return Ok(0);
    }

    save_commit(None)?;
    LATEST_CHECKED.store(false, Ordering::Relaxed);
    // Asked again, whatever this session already asked: the answer it got is
    // the mapping that does not fit.
    lock(&ASKED).remove(&build);

    Ok(fetch(vec![(build, version)], Cached::Replace))
}

/// Asks whether a newer mapping than the last one written for the installed
/// build was published, and writes it when it was. `true` when one was
/// written. `false` without asking while the game data is not loaded, and
/// once asked this session. Blocking.
pub fn check_latest() -> Result<bool, InspectorError> {
    let GameDataState::Ready(loaded) = crate::game_data::status() else {
        return Ok(false);
    };
    if LATEST_CHECKED.swap(true, Ordering::Relaxed) {
        return Ok(false);
    }

    let known = settings::with_pool(|pool| async move {
        wows_toolkit_config::queries::try_get_setting::<Option<String>>(&pool, keys::CONSTANTS_FILE_COMMIT)
            .await
            .map_err(|source| InspectorError::SettingRead { key: keys::CONSTANTS_FILE_COMMIT, source })
    })?
    .flatten();
    let checked = runtime()?.block_on(wows_toolkit_replay_data::constants::check_latest(loaded.build(), known));

    match checked {
        Checked::Written { build, commit } => {
            wows_toolkit_viewmodel::bug_report::record_constants_fetch(ConstantsFetchStatus::Succeeded);
            tracing::info!(build, "constants: the newest result mapping was written");
            save_commit(Some(commit))?;
            Ok(true)
        }
        Checked::UpToDate => {
            wows_toolkit_viewmodel::bug_report::record_constants_fetch(ConstantsFetchStatus::Succeeded);
            Ok(false)
        }
        Checked::Failed(reason) => {
            let reason = reason.to_string();
            wows_toolkit_viewmodel::bug_report::record_constants_fetch(ConstantsFetchStatus::Failed(reason.clone()));
            tracing::warn!(%reason, "constants: the published mappings could not be checked");
            Err(InspectorError::ConstantsCheck { reason })
        }
    }
}

/// The build a mapping is imported for: the one loaded. Refused while none is.
pub fn loaded_build() -> Result<u32, InspectorError> {
    match crate::game_data::status() {
        GameDataState::Ready(loaded) => Ok(loaded.build()),
        GameDataState::NotConfigured | GameDataState::Loading { .. } | GameDataState::Failed { .. } => {
            Err(InspectorError::NoBuildLoaded)
        }
    }
}

/// Takes the `constants.json` at `from` as `build`'s mapping. Blocking.
pub fn import(from: &Path, build: u32) -> Result<(), InspectorError> {
    wows_toolkit_replay_data::constants::import(from, build)
        .map_err(|reason| InspectorError::ConstantsImport { reason: reason.to_string() })
}

fn fetch(builds: Vec<(u32, Option<String>)>, cached: Cached) -> u32 {
    let wanted: Vec<(u32, Option<String>)> = {
        let asked = lock(&ASKED);
        builds
            .into_iter()
            .filter(|(build, _)| !asked.contains(build))
            .filter(|(build, _)| cached == Cached::Replace || !wows_toolkit_replay_data::constants::is_cached(*build))
            .collect()
    };
    if wanted.is_empty() {
        return 0;
    }

    let mut written = 0;
    let mut answered = Vec::new();
    for (build, outcome) in wows_toolkit_replay_data::constants::fetch_for_builds(wanted, cached) {
        match outcome {
            Fetched::Written { build, actual } => {
                wows_toolkit_viewmodel::bug_report::record_constants_fetch(ConstantsFetchStatus::Succeeded);
                tracing::info!(build, actual, "constants: the result mapping was fetched");
                written += 1;
                answered.push(build);
            }
            Fetched::AlreadyOnDisk => answered.push(build),
            // Left unanswered: a repository that refused once is worth asking
            // again on a later listing.
            Fetched::Failed(reason) => {
                wows_toolkit_viewmodel::bug_report::record_constants_fetch(ConstantsFetchStatus::Failed(format!(
                    "build {build}: {reason}"
                )));
                tracing::warn!(build, %reason, "constants: the result mapping was not fetched");
            }
        }
    }
    lock(&ASKED).extend(answered);

    // A mapping that fits is on disk now, so a mismatch seen again is a new one.
    if written > 0 {
        lock(&MISMATCHED).clear();
    }

    written
}

/// Stores the newest-mapping commit as the nullable string the other front
/// ends read: absent means never checked.
fn save_commit(commit: Option<String>) -> Result<(), InspectorError> {
    settings::with_pool(|pool| async move {
        wows_toolkit_config::queries::set_setting(&pool, keys::CONSTANTS_FILE_COMMIT, &commit)
            .await
            .map_err(|source| InspectorError::SettingWrite { key: keys::CONSTANTS_FILE_COMMIT, source })
    })
}

/// A runtime for the repository client, which is async; the caller is on a
/// `blocking` thread.
fn runtime() -> Result<tokio::runtime::Runtime, InspectorError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| InspectorError::Runtime { source })
}

fn lock(set: &Mutex<BTreeSet<u32>>) -> std::sync::MutexGuard<'_, BTreeSet<u32>> {
    set.lock().unwrap_or_else(PoisonError::into_inner)
}
