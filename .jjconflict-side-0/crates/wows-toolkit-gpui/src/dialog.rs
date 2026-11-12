//! Native file dialogs, opened without stopping the app.
//!
//! On Windows and macOS rfd's blocking dialogs pump a modal message loop on the
//! thread that opens them. Opened from a gpui handler -- which always runs
//! inside an `App` update -- that loop runs gpui's own foreground tasks
//! re-entrantly, and the first one to touch the app (the text cursor's blink
//! timer, in practice) panics with "RefCell already borrowed" inside a Windows
//! callback, where a panic cannot unwind and takes the process with it. The
//! async dialogs run the picker on a thread of their own and hand the result
//! back through a future, so the handler returns at once and the app keeps
//! drawing.
//!
//! On Linux the dialog is the desktop's file chooser portal. rfd answers a
//! portal failure by falling back to zenity, and a zenity failure as a cancel,
//! so a dialog that never opened looked like one the reader closed. Here a
//! failure carries its reason, and zenity is only asked when there is no
//! portal to ask: no session bus, or no file chooser on it.
//!
//! Every caller awaits one of these inside `cx.spawn` through [`chosen`].

#[cfg(all(not(target_os = "linux"), not(test)))]
mod native;
#[cfg(all(target_os = "linux", not(test)))]
mod portal;
#[cfg(all(target_os = "linux", not(test)))]
mod zenity;

use std::path::Path;
use std::path::PathBuf;

use gpui_kit::AppContext;
use gpui_kit::WeakEntity;
use rust_i18n::t;

/// One file-type filter: the name the dialog shows for it, and the extensions
/// it accepts.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(test, expect(dead_code, reason = "tests answer a dialog without opening it"))]
pub struct Filter {
    pub label: &'static str,
    pub extensions: &'static [&'static str],
}

/// The game's replay files.
pub const REPLAYS: Filter = Filter { label: "WoWs Replays", extensions: &["wowsreplay"] };

/// An exported 3D model.
pub const GLB: Filter = Filter { label: "glTF Binary", extensions: &["glb"] };

/// The kind of dialog asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    File,
    Folder,
    Save { file_name: String },
}

/// Why a dialog could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum DialogError {
    #[cfg(all(target_os = "linux", not(test)))]
    #[error(transparent)]
    Portal(portal::PortalError),

    #[cfg(all(target_os = "linux", not(test)))]
    #[error("{portal}, and zenity failed ({zenity})")]
    Unavailable { portal: portal::PortalError, zenity: zenity::ZenityError },

    #[cfg(test)]
    #[error("the test refused the dialog")]
    Refused,
}

/// A dialog's answer: the path picked, or `None` when the reader dismissed it.
pub type Answer = Result<Option<PathBuf>, DialogError>;

/// Everything one dialog is asked with.
#[cfg_attr(test, expect(dead_code, reason = "tests answer a dialog without opening it"))]
struct Request {
    asked: Asked,
    title: Option<String>,
    filter: Option<Filter>,
    start: Option<PathBuf>,
}

/// Asks for one existing file.
pub fn pick_file(title: Option<&str>, filter: Option<Filter>) -> impl Future<Output = Answer> + use<> {
    ask(Request { asked: Asked::File, title: title.map(str::to_owned), filter, start: None })
}

/// Asks for one directory.
pub fn pick_folder(title: &str) -> impl Future<Output = Answer> + use<> {
    ask(Request { asked: Asked::Folder, title: Some(title.to_owned()), filter: None, start: None })
}

/// Asks for one directory, starting in `start`.
pub fn pick_folder_in(title: &str, start: &Path) -> impl Future<Output = Answer> + use<> {
    ask(Request { asked: Asked::Folder, title: Some(title.to_owned()), filter: None, start: Some(start.to_owned()) })
}

/// Asks where to write a new file.
pub fn save_file(title: Option<&str>, file_name: &str, filter: Option<Filter>) -> impl Future<Output = Answer> + use<> {
    ask(Request {
        asked: Asked::Save { file_name: file_name.to_owned() },
        title: title.map(str::to_owned),
        filter,
        start: None,
    })
}

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

#[cfg(all(target_os = "linux", not(test)))]
async fn ask(request: Request) -> Answer {
    let portal = match portal::ask(&request).await {
        Ok(chosen) => return Ok(chosen),
        Err(portal) => portal,
    };
    if !portal.is_missing() {
        return Err(DialogError::Portal(portal));
    }

    tracing::warn!("file dialog: {portal}, so zenity is asked instead");
    zenity::ask(&request).await.map_err(|zenity| DialogError::Unavailable { portal, zenity })
}

#[cfg(all(not(target_os = "linux"), not(test)))]
use native::ask;

#[cfg(test)]
thread_local! {
    static ASKED: std::cell::RefCell<Vec<Asked>> = const { std::cell::RefCell::new(Vec::new()) };
    static REFUSE_NEXT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The dialogs asked for on this thread since the last call.
#[cfg(test)]
pub fn take_asked() -> Vec<Asked> {
    ASKED.with_borrow_mut(std::mem::take)
}

/// Makes the next dialog asked for on this thread fail to open.
#[cfg(test)]
pub fn refuse_next() {
    REFUSE_NEXT.set(true);
}

/// Tests run headless, so the dialog is recorded and answered as dismissed, or
/// as failed after [`refuse_next`], instead of opened.
#[cfg(test)]
fn ask(request: Request) -> impl Future<Output = Answer> + use<> {
    ASKED.with_borrow_mut(|asked| asked.push(request.asked));
    let answer = if REFUSE_NEXT.take() { Err(DialogError::Refused) } else { Ok(None) };
    std::future::ready(answer)
}
