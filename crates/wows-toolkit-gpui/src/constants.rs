//! Keeping the result mappings up to date.
//!
//! Post-battle results are read through a per-build mapping published in
//! `padtrack/wows-constants`, cached as `constants_{build}.json` under the shared
//! storage directory. Reading that file is `replay_inspector::load`; what is here
//! is everything that puts it there: the check for a newer published mapping, the
//! fetch of a build's own mapping, and the reader's own file when they have one
//! the repository does not.
//!
//! The egui app does the same three things from its networking thread
//! (`task/networking.rs`'s `FetchLatestConstants` and `FetchVersionedConstants`,
//! and `app.rs`'s import), through the same commit row, so a fetch by either app
//! serves both.

use std::path::Path;
use std::path::PathBuf;

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;

/// What a check of the published constants came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// A newer mapping was written for `build`, at this commit.
    Written { build: u32, commit: Option<String> },
    /// The repository is where it was when the mapping was last written.
    UpToDate,
    /// The repository could not be asked, or what it gave could not be written.
    Failed(String),
}

/// What a fetch of one build's mapping came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// The mapping for `build` is now on disk. `actual` is the build it was
    /// published under, which differs when a version fallback served it.
    Written {
        build: u32,
        actual: u32,
    },
    /// It was already there, so nothing was fetched.
    AlreadyOnDisk,
    Failed(String),
}

/// Where a build's mapping is cached.
///
/// `None` when there is no storage directory at all, which is the one case
/// nothing can be cached anywhere.
pub fn cached_path(build: u32) -> Option<PathBuf> {
    Some(wows_toolkit_config::storage_dir()?.join(format!("constants_{build}.json")))
}

/// Whether this build's mapping is already cached.
pub fn is_cached(build: u32) -> bool {
    cached_path(build).is_some_and(|path| path.exists())
}

/// Asks whether the published mapping has moved since `known_commit`, and writes
/// it for `build` when it has.
///
/// `build` is the installed one, which is what the newest published mapping is
/// for; an older replay's own build is fetched by [`fetch_for_build`] instead.
pub fn check_latest(build: u32, known_commit: Option<String>, cx: &App) -> Task<Checked> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return Checked::Failed("no runtime to fetch on".to_owned()) };
        runtime.block_on(async move {
            match wows_data_mgr::constants::fetch_latest_constants(known_commit.as_deref()).await {
                Ok(Some(latest)) => match write_cached(build, &latest.data) {
                    Ok(()) => Checked::Written { build, commit: latest.commit },
                    Err(err) => Checked::Failed(err),
                },
                Ok(None) => Checked::UpToDate,
                Err(err) => Checked::Failed(err.to_string()),
            }
        })
    })
}

/// Fetches the mapping for one replay's own build, when nothing has it yet.
///
/// `version` is the replay's `major.minor.patch`, which is what resolves a
/// mapping across servers when the build numbers differ.
pub fn fetch_for_build(build: u32, version: Option<String>, cx: &App) -> Task<Fetched> {
    if is_cached(build) {
        return Task::ready(Fetched::AlreadyOnDisk);
    }
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return Fetched::Failed("no runtime to fetch on".to_owned()) };
        runtime.block_on(async move {
            match wows_data_mgr::constants::fetch_versioned_constants(build, version.as_deref()).await {
                // Written under the build that asked, so the next read of this
                // replay finds it whichever build it was published under.
                Ok((data, actual)) => match serde_json::to_vec(&data) {
                    Ok(bytes) => match write_cached(build, &bytes) {
                        Ok(()) => Fetched::Written { build, actual },
                        Err(err) => Fetched::Failed(err),
                    },
                    Err(err) => Fetched::Failed(err.to_string()),
                },
                Err(err) => Fetched::Failed(err.to_string()),
            }
        })
    })
}

/// Takes a `constants.json` the reader points at as `build`'s mapping.
///
/// For a build the repository does not publish: the file is read and parsed
/// before anything is written, so a file that is not a mapping cannot replace one
/// that is.
pub fn import(from: &Path, build: u32) -> Result<(), String> {
    let bytes = std::fs::read(from).map_err(|err| err.to_string())?;
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
    if !parsed.is_object() {
        return Err("that file is not a constants mapping".to_owned());
    }
    write_cached(build, &bytes)
}

/// Writes `bytes` as `build`'s cached mapping.
fn write_cached(build: u32, bytes: &[u8]) -> Result<(), String> {
    let Some(path) = cached_path(build) else {
        return Err("there is no storage directory to cache constants in".to_owned());
    };
    std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    /// A file that is not a mapping does not replace one that is.
    #[test]
    fn an_import_of_something_else_is_refused() {
        let dir = tempfile::tempdir().expect("a temp directory");
        let path = dir.path().join("constants.json");
        std::fs::write(&path, b"[1, 2, 3]").expect("the file is written");

        assert!(super::import(&path, 7062104).is_err(), "a list is not a mapping");

        std::fs::write(&path, b"not json at all").expect("the file is written");
        assert!(super::import(&path, 7062104).is_err());
    }
}
