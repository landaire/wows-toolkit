//! Which battles make up the session, under the scope the reader chose.
//!
//! The saved session is the `session_stats` table: the battles played while
//! the scope is Manual and the replays the reader set or added. A session
//! since the app opened is held in memory for this run only. Choosing it
//! leaves the saved session unchanged, so switching back to Manual shows the
//! saved session as it was, without the battles played in between.

use jiff::civil::DateTime;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::GameParamId;

use crate::stats::PerGameStat;
use crate::stats::replay_set::ReplaySetGames;

/// Which battles the session holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionScope {
    /// The saved session: battles played, and the replays the reader set or added.
    #[default]
    Manual,
    /// Every battle that started at or after this run of the app opened.
    SinceAppOpened,
}

impl SessionScope {
    /// Every choice, in the order the controls list them.
    pub const ALL: [Self; 2] = [Self::Manual, Self::SinceAppOpened];

    /// The catalogue key this choice's name lives under.
    pub const fn translation_key(self) -> &'static str {
        match self {
            Self::Manual => "ui.stats.session_manual",
            Self::SinceAppOpened => "ui.stats.session_since_opened",
        }
    }
}

/// The local wall-clock time this run of the app opened. Never saved: each run has its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AppOpened(DateTime);

impl AppOpened {
    pub fn new(at: DateTime) -> Self {
        Self(at)
    }

    pub fn now() -> Self {
        Self(jiff::Zoned::now().datetime())
    }

    pub fn at(self) -> DateTime {
        self.0
    }

    /// Whether a battle that started at `game_time`, the replay's own local
    /// `dd.mm.yyyy HH:MM:SS`, started at or after the app opened. A time that
    /// does not read is not admitted: nothing says the battle is recent.
    pub fn admits(self, game_time: &str) -> bool {
        battle_start(game_time).is_some_and(|start| start >= self.0)
    }
}

/// When the battle started, from the replay's `dd.mm.yyyy HH:MM:SS`.
pub fn battle_start(game_time: &str) -> Option<DateTime> {
    DateTime::strptime("%d.%m.%Y %H:%M:%S", game_time).ok()
}

/// The session's games under either scope.
#[derive(Debug, Clone)]
pub struct SessionBattles {
    opened: AppOpened,
    /// What `session_stats` holds.
    saved: ReplaySetGames,
    /// Battles read this run that started once the app opened, whatever the scope.
    since_opened: ReplaySetGames,
}

impl SessionBattles {
    pub fn new(opened: AppOpened) -> Self {
        Self { opened, saved: ReplaySetGames::new(), since_opened: ReplaySetGames::new() }
    }

    pub fn opened(&self) -> AppOpened {
        self.opened
    }

    /// The session's games under `scope`, oldest first.
    pub fn games(&self, scope: SessionScope) -> &[PerGameStat] {
        match scope {
            SessionScope::Manual => self.saved.games(),
            SessionScope::SinceAppOpened => self.since_opened.games(),
        }
    }

    /// Adds games to the saved session. Also takes the rows read at startup:
    /// a battle recorded before that read landed was written too, so it is
    /// the same row and replaces itself.
    pub fn save(&mut self, games: impl IntoIterator<Item = PerGameStat>) {
        self.saved.extend(games);
    }

    pub fn clear_saved(&mut self) {
        self.saved.retain(|_| false);
    }

    pub fn clear_saved_ship(&mut self, ship: GameParamId) {
        self.saved.retain(|game| game.ship_id != ship);
    }

    /// A battle read this run from the game's replays directory. It joins the
    /// session since the app opened when it started at or after the app did.
    /// It never joins the saved session here: that takes a written row.
    pub fn listed(&mut self, game: PerGameStat) {
        if self.opened.admits(&game.game_time) {
            self.since_opened.extend([game]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> AppOpened {
        AppOpened::new(jiff::civil::date(2026, 11, 16).at(14, 32, 0, 0))
    }

    fn game(game_time: &str, ship: u64) -> PerGameStat {
        PerGameStat {
            ship_name: "Ship".to_string(),
            ship_id: ship.into(),
            game_time: game_time.to_string(),
            sort_key: crate::stats::sortable_game_time(game_time),
            player_id: 7,
            damage: 1,
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
            match_contribution: None,
        }
    }

    fn times(games: &[PerGameStat]) -> Vec<&str> {
        games.iter().map(|game| game.game_time.as_str()).collect()
    }

    #[test]
    fn a_battle_belongs_from_the_second_the_app_opened() {
        let opened = opened();

        assert!(!opened.admits("16.11.2026 14:31:59"), "a battle started a second before");
        assert!(opened.admits("16.11.2026 14:32:00"), "a battle started as the app opened");
        assert!(opened.admits("17.11.2026 00:05:00"), "a battle after midnight");
        assert!(!opened.admits("15.11.2026 23:59:59"), "a battle the day before");
        assert!(!opened.admits("not a time"), "a time that does not read");
    }

    #[test]
    fn battles_started_before_the_app_opened_stay_out_of_the_session_since_then() {
        let mut battles = SessionBattles::new(opened());
        battles.save([game("10.11.2026 20:00:00", 1)]);

        battles.listed(game("16.11.2026 15:30:00", 2));
        battles.listed(game("16.11.2026 14:00:00", 3));
        battles.listed(game("16.11.2026 15:00:00", 1));

        assert_eq!(
            times(battles.games(SessionScope::SinceAppOpened)),
            ["16.11.2026 15:00:00", "16.11.2026 15:30:00"],
            "the battle started before the app opened stays out, and the rest are in played order"
        );
        assert_eq!(times(battles.games(SessionScope::Manual)), ["10.11.2026 20:00:00"]);
    }

    #[test]
    fn switching_scope_keeps_the_saved_session() {
        let mut battles = SessionBattles::new(opened());
        battles.save([game("01.11.2026 20:00:00", 1), game("02.11.2026 20:00:00", 2)]);

        battles.listed(game("16.11.2026 16:00:00", 3));
        battles.listed(game("16.11.2026 10:00:00", 4));
        battles.clear_saved_ship(2_u64.into());

        assert_eq!(times(battles.games(SessionScope::SinceAppOpened)), ["16.11.2026 16:00:00"]);
        assert_eq!(
            times(battles.games(SessionScope::Manual)),
            ["01.11.2026 20:00:00"],
            "a listed replay never joins the saved session"
        );

        battles.clear_saved();
        assert!(battles.games(SessionScope::Manual).is_empty());
        assert_eq!(battles.games(SessionScope::SinceAppOpened).len(), 1, "clearing the saved session keeps this run's");
    }

    #[test]
    fn a_battle_read_again_with_its_results_replaces_itself() {
        let mut battles = SessionBattles::new(opened());
        battles.listed(game("16.11.2026 15:00:00", 1));
        let mut finished = game("16.11.2026 15:00:00", 1);
        finished.damage = 90_000;
        battles.listed(finished);

        let damage: Vec<u64> = battles.games(SessionScope::SinceAppOpened).iter().map(|game| game.damage).collect();
        assert_eq!(damage, [90_000]);
    }

    #[test]
    fn the_scope_reads_back_as_saved() {
        for scope in SessionScope::ALL {
            let json = serde_json::to_string(&scope).expect("the scope serializes");
            assert_eq!(serde_json::from_str::<SessionScope>(&json).expect("the scope reads back"), scope);
        }
        assert_eq!(serde_json::to_string(&SessionScope::SinceAppOpened).expect("serializes"), "\"SinceAppOpened\"");
    }
}
