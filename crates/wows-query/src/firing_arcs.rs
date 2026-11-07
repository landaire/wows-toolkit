//! Which guns and torpedo tubes bear on a target, by target bearing, and what
//! one salvo and one minute of fire from them deliver.
//!
//! The target is distant: every mount sees it at the same bearing, so mount
//! positions along the hull are ignored and only each mount's facing and yaw
//! limits count.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wows_ship_stats::mounts::aiming_mounts;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::labels::TtxStat;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::ttx::model::ShipStats;
use wowsunpack::game_params::ttx::orchestration::artillery_dispersion;
use wowsunpack::game_params::ttx::render_attributions;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::HullUpgradeConfig;
use wowsunpack::game_params::types::MountPoint;

use crate::data::GameData;
use crate::error::QueryError;
use crate::fire_target::FireTarget;
use crate::fire_target::TargetRequest;
use crate::loadout::AppliedSkill;
use crate::loadout::AppliedUpgrade;
use crate::loadout::LoadoutRequest;
use crate::loadout::ResolvedLoadout;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::modules::ModuleSelection;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipSummary;
use crate::stats::StatChange;
use crate::stats::changes;

const BEARING_NOTE: &str = "Bearing is the target's direction from the bow, clockwise: 0 bow, 90 starboard, 180 stern, 270 port. Bow is 331-29, stern 151-209, starboard 30-150, port 210-330. The target is distant, so every mount sees it at the same bearing; mount positions on the hull are ignored. Each arc's salvo is at the bearing inside it where the most damage bears, nearest the arc's centre on a tie.";

const FIRE_NOTE: &str = "Fire figures assume every bearing shell hits and the guns fire at their reload. fires_per_minute = 60 / reload_s x barrels bearing x p, summed over mounts. With a target, p is the shell's fire chance times the target's fire_chance_factor (its hull's fire-node probability, lower at higher tiers, times its build's burnProb, e.g. x0.9 with Fire Prevention Expert), capped at 100%. Not applied: the fire limit (target fire_zones fires at once), fire duration and the Damage Control Party consumable.";

const NO_TARGET_NOTE: &str = "No target ship: p is the shell's fire chance alone. The target's hull and build lower it (x0.5005 on a tier 10 hull); give target_ship to apply them.";

const SUBMARINE_NOTE: &str =
    "Submarine torpedo tubes aim by firing sectors this tool does not model; torpedoes are left out.";

const HIT_NOTE: &str = "Dispersion is the main battery shell ellipse at the range. How many shells hit depends on the target's size, angle and the aim point; this tool does not estimate hits.";

/// Arguments for `ship_firing_arcs`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FiringArcsArgs {
    /// Ship name, e.g. "Des Moines".
    pub ship: String,

    /// Modules to mount instead of stock, e.g. ["Hull B"]. Omit for stock.
    pub modules: Option<Vec<String>>,

    /// Captain skills by name or acronym, e.g. ["Demolition Expert"]. They change reload and fire chance.
    pub skills: Option<Vec<String>>,

    /// Upgrades by name, e.g. ["Main Battery Modification 3"]. One per slot.
    pub upgrades: Option<Vec<String>>,

    /// Ship being fired at, e.g. "Colbert". Its hull's fire resistance and its build scale the fire chance. Omit to use the shell's fire chance alone.
    pub target_ship: Option<String>,

    /// The target's modules, e.g. ["Hull B"]. Omit for stock; an upgraded hull usually resists fire more.
    pub target_modules: Option<Vec<String>>,

    /// The target's captain skills, e.g. ["Fire Prevention Expert"].
    pub target_skills: Option<Vec<String>>,

    /// The target's upgrades, e.g. ["Damage Control System Modification 1"].
    pub target_upgrades: Option<Vec<String>>,

    /// Firing range in km for the dispersion ellipse, 0 or more; 0 gives the ellipse at the muzzle and ranges past the main battery's maximum use the maximum. Defaults to the maximum range.
    pub range_km: Option<Km>,
}

/// A target bearing off the bow in whole degrees, clockwise, 0 to 359.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bearing(u16);

impl Bearing {
    pub fn new(degrees: u16) -> Self {
        Self(degrees % 360)
    }

    pub fn degrees(self) -> u16 {
        self.0
    }

    fn all() -> impl Iterator<Item = Bearing> {
        (0..360).map(Bearing)
    }
}

impl Serialize for Bearing {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u16(self.0)
    }
}

/// Bearings from `from` clockwise to `to`, both included. Serializes as
/// "from-to", or as one number when both ends are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BearingRange {
    pub from: Bearing,
    pub to: Bearing,
}

impl Serialize for BearingRange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.from == self.to {
            self.from.serialize(serializer)
        } else {
            serializer.serialize_str(&format!("{}-{}", self.from.0, self.to.0))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Weapon {
    MainBattery,
    Secondaries,
    Torpedoes,
}

/// The four arcs a target can lie in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arc {
    Bow,
    Stern,
    Starboard,
    Port,
}

impl Arc {
    /// Bearings of the arc in clockwise order.
    fn bearings(self) -> Vec<Bearing> {
        match self {
            Self::Bow => arc(331, 29),
            Self::Stern => arc(151, 209),
            Self::Starboard => arc(30, 150),
            Self::Port => arc(210, 330),
        }
    }

    /// Degrees from `bearing` to the arc's centre.
    fn off_centre(self, bearing: Bearing) -> u16 {
        let centre = match self {
            Self::Bow => 0,
            Self::Stern => 180,
            Self::Starboard => 90,
            Self::Port => 270,
        };
        let difference = (bearing.0 + 360 - centre) % 360;

        difference.min(360 - difference)
    }
}

/// One value per arc.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerArc<T> {
    /// Less than 30 degrees off the bow: 331-29.
    pub bow: T,

    /// Less than 30 degrees off the stern: 151-209.
    pub stern: T,

    /// Starboard broadside, 30-150.
    pub starboard: T,

    /// Port broadside, 210-330.
    pub port: T,
}

impl<T> PerArc<T> {
    fn from_fn(mut value: impl FnMut(Arc) -> T) -> Self {
        Self {
            bow: value(Arc::Bow),
            stern: value(Arc::Stern),
            starboard: value(Arc::Starboard),
            port: value(Arc::Port),
        }
    }
}

/// Barrels that bear inside one arc: the most, where they do, and the fewest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArcCoverage {
    pub barrels: u32,

    /// Bearings inside the arc where that many bear. Empty when none bear.
    pub bearings: Vec<BearingRange>,

    /// Barrels at the bearing inside the arc where the fewest bear.
    pub fewest_barrels: u32,
}

/// Bearings where every barrel of a battery bears.
#[derive(Debug, Clone, PartialEq)]
pub enum AllBarrels {
    At(Vec<BearingRange>),
    Never,
}

impl Serialize for AllBarrels {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::At(ranges) => ranges.serialize(serializer),
            Self::Never => serializer.serialize_str("never"),
        }
    }
}

/// One projectile type of a battery.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Projectile {
    /// English name, e.g. "203 mm AP Mk21".
    pub name: String,

    /// GameParams projectile name.
    pub param: String,

    /// Caliber of a shell; absent for torpedoes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caliber_mm: Option<Millimeters>,

    pub damage: Hp,

    /// Reload of the guns or tubes firing it, with the loadout applied.
    pub reload_s: Seconds,

    /// Chance of one shell starting a fire on a hit, percent, with the loadout
    /// applied. Absent when it cannot start fires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_chance_pct: Option<Percent>,

    /// `fire_chance_pct` times the target hull's fire-node probability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_chance_on_target_pct: Option<Percent>,
}

/// Fires one salvo or one minute of fire can start, every shell hitting.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FireRate {
    pub fires_per_minute: f32,

    /// 1 - product of (1 - p) over the salvo's shells, percent.
    pub salvo_at_least_one_fire_pct: Percent,

    /// Sum of p over the salvo's shells.
    pub salvo_expected_fires: f32,
}

/// One salvo at the best bearing inside an arc.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArcSalvo {
    /// The bearing the figures are for; absent when nothing bears in the arc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearing: Option<Bearing>,

    /// Barrels or tubes firing.
    pub barrels: u32,

    /// Damage of every projectile in the salvo added up.
    pub damage: Hp,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire: Option<FireRate>,
}

/// One ammo type of a battery fired from every bearing barrel. Batteries are
/// never added together.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AmmoSalvo {
    /// "HE", "AP", "SAP", or a torpedo's English name.
    pub ammo: String,

    pub projectiles: Vec<Projectile>,

    pub arcs: PerArc<ArcSalvo>,

    /// Every barrel firing; absent when all barrels never bear together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_barrels: Option<ArcSalvo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BatteryArcs {
    pub weapon: Weapon,
    pub mounts: u32,

    /// Barrels, or tubes for torpedoes, on all mounts.
    pub barrels: u32,

    pub coverage: PerArc<ArcCoverage>,
    pub all_barrels_bear: AllBarrels,
    pub ammo: Vec<AmmoSalvo>,
}

/// Main battery dispersion at one range.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dispersion {
    pub range_km: Km,

    /// Horizontal semi-axis of the shell ellipse.
    pub horizontal_m: Meters,

    /// Vertical (along the line of fire) semi-axis.
    pub vertical_m: Meters,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShipFiringArcs {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Module>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<AppliedSkill>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upgrades: Vec<AppliedUpgrade>,

    /// Reload and fire chance changes the skills and upgrades make.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub loadout_changes: Vec<StatChange>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<FireTarget>,

    pub notes: Vec<&'static str>,
    pub batteries: Vec<BatteryArcs>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispersion: Option<Dispersion>,
}

/// A mount with the hull frame it turns in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OrientedMount<'a> {
    pub(crate) mount: &'a MountPoint,

    /// Yaw of the mount's hull hardpoint from the bow, clockwise.
    pub(crate) facing: Degrees,

    pub(crate) barrels: u32,
}

impl OrientedMount<'_> {
    /// Whether the mount fires at a distant target on `bearing`. Mount yaw
    /// limits are measured in the hardpoint frame, clockwise from above.
    fn bears(&self, bearing: Bearing) -> bool {
        let local = Degrees::new(f32::from(bearing.0) - self.facing.value());

        self.mount.can_fire_at(local).unwrap_or(false)
    }
}

/// One projectile as reported, and the unrounded figures fire rates use.
struct Fired {
    projectile: Projectile,
    reload: Seconds,

    /// Chance of one hit starting a fire, 0 to 1.
    p: f32,
}

/// The ammo kinds a battery fires, each with its projectiles by name.
struct Ordnance {
    kinds: Vec<(String, HashMap<String, Fired>)>,
}

/// What one mount contributes to a salvo of one ammo kind.
#[derive(Debug, Clone, Copy, Default)]
struct MountSalvo {
    barrels: u32,
    damage: f32,
    fires_per_minute: f32,
    expected_fires: f32,

    /// Product of (1 - p) over the mount's shells.
    no_fire: f32,
}

impl GameData {
    /// Guns and torpedo tubes on target per bearing, per battery, with salvo
    /// damage, fires per minute and main battery dispersion.
    pub fn ship_firing_arcs(&self, args: &FiringArcsArgs) -> Result<ShipFiringArcs, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let modules = self.resolve_modules(ship, args.modules.as_deref().unwrap_or_default())?;
        self.check_hull_fit(ship, &modules)?;
        let selection = &modules.selection;
        let request = LoadoutRequest {
            skills: args.skills.as_deref().unwrap_or_default(),
            upgrades: args.upgrades.as_deref().unwrap_or_default(),
            conditions_met: false,
            hp_percent: None,
            commander: None,
        };
        let loadout = self.resolve_loadout(ship, &request, &mut interpretations)?;
        let target = match args.target_ship.as_deref() {
            Some(name) => {
                let request = TargetRequest {
                    ship: name,
                    modules: args.target_modules.as_deref().unwrap_or_default(),
                    skills: args.target_skills.as_deref().unwrap_or_default(),
                    upgrades: args.target_upgrades.as_deref().unwrap_or_default(),
                };
                Some(self.fire_target(request, &mut interpretations)?)
            }
            None => {
                if args.target_modules.is_some() || args.target_skills.is_some() || args.target_upgrades.is_some() {
                    return Err(QueryError::InvalidArguments {
                        reason: "target_modules, target_skills and target_upgrades need target_ship",
                    });
                }
                None
            }
        };

        let facings = self.hardpoint_facings(ship, selection)?;
        let card = self.card_with(ship, &loadout, selection)?;
        let loadout_changes = if loadout.is_empty() {
            Vec::new()
        } else {
            self.reload_and_fire_changes(ship, &loadout, selection, &card)?
        };

        let submarine = ship.class == ShipClass::Submarine;
        let mut batteries = Vec::new();
        for weapon in [Weapon::MainBattery, Weapon::Secondaries, Weapon::Torpedoes] {
            if submarine && weapon == Weapon::Torpedoes {
                continue;
            }
            let oriented = oriented_mounts(ship, selection, weapon, &facings)?;
            if oriented.is_empty() {
                continue;
            }

            let ordnance = self.ordnance(&oriented, &card.stats, weapon, target.as_ref());
            batteries.push(battery_arcs(weapon, &oriented, &ordnance));
        }

        let dispersion = self.dispersion(ship, &loadout, selection, args.range_km)?;
        let mut notes = vec![BEARING_NOTE, FIRE_NOTE];
        if target.is_none() {
            notes.push(NO_TARGET_NOTE);
        }
        if submarine {
            notes.push(SUBMARINE_NOTE);
        }
        if dispersion.is_some() {
            notes.push(HIT_NOTE);
        }

        Ok(ShipFiringArcs {
            interpretations,
            ship: ship.summary(),
            modules: modules.chosen,
            skills: loadout.applied_skills,
            upgrades: loadout.applied_upgrades,
            loadout_changes,
            target,
            notes,
            batteries,
            dispersion,
        })
    }

    /// Rejects guns or torpedoes the selected hull cannot mount, which would
    /// leave no mounts for the card's projectiles. As in stat_search, data
    /// without component links places no limit.
    pub(crate) fn check_hull_fit(&self, ship: &Ship, modules: &ModuleSelection) -> Result<(), QueryError> {
        let selection = &modules.selection;
        let Some(vehicle) = ship.param.vehicle() else {
            return Ok(());
        };
        let Some(components) = vehicle.ttx_components() else {
            return Ok(());
        };
        let Some(hull_param) = selection.hull.as_deref() else {
            return Ok(());
        };
        let Some(hull) = vehicle.hull_upgrade(hull_param) else {
            return Ok(());
        };
        let fits = |param: &str, hull: &HullUpgradeConfig, component_type: ComponentType| {
            components.upgrade_components(param).is_empty()
                || components.component_on_hull(param, hull, component_type).is_some()
        };

        let ship_modules = self.ship_modules(ship);
        let hull_name = ship_modules
            .iter()
            .find(|module| module.kind == ModuleKind::Hull && module.param == hull_param)
            .map_or_else(|| hull_param.to_owned(), |module| module.name.clone());
        let slots = [
            (ModuleKind::Guns, ComponentType::Artillery, selection.artillery.as_deref()),
            (ModuleKind::Torpedoes, ComponentType::Torpedoes, selection.torpedoes.as_deref()),
        ];
        for (kind, component_type, param) in slots {
            let Some(param) = param else {
                continue;
            };
            if fits(param, hull, component_type) {
                continue;
            }

            if let Some(module) = modules.chosen.iter().find(|module| module.param == param) {
                let hulls = ship_modules
                    .iter()
                    .filter(|other| other.kind == ModuleKind::Hull)
                    .filter(|other| {
                        vehicle.hull_upgrade(&other.param).is_some_and(|other| fits(param, other, component_type))
                    })
                    .map(|other| other.name.clone())
                    .collect();
                return Err(QueryError::ModuleNeedsHull {
                    ship: ship.name.clone(),
                    module: module.name.clone(),
                    hull: hull_name,
                    hulls,
                });
            }

            let fitting = ship_modules
                .iter()
                .filter(|other| other.kind == kind && fits(&other.param, hull, component_type))
                .map(|other| other.name.clone())
                .collect();
            return Err(QueryError::HullNeedsModule {
                ship: ship.name.clone(),
                hull: hull_name,
                kind: kind.label(),
                modules: fitting,
            });
        }

        Ok(())
    }

    /// Hardpoint name to the yaw it faces, for the selected hull.
    pub(crate) fn hardpoint_facings(
        &self,
        ship: &Ship,
        selection: &ShipUpgradeSelection,
    ) -> Result<HashMap<String, Degrees>, QueryError> {
        let unavailable = || QueryError::HullFacingsUnavailable { ship: ship.name.clone() };
        let armor = self.vehicle_armor(ship)?.ok_or_else(unavailable)?;
        let hull = armor.hull_upgrade(selection.hull.as_deref()).ok_or_else(unavailable)?;
        let model = armor.hull_models.get(hull.hull_model.0).ok_or_else(unavailable)?;

        Ok(model.hardpoints.iter().map(|hardpoint| (hardpoint.name.clone(), hardpoint.yaw())).collect())
    }

    fn reload_and_fire_changes(
        &self,
        ship: &Ship,
        loadout: &ResolvedLoadout,
        selection: &ShipUpgradeSelection,
        card: &crate::stats::Card,
    ) -> Result<Vec<StatChange>, QueryError> {
        let baseline = self.card_with(ship, &self.stock_loadout(), selection)?;
        let rows = card.stats.rows();
        let attributions = render_attributions(&card.provenance, &rows, self.provider(), self.provider());

        Ok(changes(&baseline.stats, &card.stats, &attributions, loadout, self, |stat| {
            matches!(
                stat,
                TtxStat::ArtilleryReloadTime
                    | TtxStat::SecondaryReloadTime
                    | TtxStat::TorpedoReloadTime
                    | TtxStat::ShellBurnChance
                    | TtxStat::SecondaryShellBurnChance
            )
        }))
    }

    fn ordnance(
        &self,
        mounts: &[OrientedMount<'_>],
        stats: &ShipStats,
        weapon: Weapon,
        target: Option<&FireTarget>,
    ) -> Ordnance {
        // Damage, reload and fire chance per projectile name, from the card.
        struct CardEntry {
            damage: Hp,
            reload: Option<Seconds>,
            fire_chance: Option<Percent>,
        }
        let card: HashMap<&str, CardEntry> = match weapon {
            Weapon::MainBattery => stats
                .artillery
                .iter()
                .flat_map(|artillery| {
                    let reload = artillery.reload_time.map(|reload| Seconds::new(reload.value()));
                    artillery.shells.iter().filter_map(move |shell| {
                        Some((
                            shell.name.as_str(),
                            CardEntry {
                                damage: shell.damage?,
                                reload,
                                fire_chance: shell.burn_chance.map(|chance| Percent::new(chance.value())),
                            },
                        ))
                    })
                })
                .collect(),
            Weapon::Secondaries => stats
                .secondaries
                .iter()
                .flat_map(|secondaries| secondaries.mounts.iter())
                .filter_map(|mount| {
                    Some((
                        mount.shell.name.as_str(),
                        CardEntry {
                            damage: mount.shell.damage?,
                            reload: Some(Seconds::new(mount.reload_time.value())),
                            fire_chance: mount.shell.burn_chance.map(|chance| Percent::new(chance.value())),
                        },
                    ))
                })
                .collect(),
            Weapon::Torpedoes => stats
                .torpedoes
                .iter()
                .flat_map(|torpedoes| {
                    let reload = torpedoes.reload_time.map(|reload| Seconds::new(reload.value()));
                    torpedoes.torpedoes.iter().filter_map(move |torpedo| {
                        Some((torpedo.name.as_str(), CardEntry { damage: torpedo.damage?, reload, fire_chance: None }))
                    })
                })
                .collect(),
        };

        let mut kinds: Vec<(String, HashMap<String, Fired>)> = Vec::new();
        for oriented in mounts {
            for name in oriented.mount.ammo() {
                let Some(entry) = card.get(name.as_str()) else {
                    continue;
                };
                let Some(reload) = entry.reload else {
                    continue;
                };
                let Some(param) = self.provider().game_param_by_name(name) else {
                    continue;
                };
                let Some(projectile) = param.projectile() else {
                    continue;
                };

                let projectile_name = self.projectile_name(name);
                let (label, caliber) = match weapon {
                    Weapon::Torpedoes => (projectile_name.clone(), None),
                    Weapon::MainBattery | Weapon::Secondaries => {
                        let label = match AmmoType::from_game_str(projectile.ammo_type()) {
                            AmmoType::AP => "AP",
                            AmmoType::HE => "HE",
                            AmmoType::SAP => "SAP",
                            AmmoType::Unknown(_) => continue,
                        };
                        let caliber = projectile
                            .bullet_diametr()
                            .map(|meters| Millimeters::new(round_to(Meters::new(meters).to_mm().value(), 10.0)));
                        (label.to_owned(), caliber)
                    }
                };

                let fire_chance = entry.fire_chance.filter(|chance| chance.value() > 0.0);
                let on_target = fire_chance.zip(target).map(|(chance, target)| target.chance_against(chance));
                let fired = Fired {
                    projectile: Projectile {
                        name: projectile_name,
                        param: name.clone(),
                        caliber_mm: caliber,
                        damage: entry.damage,
                        reload_s: Seconds::new(round_to(reload.value(), 100.0)),
                        fire_chance_pct: fire_chance.map(|chance| Percent::new(round_to(chance.value(), 100.0))),
                        fire_chance_on_target_pct: on_target
                            .map(|chance| Percent::new(round_to(chance.value(), 100.0))),
                    },
                    reload,
                    // A shell without a fire chance cannot start fires.
                    p: on_target.or(fire_chance).map_or(0.0, Percent::to_fraction),
                };

                match kinds.iter_mut().find(|(kind, _)| *kind == label) {
                    Some((_, entry)) => {
                        entry.insert(name.clone(), fired);
                    }
                    None => kinds.push((label, HashMap::from([(name.clone(), fired)]))),
                }
            }
        }
        kinds.sort_by(|left, right| left.0.cmp(&right.0));

        Ordnance { kinds }
    }

    fn dispersion(
        &self,
        ship: &Ship,
        loadout: &ResolvedLoadout,
        selection: &ShipUpgradeSelection,
        range_km: Option<Km>,
    ) -> Result<Option<Dispersion>, QueryError> {
        if let Some(range) = range_km
            && (range.value().is_nan() || range.value() < 0.0)
        {
            return Err(QueryError::InvalidArguments { reason: "range_km must be 0 or more" });
        }

        let modifiers = self.effective_modifiers(ship, loadout)?.build.modifiers;
        let Some(profile) = artillery_dispersion(&ship.param, selection, modifiers.bundle()) else {
            return Ok(None);
        };
        let range = range_km.unwrap_or(profile.max_range);
        let ellipse = profile.ellipse_at(range);
        let range = range.value().min(profile.max_range.value());

        Ok(Some(Dispersion {
            range_km: Km::new(round_to(range, 100.0)),
            horizontal_m: Meters::new(round_to(ellipse.horizontal.to_meters().value(), 10.0)),
            vertical_m: Meters::new(round_to(ellipse.vertical.to_meters().value(), 10.0)),
        }))
    }
}

/// The aiming mounts of one weapon for the selected hull and modules, with
/// their barrel counts.
pub(crate) fn weapon_mounts<'a>(
    ship: &'a Ship,
    selection: &ShipUpgradeSelection,
    weapon: Weapon,
) -> Vec<(&'a MountPoint, u32)> {
    let component_type = match weapon {
        Weapon::MainBattery => ComponentType::Artillery,
        Weapon::Secondaries => ComponentType::Atba,
        Weapon::Torpedoes => ComponentType::Torpedoes,
    };

    aiming_mounts(&ship.param, selection, component_type)
}

/// The aiming mounts of one weapon, each with the facing of its hardpoint.
pub(crate) fn oriented_mounts<'a>(
    ship: &'a Ship,
    selection: &ShipUpgradeSelection,
    weapon: Weapon,
    facings: &HashMap<String, Degrees>,
) -> Result<Vec<OrientedMount<'a>>, QueryError> {
    weapon_mounts(ship, selection, weapon)
        .into_iter()
        .map(|(mount, barrels)| {
            let facing = facings.get(mount.hp_name()).copied().ok_or_else(|| QueryError::MountFacingMissing {
                ship: ship.name.clone(),
                hardpoint: mount.hp_name().to_owned(),
            })?;

            Ok(OrientedMount { mount, facing, barrels })
        })
        .collect()
}

/// Per arc, each distinct set of mounts that bear together at one bearing
/// inside it, as indices into `mounts`.
pub(crate) fn bearing_sets(mounts: &[OrientedMount<'_>]) -> PerArc<Vec<Vec<usize>>> {
    PerArc::from_fn(|arc| {
        let mut sets: Vec<Vec<usize>> = Vec::new();
        for bearing in arc.bearings() {
            let set: Vec<usize> = (0..mounts.len()).filter(|index| mounts[*index].bears(bearing)).collect();
            if !set.is_empty() && !sets.contains(&set) {
                sets.push(set);
            }
        }

        sets
    })
}

fn battery_arcs(weapon: Weapon, mounts: &[OrientedMount<'_>], ordnance: &Ordnance) -> BatteryArcs {
    let total: u32 = mounts.iter().map(|mount| mount.barrels).sum();
    let on_target: Vec<u32> = Bearing::all()
        .map(|bearing| mounts.iter().filter(|mount| mount.bears(bearing)).map(|mount| mount.barrels).sum())
        .collect();

    let all: Vec<Bearing> = Bearing::all().filter(|bearing| on_target[usize::from(bearing.0)] == total).collect();
    let all_barrels_bear = if all.is_empty() { AllBarrels::Never } else { AllBarrels::At(ranges(&all)) };

    let ammo = ordnance
        .kinds
        .iter()
        .map(|(ammo, projectiles)| {
            let per_mount: Vec<MountSalvo> = mounts.iter().map(|mount| mount_salvo(mount, projectiles)).collect();
            let at = |bearing: Bearing| -> ArcSalvo {
                let bearing_mounts = mounts.iter().zip(&per_mount).filter(|(mount, _)| mount.bears(bearing));
                arc_salvo(Some(bearing), bearing_mounts.map(|(_, salvo)| *salvo))
            };

            let best = |arc: Arc| -> ArcSalvo {
                arc.bearings()
                    .into_iter()
                    .map(at)
                    .filter(|salvo| salvo.barrels > 0)
                    .reduce(|best, salvo| {
                        let nearer =
                            |salvo: &ArcSalvo| salvo.bearing.map_or(u16::MAX, |bearing| arc.off_centre(bearing));
                        let better = salvo.damage.value() > best.damage.value()
                            || (salvo.damage == best.damage && nearer(&salvo) < nearer(&best));
                        if better { salvo } else { best }
                    })
                    .unwrap_or_else(|| arc_salvo(None, std::iter::empty()))
            };

            let mut projectiles: Vec<Projectile> = projectiles.values().map(|fired| fired.projectile.clone()).collect();
            projectiles.sort_by(|left, right| left.param.cmp(&right.param));

            AmmoSalvo {
                ammo: ammo.clone(),
                projectiles,
                arcs: PerArc::from_fn(best),
                all_barrels: (!all.is_empty()).then(|| arc_salvo(None, per_mount.iter().copied())),
            }
        })
        .collect();

    BatteryArcs {
        weapon,
        mounts: mounts.len() as u32,
        barrels: total,
        coverage: PerArc::from_fn(|arc| coverage(&on_target, &arc.bearings())),
        all_barrels_bear,
        ammo,
    }
}

/// A mount's salvo of the one projectile in `projectiles` it fires.
fn mount_salvo(mount: &OrientedMount<'_>, projectiles: &HashMap<String, Fired>) -> MountSalvo {
    let Some(fired) = mount.mount.ammo().iter().find_map(|name| projectiles.get(name)) else {
        return MountSalvo { no_fire: 1.0, ..MountSalvo::default() };
    };

    let shells = mount.barrels as f32;

    MountSalvo {
        barrels: mount.barrels,
        damage: fired.projectile.damage.value() * shells,
        fires_per_minute: 60.0 / fired.reload.value() * shells * fired.p,
        expected_fires: shells * fired.p,
        no_fire: (1.0 - fired.p).powi(mount.barrels as i32),
    }
}

fn arc_salvo(bearing: Option<Bearing>, mounts: impl Iterator<Item = MountSalvo>) -> ArcSalvo {
    let total = mounts.fold(MountSalvo { no_fire: 1.0, ..MountSalvo::default() }, |sum, mount| MountSalvo {
        barrels: sum.barrels + mount.barrels,
        damage: sum.damage + mount.damage,
        fires_per_minute: sum.fires_per_minute + mount.fires_per_minute,
        expected_fires: sum.expected_fires + mount.expected_fires,
        no_fire: sum.no_fire * mount.no_fire,
    });

    let fire = (total.expected_fires > 0.0).then(|| FireRate {
        fires_per_minute: round_to(total.fires_per_minute, 100.0),
        salvo_at_least_one_fire_pct: Percent::new(round_to(Percent::from_fraction(1.0 - total.no_fire).value(), 10.0)),
        salvo_expected_fires: round_to(total.expected_fires, 100.0),
    });

    ArcSalvo { bearing, barrels: total.barrels, damage: Hp::from(total.damage.round()), fire }
}

/// Bearings from `from` clockwise to `to`, both included.
fn arc(from: u16, to: u16) -> Vec<Bearing> {
    let width = (to + 360 - from) % 360;

    (0..=width).map(|step| Bearing::new(from + step)).collect()
}

fn coverage(on_target: &[u32], arc: &[Bearing]) -> ArcCoverage {
    let at = |bearing: &Bearing| on_target[usize::from(bearing.0)];
    let barrels = arc.iter().map(at).max().unwrap_or(0);
    let fewest_barrels = arc.iter().map(at).min().unwrap_or(0);
    let peak: Vec<Bearing> = arc.iter().copied().filter(|bearing| barrels > 0 && at(bearing) == barrels).collect();

    ArcCoverage { barrels, bearings: ranges(&peak), fewest_barrels }
}

/// Collapses bearings, given in clockwise order, into ranges. A run through
/// 359 and 0 is one range.
fn ranges(bearings: &[Bearing]) -> Vec<BearingRange> {
    let mut out: Vec<BearingRange> = Vec::new();
    for bearing in bearings {
        match out.last_mut() {
            Some(range) if Bearing::new(range.to.0 + 1) == *bearing => range.to = *bearing,
            _ => out.push(BearingRange { from: *bearing, to: *bearing }),
        }
    }

    if out.len() > 1
        && let (Some(first), Some(last)) = (out.first().copied(), out.last().copied())
        && Bearing::new(last.to.0 + 1) == first.from
    {
        out.pop();
        out[0].from = last.from;
    }

    out
}

pub(crate) fn round_to(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Louisiana's port wing secondary sits at negative x and its GameParams
    /// arc runs to negative yaw; the starboard one mirrors it. Each must bear
    /// only on its own side, which fixes the sign of hardpoint yaw.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn louisiana_wing_secondaries_bear_on_their_own_side() {
        let root = PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"));
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");
        let game = GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads");
        let ship = game.resolve_ship("Louisiana", &mut Interpretations::default()).expect("Louisiana exists");
        let selection = ShipUpgradeSelection::stock(&ship.param);
        let facings = game.hardpoint_facings(ship, &selection).expect("hull facings");

        let side = |hardpoint: &str| {
            let (mount, barrels) = weapon_mounts(ship, &selection, Weapon::Secondaries)
                .into_iter()
                .find(|(mount, _)| mount.hp_name() == hardpoint)
                .expect("secondary mount");
            let oriented = OrientedMount { mount, facing: facings[hardpoint], barrels };
            (oriented.bears(Bearing(90)), oriented.bears(Bearing(270)))
        };

        assert_eq!(side("HP_AGS_1"), (false, true));
        assert_eq!(side("HP_AGS_2"), (true, false));
    }

    #[test]
    fn end_arcs_stop_short_of_the_broadside_arcs() {
        let bow = Arc::Bow.bearings();
        let starboard = Arc::Starboard.bearings();

        assert_eq!(bow.len(), 59);
        assert_eq!((bow.first(), bow.last()), (Some(&Bearing(331)), Some(&Bearing(29))));
        assert_eq!((starboard.first(), starboard.last()), (Some(&Bearing(30)), Some(&Bearing(150))));
        assert_eq!(Arc::Bow.off_centre(Bearing(331)), 29);
        assert_eq!(Arc::Port.off_centre(Bearing(300)), 30);
    }

    #[test]
    fn ranges_join_a_run_through_zero() {
        let joined = ranges(&[Bearing(0), Bearing(1), Bearing(2), Bearing(10), Bearing(358), Bearing(359)]);

        assert_eq!(
            joined,
            vec![
                BearingRange { from: Bearing(358), to: Bearing(2) },
                BearingRange { from: Bearing(10), to: Bearing(10) }
            ]
        );
    }

    #[test]
    fn salvo_fire_figures_combine_mounts() {
        let mount = MountSalvo {
            barrels: 3,
            damage: 300.0,
            fires_per_minute: 60.0 / 6.0 * 3.0 * 0.1,
            expected_fires: 0.3,
            no_fire: 0.9_f32.powi(3),
        };
        let salvo = arc_salvo(Some(Bearing(90)), [mount, mount].into_iter());

        assert_eq!(salvo.barrels, 6);
        assert_eq!(
            salvo.fire,
            Some(FireRate {
                fires_per_minute: 6.0,
                salvo_at_least_one_fire_pct: Percent::new(46.9),
                salvo_expected_fires: 0.6
            })
        );
    }
}
