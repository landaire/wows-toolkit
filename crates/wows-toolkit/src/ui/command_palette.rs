//! Unified fuzzy command palette (Ctrl+K / Ctrl+P) over the replay index.
//! Entry sourcing and dispatch are added in later tasks.
#![allow(dead_code)]

use std::path::PathBuf;

use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;

use crate::app::Tab;

/// What a picked palette entry does. Carried as the `Entry` payload.
#[derive(Clone)]
pub enum PaletteAction {
    ViewArmor { ship_index: String },
    MyMatchesInShip { ship_id: GameParamId },
    FindMatchesWithPlayer { account_id: AccountId },
    OpenReplay { path: PathBuf },
    OpenSearchTab,
    IndexAllReplays,
    GoToTab(Tab),
}

/// Optional type narrowing from a leading sigil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Ship,
    Player,
    Replay,
    Command,
}

/// Split a raw query into an optional type filter (from a leading sigil) and the
/// remaining trimmed query text. `#`=Ship, `@`=Player, `>`=Command; none = all.
pub fn parse_query(raw: &str) -> (Option<EntryKind>, &str) {
    let trimmed = raw.trim_start();
    let (kind, rest) = match trimmed.as_bytes().first() {
        Some(b'#') => (Some(EntryKind::Ship), &trimmed[1..]),
        Some(b'@') => (Some(EntryKind::Player), &trimmed[1..]),
        Some(b'>') => (Some(EntryKind::Command), &trimmed[1..]),
        _ => (None, trimmed),
    };
    (kind, rest.trim())
}

#[derive(Default)]
pub struct CommandPalette {
    pub state: egui_palette::State,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_maps_sigils() {
        assert!(matches!(parse_query("haru"), (None, "haru")));
        assert!(matches!(parse_query("#haru"), (Some(EntryKind::Ship), "haru")));
        assert!(matches!(parse_query("@ward"), (Some(EntryKind::Player), "ward")));
        assert!(matches!(parse_query(">index"), (Some(EntryKind::Command), "index")));
        // sigil with surrounding space
        assert!(matches!(parse_query("#  haru "), (Some(EntryKind::Ship), "haru")));
        // bare sigil -> empty remainder
        assert!(matches!(parse_query("@"), (Some(EntryKind::Player), "")));
    }
}
