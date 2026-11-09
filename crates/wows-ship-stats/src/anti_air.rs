//! AA rating: the damage a ship's AA deals to one plane flying straight in
//! from the edge of the farthest aura to the ship, a composite to compare
//! ships of one class and tier. It is a judgment, not a game value: players
//! dodge flak, fly in from the side and leave before reaching the ship.
//!
//! - Continuous damage: for every aura, the time the plane spends inside it,
//!   `(maxDistance - minDistance) / plane speed`, times its damage per second
//!   and hit chance (`calculateAverageAuraDPS`, `ModifiersApply.py:755`).
//! - Flak: the bursts the client spawns while the plane crosses the flak zone,
//!   `bursts * hitChance / BUBBLE_UPDATE_INTERVAL` per second
//!   (`m979f9c73/AirDefense.py:1068`), times the burst damage and
//!   [`FLAK_HIT_FACTOR`].
//!
//! The plane flies at [`PLANE_SPEED`] knots, about the median cruise speed of
//! tier X torpedo bombers (145 kn) and dive bombers (166 kn). The client moves
//! planes at `speed * BW_KNOTS_TO_MPS` world units per second
//! (`AirplaneUtils.py:442`, `m63fc2621.py:47`) and places auras with
//! `fromBallistic`, so in ballistic meters a plane covers
//! `speed * BW_KNOTS_TO_MPS * BW_TO_BALLISTIC`, 2.69 m/s per knot.

use serde::Serialize;
use wows_core::units::DamagePerSecond;
use wowsunpack::game_params::ttx::anti_air::AntiAirDefense;
use wowsunpack::game_params::ttx::anti_air::AuraBand;
use wowsunpack::game_params::ttx::anti_air::bubble_update_interval;
use wowsunpack::game_params::ttx::constants::BW_KNOTS_TO_MPS;
use wowsunpack::game_params::ttx::constants::BW_TO_BALLISTIC;
use wowsunpack::game_params::ttx::model::Hp;

/// Speed of the plane the rating assumes, knots.
pub const PLANE_SPEED: f32 = 150.0;

/// Share of the spawned flak bursts counted as hitting the plane. The
/// client's own port AA rating weighs flak by 0.1 twice
/// (`getIntegralParamsAA`, `mab132439/m9ab28003.py:66-96`); we take it once,
/// for a plane that holds its course without dodging.
pub const FLAK_HIT_FACTOR: f32 = 0.1;

/// The AA rating and its parts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AaRating {
    /// Continuous damage of each band with auras, short to long.
    pub bands: Vec<BandShare>,

    /// Expected flak damage. Zero without flak.
    pub flak: Hp,

    /// Continuous damage of every band plus flak.
    pub total: Hp,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BandShare {
    pub band: AuraBand,

    /// Seconds the plane spends inside the band's widest aura.
    pub seconds_inside: f32,

    /// Damage per second times hit chance, every aura of the band.
    pub expected_dps: DamagePerSecond,
    pub damage: Hp,
}

impl AaRating {
    /// The rating of `defense`.
    pub fn from_defense(defense: &AntiAirDefense) -> Self {
        let speed = PLANE_SPEED * BW_KNOTS_TO_MPS * BW_TO_BALLISTIC;
        let crossing = |min_km: f32, max_km: f32| ((max_km - min_km) * 1000.0).max(0.0) / speed;

        let bands: Vec<BandShare> = defense
            .bands
            .iter()
            .map(|band| {
                let damage: f32 = band
                    .auras
                    .iter()
                    .map(|aura| aura.expected_dps().value() * crossing(aura.min_range.value(), aura.max_range.value()))
                    .sum();
                let expected_dps: f32 = band.auras.iter().map(|aura| aura.expected_dps().value()).sum();
                let seconds_inside = band
                    .auras
                    .iter()
                    .map(|aura| crossing(aura.min_range.value(), aura.max_range.value()))
                    .fold(0.0, f32::max);

                BandShare {
                    band: band.band,
                    seconds_inside,
                    expected_dps: DamagePerSecond::new(expected_dps),
                    damage: Hp::from(damage),
                }
            })
            .collect();

        let interval = bubble_update_interval().value();
        let flak: f32 = defense
            .flak
            .iter()
            .flat_map(|flak| &flak.auras)
            .map(|aura| {
                let per_second = aura.bursts.0 as f32 * aura.hit_chance.0 / interval;
                per_second
                    * crossing(aura.min_range.value(), aura.max_range.value())
                    * aura.damage.value()
                    * FLAK_HIT_FACTOR
            })
            .sum();

        let continuous: f32 = bands.iter().map(|band| band.damage.value()).sum();

        Self { bands, flak: Hp::from(flak), total: Hp::from(continuous + flak) }
    }
}
