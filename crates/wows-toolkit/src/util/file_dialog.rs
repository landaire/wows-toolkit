//! Native file dialogs that do not block the UI thread.
//!
//! A blocking rfd call inside the frame closure stops the event loop, so the
//! compositor marks the window unresponsive while the dialog is open. The
//! async builder is used because on macOS the sync dialogs must run on the
//! main thread, while the async variant marshals to the main run loop itself.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc;

use crate::util::thread::spawn_logged;

/// One file-type filter: the name the dialog shows for it, and the extensions
/// it accepts.
pub struct Filter {
    pub label: &'static str,
    pub extensions: &'static [&'static str],
}

impl Filter {
    pub fn apply(&self, dialog: rfd::AsyncFileDialog) -> rfd::AsyncFileDialog {
        dialog.add_filter(self.label, self.extensions)
    }
}

/// The game's replay files.
pub const REPLAYS: Filter = Filter { label: "WoWs Replays", extensions: &["wowsreplay"] };

/// Which dialog to present.
#[derive(Clone, Copy, Debug)]
pub enum DialogKind {
    PickFile,
    PickFolder,
    SaveFile,
}

/// Holds at most one open dialog plus caller context `C` that is returned
/// with the chosen path.
pub struct FileDialogSlot<C = ()> {
    pending: Option<PendingDialog<C>>,
}

impl<C> FileDialogSlot<C> {
    pub fn new() -> Self {
        Self { pending: None }
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Present `dialog`. Ignored while a dialog from this slot is still open.
    pub fn open(&mut self, ctx: &egui::Context, kind: DialogKind, dialog: rfd::AsyncFileDialog, context: C) {
        if self.pending.is_some() {
            return;
        }

        // The future is created here so platform setup runs on the UI thread;
        // only waiting for the user happens on the worker.
        let future: Pin<Box<dyn Future<Output = Option<rfd::FileHandle>> + Send>> = match kind {
            DialogKind::PickFile => Box::pin(dialog.pick_file()),
            DialogKind::PickFolder => Box::pin(dialog.pick_folder()),
            DialogKind::SaveFile => Box::pin(dialog.save_file()),
        };

        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        spawn_logged("file-dialog", move || {
            let picked = pollster::block_on(future).map(|handle| handle.path().to_path_buf());
            let _ = sender.send(picked);
            ctx.request_repaint();
        });

        self.pending = Some(PendingDialog { context, receiver });
    }

    /// Return the chosen path once the dialog closes. Clears the slot when the
    /// dialog closes for any reason.
    pub fn poll(&mut self) -> Option<(C, PathBuf)> {
        let pending = self.pending.as_ref()?;

        let picked = match pending.receiver.try_recv() {
            Ok(picked) => picked,
            Err(mpsc::TryRecvError::Empty) => return None,
            // Worker panicked; spawn_logged already logged it.
            Err(mpsc::TryRecvError::Disconnected) => None,
        };

        let pending = self.pending.take()?;

        picked.map(|path| (pending.context, path))
    }
}

impl FileDialogSlot<()> {
    pub fn poll_path(&mut self) -> Option<PathBuf> {
        self.poll().map(|((), path)| path)
    }
}

impl<C> Default for FileDialogSlot<C> {
    fn default() -> Self {
        Self::new()
    }
}

struct PendingDialog<C> {
    context: C,
    receiver: mpsc::Receiver<Option<PathBuf>>,
}
