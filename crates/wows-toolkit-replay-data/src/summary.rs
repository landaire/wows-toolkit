//! What the header of an open replay says about it: its title, the PR badge,
//! the match-context line and each side's damage. Mirrors the egui app's
//! Row 1 and Row 2 (`ui/replay_parser/mod.rs`).

use wows_replay_insights::personal_rating::PersonalRatingResult;

use crate::model::MatchContext;
use crate::model::PlayerRow;
use crate::model::ReplayReportModel;

/// Damage dealt by each side.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TeamDamage {
    /// The recording player's own team.
    pub friendly: u64,
    pub enemy: u64,
}

impl TeamDamage {
    pub fn total(self) -> u64 {
        self.friendly + self.enemy
    }
}

/// The recording player's row, when the replay has one.
pub fn self_row(model: &ReplayReportModel) -> Option<&PlayerRow> {
    model.rows.iter().find(|row| row.is_self)
}

/// The tab title: "{self ship} - {map}", or the map alone without a self row.
pub fn title(model: &ReplayReportModel) -> String {
    match self_row(model).map(|row| row.ship_name.as_str()).filter(|ship| !ship.is_empty()) {
        Some(ship) => format!("{ship} - {}", model.map),
        None => model.map.clone(),
    }
}

/// Each side's damage. A row with no server damage figure counts as zero,
/// as the egui app's tally does: a row that reported nothing did not thereby
/// deal damage.
pub fn team_damage(model: &ReplayReportModel) -> TeamDamage {
    let mut damage = TeamDamage::default();
    for row in &model.rows {
        let dealt = row.actual_damage.unwrap_or(0);
        if row.relation.is_enemy() {
            damage.enemy += dealt;
        } else {
            damage.friendly += dealt;
        }
    }

    damage
}

/// The PR badge's text, verbatim the egui `pr_chip` call's format string.
pub fn personal_rating_label(rating: &PersonalRatingResult) -> String {
    format!("PR: {:.0} ({})", rating.pr, rating.category.name())
}

/// The match-context fields in the line's order, empty ones left out: who was
/// recording (the clan tag against the name), the battle type, the version,
/// the mode and the map.
pub fn match_context_fields(context: &MatchContext) -> Vec<String> {
    let who = match context.clan_tag.as_ref() {
        Some(clan) => format!("{clan} {}", context.player_name),
        None => context.player_name.clone(),
    };

    [who, context.game_type.clone(), context.version.clone(), context.game_mode.clone(), context.map.clone()]
        .into_iter()
        .filter(|field| !field.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::personal_rating::PersonalRatingCategory;

    use super::*;

    #[test]
    fn the_badge_label_matches_the_egui_chips_wording() {
        let rating = PersonalRatingResult::new(1150.0);

        assert_eq!(rating.category, PersonalRatingCategory::Average);
        assert_eq!(personal_rating_label(&rating), "PR: 1150 (Average)");
        assert_eq!(personal_rating_label(&PersonalRatingResult::new(2456.4)), "PR: 2456 (Super Unicum)");
    }

    #[test]
    fn the_title_names_the_self_ship_and_the_map() {
        let mut model = crate::sample::sample_model();
        assert_eq!(title(&model), "Shimakaze - Fault Line");

        model.rows.retain(|row| !row.is_self);
        assert_eq!(title(&model), "Fault Line", "without a self row the map stands alone");
    }

    #[test]
    fn each_side_counts_its_own_damage() {
        let model = crate::sample::sample_model();

        let damage = team_damage(&model);

        assert_eq!(damage.friendly, 74_500 + 112_300 + 96_800 + 61_200);
        assert_eq!(damage.enemy, 130_400 + 48_900 + 88_100 + 70_300);
        assert_eq!(damage.total(), damage.friendly + damage.enemy);
    }

    #[test]
    fn empty_context_fields_are_left_out() {
        let context = MatchContext {
            clan_tag: Some("[WTK]".to_owned()),
            player_name: "You".to_owned(),
            game_type: "Random Battle".to_owned(),
            version: String::new(),
            game_mode: "Domination".to_owned(),
            map: "Fault Line".to_owned(),
        };

        assert_eq!(match_context_fields(&context), ["[WTK] You", "Random Battle", "Domination", "Fault Line"]);
    }
}
