//! The command palette's second layers: the players and ships the replay
//! index has seen, and every ship of the loaded build, each filtered as the
//! reader types.

use rust_i18n::t;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::query_text;

use crate::query_bar::seed;
use crate::search::normalize_ship_search;

/// Maximum rows returned by one cascading index query.
pub const MODE_LIMIT: i64 = 50;

/// Which cascading list the palette is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    /// Players the index has seen, most met first.
    Players,
    /// Ships the reader has played, most played first.
    MyShips,
    /// Every ship in the loaded build, to open in the Armor Viewer.
    ArmorShips,
}

impl PaletteMode {
    pub const ALL: [PaletteMode; 3] = [Self::Players, Self::MyShips, Self::ArmorShips];

    /// What the root entry that enters this mode is called.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.search_players",
            Self::MyShips => "ui.palette.my_matches_in_ship",
            Self::ArmorShips => "ui.palette.open_ship_in_armor_viewer",
        }
    }

    /// What the layer is called in its header, after the root's name.
    pub const fn title_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_title",
            Self::MyShips => "ui.palette.my_ships_title",
            Self::ArmorShips => "ui.palette.armor_ships_title",
        }
    }

    pub const fn placeholder_key(self) -> &'static str {
        match self {
            Self::Players => "ui.palette.players_placeholder",
            Self::MyShips => "ui.palette.my_ships_placeholder",
            Self::ArmorShips => "ui.palette.armor_ships_placeholder",
        }
    }

    /// Whether the replay index answers this layer, rather than the loaded
    /// build's ship list.
    pub const fn reads_index(self) -> bool {
        match self {
            Self::Players | Self::MyShips => true,
            Self::ArmorShips => false,
        }
    }
}

/// One row an index-backed layer offers: what it reads as and the Search
/// query confirming it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRow {
    pub label: String,
    pub query: String,
}

/// Why an index-backed layer has no rows to show.
#[derive(Debug, thiserror::Error)]
pub enum PaletteError {
    #[error("The indexed players could not be read: {source}")]
    Players { source: wows_toolkit_config::index::rows::IndexError },
    #[error("The indexed ships could not be read: {source}")]
    Ships { source: wows_toolkit_config::index::rows::IndexError },
    #[error("{mode:?} is not read from the replay index")]
    NotIndexed { mode: PaletteMode },
}

/// One ship the ship layer lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipRow {
    pub param_index: String,
    pub display_name: String,
    /// The name as [`normalize_ship_search`] folds it.
    search_name: String,
    /// The name with tier, class and nation, which is what the row shows.
    pub label: String,
}

impl ShipRow {
    /// A row for one ship. `tier` and `class` are already written for
    /// reading (`X`, `Battleship`).
    pub fn new(param_index: String, display_name: String, tier: &str, class: &str, nation: &str) -> Self {
        let search_name = normalize_ship_search(&display_name);
        let label =
            t!("ui.palette.armor_ship_row", name = display_name, tier = tier, class = class, nation = nation)
                .into_owned();

        Self { param_index, display_name, search_name, label }
    }
}

/// How closely a folded ship name matches the folded query. Ordered best
/// first.
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

/// The ship layer's rows for `query`: every ship when it is empty, else the
/// matches, best first.
pub fn rank_ships<'a>(rows: &'a [ShipRow], query: &str) -> Vec<&'a ShipRow> {
    let needle = normalize_ship_search(query);
    let mut matched: Vec<(ShipMatch, &ShipRow)> = if needle.is_empty() {
        rows.iter().map(|row| (ShipMatch::Exact, row)).collect()
    } else {
        rows.iter().filter_map(|row| ship_match(&row.search_name, &needle).map(|rank| (rank, row))).collect()
    };

    // Stable, so equal matches keep the catalogue's order.
    matched.sort_by_key(|(rank, _)| *rank);

    matched.into_iter().map(|(_, row)| row).collect()
}

/// Rows one index-backed mode offers for `needle`.
///
/// An empty result is distinct from a query failure so the palette can report
/// each one correctly.
pub async fn index_rows(
    mode: PaletteMode,
    pool: &sqlx::SqlitePool,
    needle: &str,
) -> Result<Vec<IndexRow>, PaletteError> {
    match mode {
        PaletteMode::Players => {
            let found = query::search_players(pool, needle, MODE_LIMIT)
                .await
                .map_err(|source| PaletteError::Players { source })?;

            Ok(found
                .into_iter()
                .map(|player| {
                    let named = match player.clan.as_str() {
                        "" => player.latest_name.clone(),
                        clan => format!("[{clan}] {}", player.latest_name),
                    };
                    IndexRow {
                        label: format!("{named} -- {}", t!("ui.palette.matches_found", count = player.match_count)),
                        query: query_text::print_query(&seed::matches_with_player(player.account_id)),
                    }
                })
                .collect())
        }
        PaletteMode::MyShips => {
            let found = query::search_self_ships(pool, needle, MODE_LIMIT)
                .await
                .map_err(|source| PaletteError::Ships { source })?;

            Ok(found
                .into_iter()
                .map(|ship| IndexRow {
                    label: format!("{} -- {}", ship.ship_name, t!("ui.palette.matches_found", count = ship.match_count)),
                    query: query_text::print_query(&seed::my_matches_in_ship(ship.ship_id)),
                })
                .collect())
        }
        // Read from the loaded build rather than the index: armor is a property of
        // the ship, not of anything the reader has played.
        PaletteMode::ArmorShips => Err(PaletteError::NotIndexed { mode }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The ship search folds diacritics, lists the name's own prefix ahead of a
    /// later word's, and still finds a name typed with letters left out.
    #[test]
    fn ship_search_folds_and_ranks_names() {
        let rows: Vec<ShipRow> = ["Musashi", "Yamato", "Sm\u{e5}land", "Des Moines", "Moskva"]
            .into_iter()
            .map(|name| ShipRow::new(name.to_lowercase(), name.to_owned(), "X", "Cruiser", "Nation"))
            .collect();
        let found = |query: &str| -> Vec<String> {
            rank_ships(&rows, query).into_iter().map(|row| row.display_name.clone()).collect()
        };

        assert_eq!(found("").len(), rows.len(), "an empty search lists every ship");
        assert_eq!(found("smal"), ["Sm\u{e5}land"]);
        assert_eq!(found(" MO "), ["Moskva", "Des Moines", "Yamato"]);
        assert_eq!(found("dsmns"), ["Des Moines"]);
        assert!(found("zzz").is_empty());
    }

    #[test]
    fn a_ship_row_reads_with_its_tier_class_and_nation() {
        let row = ShipRow::new("PJSB018".into(), "Yamato".into(), "X", "Battleship", "Japan");

        assert!(row.label.starts_with("Yamato"));
        assert!(row.label.contains("Battleship"));
        assert!(row.label.contains("Japan"));
    }

    /// A player row leads to the search that finds their battles, and a ship row
    /// to the reader's own battles in it, both as the shared seeds express them.
    #[tokio::test]
    async fn an_empty_index_has_no_rows_and_the_seeds_parse_back() {
        let pool = wows_toolkit_config::test_pool().await;

        assert!(index_rows(PaletteMode::Players, &pool, "").await.expect("query players").is_empty());
        assert!(index_rows(PaletteMode::MyShips, &pool, "").await.expect("query ships").is_empty());
        assert!(matches!(
            index_rows(PaletteMode::ArmorShips, &pool, "").await,
            Err(PaletteError::NotIndexed { mode: PaletteMode::ArmorShips })
        ));

        let account = wows_replays::types::AccountId(7);
        let printed = query_text::print_query(&seed::matches_with_player(account));
        assert_eq!(
            query_text::parse_query(&printed).expect("the seeded query parses"),
            seed::matches_with_player(account)
        );
    }
}
