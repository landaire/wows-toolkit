//! Each team's sum of a player table's figures, and a row's share of its team's sum.

use std::borrow::Borrow;

use wows_replays::types::TeamId;
use wt_translations::t;

use crate::columns::ReplayColumn;
use crate::model::PlayerRow;

/// The two teams, as the replay's owner sees them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamSide {
    Allies,
    Enemies,
}

impl TeamSide {
    pub fn of(row: &PlayerRow, self_team: TeamId) -> Self {
        if row.team_id == self_team { Self::Allies } else { Self::Enemies }
    }

    pub fn label(self) -> String {
        match self {
            Self::Allies => t!("ui-player_tracker-allies").into_owned(),
            Self::Enemies => t!("ui-player_tracker-enemies").into_owned(),
        }
    }
}

/// Where one team's run of rows begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamRun {
    pub side: TeamSide,
    pub start: usize,
}

/// The run each team's rows form, in table order, or `None` when the sort
/// interleaves the teams and a heading would split a team in two.
pub fn team_runs<R>(rows: &[R], self_team: TeamId) -> Option<Vec<TeamRun>>
where
    R: Borrow<PlayerRow>,
{
    let mut runs: Vec<TeamRun> = Vec::new();
    for (start, row) in rows.iter().enumerate() {
        let side = TeamSide::of(row.borrow(), self_team);
        if runs.last().is_some_and(|run| run.side == side) {
            continue;
        }
        if runs.iter().any(|run| run.side == side) {
            return None;
        }
        runs.push(TeamRun { side, start });
    }

    Some(runs)
}

/// Each team's sum of one figure over its rows. `None` for a team none of
/// whose rows has the figure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TeamDamage {
    pub allies: Option<u64>,
    pub enemies: Option<u64>,
}

impl TeamDamage {
    pub fn summing<F>(rows: &[PlayerRow], self_team: TeamId, figure: F) -> Self
    where
        F: Fn(&PlayerRow) -> Option<u64>,
    {
        let mut damage = Self::default();
        for row in rows {
            let Some(amount) = figure(row) else { continue };

            let total = damage.side_mut(TeamSide::of(row, self_team));
            *total = Some(total.map_or(amount, |sum| sum.saturating_add(amount)));
        }

        damage
    }

    pub fn side(self, side: TeamSide) -> Option<u64> {
        match side {
            TeamSide::Allies => self.allies,
            TeamSide::Enemies => self.enemies,
        }
    }

    fn side_mut(&mut self, side: TeamSide) -> &mut Option<u64> {
        match side {
            TeamSide::Allies => &mut self.allies,
            TeamSide::Enemies => &mut self.enemies,
        }
    }
}

/// The figure a column draws a share bar for, when the cell shows it. A
/// hidden figure gets no bar and is left out of its team's total, as it is
/// from the team heading.
pub fn bar_figure(row: &PlayerRow, col: ReplayColumn, debug: bool) -> Option<u64> {
    let hidden = row.should_hide_stats() && !debug;
    match col {
        ReplayColumn::ActualDamage if !hidden => row.actual_damage,
        ReplayColumn::ReceivedDamage if !hidden => row.received_damage,
        ReplayColumn::PotentialDamage if !hidden => row.potential_damage,
        // Shown on every row: it is damage allies dealt on this player's spotting.
        ReplayColumn::SpottingDamage => row.spotting_damage,
        _ => None,
    }
}

/// A row's figure as part of its own team's total in that column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeamShare {
    pub side: TeamSide,
    /// From 0 to 1.
    pub fraction: f32,
}

impl TeamShare {
    pub fn text(self) -> String {
        let pct = format!("{:.0}%", self.fraction * 100.);
        t!("ui-replay-team_share", pct = pct, team = self.side.label()).into_owned()
    }
}

/// A row's figure relative to the greatest figure on its own team.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeamScale {
    /// From 0 to 1.
    pub fraction: f32,
}

impl TeamScale {
    pub const FULL: Self = Self { fraction: 1.0 };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TeamColumn {
    total: TeamDamage,
    greatest: TeamDamage,
}

impl TeamColumn {
    fn of(rows: &[PlayerRow], self_team: TeamId, col: ReplayColumn, debug: bool) -> Self {
        let mut column = Self::default();
        for row in rows {
            let Some(amount) = bar_figure(row, col, debug) else { continue };
            let side = TeamSide::of(row, self_team);
            let total = column.total.side_mut(side);
            *total = Some(total.map_or(amount, |sum| sum.saturating_add(amount)));
            let greatest = column.greatest.side_mut(side);
            *greatest = Some(greatest.map_or(amount, |current| current.max(amount)));
        }

        column
    }
}

/// Each barred column's total and greatest figure for each team, indexed by
/// `ReplayColumn as usize`. The team heading shows the Actual Damage total,
/// so a bar and its heading count the same rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamTotals {
    self_team: TeamId,
    columns: Vec<TeamColumn>,
}

impl TeamTotals {
    pub fn of(rows: &[PlayerRow], self_team: TeamId, debug: bool) -> Self {
        let columns = ReplayColumn::ALL.into_iter().map(|col| TeamColumn::of(rows, self_team, col, debug)).collect();

        Self { self_team, columns }
    }

    pub fn column(&self, col: ReplayColumn) -> TeamDamage {
        self.columns[col as usize].total
    }

    /// `row`'s figure in `col` against its team's total. `None` where there
    /// is no bar to draw.
    pub fn share(&self, row: &PlayerRow, col: ReplayColumn, debug: bool) -> Option<TeamShare> {
        let figure = bar_figure(row, col, debug)?;
        let side = TeamSide::of(row, self.self_team);
        let total = self.column(col).side(side)?;

        (figure > 0 && total > 0).then(|| TeamShare { side, fraction: (figure as f64 / total as f64) as f32 })
    }

    /// `row`'s figure in `col` against the greatest figure on its team.
    pub fn scale(&self, row: &PlayerRow, col: ReplayColumn, debug: bool) -> Option<TeamScale> {
        let figure = bar_figure(row, col, debug)?;
        let side = TeamSide::of(row, self.self_team);
        let greatest = self.columns[col as usize].greatest.side(side)?;

        (figure > 0 && greatest > 0).then(|| TeamScale { fraction: (figure as f64 / greatest as f64) as f32 })
    }
}

#[cfg(test)]
mod tests {
    use wows_replays::types::Relation;

    use super::*;
    use crate::test_support::base_row;

    fn row(id: i64, team: i64, damage: Option<u64>) -> PlayerRow {
        let relation = if team == 0 { Relation::new(1) } else { Relation::new(2) };
        PlayerRow { team_id: TeamId::from(team), actual_damage: damage, ..base_row(id, relation, false) }
    }

    #[test]
    fn grouped_teams_each_get_one_run_in_table_order() {
        let rows = [row(1, 0, None), row(2, 0, None), row(3, 1, None), row(4, 1, None)];

        assert_eq!(
            team_runs(&rows, TeamId::from(0i64)),
            Some(vec![TeamRun { side: TeamSide::Allies, start: 0 }, TeamRun { side: TeamSide::Enemies, start: 2 }])
        );
    }

    #[test]
    fn interleaved_teams_get_no_runs() {
        let rows = [row(1, 0, None), row(2, 1, None), row(3, 0, None)];

        assert_eq!(team_runs(&rows, TeamId::from(0i64)), None);
    }

    #[test]
    fn team_damage_sums_each_side() {
        let rows = [row(1, 0, Some(3_000)), row(2, 0, Some(1_000)), row(3, 1, Some(4_000))];

        let damage = TeamDamage::summing(&rows, TeamId::from(0i64), |row| row.actual_damage);

        assert_eq!(damage, TeamDamage { allies: Some(4_000), enemies: Some(4_000) });
    }

    #[test]
    fn a_team_without_figures_has_no_total() {
        let rows = [row(1, 0, Some(3_000)), row(2, 1, None)];

        let damage = TeamDamage::summing(&rows, TeamId::from(0i64), |row| row.actual_damage);

        assert_eq!(damage.side(TeamSide::Enemies), None);
    }

    #[test]
    fn a_share_is_of_the_rows_own_team() {
        let rows = [row(1, 0, Some(3_000)), row(2, 0, Some(1_000)), row(3, 1, Some(4_000))];
        let totals = TeamTotals::of(&rows, TeamId::from(0i64), false);

        let share = totals.share(&rows[0], ReplayColumn::ActualDamage, false).expect("damage was dealt");

        assert_eq!(share, TeamShare { side: TeamSide::Allies, fraction: 0.75 });
        assert_eq!(totals.share(&rows[0], ReplayColumn::Kills, false), None);
    }

    #[test]
    fn a_scale_is_against_the_largest_figure_on_the_rows_own_team() {
        let rows = [row(1, 0, Some(3_000)), row(2, 0, Some(1_000)), row(3, 1, Some(4_000))];
        let totals = TeamTotals::of(&rows, TeamId::from(0i64), false);

        assert_eq!(totals.scale(&rows[0], ReplayColumn::ActualDamage, false), Some(TeamScale::FULL));
        assert_eq!(totals.scale(&rows[1], ReplayColumn::ActualDamage, false), Some(TeamScale { fraction: 1.0 / 3.0 }));
        assert_eq!(totals.scale(&rows[2], ReplayColumn::ActualDamage, false), Some(TeamScale::FULL));
        assert_eq!(totals.scale(&rows[0], ReplayColumn::Kills, false), None);
    }
}
