//! The chords the whole viewer takes: Ctrl+S opens and closes the options rail, Ctrl+Z undoes a
//! visibility change, and Ctrl+Shift+Z or Ctrl+R redoes one.

use zgui::view::EventCx;
use zgui::view::events;
use zgui::view::focus_is_text_entry;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;

use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerShortcut {
    ToggleOptions,
    Undo,
    Redo,
}

impl ViewerShortcut {
    pub fn from_key(key: &Key, modifiers: Modifiers) -> Option<Self> {
        if !modifiers.primary() {
            return None;
        }
        let Key::Character(text) = key else { return None };

        match (text.to_lowercase().as_str(), modifiers.shift()) {
            ("s", _) => Some(Self::ToggleOptions),
            ("z", false) => Some(Self::Undo),
            ("z", true) | ("r", _) => Some(Self::Redo),
            _ => None,
        }
    }

    /// Applies the chord to `viewport`. `toggle_options` opens or closes the rail.
    pub fn run(self, viewport: ArmorViewportState, toggle_options: impl FnOnce()) {
        match self {
            Self::ToggleOptions => toggle_options(),
            Self::Undo => viewport.undo_visibility(),
            Self::Redo => viewport.redo_visibility(),
        }
    }
}

/// Takes a chord pressed anywhere in the viewer for the active pane. A focused text field keeps
/// the chord for its own undo, and a focused viewport has taken it for itself before this runs.
pub fn take_viewer_shortcut(workspace: ArmorWorkspace, ev: &mut EventCx<'_, events::KeyDown>) {
    let Some(shortcut) = ViewerShortcut::from_key(&ev.key_without_modifiers, ev.modifiers) else { return };
    if focus_is_text_entry() {
        return;
    }

    if !ev.repeat {
        shortcut.run(workspace.active_untracked(), || workspace.toggle_options());
    }
    ev.prevent_default();
    ev.stop_propagation();
}
