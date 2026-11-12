//! The command palette: everything the window can do, by name.

use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::view::IntoView;

use crate::shell::Shell;

/// Whether the palette is open.
#[derive(Clone, Copy)]
pub struct PaletteControl {
    open: RwSignal<bool, LocalStorage>,
}

impl PaletteControl {
    pub fn new() -> Self {
        Self { open: RwSignal::new_local(false) }
    }

    pub fn is_open(self) -> bool {
        self.open.get()
    }

    /// Opens the palette on its root list.
    pub fn open_root(self) {
        self.open.set(true);
    }

    pub fn close(self) {
        self.open.set(false);
    }
}

impl Default for PaletteControl {
    fn default() -> Self {
        Self::new()
    }
}

/// The palette's dialog, while it is open.
#[component]
pub fn CommandPalette(shell: Shell) -> impl IntoView {
    let _ = shell;
}
