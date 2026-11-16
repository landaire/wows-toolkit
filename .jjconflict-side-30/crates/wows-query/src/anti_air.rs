//! Anti-aircraft defense on the builds players run: each aura band's damage,
//! flak, the priority sector and the AA rating, with Defensive AA Fire active
//! as an alternative. The damage model and the rating are documented in
//! `wowsunpack::game_params::ttx::anti_air` and
//! [`wows_ship_stats::anti_air`].

use serde::Serialize;
use wows_core::units::DamagePerSecond;
use wows_core::units::Km;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wows_ship_stats::AaRating;
use wows_ship_stats::anti_air::FLAK_HIT_FACTOR;
use wows_ship_stats::anti_air::PLANE_SPEED;
use wowsunpack::game_params::ttx::anti_air::AntiAirDefense;
use wowsunpack::game_params::ttx::anti_air::BandDefense;
use wowsunpack::game_params::ttx::anti_air::DefensiveAaFire;
use wowsunpack::game_params::ttx::anti_air::FlakBurstCount;
use wowsunpack::game_params::ttx::anti_air::anti_air;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::types::GameParamProvider;

use crate::builds::Better;
use crate::builds::BuildAlternative;
use crate::builds::BuildStat;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::builds::StatUnit;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::ships::Ship;

/// AA defense on the best-fit build of each stat.
#[derive(Debug, Clone, Serialize)]
pub struct AntiAircraftBuilds {
    /// One entry per aura band, short to long range.
    pub auras: Vec<AuraBuilds>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub flak: Option<FlakBuilds>,

    /// Stock modules, no skills or upgrades.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority_sector: Option<PrioritySectorStats>,

    pub rating: RatingBuilds,
}

/// Every aura of one band.
#[derive(Debug, Clone, Serialize)]
pub struct AuraBuilds {
    /// e.g. "long-range".
    pub band: &'static str,

    /// Reach of the farthest aura, stock modules.
    pub range: Km,

    /// Mean hit chance of the band's auras, as the port card shows it.
    pub hit_chance: Percent,

    /// Gun mounts feeding the band, stock modules, e.g. "6 x 127 mm twin".
    pub mounts: Vec<String>,

    /// Continuous damage per second before hit chance, every aura of the band.
    pub damage_per_second: BuildStat<DamagePerSecond>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlakBuilds {
    /// Distances the bursts appear at, stock modules.
    pub zone: String,

    pub hit_chance: Percent,

    /// Bursts in one salvo.
    pub bursts: BuildStat<FlakBurstCount>,

    pub damage_per_burst: BuildStat<Hp>,
}

/// The priority sector the player picks with the AA sector key.
#[derive(Debug, Clone, Serialize)]
pub struct PrioritySectorStats {
    /// Damage dealt at once on activation, percent of the squadron's health.
    pub instant_damage: Percent,

    /// Peak damage multiplier in the chosen sector.
    pub damage_multiplier: f32,

    pub duration: Seconds,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown: Option<Seconds>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RatingBuilds {
    pub method: String,

    /// Damage to one plane flying straight in from the farthest aura.
    pub rating: BuildStat<Hp>,

    /// Parts of the best-fit rating.
    pub breakdown: Vec<String>,
}

impl StatAmount for DamagePerSecond {
    fn amount(self) -> f32 {
        self.value()
    }
}

impl StatUnit for DamagePerSecond {
    const UNIT: &'static str = "hp/s";
}

impl StatAmount for Hp {
    fn amount(self) -> f32 {
        self.value()
    }
}

impl StatUnit for Hp {
    const UNIT: &'static str = "hp";
}

impl StatAmount for FlakBurstCount {
    fn amount(self) -> f32 {
        self.0 as f32
    }
}

impl StatUnit for FlakBurstCount {
    const UNIT: &'static str = "bursts";
}

impl GameData {
    /// AA defense of `planner`'s ship; `None` without an aura.
    pub(crate) fn anti_aircraft_builds(&self, planner: &Planner<'_>) -> Result<Option<AntiAircraftBuilds>, QueryError> {
        let ship = planner.ship();
        let defense = |outcome: &Outcome, barrage: Option<DefensiveAaFire>| {
            anti_air(&ship.param, planner.selection(outcome.selection), &outcome.bundle, barrage)
        };
        let Some(stock) = defense(&*planner.stock()?, None) else {
            return Ok(None);
        };
        let barrages = self.defensive_aa_fire(ship);

        let mut auras = Vec::new();
        for band in &stock.bands {
            let extract = |barrage: Option<DefensiveAaFire>| {
                move |outcome: &Outcome| {
                    defense(outcome, barrage)?
                        .band(band.band)
                        .map(|band| DamagePerSecond::new(band.dps().value().round()))
                }
            };
            let label = format!("{} AA damage per second", band.band.label());
            let Some(damage_per_second) =
                with_barrage(planner, &label, Better::Higher, &extract(None), &barrages, |barrage| {
                    extract(Some(barrage))
                })?
            else {
                continue;
            };

            auras.push(AuraBuilds {
                band: band.band.label(),
                range: Km::new(round(band.range().value())),
                hit_chance: percent(band.hit_chance().0),
                mounts: mount_lines(band),
                damage_per_second,
            });
        }

        let flak = match &stock.flak {
            Some(flak) => {
                let bursts = |outcome: &Outcome| Some(defense(outcome, None)?.flak?.bursts());
                let damage = |barrage: Option<DefensiveAaFire>| {
                    move |outcome: &Outcome| Some(Hp::from(defense(outcome, barrage)?.flak?.damage().value().round()))
                };
                let bursts =
                    planner.plan(&StatPlan::new("flak bursts per salvo".to_owned(), Better::Higher, &bursts))?;
                let damage =
                    with_barrage(planner, "flak burst damage", Better::Higher, &damage(None), &barrages, |barrage| {
                        damage(Some(barrage))
                    })?;

                bursts.zip(damage).map(|(bursts, damage_per_burst)| FlakBuilds {
                    zone: format!("{} to {} km", round(flak.min_range().value()), round(flak.max_range().value())),
                    hit_chance: percent(flak.hit_chance().0),
                    bursts,
                    damage_per_burst,
                })
            }
            None => None,
        };

        let priority_sector = stock.priority_sector.map(|sector| PrioritySectorStats {
            instant_damage: percent(sector.instant_damage),
            damage_multiplier: round(sector.strength),
            duration: Seconds::new(round(sector.duration.value())),
            cooldown: sector.cooldown.map(|cooldown| Seconds::new(round(cooldown.value()))),
        });

        let rating = |barrage: Option<DefensiveAaFire>| {
            move |outcome: &Outcome| {
                Some(Hp::from(AaRating::from_defense(&defense(outcome, barrage)?).total.value().round()))
            }
        };
        let label = "AA rating, damage to one plane flying straight in";
        let Some(rating_stat) =
            with_barrage(planner, label, Better::Higher, &rating(None), &barrages, |barrage| rating(Some(barrage)))?
        else {
            return Ok(None);
        };
        let Some(best) = planner
            .best_fit(&StatPlan::new(label.to_owned(), Better::Higher, &rating(None)))?
            .and_then(|fit| defense(&fit.outcome, None))
        else {
            return Ok(None);
        };

        Ok(Some(AntiAircraftBuilds {
            auras,
            flak,
            priority_sector,
            rating: RatingBuilds { method: rating_method(), rating: rating_stat, breakdown: breakdown(&best) },
        }))
    }

    /// Defensive AA Fire variants of `ship`, by English name, without
    /// repeats.
    fn defensive_aa_fire(&self, ship: &Ship) -> Vec<(String, DefensiveAaFire)> {
        let slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();
        let mut found: Vec<(String, DefensiveAaFire)> = Vec::new();

        for (ability, variant) in slots.iter().flatten() {
            let Some(param) = self.provider().game_param_by_name(ability) else {
                continue;
            };
            let Some(category) = param.ability().and_then(|ability| ability.get_category(variant)) else {
                continue;
            };
            let Some(barrage) = DefensiveAaFire::from_category(category) else {
                continue;
            };

            let name = self.consumable_name(ability, category);
            if !found.iter().any(|(known, known_barrage)| *known == name && *known_barrage == barrage) {
                found.push((name, barrage));
            }
        }

        found
    }
}

/// The builds of a stat, with each Defensive AA Fire variant active as an
/// alternative on top of the best-fit build.
fn with_barrage<V, F>(
    planner: &Planner<'_>,
    label: &str,
    better: Better,
    extract: &dyn Fn(&Outcome) -> Option<V>,
    barrages: &[(String, DefensiveAaFire)],
    active: impl Fn(DefensiveAaFire) -> F,
) -> Result<Option<BuildStat<V>>, QueryError>
where
    V: StatUnit,
    F: Fn(&Outcome) -> Option<V>,
{
    let Some(mut stat) = planner.plan(&StatPlan::new(label.to_owned(), better, extract))? else {
        return Ok(None);
    };

    for (name, barrage) in barrages {
        let extract = active(*barrage);
        let plan = StatPlan::new(format!("{label} with {name} active"), better, &extract);
        let Some(with_consumable) = planner.best(&plan)? else {
            continue;
        };
        if !better.improves(with_consumable.best.value.amount(), stat.builds.best.value.amount()) {
            continue;
        }

        let mut assumed = with_consumable.best.assumed;
        assumed.push(name.clone());
        stat.builds.alternatives.push(BuildAlternative {
            label: format!("with {name} active"),
            value: with_consumable.best.value,
            assumed,
            tier_fillers: with_consumable.best.tier_fillers,
            note: None,
        });
    }

    Ok(Some(stat))
}

/// A fraction 0 to 1 as a percent, two decimals.
fn percent(fraction: f32) -> Percent {
    Percent::new(round(fraction * 100.0))
}

/// The band's mounts by caliber and barrels, e.g. "12 x 20 mm single".
fn mount_lines(band: &BandDefense) -> Vec<String> {
    let mut lines: Vec<(Option<f32>, Option<u32>, u32)> = Vec::new();
    for group in &band.mounts {
        let caliber = group.caliber.map(|caliber| caliber.value().round());
        match lines.iter_mut().find(|(known, barrels, _)| *known == caliber && *barrels == group.barrels_per_mount) {
            Some((_, _, count)) => *count += group.count.0,
            None => lines.push((caliber, group.barrels_per_mount, group.count.0)),
        }
    }

    lines
        .into_iter()
        .map(|(caliber, barrels, count)| {
            let caliber = caliber.map_or_else(|| "unknown caliber".to_owned(), |caliber| format!("{caliber} mm"));
            let barrels = match barrels {
                Some(1) => " single".to_owned(),
                Some(2) => " twin".to_owned(),
                Some(3) => " triple".to_owned(),
                Some(4) => " quadruple".to_owned(),
                Some(barrels) => format!(" {barrels}-barrel"),
                None => String::new(),
            };
            format!("{count} x {caliber}{barrels}")
        })
        .collect()
}

fn rating_method() -> String {
    format!(
        "Damage the AA deals to one plane flying straight in at {PLANE_SPEED} knots (2.69 m/s per knot on the game map) from the edge of the farthest aura to the ship: for each aura, seconds inside it times damage per second times hit chance, plus the flak bursts the client spawns on the way (bursts x hit chance every 2.02 s) times burst damage times {FLAK_HIT_FACTOR}, the share counted as hitting. A judgment to compare ships of one class and tier: players dodge flak and do not fly straight in. The priority sector is left out, and so is Defensive AA Fire except in the alternatives that name it."
    )
}

fn breakdown(defense: &AntiAirDefense) -> Vec<String> {
    let rating = AaRating::from_defense(defense);
    let mut lines: Vec<String> = rating
        .bands
        .iter()
        .map(|band| {
            format!(
                "{}: up to {} s inside, {} hp/s after hit chance, {} hp",
                band.band.label(),
                round(band.seconds_inside),
                round(band.expected_dps.value()),
                band.damage.value().round()
            )
        })
        .collect();
    if let Some(flak) = &defense.flak {
        lines.push(format!(
            "flak: {} bursts of {} hp, {} hit chance, {} hp",
            flak.bursts().0,
            flak.damage().value().round(),
            round(flak.hit_chance().0),
            rating.flak.value().round()
        ));
    }
    lines.push(format!("total: {} hp", rating.total.value().round()));

    lines
}
