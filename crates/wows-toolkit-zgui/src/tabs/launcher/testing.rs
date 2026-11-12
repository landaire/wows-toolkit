//! Lets a test keep the tab from reading its settings and reach the launcher it shows, so the test
//! sets the state itself instead of the network, the keyring and this machine's installs.

use std::cell::Cell;
use std::cell::RefCell;

use crate::tabs::launcher::model::LauncherModel;

thread_local! {
    static HELD: Cell<bool> = const { Cell::new(false) };
    static MOUNTED: RefCell<Option<LauncherModel>> = const { RefCell::new(None) };
}

/// Keeps a tab built on this thread from reading its settings row and starting its work. It shows
/// its loading state until the test adopts settings.
pub fn hold_startup() {
    HELD.set(true);
}

pub(super) fn held() -> bool {
    HELD.get()
}

pub(super) fn mounted(model: LauncherModel) {
    MOUNTED.set(Some(model));
}

/// The launcher of the tab built last on this thread.
pub fn model() -> Option<LauncherModel> {
    MOUNTED.with_borrow(|model| *model)
}
