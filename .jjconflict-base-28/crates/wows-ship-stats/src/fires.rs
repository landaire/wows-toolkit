//! Theoretical fire starts per minute of each armament: projectiles released
//! per minute times the chance that one starts a fire.
//!
//! Every projectile is taken to hit a target that can burn, with no limit on
//! the fires the target can carry and none of the target's fire resistance.
//! Against one target only the gun mounts that bear on it fire
//! ([`crate::bearing`]); the sum over every mount is a ceiling no single
//! target sees.
//! Guns fire at their reload. A squadron with several attacks per sortie
//! attacks once per `attackCooldown`, the shortest time between two of its
//! attacks, while it has attacks left; flight to the target, returns and
//! losses are left out. A squadron that attacks once per sortie, as hybrid
//! ships launch them, attacks again when the hangar has restored the planes.
//! An air strike releases its whole squadron once per consumable reload.

use serde::Serialize;
use wows_core::units::FiresPerMinute;
use wows_core::units::Percent;
use wows_core::units::ProjectilesPerMinute;
use wows_core::units::Seconds;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::factories::SMALL_PROJECTILE_MAX_DIAMETER_M;
use wowsunpack::game_params::ttx::model::ShellStats;
use wowsunpack::game_params::ttx::model::ShipStats;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::weapon_tables::BurnSpecies;
use wowsunpack::game_params::ttx::weapon_tables::calculate_burn_chance;
use wowsunpack::game_params::ttx::weapon_tables::is_small_projectile;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;

use crate::bearing::Bearing;
use crate::bearing::MountCoverage;
use crate::bearing::PerBearing;
use crate::mounts::aiming_mounts;

/// What releases the projectiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FireArmament {
    MainBattery,
    Secondary,

    /// A carrier or hybrid squadron the player flies; one at a time.
    Squadron,

    /// An air strike consumable.
    AirStrike,
}

impl FireArmament {
    pub fn label(self) -> &'static str {
        match self {
            Self::MainBattery => "main battery",
            Self::Secondary => "secondary battery",
            Self::Squadron => "squadron",
            Self::AirStrike => "air strike",
        }
    }
}

/// One armament's fire output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FireSource {
    pub armament: FireArmament,

    /// Ammo type of the main battery, secondary gun group label, or the
    /// aircraft's GameParams name.
    pub qualifier: String,

    /// Carried by a researched plane module rather than the stock one.
    pub module_upgrade: bool,

    pub projectiles_per_minute: ProjectilesPerMinute,

    /// Chance one projectile starts a fire, with the build applied.
    pub fire_chance: Percent,

    /// Every mount firing, or the aircraft attacking.
    pub fires_per_minute: FiresPerMinute,

    /// Each gun mount's share of `fires_per_minute`. Empty for aircraft.
    pub mounts: Vec<MountFires>,
}

impl FireSource {
    /// Fires against one target at each bearing: guns count only the mounts
    /// that bear on it; aircraft reach every bearing.
    pub fn by_bearing(&self, coverage: &MountCoverage) -> PerBearing<FiresPerMinute> {
        match self.armament {
            FireArmament::Squadron | FireArmament::AirStrike => PerBearing::uniform(self.fires_per_minute),
            FireArmament::MainBattery | FireArmament::Secondary => {
                self.mounts.iter().fold(PerBearing::uniform(FiresPerMinute::new(0.0)), |sum, mount| {
                    sum + PerBearing::mount(coverage, &mount.hardpoint, mount.fires)
                })
            }
        }
    }

    /// Chance one salvo of the mounts bearing at `bearing` starts at least
    /// one fire, every shell hitting independently: 1 - product of (1 - p)
    /// over the bearing shells. `None` for aircraft.
    pub fn salvo_fire_chance(&self, coverage: &MountCoverage, bearing: Bearing) -> Option<Percent> {
        if self.mounts.is_empty() {
            return None;
        }

        let no_fire: f32 = self
            .mounts
            .iter()
            .filter(|mount| coverage.bears(&mount.hardpoint, bearing))
            .map(|mount| (1.0 - mount.fire_chance.to_fraction()).powi(mount.barrels as i32))
            .product();

        Some(Percent::from_fraction(1.0 - no_fire))
    }

    /// Projectiles per minute from the mounts bearing at `bearing`; every
    /// projectile for aircraft.
    pub fn projectiles_at(&self, coverage: &MountCoverage, bearing: Bearing) -> ProjectilesPerMinute {
        if self.mounts.is_empty() {
            return self.projectiles_per_minute;
        }

        let bearing_mounts = self.mounts.iter().filter(|mount| coverage.bears(&mount.hardpoint, bearing));
        ProjectilesPerMinute::new(bearing_mounts.map(|mount| mount.projectiles.value()).sum())
    }
}

/// One gun mount's fires per minute.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MountFires {
    pub hardpoint: String,

    /// Barrels on the mount.
    pub barrels: u32,

    /// Chance one of its shells starts a fire.
    pub fire_chance: Percent,
    pub projectiles: ProjectilesPerMinute,
    pub fires: FiresPerMinute,
}

/// The inputs fire output reads besides the card.
pub struct FireInputs<'a> {
    pub ship: &'a Param,
    pub selection: &'a ShipUpgradeSelection,
    pub stats: &'a ShipStats,

    /// The modifiers the card was computed with.
    pub modifiers: &'a ModifierBundle,
    pub provider: &'a dyn GameParamProvider,
}

impl FireInputs<'_> {
    /// Every armament that can start fires, main battery first.
    pub fn sources(&self) -> Vec<FireSource> {
        let mut sources = self.main_battery();
        sources.extend(self.secondaries());
        sources.extend(self.squadrons());
        sources.extend(self.air_strikes());

        sources
    }

    /// Main battery per ammo type with a fire chance. A mount fires the first
    /// shell of the type it lists.
    fn main_battery(&self) -> Vec<FireSource> {
        let Some(artillery) = self.stats.artillery.as_ref() else {
            return Vec::new();
        };
        let Some(reload) = artillery.reload_time.filter(|reload| reload.value() > 0.0) else {
            return Vec::new();
        };
        let reload = Seconds::new(reload.value());
        let mounts = aiming_mounts(self.ship, self.selection, ComponentType::Artillery);

        let mut kinds: Vec<(String, Vec<&ShellStats>)> = Vec::new();
        for shell in &artillery.shells {
            let Some(kind) = shell.ammo_kind.clone() else {
                continue;
            };
            match kinds.iter_mut().find(|(known, _)| *known == kind) {
                Some((_, shells)) => shells.push(shell),
                None => kinds.push((kind, vec![shell])),
            }
        }

        let mut sources = Vec::new();
        for (kind, shells) in kinds {
            let mut projectiles = 0.0;
            let mut fires = 0.0;
            let mut per_mount = Vec::new();
            for (mount, barrels) in &mounts {
                let Some(shell) = mount.ammo().iter().find_map(|name| shells.iter().find(|shell| shell.name == *name))
                else {
                    continue;
                };
                let barrels = *barrels as f32;
                projectiles += barrels;
                fires += barrels * burn_fraction(shell);
                per_mount.push(MountFires {
                    hardpoint: mount.hp_name().to_owned(),
                    barrels: barrels as u32,
                    fire_chance: Percent::from_fraction(burn_fraction(shell)),
                    projectiles: ProjectilesPerMinute::from_interval(barrels, reload),
                    fires: FiresPerMinute::new(60.0 * barrels * burn_fraction(shell) / reload.value()),
                });
            }
            if fires <= 0.0 {
                continue;
            }

            sources.push(FireSource {
                armament: FireArmament::MainBattery,
                qualifier: kind,
                module_upgrade: false,
                projectiles_per_minute: ProjectilesPerMinute::from_interval(projectiles, reload),
                fire_chance: Percent::from_fraction(fires / projectiles),
                fires_per_minute: FiresPerMinute::new(60.0 * fires / reload.value()),
                mounts: per_mount,
            });
        }

        sources
    }

    /// Each secondary gun group with a fire chance, from its aiming mounts.
    fn secondaries(&self) -> Vec<FireSource> {
        let Some(battery) = self.stats.secondaries.as_ref() else {
            return Vec::new();
        };
        let mounts = aiming_mounts(self.ship, self.selection, ComponentType::Atba);

        battery
            .mounts
            .iter()
            .filter(|group| burn_fraction(&group.shell) > 0.0 && group.reload_time.value() > 0.0)
            .map(|group| {
                let reload = Seconds::new(group.reload_time.value());
                let chance = Percent::from_fraction(burn_fraction(&group.shell));
                let in_group: Vec<(String, f32)> = mounts
                    .iter()
                    .filter(|(mount, _)| group.hardpoints.iter().any(|hardpoint| hardpoint == mount.hp_name()))
                    .map(|(mount, barrels)| (mount.hp_name().to_owned(), *barrels as f32))
                    .collect();
                let barrels: f32 = in_group.iter().map(|(_, barrels)| barrels).sum();
                let projectiles = ProjectilesPerMinute::from_interval(barrels, reload);
                FireSource {
                    armament: FireArmament::Secondary,
                    qualifier: group.label.clone(),
                    module_upgrade: false,
                    projectiles_per_minute: projectiles,
                    fire_chance: chance,
                    fires_per_minute: projectiles.fires(chance),
                    mounts: in_group
                        .into_iter()
                        .map(|(hardpoint, barrels)| MountFires {
                            hardpoint,
                            barrels: barrels as u32,
                            fire_chance: chance,
                            projectiles: ProjectilesPerMinute::from_interval(barrels, reload),
                            fires: ProjectilesPerMinute::from_interval(barrels, reload).fires(chance),
                        })
                        .collect(),
                }
            })
            .filter(|source| source.fires_per_minute.value() > 0.0)
            .collect()
    }

    /// Every squadron of every plane module whose ordnance can start fires,
    /// stock modules first.
    fn squadrons(&self) -> Vec<FireSource> {
        let Some(components) = self.ship.vehicle().and_then(|vehicle| vehicle.ttx_components()) else {
            return Vec::new();
        };
        let mut upgrades: Vec<_> = components.squadrons.iter().collect();
        upgrades.sort_by(|(left_name, left), (right_name, right)| {
            right.stock.cmp(&left.stock).then_with(|| left_name.cmp(right_name))
        });

        let mut sources = Vec::new();
        for (_, upgrade) in upgrades {
            for plane in &upgrade.planes {
                let sortie = Sortie::Squadron { module_upgrade: !upgrade.stock };
                let Some(source) = self.aircraft(plane, sortie) else {
                    continue;
                };
                if !sources.iter().any(|known: &FireSource| known.qualifier == source.qualifier) {
                    sources.push(source);
                }
            }
        }

        sources
    }

    /// The air strike of the selected hull.
    fn air_strikes(&self) -> Vec<FireSource> {
        let Some(strike) = self
            .ship
            .vehicle()
            .and_then(|vehicle| vehicle.ttx_components())
            .and_then(|components| components.air_supports.get(self.selection.hull.as_deref()?))
        else {
            return Vec::new();
        };

        let Some(reload) = strike.reload_time.filter(|reload| reload.value() > 0.0) else {
            return Vec::new();
        };
        let sortie = Sortie::AirStrike { reload: Seconds::new(reload.value()) };

        strike.planes.iter().filter_map(|plane| self.aircraft(plane, sortie)).collect()
    }

    /// One aircraft's fire output.
    fn aircraft(&self, plane: &str, sortie: Sortie) -> Option<FireSource> {
        let aircraft = self.provider.game_param_by_name(plane)?;
        let aircraft = aircraft.aircraft()?;
        let ordnance = self.provider.game_param_by_name(aircraft.ordnance()?)?;
        let projectile = ordnance.projectile()?;

        let species = BurnSpecies::from_species(ordnance.species().and_then(|species| species.known()));
        let is_small = is_small_projectile(species, projectile.bullet_diametr(), SMALL_PROJECTILE_MAX_DIAMETER_M);
        let level = self.ship.vehicle()?.level();
        let (chance, _) = calculate_burn_chance(species, level, projectile.burn_prob()?, self.modifiers, is_small);
        let chance = chance.clamp(0.0, 1.0);
        if chance <= 0.0 {
            return None;
        }

        let projectiles = match sortie {
            Sortie::Squadron { .. } => {
                let interval = if aircraft.attacks_per_sortie()? > 1 {
                    aircraft.attack_cooldown()?
                } else {
                    let restore = aircraft.hangar_restore().filter(|restore| restore.planes > 0)?;
                    let restores = aircraft.attack_flight_size()?.div_ceil(restore.planes);
                    restore.time * restores as f32
                };
                if interval.value() <= 0.0 {
                    return None;
                }
                ProjectilesPerMinute::from_interval(aircraft.payload_per_attack()? as f32, interval)
            }
            Sortie::AirStrike { reload } => {
                let per_strike = aircraft.squadron_size()? * aircraft.payload_per_plane()?;
                ProjectilesPerMinute::from_interval(per_strike as f32, reload)
            }
        };
        let chance = Percent::from_fraction(chance);

        Some(FireSource {
            armament: sortie.armament(),
            qualifier: plane.to_owned(),
            module_upgrade: matches!(sortie, Sortie::Squadron { module_upgrade: true }),
            projectiles_per_minute: projectiles,
            fire_chance: chance,
            fires_per_minute: projectiles.fires(chance),
            mounts: Vec::new(),
        })
    }
}

/// Fires against one target at `bearing`, in total and per armament.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FireBreakdown {
    pub bearing: Bearing,
    pub total: FiresPerMinute,

    /// The best ammo type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub main_battery: Option<FiresPerMinute>,

    /// Every secondary group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary: Option<FiresPerMinute>,

    /// The best stock squadron and the best air strike aircraft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aircraft: Option<FiresPerMinute>,
}

/// How often an aircraft releases its ordnance.
#[derive(Debug, Clone, Copy)]
enum Sortie {
    /// `attackerSize` planes attack every `attackCooldown` while the squadron
    /// has attacks left; a squadron with one attack per sortie attacks once
    /// per hangar restore of its planes.
    Squadron { module_upgrade: bool },

    /// Every plane attacks once per consumable reload.
    AirStrike { reload: Seconds },
}

impl Sortie {
    fn armament(self) -> FireArmament {
        match self {
            Self::Squadron { .. } => FireArmament::Squadron,
            Self::AirStrike { .. } => FireArmament::AirStrike,
        }
    }
}

/// What each armament adds to a total: the main battery's best ammo type,
/// every secondary group, the best squadron of the stock plane modules and
/// the best air strike aircraft.
#[derive(Debug, Clone)]
struct Parts<V> {
    main_battery: Option<V>,
    secondary: Option<V>,
    squadron: Option<V>,
    air_strike: Option<V>,
}

impl<V> Parts<V>
where
    V: std::ops::Add<Output = V>,
{
    /// `best` chooses between alternatives of one armament.
    fn new(sources: &[FireSource], value: impl Fn(&FireSource) -> V, best: impl Fn(V, V) -> V) -> Self {
        let counted = |armament: FireArmament| {
            sources.iter().filter(move |source| source.armament == armament && !source.module_upgrade).map(&value)
        };

        Self {
            main_battery: counted(FireArmament::MainBattery).reduce(&best),
            secondary: counted(FireArmament::Secondary).reduce(|sum, group| sum + group),
            squadron: counted(FireArmament::Squadron).reduce(&best),
            air_strike: counted(FireArmament::AirStrike).reduce(&best),
        }
    }

    fn sum(self) -> Option<V> {
        [self.main_battery, self.secondary, self.squadron, self.air_strike]
            .into_iter()
            .flatten()
            .reduce(|sum, part| sum + part)
    }
}

/// Fires every armament starts together, every gun mount firing: the main
/// battery's best ammo type, every secondary group, the best squadron of the
/// stock plane modules (one flies at a time) and the best air strike aircraft
/// (the aircraft of one air strike are alternatives sharing its charges). A
/// ceiling: no single target draws every mount. `None` when nothing can start
/// fires.
pub fn total_fires_all_mounts(sources: &[FireSource]) -> Option<FiresPerMinute> {
    Parts::new(sources, |source| source.fires_per_minute, |left, right| if right > left { right } else { left }).sum()
}

/// The same total against one target at each bearing: only the gun mounts
/// that bear on it fire. `None` when nothing can start fires.
pub fn total_fires_by_bearing(sources: &[FireSource], coverage: &MountCoverage) -> Option<PerBearing<FiresPerMinute>> {
    Parts::new(sources, |source| source.by_bearing(coverage), PerBearing::max).sum()
}

/// The secondary groups together against one target at each bearing. `None`
/// without secondaries that start fires.
pub fn secondary_fires_by_bearing(
    sources: &[FireSource],
    coverage: &MountCoverage,
) -> Option<PerBearing<FiresPerMinute>> {
    Parts::new(sources, |source| source.by_bearing(coverage), PerBearing::max).secondary
}

/// What each armament adds to the fires against one target at the bearing
/// where the total is highest. `None` when nothing can start fires.
pub fn fire_breakdown(sources: &[FireSource], coverage: &MountCoverage) -> Option<FireBreakdown> {
    let parts = Parts::new(sources, |source| source.by_bearing(coverage), PerBearing::max);
    let total = parts.clone().sum()?;
    let bearing = total.best();
    let at = |part: Option<PerBearing<FiresPerMinute>>| part.map(|part| part.at(bearing));
    let aircraft = match (at(parts.squadron), at(parts.air_strike)) {
        (Some(squadron), Some(strike)) => Some(squadron + strike),
        (squadron, strike) => squadron.or(strike),
    };

    Some(FireBreakdown {
        bearing,
        total: total.at(bearing),
        main_battery: at(parts.main_battery),
        secondary: at(parts.secondary),
        aircraft,
    })
}

/// The card's fire chance of a shell, 0 to 1. A shell the card gives no fire
/// chance starts no fires.
fn burn_fraction(shell: &ShellStats) -> f32 {
    shell.burn_chance.map_or(0.0, |chance| (chance.value() / 100.0).clamp(0.0, 1.0))
}
