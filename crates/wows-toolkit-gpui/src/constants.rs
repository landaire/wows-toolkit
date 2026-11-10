//! The result-mapping fetches on GPUI's executors. The work itself is
//! `wows_toolkit_replay_data::constants`.

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;
use wows_toolkit_replay_data::constants::Cached;
use wows_toolkit_replay_data::constants::Checked;
use wows_toolkit_replay_data::constants::Fetched;

/// Asks whether the published mapping has moved since `known_commit`, and writes
/// it for `build` when it has (see `wows_toolkit_replay_data::constants::check_latest`).
pub fn check_latest(build: u32, known_commit: Option<String>, cx: &App) -> Task<Checked> {
    let runtime = crate::runtime::runtime(cx);

    cx.background_spawn(async move {
        let Some(runtime) = runtime else { return Checked::Failed("no runtime to fetch on".to_owned()) };
        runtime.block_on(wows_toolkit_replay_data::constants::check_latest(build, known_commit))
    })
}

/// Fetches the mappings for builds that have none on the background executor
/// (see `wows_toolkit_replay_data::constants::fetch_for_builds`).
pub fn fetch_for_builds(wanted: Vec<(u32, Option<String>)>, cached: Cached, cx: &App) -> Task<Vec<(u32, Fetched)>> {
    cx.background_spawn(async move { wows_toolkit_replay_data::constants::fetch_for_builds(wanted, cached) })
}
