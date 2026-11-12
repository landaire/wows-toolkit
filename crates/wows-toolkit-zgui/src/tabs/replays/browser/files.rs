//! What a replay row's menu does with the files themselves. Public for the other lists of
//! replays, such as the search results.

use std::path::Path;
use std::path::PathBuf;

use landaire_ui::Toasts;
use rust_i18n::t;

use crate::shell::Shell;

/// Puts the paths on the clipboard as text, one per line.
pub fn copy_paths(shell: Shell, paths: &[PathBuf]) {
    let text = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
    shell.copy_text(text);
}

/// Puts the replay files themselves on the clipboard, so they paste into a file manager or an
/// upload dialog rather than as their paths.
///
/// zgui's clipboard writes text only, so this goes through `arboard`, as the other front ends do.
pub fn copy_replay_files(toasts: Toasts, paths: &[PathBuf]) {
    let outcome = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set().file_list(paths));
    match outcome {
        Ok(()) => {
            let message = if paths.len() == 1 {
                t!("ui.replay.context.copy_replay").into_owned()
            } else {
                t!("ui.replay.context.copy_replays", count = paths.len()).into_owned()
            };
            toasts.ok(message);
        }
        Err(err) => {
            tracing::warn!("the replay files could not be copied: {err}");
            toasts.failed(err.to_string());
        }
    }
}

/// Asks the game in `wows_dir` to play `replay`, saying so when it cannot be started.
pub fn open_replay_in_game(toasts: Toasts, wows_dir: &Path, replay: &Path) {
    let opened = wows_toolkit_launcher::open_replay_in_game(wows_dir, replay.to_path_buf(), |command| {
        wows_toolkit_services::child_process::prepare(command);
    });
    if let Err(err) = opened {
        tracing::warn!("the game in {} could not be launched: {err}", wows_dir.display());
        toasts.failed(err.to_string());
    }
}
