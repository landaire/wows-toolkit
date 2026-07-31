//! Cascading command palette (Ctrl+K / Ctrl+P) over the replay index.
//!
//! The palette opens on a static `Root` menu; picking a category enters a
//! `Sub` mode that sources entries on demand (bounded DB queries or an
//! in-memory ship-catalog filter) as the user types, instead of loading the
//! entire index up front.
#![allow(dead_code)]

use std::path::PathBuf;

use wows_replays::types::AccountId;
use wows_replays::types::GameParamId;

use crate::app::Tab;
use crate::armor_viewer::ship_selector::ShipCatalog;
use crate::armor_viewer::ship_selector::ShipEntry;
use crate::armor_viewer::ship_selector::tier_roman;
use crate::db::index::query;
use crate::db::index::query_model::Chip;
use crate::db::index::query_model::Connector;
use crate::db::index::query_model::Field;
use crate::db::index::query_model::Group;
use crate::db::index::query_model::Op;
use crate::db::index::query_model::Query;
use crate::db::index::query_model::StatKind;
use crate::db::index::query_model::Subject;
use crate::db::index::query_model::Value;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::PlayerFacet;
use crate::db::index::rows::ShipFacet;

/// What a picked palette entry does. Carried as the `Entry` payload.
#[derive(Clone)]
pub enum PaletteAction {
    ViewArmor {
        ship_index: String,
    },
    MyMatchesInShip {
        ship_id: GameParamId,
    },
    FindMatchesWithPlayer {
        account_id: AccountId,
    },
    OpenReplay {
        path: PathBuf,
    },
    OpenReplayFile,
    OpenSearchTab,
    IndexAllReplays,
    GoToTab(Tab),
    /// Enter a cascade sub-mode. Handled by the render loop before it
    /// reaches `dispatch_palette_action` (the palette stays open).
    EnterSub(SubKind),
    /// Hand a pre-built query to the Search tab and focus it.
    OpenSearchWith(Query),
    /// Switch the app theme.
    SetTheme(crate::data::settings::ThemeChoice),
}

/// Which screen the palette is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteMode {
    #[default]
    Root,
    Sub(SubKind),
}

/// A cascade sub-mode: entries are sourced on demand, bounded, as the user types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubKind {
    Players,
    MyShips,
    ArmorShips,
}

/// Bound applied to every on-demand sub-query (DB or in-memory).
const SUB_QUERY_LIMIT: i64 = 50;

#[derive(Default)]
pub struct CommandPalette {
    pub state: egui_palette::State,
    pub mode: PaletteMode,
    /// `state.query` as of the last time a sub-query actually ran; re-query
    /// only when this no longer matches.
    last_query: String,
    /// Which `SubKind` `last_query` was captured for. A kind change forces a
    /// re-query even if the (now-different) query text happens to coincide
    /// with the empty string left over from the previous sub-mode.
    last_kind: Option<SubKind>,
    player_results: Vec<PlayerFacet>,
    ship_results: Vec<ShipFacet>,
    armor_results: Vec<ShipEntry>,
}

impl CommandPalette {
    /// Enter a cascade sub-mode with a fresh (empty) query. Forces
    /// `sub_entries` to re-run its bounded query on the next call even if
    /// the leftover query text happens to match what the previous sub-mode
    /// (or the previous visit to this same sub-mode) left behind.
    pub fn enter_sub(&mut self, kind: SubKind) {
        self.mode = PaletteMode::Sub(kind);
        self.state.query.clear();
        self.last_kind = None;
    }

    /// Back out of a cascade sub-mode to the root menu with a fresh query.
    pub fn back_to_root(&mut self) {
        self.mode = PaletteMode::Root;
        self.state.query.clear();
        self.last_kind = None;
    }

    /// Static root menu: categories that cascade into a sub-search, plus a
    /// handful of direct shortcuts. Always fuzzy-filtered by `egui_palette`
    /// against the raw query (no DB/catalog access).
    pub fn root_entries(&self) -> Vec<egui_palette::Entry<'static, PaletteAction>> {
        let mut entries = Vec::new();

        entries.push(egui_palette::Entry::new("Search players...", PaletteAction::EnterSub(SubKind::Players)));
        entries.push(egui_palette::Entry::new("My matches in ship...", PaletteAction::EnterSub(SubKind::MyShips)));
        entries.push(egui_palette::Entry::new("View armor for ship...", PaletteAction::EnterSub(SubKind::ArmorShips)));

        let died_query = Query {
            groups: vec![Group {
                chips: vec![
                    Chip { field: Field::Outcome, op: Op::Is, value: Value::Outcome(MatchOutcome::Loss) },
                    Chip {
                        field: Field::Stat { kind: StatKind::Survived, subject: Subject::SelfPlayer },
                        op: Op::Is,
                        value: Value::Bool(false),
                    },
                ],
            }],
            connector: Connector::And,
        };
        entries.push(egui_palette::Entry::new("Games I died in", PaletteAction::OpenSearchWith(died_query)));

        let won_query = Query {
            groups: vec![Group {
                chips: vec![Chip { field: Field::Outcome, op: Op::Is, value: Value::Outcome(MatchOutcome::Win) }],
            }],
            connector: Connector::And,
        };
        entries.push(egui_palette::Entry::new("Games I won", PaletteAction::OpenSearchWith(won_query)));

        entries.push(egui_palette::Entry::new(rust_i18n::t!("ui.replay.open_manually"), PaletteAction::OpenReplayFile));
        entries.push(egui_palette::Entry::new("Advanced search...", PaletteAction::OpenSearchTab));
        entries.push(egui_palette::Entry::new("Index all replays", PaletteAction::IndexAllReplays));
        for (label, choice) in [
            ("Theme: follow system", crate::data::settings::ThemeChoice::System),
            ("Theme: dark", crate::data::settings::ThemeChoice::Dark),
            ("Theme: light", crate::data::settings::ThemeChoice::Light),
        ] {
            entries.push(egui_palette::Entry::new(label, PaletteAction::SetTheme(choice)));
        }
        for (label, tab) in [
            ("Go to: Replay parser", Tab::ReplayParser),
            ("Go to: Player tracker", Tab::PlayerTracker),
            ("Go to: Armor viewer", Tab::ArmorViewer),
            ("Go to: Stats", Tab::Stats),
            ("Go to: Settings", Tab::Settings),
        ] {
            entries.push(egui_palette::Entry::new(label, PaletteAction::GoToTab(tab)));
        }

        entries
    }

    /// On-demand entries for a cascade sub-mode. Only re-runs the bounded
    /// query when `state.query` (or the mode itself) changed since the last
    /// call; otherwise reuses the cached result set. Query/catalog errors
    /// are logged and swallowed, leaving the previous results in place.
    pub fn sub_entries(
        &mut self,
        kind: SubKind,
        pool: Option<&sqlx::SqlitePool>,
        rt: &tokio::runtime::Runtime,
        catalog: Option<&ShipCatalog>,
    ) -> Vec<egui_palette::Entry<'static, PaletteAction>> {
        let changed = self.state.query != self.last_query || self.last_kind != Some(kind);
        if changed {
            self.last_query.clone_from(&self.state.query);
            self.last_kind = Some(kind);
            match kind {
                SubKind::Players => {
                    if let Some(pool) = pool {
                        match rt.block_on(query::search_players(pool, &self.state.query, SUB_QUERY_LIMIT)) {
                            Ok(results) => self.player_results = results,
                            Err(e) => tracing::warn!("palette: search_players failed: {e}"),
                        }
                    }
                }
                SubKind::MyShips => {
                    if let Some(pool) = pool {
                        match rt.block_on(query::search_self_ships(pool, &self.state.query, SUB_QUERY_LIMIT)) {
                            Ok(results) => self.ship_results = results,
                            Err(e) => tracing::warn!("palette: search_self_ships failed: {e}"),
                        }
                    }
                }
                SubKind::ArmorShips => {
                    if let Some(catalog) = catalog {
                        self.armor_results =
                            catalog.search(&self.state.query, SUB_QUERY_LIMIT as usize).into_iter().cloned().collect();
                    }
                }
            }
        }

        match kind {
            SubKind::Players => self
                .player_results
                .iter()
                .map(|p| {
                    let label = if p.clan.is_empty() {
                        p.latest_name.clone()
                    } else {
                        format!("[{}] {}", p.clan, p.latest_name)
                    };
                    egui_palette::Entry::new(label, PaletteAction::FindMatchesWithPlayer { account_id: p.account_id })
                        .with_subtitle(format!("Find matches ({})", p.match_count))
                })
                .collect(),
            SubKind::MyShips => self
                .ship_results
                .iter()
                .map(|s| {
                    egui_palette::Entry::new(s.ship_name.clone(), PaletteAction::MyMatchesInShip { ship_id: s.ship_id })
                        .with_subtitle(format!("My matches ({})", s.match_count))
                })
                .collect(),
            SubKind::ArmorShips => self
                .armor_results
                .iter()
                .map(|ship| {
                    egui_palette::Entry::new(
                        ship.display_name.clone(),
                        PaletteAction::ViewArmor { ship_index: ship.param_index.clone() },
                    )
                    .with_subtitle(format!("View armor (Tier {})", tier_roman(ship.tier)))
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_entries_include_categories_and_shortcuts() {
        let p = CommandPalette::default();
        let entries = p.root_entries();
        assert!(entries.iter().any(|e| matches!(e.data, PaletteAction::EnterSub(SubKind::Players))));
        assert!(entries.iter().any(|e| matches!(e.data, PaletteAction::EnterSub(SubKind::MyShips))));
        assert!(entries.iter().any(|e| matches!(e.data, PaletteAction::EnterSub(SubKind::ArmorShips))));
        assert!(entries.iter().any(|e| matches!(e.data, PaletteAction::OpenSearchTab)));
        assert!(entries.iter().any(|e| matches!(e.data, PaletteAction::IndexAllReplays)));
        assert!(entries.iter().any(|e| matches!(e.data, PaletteAction::OpenReplayFile)));

        let died = entries
            .iter()
            .find_map(|e| match &e.data {
                PaletteAction::OpenSearchWith(q) if e.title == "Games I died in" => Some(q.clone()),
                _ => None,
            })
            .expect("Games I died in entry");
        assert_eq!(died.groups.len(), 1);
        assert_eq!(died.groups[0].chips.len(), 2);
        assert!(
            died.groups[0]
                .chips
                .iter()
                .any(|c| matches!(c.value, Value::Outcome(MatchOutcome::Loss)) && matches!(c.op, Op::Is))
        );
        assert!(died.groups[0].chips.iter().any(|c| matches!(c.value, Value::Bool(false))));

        let won = entries
            .iter()
            .find_map(|e| match &e.data {
                PaletteAction::OpenSearchWith(q) if e.title == "Games I won" => Some(q.clone()),
                _ => None,
            })
            .expect("Games I won entry");
        assert_eq!(won.groups.len(), 1);
        assert_eq!(won.groups[0].chips.len(), 1);
        assert!(matches!(won.groups[0].chips[0].value, Value::Outcome(MatchOutcome::Win)));
    }

    #[test]
    fn sub_entries_armor_ships_uses_catalog_without_db() {
        let mut p = CommandPalette::default();
        // No pool, no runtime call needed on this path: ArmorShips is served
        // entirely from the (absent-here) in-memory catalog, so `None` is a
        // valid catalog and should just yield no entries rather than panic.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let entries = p.sub_entries(SubKind::ArmorShips, None, &rt, None);
        assert!(entries.is_empty());
    }
}
