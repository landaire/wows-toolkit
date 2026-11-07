//! Rates of fire on the best-fit build: torpedoes launched per minute, and
//! the theoretical damage per minute and per second of each ammo, also over
//! a cycle of a consumable that speeds up the torpedo reload.
//!
//! A torpedo module may limit how many of its launchers reload at the same
//! time; `wows_ship_stats::torpedoes` reads the loaders.

use std::rc::Rc;

use serde::Serialize;
use wows_core::units::DamagePerMinute;
use wows_core::units::DamagePerSecond;
use wows_core::units::Seconds;
use wows_core::units::TorpedoesPerMinute;
use wows_ship_stats::Bearing;
use wows_ship_stats::MountCoverage;
use wows_ship_stats::PerBearing;
use wows_ship_stats::bearing::BearingRow;
use wows_ship_stats::mount_salvo;
use wows_ship_stats::mounts::BarrelCount;
use wows_ship_stats::mounts::MountCount;
use wows_ship_stats::torpedoes::BoosterCycle;
use wows_ship_stats::torpedoes::ReloadBooster;
use wows_ship_stats::torpedoes::ReloadBuff;
use wows_ship_stats::torpedoes::ReloadFactor;
use wows_ship_stats::torpedoes::SalvoCount;
use wows_ship_stats::torpedoes::SalvosPerMinute;
use wows_ship_stats::torpedoes::TORPEDO_RELOADER;
use wows_ship_stats::torpedoes::TorpedoLayout;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::components::LauncherDirection;
use wowsunpack::game_params::ttx::components::LoaderCount;
use wowsunpack::game_params::ttx::consumables::effective_consumable;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::model::ShellStats;
use wowsunpack::game_params::types::AbilityCategory;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::MountPoint;

use crate::builds::Better;
use crate::builds::BuildAlternative;
use crate::builds::BuildStat;
use crate::builds::Builds;
use crate::builds::Charges;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatAmount;
use crate::builds::StatPlan;
use crate::builds::StatUnit;
use crate::builds::round;
use crate::consumables::Capacity;
use crate::consumables::ReloadMultiplier;
use crate::consumables::consumable_builds;
use crate::consumables::consumable_plan;
use crate::consumables::consumable_stats;
use crate::consumables::main_battery_reload_factor;
use crate::consumables::slot_choice;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::Weapon;
use crate::firing_arcs::round_to;
use crate::firing_arcs::weapon_mounts;
use crate::loadout::AbilityModifiers;
use crate::ships::Ship;
use crate::special_abilities::Burst;

const DAMAGE_NOTE: &str = "Damage rates are theoretical maxima: every shell or torpedo fired at its reload hits for its full damage. Not applied: fires, flooding, citadel and overpenetration multipliers, armor, dispersion, shell flight time, turret traverse and ammo switching. Guns: damage_per_minute is against one distant target at the bearing (degrees off the bow, clockwise) where the most damage bears, counting only the barrels whose mounts turn to it outside their dead zones; all_barrels_ceiling is every barrel firing at once, which no single target sees. Each barrel adds damage x 60 / reload. by_bearing gives the rate at standard bearings, starboard and its port mirror, on the base and best builds. Torpedoes: damage_per_minute = torpedoes_per_minute x damage, against one target at the bearing where the most tubes bear as for torpedoes_per_minute; all_barrels_ceiling counts every tube. damage_per_second = damage_per_minute / 60. An ammo with modules is carried only with those modules; its base uses them.";

const BURST_NOTE: &str = "Main battery in Burst Fire: each turret fires shots salvos interval apart, then reloads for reload. damage_per_minute averages one burst and that reload: barrels x damage x shots x 60 / ((shots - 1) x interval + reload). during_burst is the rate while a burst fires: barrels x damage x 60 / interval.";

const TORPEDO_NOTE: &str = "torpedoes_per_minute = 60 x tubes_per_reload / reload, with no consumable, against one distant target at the bearing (degrees off the bow, clockwise) where the most tubes bear, counting only the launchers that turn to it outside their dead zones. all_launchers_ceiling is every launcher firing at once, which a ship with launchers on both sides brings to bear on no single target without turning. by_bearing gives the rate at standard bearings, starboard and its port mirror, on the base and best builds. A loader group reloads at most `loaders` of its launchers at a time and the others wait, so it reloads tubes x min(loaders, launchers) / launchers tubes per reload time; against one target it reloads only its launchers bearing on it. Launchers outside every loader group reload all at once. Submarine tubes have no bearing figures; their rates count every launcher. launchers, tubes, loader_groups and limit describe the stock modules; a build that assumes another hull or torpedo module uses that module's launchers.";

const BOOSTER_NOTE: &str = "torpedo_reload_booster is the Torpedo Reload Booster consumable, not the launcher reload: torpedo_reload is how long the launchers take to reload, shortened by skills and upgrades such as Fill the Tubes and Torpedo Tubes Modification 2. Activated while the launchers still need launchers_loaded_in_s or more, the booster loads every launcher still reloading within launchers_loaded_in_s, giving up any reload progress, then cools down for cooldown once action_time ends. Its torpedoes_per_minute and torpedo_damage average one booster cycle with the booster used whenever it is ready: one booster salvo plus the salvos on the launcher reload until it is ready again, salvos_per_cycle in cycle_s, against one target and with every launcher as for the launcher reload. They hold only while charges last, charges x cycle_s; then the plain rates apply. With a choice, the ship carries the booster only in place of the slot's other options.";

const BUFF_NOTE: &str = "torpedo_reload_buffs are consumables, such as Auxiliary and Torpedo Weaponry Coordination, whose buff multiplies the launcher reload time by reload_factor_while_active for action_time; a reload in progress speeds up while it lasts. Their torpedoes_per_minute and torpedo_damage average one cycle of action_time + cooldown with the consumable used whenever it is ready: the launcher reload rate times (cooldown + action_time / reload_factor_while_active) / (action_time + cooldown), against one target and with every launcher as for the launcher reload. They hold only while charges last, charges x (action_time + cooldown); then the plain rates apply. With a choice, the ship carries the consumable only in place of the slot's other options.";

const MAIN_BATTERY_BOOSTER_NOTE: &str = "The main battery alternative \"with Main Battery Reload Booster active\" is the rate while the booster runs: the main battery reload time times main_battery_reload_factor. It holds only while the booster is active; with special_ability_active it is the rate with both the special ability and the booster active.";

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

    /// Rates over a Torpedo Reload Booster cycle. Absent without the
    /// booster, or when loaders limit the launcher reload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub torpedo_reload_booster: Option<ReloadBoosterRates>,

    /// Rates over a cycle of each consumable that shortens the launcher
    /// reload while active.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub torpedo_reload_buffs: Vec<ReloadBuffRates>,

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

    pub rate: BuildStat<LaunchRate>,
}

/// Torpedoes launched per minute. With launcher facings known, against one
/// target at its best bearing; otherwise every launcher firing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LaunchRate {
    pub torpedoes_per_minute: TorpedoesPerMinute,

    /// The target bearing the rate is for, degrees off the bow, clockwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing: Option<Bearing>,

    /// Every launcher firing at once, a ceiling no single target sees when
    /// the launchers do not all bear on one bearing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_launchers_ceiling: Option<TorpedoesPerMinute>,

    /// The rate at standard bearings; on the base and best builds only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_bearing: Option<[BearingRow<TorpedoesPerMinute>; 9]>,
}

impl LaunchRate {
    /// `total` is every launcher firing; `by_bearing` the launchers bearing
    /// at each bearing, when the launcher facings are known.
    fn new(total: TorpedoesPerMinute, by_bearing: Option<&PerBearing<TorpedoesPerMinute>>) -> Self {
        let rounded = |rate: TorpedoesPerMinute| TorpedoesPerMinute::new(round(rate.value()));

        let Some(by_bearing) = by_bearing else {
            return Self {
                torpedoes_per_minute: rounded(total),
                bearing: None,
                all_launchers_ceiling: None,
                by_bearing: None,
            };
        };
        let bearing = by_bearing.best();

        Self {
            torpedoes_per_minute: rounded(by_bearing.at(bearing)),
            bearing: Some(bearing),
            all_launchers_ceiling: Some(rounded(total)),
            by_bearing: Some(by_bearing.table().map(|row| BearingRow {
                bearing: row.bearing,
                starboard: rounded(row.starboard),
                port: rounded(row.port),
            })),
        }
    }
}

impl StatAmount for LaunchRate {
    fn amount(self) -> f32 {
        self.torpedoes_per_minute.value()
    }
}

impl StatUnit for LaunchRate {
    const UNIT: &'static str = "torpedoes/min";
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

impl LoaderGroup {
    fn from_layout(group: &wows_ship_stats::torpedoes::LoaderGroup) -> Self {
        Self {
            direction: group.direction.as_ref().map(TubeDirection::from_launcher),
            launchers: group.launchers,
            tubes: group.tubes,
            loaders: group.loaders,
            tubes_per_reload: group.tubes_per_reload(),
        }
    }
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

/// Damage per minute and per second of one ammo. For guns whose mount
/// facings are known, against one target at its best bearing; otherwise
/// every barrel or tube firing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DamageRate {
    pub damage_per_minute: DamagePerMinute,
    pub damage_per_second: DamagePerSecond,

    /// The target bearing the rate is for, degrees off the bow, clockwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing: Option<Bearing>,

    /// Every barrel firing at once, a ceiling no single target sees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_barrels_ceiling: Option<DamagePerMinute>,

    /// The rate at standard bearings; on the base and best builds only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_bearing: Option<[BearingRow<DamagePerMinute>; 9]>,
}

impl DamageRate {
    /// `total` is every barrel firing; `by_bearing` the barrels bearing at
    /// each bearing, when the mount facings are known.
    fn new(total: DamagePerMinute, by_bearing: Option<PerBearing<DamagePerMinute>>) -> Self {
        let whole = |rate: DamagePerMinute| DamagePerMinute::new(rate.value().round());
        let per_second = |rate: DamagePerMinute| DamagePerSecond::new(round_to(rate.per_second().value(), 10.0));

        let Some(by_bearing) = by_bearing else {
            return Self {
                damage_per_minute: whole(total),
                damage_per_second: per_second(total),
                bearing: None,
                all_barrels_ceiling: None,
                by_bearing: None,
            };
        };
        let bearing = by_bearing.best();
        let rate = by_bearing.at(bearing);

        Self {
            damage_per_minute: whole(rate),
            damage_per_second: per_second(rate),
            bearing: Some(bearing),
            all_barrels_ceiling: Some(whole(total)),
            by_bearing: Some(by_bearing.table().map(|row| BearingRow {
                bearing: row.bearing,
                starboard: whole(row.starboard),
                port: whole(row.port),
            })),
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

/// The Torpedo Reload Booster on the best-fit build, and torpedo rates over
/// one booster cycle with the booster used whenever it is ready.
#[derive(Debug, Clone, Serialize)]
pub struct ReloadBoosterRates {
    pub consumable: String,

    /// "choose one of: A, B" when the booster shares its slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,

    /// Every launcher still reloading is loaded this long after
    /// activation. No skill or upgrade changes it.
    pub launchers_loaded_in_s: Seconds,

    #[serde(flatten)]
    pub cycle: ConsumableCycle,

    pub torpedoes_per_minute: BuildStat<BoosterRate>,

    /// One entry per torpedo any torpedo module carries.
    pub torpedo_damage: Vec<AmmoDamage>,
}

/// Torpedoes per minute over one booster cycle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BoosterRate {
    #[serde(flatten)]
    pub rate: LaunchRate,

    pub cycle_s: Seconds,

    /// Salvos of every tube per cycle: the booster's and those on the
    /// launcher reload.
    pub salvos_per_cycle: SalvoCount,
}

impl StatAmount for BoosterRate {
    fn amount(self) -> f32 {
        self.rate.amount()
    }
}

impl StatUnit for BoosterRate {
    const UNIT: &'static str = "torpedoes/min";
}

/// A consumable whose buff shortens the launcher reload while active, on
/// the best-fit build, and torpedo rates over one of its cycles with it
/// used whenever it is ready.
#[derive(Debug, Clone, Serialize)]
pub struct ReloadBuffRates {
    pub consumable: String,

    /// "choose one of: A, B" when the consumable shares its slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,

    /// Multiplier of the launcher reload time while the buff lasts.
    pub reload_factor_while_active: ReloadFactor,

    #[serde(flatten)]
    pub cycle: ConsumableCycle,

    pub torpedoes_per_minute: BuildStat<LaunchRate>,

    /// One entry per torpedo any torpedo module carries.
    pub torpedo_damage: Vec<AmmoDamage>,
}

/// Charges, action time, cooldown and preparation of a consumable, each on
/// its best-fit build.
#[derive(Debug, Clone, Serialize)]
pub struct ConsumableCycle {
    pub charges: BuildStat<Charges>,
    pub action_time: BuildStat<Seconds>,

    /// From the end of the action time until the consumable is ready again.
    pub cooldown: BuildStat<Seconds>,

    /// Delay at the start of a battle before the first use. Absent when
    /// zero.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preparation: Option<BuildStat<Seconds>>,
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

    /// Which mounts bear at each bearing. `None` when the hull's hardpoint
    /// facings are unknown.
    coverage: Option<Rc<MountCoverage>>,
}

impl Battery {
    /// Damage per minute of all guns and against one target per bearing,
    /// given each gun's damage per minute. `None` when no gun fires the ammo.
    fn rate(&self, per_gun: &[DamagePerMinute]) -> Option<DamageRate> {
        let total: DamagePerMinute = per_gun.iter().copied().sum();
        if total.value() <= 0.0 {
            return None;
        }

        let by_bearing = self.coverage.as_deref().map(|coverage| {
            self.guns.iter().zip(per_gun).fold(PerBearing::uniform(DamagePerMinute::new(0.0)), |sum, (gun, rate)| {
                sum + PerBearing::mount(coverage, &gun.hardpoint, *rate)
            })
        });

        Some(DamageRate::new(total, by_bearing))
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

/// Module combination layouts the extractors read, indexed like
/// [`Planner::selections`].
struct Layouts {
    main_battery: Vec<Battery>,
    secondaries: Vec<Battery>,
    torpedoes: Vec<Option<TorpedoLayout>>,

    /// Tubes reloaded per reload time by the launchers bearing at each
    /// bearing. `None` without torpedoes, for submarines, and when the
    /// hull's hardpoint facings are unknown.
    bearing_tubes: Vec<Option<PerBearing<f32>>>,
}

/// The torpedo launchers of an outcome's module combination and their
/// reload.
struct Launchers<'l> {
    layout: &'l TorpedoLayout,
    bearing_tubes: Option<&'l PerBearing<f32>>,
    reload: wowsunpack::game_params::ttx::model::Seconds,
}

impl<'l> Launchers<'l> {
    /// `None` without torpedoes.
    fn of(outcome: &Outcome, layouts: &'l Layouts) -> Option<Self> {
        Some(Self {
            layout: layouts.torpedoes[outcome.selection].as_ref()?,
            bearing_tubes: layouts.bearing_tubes[outcome.selection].as_ref(),
            reload: outcome.stats.torpedoes.as_ref()?.reload_time?,
        })
    }

    /// Torpedoes per minute with each launcher firing `salvos` per minute.
    fn launches(&self, salvos: SalvosPerMinute) -> Launches {
        Launches {
            all_launchers: self.layout.torpedoes_per_minute(salvos),
            by_bearing: self.bearing_tubes.map(|tubes| tubes.map(|tubes| salvos.torpedoes(tubes))),
        }
    }
}

/// Torpedoes per minute of every launcher, and per bearing of the launchers
/// bearing there when their facings are known.
struct Launches {
    all_launchers: TorpedoesPerMinute,
    by_bearing: Option<PerBearing<TorpedoesPerMinute>>,
}

impl Launches {
    fn rate(&self) -> LaunchRate {
        LaunchRate::new(self.all_launchers, self.by_bearing.as_ref())
    }

    /// Damage rates of a torpedo dealing `damage`.
    fn damage(&self, damage: Hp) -> DamageRate {
        let per_minute = |torpedoes: TorpedoesPerMinute| DamagePerMinute::new(torpedoes.value() * damage.value());

        DamageRate::new(
            per_minute(self.all_launchers),
            self.by_bearing.as_ref().map(|by_bearing| by_bearing.map(per_minute)),
        )
    }
}

/// A consumable slot offering a consumable of interest.
struct ConsumableSlot {
    /// GameParams ability and its variant.
    ability: String,
    variant: String,

    name: String,
    choice: Option<String>,
}

/// A special ability that may change consumable cooldowns, as a build label.
struct CooldownAbility {
    label: String,
    note: String,
    modifiers: Vec<AbilityModifiers>,
}

/// The Main Battery Reload Booster a ship can carry, as a build label.
struct MainBatteryBooster {
    label: String,
    note: String,
    reload_factor: ReloadMultiplier,
}

/// A rate with a per-bearing table, kept on the base and best builds.
trait BearingTable {
    fn drop_table(&mut self);
}

impl BearingTable for DamageRate {
    fn drop_table(&mut self) {
        self.by_bearing = None;
    }
}

impl BearingTable for LaunchRate {
    fn drop_table(&mut self) {
        self.by_bearing = None;
    }
}

impl BearingTable for BoosterRate {
    fn drop_table(&mut self) {
        self.rate.drop_table();
    }
}

impl GameData {
    pub(crate) fn rates(&self, planner: &Planner<'_>) -> Result<Rates, QueryError> {
        let ship = planner.ship();
        let layouts = self.layouts(ship, planner)?;
        let stock = planner.stock()?;

        let on_reload = |outcome: &Outcome| {
            let launchers = Launchers::of(outcome, &layouts)?;
            Some(launchers.launches(SalvosPerMinute::on_reload(launchers.reload)))
        };
        let torpedoes_per_minute = match &layouts.torpedoes[0] {
            Some(layout) => {
                let extract = |outcome: &Outcome| on_reload(outcome).map(|launches| launches.rate());
                planner.plan(&StatPlan::new("torpedoes per minute".to_owned(), Better::Higher, &extract))?.map(
                    |mut rate| {
                        tables_on_base_and_best(&mut rate);
                        TorpedoRate {
                            launchers: layout.launchers,
                            tubes: layout.tubes,
                            loader_groups: layout.groups.iter().map(LoaderGroup::from_layout).collect(),
                            tubes_per_reload: round(layout.tubes_per_reload),
                            limit: loader_limit(layout),
                            rate,
                        }
                    },
                )
            }
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
        let booster = self.main_battery_booster(planner)?;
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
                main_battery_rate(outcome, &layouts.main_battery[outcome.selection], &kind, standard, None)
            };
            let Some(mut rates) = planner.plan(&StatPlan::new(label, Better::Higher, &extract))? else {
                continue;
            };

            if let Some(booster) = &booster {
                let label = format!("main battery {kind} damage rate {}", booster.label);
                let extract = |outcome: &Outcome| {
                    let battery = &layouts.main_battery[outcome.selection];
                    main_battery_rate(outcome, battery, &kind, standard, Some(booster.reload_factor))
                };
                if let Some(active) = planner.best(&StatPlan::new(label, Better::Higher, &extract))? {
                    rates.builds.alternatives.push(BuildAlternative {
                        label: booster.label.clone(),
                        value: active.best.value,
                        assumed: active.best.assumed,
                        tier_fillers: active.best.tier_fillers,
                        note: Some(booster.note.clone()),
                    });
                }
            }
            tables_on_base_and_best(&mut rates);
            let mut ammo = AmmoDamage::new(kind.clone(), Vec::new(), damage, barrels, rates);
            if let Some(burst) = planner.burst() {
                let label = format!("main battery {kind} damage rate during a burst");
                let extract = |outcome: &Outcome| {
                    main_battery_rate(
                        outcome,
                        &layouts.main_battery[outcome.selection],
                        &kind,
                        Cadence::InBurst(burst),
                        None,
                    )
                };
                ammo.during_burst = planner.plan(&StatPlan::new(label, Better::Higher, &extract))?;
                if let Some(during) = &mut ammo.during_burst {
                    tables_on_base_and_best(during);
                }
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
            if let Some(mut rates) = planner.plan(&StatPlan::new(label, Better::Higher, &extract))? {
                tables_on_base_and_best(&mut rates);
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
        let torpedo_damage = self.torpedo_damages(planner, &layouts, &torpedoes, "damage rate", &on_reload)?;
        let torpedo_reload_booster = self.reload_booster_rates(planner, &layouts, &torpedoes)?;
        let torpedo_reload_buffs = self.reload_buff_rates(planner, &layouts, &torpedoes)?;

        let mut rate_notes = Vec::new();
        if torpedoes_per_minute.is_some() {
            rate_notes.push(TORPEDO_NOTE);
        }
        if torpedo_reload_booster.is_some() {
            rate_notes.push(BOOSTER_NOTE);
        }
        if !torpedo_reload_buffs.is_empty() {
            rate_notes.push(BUFF_NOTE);
        }
        if !(main_battery_damage.is_empty() && secondary_damage.is_empty() && torpedo_damage.is_empty()) {
            rate_notes.push(DAMAGE_NOTE);
        }
        if planner.burst().is_some() && !main_battery_damage.is_empty() {
            rate_notes.push(BURST_NOTE);
        }
        if booster.is_some() && !main_battery_damage.is_empty() {
            rate_notes.push(MAIN_BATTERY_BOOSTER_NOTE);
        }

        Ok(Rates {
            torpedoes_per_minute,
            main_battery_damage,
            secondary_damage,
            torpedo_damage,
            torpedo_reload_booster,
            torpedo_reload_buffs,
            rate_notes,
        })
    }

    fn layouts(&self, ship: &Ship, planner: &Planner<'_>) -> Result<Layouts, QueryError> {
        let coverages = self.mount_coverages(ship, planner.selections())?;
        let mut layouts = Layouts {
            main_battery: Vec::new(),
            secondaries: Vec::new(),
            torpedoes: Vec::new(),
            bearing_tubes: Vec::new(),
        };
        for (selection, coverage) in planner.selections().zip(coverages) {
            let torpedoes = TorpedoLayout::from_selection(&ship.param, selection);
            let bearing_tubes = torpedoes
                .as_ref()
                .zip(coverage.as_deref())
                .and_then(|(torpedoes, coverage)| torpedoes.tubes_per_reload_by_bearing(coverage));
            layouts.main_battery.push(battery(ship, selection, Weapon::MainBattery, coverage.as_ref().map(Rc::clone)));
            layouts.secondaries.push(battery(ship, selection, Weapon::Secondaries, coverage));
            layouts.torpedoes.push(torpedoes);
            layouts.bearing_tubes.push(bearing_tubes);
        }

        Ok(layouts)
    }

    /// Damage rates of each torpedo, with the first module combination that
    /// carries it, given the torpedoes per minute of an outcome.
    fn torpedo_damages(
        &self,
        planner: &Planner<'_>,
        layouts: &Layouts,
        torpedoes: &[(String, usize)],
        stat: &str,
        launches: &dyn Fn(&Outcome) -> Option<Launches>,
    ) -> Result<Vec<AmmoDamage>, QueryError> {
        let mut damages = Vec::new();
        for (name, selection) in torpedoes {
            let Some(layout) = &layouts.torpedoes[*selection] else {
                continue;
            };
            let outcome = planner.bare(*selection)?;
            let Some(damage) = torpedo_damage(&outcome, name) else {
                continue;
            };

            let ammo = self.projectile_name(name);
            let label = format!("{ammo} {stat}");
            let extract = |outcome: &Outcome| Some(launches(outcome)?.damage(torpedo_damage(outcome, name)?));
            let plan = StatPlan::new(label, Better::Higher, &extract).on_selection(*selection);
            if let Some(mut rates) = planner.plan(&plan)? {
                tables_on_base_and_best(&mut rates);
                let modules = planner.selection_modules(*selection).to_vec();
                damages.push(AmmoDamage::new(ammo, modules, damage, layout.tubes, rates));
            }
        }

        Ok(damages)
    }

    /// The Torpedo Reload Booster and the torpedo rates over its cycle.
    /// `None` without the booster, or when loaders limit the reload.
    fn reload_booster_rates(
        &self,
        planner: &Planner<'_>,
        layouts: &Layouts,
        torpedoes: &[(String, usize)],
    ) -> Result<Option<ReloadBoosterRates>, QueryError> {
        let is_booster = |category: &AbilityCategory| category.consumable_type_raw() == TORPEDO_RELOADER;
        let Some(slot) = self.consumable_slots(planner.ship(), &is_booster).into_iter().next() else {
            return Ok(None);
        };
        let Some(param) = self.provider().game_param_by_name(&slot.ability) else {
            return Ok(None);
        };
        let Some(category) = param.ability().and_then(|ability| ability.get_category(&slot.variant)) else {
            return Ok(None);
        };
        let Some(torpedo_reload) = category.effect_fields().get("torpedoReloadTime").copied() else {
            return Ok(None);
        };
        let ability = self.cooldown_ability(planner)?;
        let ability = ability.as_ref();

        let cycle_launches = |outcome: &Outcome| -> Option<(Launches, BoosterCycle)> {
            let launchers = Launchers::of(outcome, layouts)?;
            let booster = ReloadBooster::from_category(category, &outcome.bundle)?;
            let salvos = launchers.layout.booster_salvos(launchers.reload, &booster)?;

            Some((launchers.launches(salvos), booster.cycle(launchers.reload)))
        };
        let rate = |outcome: &Outcome| {
            cycle_launches(outcome).map(|(launches, cycle)| BoosterRate {
                rate: launches.rate(),
                cycle_s: Seconds::new(round(cycle.period.value())),
                salvos_per_cycle: cycle.salvos,
            })
        };
        let rate_plan = StatPlan::new(format!("torpedoes per minute with {}", slot.name), Better::Higher, &rate);
        let Some(mut torpedoes_per_minute) = planner.plan(&rate_plan)? else {
            return Ok(None);
        };
        let Some(cycle) = self.consumable_cycle(planner, &slot.name, category, ability)? else {
            return Ok(None);
        };

        let launches = |outcome: &Outcome| cycle_launches(outcome).map(|(launches, _)| launches);
        let stat = format!("damage rate with {}", slot.name);
        let mut torpedo_damage = self.torpedo_damages(planner, layouts, torpedoes, &stat, &launches)?;

        if let Some(ability) = ability {
            let active = planner.with_abilities_on(ability.modifiers.clone());
            push_ability_build(&mut torpedoes_per_minute, active.best(&rate_plan)?, ability);
            let active_damage = self.torpedo_damages(&active, layouts, torpedoes, &stat, &launches)?;
            for (damage, active) in torpedo_damage.iter_mut().zip(active_damage) {
                push_ability_build(&mut damage.rates, Some(active.rates.builds), ability);
            }
        }
        tables_on_base_and_best(&mut torpedoes_per_minute);
        for damage in &mut torpedo_damage {
            tables_on_base_and_best(&mut damage.rates);
        }

        Ok(Some(ReloadBoosterRates {
            consumable: slot.name,
            choice: slot.choice,
            launchers_loaded_in_s: Seconds::new(round(torpedo_reload)),
            cycle,
            torpedoes_per_minute,
            torpedo_damage,
        }))
    }

    /// Each consumable whose buff shortens the launcher reload while
    /// active, and the torpedo rates over its cycle.
    fn reload_buff_rates(
        &self,
        planner: &Planner<'_>,
        layouts: &Layouts,
        torpedoes: &[(String, usize)],
    ) -> Result<Vec<ReloadBuffRates>, QueryError> {
        if layouts.torpedoes.iter().all(Option::is_none) {
            return Ok(Vec::new());
        }

        let stock = planner.stock()?;
        let provider = self.provider();
        let is_buff =
            |category: &AbilityCategory| ReloadBuff::from_category(category, provider, &stock.bundle).is_some();
        let slots = self.consumable_slots(planner.ship(), &is_buff);
        if slots.is_empty() {
            return Ok(Vec::new());
        }

        let ability = self.cooldown_ability(planner)?;
        let ability = ability.as_ref();
        let mut buffs = Vec::new();
        for slot in slots {
            let Some(param) = provider.game_param_by_name(&slot.ability) else {
                continue;
            };
            let Some(category) = param.ability().and_then(|ability| ability.get_category(&slot.variant)) else {
                continue;
            };
            let Some(stock_buff) = ReloadBuff::from_category(category, provider, &stock.bundle) else {
                continue;
            };

            let launches = |outcome: &Outcome| {
                let launchers = Launchers::of(outcome, layouts)?;
                let buff = ReloadBuff::from_category(category, provider, &outcome.bundle)?;
                Some(launchers.launches(buff.salvos_per_minute(launchers.reload)))
            };
            let rate = |outcome: &Outcome| launches(outcome).map(|launches| launches.rate());
            let rate_plan = StatPlan::new(format!("torpedoes per minute with {}", slot.name), Better::Higher, &rate);
            let Some(mut torpedoes_per_minute) = planner.plan(&rate_plan)? else {
                continue;
            };
            let Some(cycle) = self.consumable_cycle(planner, &slot.name, category, ability)? else {
                continue;
            };

            let stat = format!("damage rate with {}", slot.name);
            let mut torpedo_damage = self.torpedo_damages(planner, layouts, torpedoes, &stat, &launches)?;

            if let Some(ability) = ability {
                let active = planner.with_abilities_on(ability.modifiers.clone());
                push_ability_build(&mut torpedoes_per_minute, active.best(&rate_plan)?, ability);
                let active_damage = self.torpedo_damages(&active, layouts, torpedoes, &stat, &launches)?;
                for (damage, active) in torpedo_damage.iter_mut().zip(active_damage) {
                    push_ability_build(&mut damage.rates, Some(active.rates.builds), ability);
                }
            }
            tables_on_base_and_best(&mut torpedoes_per_minute);
            for damage in &mut torpedo_damage {
                tables_on_base_and_best(&mut damage.rates);
            }

            buffs.push(ReloadBuffRates {
                consumable: slot.name,
                choice: slot.choice,
                reload_factor_while_active: stock_buff.reload_factor,
                cycle,
                torpedoes_per_minute,
                torpedo_damage,
            });
        }

        Ok(buffs)
    }

    /// Charges, action time, cooldown and preparation of the consumable
    /// `category`, with the cooldown under `ability` as an alternative when
    /// it changes it. `None` without a count-based action time and charges.
    fn consumable_cycle(
        &self,
        planner: &Planner<'_>,
        name: &str,
        category: &AbilityCategory,
        ability: Option<&CooldownAbility>,
    ) -> Result<Option<ConsumableCycle>, QueryError> {
        let builds = consumable_builds(planner, name, category)?;
        let (Some(action_time), Some(charges)) = (builds.action_time, builds.charges) else {
            return Ok(None);
        };

        let effective = |outcome: &Outcome| effective_consumable(category, &outcome.bundle).0;
        let cooldown_extract = |outcome: &Outcome| Some(Seconds::new(round(effective(outcome).reload_time.value())));
        let cooldown_plan = consumable_plan(name, "cooldown", Better::Lower, &cooldown_extract);
        let Some(mut cooldown) = planner.plan(&cooldown_plan)? else {
            return Ok(None);
        };
        if let Some(ability) = ability {
            let active = planner.with_abilities_on(ability.modifiers.clone());
            push_ability_build(&mut cooldown, active.best(&cooldown_plan)?, ability);
        }

        let preparation_extract = |outcome: &Outcome| {
            effective(outcome)
                .preparation_time
                .map(|time| Seconds::new(round(time.value())))
                .filter(|time| time.value() > 0.0)
        };
        let preparation = planner.plan(&consumable_plan(name, "preparation", Better::Lower, &preparation_extract))?;

        Ok(Some(ConsumableCycle { charges, action_time, cooldown, preparation }))
    }

    /// The slots offering a consumable `wanted` accepts, one entry per such
    /// consumable.
    fn consumable_slots(&self, ship: &Ship, wanted: &dyn Fn(&AbilityCategory) -> bool) -> Vec<ConsumableSlot> {
        let Some(slots) = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()) else {
            return Vec::new();
        };

        let mut found = Vec::new();
        for slot in slots {
            let mut names = Vec::new();
            let mut matches = Vec::new();
            for (ability, variant) in slot {
                let Some(param) = self.provider().game_param_by_name(ability) else {
                    continue;
                };
                let Some(category) = param.ability().and_then(|ability| ability.get_category(variant)) else {
                    continue;
                };
                let name = self.consumable_name(ability, category);
                names.push(name.clone());
                if wanted(category) {
                    matches.push((ability.clone(), variant.clone(), name));
                }
            }
            let choice = slot_choice(&names);
            found.extend(matches.into_iter().map(|(ability, variant, name)| ConsumableSlot {
                ability,
                variant,
                name,
                choice: choice.clone(),
            }));
        }

        found
    }

    /// The first Main Battery Reload Booster the ship can carry. `None`
    /// without one.
    fn main_battery_booster(&self, planner: &Planner<'_>) -> Result<Option<MainBatteryBooster>, QueryError> {
        let is_booster = |category: &AbilityCategory| main_battery_reload_factor(category).is_some();
        let Some(slot) = self.consumable_slots(planner.ship(), &is_booster).into_iter().next() else {
            return Ok(None);
        };
        let Some(category) = self
            .provider()
            .game_param_by_name(&slot.ability)
            .and_then(|param| param.ability().and_then(|ability| ability.get_category(&slot.variant)).cloned())
        else {
            return Ok(None);
        };
        let Some(reload_factor) = main_battery_reload_factor(&category) else {
            return Ok(None);
        };

        // Uses on the stock build; ship_consumables gives them on other builds.
        let stock = planner.stock()?;
        let stats = consumable_stats(slot.name.clone(), &category, &stock.bundle);
        let uses = match (stats.charges, stats.action_time_s, stats.capacity_s) {
            (Some(Charges::Limited(count)), Some(action), _) => {
                format!("{count} charges of {} s, {} s cooldown", action.value(), stats.reload_s.value())
            }
            (Some(Charges::Unlimited), Some(action), _) => {
                format!("unlimited charges of {} s, {} s cooldown", action.value(), stats.reload_s.value())
            }
            (_, _, Some(Capacity::Limited(capacity))) => format!(
                "{} s of use per battle in activations of at least {} s, {} s between activations",
                capacity.value(),
                stats.min_activation_s.map_or(0.0, |time| time.value()),
                stats.reload_s.value()
            ),
            _ => format!("{} s cooldown", stats.reload_s.value()),
        };
        let carried = match &slot.choice {
            Some(choice) => {
                format!(" Its slot offers a choice ({choice}); the ship carries it only in place of the others.")
            }
            None => String::new(),
        };
        let note = format!(
            "Only while {} is active: {uses}, stock build. It multiplies the main battery reload time by {}.{carried}",
            slot.name, reload_factor.0
        );

        Ok(Some(MainBatteryBooster { label: format!("with {} active", slot.name), note, reload_factor }))
    }

    /// The ship's special ability as a build to compare consumable cycles
    /// with, unless `planner` switches it on in every build already.
    fn cooldown_ability(&self, planner: &Planner<'_>) -> Result<Option<CooldownAbility>, QueryError> {
        if planner.has_ability() {
            return Ok(None);
        }

        let ship = planner.ship();
        let ability = match self.active_ability(ship) {
            Ok(ability) => ability,
            Err(QueryError::NoSpecialAbility { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };

        let names = ability.names.join(", ");
        let duration = ship
            .param
            .vehicle()
            .and_then(|vehicle| vehicle.ttx_components())
            .zip(ShipUpgradeSelection::stock(&ship.param).hull)
            .and_then(|(components, hull)| {
                components.combat_instructions(&hull).and_then(|instructions| instructions.duration)
            });
        let note = match duration {
            Some(duration) => format!(
                "{names} lasts {} s per use and each use needs its meter refilled, so the consumable cools down this fast only while it is active.",
                round(duration.value())
            ),
            None => format!("Only while {names} is active."),
        };

        Ok(Some(CooldownAbility { label: format!("with {names} active"), note, modifiers: ability.modifiers }))
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

/// Keeps the per-bearing table on the base and best builds and drops it
/// from the alternatives, which differ only in the headline.
fn tables_on_base_and_best<V>(stat: &mut BuildStat<V>)
where
    V: BearingTable,
{
    for alternative in &mut stat.builds.alternatives {
        alternative.value.drop_table();
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
    coverage: Option<Rc<MountCoverage>>,
) -> Battery {
    let guns =
        weapon_mounts(ship, selection, weapon).into_iter().map(|(mount, barrels)| Gun::new(mount, barrels)).collect();

    Battery { guns, coverage }
}

/// The best value of `active` as an alternative of `stat`, when it differs
/// from the best-fit build's.
fn push_ability_build<V>(stat: &mut BuildStat<V>, active: Option<Builds<V>>, ability: &CooldownAbility)
where
    V: StatAmount + Clone,
{
    let Some(active) = active else {
        return;
    };
    if !Better::Higher.improves(active.best.value.amount(), stat.builds.best.value.amount())
        && !Better::Lower.improves(active.best.value.amount(), stat.builds.best.value.amount())
    {
        return;
    }

    stat.builds.alternatives.push(BuildAlternative {
        label: ability.label.clone(),
        value: active.best.value,
        assumed: active.best.assumed,
        tier_fillers: active.best.tier_fillers,
        note: Some(ability.note.clone()),
    });
}

/// Damage of torpedo `name` on an outcome.
fn torpedo_damage(outcome: &Outcome, name: &str) -> Option<Hp> {
    outcome.stats.torpedoes.as_ref()?.torpedoes.iter().find(|torpedo| torpedo.name == name)?.damage
}

/// How the loaders of `layout` limit the rate. `None` when they do not.
fn loader_limit(layout: &TorpedoLayout) -> Option<String> {
    let mut sentences: Vec<String> = layout
        .groups
        .iter()
        .filter(|group| group.loaders.0 < group.launchers.0)
        .map(|group| {
            let launchers = match group.direction.as_ref().map(TubeDirection::from_launcher) {
                Some(direction) => format!("{} {} launchers", group.launchers.0, direction.label()),
                None => format!("{} launchers", group.launchers.0),
            };
            let verb = if group.loaders.0 == 1 { "reloads" } else { "reload" };
            format!(
                "{} of the {launchers} ({} of the {} tubes) {verb} at a time.",
                group.loaders.0,
                round(group.tubes_per_reload()),
                group.tubes.0
            )
        })
        .collect();
    if sentences.is_empty() {
        return None;
    }

    let tubes = layout.tubes.0;
    sentences.push(format!(
        "{} of the {tubes} tubes reload per reload time, not all {tubes}.",
        round(layout.tubes_per_reload)
    ));

    Some(sentences.join(" "))
}

/// `booster`, when given, multiplies the reload time while a Main Battery
/// Reload Booster is active.
fn main_battery_rate(
    outcome: &Outcome,
    battery: &Battery,
    kind: &str,
    cadence: Cadence,
    booster: Option<ReloadMultiplier>,
) -> Option<DamageRate> {
    let artillery = outcome.stats.artillery.as_ref()?;
    let factor = booster.map_or(1.0, |booster| booster.0);
    let reload = cadence.salvo_time(Seconds::new(artillery.reload_time?.value() * factor));
    let shells: Vec<&ShellStats> = artillery
        .shells
        .iter()
        .filter(|shell| shell.ammo_kind.as_deref().and_then(ammo_label).as_deref() == Some(kind))
        .collect();

    // A gun without a rated shell of this ammo adds nothing; per_gun stays
    // aligned with battery.guns.
    let per_gun: Vec<DamagePerMinute> = battery
        .guns
        .iter()
        .map(|gun| {
            let salvo = mount_salvo(&gun.ammo, gun.barrels.0, &shells).map_or(0.0, |salvo| salvo.value());
            DamagePerMinute::from_reload(salvo, reload)
        })
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
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
            .expect("dump root has a readable builds.toml");
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");

        let mut limited: Vec<String> = game
            .ships
            .iter()
            .filter(|ship| {
                TorpedoLayout::from_selection(&ship.param, &ShipUpgradeSelection::stock(&ship.param))
                    .is_some_and(|layout| layout.is_loader_limited())
            })
            .map(|ship| ship.name.clone())
            .collect();
        limited.sort();
        limited.dedup();

        assert!(limited.iter().any(|name| name == "Barfleur"), "{limited:?}");
        insta::assert_yaml_snapshot!((limited.len(), limited));
    }
}
