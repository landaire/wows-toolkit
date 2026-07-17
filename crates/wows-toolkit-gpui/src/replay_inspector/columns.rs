//! Column set, cell formatting, and number formatting. Mirrors
//! `ui/replay_parser/sorting.rs` (`ReplayColumn`, declaration order) and the
//! per-cell mapping in `ui/replay_parser/mod.rs`'s `TableDelegate::cell_ui`.
//! Colors are represented as `ColorRole`, resolved to real colors by the
//! render layer (Milestone 2); no `gpui`/`egui` color type appears here.

use wows_replay_insights::personal_rating::PersonalRatingCategory;
use wows_replays::types::Relation;
use wows_toolkit_config::ReplaySettings;

use super::model::PlayerRow;
pub use super::model::separate_number;

/// All displayable columns in the replay player list, in the same
/// declaration order as the egui app's `ReplayColumn` (`sorting.rs:101`).
/// `default_columns` re-sorts to this order after filtering, matching
/// `update_visible_columns`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReplayColumn {
    Actions,
    Name,
    ShipName,
    Skills,
    PersonalRating,
    BaseXp,
    RawXp,
    Kills,
    ObservedDamage,
    ActualDamage,
    ReceivedDamage,
    SpottingDamage,
    PotentialDamage,
    Hits,
    Heals,
    DistanceTraveled,
    TimeLived,
}

impl ReplayColumn {
    pub const ALL: [ReplayColumn; 17] = [
        ReplayColumn::Actions,
        ReplayColumn::Name,
        ReplayColumn::ShipName,
        ReplayColumn::Skills,
        ReplayColumn::PersonalRating,
        ReplayColumn::BaseXp,
        ReplayColumn::RawXp,
        ReplayColumn::Kills,
        ReplayColumn::ObservedDamage,
        ReplayColumn::ActualDamage,
        ReplayColumn::ReceivedDamage,
        ReplayColumn::SpottingDamage,
        ReplayColumn::PotentialDamage,
        ReplayColumn::Hits,
        ReplayColumn::Heals,
        ReplayColumn::DistanceTraveled,
        ReplayColumn::TimeLived,
    ];

    /// The `ReplaySettings` flag that gates this column's visibility, for the
    /// five user-toggleable columns. `None` for always-on columns. Mirrors
    /// `update_visible_columns`'s `optional_columns` table.
    fn settings_gate(self) -> Option<fn(&ReplaySettings) -> bool> {
        match self {
            ReplayColumn::RawXp => Some(|s| s.show_raw_xp),
            ReplayColumn::ObservedDamage => Some(|s| s.show_observed_damage),
            ReplayColumn::Heals => Some(|s| s.show_heals),
            ReplayColumn::ReceivedDamage => Some(|s| s.show_received_damage),
            ReplayColumn::DistanceTraveled => Some(|s| s.show_distance_traveled),
            _ => None,
        }
    }
}

/// The column set for a fresh replay load, given the persisted display
/// settings: always-on columns unconditionally, the five optional columns
/// only when their `ReplaySettings` flag is set, in declaration order.
/// Mirrors `UiReport::update_visible_columns`.
pub fn default_columns(settings: &ReplaySettings) -> Vec<ReplayColumn> {
    ReplayColumn::ALL.into_iter().filter(|col| col.settings_gate().is_none_or(|gate| gate(settings))).collect()
}

/// A color role a cell can carry, resolved to a real color by the render
/// layer (Milestone 2's palette resolver). Never an actual RGBA/HSLA value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorRole {
    /// Team/division-relative player color for stat cells and (via
    /// `PlayerColorKind::Abuser`) the name cell.
    Player(PlayerColorKind),
    /// Personal Rating tier tint.
    PrTier(PersonalRatingCategory),
    /// Captain-points tier tint.
    CaptainPoints(CaptainPointsTier),
    /// Battle-outcome coloring (win/loss/draw), for the per-replay header.
    WinLoss(BattleOutcome),
    /// A packed `0xRRGGBB` color read verbatim from replay/game data (e.g.
    /// the clan-league color).
    Fixed(u32),
}

/// Team/division/abuse-relative player color. Self = white, ally = light
/// green, enemy = light red, division-mate (not self) = gold, abuser = pink.
/// Mirrors `player_color_for_team_relation` plus the division/abuser
/// overrides in `ui/replay_parser/mod.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerColorKind {
    SelfPlayer,
    Ally,
    Enemy,
    DivisionMate,
    Abuser,
}

/// Captain-points tier tint. 0-9 = Bad, 10-12 = Warning, 13-16 = Caution,
/// 17+ = Good; forced `Bad` on the "tower defense" / "no skills above tier
/// 2" warnings regardless of point count. Mirrors
/// `util::colorize_captain_points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptainPointsTier {
    Bad,
    Warning,
    Caution,
    Good,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleOutcome {
    Win,
    Loss,
    Draw,
}

pub(crate) const NDA: &str = "NDA";
const DASH: &str = "-";
const HEALS_TOOLTIP: &str = "Number of Repair Party activations observed for this player. May be inaccurate for ships not rendered on screen (i.e. enemy ships that were never spotted).";
const HEALS_NO_REPAIR_TOOLTIP: &str = "This ship does not have a Repair Party consumable.";
const NOT_SPOTTED_TOOLTIP: &str = "This ship was never spotted. Build info unavailable.";

/// Display text, optional color role, and optional hover text for one cell.
/// Plain data: no gpui/egui type appears here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellValue {
    pub text: String,
    pub color: Option<ColorRole>,
    pub hover: Option<String>,
}

impl CellValue {
    fn plain(text: impl Into<String>) -> Self {
        CellValue { text: text.into(), color: None, hover: None }
    }

    fn colored(text: impl Into<String>, color: ColorRole) -> Self {
        CellValue { text: text.into(), color: Some(color), hover: None }
    }

    fn with_hover(mut self, hover: impl Into<String>) -> Self {
        self.hover = Some(hover.into());
        self
    }
}

/// Packed `0xRRGGBB` color for the self/ally/enemy team-relation triad:
/// self = white, ally = light green, enemy = light red. The single source of
/// truth for these three values; `table.rs::resolve_color` (the
/// `PlayerColorKind::SelfPlayer`/`Ally`/`Enemy` arms), `chat.rs`'s sender-name
/// color, and `model.rs`'s clan-color fallback all resolve through this
/// instead of each re-deriving the same three branches.
pub(crate) fn relation_color_rgb(relation: Relation) -> u32 {
    if relation.is_self() {
        0xffffff
    } else if relation.is_ally() {
        0x90ee90
    } else {
        0xff8080
    }
}

/// The team/division-relative color for stat cells (not the name cell, which
/// additionally turns pink for abusers via `name_color_kind`). `pub(crate)`
/// so the render layer (`table.rs`) can reuse it for the multi-segment Name
/// cell, which colors its ship-icon/name segments individually instead of
/// through a single `cell_value` string.
pub(crate) fn player_color_kind(row: &PlayerRow) -> PlayerColorKind {
    if row.is_self_division_mate {
        return PlayerColorKind::DivisionMate;
    }
    if row.relation.is_self() {
        PlayerColorKind::SelfPlayer
    } else if row.relation.is_ally() {
        PlayerColorKind::Ally
    } else {
        PlayerColorKind::Enemy
    }
}

/// The name cell's color: pink for abusers, otherwise the same team/division
/// color as the stat cells. `pub(crate)`; see `player_color_kind`.
pub(crate) fn name_color_kind(row: &PlayerRow) -> PlayerColorKind {
    if row.is_abuser { PlayerColorKind::Abuser } else { player_color_kind(row) }
}

fn captain_points_tier(row: &PlayerRow) -> CaptainPointsTier {
    if row.skill_warning {
        return CaptainPointsTier::Bad;
    }
    match row.skill_points {
        0..=9 => CaptainPointsTier::Bad,
        10..=12 => CaptainPointsTier::Warning,
        13..=16 => CaptainPointsTier::Caution,
        _ => CaptainPointsTier::Good,
    }
}

/// Cell text/color/hover for one column of one row. `debug` mirrors the
/// app's debug-mode flag, which lifts NDA hiding and the enemy-only Skills
/// gate (the brief's sketch omits this parameter, but NDA gating and the
/// egui source both key off it alongside `should_hide_stats()`). Mirrors the
/// per-column mapping in `ui/replay_parser/mod.rs`'s `TableDelegate::cell_ui`
/// (values only; icons, menus, and multi-segment name coloring are
/// Milestone 2).
pub fn cell_value(row: &PlayerRow, col: ReplayColumn, debug: bool) -> CellValue {
    let nda_active = row.should_hide_stats() && !debug;

    match col {
        ReplayColumn::Actions => CellValue::plain(""),
        ReplayColumn::Name => {
            let mut parts = Vec::new();
            if let Some(div) = row.division_label.as_ref() {
                parts.push(div.clone());
            }
            if let Some(clan) = row.clan_tag.as_ref() {
                parts.push(clan.clone());
            }
            parts.push(row.display_name.clone());
            CellValue::colored(parts.join(" "), ColorRole::Player(name_color_kind(row)))
        }
        ReplayColumn::ShipName => CellValue::plain(row.ship_name.clone()),
        ReplayColumn::Skills => {
            if row.relation.is_enemy() && !debug {
                CellValue::plain(DASH)
            } else if !row.has_vehicle_entity {
                CellValue::colored(DASH, ColorRole::CaptainPoints(CaptainPointsTier::Bad))
                    .with_hover(NOT_SPOTTED_TOOLTIP)
            } else {
                let mut cell = CellValue::colored(
                    row.skill_label_text.clone(),
                    ColorRole::CaptainPoints(captain_points_tier(row)),
                );
                if let Some(hover) = row.skill_hover_text.as_ref() {
                    cell = cell.with_hover(hover.clone());
                }
                cell
            }
        }
        ReplayColumn::PersonalRating => match row.personal_rating.as_ref() {
            Some(pr) => CellValue::colored(format!("{:.0}", pr.pr), ColorRole::PrTier(pr.category))
                .with_hover(pr.category.name()),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::BaseXp => match row.base_xp_text.as_ref() {
            Some(text) => CellValue::colored(text.clone(), ColorRole::Player(player_color_kind(row))),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::RawXp => match row.raw_xp_text.as_ref() {
            Some(text) => CellValue::plain(text.clone()),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::Kills => CellValue::plain(row.kills.unwrap_or(row.observed_kills).to_string()),
        ReplayColumn::ObservedDamage => {
            if nda_active {
                CellValue::plain(NDA)
            } else {
                CellValue::plain(row.observed_damage_text.clone())
            }
        }
        ReplayColumn::ActualDamage => match row.actual_damage_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => {
                let mut cell = CellValue::colored(text.clone(), ColorRole::Player(player_color_kind(row)));
                if let Some(hover) = row.actual_damage_hover_text.as_ref() {
                    cell = cell.with_hover(hover.clone());
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::ReceivedDamage => match row.received_damage_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => {
                let mut cell = CellValue::colored(text.clone(), ColorRole::Player(player_color_kind(row)));
                if let Some(hover) = row.received_damage_hover_text.as_ref() {
                    cell = cell.with_hover(hover.clone());
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::SpottingDamage => match row.spotting_damage_text.as_ref() {
            Some(text) => {
                let mut cell = CellValue::plain(text.clone());
                if let Some(hover) = row.spotting_damage_hover_text.as_ref() {
                    cell = cell.with_hover(hover.clone());
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::PotentialDamage => match row.potential_damage_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => {
                let mut cell = CellValue::plain(text.clone());
                if let Some(hover) = row.potential_damage_hover_text.as_ref() {
                    cell = cell.with_hover(hover.clone());
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::Hits => match row.hits_text.as_ref() {
            Some(_) if nda_active => CellValue::plain(NDA),
            Some(text) => {
                let mut cell = CellValue::colored(text.clone(), ColorRole::Player(player_color_kind(row)));
                if let Some(hover) = row.hits_hover_text.as_ref() {
                    cell = cell.with_hover(hover.clone());
                }
                cell
            }
            None => CellValue::plain(DASH),
        },
        ReplayColumn::Heals => match row.heal_count {
            Some(count) => CellValue::plain(count.to_string()).with_hover(HEALS_TOOLTIP),
            None => CellValue::plain(DASH).with_hover(HEALS_NO_REPAIR_TOOLTIP),
        },
        ReplayColumn::DistanceTraveled => match row.distance_traveled {
            Some(distance) => CellValue::plain(format!("{distance:.2}km")),
            None => CellValue::plain(DASH),
        },
        ReplayColumn::TimeLived => match row.time_lived_text.as_ref() {
            Some(text) => CellValue::plain(text.clone()),
            None => CellValue::plain(DASH),
        },
    }
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::personal_rating::PersonalRatingResult;
    use wows_replays::types::Relation;

    use super::*;
    use crate::replay_inspector::test_support::base_row;

    #[test]
    fn separate_number_is_reexported_and_groups_by_thousands() {
        assert_eq!(separate_number(1_234_567i64), "1,234,567");
    }

    #[test]
    fn default_columns_always_on_only() {
        let settings = ReplaySettings {
            show_raw_xp: false,
            show_observed_damage: false,
            show_heals: false,
            show_received_damage: false,
            show_distance_traveled: false,
            ..ReplaySettings::default()
        };

        let columns = default_columns(&settings);

        assert!(!columns.contains(&ReplayColumn::RawXp));
        assert!(!columns.contains(&ReplayColumn::Heals));
        assert!(!columns.contains(&ReplayColumn::ReceivedDamage));
        assert!(!columns.contains(&ReplayColumn::DistanceTraveled));
        // ObservedDamage is off by ReplaySettings::default() already, and
        // stays off here too.
        assert!(!columns.contains(&ReplayColumn::ObservedDamage));
        // Always-on columns remain, in declaration order.
        assert_eq!(columns.first(), Some(&ReplayColumn::Actions));
        assert!(columns.contains(&ReplayColumn::Name));
        assert!(columns.contains(&ReplayColumn::PersonalRating));
    }

    #[test]
    fn default_columns_includes_optional_columns_when_enabled() {
        let settings = ReplaySettings::default();
        let columns = default_columns(&settings);

        // ReplaySettings::default() enables heals/received/distance but not
        // raw_xp/observed_damage.
        assert!(columns.contains(&ReplayColumn::Heals));
        assert!(columns.contains(&ReplayColumn::ReceivedDamage));
        assert!(columns.contains(&ReplayColumn::DistanceTraveled));
        assert!(!columns.contains(&ReplayColumn::RawXp));
        assert!(!columns.contains(&ReplayColumn::ObservedDamage));

        // Declaration order is preserved (Heals sits before DistanceTraveled).
        let heals_ix = columns.iter().position(|c| *c == ReplayColumn::Heals).unwrap();
        let distance_ix = columns.iter().position(|c| *c == ReplayColumn::DistanceTraveled).unwrap();
        assert!(heals_ix < distance_ix);
    }

    #[test]
    fn cell_value_name_joins_division_clan_and_display_name() {
        let row = PlayerRow {
            display_name: "Steve".to_string(),
            clan_tag: Some("[ABC]".to_string()),
            division_label: Some("(A)".to_string()),
            ..base_row(1, Relation::new(1), false)
        };

        let cell = cell_value(&row, ReplayColumn::Name, false);

        assert_eq!(cell.text, "(A) [ABC] Steve");
        assert_eq!(cell.color, Some(ColorRole::Player(PlayerColorKind::Ally)));
    }

    #[test]
    fn cell_value_name_colors_abuser_pink_even_as_division_mate() {
        let row = PlayerRow {
            display_name: "Rude".to_string(),
            is_abuser: true,
            is_self_division_mate: true,
            ..base_row(1, Relation::new(1), false)
        };

        let cell = cell_value(&row, ReplayColumn::Name, false);

        assert_eq!(cell.color, Some(ColorRole::Player(PlayerColorKind::Abuser)));
    }

    #[test]
    fn cell_value_actual_damage_shows_nda_when_hidden_and_not_debug() {
        let row = PlayerRow {
            is_test_ship: true,
            actual_damage_text: Some("50,000".to_string()),
            actual_damage_hover_text: Some("AP: 50,000".to_string()),
            ..base_row(1, Relation::new(2), false)
        };

        let hidden = cell_value(&row, ReplayColumn::ActualDamage, false);
        assert_eq!(hidden.text, "NDA");
        assert_eq!(hidden.hover, None);

        let debug_visible = cell_value(&row, ReplayColumn::ActualDamage, true);
        assert_eq!(debug_visible.text, "50,000");
        assert_eq!(debug_visible.hover.as_deref(), Some("AP: 50,000"));
    }

    #[test]
    fn cell_value_actual_damage_missing_is_a_dash_regardless_of_nda() {
        let row = base_row(1, Relation::new(1), false);
        let cell = cell_value(&row, ReplayColumn::ActualDamage, false);
        assert_eq!(cell.text, "-");
    }

    #[test]
    fn cell_value_observed_damage_is_uncolored_unlike_actual_damage() {
        let row = PlayerRow { observed_damage_text: "12,345".to_string(), ..base_row(1, Relation::new(1), false) };

        let cell = cell_value(&row, ReplayColumn::ObservedDamage, false);

        assert_eq!(cell.text, "12,345");
        assert_eq!(cell.color, None, "ObservedDamage renders as a bare label in the egui original, never colored");
    }

    #[test]
    fn cell_value_observed_damage_shows_nda_when_hidden_and_not_debug() {
        let row = PlayerRow {
            is_test_ship: true,
            observed_damage_text: "12,345".to_string(),
            ..base_row(1, Relation::new(2), false)
        };

        let hidden = cell_value(&row, ReplayColumn::ObservedDamage, false);
        assert_eq!(hidden.text, "NDA");

        let debug_visible = cell_value(&row, ReplayColumn::ObservedDamage, true);
        assert_eq!(debug_visible.text, "12,345");
    }

    // The following two tests exercise only `cell_value`'s render mapping for
    // an already-`Some` `PersonalRatingResult`; they say nothing about how
    // `personal_rating` gets populated. `from_normalized` always leaves it
    // `None` (see `model.rs`'s `PlayerRow::personal_rating` doc); a real PR
    // value comes from `ReplayReportModel::populate_personal_ratings`.

    #[test]
    fn cell_value_personal_rating_shows_rounded_score_and_tier_color() {
        let row = PlayerRow {
            personal_rating: Some(PersonalRatingResult::new(1600.0)),
            ..base_row(1, Relation::new(0), true)
        };

        let cell = cell_value(&row, ReplayColumn::PersonalRating, false);

        assert_eq!(cell.text, "1600");
        assert_eq!(cell.color, Some(ColorRole::PrTier(PersonalRatingCategory::VeryGood)));
        assert_eq!(cell.hover.as_deref(), Some("Very Good"));
    }

    #[test]
    fn cell_value_personal_rating_missing_is_a_dash() {
        let row = base_row(1, Relation::new(0), true);
        let cell = cell_value(&row, ReplayColumn::PersonalRating, false);
        assert_eq!(cell.text, "-");
        assert_eq!(cell.color, None);
    }
}
