//! Rendering several battles to video in one background batch, into a directory the reader picks
//! or onto the clipboard.

use std::path::PathBuf;

use futures::StreamExt as _;
use futures::channel::mpsc;
use rust_i18n::t;
use wows_minimap_renderer::SavedRenderOptions;
use wows_toolkit_replay_data::replay_render::export::BatchOutcome;
use wows_toolkit_replay_data::replay_render::export::batch_export;
use wows_toolkit_replay_data::replay_render::export::temporary_batch_dir;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::services::Job;
use crate::services::Progress;
use crate::shell::palette::chosen;
use crate::tabs::replays::workspace::ReplayWorkspace;

/// Where a batch render's files go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchTarget {
    /// A directory the reader is asked for when the batch starts.
    Directory,
    /// A temporary directory, whose files go on the clipboard afterwards. It is not removed: the
    /// clipboard holds paths into it until the reader pastes them.
    Clipboard,
}

/// Renders each of `paths` to a video, saying which replay it has reached and what it managed.
pub fn render_many(workspace: ReplayWorkspace, paths: Vec<PathBuf>, target: BatchTarget) {
    if paths.is_empty() {
        return;
    }
    let shell = workspace.shell;
    let services = workspace.services;
    let Some(game_data) = services.game_data.cache_untracked() else { return };
    let toasts = shell.toasts();

    workspace.under_tab(|| {
        spawn_local(async move {
            let out_dir = match target {
                BatchTarget::Directory => {
                    let asked = wows_toolkit_services::dialog::pick_folder(&t!("ui.replay.renderer.export_video"));
                    match chosen(shell, asked).await {
                        Some(picked) => picked,
                        None => return,
                    }
                }
                BatchTarget::Clipboard => match temporary_batch_dir() {
                    Some(temporary) => temporary,
                    None => {
                        toasts.failed(t!("ui.replay.renderer.export_path_unusable").into_owned());
                        return;
                    }
                },
            };
            let count = paths.len();
            let shown = out_dir.display().to_string();
            toasts.info(t!("ui.replay.renderer.batch_started", count = count, dir = shown.clone()).into_owned());

            // What a viewport would have shown, so a batch does not write a different video than
            // the reader has been watching.
            let defaults = match services.settings.pool() {
                Some(pool) => services
                    .on_tokio(async move { wows_toolkit_replay_data::render_defaults::load(&pool).await })
                    .await
                    .unwrap_or_default(),
                None => SavedRenderOptions::default(),
            };

            let job = services.jobs.start(Job {
                name: t!("ui.replay.renderer.batch_progress", done = 1, total = count, replay = "").into_owned(),
                progress: None,
            });
            let (report, mut steps) = mpsc::unbounded();
            let batch = blocking(move || {
                batch_export(paths, &game_data, &out_dir, &defaults, |step| {
                    let _ = report.unbounded_send(step);
                })
            });
            let watch = async {
                while let Some(step) = steps.next().await {
                    job.report(Job {
                        name: t!(
                            "ui.replay.renderer.batch_progress",
                            done = step.done + 1,
                            total = step.total,
                            replay = step.replay
                        )
                        .into_owned(),
                        progress: Progress::new(step.done as u64, step.total as u64),
                    });
                }
            };
            let (BatchOutcome { written, failed }, ()) = futures::future::join(batch, watch).await;
            drop(job);

            let to_clipboard = target == BatchTarget::Clipboard;
            let copied = if to_clipboard && !written.is_empty() {
                arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.set().file_list(&written))
                    .map_err(|err| err.to_string())
            } else {
                Ok(())
            };
            // The files were written either way, so a refused clipboard says where they are.
            let said = match (&copied, failed.is_empty(), to_clipboard) {
                (Err(reason), _, _) => {
                    t!("ui.replay.renderer.batch_not_copied", dir = shown, reason = reason.clone()).into_owned()
                }
                (Ok(()), true, true) => t!("ui.replay.renderer.batch_copied", written = written.len()).into_owned(),
                (Ok(()), true, false) => {
                    t!("ui.replay.renderer.batch_all_written", written = written.len()).into_owned()
                }
                (Ok(()), false, _) => {
                    t!("ui.replay.renderer.batch_finished", written = written.len(), failed = failed.len()).into_owned()
                }
            };
            if failed.is_empty() && copied.is_ok() {
                toasts.ok(said);
            } else {
                toasts.warn(said);
            }
        });
    });
}
