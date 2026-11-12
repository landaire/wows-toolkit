//! The command palette's parts both desktop front ends share: its cascading modes and the rows
//! the replay index offers.

use rust_i18n::t;
use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::index::query_text::print_query;
use wows_toolkit_config::index::rows::IndexError;

use crate::name_lookup::PlayerLookup;
use crate::name_lookup::ShipLookup;
use crate::query_bar::seed;
use crate::text_filter::FilterQuery;

/// Which cascading list the palette is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaletteMode {
    /// Players the index has seen, most met first.
    Players,
    /// Ships the reader has played, most played first.
    MyShips,
    /// Every ship in the loaded build, to open in the Armor Viewer.
    ArmorShips,
    /// The game versions the listing holds, to force a re-index of one.
    ReindexVersions,
    /// "Browse..." and the directories most recently opened, to open one in the listing.
    ReplayFolders,
}

impl PaletteMode {
    /// The modes the root list leads into from its searches. The version list is offered beside
    /// the other re-index command instead, and the directory list beside opening a file.
    pub const ALL: [Self; 3] = [Self::Players, Self::MyShips, Self::ArmorShips];

    /// What the root entry that enters this mode is called.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.search_players",
            Self::MyShips => "ui.palette.my_matches_in_ship",
            Self::ArmorShips => "ui.palette.open_ship_in_armor_viewer",
            Self::ReindexVersions => "ui.reindex.palette_by_version",
            Self::ReplayFolders => "ui.replay.open_directory",
        }
    }

    /// What the layer is called in its header, after the root's name.
    pub const fn title_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_title",
            Self::MyShips => "ui.palette.my_ships_title",
            Self::ArmorShips => "ui.palette.armor_ships_title",
            Self::ReindexVersions => "ui.reindex.versions_title",
            Self::ReplayFolders => "ui.replay_folders.recent_title",
        }
    }

    pub const fn placeholder_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_placeholder",
            Self::MyShips => "ui.palette.my_ships_placeholder",
            Self::ArmorShips => "ui.palette.armor_ships_placeholder",
            Self::ReindexVersions => "ui.reindex.versions_placeholder",
            Self::ReplayFolders => "ui.replay_folders.recent_placeholder",
        }
    }
}

/// Maximum rows one index-backed layer lists.
pub const MODE_LIMIT: usize = 50;

/// What an index-backed layer lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexList {
    Players,
    /// The ships the reader has played.
    MyShips,
}

/// An index-backed layer's rows, read once and ranked for each query.
pub enum IndexLookup {
    Players(PlayerLookup),
    Ships(ShipLookup),
}

/// Why an index-backed layer has no rows to offer.
#[derive(Debug, thiserror::Error)]
pub enum ModeQueryError {
    #[error("The indexed players could not be read: {source}")]
    Players { source: IndexError },
    #[error("The indexed ships could not be read: {source}")]
    Ships { source: IndexError },
}

impl IndexList {
    /// Reads the rows `self` ranks.
    pub async fn load(self, pool: &SqlitePool) -> Result<IndexLookup, ModeQueryError> {
        match self {
            Self::Players => PlayerLookup::load(pool)
                .await
                .map(IndexLookup::Players)
                .map_err(|source| ModeQueryError::Players { source }),
            Self::MyShips => {
                ShipLookup::load_played(pool).await.map(IndexLookup::Ships).map_err(|source| ModeQueryError::Ships { source })
            }
        }
    }
}

/// One row an index-backed layer offers: what it says, and the Search tab query it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeededSearch {
    pub label: String,
    /// In the grammar the Search tab parses.
    pub query: String,
}

/// The rows `lookup` offers for `query`, best first: each player leads to the battles they were
/// in, each ship to the reader's own battles in it.
pub fn seeded_searches(lookup: &IndexLookup, query: &FilterQuery) -> Vec<SeededSearch> {
    match lookup {
        IndexLookup::Players(players) => players
            .find(query, MODE_LIMIT)
            .into_iter()
            .map(|player| {
                let named = match player.clan.as_str() {
                    "" => player.latest_name.clone(),
                    clan => format!("[{clan}] {}", player.latest_name),
                };
                SeededSearch {
                    label: format!("{named} -- {}", t!("ui.palette.matches_found", count = player.match_count)),
                    query: print_query(&seed::matches_with_player(player.account_id)),
                }
            })
            .collect(),
        IndexLookup::Ships(ships) => ships
            .find(query, MODE_LIMIT)
            .into_iter()
            .map(|ship| SeededSearch {
                label: format!("{} -- {}", ship.ship_name, t!("ui.palette.matches_found", count = ship.match_count)),
                query: print_query(&seed::my_matches_in_ship(ship.ship_id)),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing indexed is an empty result rather than a query error.
    #[tokio::test]
    async fn an_empty_index_offers_no_rows() {
        let pool = wows_toolkit_config::test_pool().await;

        for list in [IndexList::Players, IndexList::MyShips] {
            let lookup = list.load(&pool).await.expect("the empty index reads");
            assert!(seeded_searches(&lookup, &FilterQuery::default()).is_empty(), "{list:?}");
        }
    }
}
