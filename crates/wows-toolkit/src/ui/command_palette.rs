//! Unified fuzzy command palette (Ctrl+K / Ctrl+P) over the replay index.
//! Entry sourcing and dispatch are added in later tasks.
#![allow(dead_code)]

use std::path::PathBuf;

use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;

use crate::app::Tab;
use crate::armor_viewer::ship_selector::ShipCatalog;
use crate::db::index::query;
use crate::db::index::rows::MatchFilter;
use crate::db::index::rows::PlayerFacet;
use crate::db::index::rows::ShipFacet;

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
    /// Cached distinct players from the index, refreshed via `refresh_facets`.
    pub players: Vec<PlayerFacet>,
    /// Cached distinct self-piloted ships from the index, refreshed via `refresh_facets`.
    pub ships: Vec<ShipFacet>,
}

impl CommandPalette {
    /// Refresh the player/ship facet caches from the index. Best-effort: a
    /// failed query logs a warning and leaves the existing cache untouched.
    pub fn refresh_facets(&mut self, pool: &sqlx::SqlitePool, rt: &tokio::runtime::Runtime) {
        match rt.block_on(query::distinct_players(pool, &MatchFilter::default())) {
            Ok(p) => self.players = p,
            Err(e) => tracing::warn!("palette: distinct_players failed: {e}"),
        }
        match rt.block_on(query::distinct_self_ships(pool, &MatchFilter::default())) {
            Ok(s) => self.ships = s,
            Err(e) => tracing::warn!("palette: distinct_self_ships failed: {e}"),
        }
    }

    /// Build candidate entries for the given type filter. `egui_palette` does
    /// the fuzzy ranking over these; this only decides which entries are
    /// eligible for the current sigil-narrowed `kind`.
    pub fn build_entries(
        &self,
        kind: Option<EntryKind>,
        catalog: Option<&ShipCatalog>,
    ) -> Vec<egui_palette::Entry<'static, PaletteAction>> {
        let mut entries = Vec::new();
        let want = |k: EntryKind| kind.is_none() || kind == Some(k);

        if want(EntryKind::Command) {
            entries.push(egui_palette::Entry::new("Open Search tab", PaletteAction::OpenSearchTab));
            entries.push(egui_palette::Entry::new("Index all replays", PaletteAction::IndexAllReplays));
            for (label, tab) in [
                ("Go to: Replay parser", Tab::ReplayParser),
                ("Go to: Player tracker", Tab::PlayerTracker),
                ("Go to: Armor viewer", Tab::ArmorViewer),
                ("Go to: Stats", Tab::Stats),
                ("Go to: Settings", Tab::Settings),
            ] {
                entries.push(egui_palette::Entry::new(label, PaletteAction::GoToTab(tab)));
            }
        }

        if want(EntryKind::Ship) {
            if let Some(catalog) = catalog {
                for nation in &catalog.nations {
                    for class in &nation.classes {
                        for ship in &class.ships {
                            entries.push(
                                egui_palette::Entry::new(
                                    ship.display_name.clone(),
                                    PaletteAction::ViewArmor { ship_index: ship.param_index.clone() },
                                )
                                .with_subtitle("View armor"),
                            );
                        }
                    }
                }
            }
            for ship in &self.ships {
                entries.push(
                    egui_palette::Entry::new(
                        ship.ship_name.clone(),
                        PaletteAction::MyMatchesInShip { ship_id: ship.ship_id },
                    )
                    .with_subtitle(format!("My matches ({})", ship.match_count)),
                );
            }
        }

        if want(EntryKind::Player) {
            for p in &self.players {
                let label =
                    if p.clan.is_empty() { p.latest_name.clone() } else { format!("[{}] {}", p.clan, p.latest_name) };
                entries.push(
                    egui_palette::Entry::new(label, PaletteAction::FindMatchesWithPlayer { account_id: p.account_id })
                        .with_subtitle(format!("Find matches ({})", p.match_count)),
                );
            }
        }

        entries
    }
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

    #[test]
    fn build_entries_respects_kind_filter() {
        let mut p = CommandPalette::default();
        p.players = vec![]; // populated via refresh_facets at runtime
        // Commands are always present when kind is None or Command.
        let cmds = p.build_entries(Some(EntryKind::Command), None);
        assert!(cmds.iter().any(|e| matches!(e.data, PaletteAction::OpenSearchTab)));
        assert!(cmds.iter().any(|e| matches!(e.data, PaletteAction::IndexAllReplays)));
        // Player-kind filter yields no command entries.
        let players = p.build_entries(Some(EntryKind::Player), None);
        assert!(!players.iter().any(|e| matches!(e.data, PaletteAction::OpenSearchTab)));
    }
}
