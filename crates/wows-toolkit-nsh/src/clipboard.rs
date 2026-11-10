//! Files put on the clipboard as a file list, so a file manager or chat client
//! pastes the files themselves rather than their paths.

use std::path::PathBuf;
use std::sync::PoisonError;
use std::sync::RwLock;

/// A clipboard that takes file lists in place of the system's.
pub trait FileClipboard: Send + Sync {
    fn copy_files(&self, files: &[PathBuf]) -> Result<(), arboard::Error>;
}

/// The clipboard copies go to. `None` is the system clipboard.
static REPLACEMENT: RwLock<Option<&'static dyn FileClipboard>> = RwLock::new(None);

/// Sends every later copy to `clipboard` instead of the system clipboard, for
/// a headless run that must not write the user's clipboard.
pub fn replace(clipboard: &'static dyn FileClipboard) {
    *REPLACEMENT.write().unwrap_or_else(PoisonError::into_inner) = Some(clipboard);
}

/// Puts `files` on the clipboard.
pub fn copy_files(files: &[PathBuf]) -> Result<(), arboard::Error> {
    let replacement = *REPLACEMENT.read().unwrap_or_else(PoisonError::into_inner);
    match replacement {
        Some(clipboard) => clipboard.copy_files(files),
        None => arboard::Clipboard::new().and_then(|mut board| board.set().file_list(files)),
    }
}
