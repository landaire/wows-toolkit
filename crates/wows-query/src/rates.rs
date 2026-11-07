//! Rates of fire on the best-fit build: torpedoes launched per minute, and
//! the theoretical damage per minute and per second of each ammo.
//!
//! A torpedo module may limit how many of its launchers reload at the same
//! time. The client reloads a module with launcher `groups` through a queue
//! that releases at most `loaders` launchers of each group at once
//! (`TorpedoesReloadManagers`); `useGroups` only changes the port stat card.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::Entry;

use serde::Serialize;
use wows_core::units::DamagePerMinute;
use wows_core::units::DamagePerSecond;
use wows_core::units::Degrees;
use wows_core::units::Seconds;
use wows_core::units::TorpedoesPerMinute;
use wows_ship_stats::mount_salvo;
use wows_ship_stats::mounts::selected_component;
use wows_ship_stats::mounts::selected_mounts;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::components::LauncherDirection;
use wowsunpack::game_params::ttx::components::LoaderCount;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::model::ShellStats;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::MountPoint;

use crate::armament::BarrelCount;
use crate::armament::MountCount;
use crate::builds::Better;
use crate::builds::BuildStat;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::builds::StatUnit;
use crate::builds::round;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::PerArc;
use crate::firing_arcs::Weapon;
use crate::firing_arcs::bearing_sets;
use crate::firing_arcs::oriented_mounts;
use crate::firing_arcs::round_to;
use crate::firing_arcs::weapon_mounts;
use crate::ships::Ship;
use crate::special_abilities::Burst;

const DAMAGE_NOTE: &str = "Damage rates are theoretical maxima: every shell or torpedo fired at its reload hits for its full damage. Not applied: fires, flooding, citadel and overpenetration multipliers, armor, dispersion, shell flight time, turret traverse and ammo switching. Guns: damage_per_minute = barrels x damage x 60 / reload. Torpedoes: damage_per_minute = torpedoes_per_minute x damage. damage_per_second = damage_per_minute / 60. broadside and bow_on count only the barrels that bear at the bearing inside that arc where the most damage bears, as ship_firing_arcs reports; broadside is 30-150 or 210-330 off the bow, bow_on 331-29. An ammo with modules is carried only with those modules; its base uses them.";

const BURST_NOTE: &str = "Main battery in Burst Fire: each turret fires shots salvos interval apart, then reloads for reload. damage_per_minute averages one burst and that reload: barrels x damage x shots x 60 / ((shots - 1) x interval + reload). during_burst is the rate while a burst fires: barrels x damage x 60 / interval.";

const TORPEDO_NOTE: &str = "torpedoes_per_minute = 60 x tubes_per_reload / reload. A loader group reloads at most `loaders` of its launchers at a time and the others wait, so it reloads tubes x min(loaders, launchers) / launchers tubes per reload time. Launchers outside every loader group reload all at once. launchers, tubes, loader_groups and limit describe the stock modules; a build that assumes another hull or torpedo module uses that module's launchers.";

/// Rates on the best-fit build.
#[derive(Debug, Clone, Serialize)]
pub struct Rates {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub torpedoes_per_minute: Option<TorpedoRate>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub main_battery_damage: Vec<AmmoDamage>,

    /// One entry per secondary gun group.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub secondary_damage: Vec<AmmoDamage>,

    /// One entry per torpedo any torpedo module carries.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub torpedo_damage: Vec<AmmoDamage>,

    /// How the rates are computed and what they leave out.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rate_notes: Vec<&'static str>,
}

/// Torpedoes launched per minute, every launcher firing as soon as it is
/// loaded. Launchers and tubes are those of the stock modules.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TorpedoRate {
    pub launchers: MountCount,
    pub tubes: BarrelCount,

    /// Launchers that share loaders. Empty when every launcher reloads at once.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub loader_groups: Vec<LoaderGroup>,

    /// Tubes reloaded per reload time, all launchers.
    pub tubes_per_reload: f32,

    /// How the loaders limit the rate. Absent when they do not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,

    pub rate: BuildStat<TorpedoesPerMinute>,
}

/// Launchers of which at most `loaders` reload at a time.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoaderGroup {
    /// Where a submarine's launchers point.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<TubeDirection>,

    pub launchers: MountCount,
    pub tubes: BarrelCount,
    pub loaders: LoaderCount,

    /// tubes x min(loaders, launchers) / launchers.
    pub tubes_per_reload: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TubeDirection {
    Bow,
    Stern,
    Rotating,

    /// A direction the game names that this crate does not recognize.
    Other(String),
}

impl TubeDirection {
    fn from_launcher(direction: &LauncherDirection) -> Self {
        match direction {
            LauncherDirection::Forward => Self::Bow,
            LauncherDirection::Rear => Self::Stern,
            LauncherDirection::Rotating => Self::Rotating,
            LauncherDirection::Other(name) => Self::Other(name.clone()),
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Bow => "bow",
            Self::Stern => "stern",
            Self::Rotating => "rotating",
            Self::Other(name) => name,
        }
    }
}

/// Theoretical damage rates of one ammo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AmmoDamage {
    /// "HE", "AP" or "SAP"; a secondary gun group and its ammo, e.g.
    /// "127 mm HE"; or a torpedo's name.
    pub ammo: String,

    /// Modules other than stock that carry the ammo. Empty for stock ammo.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,

    /// Damage of one shell or torpedo in whole HP, no skills or upgrades.
    pub damage: Hp,

    /// Barrels or tubes firing it, all mounts.
    pub barrels: BarrelCount,

    pub rates: BuildStat<DamageRate>,

    /// Main battery in bursts: damage of one burst of every barrel, no
    /// skills or upgrades.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burst_damage: Option<Hp>,

    /// Main battery in bursts: the rate while a burst fires, one salvo per
    /// interval. `rates` then averages a burst and the reload after it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub during_burst: Option<BuildStat<DamageRate>>,
}

/// Damage per minute and per second of one ammo, every barrel firing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DamageRate {
    pub damage_per_minute: DamagePerMinute,
    pub damage_per_second: DamagePerSecond,

    /// Barrels bearing on a broadside target only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadside_damage_per_minute: Option<DamagePerMinute>,

    /// Barrels bearing on a target ahead only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bow_on_damage_per_minute: Option<DamagePerMinute>,
}

impl DamageRate {
    fn new(rate: DamagePerMinute, arcs: Option<ArcRates>) -> Self {
        let whole = |rate: DamagePerMinute| DamagePerMinute::new(rate.value().round());

        Self {
            damage_per_minute: whole(rate),
            damage_per_second: DamagePerSecond::new(round_to(rate.per_second().value(), 10.0)),
            broadside_damage_per_minute: arcs.map(|arcs| whole(arcs.broadside)),
            bow_on_damage_per_minute: arcs.map(|arcs| whole(arcs.bow_on)),
        }
    }
}

impl StatAmount for DamageRate {
    fn amount(self) -> f32 {
        self.damage_per_minute.value()
    }
}

impl StatUnit for DamageRate {
    const UNIT: &'static str = "HP/min";
}

/// Damage per minute of the barrels that bear inside the broadside and bow
/// arcs.
#[derive(Debug, Clone, Copy)]
struct ArcRates {
    broadside: DamagePerMinute,
    bow_on: DamagePerMinute,
}

/// How often the main battery fires.
#[derive(Debug, Clone, Copy)]
enum Cadence {
    /// One salvo per reload.
    Standard,

    /// Bursts, averaged over a burst and the reload after it.
    Cycle(Burst),

    /// The salvos of one burst.
    InBurst(Burst),
}

impl Cadence {
    /// Seconds per salvo, given the card's main battery reload.
    fn salvo_time(self, reload: Seconds) -> Seconds {
        match self {
            Self::Standard => reload,
            Self::Cycle(burst) => Seconds::new(burst.cycle(reload).value() / burst.shots.0 as f32),
            Self::InBurst(burst) => burst.interval(reload),
        }
    }
}

/// One gun mount: its hardpoint, the shells it may fire and its barrels.
struct Gun {
    hardpoint: String,
    ammo: Vec<String>,
    barrels: BarrelCount,
}

impl Gun {
    fn new(mount: &MountPoint, barrels: u32) -> Self {
        Self { hardpoint: mount.hp_name().to_owned(), ammo: mount.ammo().to_vec(), barrels: BarrelCount(barrels) }
    }
}

/// A battery's mounts on one module combination.
struct Battery {
    guns: Vec<Gun>,

    /// Per arc, the sets of guns that bear together. `None` when the hull's
    /// hardpoint facings are unknown.
    arcs: Option<PerArc<Vec<Vec<usize>>>>,
}

impl Battery {
    /// Damage per minute of all guns and per arc, given each gun's damage
    /// per minute. `None` when no gun fires the ammo.
    fn rate(&self, per_gun: &[DamagePerMinute]) -> Option<DamageRate> {
        let total: DamagePerMinute = per_gun.iter().copied().sum();
        if total.value() <= 0.0 {
            return None;
        }

        let best = |sets: &[Vec<usize>]| -> DamagePerMinute {
            sets.iter()
                .map(|set| set.iter().map(|index| per_gun[*index]).sum::<DamagePerMinute>())
                .fold(DamagePerMinute::new(0.0), |best, rate| if rate > best { rate } else { best })
        };
        let arcs = self.arcs.as_ref().map(|arcs| {
            let (starboard, port) = (best(&arcs.starboard), best(&arcs.port));
            ArcRates { broadside: if starboard > port { starboard } else { port }, bow_on: best(&arcs.bow) }
        });

        Some(DamageRate::new(total, arcs))
    }

    /// Barrels of the guns that may fire any of `shells`.
    fn barrels_firing(&self, shells: &[&str]) -> BarrelCount {
        BarrelCount(
            self.guns
                .iter()
                .filter(|gun| gun.ammo.iter().any(|name| shells.contains(&name.as_str())))
                .map(|gun| gun.barrels.0)
                .sum(),
        )
    }
}

/// Torpedo launchers and their loaders on one module combination.
struct TorpedoLayout {
    launchers: MountCount,
    tubes: BarrelCount,
    groups: Vec<LoaderGroup>,
    tubes_per_reload: f32,
}

impl TorpedoLayout {
    fn limit(&self) -> Option<String> {
        let mut sentences: Vec<String> = self
            .groups
            .iter()
            .filter(|group| group.loaders.0 < group.launchers.0)
            .map(|group| {
                let launchers = match &group.direction {
                    Some(direction) => format!("{} {} launchers", group.launchers.0, direction.label()),
                    None => format!("{} launchers", group.launchers.0),
                };
                let verb = if group.loaders.0 == 1 { "reloads" } else { "reload" };
                format!(
                    "{} of the {launchers} ({} of the {} tubes) {verb} at a time.",
                    group.loaders.0,
                    round(group.tubes_per_reload),
                    group.tubes.0
                )
            })
            .collect();
        if sentences.is_empty() {
            return None;
        }

        let tubes = self.tubes.0;
        sentences.push(format!(
            "{} of the {tubes} tubes reload per reload time, not all {tubes}.",
            round(self.tubes_per_reload)
        ));

        Some(sentences.join(" "))
    }
}

/// Module combination layouts the extractors read, indexed like
/// [`Planner::selections`].
struct Layouts {
    main_battery: Vec<Battery>,
    secondaries: Vec<Battery>,
    torpedoes: Vec<Option<TorpedoLayout>>,
}

impl GameData {
    pub(crate) fn rates(&self, planner: &Planner<'_>) -> Result<Rates, QueryError> {
        let ship = planner.ship();
        let layouts = self.layouts(ship, planner)?;
        let stock = planner.stock()?;

        let torpedoes_per_minute = match &layouts.torpedoes[0] {
            Some(layout) => planner
                .plan(&StatPlan::new("torpedoes per minute".to_owned(), Better::Higher, &|outcome| {
                    torpedo_rate(outcome, &layouts).map(|rate| TorpedoesPerMinute::new(round(rate.value())))
                }))?
                .map(|rate| TorpedoRate {
                    launchers: layout.launchers,
                    tubes: layout.tubes,
                    loader_groups: layout.groups.clone(),
                    tubes_per_reload: round(layout.tubes_per_reload),
                    limit: layout.limit(),
                    rate,
                }),
            None => None,
        };

        let mut kinds: Vec<String> = Vec::new();
        for shell in stock.stats.artillery.iter().flat_map(|artillery| &artillery.shells) {
            if let Some(kind) = shell.ammo_kind.as_deref().and_then(ammo_label)
                && !kinds.contains(&kind)
            {
                kinds.push(kind);
            }
        }
        kinds.sort();
        let mut main_battery_damage = Vec::new();
        let standard = planner.burst().map_or(Cadence::Standard, Cadence::Cycle);
        for kind in kinds {
            let shells: Vec<_> = stock
                .stats
                .artillery
                .iter()
                .flat_map(|artillery| &artillery.shells)
                .filter(|shell| shell.ammo_kind.as_deref().and_then(ammo_label).as_ref() == Some(&kind))
                .collect();
            let Some(damage) = shells.iter().find_map(|shell| shell.damage) else {
                continue;
            };
            let names: Vec<&str> = shells.iter().map(|shell| shell.name.as_str()).collect();
            let barrels = layouts.main_battery[0].barrels_firing(&names);

            let label = format!("main battery {kind} damage rate");
            let extract = |outcome: &Outcome| {
                main_battery_rate(outcome, &layouts.main_battery[outcome.selection], &kind, standard)
            };
            let Some(rates) = planner.plan(&StatPlan::new(label, Better::Higher, &extract))? else {
                continue;
            };

            let mut ammo = AmmoDamage::new(kind.clone(), Vec::new(), damage, barrels, rates);
            if let Some(burst) = planner.burst() {
                let label = format!("main battery {kind} damage rate during a burst");
                let extract = |outcome: &Outcome| {
                    main_battery_rate(outcome, &layouts.main_battery[outcome.selection], &kind, Cadence::InBurst(burst))
                };
                ammo.during_burst = planner.plan(&StatPlan::new(label, Better::Higher, &extract))?;
                let burst_damage = ammo.damage.value() * (ammo.barrels.0 * burst.shots.0) as f32;
                ammo.burst_damage = Some(Hp::from(burst_damage.round()));
            }
            main_battery_damage.push(ammo);
        }

        let mut secondary_damage = Vec::new();
        for mount in stock.stats.secondaries.iter().flat_map(|secondaries| &secondaries.mounts) {
            let Some(damage) = mount.shell.damage else {
                continue;
            };
            let ammo = match mount.shell.ammo_kind.as_deref().and_then(ammo_label) {
                Some(kind) => format!("{} {kind}", mount.label),
                None => mount.label.clone(),
            };
            let label = format!("secondary {ammo} damage rate");
            let extract =
                |outcome: &Outcome| secondary_rate(outcome, &layouts.secondaries[outcome.selection], &mount.label);
            if let Some(rates) = planner.plan(&StatPlan::new(label, Better::Higher, &extract))? {
                let barrels = BarrelCount(mount.num_barrels * mount.num_mounts);
                secondary_damage.push(AmmoDamage::new(ammo, Vec::new(), damage, barrels, rates));
            }
        }

        // Each torpedo with the first module combination that carries it.
        let mut torpedoes: Vec<(String, usize)> = Vec::new();
        for selection in 0..layouts.torpedoes.len() {
            let outcome = planner.bare(selection)?;
            for torpedo in outcome.stats.torpedoes.iter().flat_map(|torpedoes| &torpedoes.torpedoes) {
                if !torpedoes.iter().any(|(name, _)| *name == torpedo.name) {
                    torpedoes.push((torpedo.name.clone(), selection));
                }
            }
        }
        let mut torpedo_damage = Vec::new();
        for (name, selection) in torpedoes {
            let Some(layout) = &layouts.torpedoes[selection] else {
                continue;
            };
            let outcome = planner.bare(selection)?;
            let Some(damage) = outcome
                .stats
                .torpedoes
                .iter()
                .flat_map(|torpedoes| &torpedoes.torpedoes)
                .find(|torpedo| torpedo.name == name)
                .and_then(|torpedo| torpedo.damage)
            else {
                continue;
            };

            let ammo = self.projectile_name(&name);
            let label = format!("{ammo} damage rate");
            let extract = |outcome: &Outcome| torpedo_damage_rate(outcome, &layouts, &name);
            let plan = StatPlan::new(label, Better::Higher, &extract).on_selection(selection);
            if let Some(rates) = planner.plan(&plan)? {
                let modules = planner.selection_modules(selection).to_vec();
                torpedo_damage.push(AmmoDamage::new(ammo, modules, damage, layout.tubes, rates));
            }
        }

        let mut rate_notes = Vec::new();
        if torpedoes_per_minute.is_some() {
            rate_notes.push(TORPEDO_NOTE);
        }
        if !(main_battery_damage.is_empty() && secondary_damage.is_empty() && torpedo_damage.is_empty()) {
            rate_notes.push(DAMAGE_NOTE);
        }
        if planner.burst().is_some() && !main_battery_damage.is_empty() {
            rate_notes.push(BURST_NOTE);
        }

        Ok(Rates { torpedoes_per_minute, main_battery_damage, secondary_damage, torpedo_damage, rate_notes })
    }

    fn layouts(&self, ship: &Ship, planner: &Planner<'_>) -> Result<Layouts, QueryError> {
        let mut facings_by_hull: HashMap<Option<&str>, Option<HashMap<String, Degrees>>> = HashMap::new();
        let mut layouts = Layouts { main_battery: Vec::new(), secondaries: Vec::new(), torpedoes: Vec::new() };
        for selection in planner.selections() {
            let facings = match facings_by_hull.entry(selection.hull.as_deref()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(match self.hardpoint_facings(ship, selection) {
                    Ok(facings) => Some(facings),
                    // Without a readable armor extract, rates leave out the arcs.
                    Err(QueryError::HullFacingsUnavailable { .. } | QueryError::ArmorUnreadable { .. }) => None,
                    Err(error) => return Err(error),
                }),
            };
            let facings = facings.as_ref();

            layouts.main_battery.push(battery(ship, selection, Weapon::MainBattery, facings));
            layouts.secondaries.push(battery(ship, selection, Weapon::Secondaries, facings));
            layouts.torpedoes.push(torpedo_layout(ship, selection));
        }

        Ok(layouts)
    }
}

impl AmmoDamage {
    fn new(ammo: String, modules: Vec<String>, damage: Hp, barrels: BarrelCount, rates: BuildStat<DamageRate>) -> Self {
        Self {
            ammo,
            modules,
            damage: Hp::from(damage.value().round()),
            barrels,
            rates,
            burst_damage: None,
            during_burst: None,
        }
    }
}

/// "AP", "HE" or "SAP" for a GameParams ammo type.
fn ammo_label(kind: &str) -> Option<String> {
    match AmmoType::from_game_str(kind) {
        AmmoType::Unknown(_) => None,
        known => Some(known.display_name().to_owned()),
    }
}

fn battery(
    ship: &Ship,
    selection: &ShipUpgradeSelection,
    weapon: Weapon,
    facings: Option<&HashMap<String, Degrees>>,
) -> Battery {
    // A hardpoint missing from the hull model leaves only the arcs unknown.
    if let Some(oriented) = facings.and_then(|facings| oriented_mounts(ship, selection, weapon, facings).ok()) {
        let guns = oriented.iter().map(|mount| Gun::new(mount.mount, mount.barrels)).collect();
        return Battery { guns, arcs: Some(bearing_sets(&oriented)) };
    }

    let guns =
        weapon_mounts(ship, selection, weapon).into_iter().map(|(mount, barrels)| Gun::new(mount, barrels)).collect();

    Battery { guns, arcs: None }
}

fn torpedo_layout(ship: &Ship, selection: &ShipUpgradeSelection) -> Option<TorpedoLayout> {
    let mounts: Vec<(&str, u32)> = selected_mounts(&ship.param, selection, ComponentType::Torpedoes)
        .iter()
        .filter_map(|mount| Some((mount.hp_name(), mount.num_barrels()?)))
        .collect();
    if mounts.is_empty() {
        return None;
    }

    let loading = selected_component(&ship.param, selection, ComponentType::Torpedoes)
        .and_then(|component| ship.param.vehicle()?.ttx_components()?.torpedo_loading(component));
    let mut grouped: HashSet<&str> = HashSet::new();
    let mut groups = Vec::new();
    for group in loading.iter().flat_map(|loading| &loading.loader_groups) {
        let members: Vec<(&str, u32)> = mounts
            .iter()
            .copied()
            .filter(|(hardpoint, _)| !grouped.contains(hardpoint))
            .filter(|(hardpoint, _)| group.hardpoints.iter().any(|name| name == hardpoint))
            .collect();
        if members.is_empty() {
            continue;
        }

        grouped.extend(members.iter().map(|(hardpoint, _)| *hardpoint));
        let launchers = members.len() as u32;
        let tubes: u32 = members.iter().map(|(_, tubes)| tubes).sum();
        let direction = loading
            .iter()
            .flat_map(|loading| &loading.directions)
            .find(|direction| {
                members.iter().all(|(hardpoint, _)| direction.hardpoints.iter().any(|name| name == hardpoint))
            })
            .map(|direction| TubeDirection::from_launcher(&direction.direction));
        groups.push(LoaderGroup {
            direction,
            launchers: MountCount(launchers),
            tubes: BarrelCount(tubes),
            loaders: group.loaders,
            tubes_per_reload: tubes as f32 * group.loaders.0.min(launchers) as f32 / launchers as f32,
        });
    }

    let free: u32 = mounts.iter().filter(|(hardpoint, _)| !grouped.contains(hardpoint)).map(|(_, tubes)| tubes).sum();
    let tubes_per_reload = free as f32 + groups.iter().map(|group| group.tubes_per_reload).sum::<f32>();

    Some(TorpedoLayout {
        launchers: MountCount(mounts.len() as u32),
        tubes: BarrelCount(mounts.iter().map(|(_, tubes)| tubes).sum()),
        groups,
        tubes_per_reload,
    })
}

fn main_battery_rate(outcome: &Outcome, battery: &Battery, kind: &str, cadence: Cadence) -> Option<DamageRate> {
    let artillery = outcome.stats.artillery.as_ref()?;
    let reload = cadence.salvo_time(Seconds::new(artillery.reload_time?.value()));
    let shells: Vec<&ShellStats> = artillery
        .shells
        .iter()
        .filter(|shell| shell.ammo_kind.as_deref().and_then(ammo_label).as_deref() == Some(kind))
        .collect();

    let per_gun: Vec<DamagePerMinute> = battery
        .guns
        .iter()
        .map(|gun| DamagePerMinute::from_reload(mount_salvo(&gun.ammo, gun.barrels.0, &shells).value(), reload))
        .collect();

    battery.rate(&per_gun)
}

fn secondary_rate(outcome: &Outcome, battery: &Battery, group: &str) -> Option<DamageRate> {
    let mount = outcome.stats.secondaries.as_ref()?.mounts.iter().find(|mount| mount.label == group)?;
    let damage = mount.shell.damage?.value();
    let reload = Seconds::new(mount.reload_time.value());

    // Guns outside the group add nothing.
    let per_gun: Vec<DamagePerMinute> = battery
        .guns
        .iter()
        .map(|gun| {
            let shell = if mount.hardpoints.contains(&gun.hardpoint) { damage } else { 0.0 };
            DamagePerMinute::from_reload(gun.barrels.0 as f32 * shell, reload)
        })
        .collect();

    battery.rate(&per_gun)
}

fn torpedo_rate(outcome: &Outcome, layouts: &Layouts) -> Option<TorpedoesPerMinute> {
    let layout = layouts.torpedoes[outcome.selection].as_ref()?;
    let reload = outcome.stats.torpedoes.as_ref()?.reload_time?;

    Some(TorpedoesPerMinute::from_reload(layout.tubes_per_reload, Seconds::new(reload.value())))
}

fn torpedo_damage_rate(outcome: &Outcome, layouts: &Layouts, name: &str) -> Option<DamageRate> {
    let rate = torpedo_rate(outcome, layouts)?;
    let torpedo = outcome.stats.torpedoes.as_ref()?.torpedoes.iter().find(|torpedo| torpedo.name == name)?;

    Some(DamageRate::new(DamagePerMinute::new(rate.value() * torpedo.damage?.value()), None))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Ships whose stock torpedo modules reload fewer launchers at a time than
    /// they carry.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn ships_with_loader_limited_torpedo_reload() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");

        let mut limited: Vec<String> = game
            .ships
            .iter()
            .filter(|ship| {
                torpedo_layout(ship, &ShipUpgradeSelection::stock(&ship.param))
                    .is_some_and(|layout| layout.tubes_per_reload < layout.tubes.0 as f32)
            })
            .map(|ship| ship.name.clone())
            .collect();
        limited.sort();
        limited.dedup();

        assert!(limited.iter().any(|name| name == "Barfleur"), "{limited:?}");
        insta::assert_yaml_snapshot!((limited.len(), limited));
    }
}
