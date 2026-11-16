//! Whether one main battery salvo can sink a ship: what the shooter's shell
//! does to each hull section of the target at a range and target angle, and
//! the most one salvo deals with every shell placed where it deals the most.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_ship_stats::Bearing;
use wows_ship_stats::bearing::mount_bears;
use wowsunpack::ballistics::PlateOutcome;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::armor_material::ArmorMaterial;
use wowsunpack::game_params::ttx::armor_material::MaterialZone;
use wowsunpack::game_params::ttx::components::HullHitLocation;
use wowsunpack::game_params::ttx::components::HullSectionType;
use wowsunpack::game_params::ttx::model::Hp;
use wowsunpack::game_params::types::MountPoint;

use crate::armor_trace::GunChoice;
use crate::armor_trace::GunRequest;
use crate::armor_trace::RayOutcome;
use crate::armor_trace::TracedRay;
use crate::armor_trace::WeaponModel;
use crate::armor_trace::cast_grid;
use crate::armor_trace::join_and;
use crate::armor_trace::largest_shell;
use crate::armor_trace::line_of_fire;
use crate::armor_trace::trace_ray;
use crate::builds::Better;
use crate::builds::Outcome;
use crate::builds::Planner;
use crate::builds::StatPlan;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs;
use crate::gun_module_matchups::aspect;
use crate::hit_locations::saturates;
use crate::hit_locations::section_name;
use crate::hit_locations::section_order;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Shell;
use crate::ships::Ship;

/// Share of listed damage an overpenetration deals, and a penetration once
/// the pool its section drains is spent (Wargaming wiki).
const SPENT_SHARE: f32 = 0.1;

const ASSUMPTIONS: [&str; 8] = [
    "Every shell of the salvo hits and is placed where it deals the most damage given the pools the earlier shells of the same salvo spent, one shell at a time; a section counts as reachable when at least one traced ray deals penetration damage to it. The target is undamaged when the salvo lands.",
    "No fires, flooding, detonation or HE splash; Repair Party is not used.",
    "Saturation follows ship_hit_locations and the Wargaming wiki: a penetration takes its section's share of the listed damage from the section's pool and the hull pool's share from the hull pool (16.5% each on most sections, so 33%; 100% on the citadel, which never saturates). A pool with less HP left than its share gives only what it has, so a section deals 16.5% once either pool is spent, and a penetration deals at least 10%, which it deals once both are spent. Overpenetrations deal 10% and drain only ship HP.",
    "The game data lists one share per section for every shell type, so AP, HE and SAP penetrations follow the same section rules.",
    "AP penetration damage lands in the section whose volume the fuse goes off in; HE and SAP damage lands in the section of the first plate they defeat. Armor zones with no hull section (turrets, steering gear, torpedo protection, internal hull plates) are listed in other_zones and deal no counted damage.",
    "Each shell deals its listed damage as given; no damage roll is modelled.",
    "The shooter is on stock modules and turned so that the most main battery barrels firing the shell bear together; target_angle_deg sets only the target's heading. The target is on its stock hull; best_case_with_hp_build scales every pool with ship HP.",
    "Rays 1 m apart are cast over the target along the line of fire and traced as armor_trace traces them; rays whose first armor contact is below the waterline are left out.",
];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SalvoKillArgs {
    /// Firing ship, e.g. "Piemonte".
    pub shooter: String,

    /// Ship fired at, e.g. "G. Hoffmann".
    pub target: String,

    /// Main battery shell type. Defaults to every type the shooter's stock main battery fires.
    pub ammo: Option<Ammo>,

    /// Gun range in km, 0 or more. Defaults to 10.
    pub range_km: Option<Km>,

    /// Target heading away from broadside in degrees: 0 is broadside, 30 angled, 90 bow-on with the bow toward the shooter. Defaults to 0.
    pub target_angle_deg: Option<Degrees>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SalvoKillOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub shooter: String,
    pub target: String,
    pub target_hull: String,
    pub range_km: Km,
    pub target_angle_deg: Degrees,

    /// "broadside", or the target heading away from broadside.
    pub aspect: String,
    pub target_hp: TargetHp,

    /// One entry per shell type.
    pub salvos: Vec<AmmoSalvo>,
    pub assumptions: [&'static str; 8],
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetHp {
    /// Stock hull without captain skills or upgrades.
    pub stock: Hp,

    /// The captain skills and upgrades that raise HP the most on the stock
    /// hull; absent when none do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_build: Option<HpBuild>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HpBuild {
    pub hp: Hp,
    pub assumed: Vec<String>,
}

/// One shell type fired as one salvo at the target.
#[derive(Debug, Clone, Serialize)]
pub struct AmmoSalvo {
    pub ammo: Ammo,

    /// English name of the shell.
    pub shell: String,
    pub caliber_mm: Millimeters,

    /// Listed damage of one shell; a hit deals a share of it.
    pub alpha_per_shell: Hp,

    /// Barrels firing this shell that bear together where the most do.
    pub shells_per_salvo: u32,

    /// Main battery barrels in all.
    pub main_battery_barrels: u32,

    /// AP: penetration against a plate struck head-on at range_km.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_penetration_mm: Option<Millimeters>,

    /// HE or SAP penetration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_mm: Option<Millimeters>,

    /// Traced rays above the waterline.
    pub rays: usize,

    /// Each hull section with geometry, in display order.
    pub sections: Vec<SectionEffect>,

    /// Armor zones that are no hull section.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other_zones: Vec<ZoneEffect>,

    /// Damage of one overpenetration; present when some ray overpenetrates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overpenetration_damage: Option<Hp>,

    /// Against stock HP.
    pub best_case: BestCase,

    /// Against the target's best HP build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_case_with_hp_build: Option<BestCase>,

    pub conclusion: String,
}

/// What the shell does to one hull section.
#[derive(Debug, Clone, Serialize)]
pub struct SectionEffect {
    /// English name, e.g. "bow".
    pub section: String,

    /// Key of the section in GameParams, e.g. "Bow".
    pub game_key: String,

    /// Rays that strike this section's armor first.
    pub rays: usize,

    /// Most common outcome of those rays; absent without rays.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<RayOutcome>,

    /// Whether any of those rays penetrates.
    pub penetrates_on_some_rays: bool,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub outcomes: BTreeMap<RayOutcome, usize>,

    /// Rays, wherever they strike first, whose penetration deals its damage
    /// here: AP that goes off inside this section, HE or SAP that defeats its
    /// plate.
    pub penetrations_here: usize,

    /// Damage of one penetration here while the pools it drains have HP.
    /// Absent when the game data lacks the section's share.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_damage: Option<Hp>,

    /// Once this section's own pool is spent. Present when it saturates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturated_damage: Option<Hp>,

    /// Once the hull pool this section drains is spent while its own pool
    /// has HP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_once_hull_pool_spent: Option<Hp>,

    /// Once both are spent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fully_saturated_damage: Option<Hp>,
}

/// Rays whose armor zone is no hull section.
#[derive(Debug, Clone, Serialize)]
pub struct ZoneEffect {
    /// Armor zone, e.g. "Turret"; "no zone" for AP that goes off clear of
    /// every zone's volume.
    pub zone: String,

    /// Rays that strike this zone's armor first.
    pub rays: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<RayOutcome>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub outcomes: BTreeMap<RayOutcome, usize>,

    /// Penetrations whose damage would land in this zone; not counted.
    pub penetrations_here: usize,
}

/// One salvo with every shell placed where it deals the most damage.
#[derive(Debug, Clone, Serialize)]
pub struct BestCase {
    pub ship_hp: Hp,

    /// Damage of the whole salvo.
    pub max_damage: Hp,

    /// Shells that must hit to sink the target; absent when the whole salvo
    /// cannot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shells_to_sink: Option<u32>,

    /// Where the shells land, in firing order.
    pub hits: Vec<HitGroup>,
}

/// Consecutive shells of a salvo placed alike.
#[derive(Debug, Clone, Serialize)]
pub struct HitGroup {
    pub shells: u32,

    /// Section name, or "overpenetration".
    pub lands: String,
    pub share: DamageShare,
    pub damage_per_shell: Hp,
}

/// Which share of the listed damage a hit deals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageShare {
    /// The pools the section drains have HP for their full shares.
    Full,

    /// A pool with less HP left than its share gives only what it has.
    PoolEmptied,

    /// The section's own pool is spent; the hull pool gives its share, or
    /// what it has left.
    Saturated,

    /// The hull pool is spent; the section's own pool gives its share, or
    /// what it has left.
    HullPoolSpent,

    /// The pools give less than 10%, the least a penetration deals.
    FullySaturated,

    /// The shell passes through without going off.
    Overpenetration,
}

/// A hull's sections and the pools they drain.
#[derive(Debug, Clone)]
struct DamageModel {
    sections: Vec<SectionPool>,
}

impl DamageModel {
    /// The sections of `locations` on a ship of `ship_hp`.
    fn new(locations: &[HullHitLocation], ship_hp: Hp) -> Self {
        let sections = locations
            .iter()
            .map(|location| {
                let pool = location.max_hp.filter(|_| saturates(location, Some(ship_hp))).map(|pool| pool.value());
                let parent = match location.parent.as_deref() {
                    None => Some(None),
                    Some(name) => locations.iter().position(|other| other.name == name).and_then(|index| {
                        locations[index]
                            .volume_coeff
                            .map(|share| Some(ParentDrain { section: SectionIndex(index), share }))
                    }),
                };
                let shares = location.volume_coeff.zip(parent).map(|(own, parent)| SectionShares { own, parent });

                SectionPool { location: location.clone(), pool, shares }
            })
            .collect();

        Self { sections }
    }

    /// Every pool scaled by `scale`, as ship HP modifiers scale them.
    fn scaled(&self, scale: f32) -> Self {
        let mut scaled = self.clone();
        for section in &mut scaled.sections {
            section.pool = section.pool.map(|pool| pool * scale);
        }

        scaled
    }

    fn section(&self, index: SectionIndex) -> &SectionPool {
        &self.sections[index.0]
    }

    fn indices(&self) -> impl Iterator<Item = SectionIndex> + use<> {
        (0..self.sections.len()).map(SectionIndex)
    }

    fn index_of(&self, key: &str) -> Option<SectionIndex> {
        self.sections.iter().position(|section| section.location.name == key).map(SectionIndex)
    }

    /// The place a plate of `zone` and `material` belongs to.
    fn place<'a>(&self, zone: &'a str, material: Option<ArmorMaterial>) -> Place<'a> {
        let Some(key) = section_key(zone, material) else {
            return Place::Zone(zone);
        };

        // A hull without auxiliary rooms counts their plates as the
        // superstructure, the armor zone they belong to.
        let fallback = (key == "SSC").then_some("SS");

        [Some(key), fallback]
            .into_iter()
            .flatten()
            .find_map(|key| self.index_of(key))
            .map_or(Place::Zone(zone), Place::Section)
    }

    fn pools(&self) -> Pools {
        Pools(self.sections.iter().map(|section| section.pool).collect())
    }

    /// What one shell placed as `strike` deals with `pools` left. Each pool
    /// gives its share, or what it has left when that is less; a penetration
    /// deals at least the spent share.
    fn hit(pools: &Pools, strike: Strike, alpha: f32) -> Hit {
        let Strike::Penetration { section, shares } = strike else {
            return Hit { damage: alpha * SPENT_SHARE, share: DamageShare::Overpenetration, drains: [None, None] };
        };

        let take = |index: SectionIndex, share: f32| {
            let wanted = alpha * share;
            let hp = pools.left(index).map_or(wanted, |left| wanted.min(left));

            Drain { section: index, hp }
        };
        let own = take(section, shares.own);
        let parent = shares.parent.map(|parent| take(parent.section, parent.share));
        let pooled = own.hp + parent.map_or(0.0, |drain| drain.hp);
        let drains = [Some(own), parent];

        let floor = alpha * SPENT_SHARE;
        if pooled < floor {
            return Hit { damage: floor, share: DamageShare::FullySaturated, drains };
        }

        let full = alpha * (shares.own + shares.parent.map_or(0.0, |parent| parent.share));
        let share = if pools.is_spent(section) {
            DamageShare::Saturated
        } else if shares.parent.is_some_and(|parent| pools.is_spent(parent.section)) {
            DamageShare::HullPoolSpent
        } else if pooled < full {
            DamageShare::PoolEmptied
        } else {
            DamageShare::Full
        };

        Hit { damage: pooled, share, drains }
    }

    /// Every shell placed in turn where it deals the most, given the pools
    /// the earlier shells spent. On a tie the shell goes where it drains the
    /// least pool HP that can run out, then to the earlier strike.
    fn best_case(&self, strikes: &[Strike], alpha: f32, shells: u32, ship_hp: f32) -> BestCase {
        // Damage closer than this is a tie.
        const TIE: f32 = 0.01;

        let mut pools = self.pools();
        let mut dealt = 0.0;
        let mut shells_to_sink = None;
        let mut hits: Vec<HitGroup> = Vec::new();

        for shell in 1..=shells {
            let pool_cost = |hit: &Hit| -> f32 {
                hit.drains.iter().flatten().filter(|drain| pools.saturates(drain.section)).map(|drain| drain.hp).sum()
            };
            let chosen =
                strikes.iter().map(|strike| (*strike, Self::hit(&pools, *strike, alpha))).reduce(|best, next| {
                    let better = next.1.damage > best.1.damage + TIE
                        || ((next.1.damage - best.1.damage).abs() <= TIE && pool_cost(&next.1) < pool_cost(&best.1));
                    if better { next } else { best }
                });
            let Some((strike, hit)) = chosen else {
                break;
            };

            for drain in hit.drains.iter().flatten() {
                pools.drain(*drain);
            }

            dealt += hit.damage;
            if shells_to_sink.is_none() && dealt.round() >= ship_hp {
                shells_to_sink = Some(shell);
            }

            let lands = match strike {
                Strike::Penetration { section, .. } => section_name(&self.section(section).location),
                Strike::Overpenetration => "overpenetration".to_owned(),
            };
            let damage_per_shell = whole_hp(hit.damage);
            match hits.last_mut() {
                Some(last)
                    if last.lands == lands && last.share == hit.share && last.damage_per_shell == damage_per_shell =>
                {
                    last.shells += 1;
                }
                _ => hits.push(HitGroup { shells: 1, lands, share: hit.share, damage_per_shell }),
            }
        }

        BestCase { ship_hp: Hp::from(ship_hp), max_damage: whole_hp(dealt), shells_to_sink, hits }
    }

    fn section_effect(&self, index: SectionIndex, rays: Option<&PlaceRays>, alpha: f32) -> SectionEffect {
        let section = self.section(index);
        let outcomes = rays.map(|rays| rays.outcomes.clone()).unwrap_or_default();
        let penetrates =
            [RayOutcome::Penetration, RayOutcome::Citadel].iter().any(|outcome| outcomes.contains_key(outcome));
        let shares = section.shares;
        let parent = shares.and_then(|shares| shares.parent);
        let parent_share = parent.map_or(0.0, |parent| parent.share);
        let parent_saturates = parent.is_some_and(|parent| self.section(parent.section).pool.is_some());
        let at_least_spent = |share: f32| whole_hp(alpha * share.max(SPENT_SHARE));

        SectionEffect {
            section: section_name(&section.location),
            game_key: section.location.name.clone(),
            rays: outcomes.values().sum(),
            outcome: most_common(&outcomes),
            penetrates_on_some_rays: penetrates,
            penetrations_here: rays.map_or(0, |rays| rays.penetrations),
            penetration_damage: shares.map(|shares| whole_hp(alpha * (shares.own + parent_share))),
            saturated_damage: shares.filter(|_| section.pool.is_some()).map(|_| at_least_spent(parent_share)),
            damage_once_hull_pool_spent: shares.filter(|_| parent_saturates).map(|shares| at_least_spent(shares.own)),
            fully_saturated_damage: shares
                .filter(|_| parent_saturates && section.pool.is_some())
                .map(|_| whole_hp(alpha * SPENT_SHARE)),
            outcomes,
        }
    }
}

#[derive(Debug, Clone)]
struct SectionPool {
    location: HullHitLocation,

    /// Pool HP; absent when it never saturates.
    pool: Option<f32>,

    /// Absent when the game data lacks a share this section needs.
    shares: Option<SectionShares>,
}

impl SectionPool {
    fn has_geometry(&self) -> bool {
        self.location.section_type != HullSectionType::Hull
    }
}

/// Position of a section in its hull's hit locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SectionIndex(usize);

/// HP left in each section's pool during one salvo; `None` for a pool that
/// never saturates.
#[derive(Debug, Clone)]
struct Pools(Vec<Option<f32>>);

impl Pools {
    fn left(&self, index: SectionIndex) -> Option<f32> {
        self.0[index.0]
    }

    fn saturates(&self, index: SectionIndex) -> bool {
        self.left(index).is_some()
    }

    fn is_spent(&self, index: SectionIndex) -> bool {
        self.left(index).is_some_and(|hp| hp <= 0.0)
    }

    fn drain(&mut self, drain: Drain) {
        if let Some(pool) = self.0[drain.section.0].as_mut() {
            *pool = (*pool - drain.hp).max(0.0);
        }
    }
}

/// Shares of the listed damage one penetration takes from each pool.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SectionShares {
    own: f32,
    parent: Option<ParentDrain>,
}

/// The pool a section also drains, and the share it takes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ParentDrain {
    section: SectionIndex,
    share: f32,
}

/// What one shell may be placed to do.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Strike {
    Penetration { section: SectionIndex, shares: SectionShares },
    Overpenetration,
}

/// One shell's damage and the pools it drains.
#[derive(Debug, Clone, Copy)]
struct Hit {
    damage: f32,
    share: DamageShare,
    drains: [Option<Drain>; 2],
}

/// HP one hit takes from one pool.
#[derive(Debug, Clone, Copy)]
struct Drain {
    section: SectionIndex,
    hp: f32,
}

/// Where a ray's shell strikes first, or where its penetration deals damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Place<'a> {
    Section(SectionIndex),

    /// An armor zone that is no hull section, such as "Turret".
    Zone(&'a str),

    /// AP that went off clear of every zone's volume.
    NoZone,
}

#[derive(Debug, Clone, Default)]
struct PlaceRays {
    /// Outcomes of the rays striking here first.
    outcomes: BTreeMap<RayOutcome, usize>,

    /// Penetrations, wherever they struck first, whose damage lands here.
    penetrations: usize,
}

/// The target side shared by every shell type.
struct Target<'a> {
    ship: &'a Ship,
    meshes: &'a [InteractiveArmorMesh],
    model: &'a DamageModel,
    stock_hp: Hp,
    build: Option<&'a HpBuild>,
}

/// The firing side of one shell type.
struct Firing<'a> {
    ship: &'a Ship,
    selection: &'a ShipUpgradeSelection,
    module: &'a Module,
    shell: &'a Shell,
}

/// Barrels firing one shell.
#[derive(Debug, Clone, Copy)]
struct BarrelCount {
    /// Those that bear together at the bearing where the most do.
    bearing: u32,

    /// Main battery barrels in all.
    total: u32,
}

/// A mount firing the shell, with the facing of its hardpoint.
struct BearingMount<'a> {
    mount: &'a MountPoint,
    facing: Degrees,
    barrels: u32,
}

/// The facts one shell type's conclusion states.
struct ConclusionFacts<'a> {
    shooter: &'a Ship,
    target: &'a Ship,
    ammo: Ammo,
    aspect: &'a str,
    range: Km,
    shells: u32,
    best_case: &'a BestCase,
    build: Option<(&'a HpBuild, &'a BestCase)>,
}

impl GameData {
    /// What one main battery salvo of `args.shooter` does to `args.target`:
    /// per hull section what the shell does there, and the most one salvo
    /// deals with every shell placed where it deals the most.
    pub fn salvo_kill(&self, args: &SalvoKillArgs) -> Result<SalvoKillOutput, QueryError> {
        const DEFAULT_RANGE: Km = Km::new(10.0);

        let mut interpretations = Interpretations::default();
        let shooter = self.resolve_ship(&args.shooter, &mut interpretations)?;
        let target = self.resolve_ship(&args.target, &mut interpretations)?;

        let range = args.range_km.unwrap_or(DEFAULT_RANGE);
        let angle = args.target_angle_deg.unwrap_or(Degrees::new(0.0));
        if !(-180.0..=180.0).contains(&angle.value()) {
            return Err(QueryError::InvalidArguments { reason: "target_angle_deg must be between -180 and 180" });
        }

        let selection = ShipUpgradeSelection::stock(&shooter.param);
        let Some(module) = selection.artillery.as_deref().and_then(|param| {
            self.ship_modules(shooter)
                .into_iter()
                .find(|module| module.kind == ModuleKind::Guns && module.param == param)
        }) else {
            return Err(QueryError::WeaponNotFound { ship: shooter.name.clone(), weapon: "main battery".to_owned() });
        };

        let module_shells = self.gun_module_shells(shooter, &module.param);
        let fired: BTreeSet<Ammo> = module_shells.iter().map(|shell| shell.ammo).collect();
        if fired.is_empty() {
            return Err(QueryError::ModuleShellsMissing { ship: shooter.name.clone(), module: module.name.clone() });
        }

        let ammo: Vec<Ammo> = match args.ammo {
            None => fired.into_iter().collect(),
            Some(ammo) if fired.contains(&ammo) => vec![ammo],
            Some(ammo) => {
                return Err(QueryError::ModuleAmmoMissing {
                    ship: shooter.name.clone(),
                    module: module.name.clone(),
                    ammo,
                    fired: fired.into_iter().collect(),
                });
            }
        };

        let hull = self.target_hull(target, None)?;
        let components = target.param.vehicle().and_then(|vehicle| vehicle.ttx_components());
        let Some(hull_stats) = components.and_then(|components| components.hull(&hull.param)) else {
            return Err(QueryError::HullSectionsMissing { ship: target.name.clone(), hull: hull.name });
        };

        if hull_stats.hit_locations.is_empty() {
            return Err(QueryError::HullSectionsMissing { ship: target.name.clone(), hull: hull.name });
        }

        let Some(stock_hp) = hull_stats.health else {
            return Err(QueryError::HullHpMissing { ship: target.name.clone(), hull: hull.name });
        };

        let model = DamageModel::new(&hull_stats.hit_locations, stock_hp);
        let best_build = self.best_hp_build(target)?;
        let meshes = self.target_meshes(target, &hull.param)?;
        let target_side = Target { ship: target, meshes: &meshes, model: &model, stock_hp, build: best_build.as_ref() };
        let aspect = aspect(angle);

        let mut salvos = Vec::with_capacity(ammo.len());
        for ammo in ammo {
            let Some(shell) = largest_shell(&module_shells, ammo) else {
                continue;
            };

            let firing = Firing { ship: shooter, selection: &selection, module: &module, shell };
            salvos.push(self.ammo_salvo(&firing, &target_side, range, angle, &aspect)?);
        }

        Ok(SalvoKillOutput {
            interpretations,
            shooter: shooter.label(),
            target: target.label(),
            target_hull: hull.name,
            range_km: range,
            target_angle_deg: angle,
            aspect,
            target_hp: TargetHp { stock: stock_hp, best_build },
            salvos,
            assumptions: ASSUMPTIONS,
        })
    }

    fn ammo_salvo(
        &self,
        firing: &Firing,
        target: &Target,
        range: Km,
        angle: Degrees,
        aspect: &str,
    ) -> Result<AmmoSalvo, QueryError> {
        // Ray spacing of armor_trace's default.
        const TRACE_SPACING: Meters = Meters::new(1.0);

        let shell = firing.shell;
        let Some(alpha) = shell.alpha_damage else {
            return Err(QueryError::ShellDamageMissing { projectile: shell.name.clone() });
        };

        let barrels = self.bearing_barrels(firing)?;
        let shells = barrels.bearing;

        let request = GunRequest {
            guns: GunChoice::MainModule(firing.module.param.clone()),
            ammo: shell.ammo,
            range_km: Some(range),
            ifhe: false,
        };
        let weapon = self.gun_weapon(firing.ship, &request)?;
        let grid = cast_grid(target.meshes, line_of_fire(angle, weapon.arrival.fall_angle_deg), TRACE_SPACING)?;
        let traced: Vec<TracedRay> = grid
            .rays
            .into_iter()
            .filter(|ray| !ray.underwater)
            .map(|ray| trace_ray(&weapon.model, ray.plates))
            .collect();
        let places = tally(&traced, &weapon.model, target.model);

        let model = target.model;
        let mut ordered: Vec<SectionIndex> =
            model.indices().filter(|index| model.section(*index).has_geometry()).collect();
        ordered.sort_by(|left, right| {
            section_order(&model.section(*left).location).cmp(&section_order(&model.section(*right).location))
        });

        let mut strikes: Vec<Strike> = ordered
            .iter()
            .filter(|index| places.get(&Place::Section(**index)).is_some_and(|rays| rays.penetrations > 0))
            .filter_map(|index| {
                model.section(*index).shares.map(|shares| Strike::Penetration { section: *index, shares })
            })
            .collect();
        let overpenetrations: usize =
            places.values().filter_map(|rays| rays.outcomes.get(&RayOutcome::Overpenetration)).sum();
        if overpenetrations > 0 {
            strikes.push(Strike::Overpenetration);
        }

        let sections = ordered
            .iter()
            .map(|index| model.section_effect(*index, places.get(&Place::Section(*index)), alpha))
            .collect();
        let other_zones = places
            .iter()
            .filter_map(|(place, rays)| {
                let zone = match place {
                    Place::Section(_) => return None,
                    Place::Zone(zone) => (*zone).to_owned(),
                    Place::NoZone => "no zone".to_owned(),
                };

                Some(ZoneEffect {
                    zone,
                    rays: rays.outcomes.values().sum(),
                    outcome: most_common(&rays.outcomes),
                    outcomes: rays.outcomes.clone(),
                    penetrations_here: rays.penetrations,
                })
            })
            .collect();

        let best_case = model.best_case(&strikes, alpha, shells, target.stock_hp.value());
        let best_case_with_hp_build = target.build.map(|build| {
            let scaled = model.scaled(build.hp.value() / target.stock_hp.value());
            scaled.best_case(&strikes, alpha, shells, build.hp.value())
        });

        let conclusion = conclusion(&ConclusionFacts {
            shooter: firing.ship,
            target: target.ship,
            ammo: shell.ammo,
            aspect,
            range,
            shells,
            best_case: &best_case,
            build: target.build.zip(best_case_with_hp_build.as_ref()),
        });

        Ok(AmmoSalvo {
            ammo: shell.ammo,
            shell: shell.name.clone(),
            caliber_mm: shell.caliber_mm,
            alpha_per_shell: Hp::from(alpha),
            shells_per_salvo: shells,
            main_battery_barrels: barrels.total,
            raw_penetration_mm: weapon.arrival.raw_penetration_mm,
            penetration_mm: shell.penetration_mm,
            rays: traced.len(),
            sections,
            other_zones,
            overpenetration_damage: (overpenetrations > 0).then(|| whole_hp(alpha * SPENT_SHARE)),
            best_case,
            best_case_with_hp_build,
            conclusion,
        })
    }

    /// Barrels firing the shell that bear together at the bearing where the
    /// most do, and main battery barrels in all.
    fn bearing_barrels(&self, firing: &Firing) -> Result<BarrelCount, QueryError> {
        let facings = self.hardpoint_facings(firing.ship, firing.selection)?;
        let mounts = firing_arcs::weapon_mounts(firing.ship, firing.selection, firing_arcs::Weapon::MainBattery);
        let total: u32 = mounts.iter().map(|(_, barrels)| barrels).sum();
        let mut firing_mounts = Vec::with_capacity(mounts.len());

        for (mount, barrels) in mounts {
            if !mount.ammo().contains(&firing.shell.param) {
                continue;
            }

            let Some(facing) = facings.get(mount.hp_name()).copied() else {
                return Err(QueryError::MountFacingMissing {
                    ship: firing.ship.name.clone(),
                    hardpoint: mount.hp_name().to_owned(),
                });
            };

            firing_mounts.push(BearingMount { mount, facing, barrels });
        }

        let bearing = Bearing::all()
            .map(|bearing| {
                firing_mounts
                    .iter()
                    .filter(|firing| mount_bears(firing.mount, firing.facing, bearing))
                    .map(|firing| firing.barrels)
                    .sum::<u32>()
            })
            .max()
            .unwrap_or(0);
        if bearing == 0 {
            return Err(QueryError::WeaponNotFound {
                ship: firing.ship.name.clone(),
                weapon: format!("main battery {}", firing.shell.name),
            });
        }

        Ok(BarrelCount { bearing, total })
    }

    /// The target's HP with the captain skills and upgrades that raise it
    /// most on its stock hull; `None` when none do.
    fn best_hp_build(&self, target: &Ship) -> Result<Option<HpBuild>, QueryError> {
        let planner = Planner::new(self, target);
        let extract = |outcome: &Outcome| outcome.stats.durability.as_ref()?.health;
        let plan = StatPlan {
            modules: false,
            triggered: false,
            ..StatPlan::new("health".to_owned(), Better::Higher, &extract)
        };
        let Some(builds) = planner.best(&plan)? else {
            return Ok(None);
        };

        if builds.best.value.value() <= builds.base.value() {
            return Ok(None);
        }

        Ok(Some(HpBuild { hp: builds.best.value, assumed: builds.best.assumed }))
    }
}

/// Each ray counted where it strikes first and, when it penetrates, where
/// its damage lands.
fn tally<'a>(rays: &[TracedRay<'a>], weapon: &WeaponModel, model: &DamageModel) -> BTreeMap<Place<'a>, PlaceRays> {
    let mut places: BTreeMap<Place<'a>, PlaceRays> = BTreeMap::new();

    for ray in rays {
        let first = &ray.plates[0];
        let landing = model.place(first.zone, Some(first.material));
        *places.entry(landing).or_default().outcomes.entry(ray.outcome).or_default() += 1;

        if !matches!(ray.outcome, RayOutcome::Penetration | RayOutcome::Citadel) {
            continue;
        }

        let damaged = match weapon {
            WeaponModel::ArmorPiercing { .. } => detonation_place(ray, model),
            WeaponModel::FirstPlate { .. } => landing,
        };
        places.entry(damaged).or_default().penetrations += 1;
    }

    places
}

/// The place an AP shell's fuse went off in, telling the auxiliary rooms
/// from the superstructure by the plate the shell entered through.
fn detonation_place<'a>(ray: &TracedRay<'a>, model: &DamageModel) -> Place<'a> {
    let (Some(zone), Some(sim)) = (ray.detonated_in, ray.sim.as_ref()) else {
        return Place::NoZone;
    };

    let Some(detonated) = sim.detonated_at else {
        return Place::NoZone;
    };

    // A shell stopped by a ricochet or shatter goes off without crossing that
    // plate, as ShellSimResult::detonation_zone reads it.
    let stopped_short = sim.stopped_at == Some(detonated)
        && sim
            .plates
            .get(detonated.value())
            .is_some_and(|plate| matches!(plate.outcome, PlateOutcome::Ricochet | PlateOutcome::Shatter));
    let crossed = if stopped_short { detonated.value() } else { detonated.number() };
    let entered = ray.plates[..crossed.min(ray.plates.len())].iter().rev().find(|plate| plate.zone == zone);

    model.place(zone, entered.map(|plate| plate.material))
}

/// The GameParams hit location key of an armor zone. Armor models fold the
/// auxiliary rooms into the superstructure zone; their plates' materials
/// tell them apart. Turrets, steering gear, torpedo protection and internal
/// hull plates are no hull section.
fn section_key(zone: &str, material: Option<ArmorMaterial>) -> Option<&'static str> {
    match zone {
        "Bow" => Some("Bow"),
        "Stern" => Some("St"),
        "Casemate" => Some("Cas"),
        "Citadel" => Some("Cit"),
        "Superstructure" if material.is_some_and(|material| material.zone() == MaterialZone::AuxiliaryRooms) => {
            Some("SSC")
        }
        "Superstructure" => Some("SS"),
        _ => None,
    }
}

/// The most common outcome, the first in `RayOutcome` order on a tie.
fn most_common(outcomes: &BTreeMap<RayOutcome, usize>) -> Option<RayOutcome> {
    outcomes
        .iter()
        .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(outcome, _)| *outcome)
}

/// "At 10 km against a broadside G. Hoffmann (24500 HP), one SAP salvo of 12
/// shells from Piemonte deals at most 29700 damage if every shell hits where
/// it deals the most, so it can sink it with 10 of 12 shells hitting."
fn conclusion(facts: &ConclusionFacts) -> String {
    let ConclusionFacts { shooter, target, ammo, aspect, range, shells, best_case, build } = facts;
    let ammo = ammo.label();
    let shooter = &shooter.name;
    let target = &target.name;
    let range = range.value();

    let opening = format!(
        "At {range} km against a {aspect} {target} ({} HP), one {ammo} salvo of {shells} shells from {shooter} deals at most {} damage if every shell hits where it deals the most",
        best_case.ship_hp, best_case.max_damage
    );
    let verdict = match best_case.shells_to_sink {
        Some(needed) => format!("so it can sink it with {needed} of {shells} shells hitting."),
        None => format!(
            "so it cannot sink it: at most {} of {} HP even if every shell hits.",
            best_case.max_damage, best_case.ship_hp
        ),
    };
    let with_build = build.map_or(String::new(), |(build, case)| {
        let skills = join_and(&build.assumed);
        match case.shells_to_sink {
            Some(needed) => format!(" With {skills} ({} HP) it needs {needed} of {shells}.", build.hp),
            None => format!(" With {skills} ({} HP) it cannot sink it.", build.hp),
        }
    });

    format!("{opening}, {verdict}{with_build}")
}

fn whole_hp(value: f32) -> Hp {
    Hp::from(value.round())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(
        name: &str,
        section_type: HullSectionType,
        max_hp: f32,
        parent: Option<&str>,
        share: f32,
    ) -> HullHitLocation {
        HullHitLocation {
            name: name.to_owned(),
            section_type,
            max_hp: Some(Hp::from(max_hp)),
            parent: parent.map(str::to_owned),
            volume_coeff: Some(share),
            regenerated_hp_part: None,
            enhanced_regenerated_hp_part: None,
        }
    }

    /// A bow that drains the hull pool and a midsection that never
    /// saturates and deals less: the bow takes shells at its full share until
    /// its pool is spent, then at its saturated share until the hull pool is
    /// spent, and the midsection takes the rest once the bow's 10% falls below
    /// its share.
    #[test]
    fn greedy_salvo_saturates_the_bow_then_spends_the_hull_pool() {
        let locations = [
            location("Bow", HullSectionType::Simple, 1000.0, Some("Hull"), 0.25),
            location("Cas", HullSectionType::Simple, 165_000.0, None, 0.2),
            location("Hull", HullSectionType::Hull, 2500.0, None, 0.25),
        ];
        let model = DamageModel::new(&locations, Hp::from(10_000.0));
        let shares = |section: usize| model.sections[section].shares.expect("the data lists every share");
        let strikes = [
            Strike::Penetration { section: SectionIndex(0), shares: shares(0) },
            Strike::Penetration { section: SectionIndex(1), shares: shares(1) },
        ];

        let case = model.best_case(&strikes, 1000.0, 16, 4000.0);

        let groups: Vec<(u32, &str, DamageShare, f32)> = case
            .hits
            .iter()
            .map(|group| (group.shells, group.lands.as_str(), group.share, group.damage_per_shell.value()))
            .collect();
        // The bow's 250 HP share spends its 1000 HP pool on the fourth shell;
        // the hull pool, 2500 - 4 x 250 = 1500 HP, on the sixth shell after.
        assert_eq!(
            groups,
            [
                (4, "bow", DamageShare::Full, 500.0),
                (6, "bow", DamageShare::Saturated, 250.0),
                (6, "casemate (midsection)", DamageShare::Full, 200.0),
            ]
        );
        assert_eq!(case.max_damage, Hp::from(4700.0));
        // 3500 after 10 shells, 3900 after 12, 4100 after 13.
        assert_eq!(case.shells_to_sink, Some(13));
    }

    /// Each pool gives its share or what it has left; a spent hull pool
    /// leaves the bow its own share, and with both spent it deals 10%, as an
    /// overpenetration does.
    #[test]
    fn spent_pools_lower_a_penetration_to_ten_percent() {
        let locations = [
            location("Bow", HullSectionType::Simple, 1000.0, Some("Hull"), 0.25),
            location("Hull", HullSectionType::Hull, 2500.0, None, 0.25),
        ];
        let model = DamageModel::new(&locations, Hp::from(10_000.0));
        let bow = Strike::Penetration { section: SectionIndex(0), shares: model.sections[0].shares.expect("shares") };
        let dealt = |pools: [Option<f32>; 2], strike: Strike| {
            let hit = DamageModel::hit(&Pools(pools.to_vec()), strike, 1000.0);
            (hit.share, hit.damage)
        };

        assert_eq!(dealt([Some(1000.0), Some(2500.0)], bow), (DamageShare::Full, 500.0));
        assert_eq!(dealt([Some(50.0), Some(2500.0)], bow), (DamageShare::PoolEmptied, 300.0));
        assert_eq!(dealt([Some(0.0), Some(2500.0)], bow), (DamageShare::Saturated, 250.0));
        assert_eq!(dealt([Some(1000.0), Some(0.0)], bow), (DamageShare::HullPoolSpent, 250.0));
        assert_eq!(dealt([Some(0.0), Some(30.0)], bow), (DamageShare::FullySaturated, 100.0));
        assert_eq!(dealt([Some(0.0), Some(0.0)], Strike::Overpenetration), (DamageShare::Overpenetration, 100.0));
    }

    #[test]
    fn armor_zones_map_to_hull_sections() {
        let material = |name: &str| ArmorMaterial::parse(name).unwrap_or_else(|| panic!("{name} is a material"));

        assert_eq!(section_key("Bow", None), Some("Bow"));
        assert_eq!(section_key("Stern", None), Some("St"));
        assert_eq!(section_key("Casemate", None), Some("Cas"));
        assert_eq!(section_key("Citadel", None), Some("Cit"));
        assert_eq!(section_key("Superstructure", Some(material("SSC_Deck"))), Some("SSC"));
        assert_eq!(section_key("Superstructure", Some(material("DeckSS"))), Some("SS"));
        assert_eq!(section_key("Turret", None), None);
        assert_eq!(section_key("Hull", None), None);
    }
}
