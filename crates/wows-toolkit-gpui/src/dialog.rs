//! Native file dialogs, opened without stopping the app.
//!
//! rfd's blocking dialogs pump a modal message loop on the thread that opens
//! them. Opened from a gpui handler -- which always runs inside an `App`
//! update -- that loop runs gpui's own foreground tasks re-entrantly, and the
//! first one to touch the app (the text cursor's blink timer, in practice)
//! panics with "RefCell already borrowed" inside a Windows callback, where a
//! panic cannot unwind and takes the process with it. It also freezes the rest
//! of the window for as long as the dialog is open.
//!
//! The async dialogs run the picker on a thread of their own and hand the
//! result back through a future, so the handler returns at once and the app
//! keeps drawing. Every caller awaits one of these inside `cx.spawn`.

use std::future::Future;
use std::path::PathBuf;

/// One file-type filter: the name the dialog shows for it, and the extensions
/// it accepts.
pub struct Filter {
    pub label: &'static str,
    pub extensions: &'static [&'static str],
}

/// The game's replay files.
pub const REPLAYS: Filter = Filter { label: "WoWs Replays", extensions: &["wowsreplay"] };

/// An exported 3D model.
pub const GLB: Filter = Filter { label: "glTF Binary", extensions: &["glb"] };

/// Asks for one existing file.
#[cfg(not(test))]
pub fn pick_file(title: Option<&str>, filter: Option<Filter>) -> impl Future<Output = Option<PathBuf>> + use<> {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    if let Some(filter) = filter {
        dialog = dialog.add_filter(filter.label, filter.extensions);
    }
    let picked = dialog.pick_file();
    async move { picked.await.map(|handle| handle.path().to_path_buf()) }
}

/// Asks for one directory.
#[cfg(not(test))]
pub fn pick_folder(title: &str) -> impl Future<Output = Option<PathBuf>> + use<> {
    let picked = rfd::AsyncFileDialog::new().set_title(title).pick_folder();
    async move { picked.await.map(|handle| handle.path().to_path_buf()) }
}

/// A dialog a test asked for. Tests run headless, so the picker is recorded
/// and answered as cancelled instead of opened.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    File,
    Folder,
}

#[cfg(test)]
thread_local! {
    static ASKED: std::cell::RefCell<Vec<Asked>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The dialogs asked for on this thread since the last call.
#[cfg(test)]
pub fn take_asked() -> Vec<Asked> {
    ASKED.with_borrow_mut(std::mem::take)
}

#[cfg(test)]
pub fn pick_file(_title: Option<&str>, _filter: Option<Filter>) -> impl Future<Output = Option<PathBuf>> + use<> {
    ASKED.with_borrow_mut(|asked| asked.push(Asked::File));
    std::future::ready(None)
}

#[cfg(test)]
pub fn pick_folder(_title: &str) -> impl Future<Output = Option<PathBuf>> + use<> {
    ASKED.with_borrow_mut(|asked| asked.push(Asked::Folder));
    std::future::ready(None)
}

/// Asks for one directory, starting in `start`.
pub fn pick_folder_in(title: &str, start: &std::path::Path) -> impl Future<Output = Option<PathBuf>> + use<> {
    let picked = rfd::AsyncFileDialog::new().set_title(title).set_directory(start).pick_folder();
    async move { picked.await.map(|handle| handle.path().to_path_buf()) }
}

/// Asks where to write a new file.
pub fn save_file(
    title: Option<&str>,
    file_name: &str,
    filter: Option<Filter>,
) -> impl Future<Output = Option<PathBuf>> + use<> {
    let mut dialog = rfd::AsyncFileDialog::new().set_file_name(file_name);
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    if let Some(filter) = filter {
        dialog = dialog.add_filter(filter.label, filter.extensions);
    }
    let picked = dialog.save_file();
    async move { picked.await.map(|handle| handle.path().to_path_buf()) }
}
