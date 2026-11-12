//! Scheduling of the game-data preload and the replay parse on GPUI's
//! background executor. The work itself is `wows_toolkit_replay_data::load`.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::Task;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::heap_profile;
use wows_toolkit_replay_data::load::GameDataStatus;
use wows_toolkit_replay_data::load::ParsedReplay;
use wows_toolkit_replay_data::load::load_current_build;
use wows_toolkit_replay_data::load::parse_replay_with_alts;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

// Removed once armor viewer and replay renderer import from the shared crate.
pub use wows_toolkit_replay_data::load::GameDataCache;
pub use wows_toolkit_replay_data::load::LoadedGameData;

/// Kicks off the startup game-data preload on the background executor.
/// Callers await the returned task (typically via `cx.spawn`, to fold the
/// result back into an entity's state) rather than blocking on it.
pub fn spawn_startup_preload(wows_dir: PathBuf, game_data: GameDataCache, cx: &App) -> Task<GameDataStatus> {
    cx.background_spawn(async move {
        heap_profile::mark("startup_preload.begin");
        let preloaded = load_current_build(&wows_dir, &game_data);
        heap_profile::mark("startup_preload.done");
        match preloaded {
            Ok(loaded) => GameDataStatus::Ready(loaded),
            Err(reason) => {
                tracing::warn!(wows_dir = %wows_dir.display(), %reason, "startup game-data preload failed");
                GameDataStatus::Failed(reason)
            }
        }
    })
}

/// Parses `path` into a [`ParsedReplay`] on the background executor
/// (`cx.background_spawn`, not the tokio bridge -- this work is CPU-bound, not
/// async I/O). `game_data` lazily loads and caches each replay's own build's
/// data on first use.
pub fn spawn_parse(
    path: PathBuf,
    game_data: GameDataCache,
    personal_rating: Option<Arc<PersonalRatingData>>,
    cx: &App,
) -> Task<Result<ParsedReplay, ReplayLoadError>> {
    spawn_parse_with_alts(path, Vec::new(), game_data, personal_rating, cx)
}

/// The same, reading other recordings of the same battle alongside it.
pub fn spawn_parse_with_alts(
    path: PathBuf,
    alts: Vec<PathBuf>,
    game_data: GameDataCache,
    personal_rating: Option<Arc<PersonalRatingData>>,
    cx: &App,
) -> Task<Result<ParsedReplay, ReplayLoadError>> {
    cx.background_spawn(async move { parse_replay_with_alts(&path, &alts, &game_data, personal_rating.as_deref()) })
}

/// Says on the background executor whether `alt` is another recording of the
/// battle `primary` was read from (`wows_toolkit_replay_data::load::check_alt`).
pub fn check_alt(
    primary: PathBuf,
    alt: PathBuf,
    game_data: GameDataCache,
    cx: &App,
) -> Task<Result<(), ReplayLoadError>> {
    cx.background_spawn(async move { wows_toolkit_replay_data::load::check_alt(&primary, &alt, &game_data) })
}
