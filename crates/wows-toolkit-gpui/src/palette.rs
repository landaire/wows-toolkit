//! The command palette: everything the window can do, by name.
//!
//! Ports the egui app's `ui/command_palette.rs`, which opens on ctrl+k or
//! ctrl+p over a fuzzy-matched list. What is offered here is the flat root set --
//! going to a tab, setting the theme, opening a replay, the seeded searches.
//! The egui palette's cascading sub-modes (search a player, a ship, a ship's
//! armor) are not here yet; they need a bounded index query per keystroke.
//!
//! One list, in one order, built once: `Command` filters it itself, so this
//! only says what exists and what each entry does.

use gpui_kit::component::command::CommandItem;
use rust_i18n::t;

use crate::app::AppTab;
use wows_toolkit_viewmodel::settings::ThemeChoice;

/// What confirming an entry asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    GoTo(AppTab),
    SetTheme(ThemeChoice),
    OpenReplayFile,
    /// Put a query in the Search tab's bar and run it.
    SearchFor(&'static str),
    /// Put the newest log file on the clipboard, for a bug report.
    CopyLatestLog,
}

/// One entry: what it is called and what it does.
pub struct PaletteEntry {
    pub label: String,
    pub action: PaletteAction,
}

/// Every entry the palette offers, in the order it lists them.
///
/// Tabs first because going somewhere is the common reason to open it, then
/// the searches, then the settings-like actions.
pub fn entries() -> Vec<PaletteEntry> {
    let mut entries = Vec::new();

    for tab in AppTab::ALL {
        entries.push(PaletteEntry {
            label: t!("ui.palette.go_to", tab = t!(tab.label_key())).into_owned(),
            action: PaletteAction::GoTo(tab),
        });
    }

    // The two seeded searches the egui palette offers. The queries are the
    // grammar the Search tab parses, so confirming one fills the bar with
    // something the reader can then edit.
    entries.push(PaletteEntry {
        label: t!("ui.palette.games_i_died_in").into_owned(),
        action: PaletteAction::SearchFor("survived:false"),
    });
    entries.push(PaletteEntry {
        label: t!("ui.palette.games_i_won").into_owned(),
        action: PaletteAction::SearchFor("outcome:win"),
    });

    entries.push(PaletteEntry {
        label: t!("ui.replay.open_manually").into_owned(),
        action: PaletteAction::OpenReplayFile,
    });
    entries.push(PaletteEntry {
        label: t!("ui.replay.copy_latest_log").into_owned(),
        action: PaletteAction::CopyLatestLog,
    });

    for (key, choice) in [
        ("ui.settings.app.theme_system", ThemeChoice::System),
        ("ui.settings.app.theme_dark", ThemeChoice::Dark),
        ("ui.settings.app.theme_light", ThemeChoice::Light),
    ] {
        entries.push(PaletteEntry {
            label: t!("ui.palette.set_theme", theme = t!(key)).into_owned(),
            action: PaletteAction::SetTheme(choice),
        });
    }

    entries
}

/// The entries as the component's own items, in the same order, so an index
/// into one is an index into the other.
pub fn items(entries: &[PaletteEntry]) -> Vec<CommandItem> {
    entries.iter().map(|entry| CommandItem::new().label(entry.label.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::PaletteAction;
    use super::entries;
    use crate::app::AppTab;

    /// Every tab is reachable by name: the palette is the keyboard route to
    /// the tab strip, so a tab missing from it cannot be reached that way.
    #[test]
    fn every_tab_is_offered() {
        let offered: Vec<AppTab> = entries()
            .iter()
            .filter_map(|entry| match entry.action {
                PaletteAction::GoTo(tab) => Some(tab),
                _ => None,
            })
            .collect();
        for tab in AppTab::ALL {
            assert!(offered.contains(&tab), "{tab:?} is not in the palette");
        }
    }

    /// Labels are what the reader searches, so an empty one is unreachable.
    #[test]
    fn every_entry_is_named() {
        for entry in entries() {
            assert!(!entry.label.trim().is_empty(), "{:?} has no label", entry.action);
        }
    }
}
