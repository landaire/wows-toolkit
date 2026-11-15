//! Opening a replay in the game through the launcher: from an install that has the replay's
//! build, through the runner, and never while a game runs.

use std::path::Path;
use std::path::PathBuf;

use rust_i18n::t;
use wows_toolkit_launcher::GameBuild;
use wows_toolkit_launcher::ReplayLaunchError;
use zgui::task::spawn_detached;

use crate::launcher::model::LauncherModel;
use crate::long_job::long_job;
use crate::shell::Shell;
use crate::shell::palette::chosen;

/// Asks for a replay file and opens it in the game.
pub fn pick_and_open(shell: Shell, model: LauncherModel) {
    let asked = wows_toolkit_services::dialog::pick_file(None, Some(wows_toolkit_services::dialog::REPLAYS));
    // Detached: the popover or palette that asked has closed before the reader answers.
    spawn_detached(async move {
        let Some(replay) = chosen(shell, asked).await else { return };
        if shell.is_unmounted() {
            return;
        }
        open(shell, model, replay);
    });
}

/// Opens `replay` in the game, saying in a message why it could not.
pub fn open(shell: Shell, model: LauncherModel, replay: PathBuf) {
    spawn_detached(async move {
        let read = replay.clone();
        let build = long_job(move || replay_build(&read)).await;
        if shell.is_unmounted() {
            return;
        }

        let wows_dir = model.wows_dir_untracked();
        let opened = model.run(move |launcher, settings, env| {
            let opened = launcher.open_replay(replay, build, wows_dir.as_deref(), settings, env, |command| {
                wows_toolkit_services::child_process::prepare(command);
            });
            (opened, false)
        });
        let Some(Err(err)) = opened else { return };

        let message = match &err {
            ReplayLaunchError::NoInstallForBuild { build } => {
                t!("ui.launcher.replay.no_install", build = build).into_owned()
            }
            _ => t!("ui.launcher.launch_failed", error = err.to_string()).into_owned(),
        };
        shell.toasts().failed(message);
    });
}

/// The build `replay` was recorded with, from the `clientVersionFromExe` of its header, which
/// reads `major, minor, patch, build`. `None` when the header cannot be read.
fn replay_build(replay: &Path) -> Option<GameBuild> {
    let meta = match wows_replays::ReplayFile::meta_from_file(replay) {
        Ok(meta) => meta,
        Err(err) => {
            tracing::warn!("could not read the header of {}: {err}", replay.display());
            return None;
        }
    };

    let build = meta.clientVersionFromExe.rsplit(',').next()?.trim().parse().ok()?;
    Some(GameBuild::new(build))
}
