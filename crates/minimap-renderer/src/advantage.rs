/// Team advantage calculation for the minimap renderer.
///
/// Evaluates which team has a stronger position based on score trajectory,
/// fleet power (class-weighted HP and ship counts), and strategic threats
/// (destroyer/submarine survival, class diversity).
///
/// All scoring uses non-negative point tuples `(team0_pts, team1_pts)`.
/// Points are awarded to whichever team has the advantage in each factor.
/// The team with the higher total wins. See TEAM_ADVANTAGE_SCORING.md for
/// full documentation of the scoring model.
///
/// Per-class ship count and HP snapshot for one team.
#[derive(Debug, Clone, Default)]
pub struct ClassCount {
    /// Number of ships of this class alive
    pub alive: usize,
    /// Total number of ships of this class on this team
    pub total: usize,
    /// Sum of current HP for alive ships of this class
    pub hp: f32,
    /// Sum of max HP for alive ships of this class
    pub max_hp: f32,
}

/// Per-team snapshot of game state for a single frame.
#[derive(Debug, Clone)]
pub struct TeamState {
    pub score: i64,
    /// Number of uncontested caps owned by this team
    pub uncontested_caps: usize,
    pub total_hp: f32,
    pub max_hp: f32,
    pub ships_alive: usize,
    /// Total number of players on this team (from arena state)
    pub ships_total: usize,
    /// Number of ships with known entity data (EntityCreate received)
    pub ships_known: usize,
    // Per-class breakdown
    pub destroyers: ClassCount,
    pub cruisers: ClassCount,
    pub battleships: ClassCount,
    pub submarines: ClassCount,
    pub carriers: ClassCount,
}

impl Default for TeamState {
    fn default() -> Self {
        TeamState {
            score: 0,
            uncontested_caps: 0,
            total_hp: 0.0,
            max_hp: 0.0,
            ships_alive: 0,
            ships_total: 0,
            ships_known: 0,
            destroyers: ClassCount::default(),
            cruisers: ClassCount::default(),
            battleships: ClassCount::default(),
            submarines: ClassCount::default(),
            carriers: ClassCount::default(),
        }
    }
}

impl TeamState {
    /// Create a TeamState with all zeroes and empty class counts.
    pub fn new() -> Self {
        Self::default()
    }
}

// Re-export from wowsunpack so existing `crate::advantage::AdvantageLevel` paths keep working.
pub use wowsunpack::game_types::AdvantageLevel;

/// Which team has the advantage, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamAdvantage {
    /// Team 0 has the advantage at the given level
    Team0(AdvantageLevel),
    /// Team 1 has the advantage at the given level
    Team1(AdvantageLevel),
    /// No clear advantage
    Even,
}

impl TeamAdvantage {
    fn for_team(team: usize, level: AdvantageLevel) -> Self {
        if team == 0 { TeamAdvantage::Team0(level) } else { TeamAdvantage::Team1(level) }
    }
}

/// Scoring rules from the replay's BattleLogic.
#[derive(Debug, Clone)]
pub struct ScoringParams {
    pub team_win_score: i64,
    pub hold_reward: i64,
    pub hold_period: f32,
}

/// Breakdown of individual factors contributing to the advantage verdict.
///
/// All point values are non-negative tuples `(team0_pts, team1_pts)`.
/// Points are awarded to whichever team has the advantage in each factor area.
///
/// After team perspective normalization (swap), team0 = friendly, team1 = enemy.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct AdvantageBreakdown {
    /// Points from score trajectory: current gap, cap income, time-to-win projection.
    /// Max 10 points to the winning team.
    pub score_projection: (f32, f32),
    /// Points from fleet power: class-weighted HP and ship count advantage.
    /// Max 10 points split proportionally. Only populated when HP data is reliable.
    pub fleet_power: (f32, f32),
    /// Points from strategic threats: DD/SS survival, class diversity, CV advantage.
    /// Max 5 points to the team with more strategic resilience. Only populated when
    /// HP data is reliable.
    pub strategic_threat: (f32, f32),
    /// Sum of all factor points per team.
    pub total: (f32, f32),
    /// Whether HP/ship data was complete enough to factor in fleet power and threats.
    pub hp_data_reliable: bool,
    /// Special case: a team was fully eliminated.
    pub team_eliminated: bool,

    // Raw values for display
    /// Points per second from caps for team 0 (friendly after swap)
    pub team0_pps: f64,
    /// Points per second from caps for team 1 (enemy after swap)
    pub team1_pps: f64,
}

/// Result of advantage calculation: the verdict plus the breakdown of why.
#[derive(Debug, Clone)]
pub struct AdvantageResult {
    pub advantage: TeamAdvantage,
    pub breakdown: AdvantageBreakdown,
}

impl AdvantageResult {
    fn even() -> Self {
        AdvantageResult { advantage: TeamAdvantage::Even, breakdown: AdvantageBreakdown::default() }
    }
}

/// Swap all per-team tuple fields so that team0 = friendly, team1 = enemy.
///
/// Called when the replay owner is on team 1. See TEAM_ADVANTAGE_SCORING.md
/// "Team Perspective Normalization" for details.
pub fn swap_breakdown(bd: &mut AdvantageBreakdown) {
    swap_tuple(&mut bd.score_projection);
    swap_tuple(&mut bd.fleet_power);
    swap_tuple(&mut bd.strategic_threat);
    swap_tuple(&mut bd.total);
    std::mem::swap(&mut bd.team0_pps, &mut bd.team1_pps);
}

fn swap_tuple(t: &mut (f32, f32)) {
    std::mem::swap(&mut t.0, &mut t.1);
}

// --- Class weights for fleet power calculation ---
// See TEAM_ADVANTAGE_SCORING.md for rationale.

const WEIGHT_DESTROYER: f32 = 1.5;
const WEIGHT_CRUISER: f32 = 1.0;
const WEIGHT_BATTLESHIP: f32 = 1.0;
const WEIGHT_SUBMARINE: f32 = 1.3;
const WEIGHT_CARRIER: f32 = 1.2;

/// Maximum points for each factor category.
const MAX_SCORE_PROJECTION: f32 = 10.0;
const MAX_FLEET_POWER: f32 = 10.0;
const MAX_STRATEGIC_THREAT: f32 = 5.0;

/// Calculate class-weighted fleet power for one team.
/// Returns the sum of (class_weight * alive_count * hp_fraction) across all classes.
fn fleet_power(team: &TeamState) -> f32 {
    let class_power = |cc: &ClassCount, weight: f32| -> f32 {
        if cc.alive == 0 || cc.max_hp <= 0.0 {
            return 0.0;
        }
        let hp_fraction = cc.hp / cc.max_hp;
        weight * cc.alive as f32 * hp_fraction
    };

    class_power(&team.destroyers, WEIGHT_DESTROYER)
        + class_power(&team.cruisers, WEIGHT_CRUISER)
        + class_power(&team.battleships, WEIGHT_BATTLESHIP)
        + class_power(&team.submarines, WEIGHT_SUBMARINE)
        + class_power(&team.carriers, WEIGHT_CARRIER)
}

/// Calculate which team has the advantage.
///
/// Contested capture points (has_invaders == true) are excluded from both
/// teams' uncontested_caps counts before calling this function.
pub fn calculate_advantage(
    team0: &TeamState,
    team1: &TeamState,
    scoring: &ScoringParams,
    time_left: Option<i64>,
) -> AdvantageResult {
    // Not enough data yet (e.g. match start before enemy entities are created).
    if team0.ships_total == 0 || team1.ships_total == 0 {
        return AdvantageResult::even();
    }

    let hp_data_reliable = team0.ships_known == team0.ships_total && team1.ships_known == team1.ships_total;

    // --- Special case: team eliminated ---
    if hp_data_reliable {
        if team0.ships_alive == 0 && team1.ships_alive > 0 {
            return AdvantageResult {
                advantage: TeamAdvantage::Team1(AdvantageLevel::Absolute),
                breakdown: AdvantageBreakdown {
                    team_eliminated: true,
                    hp_data_reliable: true,
                    total: (0.0, MAX_SCORE_PROJECTION + MAX_FLEET_POWER + MAX_STRATEGIC_THREAT),
                    ..Default::default()
                },
            };
        }
        if team1.ships_alive == 0 && team0.ships_alive > 0 {
            return AdvantageResult {
                advantage: TeamAdvantage::Team0(AdvantageLevel::Absolute),
                breakdown: AdvantageBreakdown {
                    team_eliminated: true,
                    hp_data_reliable: true,
                    total: (MAX_SCORE_PROJECTION + MAX_FLEET_POWER + MAX_STRATEGIC_THREAT, 0.0),
                    ..Default::default()
                },
            };
        }
        if team0.ships_alive == 0 && team1.ships_alive == 0 {
            return AdvantageResult::even();
        }
    }

    let mut bd = AdvantageBreakdown { hp_data_reliable, ..Default::default() };

    // ═══════════════════════════════════════════════════════════════════
    // Factor 1: Score Projection (max 10 points to the winning team)
    // Combines current score gap, cap income, and time-to-win.
    // ═══════════════════════════════════════════════════════════════════

    let pps0 = if scoring.hold_period > 0.0 {
        team0.uncontested_caps as f64 * scoring.hold_reward as f64 / scoring.hold_period as f64
    } else {
        0.0
    };
    let pps1 = if scoring.hold_period > 0.0 {
        team1.uncontested_caps as f64 * scoring.hold_reward as f64 / scoring.hold_period as f64
    } else {
        0.0
    };
    bd.team0_pps = pps0;
    bd.team1_pps = pps1;

    let seconds_left = time_left.unwrap_or(0).max(0) as f64;
    let win = scoring.team_win_score as f64;

    // Project final scores (capped at win score)
    let proj0 = (team0.score as f64 + pps0 * seconds_left).min(win);
    let proj1 = (team1.score as f64 + pps1 * seconds_left).min(win);

    // Time-to-win for each team (None = can't reach win score from cap income)
    let ttw = |score: i64, pps: f64| -> Option<f64> {
        let remaining = win - score as f64;
        if remaining <= 0.0 {
            Some(0.0)
        } else if pps > 0.0 {
            Some(remaining / pps)
        } else {
            None
        }
    };
    let ttw0 = ttw(team0.score, pps0);
    let ttw1 = ttw(team1.score, pps1);

    let mut score_pts: (f32, f32) = (0.0, 0.0);

    // Sub-factor: current score gap (up to 4 pts)
    let score_gap = (team0.score - team1.score).abs() as f64;
    if score_gap > 0.0 {
        let gap_pts = (score_gap / win * 4.0).min(4.0) as f32;
        if team0.score > team1.score {
            score_pts.0 += gap_pts;
        } else {
            score_pts.1 += gap_pts;
        }
    }

    // Sub-factor: time-to-win (up to 3 pts)
    match (ttw0, ttw1) {
        (Some(t0), Some(t1)) if t0 < seconds_left && t1 < seconds_left => {
            // Both can reach win score — advantage to whoever gets there first
            let time_diff = (t1 - t0).abs();
            let ttw_pts = if time_diff > 30.0 {
                3.0
            } else if time_diff > 10.0 {
                2.0
            } else if time_diff > 3.0 {
                1.0
            } else {
                0.0
            };
            if t0 < t1 {
                score_pts.0 += ttw_pts;
            } else {
                score_pts.1 += ttw_pts;
            }
        }
        (Some(t0), _) if t0 < seconds_left => {
            score_pts.0 += 3.0; // Only team0 can win by score
        }
        (_, Some(t1)) if t1 < seconds_left => {
            score_pts.1 += 3.0; // Only team1 can win by score
        }
        _ => {}
    }

    // Sub-factor: projected final score gap (up to 3 pts)
    let proj_gap = (proj0 - proj1).abs();
    if proj_gap > 0.0 {
        let proj_pts = if proj_gap >= 300.0 {
            3.0
        } else if proj_gap >= 150.0 {
            2.0
        } else if proj_gap >= 50.0 {
            1.0
        } else {
            0.0
        };
        if proj0 > proj1 {
            score_pts.0 += proj_pts;
        } else {
            score_pts.1 += proj_pts;
        }
    }

    // Clamp to max
    bd.score_projection = (score_pts.0.min(MAX_SCORE_PROJECTION), score_pts.1.min(MAX_SCORE_PROJECTION));

    // ═══════════════════════════════════════════════════════════════════
    // Factor 2: Fleet Power (max 10 points, split proportionally)
    // Class-weighted HP × alive count. Only when HP data is reliable.
    // ═══════════════════════════════════════════════════════════════════

    if hp_data_reliable {
        let power0 = fleet_power(team0);
        let power1 = fleet_power(team1);
        let total_power = power0 + power1;

        if total_power > 0.0 {
            let frac0 = power0 / total_power;
            let frac1 = power1 / total_power;
            bd.fleet_power = (frac0 * MAX_FLEET_POWER, frac1 * MAX_FLEET_POWER);
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // Factor 3: Strategic Threat (max 5 points)
    // DD/SS survival, class diversity, CV advantage.
    // Only when HP data is reliable.
    // ═══════════════════════════════════════════════════════════════════

    if hp_data_reliable {
        let mut threat0: f32 = 0.0;
        let mut threat1: f32 = 0.0;

        // Time factor: strategic threats matter more with more time remaining.
        // Full weight at 5+ minutes, scales down linearly.
        let time_weight = (seconds_left / 300.0).clamp(0.2, 1.0) as f32;

        // DD/SS survival bonus (up to 2.5 pts)
        // These ships can contest caps and are very hard to eliminate.
        // A team losing on points but with DDs/SSs alive has a chance to come back.
        let dd_ss_score = |team: &TeamState| -> f32 {
            let dd_alive = team.destroyers.alive as f32;
            let ss_alive = team.submarines.alive as f32;
            // DDs worth 1.0 each, SSs worth 0.8 each (hard to kill but can't cap as well)
            (dd_alive * 1.0 + ss_alive * 0.8).min(2.5)
        };
        let dd_ss0 = dd_ss_score(team0) * time_weight;
        let dd_ss1 = dd_ss_score(team1) * time_weight;
        threat0 += dd_ss0;
        threat1 += dd_ss1;

        // Class diversity bonus (up to 1.5 pts)
        // A team with diverse classes alive is harder to eliminate.
        let diversity = |team: &TeamState| -> f32 {
            let mut classes = 0u32;
            if team.destroyers.alive > 0 {
                classes += 1;
            }
            if team.cruisers.alive > 0 {
                classes += 1;
            }
            if team.battleships.alive > 0 {
                classes += 1;
            }
            if team.submarines.alive > 0 {
                classes += 1;
            }
            if team.carriers.alive > 0 {
                classes += 1;
            }
            match classes {
                0..=1 => 0.0,
                2 => 0.5,
                3 => 1.0,
                _ => 1.5,
            }
        };
        threat0 += diversity(team0);
        threat1 += diversity(team1);

        // CV advantage (up to 1.0 pts)
        // Carrier spotting helps find DDs/SSs and project damage.
        let cv_diff = team0.carriers.alive as i32 - team1.carriers.alive as i32;
        if cv_diff > 0 {
            threat0 += 1.0;
        } else if cv_diff < 0 {
            threat1 += 1.0;
        }

        bd.strategic_threat = (threat0.min(MAX_STRATEGIC_THREAT), threat1.min(MAX_STRATEGIC_THREAT));
    }

    // ═══════════════════════════════════════════════════════════════════
    // Total and level determination
    // ═══════════════════════════════════════════════════════════════════

    let total0 = bd.score_projection.0 + bd.fleet_power.0 + bd.strategic_threat.0;
    let total1 = bd.score_projection.1 + bd.fleet_power.1 + bd.strategic_threat.1;
    bd.total = (total0, total1);

    let gap = (total0 - total1).abs();
    let team = if total0 > total1 { 0 } else { 1 };

    let advantage = if gap >= 10.0 {
        TeamAdvantage::for_team(team, AdvantageLevel::Absolute)
    } else if gap >= 6.0 {
        TeamAdvantage::for_team(team, AdvantageLevel::Strong)
    } else if gap >= 3.0 {
        TeamAdvantage::for_team(team, AdvantageLevel::Moderate)
    } else if gap >= 1.0 {
        TeamAdvantage::for_team(team, AdvantageLevel::Weak)
    } else {
        TeamAdvantage::Even
    };

    AdvantageResult { advantage, breakdown: bd }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_scoring() -> ScoringParams {
        ScoringParams { team_win_score: 1000, hold_reward: 3, hold_period: 5.0 }
    }

    /// Helper: create a balanced team with the given score and caps.
    /// 12 ships: 3 DD, 4 CA, 4 BB, 1 SS, all alive at full HP.
    fn even_team(score: i64, caps: usize) -> TeamState {
        TeamState {
            score,
            uncontested_caps: caps,
            total_hp: 100000.0,
            max_hp: 100000.0,
            ships_alive: 12,
            ships_total: 12,
            ships_known: 12,
            destroyers: ClassCount { alive: 3, total: 3, hp: 15000.0, max_hp: 15000.0 },
            cruisers: ClassCount { alive: 4, total: 4, hp: 40000.0, max_hp: 40000.0 },
            battleships: ClassCount { alive: 4, total: 4, hp: 40000.0, max_hp: 40000.0 },
            submarines: ClassCount { alive: 1, total: 1, hp: 5000.0, max_hp: 5000.0 },
            carriers: ClassCount::default(),
        }
    }

    #[test]
    fn even_game_start() {
        let t0 = even_team(0, 0);
        let t1 = even_team(0, 0);
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(1200));
        assert_eq!(r.advantage, TeamAdvantage::Even);
        // Both teams should have equal points
        assert!((r.breakdown.total.0 - r.breakdown.total.1).abs() < 0.01);
    }

    #[test]
    fn team_eliminated() {
        let mut t1 = even_team(300, 0);
        t1.ships_alive = 0;
        t1.total_hp = 0.0;
        let t0 = TeamState { ships_alive: 8, ..even_team(500, 2) };
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(600));
        assert_eq!(r.advantage, TeamAdvantage::Team0(AdvantageLevel::Absolute));
        assert!(r.breakdown.team_eliminated);
    }

    #[test]
    fn team_eliminated_other() {
        let mut t0 = even_team(300, 0);
        t0.ships_alive = 0;
        t0.total_hp = 0.0;
        let t1 = TeamState { ships_alive: 5, ..even_team(400, 3) };
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(600));
        assert_eq!(r.advantage, TeamAdvantage::Team1(AdvantageLevel::Absolute));
        assert!(r.breakdown.team_eliminated);
    }

    #[test]
    fn all_breakdown_values_non_negative() {
        let t0 = even_team(700, 3);
        let t1 = even_team(250, 0);
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(600));
        assert!(r.breakdown.score_projection.0 >= 0.0);
        assert!(r.breakdown.score_projection.1 >= 0.0);
        assert!(r.breakdown.fleet_power.0 >= 0.0);
        assert!(r.breakdown.fleet_power.1 >= 0.0);
        assert!(r.breakdown.strategic_threat.0 >= 0.0);
        assert!(r.breakdown.strategic_threat.1 >= 0.0);
        assert!(r.breakdown.total.0 >= 0.0);
        assert!(r.breakdown.total.1 >= 0.0);
    }

    #[test]
    fn score_gap_gives_points_to_leader() {
        let t0 = even_team(700, 2);
        let t1 = even_team(250, 1);
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(300));
        assert!(r.breakdown.score_projection.0 > r.breakdown.score_projection.1);
        assert!(matches!(r.advantage, TeamAdvantage::Team0(_)));
    }

    #[test]
    fn cap_advantage_projects_win() {
        let t0 = even_team(0, 3);
        let t1 = even_team(0, 0);
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(1200));
        assert!(matches!(r.advantage, TeamAdvantage::Team0(_)));
        // team0 should have score projection advantage
        assert!(r.breakdown.score_projection.0 > r.breakdown.score_projection.1);
    }

    #[test]
    fn fleet_power_12v6_strong_advantage() {
        let t0 = even_team(400, 1);
        let mut t1 = even_team(400, 1);
        // Kill half of team1's fleet
        t1.ships_alive = 6;
        t1.ships_known = 12;
        t1.total_hp = 50000.0;
        t1.destroyers = ClassCount { alive: 1, total: 3, hp: 5000.0, max_hp: 5000.0 };
        t1.cruisers = ClassCount { alive: 2, total: 4, hp: 20000.0, max_hp: 20000.0 };
        t1.battleships = ClassCount { alive: 2, total: 4, hp: 20000.0, max_hp: 20000.0 };
        t1.submarines = ClassCount { alive: 1, total: 1, hp: 5000.0, max_hp: 5000.0 };
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(600));
        // Fleet power should heavily favor team0
        assert!(r.breakdown.fleet_power.0 > r.breakdown.fleet_power.1);
        assert!(matches!(
            r.advantage,
            TeamAdvantage::Team0(AdvantageLevel::Moderate | AdvantageLevel::Strong | AdvantageLevel::Absolute)
        ));
    }

    #[test]
    fn fleet_power_2v1_less_extreme() {
        // Late game: 2 BBs vs 1 BB at full HP
        let t0 = TeamState {
            score: 800,
            uncontested_caps: 1,
            total_hp: 80000.0,
            max_hp: 80000.0,
            ships_alive: 2,
            ships_total: 12,
            ships_known: 12,
            destroyers: ClassCount::default(),
            cruisers: ClassCount::default(),
            battleships: ClassCount { alive: 2, total: 4, hp: 80000.0, max_hp: 80000.0 },
            submarines: ClassCount::default(),
            carriers: ClassCount::default(),
        };
        let t1 = TeamState {
            score: 800,
            uncontested_caps: 1,
            total_hp: 50000.0,
            max_hp: 50000.0,
            ships_alive: 1,
            ships_total: 12,
            ships_known: 12,
            destroyers: ClassCount::default(),
            cruisers: ClassCount::default(),
            battleships: ClassCount { alive: 1, total: 4, hp: 50000.0, max_hp: 50000.0 },
            submarines: ClassCount::default(),
            carriers: ClassCount::default(),
        };
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(300));
        // Should still favor team0 but not as dramatically as 12v6
        let power_gap = r.breakdown.fleet_power.0 - r.breakdown.fleet_power.1;
        assert!(power_gap > 0.0);
        // The 1 ship has high HP so the gap shouldn't be extreme
        assert!(power_gap < 8.0);
    }

    #[test]
    fn dd_survival_gives_threat_points() {
        // Team1 losing on points but has 2 DDs alive
        let t0 = TeamState { destroyers: ClassCount { alive: 0, total: 3, hp: 0.0, max_hp: 0.0 }, ..even_team(700, 2) };
        let t1 = TeamState {
            destroyers: ClassCount { alive: 2, total: 3, hp: 10000.0, max_hp: 10000.0 },
            ..even_team(400, 1)
        };
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(600));
        // Team1 should have more strategic threat points (DD survival)
        assert!(r.breakdown.strategic_threat.1 > r.breakdown.strategic_threat.0);
    }

    #[test]
    fn submarine_hard_to_kill() {
        // Team1 has a sub alive, team0 doesn't
        let t0 = TeamState { submarines: ClassCount::default(), ..even_team(600, 2) };
        let t1 = TeamState {
            submarines: ClassCount { alive: 1, total: 1, hp: 5000.0, max_hp: 5000.0 },
            ..even_team(500, 1)
        };
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(600));
        // Sub should contribute to team1's strategic threat
        assert!(r.breakdown.strategic_threat.1 > 0.0);
    }

    #[test]
    fn class_diversity_bonus() {
        // Team0 has 3 classes, team1 has only BBs
        let t0 = TeamState {
            ships_alive: 3,
            total_hp: 30000.0,
            max_hp: 30000.0,
            destroyers: ClassCount { alive: 1, total: 1, hp: 5000.0, max_hp: 5000.0 },
            cruisers: ClassCount { alive: 1, total: 1, hp: 10000.0, max_hp: 10000.0 },
            battleships: ClassCount { alive: 1, total: 1, hp: 15000.0, max_hp: 15000.0 },
            submarines: ClassCount::default(),
            carriers: ClassCount::default(),
            ..even_team(500, 1)
        };
        let t1 = TeamState {
            ships_alive: 3,
            total_hp: 45000.0,
            max_hp: 45000.0,
            destroyers: ClassCount::default(),
            cruisers: ClassCount::default(),
            battleships: ClassCount { alive: 3, total: 3, hp: 45000.0, max_hp: 45000.0 },
            submarines: ClassCount::default(),
            carriers: ClassCount::default(),
            ..even_team(500, 1)
        };
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(600));
        // Team0 has more diversity even though less HP
        assert!(r.breakdown.strategic_threat.0 > r.breakdown.strategic_threat.1);
    }

    #[test]
    fn no_time_left_limits_score_projection() {
        let t0 = even_team(800, 0);
        let t1 = even_team(700, 4);
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(5));
        // Team0 ahead on points, team1 has caps but no time to use them
        assert!(r.breakdown.score_projection.0 > r.breakdown.score_projection.1);
    }

    #[test]
    fn incomplete_entity_data_skips_fleet_and_threat() {
        let t0 = even_team(0, 0);
        let mut t1 = even_team(0, 0);
        t1.ships_known = 1;
        let r = calculate_advantage(&t0, &t1, &default_scoring(), Some(1200));
        assert!(!r.breakdown.hp_data_reliable);
        assert_eq!(r.breakdown.fleet_power, (0.0, 0.0));
        assert_eq!(r.breakdown.strategic_threat, (0.0, 0.0));
    }

    #[test]
    fn swap_breakdown_flips_tuples() {
        let mut bd = AdvantageBreakdown {
            score_projection: (7.0, 2.0),
            fleet_power: (6.0, 4.0),
            strategic_threat: (3.0, 1.0),
            total: (16.0, 7.0),
            team0_pps: 1.2,
            team1_pps: 0.6,
            ..Default::default()
        };
        swap_breakdown(&mut bd);
        assert_eq!(bd.score_projection, (2.0, 7.0));
        assert_eq!(bd.fleet_power, (4.0, 6.0));
        assert_eq!(bd.strategic_threat, (1.0, 3.0));
        assert_eq!(bd.total, (7.0, 16.0));
        assert!((bd.team0_pps - 0.6).abs() < 0.01);
        assert!((bd.team1_pps - 1.2).abs() < 0.01);
    }
}
