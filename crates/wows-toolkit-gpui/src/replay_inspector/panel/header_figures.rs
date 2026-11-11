//! The match header's figures: a faint tint of the result across the band,
//! the recording player's stat tiles, and the team damage split.

use std::time::Duration;
use std::time::Instant;

use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::Background;
use gpui_kit::FontWeight;
use gpui_kit::Hsla;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement as _;
use gpui_kit::ParentElement as _;
use gpui_kit::SharedString;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::linear_color_stop;
use gpui_kit::linear_gradient;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use gpui_kit::relative;
use gpui_kit::rgb;
use rust_i18n::t;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingResult;

use crate::replay_inspector::columns::BattleOutcome;
use crate::replay_inspector::columns::ColorRole;
use crate::replay_inspector::model::PlayerRow;
use crate::replay_inspector::model::separate_number;
use crate::replay_inspector::table::resolve_color;

/// The PR tile's element id, so a test can assert on its presence and label.
pub(super) const PR_TILE_ID: &str = "replay-personal-rating-badge";

/// The PR tile's spoken text, verbatim the egui `pr_chip` call's format
/// string.
pub(super) fn personal_rating_label(rating: &PersonalRatingResult) -> String {
    format!("PR: {:.0} ({})", rating.pr, rating.category.name())
}

/// The recording player's own figures, read once from the parsed model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OwnFigures {
    damage: Option<u64>,
    base_xp: Option<i64>,
    kills: Option<i64>,
}

impl OwnFigures {
    /// `None` when the replay carries no row for the recording player.
    pub(super) fn from_rows(rows: &[PlayerRow]) -> Option<Self> {
        let row = rows.iter().find(|row| row.is_self)?;

        Some(Self { damage: row.actual_damage, base_xp: row.base_xp, kills: row.kills })
    }
}

/// Each side's share of the damage dealt, from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DamageShares {
    pub(super) allies: f32,
    pub(super) enemies: f32,
}

impl DamageShares {
    /// `None` when neither side dealt any, which has no proportion to draw.
    pub(super) fn new(allies: u64, enemies: u64) -> Option<Self> {
        let total = u128::from(allies) + u128::from(enemies);
        if total == 0 {
            return None;
        }

        let allies = (allies as f64 / total as f64) as f32;
        Some(Self { allies, enemies: 1.0 - allies })
    }
}

/// What one stat tile shows.
struct Tile {
    id: SharedString,
    label: String,
    value: String,
    /// What the tile reads as to assistive technology.
    said: String,
    tone: Option<Hsla>,
}

/// The header band's background: the result's tone at the leading edge,
/// fading to nothing across the width. `None` for a replay without a result.
pub(super) fn result_tint(result: Option<BattleResult>) -> Option<Background> {
    const ALPHA: f32 = 0.12;

    let outcome = match result? {
        BattleResult::Win(_) => BattleOutcome::Win,
        BattleResult::Loss(_) => BattleOutcome::Loss,
        BattleResult::Draw => BattleOutcome::Draw,
    };
    let tone = resolve_color(ColorRole::WinLoss(outcome));

    Some(linear_gradient(90., linear_color_stop(tone.opacity(ALPHA), 0.), linear_color_stop(tone.opacity(0.), 1.)))
}

/// How far the figures have counted up for a load at `started`, eased out
/// from 0 to 1. 1 once the count is over, and at once under reduced motion.
pub(super) fn count_up_progress(started: Instant, window: &mut Window, cx: &App) -> f32 {
    const COUNT_UP: Duration = Duration::from_millis(400);

    if cx.reduce_motion() {
        return 1.0;
    }

    let elapsed = cx.background_executor().now().saturating_duration_since(started);
    if elapsed >= COUNT_UP {
        return 1.0;
    }
    window.request_animation_frame();

    let linear = elapsed.as_secs_f32() / COUNT_UP.as_secs_f32();
    1.0 - (1.0 - linear).powi(3)
}

/// `target` as far as a count at `progress` has reached.
fn counted(target: i128, progress: f32) -> i128 {
    if progress >= 1.0 {
        return target;
    }

    (target as f64 * f64::from(progress.max(0.0))).round() as i128
}

/// One figure as a small card: its name over its value.
fn stat_tile(tile: Tile, cx: &App) -> AnyElement {
    let Tile { id, label, value, said, tone } = tile;

    crate::ui::card_surface(v_flex(), cx)
        .id(id)
        .test_support()
        .aria_label(said)
        .flex_none()
        .min_w(px(64.))
        .px_2()
        .py_0p5()
        .child(div().text_xs().text_color(crate::theme::text_dim()).child(label))
        // One width per figure, so a counting number does not shift sideways.
        .child(crate::ui::tabular(
            div().text_sm().font_weight(FontWeight::BOLD).when_some(tone, |el, tone| el.text_color(tone)).child(value),
        ))
        .into_any_element()
}

/// The recording player's figures and Personal Rating as tiles, counted up to
/// `progress`. A figure the replay does not carry has no tile; `own` is
/// `None` when the player's own figures are hidden or absent.
pub(super) fn stat_tiles(
    own: Option<OwnFigures>,
    rating: Option<&PersonalRatingResult>,
    progress: f32,
    cx: &App,
) -> Vec<AnyElement> {
    let mut tiles = Vec::new();

    let figures = own.map(|own| {
        [
            ("replay-damage-tile", t!("stat.damage"), own.damage.map(i128::from)),
            ("replay-base-xp-tile", t!("ui.replay.column.base_xp"), own.base_xp.map(i128::from)),
            ("replay-kills-tile", t!("ui.replay.column.kills"), own.kills.map(i128::from)),
        ]
    });
    for (id, label, value) in figures.into_iter().flatten() {
        let Some(value) = value else { continue };
        let shown = separate_number(counted(value, progress));
        let tile = Tile { id: id.into(), label: label.into_owned(), value: shown.clone(), said: shown, tone: None };
        tiles.push(stat_tile(tile, cx));
    }

    if let Some(rating) = rating {
        let tone = resolve_color(ColorRole::PrTier(rating.category));
        let shown = format!("{:.0}", rating.pr * f64::from(progress.clamp(0.0, 1.0)));
        let label = format!("{} - {}", t!("ui.replay.column.personal_rating"), rating.category.name());
        // Read as the final rating whatever the count has reached.
        let said = personal_rating_label(rating);
        let tile = Tile { id: PR_TILE_ID.into(), label, value: shown, said, tone: Some(tone) };
        tiles.push(stat_tile(tile, cx));
    }

    tiles
}

/// The team damage line: each side's total either side of a bar split in
/// proportion to them, allies on the left.
pub(super) fn damage_split(allies: u64, enemies: u64, cx: &App) -> AnyElement {
    const BAR_WIDTH: f32 = 160.;
    const BAR_HEIGHT: f32 = 6.;
    const BAR_MIN_WIDTH: f32 = 40.;

    let win = rgb(crate::theme::semantic().win);
    let loss = rgb(crate::theme::semantic().loss);
    let dim = crate::theme::text_dim();

    // The bar gives up width before the figures do, in a narrow panel.
    let bar = h_flex()
        .id("replay-team-damage-bar")
        .min_w(px(BAR_MIN_WIDTH))
        .w(px(BAR_WIDTH))
        .h(px(BAR_HEIGHT))
        .overflow_hidden()
        .rounded(px(2.))
        .border_1()
        .border_color(cx.theme().border)
        .when_some(DamageShares::new(allies, enemies), |bar, shares| {
            bar.child(div().h_full().w(relative(shares.allies)).bg(win))
                .child(div().h_full().w(relative(shares.enemies)).bg(loss))
        });

    crate::ui::tabular(h_flex())
        .id("replay-team-damage")
        .test_support()
        .min_w_0()
        .items_center()
        .gap_1()
        .text_xs()
        .child(div().flex_none().text_color(dim).child(t!("ui.replay.team_damage").into_owned()))
        .child(
            div()
                .flex_none()
                .text_color(win)
                .child(crate::ui::selectable_text("friendly-damage", separate_number(allies))),
        )
        .child(bar)
        .child(
            div()
                .flex_none()
                .text_color(loss)
                .child(crate::ui::selectable_text("enemy-damage", separate_number(enemies))),
        )
        .child(
            div()
                .flex_none()
                .text_color(dim)
                .child(format!("({})", separate_number(i128::from(allies) + i128::from(enemies)))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Allies come first, and the two shares fill the bar between them.
    #[test]
    fn the_damage_split_is_proportional_and_allies_first() {
        let shares = DamageShares::new(30_000, 10_000).expect("damage was dealt");
        assert!((shares.allies - 0.75).abs() < 1e-6, "allies dealt three quarters, got {}", shares.allies);
        assert!((shares.allies + shares.enemies - 1.0).abs() < 1e-6, "the shares fill the bar");

        let shares = DamageShares::new(0, 5).expect("damage was dealt");
        assert_eq!(shares.allies, 0.0, "allies who dealt nothing have no segment");

        assert_eq!(DamageShares::new(0, 0), None, "no damage has no proportion");
    }

    /// The count starts at nothing and ends on the figure itself.
    #[test]
    fn a_count_ends_on_its_figure() {
        assert_eq!(counted(123_456, 0.0), 0);
        assert_eq!(counted(123_456, 1.0), 123_456);
        assert_eq!(counted(-40, 1.0), -40);
        assert!(counted(1_000, 0.5) < 1_000);
    }
}
