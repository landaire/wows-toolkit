//! What a key means to a tree.

use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

/// Where a key moves the keyboard cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Travel {
    Previous,
    Next,
    PageUp,
    PageDown,
    First,
    Last,
}

/// What a key asks a tree to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Move(Travel),
    /// Expand the cursor's node, or step into it when it is open.
    Open,
    /// Collapse the cursor's node, or step out to its parent when it is closed.
    Close,
    Activate,
    /// Select the cursor's node, or with the primary modifier toggle it.
    Select,
    SelectAll,
    OpenMenu,
}

/// What `key` with `modifiers` held asks a tree to do, or `None` for a key the tree leaves to
/// whatever is around it.
pub(crate) fn command(key: &Key, modifiers: Modifiers) -> Option<Command> {
    if let Key::Character(text) = key {
        let select_all = modifiers.primary() && !modifiers.alt() && text.eq_ignore_ascii_case("a");
        return select_all.then_some(Command::SelectAll);
    }

    let Key::Named(named) = key else {
        return None;
    };

    let command = match named {
        NamedKey::ArrowUp => Command::Move(Travel::Previous),
        NamedKey::ArrowDown => Command::Move(Travel::Next),
        NamedKey::PageUp => Command::Move(Travel::PageUp),
        NamedKey::PageDown => Command::Move(Travel::PageDown),
        NamedKey::Home => Command::Move(Travel::First),
        NamedKey::End => Command::Move(Travel::Last),
        NamedKey::ArrowRight => Command::Open,
        NamedKey::ArrowLeft => Command::Close,
        NamedKey::Enter => Command::Activate,
        NamedKey::Space => Command::Select,
        NamedKey::ContextMenu => Command::OpenMenu,
        NamedKey::F10 if modifiers.shift() => Command::OpenMenu,
        _ => return None,
    };

    Some(command)
}
