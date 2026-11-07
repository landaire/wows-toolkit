//! Torpedo launch rates: launchers that share loaders, the launchers that
//! bear on one target, the Torpedo Reload Booster used whenever it is ready,
//! and consumables that shorten the launcher reload while active.
//!
//! The client reloads a module with launcher `groups` through a queue that
//! releases at most `loaders` launchers of each group at once
//! (`TorpedoesReloadManagers`); `useGroups` only changes the port stat card.
//! The booster finishes every launcher still reloading within its
//! `torpedoReloadTime` (`CommonConsumables/Logic/Ship.TorpedoReloader`). A
//! reload time change scales the time left on a reload in progress
//! (`Reloader.__recalculateTimeToReload`), so a buff that multiplies the
//! reload time by `GTShotDelay` speeds up reload progress while it lasts.

use serde::Serialize;
use wows_core::units::TorpedoesPerMinute;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::components::LauncherDirection;
use wowsunpack::game_params::ttx::components::LoaderCount;
use wowsunpack::game_params::ttx::consumables::ConsumableCard;
use wowsunpack::game_params::ttx::consumables::effective_consumable;
use wowsunpack::game_params::ttx::model::Seconds;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::types::AbilityCategory;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::recognized::Recognized;

use crate::bearing::MountCoverage;
use crate::bearing::PerBearing;
use crate::mounts::BarrelCount;
use crate::mounts::MountCount;
use crate::mounts::selected_component;
use crate::mounts::selected_mounts;

/// Consumable type of the Torpedo Reload Booster.
pub const TORPEDO_RELOADER: &str = "torpedoReloader";

/// Modifier that multiplies the launcher reload time.
const TORPEDO_RELOAD_MODIFIER: &str = "GTShotDelay";

/// Salvos of every tube in one consumable cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SalvoCount(pub u32);

/// Salvos per minute of one launcher that fires as soon as it is loaded,
/// before loaders hold it back.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct SalvosPerMinute(f32);

impl SalvosPerMinute {
    /// One salvo per `reload`.
    pub fn on_reload(reload: Seconds) -> Self {
        Self(60.0 / reload.value())
    }

    pub fn value(self) -> f32 {
        self.0
    }

    /// Torpedoes launched per minute by `tubes_per_reload` tubes firing at
    /// this rate.
    pub fn torpedoes(self, tubes_per_reload: f32) -> TorpedoesPerMinute {
        TorpedoesPerMinute::new(tubes_per_reload * self.0)
    }
}

/// Multiplier of the launcher reload time.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReloadFactor(pub f32);

/// Launchers of which at most `loaders` reload at a time.
#[derive(Debug, Clone, PartialEq)]
pub struct LoaderGroup {
    /// Where a submarine's launchers point.
    pub direction: Option<LauncherDirection>,

    pub launchers: MountCount,
    pub tubes: BarrelCount,
    pub loaders: LoaderCount,
}

impl LoaderGroup {
    /// tubes x min(loaders, launchers) / launchers.
    pub fn tubes_per_reload(&self) -> f32 {
        loaded_tubes(self.tubes, self.launchers, self.loaders)
    }
}

/// One launcher and the loader group it reloads in.
#[derive(Debug, Clone, PartialEq)]
struct Launcher {
    hardpoint: String,
    tubes: BarrelCount,

    /// Index into [`TorpedoLayout::groups`]; `None` outside every group.
    group: Option<usize>,
}

/// Torpedo launchers and their loaders on one module combination.
#[derive(Debug, Clone, PartialEq)]
pub struct TorpedoLayout {
    pub launchers: MountCount,
    pub tubes: BarrelCount,

    /// Groups that share loaders. Launchers outside every group reload all
    /// at once.
    pub groups: Vec<LoaderGroup>,

    /// Tubes reloaded per reload time, all launchers.
    pub tubes_per_reload: f32,

    mounts: Vec<Launcher>,
}

impl TorpedoLayout {
    /// The launchers of the selected modules. `None` without torpedoes.
    pub fn from_selection(ship: &Param, selection: &ShipUpgradeSelection) -> Option<Self> {
        let mut mounts: Vec<Launcher> = selected_mounts(ship, selection, ComponentType::Torpedoes)
            .iter()
            .filter_map(|mount| {
                Some(Launcher {
                    hardpoint: mount.hp_name().to_owned(),
                    tubes: BarrelCount(mount.num_barrels()?),
                    group: None,
                })
            })
            .collect();
        if mounts.is_empty() {
            return None;
        }

        let loading = selected_component(ship, selection, ComponentType::Torpedoes)
            .and_then(|component| ship.vehicle()?.ttx_components()?.torpedo_loading(component));
        let mut groups = Vec::new();
        for group in loading.iter().flat_map(|loading| &loading.loader_groups) {
            let members: Vec<&mut Launcher> = mounts
                .iter_mut()
                .filter(|launcher| launcher.group.is_none())
                .filter(|launcher| group.hardpoints.contains(&launcher.hardpoint))
                .collect();
            if members.is_empty() {
                continue;
            }

            let direction = loading
                .iter()
                .flat_map(|loading| &loading.directions)
                .find(|direction| members.iter().all(|launcher| direction.hardpoints.contains(&launcher.hardpoint)))
                .map(|direction| direction.direction.clone());
            let launchers = MountCount(members.len() as u32);
            let tubes = BarrelCount(members.iter().map(|launcher| launcher.tubes.0).sum());
            for launcher in members {
                launcher.group = Some(groups.len());
            }
            groups.push(LoaderGroup { direction, launchers, tubes, loaders: group.loaders });
        }

        let free: u32 =
            mounts.iter().filter(|launcher| launcher.group.is_none()).map(|launcher| launcher.tubes.0).sum();
        let tubes_per_reload = free as f32 + groups.iter().map(LoaderGroup::tubes_per_reload).sum::<f32>();

        Some(Self {
            launchers: MountCount(mounts.len() as u32),
            tubes: BarrelCount(mounts.iter().map(|launcher| launcher.tubes.0).sum()),
            groups,
            tubes_per_reload,
            mounts,
        })
    }

    /// Whether the loaders reload fewer tubes per reload time than the
    /// launchers carry.
    pub fn is_loader_limited(&self) -> bool {
        self.tubes_per_reload < self.tubes.0 as f32
    }

    /// Torpedoes launched per minute by every launcher, a ceiling no single
    /// target sees when the launchers do not all bear on one bearing.
    pub fn torpedoes_per_minute(&self, salvos: SalvosPerMinute) -> TorpedoesPerMinute {
        salvos.torpedoes(self.tubes_per_reload)
    }

    /// Tubes reloaded per reload time at a target on each bearing by the
    /// launchers that bear on it; a loader group then reloads only those of
    /// its launchers. `None` when `coverage` leaves the launchers out.
    pub fn tubes_per_reload_by_bearing(&self, coverage: &MountCoverage) -> Option<PerBearing<f32>> {
        if !coverage.covers_launchers() {
            return None;
        }

        // `value` of each launcher in `group` at the bearings it covers.
        let bearing = |group: Option<usize>, value: &dyn Fn(&Launcher) -> f32| {
            self.mounts
                .iter()
                .filter(|launcher| launcher.group == group)
                .fold(PerBearing::uniform(0.0), |sum, launcher| {
                    sum + PerBearing::mount(coverage, &launcher.hardpoint, value(launcher))
                })
        };
        let tubes = |launcher: &Launcher| launcher.tubes.0 as f32;

        let mut total = bearing(None, &tubes);
        for (idx, group) in self.groups.iter().enumerate() {
            let launchers = bearing(Some(idx), &|_| 1.0);
            let group_tubes = bearing(Some(idx), &tubes);
            let loaders = group.loaders.0 as f32;
            total = total
                + PerBearing::from_fn(|at| {
                    let launchers = launchers.at(at);
                    if launchers > 0.0 { group_tubes.at(at) * loaders.min(launchers) / launchers } else { 0.0 }
                });
        }

        Some(total)
    }

    /// Salvos per minute of each launcher over one booster cycle with the
    /// booster used whenever it is ready. `None` when the loaders limit the
    /// reload: the booster then finishes only the launchers being loaded,
    /// which this model does not cover.
    pub fn booster_salvos(&self, reload: Seconds, booster: &ReloadBooster) -> Option<SalvosPerMinute> {
        if self.is_loader_limited() {
            return None;
        }

        Some(booster.cycle(reload).salvos_per_minute())
    }
}

/// One period of the booster used whenever it is ready.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoosterCycle {
    pub period: Seconds,

    /// Salvos of every tube per period: the booster's own and those on the
    /// launcher reload.
    pub salvos: SalvoCount,
}

impl BoosterCycle {
    pub fn salvos_per_minute(&self) -> SalvosPerMinute {
        SalvosPerMinute(60.0 * self.salvos.0 as f32 / self.period.value())
    }
}

/// The Torpedo Reload Booster with a build's modifiers applied.
#[derive(Debug, Clone, PartialEq)]
pub struct ReloadBooster {
    /// Every launcher still reloading is loaded this long after activation
    /// (`torpedoReloadTime`; no modifier changes it).
    pub torpedo_reload: Seconds,

    pub work_time: Seconds,

    /// Cooldown after the action time ends.
    pub cooldown: Seconds,
}

impl ReloadBooster {
    /// The ship's Torpedo Reload Booster under `modifiers`. `None` when no
    /// consumable slot offers one.
    pub fn of_ship(ship: &Param, provider: &dyn GameParamProvider, modifiers: &ModifierBundle) -> Option<Self> {
        let slots = ship.vehicle()?.abilities()?;

        slots.iter().flatten().find_map(|(ability, variant)| {
            let param = provider.game_param_by_name(ability)?;
            let category = param.ability()?.get_category(variant)?;
            if category.consumable_type_raw() != TORPEDO_RELOADER {
                return None;
            }

            Self::from_category(category, modifiers)
        })
    }

    /// The booster `category` under `modifiers`. `None` when it is not a
    /// Torpedo Reload Booster with a count-based action time.
    pub fn from_category(category: &AbilityCategory, modifiers: &ModifierBundle) -> Option<Self> {
        let torpedo_reload = Seconds::from(*category.effect_fields().get("torpedoReloadTime")?);
        let (stats, _) = effective_consumable(category, modifiers);

        Some(Self { torpedo_reload, work_time: stats.work_time?, cooldown: stats.reload_time })
    }

    /// The best period with a launcher reload of `reload`. The booster can
    /// be activated only `work_time + cooldown` after its last activation,
    /// and only while the launchers need `torpedo_reload` or more to load.
    /// It is activated right after a salvo, then the tubes fire on the
    /// launcher reload. When it is ready again, either it is used at once,
    /// giving up the reload in progress, or after the next salvo; the period
    /// with more salvos per second wins. Without the booster paying off, a
    /// period is one salvo per launcher reload.
    pub fn cycle(&self, reload: Seconds) -> BoosterCycle {
        let reload = reload.value();
        let boosted = self.torpedo_reload.value();
        let ready = self.work_time.value() + self.cooldown.value();
        let rate = |cycle: &BoosterCycle| cycle.salvos.0 as f32 / cycle.period.value();

        let mut best = BoosterCycle { period: Seconds::from(reload), salvos: SalvoCount(1) };
        if boosted >= reload {
            return best;
        }

        let after_salvo = ((ready - boosted).max(0.0) / reload).ceil();
        let waiting = BoosterCycle {
            period: Seconds::from(boosted + after_salvo * reload),
            salvos: SalvoCount(after_salvo as u32 + 1),
        };
        if rate(&waiting) > rate(&best) {
            best = waiting;
        }

        // At `ready` the launchers must still need `torpedo_reload` or more
        // for the booster to activate.
        let before = ((ready - boosted) / reload).floor();
        if before >= 0.0 && (before + 1.0) * reload >= ready {
            let at_once = BoosterCycle { period: Seconds::from(ready), salvos: SalvoCount(before as u32 + 1) };
            if rate(&at_once) > rate(&best) {
                best = at_once;
            }
        }

        best
    }
}

/// A consumable whose buff on its own ship multiplies the launcher reload
/// time while active, such as Auxiliary and Torpedo Weaponry Coordination,
/// with a build's modifiers applied.
#[derive(Debug, Clone, PartialEq)]
pub struct ReloadBuff {
    /// GameParams `consumableType`.
    pub consumable_type: String,

    /// Launcher reload time multiplier while active.
    pub reload_factor: ReloadFactor,

    pub work_time: Seconds,

    /// Cooldown after the action time ends.
    pub cooldown: Seconds,
}

impl ReloadBuff {
    /// The first consumable of the ship whose buff shortens the launcher
    /// reload, under `modifiers`. `None` when no slot offers one.
    pub fn of_ship(ship: &Param, provider: &dyn GameParamProvider, modifiers: &ModifierBundle) -> Option<Self> {
        let slots = ship.vehicle()?.abilities()?;

        slots.iter().flatten().find_map(|(ability, variant)| {
            let param = provider.game_param_by_name(ability)?;
            let category = param.ability()?.get_category(variant)?;

            Self::from_category(category, provider, modifiers)
        })
    }

    /// The consumable `category` under `modifiers`. `None` when its buff on
    /// its own ship does not shorten the launcher reload, or it has no
    /// count-based action time.
    pub fn from_category(
        category: &AbilityCategory,
        provider: &dyn GameParamProvider,
        modifiers: &ModifierBundle,
    ) -> Option<Self> {
        // A consumable used on allies names the buff on its own ship apart.
        let buff = if category.target_team() == Some("ally") { category.buff_on_self() } else { category.buff() }?;
        let param = provider.game_param_by_name(buff)?;
        let buff = param.modifier_param()?;
        let species = modifiers.species();
        if !buff.applies_to(species) {
            return None;
        }

        let factors: Vec<f32> = buff
            .levels()
            .first()?
            .iter()
            .filter(|modifier| modifier.name() == TORPEDO_RELOAD_MODIFIER)
            .map(|modifier| modifier.get_for_species(&species))
            .collect();
        let reload_factor = factors.iter().product::<f32>();
        if factors.is_empty() || reload_factor >= 1.0 {
            return None;
        }

        let (stats, _) = effective_consumable(category, modifiers);

        Some(Self {
            consumable_type: category.consumable_type_raw().to_owned(),
            reload_factor: ReloadFactor(reload_factor),
            work_time: stats.work_time?,
            cooldown: stats.reload_time,
        })
    }

    /// Salvos per minute of each launcher over one cycle with the buff used
    /// whenever it is ready: for `work_time` of every `work_time + cooldown`
    /// the reload progresses `1 / reload_factor` times as fast.
    pub fn salvos_per_minute(&self, reload: Seconds) -> SalvosPerMinute {
        let period = self.work_time.value() + self.cooldown.value();
        let progress = self.cooldown.value() + self.work_time.value() / self.reload_factor.0;

        SalvosPerMinute(60.0 * progress / (period * reload.value()))
    }
}

/// The label of the card of the consumable of type `consumable_type`,
/// which qualifies its card rows.
pub fn consumable_card_label<'a>(cards: &'a [ConsumableCard], consumable_type: &str) -> Option<&'a str> {
    cards
        .iter()
        .find(|card| match &card.consumable {
            Recognized::Known(consumable) => consumable.name() == consumable_type,
            Recognized::Unknown(raw) => raw == consumable_type,
        })
        .map(|card| card.label.as_str())
}

/// tubes x min(loaders, launchers) / launchers.
fn loaded_tubes(tubes: BarrelCount, launchers: MountCount, loaders: LoaderCount) -> f32 {
    tubes.0 as f32 * loaders.0.min(launchers.0) as f32 / launchers.0 as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn booster(cooldown: f32) -> ReloadBooster {
        ReloadBooster {
            torpedo_reload: Seconds::from(5.0),
            work_time: Seconds::from(1.0),
            cooldown: Seconds::from(cooldown),
        }
    }

    /// La Pampa's booster (5 s, 1 s action, 330 s cooldown) on its 130 s
    /// reload: waiting for the third reload salvo gives 4 salvos in 395 s,
    /// more than 3 salvos in 331 s when used at once.
    #[test]
    fn a_long_cooldown_waits_for_the_next_salvo() {
        assert_eq!(
            booster(330.0).cycle(Seconds::from(130.0)),
            BoosterCycle { period: Seconds::from(395.0), salvos: SalvoCount(4) }
        );
    }

    /// A 49.5 s cooldown is ready long before the 130 s reload: one booster
    /// salvo every 50.5 s.
    #[test]
    fn a_short_cooldown_uses_the_booster_at_once() {
        assert_eq!(
            booster(49.5).cycle(Seconds::from(130.0)),
            BoosterCycle { period: Seconds::from(50.5), salvos: SalvoCount(1) }
        );
    }

    /// A booster slower than the launcher reload never pays off.
    #[test]
    fn a_slow_booster_keeps_the_reload() {
        assert_eq!(
            booster(330.0).cycle(Seconds::from(4.0)),
            BoosterCycle { period: Seconds::from(4.0), salvos: SalvoCount(1) }
        );
    }

    /// A x0.8 reload for 30 s of every 150 s: 120 + 30 / 0.8 = 157.5 s of
    /// reload progress per 150 s, 5% more salvos.
    #[test]
    fn a_reload_buff_speeds_up_its_share_of_the_cycle() {
        let buff = ReloadBuff {
            consumable_type: "auxTorpBooster".to_owned(),
            reload_factor: ReloadFactor(0.8),
            work_time: Seconds::from(30.0),
            cooldown: Seconds::from(120.0),
        };

        let salvos = buff.salvos_per_minute(Seconds::from(60.0)).value();
        assert!((salvos - 1.05).abs() < 1e-5, "{salvos}");
    }
}
