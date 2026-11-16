//! Battle impact (BI): a single-battle rating on the personal-rating scale,
//! built from the server's end-of-battle results for every player and, for
//! the recording player's team, from torpedo sightings in the packet stream.
//!
//! The rating is a core of damage and kills against each ship's wows-numbers
//! expected values, which can fall below average, plus contributions that only
//! ever add: share of the enemy hit point pool, spotting, drawing fire,
//! capture points, anti-air and torpedo warnings. 1150 is average play, as in
//! PR, so the PR bands and colors apply unchanged.
//!
//! Any change here that alters the rating a battle gets must raise
//! [`BattleImpactVersion::CURRENT`].

use std::collections::HashMap;
use std::collections::HashSet;

use serde::Serialize;
use wows_battle_world::report::BattleReport;
use wows_battle_world::resources::SightingObserver;
use wows_core::units::Km;
use wows_replays::types::AccountId;
use wows_replays::types::EntityId;
use wows_replays::types::TeamId;
use wows_replays::types::WorldPos;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::Direction;
use wowsunpack::game_types::ShotId;

use crate::battle_report::NormalizedPlayer;
use crate::battle_report::ServerResults;
use crate::personal_rating::PersonalRatingCategory;
use crate::personal_rating::PersonalRatingData;

pub use wows_core::game_types::BattleImpactVersion;

/// One player's battle impact.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BattleImpactResult {
    pub bi: f64,
    pub category: PersonalRatingCategory,
    pub version: BattleImpactVersion,
    pub components: BattleImpactComponents,
    /// `None` for players on a team no perspective of the replay belonged to,
    /// whose torpedo detections the replay does not show.
    pub torpedo_detection: Option<TorpedoDetection>,
}

/// Points each part of the rating contributed. They sum to `bi`.
///
/// A contribution is `None` where its input is not observable: a build whose
/// results lack the keys, or torpedo detections by a team the replay was not
/// recorded from. Every optional contribution only ever adds, so a missing
/// one leaves the rest of the rating meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BattleImpactComponents {
    pub damage: f64,
    pub kills: f64,
    pub enemy_hp_share: f64,
    pub tanking: f64,
    pub spotting: Option<f64>,
    pub objectives: Option<f64>,
    pub anti_air: Option<f64>,
    pub torpedo_warnings: Option<f64>,
}

impl BattleImpactComponents {
    fn total(&self) -> f64 {
        let core = self.damage + self.kills + self.enemy_hp_share + self.tanking;
        let additions: f64 =
            [self.spotting, self.objectives, self.anti_air, self.torpedo_warnings].into_iter().flatten().sum();

        core + additions
    }
}

/// Opposing torpedoes a player was the first of their team to see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct TorpedoDetection {
    /// Torpedoes attributed to this player.
    pub detected: u32,
    /// Those torpedoes weighted by how close they would have passed to a
    /// teammate: 1 within 1 km, falling to 0 at 3 km.
    pub threatening: f64,
}

/// Torpedo detections keyed by vehicle entity, for every player on a team a
/// perspective of the replay belonged to, including those who detected
/// nothing.
#[derive(Clone, Debug, Default)]
pub struct TorpedoDetections(HashMap<EntityId, TorpedoDetection>);

impl TorpedoDetections {
    /// Attribute every opposing torpedo each observing team saw.
    ///
    /// The spotter is the teammate nearest the torpedo when the client was
    /// sent it, which is when the team first detected it. Against the server's
    /// `tpds_spotted` this reproduces the per-player counts closely; the
    /// nearest teammate sits 1.1 to 1.4 km away, the torpedo detection range.
    /// A torpedo with no teammate within hydroacoustic range was found by
    /// aircraft and is attributed to nobody.
    ///
    /// Its threat is the closest it would have come to any other teammate,
    /// with both running straight on from that moment, over what is left of
    /// the torpedo's range and at most 5 km of it. Torpedoes aimed at the
    /// spotter are not counted: every ship sees the torpedoes about to hit it.
    pub fn from_report(report: &BattleReport, players: &[NormalizedPlayer], provider: &dyn GameParamProvider) -> Self {
        // Beyond the longest hydroacoustic search range.
        const MAX_SPOTTER_DISTANCE_KM: f32 = 6.0;
        // Position fixes for live vehicles arrive under a second apart.
        const MAX_FIX_AGE_SECS: f32 = 3.0;
        const MAX_THREAT_RUN: Km = Km::new(5.0);

        // Torpedo ranges and speeds are in GameParams units, as are packet
        // positions over the distances measured here.
        let max_threat_run = MAX_THREAT_RUN.to_bigworld().value();

        let self_team = players.iter().find(|p| p.is_self).map(|p| TeamId::from(p.team_id));
        let team_of: HashMap<EntityId, TeamId> =
            players.iter().map(|p| (p.entity_id, TeamId::from(p.team_id))).collect();

        let mut observing_teams: HashSet<TeamId> = self_team.into_iter().collect();
        let mut counted: HashSet<(TeamId, EntityId, ShotId)> = HashSet::new();
        let mut detections: HashMap<EntityId, TorpedoDetection> = HashMap::new();

        for sighting in report.torpedo_sightings() {
            // A spectator's single replay names no team to have seen it.
            let observer = match sighting.observer {
                SightingObserver::RecordingPlayer => self_team,
                SightingObserver::Team(team) => Some(team),
            };
            let Some(observer) = observer else {
                continue;
            };
            observing_teams.insert(observer);

            let teammates: Vec<_> = sighting
                .vehicles
                .iter()
                .filter(|fix| team_of.get(&fix.entity_id) == Some(&observer))
                .filter(|fix| report.deaths_by_victim().get(&fix.entity_id).is_none_or(|died| *died > sighting.clock))
                .filter(|fix| sighting.clock - fix.fixed_at <= MAX_FIX_AGE_SECS)
                .collect();

            for torpedo in &sighting.torpedoes {
                let Some(owner_team) = team_of.get(&torpedo.owner_id) else {
                    continue;
                };
                if *owner_team == observer {
                    continue;
                }
                // Merged perspectives on one team each forward the same torpedo.
                if !counted.insert((observer, torpedo.owner_id, torpedo.shot_id)) {
                    continue;
                }

                let Some(spotter) = teammates
                    .iter()
                    .map(|fix| (fix, fix.position.distance_xz(&torpedo.origin).to_km().value()))
                    .filter(|(_, km)| *km <= MAX_SPOTTER_DISTANCE_KM)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(fix, _)| fix)
                else {
                    continue;
                };

                let Some(range) = torpedo_range(provider, torpedo.params_id) else {
                    continue;
                };
                let speed = horizontal_length(&torpedo.direction);
                if speed <= 0.0 {
                    continue;
                }

                // The client is never told when a torpedo runs out. When the
                // launcher is in view, the run so far is the offset from it
                // back along the heading, since it fired from that track;
                // otherwise the full range bounds what is left. Past the cap
                // the projection says too little about where anyone will be.
                let launcher = sighting
                    .vehicles
                    .iter()
                    .find(|fix| fix.entity_id == torpedo.owner_id && sighting.clock - fix.fixed_at <= MAX_FIX_AGE_SECS);
                let run_so_far = launcher.map_or(0.0, |fix| {
                    let back_x = torpedo.origin.x - fix.position.x;
                    let back_z = torpedo.origin.z - fix.position.z;
                    ((back_x * torpedo.direction.x + back_z * torpedo.direction.z) / speed).max(0.0)
                });
                let remaining = (range - run_so_far).min(max_threat_run);
                let run_secs = remaining.max(0.0) / speed;

                // A teammate without a velocity cannot be projected, so it is
                // left out rather than assumed to be stopped.
                let closest_km = teammates
                    .iter()
                    .filter(|fix| fix.entity_id != spotter.entity_id)
                    .filter_map(|fix| {
                        let velocity = fix.velocity?;
                        Some(closest_approach_km(torpedo.origin, &torpedo.direction, fix.position, &velocity, run_secs))
                    })
                    .min_by(f32::total_cmp);

                let detection = detections.entry(spotter.entity_id).or_default();
                detection.detected += 1;
                detection.threatening += closest_km.map_or(0.0, threat_weight);
            }
        }

        for (entity_id, team) in &team_of {
            if observing_teams.contains(team) {
                detections.entry(*entity_id).or_default();
            }
        }

        Self(detections)
    }

    pub fn get(&self, vehicle: EntityId) -> Option<TorpedoDetection> {
        self.0.get(&vehicle).copied()
    }
}

/// Rate every player in one battle. The result is positional with `players`;
/// a player is `None` without server results carrying damage, kills and hit
/// points, without expected values for the ship, or in a ship class the
/// rating has no baseline for.
pub fn rate_battle(
    players: &[NormalizedPlayer],
    table: &PersonalRatingData,
    torpedoes: &TorpedoDetections,
) -> Vec<Option<BattleImpactResult>> {
    let context = BattleContext::new(players);

    players.iter().map(|player| rate_player(player, &context, table, torpedoes)).collect()
}

/// Per-team totals the per-player axes are measured against.
struct BattleContext {
    teams: HashMap<u32, TeamTotals>,
    victims: HashMap<AccountId, Victim>,
}

#[derive(Default)]
struct TeamTotals {
    players: u32,
    /// Hit points, with destroyers and submarines weighted up.
    weighted_hp_pool: f64,
    potential_damage: f64,
    /// Sum over the team of potential damage per hit point.
    potential_per_hp: f64,
}

struct Victim {
    max_health: u64,
    is_priority: bool,
}

impl BattleContext {
    fn new(players: &[NormalizedPlayer]) -> Self {
        let mut teams: HashMap<u32, TeamTotals> = HashMap::new();
        let mut victims = HashMap::new();

        for player in players {
            let Some(results) = &player.server_results else {
                continue;
            };
            let Some(max_health) = results.max_health.filter(|hp| *hp > 0) else {
                continue;
            };
            let is_priority = is_priority_target(player.ship_class);

            let team = teams.entry(player.team_id).or_default();
            team.players += 1;
            team.weighted_hp_pool += max_health as f64 * priority_weight(is_priority);
            team.potential_damage += results.potential_damage as f64;
            team.potential_per_hp += results.potential_damage as f64 / max_health as f64;

            victims.insert(player.db_id, Victim { max_health, is_priority });
        }

        Self { teams, victims }
    }

    /// Hit points of every team but `team_id`, weighted as in `TeamTotals`.
    /// `None` when no other team has a rated player.
    fn opposing_hp_pool(&self, team_id: u32) -> Option<f64> {
        self.teams
            .iter()
            .filter(|(id, _)| **id != team_id)
            .map(|(_, totals)| totals.weighted_hp_pool)
            .reduce(|a, b| a + b)
    }
}

fn rate_player(
    player: &NormalizedPlayer,
    context: &BattleContext,
    table: &PersonalRatingData,
    torpedoes: &TorpedoDetections,
) -> Option<BattleImpactResult> {
    let results = player.server_results.as_ref()?;
    let damage = results.damage? as f64;
    let kills = results.kills? as f64;
    let max_health = results.max_health.filter(|hp| *hp > 0)? as f64;
    let expected = table
        .get_ship_expected(player.ship_id)
        .filter(|expected| expected.average_damage_dealt > 0.0 && expected.average_frags > 0.0)?;
    let spotting_baseline = spotting_baseline(player.ship_class)?;
    let team = context.teams.get(&player.team_id)?;
    let opposing_hp_pool = context.opposing_hp_pool(player.team_id).filter(|pool| *pool > 0.0)?;

    let weighted_damage = damage + priority_damage(results, context) * (priority_weight(true) - 1.0);

    let damage_points = 550.0 * normalize(weighted_damage / expected.average_damage_dealt, 0.4);
    let kill_points = 150.0 * normalize(kills / expected.average_frags, 0.1);

    let enemy_hp_share = 200.0 * weighted_damage / opposing_hp_pool * f64::from(team.players);

    let tanking = 120.0 * tanking_ratio(results.potential_damage as f64, max_health, team);

    let spotting = results.spotting_damage.map(|spotting_damage| {
        0.2 * spotting_score(spotting_ratio(spotting_damage as f64, results, context, spotting_baseline))
    });

    let objectives = results.objective_points.map(|points| {
        // Hold points accrue to the captors for as long as the team keeps the
        // point, which is mostly the team's doing.
        (points.capture + points.block + points.drop + points.battle_end + 0.25 * points.hold) / 100.0
    });

    let anti_air = anti_air_points(results);

    let torpedo_detection = torpedoes.get(player.entity_id);
    let torpedo_warnings = torpedo_detection.map(|detection| 8.0 * detection.threatening);

    let components = BattleImpactComponents {
        damage: damage_points,
        kills: kill_points,
        enemy_hp_share,
        tanking,
        spotting,
        objectives,
        anti_air,
        torpedo_warnings,
    };
    let bi = components.total();

    Some(BattleImpactResult {
        bi,
        category: PersonalRatingCategory::from_pr(bi),
        version: BattleImpactVersion::CURRENT,
        components,
        torpedo_detection,
    })
}

/// Destroyers and submarines: damage to them counts extra.
fn is_priority_target(class: Species) -> bool {
    matches!(class, Species::Destroyer | Species::Submarine)
}

fn priority_weight(is_priority: bool) -> f64 {
    if is_priority { 1.2 } else { 1.0 }
}

/// Damage this player dealt to destroyers and submarines.
fn priority_damage(results: &ServerResults, context: &BattleContext) -> f64 {
    results
        .damage_interactions
        .iter()
        .filter(|(victim, _)| context.victims.get(victim).is_some_and(|v| v.is_priority))
        .map(|(_, interaction)| interaction.damage_dealt as f64)
        .sum()
}

/// Spotting damage at which a ship's spotting is average.
fn spotting_baseline(class: Species) -> Option<f64> {
    match class {
        Species::Destroyer | Species::Submarine | Species::AirCarrier => Some(75_000.0),
        Species::Cruiser | Species::Battleship => Some(35_000.0),
        _ => None,
    }
}

/// Spotting against the class baseline, half by damage and half by the share
/// of the spotted ships' hit points it amounted to.
fn spotting_ratio(spotting_damage: f64, results: &ServerResults, context: &BattleContext, baseline: f64) -> f64 {
    // Typical hit points of a ship being spotted, which turns the damage
    // baseline into a baseline in ships' worth of hit points.
    const TYPICAL_VICTIM_HP: f64 = 65_000.0;

    // A victim missing from the results has no hit points to divide by and
    // adds nothing to the share.
    let ships_spotted: f64 = results
        .damage_interactions
        .iter()
        .filter_map(|(victim, interaction)| {
            let victim = context.victims.get(victim)?;
            Some(interaction.spotting_damage as f64 / victim.max_health as f64)
        })
        .sum();

    0.5 * (spotting_damage / baseline) + 0.5 * (ships_spotted / (baseline / TYPICAL_VICTIM_HP))
}

/// Maps a spotting ratio onto the rating scale: the baseline is average,
/// twice it is Great, and seven thirds of it is Super Unicum.
fn spotting_score(ratio: f64) -> f64 {
    const ANCHORS: [(f64, f64); 3] = [(1.0, 1150.0), (2.0, 1750.0), (7.0 / 3.0, 2450.0)];

    if ratio <= ANCHORS[0].0 {
        return ANCHORS[0].1 * ratio;
    }
    for pair in ANCHORS.windows(2) {
        let ((low, low_score), (high, high_score)) = (pair[0], pair[1]);
        if ratio <= high {
            return low_score + (ratio - low) / (high - low) * (high_score - low_score);
        }
    }

    // Past the last anchor, continue at the average slope across all of them
    // rather than the steep final segment.
    let (first, first_score) = ANCHORS[0];
    let (last, last_score) = ANCHORS[ANCHORS.len() - 1];
    last_score + (ratio - last) * (last_score - first_score) / (last - first)
}

/// Geometric mean of the player's share of the team's potential damage and
/// their potential damage per hit point, each relative to the team average.
fn tanking_ratio(potential: f64, max_health: f64, team: &TeamTotals) -> f64 {
    if team.potential_damage <= 0.0 || team.potential_per_hp <= 0.0 {
        return 0.0;
    }
    let players = f64::from(team.players);
    let share = potential / team.potential_damage * players;
    let per_hp = (potential / max_health) / (team.potential_per_hp / players);

    (share * per_hp).sqrt()
}

/// `None` on builds whose results report neither plane kills by kind nor
/// damage to aircraft.
fn anti_air_points(results: &ServerResults) -> Option<f64> {
    let planes = results.planes_killed.map(|planes| {
        let strike = planes.rocket_planes + planes.bombers + planes.torpedo_bombers + planes.skip_bombers;
        let support = planes.consumable_fighters + planes.scouts;
        3.0 * strike as f64 + 0.75 * support as f64
    });
    let damage = results.anti_air_damage.map(|damage| damage as f64 / 2000.0);

    // Both are tallies of what the player did, so a build reporting only one
    // still credits that one.
    match (planes, damage) {
        (None, None) => None,
        (planes, damage) => Some(planes.unwrap_or(0.0) + damage.unwrap_or(0.0)),
    }
}

/// PR's normalization: 0 at `floor`, 1 at the expected value.
fn normalize(ratio: f64, floor: f64) -> f64 {
    f64::max(0.0, (ratio - floor) / (1.0 - floor))
}

/// Torpedo range in position units, which the projectile states in GameParams
/// units. Torpedo speeds in the packet stream match the GameParams speed in
/// those units, so the two can be divided.
fn torpedo_range(provider: &dyn GameParamProvider, params_id: wows_replays::types::GameParamId) -> Option<f32> {
    let param = provider.game_param_by_id(params_id)?;
    let ParamData::Projectile(projectile) = param.data() else {
        return None;
    };

    projectile.max_dist().map(|dist| dist.value())
}

fn horizontal_length(direction: &Direction) -> f32 {
    (direction.x * direction.x + direction.z * direction.z).sqrt()
}

/// Closest horizontal approach between a torpedo and a ship over the
/// torpedo's remaining run, both moving in straight lines.
fn closest_approach_km(
    torpedo_origin: WorldPos,
    torpedo_velocity: &Direction,
    ship_position: WorldPos,
    ship_velocity: &Direction,
    run_secs: f32,
) -> f32 {
    let offset_x = torpedo_origin.x - ship_position.x;
    let offset_z = torpedo_origin.z - ship_position.z;
    let closing_x = torpedo_velocity.x - ship_velocity.x;
    let closing_z = torpedo_velocity.z - ship_velocity.z;

    let closing_sq = closing_x * closing_x + closing_z * closing_z;
    let at = if closing_sq > 0.0 {
        (-(offset_x * closing_x + offset_z * closing_z) / closing_sq).clamp(0.0, run_secs)
    } else {
        0.0
    };

    let torpedo_at =
        WorldPos::new(torpedo_origin.x + torpedo_velocity.x * at, 0.0, torpedo_origin.z + torpedo_velocity.z * at);
    let ship_at = WorldPos::new(ship_position.x + ship_velocity.x * at, 0.0, ship_position.z + ship_velocity.z * at);

    torpedo_at.distance_xz(&ship_at).to_km().value()
}

fn threat_weight(closest_km: f32) -> f64 {
    const FULL_KM: f32 = 1.0;
    const NONE_KM: f32 = 3.0;

    f64::from(((NONE_KM - closest_km) / (NONE_KM - FULL_KM)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotting_score_meets_the_band_anchors() {
        assert_eq!(spotting_score(0.0), 0.0);
        assert_eq!(spotting_score(1.0), 1150.0);
        assert_eq!(spotting_score(2.0), 1750.0);
        assert!((spotting_score(7.0 / 3.0) - 2450.0).abs() < 1e-9);
        assert_eq!(
            PersonalRatingCategory::from_pr(spotting_score(7.0 / 3.0) + 1e-6),
            PersonalRatingCategory::SuperUnicum
        );
    }

    #[test]
    fn spotting_score_keeps_rising_past_the_last_anchor() {
        let at_anchor = spotting_score(7.0 / 3.0);
        let past = spotting_score(3.0);
        assert!(past > at_anchor);
        // Past the last anchor the score rises at the average slope across
        // the anchors, not the steep final segment.
        assert!((past - at_anchor - (3.0 - 7.0 / 3.0) * 975.0).abs() < 1e-6);
    }

    #[test]
    fn threat_weight_falls_from_one_to_three_km() {
        assert_eq!(threat_weight(0.2), 1.0);
        assert_eq!(threat_weight(1.0), 1.0);
        assert_eq!(threat_weight(2.0), 0.5);
        assert_eq!(threat_weight(3.0), 0.0);
        assert_eq!(threat_weight(5.0), 0.0);
    }

    #[test]
    fn a_torpedo_crossing_a_ships_track_comes_close() {
        // Torpedo heading +z from the origin; ship heading -x, timed to cross.
        let closest = closest_approach_km(
            WorldPos::new(0.0, 0.0, 0.0),
            &Direction(wowsunpack::game_types::Vec3::new(0.0, 0.0, 6.0)),
            WorldPos::new(20.0, 0.0, 60.0),
            &Direction(wowsunpack::game_types::Vec3::new(-2.0, 0.0, 0.0)),
            100.0,
        );
        assert!(closest < 0.05, "closest {closest}");
    }

    #[test]
    fn a_torpedo_that_runs_out_never_reaches_the_ship() {
        // Same geometry, but the torpedo stops after 5 s, 30 units short.
        let closest = closest_approach_km(
            WorldPos::new(0.0, 0.0, 0.0),
            &Direction(wowsunpack::game_types::Vec3::new(0.0, 0.0, 6.0)),
            WorldPos::new(20.0, 0.0, 60.0),
            &Direction(wowsunpack::game_types::Vec3::new(-2.0, 0.0, 0.0)),
            5.0,
        );
        assert!(closest > 0.9, "closest {closest}");
    }
}
