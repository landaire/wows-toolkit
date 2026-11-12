//! Games read from an explicit set of replays, such as every replay under one
//! directory, rather than the games the session recorded as they were played.
//!
//! A front end reads each replay in the set into a [`PerGameStat`] however it
//! parses replays, feeds them here, and runs the result through the same
//! filters and aggregates the session uses.

use std::collections::HashSet;

use crate::stats::PerGameStat;

/// How far reading a replay set has got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplaySetProgress {
    /// Replays visited, whether or not they yielded a game.
    pub done: usize,
    /// Replays the set holds.
    pub total: usize,
}

impl ReplaySetProgress {
    /// How far the read has got, in `0.0..=1.0`. An empty set has not
    /// finished; dividing by its zero total would report it as complete.
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }

        (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
    }
}

/// Replays in a set that yielded no game, by why.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplaySetSkips {
    /// Would not read or parse.
    pub unreadable: usize,
    /// Recorded on a build this machine has no game data for.
    pub missing_build: usize,
    /// Parsed, but carry no game for the recording player: the battle results
    /// never arrived, or nobody was recording. The session records neither.
    pub no_results: usize,
}

impl ReplaySetSkips {
    pub fn total(self) -> usize {
        self.unreadable + self.missing_build + self.no_results
    }
}

/// The games read so far from one replay set, in `sort_key` order, which is
/// the order [`crate::stats::filter_games`] expects.
#[derive(Debug, Clone, Default)]
pub struct ReplaySetGames {
    games: Vec<PerGameStat>,
    /// `game_time` and `player_id` of every held game: the key the recorded
    /// session dedupes on.
    keys: HashSet<(String, i64)>,
}

impl ReplaySetGames {
    pub fn new() -> Self {
        Self { games: Vec::new(), keys: HashSet::new() }
    }

    /// Adds games read from the set. A game already held under the same
    /// `game_time` and `player_id` is replaced rather than counted again: a
    /// directory is walked recursively, so one replay copied into two
    /// subdirectories is one battle.
    pub fn extend(&mut self, games: impl IntoIterator<Item = PerGameStat>) {
        let mut inserted = false;
        for game in games {
            if self.keys.insert((game.game_time.clone(), game.player_id)) {
                self.games.push(game);
                inserted = true;
                continue;
            }

            // Same key, so same `sort_key`: a replacement keeps its place.
            if let Some(held) =
                self.games.iter_mut().find(|held| held.game_time == game.game_time && held.player_id == game.player_id)
            {
                *held = game;
            }
        }
        if !inserted {
            return;
        }

        // Workers finish replays in any order; the player id breaks a tie so
        // two perspectives on one battle keep a stable order.
        self.games.sort_by(|a, b| a.sort_key.cmp(&b.sort_key).then_with(|| a.player_id.cmp(&b.player_id)));
    }

    pub fn games(&self) -> &[PerGameStat] {
        &self.games
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(game_time: &str, player_id: i64, damage: u64) -> PerGameStat {
        PerGameStat {
            ship_name: "Ship".to_string(),
            ship_id: 1u64.into(),
            game_time: game_time.to_string(),
            sort_key: crate::stats::sortable_game_time(game_time),
            player_id,
            damage,
            spotting_damage: 0,
            frags: 0,
            raw_xp: 0,
            base_xp: 0,
            is_win: true,
            is_loss: false,
            is_draw: false,
            is_div: false,
            match_group: "pvp".to_string(),
            achievements: Vec::new(),
        }
    }

    /// The same replay found twice under one directory is one battle, and
    /// games arriving out of order across a month boundary still come out in
    /// the order they were played, which is what the recency limit counts
    /// back from.
    #[test]
    fn a_copied_replay_counts_once_and_games_are_in_played_order() {
        let mut set = ReplaySetGames::new();
        set.extend([game("02.03.2026 10:00:00", 7, 100), game("28.02.2026 22:00:00", 7, 200)]);
        set.extend([game("02.03.2026 10:00:00", 7, 300), game("02.03.2026 10:00:00", 8, 400)]);

        let played: Vec<(&str, i64, u64)> =
            set.games().iter().map(|game| (game.game_time.as_str(), game.player_id, game.damage)).collect();
        assert_eq!(
            played,
            [("28.02.2026 22:00:00", 7, 200), ("02.03.2026 10:00:00", 7, 300), ("02.03.2026 10:00:00", 8, 400)],
            "the copy replaces the held game, a second perspective on the battle is kept"
        );
    }
}
