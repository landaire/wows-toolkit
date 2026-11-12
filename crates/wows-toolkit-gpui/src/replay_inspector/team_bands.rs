//! Team structure in the player table: a stripe in each row's gutter for
//! the player's side, and a heading over each team's run of rows with that
//! team's damage.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::h_flex;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use wows_replays::types::TeamId;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::separate_number;
use wows_toolkit_replay_data::team_totals::TeamDamage;
use wows_toolkit_replay_data::team_totals::TeamRun;
use wows_toolkit_replay_data::team_totals::TeamSide;

/// Width of the side stripe in a row's left gutter.
const STRIPE_WIDTH: Pixels = px(3.);

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

/// The stripe in a row's left gutter. Absolute, so the row it is placed in
/// must be `relative`; it spans the row's full height, detail included.
pub fn stripe(mark: RowMark, cx: &App) -> impl IntoElement {
    div().absolute().left_0().top_0().bottom_0().w(STRIPE_WIDTH).bg(mark.color(cx))
}

fn side_mark(side: TeamSide) -> RowMark {
    match side {
        TeamSide::Allies => RowMark::Ally,
        TeamSide::Enemies => RowMark::Enemy,
    }
}

/// The heading drawn over a team's first row: the team's name and its
/// damage. The match header compares the two teams already.
pub fn heading(run: TeamRun, damage: TeamDamage, cx: &App) -> AnyElement {
    let total = damage.side(run.side).map(separate_number);

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
        .child(stripe(side_mark(run.side), cx))
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
        .into_any_element()
}
