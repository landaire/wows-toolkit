//! Win records split by the battle kinds a rating applies to.
//!
//! The win rate shown beside a personal rating counts the battles the rating
//! counts: Random and Ranked. The other modes are kept apart so the rate can
//! be broken down on hover rather than diluted by them.

use rust_i18n::t;
use wowsunpack::rating_scope::RatedBattle;
use wowsunpack::rating_scope::RatingScope;

use super::PerGameStat;

/// Wins, losses and draws. A battle with no result counts for none of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Record {
    pub wins: usize,
    pub losses: usize,
    pub draws: usize,
}

impl Record {
    fn add(&mut self, game: &PerGameStat) {
        if game.is_win {
            self.wins += 1;
        } else if game.is_loss {
            self.losses += 1;
        } else if game.is_draw {
            self.draws += 1;
        }
    }

    fn plus(self, other: Self) -> Self {
        Self { wins: self.wins + other.wins, losses: self.losses + other.losses, draws: self.draws + other.draws }
    }

    /// Battles that reached a result.
    pub fn battles(&self) -> usize {
        self.wins + self.losses + self.draws
    }

    /// `None` when no battle reached a result, rather than a zero percent
    /// that reads as having lost them all.
    pub fn win_rate(&self) -> Option<f64> {
        let battles = self.battles();
        (battles > 0).then(|| self.wins as f64 / battles as f64 * 100.0)
    }

    /// `3W/1L`, with draws only when there are any.
    pub fn label(&self) -> String {
        if self.draws > 0 {
            format!("{}W/{}L/{}D", self.wins, self.losses, self.draws)
        } else {
            format!("{}W/{}L", self.wins, self.losses)
        }
    }
}

/// A set of games' records, one per battle kind the hover names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordBreakdown {
    pub random: Record,
    pub ranked: Record,
    /// Every battle kind a rating does not apply to, combined.
    pub other: Record,
}

impl RecordBreakdown {
    pub fn from_games<'a>(games: impl IntoIterator<Item = &'a PerGameStat>) -> Self {
        let mut breakdown = Self::default();
        for game in games {
            breakdown.add(game);
        }

        breakdown
    }

    pub(super) fn add(&mut self, game: &PerGameStat) {
        let record = match game.rating_scope() {
            RatingScope::Rated(RatedBattle::Random) => &mut self.random,
            RatingScope::Rated(RatedBattle::Ranked) => &mut self.ranked,
            RatingScope::Unrated => &mut self.other,
        };
        record.add(game);
    }

    /// Random and Ranked together, which is the record a shown win rate
    /// counts. With no rated battle its win rate is `None`, and the line it
    /// would fill is left out.
    pub fn rated(&self) -> Record {
        self.random.plus(self.ranked)
    }

    /// What the shown win rate counts, then each battle kind played with its
    /// battles and win rate.
    pub fn hover_text(&self) -> String {
        let kinds = [
            (t!("ui.stats.mode_random"), self.random),
            (t!("ui.stats.mode_ranked"), self.ranked),
            (t!("ui.stats.mode_other"), self.other),
        ];

        let mut lines = vec![t!("ui.stats.win_rate_rated_only").into_owned()];
        for (kind, record) in kinds {
            let Some(rate) = record.win_rate() else { continue };
            lines.push(
                t!(
                    "ui.stats.win_rate_breakdown_line",
                    mode = kind,
                    battles = record.battles(),
                    rate = format!("{rate:.1}%")
                )
                .into_owned(),
            );
        }

        lines.join("\n")
    }
}
