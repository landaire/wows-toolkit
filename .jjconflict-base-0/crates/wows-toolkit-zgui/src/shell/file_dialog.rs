//! Reports a file dialog's failure over the window that asked for it.

use std::path::PathBuf;

use rust_i18n::t;
use wows_toolkit_services::dialog::Answer;

use crate::shell::Shell;

impl Shell {
    /// Awaits a dialog's answer. A dialog that could not be opened is logged and reported as a
    /// message, and answers `None` like a dismissed one.
    pub async fn chosen_path(self, asked: impl Future<Output = Answer>) -> Option<PathBuf> {
        // A closure rather than a match: off Linux the error type is empty, and an arm for it would
        // be an unreachable pattern.
        asked.await.unwrap_or_else(|err| {
            tracing::error!("file dialog could not be opened: {err}");
            self.toasts().failed(t!("ui.dialogs.file_dialog_failed", reason = err).into_owned());
            None
        })
    }
}
