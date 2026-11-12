//! The match header's figures: the recording player's stat tiles, counted up once per tab, and the
//! team damage split.

use landaire_ui::CountFormat;
use landaire_ui::CountUpProps;
use rust_i18n::t;
use wows_replay_insights::personal_rating::PersonalRatingCategory;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::separate_number;
use zgui::component;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;

use crate::theme::rating_band_name;

/// The PR tile's test id.
pub const PR_TILE_ID: &str = "replay-personal-rating-badge";

/// The PR tile's spoken text, verbatim the egui `pr_chip` call's format string.
pub fn personal_rating_label(rating: &PersonalRatingResult) -> String {
    format!("PR: {:.0} ({})", rating.pr, rating.category.name())
}

/// The recording player's own figures, read once from the parsed model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnFigures {
    damage: Option<u64>,
    base_xp: Option<i64>,
    kills: Option<i64>,
}

impl OwnFigures {
    /// `None` when the replay carries no row for the recording player.
    pub fn from_rows(rows: &[PlayerRow]) -> Option<Self> {
        let row = rows.iter().find(|row| row.is_self)?;
        Some(Self { damage: row.actual_damage, base_xp: row.base_xp, kills: row.kills })
    }
}

/// Each side's share of the damage dealt, from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageShares {
    pub allies: f32,
    pub enemies: f32,
}

impl DamageShares {
    /// `None` when neither side dealt any, which has no proportion to draw.
    pub fn new(allies: u64, enemies: u64) -> Option<Self> {
        let total = u128::from(allies) + u128::from(enemies);
        if total == 0 {
            return None;
        }
        let allies = (allies as f64 / total as f64) as f32;
        Some(Self { allies, enemies: 1.0 - allies })
    }
}

/// Each team's total damage from the results, allies first. A row without results adds nothing.
pub fn team_damage(rows: &[PlayerRow]) -> (u64, u64) {
    rows.iter().fold((0, 0), |(allies, enemies), row| {
        let dealt = row.actual_damage.unwrap_or(0);
        if row.relation.is_enemy() { (allies, enemies + dealt) } else { (allies + dealt, enemies) }
    })
}

/// The CSS colour a PR band's text is drawn in: the theme's `--wt-pr-<band>-text`.
fn tier_color(category: PersonalRatingCategory) -> String {
    format!("var(--wt-pr-{}-text)", rating_band_name(category))
}

/// The recording player's figures and Personal Rating as tiles, counted up from nothing when
/// `counting`. A figure the replay does not carry has no tile.
#[component]
pub fn StatTiles(own: Option<OwnFigures>, rating: Option<PersonalRatingResult>, counting: bool) -> impl IntoView {
    let from = counting.then_some(0.0);
    let mut tiles: Vec<AnyView> = Vec::new();
    let figures = own.map(|own| {
        [
            ("replay-damage-tile", t!("stat.damage"), own.damage.map(i128::from)),
            ("replay-base-xp-tile", t!("ui.replay.column.base_xp"), own.base_xp.map(i128::from)),
            ("replay-kills-tile", t!("ui.replay.column.kills"), own.kills.map(i128::from)),
        ]
    });
    for (id, label, value) in figures.into_iter().flatten() {
        let Some(value) = value else { continue };
        let label = label.into_owned();
        let said = separate_number(value);
        // Floored, so the count reaches the figure only as it ends.
        let format = CountFormat::new(|figure| separate_number(figure.floor() as i128));
        tiles.push(AnyView::new(view! {
            column(class = "wt-rp-tile", attr:data-testid = id, a11y:label = said) {
                text(class = "wt-rp-tile__label") {{label}}
                CountUp(value = value as f64, from = from, format = format, class = "wt-rp-tile__value")
            }
        }));
    }

    if let Some(rating) = rating {
        let label = format!("{} - {}", t!("ui.replay.column.personal_rating"), rating.category.name());
        let said = personal_rating_label(&rating);
        let color = tier_color(rating.category);
        tiles.push(AnyView::new(view! {
            column(class = "wt-rp-tile", attr:data-testid = PR_TILE_ID, a11y:label = said) {
                text(class = "wt-rp-tile__label") {{label}}
                CountUp(
                    value = rating.pr,
                    from = from,
                    format = CountFormat::decimal(0),
                    class = "wt-rp-tile__value",
                    style:color = color
                )
            }
        }));
    }

    tiles
}

/// Each side's total either side of a bar split in proportion to them, allies on the left.
#[component]
pub fn DamageSplit(allies: u64, enemies: u64) -> impl IntoView {
    let label = t!("ui.replay.team_damage").into_owned();
    let total = format!("({})", separate_number(i128::from(allies) + i128::from(enemies)));
    let shares = DamageShares::new(allies, enemies);

    view! {
        row(class = "wt-rp-split", attr:data-testid = "replay-team-damage") {
            text(class = "wt-rp-dim") {{label}}
            text(class = "wt-rp-win") {{separate_number(allies)}}
            row(class = "wt-rp-split__bar", attr:data-testid = "replay-team-damage-bar") {
                {shares.map(|shares| view! {
                    box(class = "wt-rp-split__allies", style:width = format!("{}%", shares.allies * 100.0))
                    box(class = "wt-rp-split__enemies", style:width = format!("{}%", shares.enemies * 100.0))
                })}
            }
            text(class = "wt-rp-loss") {{separate_number(enemies)}}
            text(class = "wt-rp-dim") {{total}}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DamageShares;

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
}
