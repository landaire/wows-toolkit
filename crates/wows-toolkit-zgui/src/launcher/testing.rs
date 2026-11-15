//! Lets a test keep the launcher from reading its settings row, so the test sets the state itself
//! instead of the network, the keyring and this machine's installs.

use std::cell::Cell;

thread_local! {
    static HELD: Cell<bool> = const { Cell::new(false) };
}

/// Keeps a shell built on this thread from reading the launcher's settings row and starting its
/// work. The launcher stays unloaded until the test adopts settings.
pub fn hold_startup() {
    HELD.set(true);
}

/// Lets a shell built on this thread read its settings row as the app does. Shells that are not
/// launcher tests start here, so a hold left by an earlier test on the thread does not change what
/// they show.
pub fn release_startup() {
    HELD.set(false);
}

pub(super) fn held() -> bool {
    HELD.get()
}
