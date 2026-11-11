//! Team structure in the player table: a stripe in each row's gutter for
//! the player's side, and a heading over each team's run of rows with that
//! team's damage against the other's.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::h_flex;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_replays::types::TeamId;

use super::model::PlayerRow;
use super::model::separate_number;

/// Width of the side stripe in a row's left gutter.
const STRIPE_WIDTH: Pixels = px(3.);

/// Size of the bar comparing the two teams' damage.
const SPLIT_BAR_WIDTH: Pixels = px(96.);
const SPLIT_BAR_HEIGHT: Pixels = px(4.);

/// Which side of the battle a row stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowMark {
    /// The player who recorded the replay.
    Owner,
    Ally,
    Enemy,
}

impl RowMark {
    pub fn of(row: &PlayerRow, self_team: TeamId) -> Self {
        if row.is_self {
            Self::Owner
        } else if TeamSide::of(row, self_team) == TeamSide::Allies {
            Self::Ally
        } else {
            Self::Enemy
        }
    }

    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Owner => crate::theme::accent(),
            Self::Ally => cx.theme().success,
            Self::Enemy => cx.theme().danger,
        }
    }
}

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
            Self::Allies => t!("ui.player_tracker.allies").into_owned(),
            Self::Enemies => t!("ui.player_tracker.enemies").into_owned(),
        }
    }

    fn mark(self) -> RowMark {
        match self {
            Self::Allies => RowMark::Ally,
            Self::Enemies => RowMark::Enemy,
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
pub fn team_runs(rows: &[PlayerRow], self_team: TeamId) -> Option<Vec<TeamRun>> {
    let mut runs: Vec<TeamRun> = Vec::new();
    for (start, row) in rows.iter().enumerate() {
        let side = TeamSide::of(row, self_team);
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

            let total = match TeamSide::of(row, self_team) {
                TeamSide::Allies => &mut damage.allies,
                TeamSide::Enemies => &mut damage.enemies,
            };
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

    /// The allies' share of both teams' damage, from 0 to 1. `None` unless
    /// both teams have a figure and together they dealt some damage.
    pub fn allies_share(self) -> Option<f32> {
        let (allies, enemies) = (self.allies?, self.enemies?);
        let total = u128::from(allies) + u128::from(enemies);
        (total > 0).then(|| (allies as f64 / total as f64) as f32)
    }
}

/// The stripe in a row's left gutter. Absolute, so the row it is placed in
/// must be `relative`; it spans the row's full height, detail included.
pub fn stripe(mark: RowMark, cx: &App) -> impl IntoElement {
    div().absolute().left_0().top_0().bottom_0().w(STRIPE_WIDTH).bg(mark.color(cx))
}

/// The heading drawn over a team's first row: the team's name, its damage,
/// and a bar of the allies' damage against the enemies'.
pub fn heading(run: TeamRun, damage: TeamDamage, cx: &App) -> AnyElement {
    let total = damage.side(run.side).map(separate_number);
    let split = damage.allies_share().map(|share| split_bar(share, cx));

    h_flex()
        .id(("replay-team-heading", run.start))
        .relative()
        .w_full()
        .gap_2()
        .pl_2()
        .pr_1()
        .py_0p5()
        .border_t_1()
        .border_b_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().table_head)
        .text_xs()
        .text_color(cx.theme().table_head_foreground)
        .child(stripe(run.side.mark(), cx))
        .child(div().font_weight(FontWeight::BOLD).child(run.side.label()))
        .when_some(total, |el, total| {
            let hover = SharedString::from(format!("{}{total}", t!("ui.replay.team_damage")));
            el.child(
                div()
                    .id(("replay-team-damage", run.start))
                    .child(total)
                    .tooltip(move |window, cx| Tooltip::new(hover.clone()).build(window, cx)),
            )
        })
        .children(split)
        .into_any_element()
}

/// Two segments in the teams' colours, the allies' on the left.
fn split_bar(allies_share: f32, cx: &App) -> impl IntoElement {
    h_flex()
        .flex_none()
        .w(SPLIT_BAR_WIDTH)
        .h(SPLIT_BAR_HEIGHT)
        .child(div().h_full().w(relative(allies_share)).bg(RowMark::Ally.color(cx)))
        .child(div().h_full().flex_1().bg(RowMark::Enemy.color(cx)))
}

#[cfg(test)]
mod tests {
    use wows_replays::types::Relation;

    use super::super::test_support::base_row;
    use super::PlayerRow;
    use super::TeamDamage;
    use super::TeamId;
    use super::TeamRun;
    use super::TeamSide;
    use super::team_runs;

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
        assert_eq!(damage.allies_share(), Some(0.5));
    }

    #[test]
    fn a_team_without_figures_has_no_total_and_no_split() {
        let rows = [row(1, 0, Some(3_000)), row(2, 1, None)];

        let damage = TeamDamage::summing(&rows, TeamId::from(0i64), |row| row.actual_damage);

        assert_eq!(damage.enemies, None);
        assert_eq!(damage.allies_share(), None);
    }
}
