//! What a PR or BI was made of, for the hover beside the rating: each part's
//! points, what it was computed from, what average play would have earned
//! there, and whether it was measured against a fixed expectation or this
//! battle's data, largest part first.

use rust_i18n::t;
use wows_replay_insights::battle_impact::BattleImpactFactor;
use wows_replay_insights::battle_impact::BattleImpactResult;
use wows_replay_insights::battle_impact::FactorBasis;
use wows_replay_insights::battle_impact::FactorScore;
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
    pub basis: FactorBasis,
}

impl RatingLine {
    /// Points above (positive) or below (negative) average play.
    pub fn versus_average(&self) -> Option<f64> {
        self.average.map(|average| self.points - average)
    }
}

/// The short label shown beside a part.
pub fn basis_badge(basis: FactorBasis) -> String {
    match basis {
        FactorBasis::Static => t!("ui.rating.basis.static").into(),
        FactorBasis::Dynamic => t!("ui.rating.basis.dynamic").into(),
    }
}

/// One line saying what a badge means.
pub fn basis_explanation(basis: FactorBasis) -> String {
    match basis {
        FactorBasis::Static => t!("ui.rating.basis.static_explained").into(),
        FactorBasis::Dynamic => t!("ui.rating.basis.dynamic_explained").into(),
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
                basis: FactorBasis::Dynamic,
            },
        ];
        sort_largest_first(&mut lines);

        Some(Self { kind: RatingKind::Personal, total: result.pr, category: result.category, lines })
    }

    pub fn battle_impact(result: &BattleImpactResult, locale: Option<&str>) -> Self {
        let parts = &result.components;
        let inputs = &result.inputs;
        let whole = |value: f64| separate_number(value.round() as i64, locale);

        let mut lines = vec![
            bi_line(
                BattleImpactFactor::Damage,
                parts.damage,
                t!(
                    "ui.rating.detail.weighted_damage",
                    actual = whole(inputs.weighted_damage),
                    expected = whole(inputs.expected_damage)
                )
                .into(),
            ),
            bi_line(
                BattleImpactFactor::Kills,
                parts.kills,
                t!(
                    "ui.rating.detail.versus_expected",
                    actual = format!("{:.0}", inputs.kills),
                    expected = format!("{:.2}", inputs.expected_kills)
                )
                .into(),
            ),
        ];

        if let (Some(score), Some(ratios)) = (parts.tanking, inputs.tanking) {
            lines.push(bi_line(
                BattleImpactFactor::Tanking,
                score,
                t!(
                    "ui.rating.detail.tanking",
                    potential = separate_number(inputs.potential_damage, locale),
                    class = format!("{:.2}", ratios.class.0),
                    team = format!("{:.2}", ratios.team.0)
                )
                .into(),
            ));
        }

        if let (Some(score), Some(share)) = (parts.team_damage_share, inputs.team_damage_share) {
            lines.push(bi_line(
                BattleImpactFactor::TeamDamageShare,
                score,
                t!(
                    "ui.rating.detail.team_damage_share",
                    share = format!("{:.1}", share.share.0 * 100.0),
                    even = format!("{:.1}", share.even.0 * 100.0)
                )
                .into(),
            ));
        }

        if let (Some(score), Some(spotting_damage)) = (parts.spotting, inputs.spotting_damage) {
            lines.push(bi_line(
                BattleImpactFactor::Spotting,
                score,
                t!(
                    "ui.rating.detail.spotting",
                    damage = separate_number(spotting_damage, locale),
                    baseline = whole(inputs.spotting_baseline),
                    ships = format!("{:.2}", inputs.ships_spotted),
                    ships_baseline = format!("{:.2}", inputs.ships_spotted_baseline)
                )
                .into(),
            ));
        }

        if let (Some(score), Some(objective)) = (parts.objectives, inputs.objective_points) {
            // The server's victory points run at 100 to a score point.
            let points = |victory_points: f64| format!("{:.0}", victory_points / 100.0);
            lines.push(bi_line(
                BattleImpactFactor::Objectives,
                score,
                t!(
                    "ui.rating.detail.objectives",
                    capture = points(objective.capture),
                    block = points(objective.block),
                    reset = points(objective.drop),
                    battle_end = points(objective.battle_end),
                    hold = points(objective.hold)
                )
                .into(),
            ));
        }

        if let Some(score) = parts.anti_air {
            // A build may report only one of the two; list what it reported.
            let planes = inputs.planes_killed.map(|planes| {
                let strike = planes.rocket_planes + planes.bombers + planes.torpedo_bombers + planes.skip_bombers;
                let support = planes.consumable_fighters + planes.scouts;
                t!("ui.rating.detail.anti_air_planes", strike = strike, support = support).into_owned()
            });
            let damage = inputs.anti_air_damage.map(|damage| {
                t!("ui.rating.detail.anti_air_damage", damage = separate_number(damage, locale)).into_owned()
            });
            lines.push(bi_line(
                BattleImpactFactor::AntiAir,
                score,
                [planes, damage].into_iter().flatten().collect::<Vec<_>>().join(", "),
            ));
        }

        if let (Some(score), Some(detection)) = (parts.torpedo_warnings, result.torpedo_detection) {
            let detail = match detection.threatening {
                1 => t!("ui.rating.detail.torpedo_warnings_one"),
                count => t!("ui.rating.detail.torpedo_warnings", threatening = count),
            };
            lines.push(bi_line(BattleImpactFactor::TorpedoWarnings, score, detail.into()));
        }

        if let (Some(score), Some(bursts)) = (parts.critical_bursts, &result.critical_bursts) {
            let listed = bursts
                .iter()
                .map(|burst| {
                    let ship = burst.victim_ship_name.clone().unwrap_or_else(|| burst.victim_ship.to_string());
                    let share = format!("{:.0}", burst.share.0 * 100.0);
                    // A single salvo or torpedo spread lands within a second.
                    if burst.duration.value() < 1.0 {
                        t!("ui.rating.detail.critical_burst_at_once", share = share, ship = ship).into_owned()
                    } else {
                        let seconds = format!("{:.0}", burst.duration.value());
                        t!("ui.rating.detail.critical_burst", share = share, ship = ship, seconds = seconds)
                            .into_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            let detail = match bursts.len() {
                1 => t!("ui.rating.detail.critical_bursts_one", bursts = listed),
                count => t!("ui.rating.detail.critical_bursts", count = count, bursts = listed),
            };
            lines.push(bi_line(BattleImpactFactor::CriticalBursts, score, detail.into()));
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

    /// The bases the lines were measured against, each once, for the legend
    /// under the lines.
    pub fn bases(&self) -> Vec<FactorBasis> {
        [FactorBasis::Dynamic, FactorBasis::Static]
            .into_iter()
            .filter(|basis| self.lines.iter().any(|line| line.basis == *basis))
            .collect()
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
                "\n{label:<label_width$}  {points:>5.0}{versus}  [{badge}]  {detail}",
                label = line.label,
                points = line.points,
                badge = basis_badge(line.basis),
                detail = line.detail
            ));
        }
        for basis in self.bases() {
            text.push('\n');
            text.push_str(&basis_explanation(basis));
        }

        text
    }
}

fn bi_label(factor: BattleImpactFactor) -> String {
    match factor {
        BattleImpactFactor::Damage => t!("ui.rating.damage").into(),
        BattleImpactFactor::Kills => t!("ui.rating.kills").into(),
        BattleImpactFactor::TeamDamageShare => t!("ui.rating.team_damage_share").into(),
        BattleImpactFactor::Tanking => t!("ui.rating.tanking").into(),
        BattleImpactFactor::Spotting => t!("ui.rating.spotting").into(),
        BattleImpactFactor::Objectives => t!("ui.rating.objectives").into(),
        BattleImpactFactor::AntiAir => t!("ui.rating.anti_air").into(),
        BattleImpactFactor::TorpedoWarnings => t!("ui.rating.torpedo_warnings").into(),
        BattleImpactFactor::CriticalBursts => t!("ui.rating.critical_bursts").into(),
    }
}

fn bi_line(factor: BattleImpactFactor, score: FactorScore, detail: String) -> RatingLine {
    RatingLine {
        label: bi_label(factor),
        detail,
        points: score.points,
        average: factor.average_points(),
        basis: score.basis,
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

    // wows-numbers expected values for the ship.
    RatingLine {
        label,
        detail: detail.into(),
        points: term.points,
        average: Some(term.points_at_expected),
        basis: FactorBasis::Dynamic,
    }
}

fn sort_largest_first(lines: &mut [RatingLine]) {
    lines.sort_by(|a, b| b.points.total_cmp(&a.points));
}

#[cfg(test)]
mod tests {
    use wows_core::units::Seconds;
    use wows_replay_insights::battle_impact::BattleImpactComponents;
    use wows_replay_insights::battle_impact::BattleImpactInputs;
    use wows_replay_insights::battle_impact::BattleImpactVersion;
    use wows_replay_insights::battle_impact::CriticalBurst;
    use wows_replay_insights::battle_impact::Ratio;
    use wows_replay_insights::battle_impact::Share;
    use wows_replay_insights::battle_impact::TankingRatios;
    use wows_replay_insights::battle_impact::TeamDamageShare;
    use wows_replay_insights::battle_impact::TorpedoDetection;
    use wows_replays::types::GameParamId;

    use super::*;

    fn score(factor: BattleImpactFactor, points: f64) -> FactorScore {
        FactorScore { points, basis: factor.basis() }
    }

    fn rating(components: BattleImpactComponents, torpedo_detection: Option<TorpedoDetection>) -> BattleImpactResult {
        let inputs = BattleImpactInputs {
            weighted_damage: 90_000.0,
            expected_damage: 60_000.0,
            kills: 1.0,
            expected_kills: 0.8,
            team_damage_share: Some(TeamDamageShare { share: Share(0.1), even: Share(1.0 / 12.0) }),
            potential_damage: 500_000,
            tanking: Some(TankingRatios { class: Ratio(1.0), class_peers: 5, team: Ratio(1.0) }),
            spotting_damage: Some(40_000),
            spotting_baseline: 35_000.0,
            ships_spotted: 0.6,
            ships_spotted_baseline: 0.54,
            objective_points: None,
            planes_killed: None,
            anti_air_damage: None,
        };
        let total = components.total();

        BattleImpactResult {
            bi: total,
            category: PersonalRatingCategory::from_pr(total),
            version: BattleImpactVersion::CURRENT,
            components,
            inputs,
            torpedo_detection,
            critical_bursts: None,
        }
    }

    /// The hover reads top to bottom as what mattered most, and the lines
    /// account for the whole rating.
    #[test]
    fn lines_run_largest_first_and_sum_to_the_rating() {
        let components = BattleImpactComponents {
            damage: score(BattleImpactFactor::Damage, 600.0),
            kills: score(BattleImpactFactor::Kills, 150.0),
            team_damage_share: Some(score(BattleImpactFactor::TeamDamageShare, 240.0)),
            tanking: Some(score(BattleImpactFactor::Tanking, 90.0)),
            spotting: Some(score(BattleImpactFactor::Spotting, 260.0)),
            objectives: None,
            anti_air: None,
            torpedo_warnings: Some(score(BattleImpactFactor::TorpedoWarnings, 40.0)),
            critical_bursts: None,
        };
        let result = rating(components, Some(TorpedoDetection { detected: 22, threatening: 9, threat_weight: 5.0 }));

        let breakdown = RatingBreakdown::battle_impact(&result, None);

        let points: Vec<f64> = breakdown.lines.iter().map(|line| line.points).collect();
        assert_eq!(points, vec![600.0, 260.0, 240.0, 150.0, 90.0, 40.0]);
        assert!((points.iter().sum::<f64>() - result.bi).abs() < 1e-9);
        assert_eq!(breakdown.scale(), 600.0);

        // A whole count of threatening torpedoes and nothing about the rest.
        let torpedoes = &breakdown.lines.last().expect("the torpedo line is the smallest").detail;
        assert!(torpedoes.contains('9') && !torpedoes.contains('.'), "{torpedoes}");
        assert!(!torpedoes.contains("22"), "{torpedoes}");
    }

    /// A part the replay could not observe, or a bonus nothing was earned
    /// for, is left out; a core part at zero stays, since it shows a deficit.
    #[test]
    fn unobservable_parts_and_empty_bonuses_are_not_listed() {
        let components = BattleImpactComponents {
            damage: score(BattleImpactFactor::Damage, 300.0),
            kills: score(BattleImpactFactor::Kills, 0.0),
            team_damage_share: Some(score(BattleImpactFactor::TeamDamageShare, 100.0)),
            tanking: Some(score(BattleImpactFactor::Tanking, 50.0)),
            spotting: None,
            objectives: None,
            anti_air: Some(score(BattleImpactFactor::AntiAir, 0.0)),
            torpedo_warnings: None,
            critical_bursts: None,
        };

        let breakdown = RatingBreakdown::battle_impact(&rating(components, None), None);

        assert_eq!(breakdown.lines.len(), 4, "only the four core parts");
        assert!(breakdown.lines.iter().all(|line| line.average.is_some()), "every core part has an average");
        let damage = breakdown.lines.iter().find(|line| line.points == 300.0).expect("damage is listed");
        assert_eq!(damage.versus_average(), BattleImpactFactor::Damage.average_points().map(|average| 300.0 - average));
    }

    /// Each line shows the basis the rating recorded for that player, not
    /// one looked up from the factor, and the legend explains each basis
    /// shown once.
    #[test]
    fn lines_carry_the_recorded_basis_and_the_legend_lists_each_once() {
        let components = BattleImpactComponents {
            damage: score(BattleImpactFactor::Damage, 300.0),
            kills: score(BattleImpactFactor::Kills, 100.0),
            team_damage_share: Some(score(BattleImpactFactor::TeamDamageShare, 100.0)),
            // Recorded as static although the factor is dynamic when complete.
            tanking: Some(FactorScore { points: 50.0, basis: FactorBasis::Static }),
            spotting: Some(score(BattleImpactFactor::Spotting, 200.0)),
            objectives: None,
            anti_air: None,
            torpedo_warnings: None,
            critical_bursts: None,
        };

        let breakdown = RatingBreakdown::battle_impact(&rating(components, None), None);

        let bases: Vec<(f64, FactorBasis)> = breakdown.lines.iter().map(|line| (line.points, line.basis)).collect();
        assert_eq!(
            bases,
            vec![
                (300.0, FactorBasis::Dynamic),
                (200.0, FactorBasis::Static),
                (100.0, FactorBasis::Dynamic),
                (100.0, FactorBasis::Dynamic),
                (50.0, FactorBasis::Static),
            ]
        );
        assert_eq!(breakdown.bases(), vec![FactorBasis::Dynamic, FactorBasis::Static]);
    }

    /// A team that dealt no damage leaves its players without a share of it,
    /// so the hover lists no line for it rather than a zero.
    #[test]
    fn a_team_without_damage_lists_no_share_of_it() {
        let components = BattleImpactComponents {
            damage: score(BattleImpactFactor::Damage, 0.0),
            kills: score(BattleImpactFactor::Kills, 0.0),
            team_damage_share: None,
            tanking: Some(score(BattleImpactFactor::Tanking, 50.0)),
            spotting: None,
            objectives: None,
            anti_air: None,
            torpedo_warnings: None,
            critical_bursts: None,
        };
        let mut result = rating(components, None);
        result.inputs.team_damage_share = None;

        let breakdown = RatingBreakdown::battle_impact(&result, None);

        let share_label = bi_label(BattleImpactFactor::TeamDamageShare);
        assert!(breakdown.lines.iter().all(|line| line.label != share_label));
        assert_eq!(breakdown.lines.len(), 3, "damage, kills and tanking");
    }

    /// Each burst names its victim, the share and the time it took, a burst
    /// within a second reads as at once, and the line says what the replay
    /// cannot see.
    #[test]
    fn critical_bursts_list_each_victim() {
        let components = BattleImpactComponents {
            damage: score(BattleImpactFactor::Damage, 300.0),
            kills: score(BattleImpactFactor::Kills, 100.0),
            team_damage_share: None,
            tanking: None,
            spotting: None,
            objectives: None,
            anti_air: None,
            torpedo_warnings: None,
            critical_bursts: Some(score(BattleImpactFactor::CriticalBursts, 50.0)),
        };
        let mut result = rating(components, None);
        let burst = |ship: &str, share: f64, seconds: f32| CriticalBurst {
            victim_ship: GameParamId::from(1u64),
            victim_ship_name: Some(ship.to_owned()),
            share: Share(share),
            duration: Seconds::new(seconds),
        };
        result.critical_bursts = Some(vec![burst("Montana", 0.92, 8.0), burst("Zao", 1.0, 0.4)]);

        let breakdown = RatingBreakdown::battle_impact(&result, None);

        let line = breakdown.lines.iter().find(|line| line.points == 50.0).expect("the bursts are listed");
        assert_eq!(line.basis, FactorBasis::Static);
        assert_eq!(
            line.detail,
            "2 critical bursts: 92% of Montana in 8 s, 100% of Zao at once. Seen only for ships near the recording \
             player"
        );
    }
}
