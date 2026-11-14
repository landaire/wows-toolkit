//! Battle impact (BI): a single-battle rating on the personal-rating scale,
//! built from the server's end-of-battle results for every player and, for
//! the recording player's team, from torpedo sightings in the packet stream.
//!
//! The rating is a core of damage and kills against each ship's wows-numbers
//! expected values, which can fall below average, plus each player's share of
//! the damage their team dealt and the fire they drew, and contributions that
//! only ever add: spotting, capture points, anti-air, torpedo warnings and
//! critical bursts. The parts with an average sum to 1130 at average play and
//! the bonuses add the rest, so a typical battle lands near PR's average of
//! 1150 (1138 across the fixture battles) and the PR bands and colors apply.
//!
//! Any change here that alters the rating a battle gets must raise
//! [`BattleImpactVersion::CURRENT`].

use std::collections::HashMap;
use std::collections::HashSet;

use serde::Serialize;
use wows_battle_world::report::BattleReport;
use wows_battle_world::resources::SightingObserver;
use wows_core::units::Km;
use wows_core::units::Seconds;
use wows_replays::analyzer::battle_controller::DamageEvent;
use wows_replays::types::AccountId;
use wows_replays::types::EntityId;
use wows_replays::types::GameParamId;
use wows_replays::types::TeamId;
use wows_replays::types::WorldPos;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_types::Direction;
use wowsunpack::game_types::ShotId;

use crate::battle_report::NormalizedPlayer;
use crate::battle_report::ObjectivePoints;
use crate::battle_report::PlanesKilled;
use crate::battle_report::ServerResults;
use crate::personal_rating::PersonalRatingCategory;
use crate::personal_rating::PersonalRatingData;

pub use wows_core::game_types::BattleImpactVersion;

/// Points each core part earns for average play, so a front end can show
/// what pulled a rating up or down.
const DAMAGE_POINTS: f64 = 550.0;
const KILL_POINTS: f64 = 150.0;
const TEAM_DAMAGE_SHARE_POINTS: f64 = 200.0;
const SPOTTING_SCALE: f64 = 0.2;

/// Tanking reward per unit of fire drawn above both the class peers' mean
/// and an even share of the team's: drawing twice both earns this much.
const TANKING_REWARD_PER_EXCESS: f64 = 100.0;
/// Reached at two and a half times both references, about the top tenth of
/// battleships in the fixture battles.
const TANKING_MAX_REWARD: f64 = 150.0;
/// Fire drawn, relative to the class peers' mean, below which a class that
/// is penalized starts losing points. About the bottom fifth of battleships
/// and cruisers in the fixture battles sit under it.
const TANKING_PENALTY_BELOW: f64 = 0.5;

/// How far tanking can move each class's rating, the one place its
/// asymmetry lives. Only the penalty differs by class:
/// - destroyers, submarines and carriers survive by not being shot at, so
///   drawing little fire is the class played well and costs nothing;
/// - cruisers screen and trade, so hiding from fire costs a little;
/// - battleships carry the hit points and armor to absorb fire, and one
///   drawing far less than its peers leaves that fire on its teammates.
const TANKING_WEIGHTS: [(Species, TankingWeights); 5] = [
    (Species::Destroyer, TankingWeights { max_penalty: 0.0 }),
    (Species::Submarine, TankingWeights { max_penalty: 0.0 }),
    (Species::AirCarrier, TankingWeights { max_penalty: 0.0 }),
    (Species::Cruiser, TankingWeights { max_penalty: 40.0 }),
    (Species::Battleship, TankingWeights { max_penalty: 150.0 }),
];

/// A critical burst removes at least this share of one enemy's maximum hit
/// points. Half is where a ship stops trading: a destroyer at half must
/// disengage, a battleship has lost more than its heal restores. The six
/// random fixture battles hold about three such bursts each; at 90% they
/// held one between them, too rare to score.
const CRITICAL_BURST_SHARE: f64 = 0.5;
/// The span a burst's damage must land in. Across the fixture battles 22
/// bursts land within 3 s, 28 within 10 s and 35 within 20 s: the count
/// levels off once one salvo or torpedo spread fits with its spread of
/// arrival times, and the span stays under battleship reloads.
const CRITICAL_BURST_WINDOW_SECS: f32 = 10.0;
/// Points per burst. Most bursts end in a kill the kill part already
/// credits, so this rewards the tempo, not the kill again.
const CRITICAL_BURST_POINTS: f64 = 25.0;

/// One part of the rating.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum BattleImpactFactor {
    Damage,
    Kills,
    TeamDamageShare,
    Tanking,
    Spotting,
    Objectives,
    AntiAir,
    TorpedoWarnings,
    CriticalBursts,
}

impl BattleImpactFactor {
    /// What the part earns for average play: expected damage and kills, an
    /// even share of the team's damage, nothing either way for drawing the
    /// usual fire for the class and team, and spotting at the class baseline.
    /// `None` for objectives, anti-air and torpedo warnings, which have no
    /// average; any of them is a bonus.
    pub fn average_points(self) -> Option<f64> {
        match self {
            Self::Damage => Some(DAMAGE_POINTS),
            Self::Kills => Some(KILL_POINTS),
            Self::TeamDamageShare => Some(TEAM_DAMAGE_SHARE_POINTS),
            Self::Tanking => Some(0.0),
            Self::Spotting => Some(SPOTTING_SCALE * 1150.0),
            Self::Objectives | Self::AntiAir | Self::TorpedoWarnings | Self::CriticalBursts => None,
        }
    }

    /// What the part measures a player against.
    pub fn basis(self) -> FactorBasis {
        match self {
            // wows-numbers expected values for the ship.
            Self::Damage | Self::Kills => FactorBasis::Dynamic,
            // The other players in this battle.
            Self::TeamDamageShare | Self::Tanking => FactorBasis::Dynamic,
            // A class baseline.
            Self::Spotting => FactorBasis::Static,
            // Fixed points per unit of what the player did.
            Self::Objectives | Self::AntiAir | Self::TorpedoWarnings => FactorBasis::Static,
            // Fixed points per burst past a fixed share of the victim's hit points.
            Self::CriticalBursts => FactorBasis::Static,
        }
    }
}

/// What a factor measures a player against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum FactorBasis {
    /// A general expectation: a fixed constant or a class baseline.
    Static,
    /// Data: wows-numbers expected values for the ship, or the other players
    /// in this battle.
    Dynamic,
}

/// One part's points and the basis they were measured against for this
/// player.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct FactorScore {
    pub points: f64,
    pub basis: FactorBasis,
}

impl FactorScore {
    fn new(factor: BattleImpactFactor, points: f64) -> Self {
        Self { points, basis: factor.basis() }
    }
}

/// One player's battle impact.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BattleImpactResult {
    pub bi: f64,
    pub category: PersonalRatingCategory,
    pub version: BattleImpactVersion,
    pub components: BattleImpactComponents,
    pub inputs: BattleImpactInputs,
    /// `None` for players on a team no perspective of the replay belonged to,
    /// whose torpedo detections the replay does not show.
    pub torpedo_detection: Option<TorpedoDetection>,
    /// `None` when the replay carries no damage events. Only damage to ships
    /// the recording client was told about is seen, so a burst on a ship far
    /// from the recording player can be missing.
    pub critical_bursts: Option<Vec<CriticalBurst>>,
}

/// Points each part of the rating contributed. They sum to `bi`.
///
/// A contribution is `None` where its input is not observable: a build whose
/// results lack the keys, torpedo detections by a team the replay was not
/// recorded from, or a share of team damage when the team dealt none. Every
/// optional bonus only ever adds, so a missing one leaves the rest of the
/// rating meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BattleImpactComponents {
    pub damage: FactorScore,
    pub kills: FactorScore,
    pub team_damage_share: Option<FactorScore>,
    /// `None` without a class peer to compare against or fire drawn by the
    /// team. Negative only for cruisers and battleships.
    pub tanking: Option<FactorScore>,
    pub spotting: Option<FactorScore>,
    pub objectives: Option<FactorScore>,
    pub anti_air: Option<FactorScore>,
    pub torpedo_warnings: Option<FactorScore>,
    pub critical_bursts: Option<FactorScore>,
}

impl BattleImpactComponents {
    /// Every present part with its factor.
    pub fn scores(&self) -> impl Iterator<Item = (BattleImpactFactor, FactorScore)> {
        [
            (BattleImpactFactor::Damage, Some(self.damage)),
            (BattleImpactFactor::Kills, Some(self.kills)),
            (BattleImpactFactor::TeamDamageShare, self.team_damage_share),
            (BattleImpactFactor::Tanking, self.tanking),
            (BattleImpactFactor::Spotting, self.spotting),
            (BattleImpactFactor::Objectives, self.objectives),
            (BattleImpactFactor::AntiAir, self.anti_air),
            (BattleImpactFactor::TorpedoWarnings, self.torpedo_warnings),
            (BattleImpactFactor::CriticalBursts, self.critical_bursts),
        ]
        .into_iter()
        .filter_map(|(factor, score)| Some((factor, score?)))
    }

    pub fn total(&self) -> f64 {
        self.scores().map(|(_, score)| score.points).sum()
    }
}

/// What the rating was computed from, for showing beside each part.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BattleImpactInputs {
    /// Damage dealt, with damage to destroyers and submarines weighted up.
    pub weighted_damage: f64,
    /// wows-numbers average damage for the ship.
    pub expected_damage: f64,
    pub kills: f64,
    /// wows-numbers average kills for the ship.
    pub expected_kills: f64,
    /// `None` when the player's team dealt no damage.
    pub team_damage_share: Option<TeamDamageShare>,
    pub potential_damage: u64,
    /// `None` when `tanking` in the components is.
    pub tanking: Option<TankingRatios>,
    pub spotting_damage: Option<u64>,
    /// Spotting damage at which the ship's class is average.
    pub spotting_baseline: f64,
    /// Spotted ships' hit points this player was responsible for, in ships.
    pub ships_spotted: f64,
    pub ships_spotted_baseline: f64,
    pub objective_points: Option<ObjectivePoints>,
    pub planes_killed: Option<PlanesKilled>,
    pub anti_air_damage: Option<u64>,
}

/// A fraction of a whole: 1 is all of it.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Share(pub f64);

/// A player's part of the damage their team dealt to enemies.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TeamDamageShare {
    /// The player's damage over the team's.
    pub share: Share,
    /// One player's share had every teammate dealt the same.
    pub even: Share,
}

impl TeamDamageShare {
    /// `None` when the team dealt no damage, where a share is undefined.
    fn new(damage: f64, team: &TeamTotals) -> Option<Self> {
        if team.damage_dealt <= 0.0 || team.damage_dealers == 0 {
            return None;
        }

        Some(Self { share: Share(damage / team.damage_dealt), even: Share(1.0 / f64::from(team.damage_dealers)) })
    }

    /// Average play's points at an even share. Over a team the shares sum to
    /// one, so the team's mean is exactly that average.
    fn points(self) -> f64 {
        TEAM_DAMAGE_SHARE_POINTS * self.share.0 / self.even.0
    }
}

/// Opposing torpedoes a player was the first of their team to see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct TorpedoDetection {
    /// Torpedoes attributed to this player.
    pub detected: u32,
    /// Those whose projected track passed within 3 km of a teammate.
    pub threatening: u32,
    /// The threatening torpedoes, each weighted by how close it would have
    /// passed: 1 within 1 km, falling to 0 at 3 km. Scores the part; the
    /// count above is what is shown.
    pub threat_weight: f64,
}

impl TorpedoDetection {
    /// One more torpedo attributed to this player. `closest_km` is its closest
    /// projected approach to a teammate, `None` when no teammate could be
    /// projected.
    fn record(&mut self, closest_km: Option<f32>) {
        self.detected += 1;

        let weight = closest_km.map_or(0.0, threat_weight);
        if weight > 0.0 {
            self.threatening += 1;
            self.threat_weight += weight;
        }
    }

    /// Never below zero: every recorded weight is.
    fn points(self) -> f64 {
        // A torpedo that would have passed within a kilometer of a teammate.
        const POINTS_PER_TORPEDO: f64 = 8.0;

        POINTS_PER_TORPEDO * self.threat_weight
    }
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

                detections.entry(spotter.entity_id).or_default().record(closest_km);
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

/// One enemy that lost a large part of its hit points to one player within
/// a short window, which changes how that enemy can go on playing.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CriticalBurst {
    pub victim_ship: GameParamId,
    /// The victim ship's localized name. `None` for an untranslated ship.
    pub victim_ship_name: Option<String>,
    /// Damage within the window over the victim's maximum hit points.
    pub share: Share,
    /// From the first hit in the window to the last.
    pub duration: Seconds,
}

/// Critical bursts keyed by aggressor vehicle entity.
#[derive(Clone, Debug, Default)]
pub struct CriticalBursts(HashMap<EntityId, Vec<CriticalBurst>>);

impl CriticalBursts {
    /// `None` when the replay carries no damage events at all, which leaves
    /// bursts unobservable rather than absent.
    ///
    /// The client is only sent damage to ships inside its area of interest,
    /// so a burst on a ship far from the recording player can be missed.
    /// Nothing unobserved is estimated.
    pub fn from_report(report: &BattleReport, players: &[NormalizedPlayer]) -> Option<Self> {
        if report.damage_events().is_empty() {
            return None;
        }

        let by_vehicle: HashMap<EntityId, &NormalizedPlayer> =
            players.iter().map(|player| (player.entity_id, player)).collect();
        let mut bursts = HashMap::new();
        for (aggressor, events) in report.damage_events() {
            let Some(attacker) = by_vehicle.get(aggressor) else {
                continue;
            };
            let found = find_bursts(events, |victim| {
                let victim = by_vehicle.get(&victim).filter(|victim| victim.team_id != attacker.team_id)?;
                let max_health = victim.server_results.as_ref()?.max_health.filter(|hp| *hp > 0)?;

                Some(BurstVictim {
                    max_health: max_health as f64,
                    ship: victim.ship_id,
                    ship_name: victim.ship_name.as_deref(),
                })
            });
            bursts.insert(*aggressor, found);
        }

        Some(Self(bursts))
    }

    /// Empty for a player the replay saw deal no burst.
    pub fn get(&self, vehicle: EntityId) -> &[CriticalBurst] {
        self.0.get(&vehicle).map_or(&[], Vec::as_slice)
    }
}

/// What a burst on one victim is measured against.
struct BurstVictim<'a> {
    max_health: f64,
    ship: GameParamId,
    ship_name: Option<&'a str>,
}

/// The heaviest stretch of damage within the window.
struct BurstWindow {
    damage: f64,
    duration: Seconds,
}

/// One aggressor's bursts: per victim `victim` describes, at most one, the
/// heaviest window. A ship cannot lose half its hit points twice without
/// healing back more than most ships can, and counting once keeps
/// overlapping windows from each counting.
fn find_bursts<'a>(events: &[DamageEvent], victim: impl Fn(EntityId) -> Option<BurstVictim<'a>>) -> Vec<CriticalBurst> {
    let mut by_victim: HashMap<EntityId, Vec<&DamageEvent>> = HashMap::new();
    for event in events {
        by_victim.entry(event.victim).or_default().push(event);
    }

    let mut bursts = Vec::new();
    for (victim_id, mut hits) in by_victim {
        let Some(victim) = victim(victim_id) else {
            continue;
        };
        hits.sort_by(|a, b| a.clock.seconds().total_cmp(&b.clock.seconds()));
        let Some(window) = heaviest_window(&hits) else {
            continue;
        };
        let share = window.damage / victim.max_health;
        if share < CRITICAL_BURST_SHARE {
            continue;
        }

        bursts.push(CriticalBurst {
            victim_ship: victim.ship,
            victim_ship_name: victim.ship_name.map(str::to_owned),
            share: Share(share),
            duration: window.duration,
        });
    }

    // `by_victim` iterates in no fixed order.
    bursts.sort_by(|a, b| b.share.0.total_cmp(&a.share.0).then_with(|| a.victim_ship.raw().cmp(&b.victim_ship.raw())));

    bursts
}

/// The most damage `hits`, in clock order, deal within any span of
/// [`CRITICAL_BURST_WINDOW_SECS`]. `None` without hits.
fn heaviest_window(hits: &[&DamageEvent]) -> Option<BurstWindow> {
    let mut heaviest: Option<BurstWindow> = None;
    let mut start = 0;
    let mut damage = 0.0;

    for hit in hits {
        damage += f64::from(hit.amount);
        while hit.clock.seconds() - hits[start].clock.seconds() > CRITICAL_BURST_WINDOW_SECS {
            damage -= f64::from(hits[start].amount);
            start += 1;
        }

        if heaviest.as_ref().is_none_or(|heaviest| damage > heaviest.damage) {
            let duration = Seconds::new(hit.clock.seconds() - hits[start].clock.seconds());
            heaviest = Some(BurstWindow { damage, duration });
        }
    }

    heaviest
}

/// Rate every player in one battle. The result is positional with `players`;
/// a player is `None` without server results carrying damage, kills and hit
/// points, without expected values for the ship, or in a ship class the
/// rating has no baseline for.
pub fn rate_battle(
    players: &[NormalizedPlayer],
    table: &PersonalRatingData,
    torpedoes: &TorpedoDetections,
    bursts: Option<&CriticalBursts>,
) -> Vec<Option<BattleImpactResult>> {
    let context = BattleContext::new(players);

    players.iter().map(|player| rate_player(player, &context, table, torpedoes, bursts)).collect()
}

/// A quantity relative to a reference: 1 is equal to it.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Ratio(pub f64);

/// Fire a player drew, as potential damage, against two references.
///
/// The class reference is every other ship of the class in the battle, on
/// both teams: matchmaking mirrors class counts, so the two teams together
/// give a peer for nearly every ship, where one team often has a single
/// carrier, submarine or battleship.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TankingRatios {
    /// Over the class peers' mean.
    pub class: Ratio,
    pub class_peers: u32,
    /// Over an even share of the potential damage the team drew.
    pub team: Ratio,
}

impl TankingRatios {
    /// `None` with no class peer, peers that drew no fire, or a team that
    /// drew none.
    fn new(potential: f64, class: Species, context: &BattleContext, team: &TeamTotals) -> Option<Self> {
        let peers = context.classes.get(&class)?;
        let class_peers = peers.players.checked_sub(1).filter(|peers| *peers > 0)?;
        let peer_mean = (peers.potential_damage - potential) / f64::from(class_peers);
        if peer_mean <= 0.0 || team.potential_damage <= 0.0 {
            return None;
        }

        Some(Self {
            class: Ratio(potential / peer_mean),
            class_peers,
            team: Ratio(potential / team.potential_damage * f64::from(team.players)),
        })
    }

    /// A reward needs both: more fire than the class usually draws, and more
    /// than an even part of what the team drew. A penalty follows only the
    /// class, weighted by `weights`.
    fn points(self, weights: TankingWeights) -> f64 {
        let excess = self.class.0.min(self.team.0) - 1.0;
        let reward = (TANKING_REWARD_PER_EXCESS * excess).clamp(0.0, TANKING_MAX_REWARD);

        let shortfall = ((TANKING_PENALTY_BELOW - self.class.0) / TANKING_PENALTY_BELOW).clamp(0.0, 1.0);
        let penalty = weights.max_penalty * shortfall;

        reward - penalty
    }
}

#[derive(Clone, Copy, Debug)]
struct TankingWeights {
    /// Points taken from a ship that drew no fire at all.
    max_penalty: f64,
}

impl TankingWeights {
    fn of(class: Species) -> Option<Self> {
        TANKING_WEIGHTS.iter().find(|(species, _)| *species == class).map(|(_, weights)| *weights)
    }
}

/// Per-team and per-class totals the per-player axes are measured against.
struct BattleContext {
    teams: HashMap<u32, TeamTotals>,
    classes: HashMap<Species, ClassTotals>,
    victims: HashMap<AccountId, Victim>,
}

#[derive(Default)]
struct TeamTotals {
    /// Teammates with hit points, which the tanking axis is measured over.
    players: u32,
    potential_damage: f64,

    /// Damage dealt to enemies by every teammate whose results carry it.
    damage_dealt: f64,
    damage_dealers: u32,
}

/// Ships of one class on both teams, with hit points.
#[derive(Default)]
struct ClassTotals {
    players: u32,
    potential_damage: f64,
}

struct Victim {
    max_health: u64,
    is_priority: bool,
}

impl BattleContext {
    fn new(players: &[NormalizedPlayer]) -> Self {
        let mut teams: HashMap<u32, TeamTotals> = HashMap::new();
        let mut classes: HashMap<Species, ClassTotals> = HashMap::new();
        let mut victims = HashMap::new();

        for player in players {
            let Some(results) = &player.server_results else {
                continue;
            };
            let team = teams.entry(player.team_id).or_default();
            if let Some(damage) = results.damage {
                team.damage_dealt += damage as f64;
                team.damage_dealers += 1;
            }

            let Some(max_health) = results.max_health.filter(|hp| *hp > 0) else {
                continue;
            };
            team.players += 1;
            team.potential_damage += results.potential_damage as f64;

            let class = classes.entry(player.ship_class).or_default();
            class.players += 1;
            class.potential_damage += results.potential_damage as f64;

            victims.insert(player.db_id, Victim { max_health, is_priority: is_priority_target(player.ship_class) });
        }

        Self { teams, classes, victims }
    }
}

fn rate_player(
    player: &NormalizedPlayer,
    context: &BattleContext,
    table: &PersonalRatingData,
    torpedoes: &TorpedoDetections,
    bursts: Option<&CriticalBursts>,
) -> Option<BattleImpactResult> {
    let results = player.server_results.as_ref()?;
    let damage = results.damage? as f64;
    let kills = results.kills? as f64;
    results.max_health.filter(|hp| *hp > 0)?;
    let tanking_weights = TankingWeights::of(player.ship_class)?;
    let expected = table
        .get_ship_expected(player.ship_id)
        .filter(|expected| expected.average_damage_dealt > 0.0 && expected.average_frags > 0.0)?;
    let spotting_baseline = spotting_baseline(player.ship_class)?;
    let team = context.teams.get(&player.team_id)?;

    let weighted_damage = damage + priority_damage(results, context) * (priority_weight(true) - 1.0);

    let damage_points = DAMAGE_POINTS * normalize(weighted_damage / expected.average_damage_dealt, 0.4);
    let kill_points = KILL_POINTS * normalize(kills / expected.average_frags, 0.1);

    let team_damage_share = TeamDamageShare::new(damage, team);

    let tanking = TankingRatios::new(results.potential_damage as f64, player.ship_class, context, team);
    let tanking_points = tanking.map(|ratios| ratios.points(tanking_weights));

    let ships_spotted = ships_spotted(results, context);
    let spotting = results.spotting_damage.map(|spotting_damage| {
        SPOTTING_SCALE * spotting_score(spotting_ratio(spotting_damage as f64, ships_spotted, spotting_baseline))
    });

    let objectives = results.objective_points.map(|points| {
        // Hold points accrue to the captors for as long as the team keeps the
        // point, which is mostly the team's doing.
        (points.capture + points.block + points.drop + points.battle_end + 0.25 * points.hold) / 100.0
    });

    let anti_air = anti_air_points(results);

    let torpedo_detection = torpedoes.get(player.entity_id);
    let torpedo_warnings = torpedo_detection.map(TorpedoDetection::points);

    let critical_bursts = bursts.map(|bursts| bursts.get(player.entity_id).to_vec());
    let critical_burst_points = critical_bursts.as_ref().map(|bursts| CRITICAL_BURST_POINTS * bursts.len() as f64);

    let score = FactorScore::new;
    let components = BattleImpactComponents {
        damage: score(BattleImpactFactor::Damage, damage_points),
        kills: score(BattleImpactFactor::Kills, kill_points),
        team_damage_share: team_damage_share.map(|share| score(BattleImpactFactor::TeamDamageShare, share.points())),
        tanking: tanking_points.map(|points| score(BattleImpactFactor::Tanking, points)),
        spotting: spotting.map(|points| score(BattleImpactFactor::Spotting, points)),
        objectives: objectives.map(|points| score(BattleImpactFactor::Objectives, points)),
        anti_air: anti_air.map(|points| score(BattleImpactFactor::AntiAir, points)),
        torpedo_warnings: torpedo_warnings.map(|points| score(BattleImpactFactor::TorpedoWarnings, points)),
        critical_bursts: critical_burst_points.map(|points| score(BattleImpactFactor::CriticalBursts, points)),
    };
    let bi = components.total();

    let inputs = BattleImpactInputs {
        weighted_damage,
        expected_damage: expected.average_damage_dealt,
        kills,
        expected_kills: expected.average_frags,
        team_damage_share,
        potential_damage: results.potential_damage,
        tanking,
        spotting_damage: results.spotting_damage,
        spotting_baseline,
        ships_spotted,
        ships_spotted_baseline: ships_spotted_baseline(spotting_baseline),
        objective_points: results.objective_points,
        planes_killed: results.planes_killed,
        anti_air_damage: results.anti_air_damage,
    };

    Some(BattleImpactResult {
        bi,
        category: PersonalRatingCategory::from_pr(bi),
        version: BattleImpactVersion::CURRENT,
        components,
        inputs,
        torpedo_detection,
        critical_bursts,
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

/// Hit points' worth of ships spotted: each victim's spotting damage over its
/// maximum hit points, summed. A victim missing from the results has no hit
/// points to divide by and adds nothing.
fn ships_spotted(results: &ServerResults, context: &BattleContext) -> f64 {
    results
        .damage_interactions
        .iter()
        .filter_map(|(victim, interaction)| {
            let victim = context.victims.get(victim)?;
            Some(interaction.spotting_damage as f64 / victim.max_health as f64)
        })
        .sum()
}

/// Spotting against the class baseline, half by damage and half by the share
/// of the spotted ships' hit points it amounted to.
fn spotting_ratio(spotting_damage: f64, ships_spotted: f64, baseline: f64) -> f64 {
    0.5 * (spotting_damage / baseline) + 0.5 * (ships_spotted / ships_spotted_baseline(baseline))
}

/// The spotting damage baseline as ships' worth of hit points.
fn ships_spotted_baseline(baseline: f64) -> f64 {
    // Typical hit points of a ship being spotted.
    const TYPICAL_VICTIM_HP: f64 = 65_000.0;

    baseline / TYPICAL_VICTIM_HP
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

/// 1 within 1 km, falling to 0 at 3 km. 0 for a distance that is not a
/// number, which a degenerate projection can produce.
fn threat_weight(closest_km: f32) -> f64 {
    const FULL_KM: f32 = 1.0;
    const NONE_KM: f32 = 3.0;

    if !closest_km.is_finite() {
        return 0.0;
    }

    f64::from(((NONE_KM - closest_km) / (NONE_KM - FULL_KM)).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team_dealing(damage: &[f64]) -> TeamTotals {
        TeamTotals { damage_dealt: damage.iter().sum(), damage_dealers: damage.len() as u32, ..TeamTotals::default() }
    }

    /// The share is of what the player's own team dealt, not of the enemy
    /// hit point pool: a team that dealt 100k leaves a 25k player a quarter.
    #[test]
    fn team_damage_share_divides_by_the_teams_damage() {
        let team = team_dealing(&[25_000.0, 50_000.0, 25_000.0, 0.0]);

        let share = TeamDamageShare::new(25_000.0, &team).expect("the team dealt damage");

        assert_eq!(share.share, Share(0.25));
        assert_eq!(share.even, Share(0.25));
        assert_eq!(share.points(), TEAM_DAMAGE_SHARE_POINTS);
    }

    /// Shares over a team sum to one, so the team's mean points are exactly
    /// what average play earns.
    #[test]
    fn a_teams_mean_team_damage_points_are_the_average() {
        let damage = [12_000.0, 87_000.0, 0.0, 41_500.0, 140_000.0, 3_300.0, 60_000.0];
        let team = team_dealing(&damage);

        let points: Vec<f64> = damage
            .iter()
            .map(|dealt| TeamDamageShare::new(*dealt, &team).expect("the team dealt damage").points())
            .collect();

        let mean = points.iter().sum::<f64>() / points.len() as f64;
        assert!((mean - TEAM_DAMAGE_SHARE_POINTS).abs() < 1e-9, "mean {mean}");
    }

    /// A team that dealt nothing has no shares to hand out: the part is
    /// absent rather than zero or not a number.
    #[test]
    fn a_team_that_dealt_nothing_gives_no_share() {
        assert_eq!(TeamDamageShare::new(0.0, &team_dealing(&[0.0, 0.0, 0.0])), None);
        assert_eq!(TeamDamageShare::new(0.0, &team_dealing(&[])), None);
    }

    fn tanking(class: f64, team: f64, of: Species) -> f64 {
        let ratios = TankingRatios { class: Ratio(class), class_peers: 3, team: Ratio(team) };
        ratios.points(TankingWeights::of(of).expect("a rated class"))
    }

    /// Drawing almost none of the fire class peers drew costs a destroyer,
    /// submarine or carrier nothing, a cruiser a little and a battleship a
    /// clear amount.
    #[test]
    fn low_tanking_costs_by_class() {
        for class in [Species::Destroyer, Species::Submarine, Species::AirCarrier] {
            assert_eq!(tanking(0.1, 0.1, class), 0.0, "{class:?}");
            assert_eq!(tanking(0.0, 0.0, class), 0.0, "{class:?}");
        }

        let cruiser = tanking(0.1, 0.1, Species::Cruiser);
        assert!((-40.0..0.0).contains(&cruiser), "a small capped penalty, got {cruiser}");
        assert_eq!(tanking(0.0, 0.0, Species::Cruiser), -40.0, "the cap");

        let battleship = tanking(0.1, 0.1, Species::Battleship);
        assert!(battleship <= -100.0, "a clear penalty, got {battleship}");
        assert!(battleship < 2.0 * cruiser);
    }

    /// Drawing about what the class draws is neutral for every class.
    #[test]
    fn ordinary_tanking_is_neutral() {
        for class in TANKING_WEIGHTS.map(|(class, _)| class) {
            assert_eq!(tanking(1.0, 1.0, class), 0.0, "{class:?}");
            assert_eq!(tanking(TANKING_PENALTY_BELOW, 0.4, class), 0.0, "{class:?}");
        }
    }

    /// More fire than the class peers and more than an even part of the
    /// team's earns the same reward in any class, up to the cap; either alone
    /// earns nothing.
    #[test]
    fn high_tanking_is_rewarded_only_against_both_references() {
        for class in TANKING_WEIGHTS.map(|(class, _)| class) {
            assert_eq!(tanking(2.0, 1.8, class), 80.0, "{class:?}");
            assert_eq!(tanking(6.0, 5.0, class), TANKING_MAX_REWARD, "{class:?}");
            assert_eq!(tanking(2.0, 0.7, class), 0.0, "above the class alone, {class:?}");
            assert_eq!(tanking(0.7, 2.0, class), 0.0, "above the team alone, {class:?}");
        }
    }

    /// The class reference leaves the player out, and spans both teams.
    #[test]
    fn tanking_ratios_compare_against_the_other_ships_of_the_class() {
        let context = BattleContext {
            teams: HashMap::new(),
            classes: HashMap::from([
                (Species::Battleship, ClassTotals { players: 3, potential_damage: 3_000_000.0 }),
                (Species::AirCarrier, ClassTotals { players: 1, potential_damage: 200_000.0 }),
            ]),
            victims: HashMap::new(),
        };
        let team = TeamTotals { players: 12, potential_damage: 6_000_000.0, ..TeamTotals::default() };

        let ratios = TankingRatios::new(1_500_000.0, Species::Battleship, &context, &team).expect("two peers");
        assert_eq!(ratios, TankingRatios { class: Ratio(2.0), class_peers: 2, team: Ratio(3.0) });

        assert_eq!(TankingRatios::new(200_000.0, Species::AirCarrier, &context, &team), None, "no peer");
        assert_eq!(TankingRatios::new(3_000_000.0, Species::Battleship, &context, &team), None, "peers drew none");
    }

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

    const TEAMMATE: u32 = 99;

    fn hit(victim: u32, at: f32, amount: f32) -> DamageEvent {
        DamageEvent { amount, victim: EntityId::from(victim), clock: wows_replays::types::GameClock(at) }
    }

    /// Every victim an enemy with 10,000 hit points, except `TEAMMATE`.
    fn bursts_in(events: &[DamageEvent]) -> Vec<(u64, f64, f32)> {
        let victim = |id: EntityId| {
            (id.raw() != TEAMMATE).then_some(BurstVictim {
                max_health: 10_000.0,
                ship: GameParamId::from(u64::from(id.raw())),
                ship_name: Some("Montana"),
            })
        };

        find_bursts(events, victim)
            .into_iter()
            .map(|burst| (burst.victim_ship.raw(), burst.share.0, burst.duration.value()))
            .collect()
    }

    /// Half the victim's hit points counts, a hair under does not.
    #[test]
    fn a_burst_needs_half_the_victims_hit_points() {
        assert_eq!(bursts_in(&[hit(1, 100.0, 3_000.0), hit(1, 104.0, 2_000.0)]), vec![(1, 0.5, 4.0)]);
        assert_eq!(bursts_in(&[hit(1, 100.0, 3_000.0), hit(1, 104.0, 1_990.0)]), vec![]);
    }

    /// Hits exactly the window apart still form one burst; a little further
    /// apart they do not.
    #[test]
    fn a_burst_lands_within_the_window() {
        let edge = CRITICAL_BURST_WINDOW_SECS;
        assert_eq!(bursts_in(&[hit(1, 0.0, 2_500.0), hit(1, edge, 2_500.0)]), vec![(1, 0.5, edge)]);
        assert_eq!(bursts_in(&[hit(1, 0.0, 2_500.0), hit(1, edge + 0.5, 2_500.0)]), vec![]);
    }

    /// Of every window, the heaviest is the one measured, once per victim.
    #[test]
    fn the_heaviest_window_is_measured_once_per_victim() {
        let events = [
            hit(1, 0.0, 2_000.0),
            hit(1, 5.0, 2_000.0),
            hit(1, 12.0, 2_000.0),
            hit(1, 14.0, 2_000.0),
            hit(2, 30.0, 9_000.0),
            hit(2, 31.0, 1_000.0),
            hit(2, 90.0, 8_000.0),
        ];
        assert_eq!(bursts_in(&events), vec![(2, 1.0, 1.0), (1, 0.6, 9.0)]);
    }

    /// Damage to a ship of the aggressor's own team is never a burst.
    #[test]
    fn damage_to_a_teammate_is_not_a_burst() {
        assert_eq!(bursts_in(&[hit(TEAMMATE, 0.0, 9_000.0)]), vec![]);
    }

    /// The count is whole torpedoes; the weighting only scales the points,
    /// which never fall below zero whatever the projection produced.
    #[test]
    fn torpedo_warnings_count_whole_torpedoes_and_never_subtract() {
        let mut detection = TorpedoDetection::default();
        for closest in [Some(0.4), Some(1.0), Some(2.0), Some(2.9), Some(3.0), Some(8.0), None] {
            detection.record(closest);
        }
        assert_eq!(detection.detected, 7);
        assert_eq!(detection.threatening, 4, "the four within 3 km");
        assert!((detection.points() - 8.0 * (1.0 + 1.0 + 0.5 + 0.05)).abs() < 1e-6);

        let mut degenerate = TorpedoDetection::default();
        for closest in [Some(f32::NAN), Some(f32::INFINITY), Some(f32::NEG_INFINITY), Some(-5.0), Some(-0.0)] {
            degenerate.record(closest);
            assert!(degenerate.points() >= 0.0, "{degenerate:?}");
        }
        assert_eq!(degenerate.threatening, 2, "only the finite distances, clamped to full weight");
        assert_eq!(TorpedoDetection::default().points(), 0.0);
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
