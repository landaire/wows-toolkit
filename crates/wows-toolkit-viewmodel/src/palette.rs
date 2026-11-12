//! The command palette's parts both desktop front ends share: its cascading modes, how a typed
//! ship name ranks the ship list, and the rows the replay index offers.

use rust_i18n::t;
use sqlx::sqlite::SqlitePool;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query_text::print_query;
use wows_toolkit_config::index::rows::IndexError;

use crate::query_bar::seed;

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

/// Maximum rows returned by one cascading index query.
pub const MODE_LIMIT: i64 = 50;

/// A ship name folded for search: diacritics transliterated, case dropped, outer space trimmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipSearchKey(String);

impl ShipSearchKey {
    pub fn new(name: &str) -> Self {
        Self(unidecode::unidecode(name.trim()).to_lowercase())
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// How closely a folded ship name matches the folded query. Ordered best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ShipMatch {
    Exact,
    Prefix,
    /// The query starts a later word of the name ("moines" in "des moines").
    WordPrefix,
    Contains,
    /// The query's letters appear in order ("dsmns" in "des moines").
    Subsequence,
}

fn ship_match(name: &str, needle: &str) -> Option<ShipMatch> {
    if name == needle {
        return Some(ShipMatch::Exact);
    }
    if name.starts_with(needle) {
        return Some(ShipMatch::Prefix);
    }

    let mut found = name.match_indices(needle).peekable();
    if found.peek().is_some() {
        let word_start =
            found.any(|(at, _)| name[..at].chars().next_back().is_some_and(|before| !before.is_alphanumeric()));
        return Some(if word_start { ShipMatch::WordPrefix } else { ShipMatch::Contains });
    }

    let mut rest = name.chars();
    let subsequence =
        needle.chars().filter(|c| !c.is_whitespace()).all(|wanted| rest.by_ref().any(|have| have == wanted));
    subsequence.then_some(ShipMatch::Subsequence)
}

/// The rows whose name matches `query`, best first: every row, in the order given, when it is
/// empty.
pub fn rank_ships<'a, T>(rows: &'a [T], query: &str, key: impl Fn(&T) -> &ShipSearchKey) -> Vec<&'a T> {
    let needle = ShipSearchKey::new(query);
    if needle.as_str().is_empty() {
        return rows.iter().collect();
    }

    let mut matched: Vec<(ShipMatch, &T)> =
        rows.iter().filter_map(|row| ship_match(key(row).as_str(), needle.as_str()).map(|rank| (rank, row))).collect();
    // Stable, so equal matches keep the order given.
    matched.sort_by_key(|(rank, _)| *rank);

    matched.into_iter().map(|(_, row)| row).collect()
}

/// One row an index-backed mode offers: what it says, and the Search tab query it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeededSearch {
    pub label: String,
    /// In the grammar the Search tab parses.
    pub query: String,
}

/// Why an index-backed mode has no rows to offer.
#[derive(Debug, thiserror::Error)]
pub enum ModeQueryError {
    #[error("The indexed players could not be read: {source}")]
    Players { source: IndexError },
    #[error("The indexed ships could not be read: {source}")]
    Ships { source: IndexError },
}

/// The players whose name contains `needle`, most met first, each leading to the battles they
/// were in.
pub async fn player_searches(pool: &SqlitePool, needle: &str) -> Result<Vec<SeededSearch>, ModeQueryError> {
    let found =
        query::search_players(pool, needle, MODE_LIMIT).await.map_err(|source| ModeQueryError::Players { source })?;

    Ok(found
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
        .collect())
}

/// The ships the reader has played whose name contains `needle`, most played first, each leading
/// to the reader's own battles in it.
pub async fn ship_searches(pool: &SqlitePool, needle: &str) -> Result<Vec<SeededSearch>, ModeQueryError> {
    let found =
        query::search_self_ships(pool, needle, MODE_LIMIT).await.map_err(|source| ModeQueryError::Ships { source })?;

    Ok(found
        .into_iter()
        .map(|ship| SeededSearch {
            label: format!("{} -- {}", ship.ship_name, t!("ui.palette.matches_found", count = ship.match_count)),
            query: print_query(&seed::my_matches_in_ship(ship.ship_id)),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ship search folds diacritics, lists the name's own prefix ahead of a later word's, and
    /// still finds a name typed with letters left out.
    #[test]
    fn ship_search_folds_and_ranks_names() {
        let rows: Vec<(ShipSearchKey, &str)> = ["Musashi", "Yamato", "Sm\u{e5}land", "Des Moines", "Moskva"]
            .into_iter()
            .map(|name| (ShipSearchKey::new(name), name))
            .collect();
        let found = |query: &str| -> Vec<&str> {
            rank_ships(&rows, query, |(key, _)| key).into_iter().map(|(_, name)| *name).collect()
        };

        assert_eq!(found("").len(), rows.len(), "an empty search lists every ship");
        assert_eq!(found("smal"), ["Sm\u{e5}land"]);
        assert_eq!(found(" MO "), ["Moskva", "Des Moines", "Yamato"]);
        assert_eq!(found("dsmns"), ["Des Moines"]);
        assert!(found("zzz").is_empty());
    }

    /// Nothing indexed is an empty result rather than a query error.
    #[tokio::test]
    async fn an_empty_index_offers_no_rows() {
        let pool = wows_toolkit_config::test_pool().await;

        assert!(player_searches(&pool, "").await.expect("query players").is_empty());
        assert!(ship_searches(&pool, "").await.expect("query ships").is_empty());
    }
}
