//! Generated session games, for looking over the Stats panels without recording any.
//!
//! The games are made from one build's ships and achievements so the panels resolve their art
//! and names, and are never stored.

use std::collections::BTreeMap;

use jiff::Span;
use jiff::civil::DateTime;
use wows_replays::types::GameParamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::translations::translate_achievement_description;
use wowsunpack::game_params::translations::translate_achievement_name;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::game_params::types::Species;

use crate::personal_rating::PersonalRatingData;
use crate::stats::PerGameStat;
use crate::stats::SerializableAchievement;
use crate::stats::sortable_game_time;

/// Ships kept for each class and tier.
const SHIPS_PER_SLOT: usize = 2;
/// Achievements the games draw from.
const ACHIEVEMENT_POOL: usize = 16;
/// Ships played far more often than the rest, so the Ships panel has a spread of counts.
const FAVORITES: usize = 8;
const DAYS: i64 = 56;
/// Above every id a build uses, so a placeholder ship never resolves to a real one.
const PLACEHOLDER_IDS: u64 = 1 << 40;
const PLAYER_ID: i64 = 1;

const CLASSES: [Species; 5] =
    [Species::Destroyer, Species::Cruiser, Species::Battleship, Species::AirCarrier, Species::Submarine];

/// Match groups with their share of the games, out of 100.
const MODES: [(&str, u32); 7] =
    [("pvp", 66), ("ranked", 12), ("cooperative", 8), ("clan", 6), ("brawl", 4), ("event", 2), ("pve", 2)];

/// One ship the games can be played in.
#[derive(Clone, Debug, PartialEq)]
struct SampleShip {
    id: GameParamId,
    name: String,
    class: Species,
    tier: u32,
}

/// The ships and achievements one build offers the generated games.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SampleCatalog {
    ships: Vec<SampleShip>,
    achievements: Vec<SerializableAchievement>,
}

impl SampleCatalog {
    /// Up to [`SHIPS_PER_SLOT`] ships of each playable class and tier, and the build's
    /// translated single-battle achievements.
    pub fn from_build(provider: &GameMetadataProvider) -> Self {
        let mut slots: BTreeMap<(usize, u32), Vec<SampleShip>> = BTreeMap::new();
        let mut achievements = Vec::new();
        for param in provider.params() {
            match param.data() {
                ParamData::Vehicle(vehicle) => {
                    let Some(class) = param.species().and_then(|species| species.known()).copied() else { continue };
                    let Some(class_at) = CLASSES.iter().position(|known| *known == class) else { continue };
                    // Tech tree and premium groups only: test, rental, bot and old copies are not
                    // ships a player sails.
                    let owned = matches!(
                        vehicle.group(),
                        "start"
                            | "upgradeable"
                            | "upgradeableExclusive"
                            | "upgradeableUltimate"
                            | "superShip"
                            | "premium"
                            | "special"
                            | "specialUnsellable"
                            | "ultimate"
                            | "earlyAccess"
                    );
                    if !owned || !(1..=11).contains(&vehicle.level()) {
                        continue;
                    }
                    let Some(name) = provider.localized_name_from_param(param) else { continue };
                    if name.ends_with("(old)") {
                        continue;
                    }
                    let slot = slots.entry((class_at, vehicle.level())).or_default();
                    slot.push(SampleShip { id: param.id(), name, class, tier: vehicle.level() });
                }
                ParamData::Achievement(achievement) if !achievement.is_group() => {
                    let Some(ui_name) = achievement.ui_name() else { continue };
                    let Some(display_name) = translate_achievement_name(ui_name, provider) else { continue };
                    achievements.push(SerializableAchievement {
                        game_param_id: param.id(),
                        display_name,
                        description: translate_achievement_description(ui_name, provider).unwrap_or_default(),
                        icon_key: ui_name.to_owned(),
                        count: 1,
                    });
                }
                _ => {}
            }
        }

        let ships = slots
            .into_values()
            .flat_map(|mut slot| {
                slot.sort_by_key(|ship| ship.id.raw());
                slot.into_iter().take(SHIPS_PER_SLOT)
            })
            .collect();
        achievements.sort_by_key(|achievement| achievement.game_param_id.raw());

        Self { ships, achievements }
    }

    pub fn is_empty(&self) -> bool {
        self.ships.is_empty()
    }

    /// One made-up ship per class and tier and a set of made-up achievements, for when no build
    /// is loaded. Their ids name nothing, so the panels draw them without art.
    pub fn placeholder() -> Self {
        let mut ships = Vec::new();
        for (class_at, class) in CLASSES.iter().enumerate() {
            for tier in 1..=11 {
                ships.push(SampleShip {
                    id: GameParamId::from(PLACEHOLDER_IDS + (class_at as u64 + 1) * 100 + u64::from(tier)),
                    name: format!("{class:?} {tier}"),
                    class: *class,
                    tier,
                });
            }
        }
        let achievements = (0..ACHIEVEMENT_POOL as u64)
            .map(|at| SerializableAchievement {
                game_param_id: GameParamId::from(PLACEHOLDER_IDS + 9_000 + at),
                display_name: format!("Achievement {at}"),
                description: String::new(),
                icon_key: format!("SAMPLE_{at}"),
                count: 1,
            })
            .collect();

        Self { ships, achievements }
    }
}

/// `count` games ending at `end`, oldest first, spread over eight weeks of evening sessions.
///
/// Damage, frags and wins follow the ship's expected values where `rating` has them, scaled by
/// a per-game form so the games span every rating band. The same `seed` gives the same games.
pub fn sample_games(
    catalog: &SampleCatalog,
    rating: Option<&PersonalRatingData>,
    end: DateTime,
    count: usize,
    seed: u64,
) -> Vec<PerGameStat> {
    if catalog.ships.is_empty() {
        return Vec::new();
    }
    let mut rng = SplitMix(seed);

    let mut pool = catalog.achievements.clone();
    rng.shuffle(&mut pool);
    pool.truncate(ACHIEVEMENT_POOL);

    let mut favorites = catalog.ships.clone();
    rng.shuffle(&mut favorites);
    favorites.truncate(FAVORITES);

    let start = end.checked_sub(Span::new().days(DAYS)).unwrap_or(end);
    let mut games = Vec::with_capacity(count);
    let mut day = 0;
    while games.len() < count {
        // Most days have one evening session; a gap day keeps the date axis honest.
        if rng.chance(0.25) {
            day = (day + 1) % DAYS;
            continue;
        }
        let session = (3 + rng.below(12) as usize).min(count - games.len());
        let hour = 17 + rng.below(5) as i64;
        let mut at =
            start.checked_add(Span::new().days(day).hours(hour).minutes(rng.below(60) as i64)).unwrap_or(start);
        let in_division = rng.chance(0.3);
        for _ in 0..session {
            let ship = if rng.chance(0.55) {
                &favorites[rng.below(favorites.len() as u64) as usize]
            } else {
                &catalog.ships[rng.below(catalog.ships.len() as u64) as usize]
            };
            // Form improves across the weeks, so a running average has a slope to show.
            let progress = day as f64 / DAYS as f64;
            let form = (rng.normal() * 0.45).exp() * (0.85 + 0.3 * progress);
            games.push(sample_game(&mut rng, ship, rating, &pool, form, in_division, at));
            at = at.checked_add(Span::new().minutes(18 + rng.below(10) as i64)).unwrap_or(at);
        }
        day = (day + 1) % DAYS;
    }

    games.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
    games
}

fn sample_game(
    rng: &mut SplitMix,
    ship: &SampleShip,
    rating: Option<&PersonalRatingData>,
    achievements: &[SerializableAchievement],
    form: f64,
    is_div: bool,
    at: DateTime,
) -> PerGameStat {
    let expected = rating.and_then(|rating| rating.get_ship_expected(ship.id));
    let (expected_damage, expected_frags, expected_win_rate) = match expected {
        Some(values) => (values.average_damage_dealt, values.average_frags, values.win_rate / 100.0),
        None => (f64::from(ship.tier) * 4_500.0 * class_damage(ship.class), 0.7, 0.5),
    };

    let roll = rng.unit();
    let win_chance = (expected_win_rate + 0.3 * (form - 1.0)).clamp(0.15, 0.85);
    let is_draw = roll < 0.02;
    let is_win = !is_draw && roll < 0.02 + win_chance * 0.98;
    let is_loss = !is_draw && !is_win;

    let damage = if rng.chance(0.02) { 0.0 } else { expected_damage * form * (rng.normal() * 0.35).exp() };
    let frag_mean = expected_frags * form * if is_win { 1.25 } else { 0.75 };
    let frags = rng.poisson(frag_mean).min(9);
    let spotting_share = if matches!(ship.class, Species::Destroyer | Species::AirCarrier) { 0.9 } else { 0.25 };
    let spotting_damage = damage * spotting_share * rng.unit() * 1.6;
    let base_xp = (damage / 40.0) as i64 + frags * 120 + if is_win { 450 } else { 0 };

    let earned = if achievements.is_empty() { 0 } else { rng.poisson(0.6 * form).min(3) as usize };
    let mut earned_achievements: Vec<SerializableAchievement> = Vec::with_capacity(earned);
    for _ in 0..earned {
        let picked = &achievements[rng.below(achievements.len() as u64) as usize];
        match earned_achievements.iter_mut().find(|held| held.game_param_id == picked.game_param_id) {
            Some(held) => held.count += 1,
            None => earned_achievements.push(picked.clone()),
        }
    }

    let game_time = at.strftime("%d.%m.%Y %H:%M:%S").to_string();
    PerGameStat {
        ship_name: ship.name.clone(),
        ship_id: ship.id,
        sort_key: sortable_game_time(&game_time),
        game_time,
        player_id: PLAYER_ID,
        damage: damage as u64,
        spotting_damage: spotting_damage as u64,
        frags,
        raw_xp: base_xp * 3 / 2,
        base_xp,
        is_win,
        is_loss,
        is_draw,
        is_div,
        match_group: pick_mode(rng).to_owned(),
        achievements: earned_achievements,
    }
}

/// Damage per tier a class deals relative to a cruiser, for a ship the rating table lacks.
fn class_damage(class: Species) -> f64 {
    match class {
        Species::Battleship => 1.35,
        Species::AirCarrier => 1.4,
        Species::Destroyer => 0.7,
        Species::Submarine => 0.9,
        _ => 1.0,
    }
}

fn pick_mode(rng: &mut SplitMix) -> &'static str {
    let mut roll = rng.below(100) as u32;
    for (mode, share) in MODES {
        if roll < share {
            return mode;
        }
        roll -= share;
    }
    MODES[0].0
}

/// SplitMix64: small, seedable and good enough for made-up games.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }

    /// Uniform in [0, bound). `bound` must be nonzero.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }

    /// Standard normal, by Box-Muller.
    fn normal(&mut self) -> f64 {
        let u1 = self.unit().max(f64::MIN_POSITIVE);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }

    /// Poisson with mean `mean`, by Knuth's product method.
    fn poisson(&mut self, mean: f64) -> i64 {
        let limit = (-mean.max(0.0)).exp();
        let mut product = self.unit();
        let mut count = 0;
        while product > limit {
            count += 1;
            product *= self.unit();
        }
        count
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for at in (1..items.len()).rev() {
            let other = self.below(at as u64 + 1) as usize;
            items.swap(at, other);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::stats::all_match_groups;

    fn catalog() -> SampleCatalog {
        SampleCatalog::placeholder()
    }

    fn end() -> DateTime {
        jiff::civil::date(2026, 11, 12).at(23, 0, 0, 0)
    }

    #[test]
    fn games_cover_the_range_the_panels_show() {
        let games = sample_games(&catalog(), None, end(), 600, 7);

        assert_eq!(games.len(), 600);
        assert!(games.windows(2).all(|pair| pair[0].sort_key <= pair[1].sort_key), "oldest first");
        assert!(games.iter().any(|game| game.is_win) && games.iter().any(|game| game.is_loss));
        assert!(games.iter().any(|game| game.is_div) && games.iter().any(|game| !game.is_div));
        assert!(all_match_groups(&games).len() >= 5);
        let ships: std::collections::HashSet<_> = games.iter().map(|game| game.ship_id).collect();
        assert!(ships.len() >= 30);
        let days: std::collections::HashSet<_> = games.iter().map(|game| &game.sort_key[..10]).collect();
        assert!(days.len() >= 21);
        assert!(games.iter().any(|game| !game.achievements.is_empty()));
        assert!(games.iter().map(|game| game.frags).max() >= Some(3));
    }

    #[test]
    fn the_same_seed_gives_the_same_games() {
        assert_eq!(sample_games(&catalog(), None, end(), 50, 3), sample_games(&catalog(), None, end(), 50, 3));
    }

    #[test]
    fn an_empty_catalog_gives_no_games() {
        assert!(sample_games(&SampleCatalog::default(), None, end(), 50, 3).is_empty());
    }
}
