//! What a PR or BI was made of, for the hover beside the rating: each part's
//! points, what it was computed from, and what average play would have
//! earned there, largest part first.

use rust_i18n::t;
use wows_replay_insights::battle_impact::BattleImpactComponents;
use wows_replay_insights::battle_impact::BattleImpactResult;
use wows_replay_insights::personal_rating::PersonalRatingTerm;

use crate::formatting::separate_number;
use crate::personal_rating::PersonalRatingCategory;
use crate::personal_rating::PersonalRatingResult;

/// Which rating a breakdown explains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RatingKind {
    Personal,
    BattleImpact,
}

impl RatingKind {
    /// The rating's full name, for the hover's title.
    pub fn title(self) -> String {
        match self {
            Self::Personal => t!("ui.rating.personal_rating").into(),
            Self::BattleImpact => t!("ui.rating.battle_impact").into(),
        }
    }
}

/// One rating split into the parts it adds up from.
#[derive(Clone, Debug, PartialEq)]
pub struct RatingBreakdown {
    pub kind: RatingKind,
    pub total: f64,
    pub category: PersonalRatingCategory,
    /// Largest contribution first.
    pub lines: Vec<RatingLine>,
}

/// One part of a rating.
#[derive(Clone, Debug, PartialEq)]
pub struct RatingLine {
    pub label: String,
    /// What the part was computed from, already formatted.
    pub detail: String,
    pub points: f64,
    /// What average play earns here. `None` for a part where any amount is a
    /// bonus rather than an expectation.
    pub average: Option<f64>,
}

impl RatingLine {
    /// Points above (positive) or below (negative) average play.
    pub fn versus_average(&self) -> Option<f64> {
        self.average.map(|average| self.points - average)
    }
}

impl RatingBreakdown {
    /// The breakdown of a PR that kept its terms. `None` for a rating read back
    /// from storage, which has only the number.
    pub fn personal_rating(result: &PersonalRatingResult, locale: Option<&str>) -> Option<Self> {
        let breakdown = result.breakdown?;
        let mut lines = vec![
            pr_line(t!("ui.rating.damage").into(), &breakdown.damage, locale),
            pr_line(t!("ui.rating.kills").into(), &breakdown.frags, locale),
            RatingLine {
                label: t!("ui.rating.wins").into(),
                detail: t!(
                    "ui.rating.detail.wins",
                    actual = format!("{:.0}", breakdown.wins.actual),
                    expected = format!("{:.2}", breakdown.wins.expected)
                )
                .into(),
                points: breakdown.wins.points,
                average: Some(breakdown.wins.points_at_expected),
            },
        ];
        sort_largest_first(&mut lines);

        Some(Self { kind: RatingKind::Personal, total: result.pr, category: result.category, lines })
    }

    pub fn battle_impact(result: &BattleImpactResult, locale: Option<&str>) -> Self {
        let parts = &result.components;
        let inputs = &result.inputs;
        let average = BattleImpactComponents::AVERAGE;
        let whole = |value: f64| separate_number(value.round() as i64, locale);

        let mut lines = vec![
            RatingLine {
                label: t!("ui.rating.damage").into(),
                detail: t!(
                    "ui.rating.detail.weighted_damage",
                    actual = whole(inputs.weighted_damage),
                    expected = whole(inputs.expected_damage)
                )
                .into(),
                points: parts.damage,
                average: Some(average.damage),
            },
            RatingLine {
                label: t!("ui.rating.kills").into(),
                detail: t!(
                    "ui.rating.detail.versus_expected",
                    actual = format!("{:.0}", inputs.kills),
                    expected = format!("{:.2}", inputs.expected_kills)
                )
                .into(),
                points: parts.kills,
                average: Some(average.kills),
            },
            RatingLine {
                label: t!("ui.rating.enemy_hp_share").into(),
                detail: t!(
                    "ui.rating.detail.enemy_hp_share",
                    share = format!("{:.1}", inputs.enemy_hp_share * 100.0),
                    even = format!("{:.1}", inputs.even_share * 100.0)
                )
                .into(),
                points: parts.enemy_hp_share,
                average: Some(average.enemy_hp_share),
            },
            RatingLine {
                label: t!("ui.rating.tanking").into(),
                detail: t!(
                    "ui.rating.detail.tanking",
                    potential = separate_number(inputs.potential_damage, locale),
                    ratio = format!("{:.2}", inputs.tanking_ratio)
                )
                .into(),
                points: parts.tanking,
                average: Some(average.tanking),
            },
        ];

        if let (Some(points), Some(spotting_damage)) = (parts.spotting, inputs.spotting_damage) {
            lines.push(RatingLine {
                label: t!("ui.rating.spotting").into(),
                detail: t!(
                    "ui.rating.detail.spotting",
                    damage = separate_number(spotting_damage, locale),
                    baseline = whole(inputs.spotting_baseline),
                    ships = format!("{:.2}", inputs.ships_spotted),
                    ships_baseline = format!("{:.2}", inputs.ships_spotted_baseline)
                )
                .into(),
                points,
                average: average.spotting,
            });
        }

        if let (Some(points), Some(objective)) = (parts.objectives, inputs.objective_points) {
            // The server's victory points run at 100 to a score point.
            let score = |victory_points: f64| format!("{:.0}", victory_points / 100.0);
            lines.push(RatingLine {
                label: t!("ui.rating.objectives").into(),
                detail: t!(
                    "ui.rating.detail.objectives",
                    capture = score(objective.capture),
                    block = score(objective.block),
                    reset = score(objective.drop),
                    battle_end = score(objective.battle_end),
                    hold = score(objective.hold)
                )
                .into(),
                points,
                average: None,
            });
        }

        if let Some(points) = parts.anti_air {
            // A build may report only one of the two; list what it reported.
            let planes = inputs.planes_killed.map(|planes| {
                let strike = planes.rocket_planes + planes.bombers + planes.torpedo_bombers + planes.skip_bombers;
                let support = planes.consumable_fighters + planes.scouts;
                t!("ui.rating.detail.anti_air_planes", strike = strike, support = support).into_owned()
            });
            let damage = inputs.anti_air_damage.map(|damage| {
                t!("ui.rating.detail.anti_air_damage", damage = separate_number(damage, locale)).into_owned()
            });
            lines.push(RatingLine {
                label: t!("ui.rating.anti_air").into(),
                detail: [planes, damage].into_iter().flatten().collect::<Vec<_>>().join(", "),
                points,
                average: None,
            });
        }

        if let (Some(points), Some(detection)) = (parts.torpedo_warnings, result.torpedo_detection) {
            lines.push(RatingLine {
                label: t!("ui.rating.torpedo_warnings").into(),
                detail: t!(
                    "ui.rating.detail.torpedo_warnings",
                    threatening = format!("{:.1}", detection.threatening),
                    detected = detection.detected
                )
                .into(),
                points,
                average: None,
            });
        }

        // A bonus with nothing earned says nothing; a core part at zero still
        // shows how far below average it fell.
        lines.retain(|line| line.average.is_some() || line.points > 0.0);
        sort_largest_first(&mut lines);

        Self { kind: RatingKind::BattleImpact, total: result.bi, category: result.category, lines }
    }

    /// The widest a bar needs to reach: the largest of every line's points
    /// and average, so bars and average markers share one scale.
    pub fn scale(&self) -> f64 {
        self.lines.iter().flat_map(|line| [Some(line.points), line.average]).flatten().fold(0.0, f64::max)
    }

    /// The breakdown as plain text, for a tooltip that cannot draw bars.
    pub fn text(&self) -> String {
        let label_width = self.lines.iter().map(|line| line.label.chars().count()).max().unwrap_or_default();
        let mut text = format!("{} {:.0}", self.kind.title(), self.total);
        for line in &self.lines {
            let versus = match line.versus_average() {
                Some(delta) => format!(" ({delta:+.0} {})", t!("ui.rating.versus_average")),
                None => String::new(),
            };
            text.push_str(&format!(
                "\n{label:<label_width$}  {points:>5.0}{versus}  {detail}",
                label = line.label,
                points = line.points,
                detail = line.detail
            ));
        }

        text
    }
}

fn pr_line(label: String, term: &PersonalRatingTerm, locale: Option<&str>) -> RatingLine {
    let detail = if term.expected >= 100.0 {
        t!(
            "ui.rating.detail.versus_expected",
            actual = separate_number(term.actual.round() as i64, locale),
            expected = separate_number(term.expected.round() as i64, locale)
        )
    } else {
        t!(
            "ui.rating.detail.versus_expected",
            actual = format!("{:.0}", term.actual),
            expected = format!("{:.2}", term.expected)
        )
    };

    RatingLine { label, detail: detail.into(), points: term.points, average: Some(term.points_at_expected) }
}

fn sort_largest_first(lines: &mut [RatingLine]) {
    lines.sort_by(|a, b| b.points.total_cmp(&a.points));
}

#[cfg(test)]
mod tests {
    use wows_replay_insights::battle_impact::BattleImpactInputs;
    use wows_replay_insights::battle_impact::BattleImpactVersion;
    use wows_replay_insights::battle_impact::TorpedoDetection;

    use super::*;

    fn rating(components: BattleImpactComponents, torpedo_detection: Option<TorpedoDetection>) -> BattleImpactResult {
        let inputs = BattleImpactInputs {
            weighted_damage: 90_000.0,
            expected_damage: 60_000.0,
            kills: 1.0,
            expected_kills: 0.8,
            enemy_hp_share: 0.1,
            even_share: 1.0 / 12.0,
            potential_damage: 500_000,
            tanking_ratio: 1.0,
            spotting_damage: Some(40_000),
            spotting_baseline: 35_000.0,
            ships_spotted: 0.6,
            ships_spotted_baseline: 0.54,
            objective_points: None,
            planes_killed: None,
            anti_air_damage: None,
        };
        let total = components.damage
            + components.kills
            + components.enemy_hp_share
            + components.tanking
            + [components.spotting, components.objectives, components.anti_air, components.torpedo_warnings]
                .into_iter()
                .flatten()
                .sum::<f64>();

        BattleImpactResult {
            bi: total,
            category: PersonalRatingCategory::from_pr(total),
            version: BattleImpactVersion::CURRENT,
            components,
            inputs,
            torpedo_detection,
        }
    }

    /// The hover reads top to bottom as what mattered most, and the lines
    /// account for the whole rating.
    #[test]
    fn lines_run_largest_first_and_sum_to_the_rating() {
        let components = BattleImpactComponents {
            damage: 600.0,
            kills: 150.0,
            enemy_hp_share: 240.0,
            tanking: 90.0,
            spotting: Some(260.0),
            objectives: None,
            anti_air: None,
            torpedo_warnings: Some(40.0),
        };
        let result = rating(components, Some(TorpedoDetection { detected: 7, threatening: 5.0 }));

        let breakdown = RatingBreakdown::battle_impact(&result, None);

        let points: Vec<f64> = breakdown.lines.iter().map(|line| line.points).collect();
        assert_eq!(points, vec![600.0, 260.0, 240.0, 150.0, 90.0, 40.0]);
        assert!((points.iter().sum::<f64>() - result.bi).abs() < 1e-9);
        assert_eq!(breakdown.scale(), 600.0);
    }

    /// A part the replay could not observe, or a bonus nothing was earned
    /// for, is left out; a core part at zero stays, since it shows a deficit.
    #[test]
    fn unobservable_parts_and_empty_bonuses_are_not_listed() {
        let components = BattleImpactComponents {
            damage: 300.0,
            kills: 0.0,
            enemy_hp_share: 100.0,
            tanking: 50.0,
            spotting: None,
            objectives: None,
            anti_air: Some(0.0),
            torpedo_warnings: None,
        };

        let breakdown = RatingBreakdown::battle_impact(&rating(components, None), None);

        assert_eq!(breakdown.lines.len(), 4, "only the four core parts");
        assert!(breakdown.lines.iter().all(|line| line.average.is_some()), "every core part has an average");
        let damage = breakdown.lines.iter().find(|line| line.points == 300.0).expect("damage is listed");
        assert_eq!(damage.versus_average(), Some(300.0 - BattleImpactComponents::AVERAGE.damage));
    }
}
