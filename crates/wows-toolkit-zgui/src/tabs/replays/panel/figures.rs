//! The match header's figures: the recording player's stat tiles, counted up once per tab, and the
//! team damage split.

use std::time::Duration;

use rust_i18n::t;
use wows_replay_insights::personal_rating::PersonalRatingCategory;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_toolkit_replay_data::model::PlayerRow;
use wows_toolkit_replay_data::model::separate_number;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::FrameHandle;
use zgui::view::IntoView;
use zgui::view::request_frame;
use zgui::vocab::Timestamp;

/// How long the figures take to count up.
pub const COUNT_UP: Duration = Duration::from_millis(400);

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

/// `target` as far as a count at `progress` has reached.
pub fn counted(target: i128, progress: f32) -> i128 {
    if progress >= 1.0 {
        return target;
    }
    (target as f64 * f64::from(progress.max(0.0))).round() as i128
}

/// How far a count `elapsed` in has got, eased out from 0 to 1.
pub fn eased(elapsed: Duration) -> f32 {
    if elapsed >= COUNT_UP {
        return 1.0;
    }
    let linear = elapsed.as_secs_f32() / COUNT_UP.as_secs_f32();
    1.0 - (1.0 - linear).powi(3)
}

/// The progress of a count that starts now, advanced once per frame until it ends. Starts at 1
/// when `run` is false or motion is reduced. Called from a component's body.
pub fn count_up(run: bool) -> Signal<f32, LocalStorage> {
    let run = run && landaire_ui::motion_preference().get_untracked() == landaire_ui::MotionPreference::Full;
    let progress = RwSignal::new_local(if run { 0.0 } else { 1.0 });
    if run {
        let started: StoredValue<Option<Timestamp>> = StoredValue::new(None);
        let frame: StoredValue<Option<FrameHandle>, LocalStorage> = StoredValue::new_local(None);
        schedule(progress, started, frame);
    }
    progress.into()
}

fn schedule(
    progress: RwSignal<f32, LocalStorage>,
    started: StoredValue<Option<Timestamp>>,
    frame: StoredValue<Option<FrameHandle>, LocalStorage>,
) {
    let handle = request_frame(move |now| {
        let start = match started.get_value() {
            Some(start) => start,
            None => {
                started.set_value(Some(now));
                now
            }
        };
        let value = eased(now - start);
        progress.set(value);
        if value < 1.0 {
            schedule(progress, started, frame);
        }
    });
    frame.set_value(Some(handle));
}

/// The CSS colour a PR band's text is drawn in: the theme's `--wt-pr-<band>-text`.
fn tier_color(category: PersonalRatingCategory) -> String {
    format!("var(--wt-pr-{}-text)", category.name().to_lowercase().replace(' ', "-"))
}

/// The recording player's figures and Personal Rating as tiles, counted up to `progress`. A
/// figure the replay does not carry has no tile.
#[component]
pub fn StatTiles(
    own: Option<OwnFigures>,
    rating: Option<PersonalRatingResult>,
    #[prop(into)] progress: Signal<f32, LocalStorage>,
) -> impl IntoView {
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
        tiles.push(AnyView::new(view! {
            column(class = "wt-rp-tile", attr:data-testid = id, a11y:label = said) {
                text(class = "wt-rp-tile__label") {{label}}
                text(class = "wt-rp-tile__value") {{move || separate_number(counted(value, progress.get()))}}
            }
        }));
    }

    if let Some(rating) = rating {
        let label = format!("{} - {}", t!("ui.replay.column.personal_rating"), rating.category.name());
        let said = personal_rating_label(&rating);
        let color = tier_color(rating.category);
        let pr = rating.pr;
        tiles.push(AnyView::new(view! {
            column(class = "wt-rp-tile", attr:data-testid = PR_TILE_ID, a11y:label = said) {
                text(class = "wt-rp-tile__label") {{label}}
                text(class = "wt-rp-tile__value", style:color = color) {{
                    move || format!("{:.0}", pr * f64::from(progress.get().clamp(0.0, 1.0)))
                }}
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
    use std::time::Duration;

    use super::COUNT_UP;
    use super::DamageShares;
    use super::counted;
    use super::eased;

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

    /// The count eases out: it covers more than half the way in the first half of the time.
    #[test]
    fn a_count_eases_out_and_ends_on_time() {
        assert_eq!(eased(Duration::ZERO), 0.0);
        assert!(eased(COUNT_UP / 2) > 0.5);
        assert_eq!(eased(COUNT_UP), 1.0);
    }
}
