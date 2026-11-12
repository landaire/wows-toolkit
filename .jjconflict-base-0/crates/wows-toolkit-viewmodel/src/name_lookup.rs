//! Players and ships the replay index has seen, found by a typed name.
//!
//! The index is read once and every name folded then, so that each keystroke
//! only scores the folded names. Ranking happens before the result is capped:
//! a capped SQL search would drop a fuzzy match before it could be ranked.

use sqlx::SqlitePool;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::NamedPlayer;
use wows_toolkit_config::index::rows::PlayerFacet;
use wows_toolkit_config::index::rows::ShipFacet;

use crate::text_filter::FilterKeyTable;
use crate::text_filter::FilterQuery;
use crate::text_filter::MatchScore;

/// Every player the index has met, with each name they were seen under.
pub struct PlayerLookup {
    players: Vec<PlayerFacet>,
    /// Every player's names, in `players` order.
    names: FilterKeyTable,
    /// Where each player's names end in `names`.
    name_ends: Vec<usize>,
}

/// Ships the index has seen.
pub struct ShipLookup {
    ships: Vec<ShipFacet>,
    /// Each ship's name, in `ships` order.
    names: FilterKeyTable,
}

impl PlayerLookup {
    /// Every non-bot player in the index.
    pub async fn load(pool: &SqlitePool) -> Result<Self, IndexError> {
        Ok(Self::new(query::named_players(pool).await?))
    }

    /// `players` in the order a query that matches all of them equally
    /// lists them.
    pub fn new(players: Vec<NamedPlayer>) -> Self {
        let mut names = FilterKeyTable::new();
        let mut name_ends = Vec::with_capacity(players.len());
        let mut facets = Vec::with_capacity(players.len());
        for player in players {
            for name in &player.names {
                names.push(name);
            }
            name_ends.push(names.len());
            facets.push(player.facet);
        }

        Self { players: facets, names, name_ends }
    }

    /// The players `query` matches one of the names of, best first, ties in
    /// the lookup's order, at most `limit` of them.
    pub fn find(&self, query: &FilterQuery, limit: usize) -> Vec<&PlayerFacet> {
        let scored = self.name_ends.iter().enumerate().filter_map(|(player, &end)| {
            let start = player.checked_sub(1).map_or(0, |previous| self.name_ends[previous]);
            let score = (start..end).filter_map(|row| query.score_at(&self.names, row)).max()?;

            Some((player, score))
        });

        best_first(scored, limit).into_iter().map(|player| &self.players[player]).collect()
    }
}

impl ShipLookup {
    /// Every ship in the index, on either side.
    pub async fn load_roster(pool: &SqlitePool) -> Result<Self, IndexError> {
        Ok(Self::new(query::distinct_ships(pool).await?))
    }

    /// The ships the reader has played.
    pub async fn load_played(pool: &SqlitePool) -> Result<Self, IndexError> {
        Ok(Self::new(query::distinct_self_ships(pool, &MatchFilter::default()).await?))
    }

    /// `ships` in the order a query that matches all of them equally lists
    /// them.
    pub fn new(ships: Vec<ShipFacet>) -> Self {
        let mut names = FilterKeyTable::new();
        for ship in &ships {
            names.push(&ship.ship_name);
        }

        Self { ships, names }
    }

    /// The ships whose name `query` matches, best first, ties in the
    /// lookup's order, at most `limit` of them.
    pub fn find(&self, query: &FilterQuery, limit: usize) -> Vec<&ShipFacet> {
        let scored = (0..self.ships.len()).filter_map(|ship| Some((ship, query.score_at(&self.names, ship)?)));

        best_first(scored, limit).into_iter().map(|ship| &self.ships[ship]).collect()
    }
}

/// The positions of the `limit` best scores, ties in their order.
fn best_first(scored: impl Iterator<Item = (usize, MatchScore)>, limit: usize) -> Vec<usize> {
    let mut scored: Vec<(usize, MatchScore)> = scored.collect();
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored.truncate(limit);

    scored.into_iter().map(|(position, _)| position).collect()
}

#[cfg(test)]
mod tests {
    use wows_replays::types::AccountId;
    use wows_replays::types::GameParamId;

    use super::*;

    fn ship(id: u64, name: &str) -> ShipFacet {
        ShipFacet { ship_id: GameParamId::from(id), ship_name: name.to_owned(), match_count: 1 }
    }

    fn player(id: i64, names: &[&str]) -> NamedPlayer {
        NamedPlayer {
            facet: PlayerFacet {
                account_id: AccountId::from(id),
                latest_name: names[0].to_owned(),
                clan: String::new(),
                match_count: 1,
            },
            names: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    fn ship_names(found: Vec<&ShipFacet>) -> Vec<&str> {
        found.into_iter().map(|ship| ship.ship_name.as_str()).collect()
    }

    /// A fuzzy or folded match is ranked against the whole roster before the
    /// cap: more-played ships the query does not match cannot crowd it out.
    #[test]
    fn a_ship_is_found_by_letters_or_folded_past_the_cap() {
        let mut ships: Vec<ShipFacet> = (0..60).map(|id| ship(id, &format!("Filler {id}"))).collect();
        ships.push(ship(100, "Yamato"));
        ships.push(ship(101, "Sm\u{e5}land"));
        let lookup = ShipLookup::new(ships);

        assert_eq!(ship_names(lookup.find(&FilterQuery::new("ymt"), 50)), ["Yamato"]);
        assert_eq!(ship_names(lookup.find(&FilterQuery::new("smaland"), 50)), ["Sm\u{e5}land"]);
        assert_eq!(lookup.find(&FilterQuery::new(""), 50).len(), 50, "an empty query lists the most played");
        assert_eq!(ship_names(lookup.find(&FilterQuery::new(""), 1)), ["Filler 0"]);
    }

    /// A name's own prefix ranks ahead of a match inside it; equal matches
    /// keep the lookup's most-played order.
    #[test]
    fn ships_are_ranked_by_match_then_play_count() {
        let lookup = ShipLookup::new(vec![ship(1, "Musashi"), ship(2, "Des Moines"), ship(3, "Moskva")]);

        assert_eq!(ship_names(lookup.find(&FilterQuery::new("mo"), 50)), ["Moskva", "Des Moines"]);
        assert_eq!(ship_names(lookup.find(&FilterQuery::new("m"), 50)), ["Musashi", "Moskva", "Des Moines"]);
    }

    /// A player is found by any name they were seen under, once.
    #[test]
    fn a_player_is_found_by_an_earlier_name() {
        let lookup = PlayerLookup::new(vec![
            player(1, &["Captain_J\u{fc}rgen", "OldSailor"]),
            player(2, &["Sailor_Moon"]),
            player(3, &["Nobody"]),
        ]);
        let found = |typed: &str| -> Vec<i64> {
            lookup.find(&FilterQuery::new(typed), 50).into_iter().map(|player| player.account_id.raw()).collect()
        };

        assert_eq!(found("jurgen"), [1]);
        assert_eq!(found("oldsl"), [1]);
        assert_eq!(found("sailor"), [2, 1], "a name's own prefix leads");
        assert_eq!(found(""), [1, 2, 3]);
    }
}
