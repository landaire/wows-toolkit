//! Theoretical fire starts per minute of each armament: projectiles released
//! per minute times the chance that one starts a fire.
//!
//! Every projectile is taken to hit a target that can burn, with no limit on
//! the fires the target can carry and none of the target's fire resistance.
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

    pub fires_per_minute: FiresPerMinute,
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
            for (mount, barrels) in &mounts {
                let Some(shell) = mount.ammo().iter().find_map(|name| shells.iter().find(|shell| shell.name == *name))
                else {
                    continue;
                };
                let barrels = *barrels as f32;
                projectiles += barrels;
                fires += barrels * burn_fraction(shell);
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
            });
        }

        sources
    }

    /// Each secondary gun group with a fire chance.
    fn secondaries(&self) -> Vec<FireSource> {
        let Some(battery) = self.stats.secondaries.as_ref() else {
            return Vec::new();
        };

        battery
            .mounts
            .iter()
            .filter(|mount| burn_fraction(&mount.shell) > 0.0 && mount.reload_time.value() > 0.0)
            .map(|mount| {
                let barrels = (mount.num_barrels * mount.num_mounts) as f32;
                let projectiles = ProjectilesPerMinute::from_interval(barrels, Seconds::new(mount.reload_time.value()));
                let chance = Percent::from_fraction(burn_fraction(&mount.shell));
                FireSource {
                    armament: FireArmament::Secondary,
                    qualifier: mount.label.clone(),
                    module_upgrade: false,
                    projectiles_per_minute: projectiles,
                    fire_chance: chance,
                    fires_per_minute: projectiles.fires(chance),
                }
            })
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
        })
    }
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

/// Fires every armament starts together: the main battery's best ammo type,
/// every secondary group, the best squadron of the stock plane modules (one
/// flies at a time) and the best air strike aircraft (the aircraft of one
/// air strike are alternatives sharing its charges). `None` when nothing can
/// start fires.
pub fn total_fires(sources: &[FireSource]) -> Option<FiresPerMinute> {
    if sources.is_empty() {
        return None;
    }

    let counted = |armament: FireArmament| {
        sources.iter().filter(move |source| source.armament == armament && !source.module_upgrade)
    };
    let best = |armament: FireArmament| -> FiresPerMinute {
        counted(armament)
            .map(|source| source.fires_per_minute)
            .fold(FiresPerMinute::new(0.0), |best, rate| if rate > best { rate } else { best })
    };
    let every =
        |armament: FireArmament| -> FiresPerMinute { counted(armament).map(|source| source.fires_per_minute).sum() };

    Some(
        best(FireArmament::MainBattery)
            + every(FireArmament::Secondary)
            + best(FireArmament::Squadron)
            + best(FireArmament::AirStrike),
    )
}

/// The card's fire chance of a shell, 0 to 1. A shell the card gives no fire
/// chance starts no fires.
fn burn_fraction(shell: &ShellStats) -> f32 {
    shell.burn_chance.map_or(0.0, |chance| (chance.value() / 100.0).clamp(0.0, 1.0))
}
