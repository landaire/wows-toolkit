//! Reports a file dialog's failure over the window that asked for it.
//!
//! Every caller awaits a `wows_toolkit_services::dialog` future inside
//! `cx.spawn` through [`chosen`].

use std::path::PathBuf;

use gpui_kit::AppContext;
use gpui_kit::WeakEntity;
use rust_i18n::t;
use wows_toolkit_services::dialog::Answer;

/// Awaits a dialog's answer. A dialog that could not be opened is logged and
/// reported over `view`'s window, and answers `None` like a dismissed one.
pub async fn chosen<V, C>(asked: impl Future<Output = Answer>, view: &WeakEntity<V>, cx: &mut C) -> Option<PathBuf>
where
    V: 'static,
    C: AppContext,
{
    // A closure rather than a match: off Linux the error type is empty, and an
    // arm for it would be an unreachable pattern.
    asked.await.unwrap_or_else(|err| {
        tracing::error!("file dialog could not be opened: {err}");
        let said = t!("ui.dialogs.file_dialog_failed", reason = err).into_owned();
        let _ = view.update_in(cx, |_, window, cx| crate::toast::failed(said, window, cx));
        None
    })
}
