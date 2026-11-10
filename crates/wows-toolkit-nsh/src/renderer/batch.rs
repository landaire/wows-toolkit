//! Rendering many replays to video without opening a viewport for any of
//! them: the listing's "Render N Replays to Video" and "Render to Clipboard".

use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;

use nsh_core::prelude::*;
use rust_i18n::t;
use tokio::sync::watch;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::playback;
use wows_toolkit_replay_data::playback::BatchOutcome;
use wows_toolkit_replay_data::playback::BatchStep;

use crate::renderer::ids::Revision;

/// Where a batch writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchTarget {
    /// A folder the reader chose.
    Directory,
    /// A temporary folder, whose files go on the clipboard.
    Clipboard,
}

/// The batch render, as its progress line reads it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct BatchView {
    /// Changes whenever anything here does.
    pub revision: Revision,
    pub running: bool,
    /// What the line says: which replay of how many, or how it ended.
    pub text: Option<String>,
    /// Whether the last batch ended with failures or an unwritable clipboard.
    pub failed: bool,
}

#[derive(Clone, Debug, Default)]
struct BatchState {
    revision: Revision,
    running: bool,
    text: Option<String>,
    failed: bool,
}

static BATCH: LazyLock<watch::Sender<BatchState>> = LazyLock::new(|| watch::channel(BatchState::default()).0);

fn publish(edit: impl FnOnce(&mut BatchState)) {
    BATCH.send_modify(|state| {
        edit(state);
        state.revision = state.revision.next();
    });
}

/// What the progress line reads now.
pub fn view() -> BatchView {
    let state = BATCH.borrow();
    BatchView { revision: state.revision, running: state.running, text: state.text.clone(), failed: state.failed }
}

/// Resolves with the progress line once it differs from `after`.
pub async fn next_change(after: Revision) -> BatchView {
    let mut changes = BATCH.subscribe();
    // The sender lives for the process, so the wait only ends on a change.
    let _ = changes.wait_for(|state| state.revision != after).await;
    view()
}

/// Whether a batch is running; only one runs at a time, since each encode
/// holds the GPU.
pub fn is_running() -> bool {
    BATCH.borrow().running
}

/// Renders `paths` into `out_dir` against `game_data` on its own thread,
/// saying which replay it has reached and what it managed in the end.
pub fn start(
    paths: Vec<PathBuf>,
    game_data: GameDataCache,
    out_dir: PathBuf,
    target: BatchTarget,
    defaults: wows_minimap_renderer::SavedRenderOptions,
) {
    let count = paths.len();
    let shown = out_dir.display().to_string();
    publish(|state| {
        state.running = true;
        state.failed = false;
        state.text = Some(t!("ui.replay.renderer.batch_started", count = count, dir = shown).into_owned());
    });

    std::thread::spawn(move || {
        // Nothing stops a batch once it is under way, as in the other apps.
        let cancel = AtomicBool::new(false);
        let outcome = playback::batch_export(paths, &game_data, &out_dir, &defaults, &cancel, |step: BatchStep| {
            publish(|state| {
                state.text = Some(
                    t!(
                        "ui.replay.renderer.batch_progress",
                        done = step.done + 1,
                        total = step.total,
                        replay = step.replay
                    )
                    .into_owned(),
                );
            });
        });
        let (said, failed) = summary(&outcome, &out_dir, target);
        publish(|state| {
            state.running = false;
            state.failed = failed;
            state.text = Some(said);
        });
    });
}

/// What a finished batch says, and whether it reads as a failure.
fn summary(outcome: &BatchOutcome, out_dir: &Path, target: BatchTarget) -> (String, bool) {
    let written = outcome.written.len();
    let failed = outcome.failed.len();
    if target == BatchTarget::Clipboard && written > 0 {
        return match crate::clipboard::copy_files(&outcome.written) {
            Ok(()) => (t!("ui.replay.renderer.batch_copied", written = written).into_owned(), failed > 0),
            Err(err) => (
                t!(
                    "ui.replay.renderer.batch_not_copied",
                    dir = out_dir.display().to_string(),
                    reason = err.to_string()
                )
                .into_owned(),
                true,
            ),
        };
    }
    if failed == 0 {
        return (t!("ui.replay.renderer.batch_all_written", written = written).into_owned(), false);
    }

    (t!("ui.replay.renderer.batch_finished", written = written, failed = failed).into_owned(), true)
}
