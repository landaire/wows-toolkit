//! Recording listed battles in the Stats tab's session.

use std::path::PathBuf;

use wows_toolkit_config::queries;
use wows_toolkit_replay_data::load::parse_replay;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::tabs::replays::workspace::ReplayWorkspace;

/// Records `paths` in the session, each read as opening it would: a session row's figures come
/// from the battle results, which are only in the file. A replay that will not read is skipped.
/// `replace` forgets what is recorded first.
pub fn record(workspace: ReplayWorkspace, paths: Vec<PathBuf>, replace: bool) {
    let services = workspace.services;
    let Some(game_data) = services.game_data.cache_untracked() else {
        tracing::warn!("session stats: asked for before the game directory was known");
        return;
    };
    let Some(pool) = services.settings.pool() else { return };

    workspace.under_tab(|| {
        spawn_local(async move {
            if replace {
                let clearing = pool.clone();
                let cleared = services.on_tokio(async move { queries::clear_session_stats(&clearing).await }).await;
                match cleared {
                    Ok(Ok(_)) => {}
                    Ok(Err(err)) => {
                        tracing::warn!("session stats: the session could not be cleared: {err}");
                        return;
                    }
                    Err(err) => {
                        tracing::warn!("session stats: the clear did not complete: {err}");
                        return;
                    }
                }
            }

            for path in paths {
                let (reading, cache) = (path.clone(), game_data.clone());
                let stat = match blocking(move || parse_replay(&reading, &cache, None)).await {
                    Ok(parsed) => parsed.session_stat,
                    Err(err) => {
                        tracing::warn!(path = %path.display(), "session stats: the replay would not parse: {err}");
                        continue;
                    }
                };
                let Some(stat) = stat else {
                    tracing::warn!(path = %path.display(), "session stats: the replay names no recording player");
                    continue;
                };
                let row = stat.to_row();
                let writing = pool.clone();
                let written = services.on_tokio(async move { queries::add_session_stat(&writing, &row).await }).await;
                match written {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => tracing::warn!("session stats: the battle could not be recorded: {err}"),
                    Err(err) => tracing::warn!("session stats: the write did not complete: {err}"),
                }
            }
        });
    });
}
