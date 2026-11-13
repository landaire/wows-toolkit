//! A ship's main battery gun modules compared as the attacker: what the shell
//! of each module does to the armor over the citadel of a set of target ships.

use std::collections::BTreeSet;
use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wowsunpack::ballistics::is_citadel_zone;
use wowsunpack::export::gltf_export::InteractiveArmorMesh;

use crate::armor_assessment::ArmorFingerprint;
use crate::armor_assessment::class_plural;
use crate::armor_assessment::parallel_map;
use crate::armor_trace::GunChoice;
use crate::armor_trace::GunRequest;
use crate::armor_trace::RayOutcome;
use crate::armor_trace::Weapon;
use crate::armor_trace::cast_grid;
use crate::armor_trace::largest_shell;
use crate::armor_trace::line_of_fire;
use crate::armor_trace::trace_ray;
use crate::citadel::has_citadel_plates;
use crate::comparison::tier_label;
use crate::data::GameData;
use crate::error::QueryError;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::names::Interpretations;
use crate::shells::Ammo;
use crate::shells::Shell;
use crate::ships::Ship;
use crate::ships::ShipClass;
use crate::ships::ShipFilter;
use crate::ships::ShipStatus;
use crate::ships::Tier;
use crate::ships::TierRange;

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GunModuleMatchupsArgs {
    /// Firing ship, e.g. "G. Kurfurst".
    pub ship: String,

    /// Main battery gun modules to compare, e.g. ["420 mm/48 SK C/40 on a Drh LC/34 mount"]. Defaults to every gun module the ship can mount, stock first.
    pub modules: Option<Vec<String>>,

    /// Shell type. Defaults to AP.
    pub ammo: Option<Ammo>,

    /// Gun range in km, 0 or more. Defaults to 12.
    pub range_km: Option<Km>,

    /// Target heading away from broadside in degrees: 0 is broadside, 30 angled, 90 bow-on. Defaults to 0.
    pub target_angle_deg: Option<Degrees>,

    /// Class of the target ships. Defaults to the firing ship's class.
    pub target_class: Option<ShipClass>,

    /// Lowest target tier. Defaults to target_max_tier when given, else the firing ship's tier.
    pub target_min_tier: Option<Tier>,

    /// Highest target tier. Defaults to target_min_tier when given, else the firing ship's tier.
    pub target_max_tier: Option<Tier>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GunModuleMatchupsOutput {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: String,
    pub ammo: Ammo,
    pub range_km: Km,

    /// "broadside", or the target heading away from broadside.
    pub aspect: String,
    pub targets: TargetSet,
    pub modules: Vec<ModuleMatchup>,

    /// Each module after the first against the first; absent with one module.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub differences: Vec<ModuleDifference>,
    pub conclusion: String,
    pub assumptions: [&'static str; 6],
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetSet {
    pub class: ShipClass,
    pub tiers: TierRange,

    /// Ships of the class and tiers, test ships included.
    pub ships: usize,

    /// Test ships among `ships`.
    #[serde(skip_serializing_if = "is_zero")]
    pub test: usize,

    /// Ships whose armor model has a citadel; the counts of each module are
    /// out of these.
    pub with_citadel: usize,

    #[serde(skip_serializing_if = "is_zero")]
    pub without_citadel: usize,

    #[serde(skip_serializing_if = "is_zero")]
    pub without_armor_model: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleMatchup {
    pub module: String,
    pub stock: bool,

    /// English name of the shell.
    pub shell: String,
    pub caliber_mm: Millimeters,

    /// AP: penetration against a plate struck head-on at range_km.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_penetration_mm: Option<Millimeters>,

    /// HE or SAP penetration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_mm: Option<Millimeters>,

    /// Thickest whole-mm plate the shell overmatches; HE does not overmatch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overmatches_up_to_mm: Option<Millimeters>,

    /// AP: the thinnest plate that arms the fuse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_threshold_mm: Option<Millimeters>,

    /// Target ships whose armor over the citadel stops the shell on at most
    /// 10% of the rays that do not ricochet.
    pub penetrates_armor: usize,

    /// Target ships where at least 5% of the rays over the citadel detonate
    /// inside it.
    pub citadels: usize,

    /// Target ships where at least 25% do.
    pub citadels_often: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModuleDifference {
    pub module: String,

    /// The first module, which `module` is compared with.
    pub against: String,
    pub penetrates_armor: ShipDifference,
    pub citadels: ShipDifference,
    pub citadels_often: ShipDifference,
}

/// Target ships one module counts and the other does not.
#[derive(Debug, Clone, Serialize)]
pub struct ShipDifference {
    /// Ships `module` counts and `against` does not, with shares at least 5
    /// points apart.
    pub gained: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gained_ships: Vec<ShipShares>,

    /// Ships `against` counts and `module` does not, with shares at least 5
    /// points apart.
    pub lost: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lost_ships: Vec<ShipShares>,
}

/// One target's share for both modules of a difference: for
/// penetrates_armor, the rays over the citadel that do not ricochet and that
/// armor does not stop; for the citadel counts, the rays over the citadel
/// that detonate inside it.
#[derive(Debug, Clone, Serialize)]
pub struct ShipShares {
    pub ship: String,
    pub pct: Percent,
    pub against_pct: Percent,
}

/// Rays over one target's citadel and what one shell did on them.
#[derive(Debug, Clone, Copy, Default)]
struct CitadelRays {
    over: usize,
    ricocheted: usize,
    stopped: usize,
    detonated_inside: usize,
}

impl CitadelRays {
    /// Share of the rays that do not ricochet on which armor does not stop the shell.
    fn armor_penetrated(self) -> Option<f32> {
        let struck = self.over - self.ricocheted;

        (struck > 0).then(|| (struck - self.stopped) as f32 / struck as f32)
    }

    fn detonated_inside(self) -> Option<f32> {
        (self.over > 0).then(|| self.detonated_inside as f32 / self.over as f32)
    }
}

/// A share each module is counted and compared on.
#[derive(Debug, Clone, Copy)]
enum Measure {
    PenetratesArmor,
    Citadels,
    CitadelsOften,
}

impl Measure {
    fn threshold(self) -> f32 {
        match self {
            Self::PenetratesArmor => 0.9,
            Self::Citadels => 0.05,
            Self::CitadelsOften => 0.25,
        }
    }

    fn share(self, rays: CitadelRays) -> Option<f32> {
        match self {
            Self::PenetratesArmor => rays.armor_penetrated(),
            Self::Citadels | Self::CitadelsOften => rays.detonated_inside(),
        }
    }

    fn meets(self, rays: CitadelRays) -> bool {
        self.share(rays).is_some_and(|share| share >= self.threshold())
    }
}

/// What a target's stock armor model holds.
enum TargetArmor {
    WithoutArmorModel,
    WithoutCitadel,
    Citadel(ArmorFingerprint),
}

impl GameData {
    /// Each main battery gun module of `args.ship` as the attacker: its
    /// shell, raw penetration, overmatch and fuse threshold, and against a
    /// set of target ships how many it penetrates the armor over the citadel
    /// of and citadels, with the ships where the modules differ.
    pub fn gun_module_matchups(&self, args: &GunModuleMatchupsArgs) -> Result<GunModuleMatchupsOutput, QueryError> {
        // The range battle_impact and dispersion_at_range compare guns at.
        const DEFAULT_RANGE: Km = Km::new(12.0);
        const ASSUMPTIONS: [&str; 6] = [
            "Targets are released ships of the class and tiers, test ships included and marked \"test\", on their stock hulls. Ships sharing one armor model are traced once and each counted.",
            "Rays 1 m apart are cast over each target along the line of fire at range_km and traced as armor_trace traces them. Only rays that cross a citadel plate count: they meet the belt, decks and slopes over the citadel. Rays whose first armor contact is below the waterline are not modelled. Shares are by area, not weighted by dispersion or aim.",
            "penetrates_armor counts a target when the shell is stopped by armor (shatters or spends its velocity) on at most 10% of the rays over its citadel that do not ricochet. Ricochets depend on the strike angle, not on penetration; overmatch, which prevents them, shows in the citadel counts.",
            "citadels counts a target when at least 5% of the rays over its citadel detonate inside it, the share armor_trace answers sometimes at; citadels_often when at least 25%, where it answers yes. HE and SAP are tested against the first plate only, so they reach the citadel only where citadel armor is struck first.",
            "differences name a ship only when one module meets the threshold, the other does not, and their shares differ by at least 5 percentage points; smaller differences come from where the ray grid falls, which shifts with each shell's fall angle. The counts of each module are not filtered this way.",
            "Strikes between the ricochet start and always-ricochet angles ricochet at random in the game and are treated as no ricochet.",
        ];

        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let modules = self.compared_gun_modules(ship, args.modules.as_deref())?;
        let ammo = args.ammo.unwrap_or(Ammo::Ap);
        let range = args.range_km.unwrap_or(DEFAULT_RANGE);
        let angle = args.target_angle_deg.unwrap_or(Degrees::new(0.0));
        if !(-180.0..=180.0).contains(&angle.value()) {
            return Err(QueryError::InvalidArguments { reason: "target_angle_deg must be between -180 and 180" });
        }
        let class = args.target_class.unwrap_or(ship.class);
        let tiers = target_tiers(ship.tier, args.target_min_tier, args.target_max_tier)?;

        let mut shells: Vec<Shell> = Vec::with_capacity(modules.len());
        let mut weapons: Vec<Weapon> = Vec::with_capacity(modules.len());
        for module in &modules {
            let module_shells = self.gun_module_shells(ship, &module.param);
            let Some(shell) = largest_shell(&module_shells, ammo) else {
                let fired: BTreeSet<Ammo> = module_shells.iter().map(|shell| shell.ammo).collect();
                return Err(QueryError::ModuleAmmoMissing {
                    ship: ship.name.clone(),
                    module: module.name.clone(),
                    ammo,
                    fired: fired.into_iter().collect(),
                });
            };
            let request = GunRequest {
                guns: GunChoice::MainModule(module.param.clone()),
                ammo,
                range_km: Some(range),
                ifhe: false,
            };
            weapons.push(self.gun_weapon(ship, &request)?);
            shells.push(shell.clone());
        }

        let filter = ShipFilter {
            tiers: Some(tiers),
            class: Some(class),
            nation: None,
            include_irregular: false,
            exclude_test: false,
            include_replicas: false,
        };
        let mut targets: Vec<&Ship> = self.ships.iter().filter(|target| filter.matches(target)).collect();
        targets.sort_by(|left, right| {
            right
                .tier
                .cmp(&left.tier)
                .then_with(|| left.rank().cmp(&right.rank()))
                .then_with(|| left.name.cmp(&right.name))
        });

        let armor = parallel_map(&targets, |target| -> Result<TargetArmor, QueryError> {
            Ok(match self.stock_meshes(target)? {
                None => TargetArmor::WithoutArmorModel,
                Some(meshes) if !has_citadel_plates(&meshes) => TargetArmor::WithoutCitadel,
                Some(meshes) => TargetArmor::Citadel(ArmorFingerprint::from_meshes(&meshes)),
            })
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

        // Ships sharing one armor model, such as a test ship that reuses a
        // released ship's armor, are traced once.
        let mut traced_index: HashMap<ArmorFingerprint, usize> = HashMap::new();
        let mut unique: Vec<&Ship> = Vec::new();
        for (target, armor) in targets.iter().zip(&armor) {
            if let TargetArmor::Citadel(fingerprint) = armor {
                traced_index.entry(*fingerprint).or_insert_with(|| {
                    unique.push(target);
                    unique.len() - 1
                });
            }
        }
        let traces = parallel_map(&unique, |target| -> Result<Vec<CitadelRays>, QueryError> {
            let Some(meshes) = self.stock_meshes(target)? else {
                return Err(QueryError::ArmorMissing { ship: target.name.clone() });
            };
            trace_target(&meshes, &weapons, angle)
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

        // Each target with a citadel, with its rays per module.
        let counted: Vec<(&Ship, &[CitadelRays])> = targets
            .iter()
            .zip(&armor)
            .filter_map(|(target, armor)| match armor {
                TargetArmor::Citadel(fingerprint) => Some((*target, traces[traced_index[fingerprint]].as_slice())),
                TargetArmor::WithoutArmorModel | TargetArmor::WithoutCitadel => None,
            })
            .collect();

        let target_set = TargetSet {
            class,
            tiers,
            ships: targets.len(),
            test: targets.iter().filter(|target| target.status() == ShipStatus::Test).count(),
            with_citadel: counted.len(),
            without_citadel: armor.iter().filter(|armor| matches!(armor, TargetArmor::WithoutCitadel)).count(),
            without_armor_model: armor.iter().filter(|armor| matches!(armor, TargetArmor::WithoutArmorModel)).count(),
        };

        let count =
            |measure: Measure, index: usize| counted.iter().filter(|(_, rays)| measure.meets(rays[index])).count();
        let matchups: Vec<ModuleMatchup> = modules
            .iter()
            .zip(&shells)
            .zip(&weapons)
            .enumerate()
            .map(|(index, ((module, shell), weapon))| ModuleMatchup {
                module: module.name.clone(),
                stock: module.stock,
                shell: shell.name.clone(),
                caliber_mm: shell.caliber_mm,
                raw_penetration_mm: weapon.arrival.raw_penetration_mm,
                penetration_mm: shell.penetration_mm,
                overmatches_up_to_mm: shell.overmatches_up_to_mm,
                fuse_threshold_mm: shell.fuse_threshold_mm,
                penetrates_armor: count(Measure::PenetratesArmor, index),
                citadels: count(Measure::Citadels, index),
                citadels_often: count(Measure::CitadelsOften, index),
            })
            .collect();

        let differences: Vec<ModuleDifference> = (1..modules.len())
            .map(|index| ModuleDifference {
                module: modules[index].name.clone(),
                against: modules[0].name.clone(),
                penetrates_armor: ship_difference(&counted, index, Measure::PenetratesArmor),
                citadels: ship_difference(&counted, index, Measure::Citadels),
                citadels_often: ship_difference(&counted, index, Measure::CitadelsOften),
            })
            .collect();

        let aspect = aspect(angle);
        let conclusion = conclusion(ship, &matchups, &target_set, &aspect, range, ammo);

        Ok(GunModuleMatchupsOutput {
            interpretations,
            ship: ship.label(),
            ammo,
            range_km: range,
            aspect,
            targets: target_set,
            modules: matchups,
            differences,
            conclusion,
            assumptions: ASSUMPTIONS,
        })
    }

    /// The gun modules named in `queries`, in request order, or every gun
    /// module of `ship`, stock first.
    fn compared_gun_modules(&self, ship: &Ship, queries: Option<&[String]>) -> Result<Vec<Module>, QueryError> {
        let mut gun_modules: Vec<Module> =
            self.ship_modules(ship).into_iter().filter(|module| module.kind == ModuleKind::Guns).collect();
        gun_modules.sort_by_key(|module| !module.stock);
        if gun_modules.is_empty() {
            return Err(QueryError::WeaponNotFound {
                ship: ship.name.clone(),
                weapon: "main battery gun module".to_owned(),
            });
        }

        let Some(queries) = queries.filter(|queries| !queries.is_empty()) else {
            return Ok(gun_modules);
        };
        let mut chosen: Vec<Module> = Vec::with_capacity(queries.len());
        for query in queries {
            let resolved = self.resolve_modules(ship, std::slice::from_ref(query))?;
            for module in resolved.chosen {
                if module.kind != ModuleKind::Guns {
                    return Err(QueryError::NotGunModule {
                        ship: ship.name.clone(),
                        module: module.name,
                        kind: module.kind.label(),
                        gun_modules: gun_modules.iter().map(|module| module.name.clone()).collect(),
                    });
                }
                if !chosen.iter().any(|other| other.param == module.param) {
                    chosen.push(module);
                }
            }
        }

        Ok(chosen)
    }
}

/// The target tiers: one bound given stands for both, none gives `tier`.
fn target_tiers(tier: Tier, min: Option<Tier>, max: Option<Tier>) -> Result<TierRange, QueryError> {
    let (min, max) = match (min, max) {
        (Some(min), Some(max)) => (min, max),
        (Some(only), None) | (None, Some(only)) => (only, only),
        (None, None) => (tier, tier),
    };
    if min > max {
        return Err(QueryError::InvalidArguments { reason: "target_min_tier must not exceed target_max_tier" });
    }

    Ok(TierRange { min, max })
}

/// The rays over `meshes`' citadel and what each weapon's shell did on them.
fn trace_target(
    meshes: &[InteractiveArmorMesh],
    weapons: &[Weapon],
    angle: Degrees,
) -> Result<Vec<CitadelRays>, QueryError> {
    // Ray spacing of armor_assessment, so shares compare with its traces.
    const TRACE_SPACING: Meters = Meters::new(1.0);

    let mut traces = Vec::with_capacity(weapons.len());
    for weapon in weapons {
        let grid = cast_grid(meshes, line_of_fire(angle, weapon.arrival.fall_angle_deg), TRACE_SPACING)?;
        let mut rays = CitadelRays::default();
        for ray in grid.rays.into_iter().filter(|ray| !ray.underwater) {
            if !ray.plates.iter().any(|plate| is_citadel_zone(plate.zone)) {
                continue;
            }

            rays.over += 1;
            match trace_ray(&weapon.model, ray.plates).outcome {
                RayOutcome::Citadel => rays.detonated_inside += 1,
                RayOutcome::Ricochet => rays.ricocheted += 1,
                RayOutcome::Shatter | RayOutcome::Stopped => rays.stopped += 1,
                // The shell got through the armor it met.
                RayOutcome::Penetration | RayOutcome::Overpenetration => {}
            }
        }
        traces.push(rays);
    }

    Ok(traces)
}

/// The targets module `index` and the first module differ on for `measure`,
/// leaving out shares too close to tell apart.
fn ship_difference(counted: &[(&Ship, &[CitadelRays])], index: usize, measure: Measure) -> ShipDifference {
    // Shares closer than this differ by where the ray grid falls on the
    // target, which shifts with each shell's fall angle, more than by the
    // shells.
    const NOISE_MARGIN: f32 = 0.05;
    // Ships named per list; the counts give the rest.
    const NAME_LIMIT: usize = 12;

    let mut gained: Vec<ShipShares> = Vec::new();
    let mut lost: Vec<ShipShares> = Vec::new();
    for (ship, rays) in counted {
        let (Some(share), Some(against)) = (measure.share(rays[index]), measure.share(rays[0])) else {
            continue;
        };
        if (share - against).abs() < NOISE_MARGIN {
            continue;
        }

        let shares = ShipShares { ship: ship.label(), pct: whole_percent(share), against_pct: whole_percent(against) };
        match (measure.meets(rays[index]), measure.meets(rays[0])) {
            (true, false) => gained.push(shares),
            (false, true) => lost.push(shares),
            // Both or neither meet the threshold.
            (true, true) | (false, false) => {}
        }
    }

    ShipDifference {
        gained: gained.len(),
        lost: lost.len(),
        gained_ships: gained.into_iter().take(NAME_LIMIT).collect(),
        lost_ships: lost.into_iter().take(NAME_LIMIT).collect(),
    }
}

fn whole_percent(share: f32) -> Percent {
    Percent::new((share * 100.0).round())
}

fn aspect(angle: Degrees) -> String {
    if angle.value() == 0.0 { "broadside".to_owned() } else { format!("{} deg from broadside", angle.value()) }
}

/// "At 12 km against the 43 tier 10 battleships with a citadel (broadside),
/// AP from the 420 mm guns (597.8 against 566.5 mm raw penetration) gets
/// through the armor over the citadel of 43 ships against 40 for AP from the
/// 406 mm guns, citadels 38 against 32 at least sometimes and 19 against 16
/// often."
fn conclusion(
    ship: &Ship,
    matchups: &[ModuleMatchup],
    targets: &TargetSet,
    aspect: &str,
    range: Km,
    ammo: Ammo,
) -> String {
    let tiers = tier_label(targets.tiers);
    let class = class_plural(targets.class);
    if targets.ships == 0 {
        return format!("No released or test {tiers} {class} is in this game data, so there is nothing to compare.");
    }
    if targets.with_citadel == 0 {
        return format!(
            "None of the {} {tiers} {class} has a citadel in an armor model of this game data, so there is no citadel armor to compare.",
            targets.ships
        );
    }

    let situation = format!(
        "At {} km against the {} {tiers} {class} with a citadel ({aspect})",
        range.value(),
        targets.with_citadel
    );
    let labels = module_labels(matchups);
    let ammo = ammo.label();

    let Some((first, others)) = matchups.split_first() else {
        return format!("{situation}, {} has no gun module to compare.", ship.name);
    };
    if others.is_empty() {
        return format!(
            "{situation}, {ammo} from {}'s {} ({}) gets through the armor over the citadel of {}, citadels {} at least sometimes and {} often; it has no other gun module.",
            ship.name,
            labels[0],
            penetration(first, None),
            first.penetrates_armor,
            first.citadels,
            first.citadels_often
        );
    }

    let clauses: Vec<String> = others
        .iter()
        .zip(&labels[1..])
        .map(|(other, label)| {
            format!(
                "{ammo} from the {label} ({}) gets through the armor over the citadel of {} ships against {} for {ammo} from the {}, citadels {} against {} at least sometimes and {} against {} often",
                penetration(other, Some(first)),
                other.penetrates_armor,
                first.penetrates_armor,
                labels[0],
                other.citadels,
                first.citadels,
                other.citadels_often,
                first.citadels_often,
            )
        })
        .collect();

    format!("{situation}, {}.", clauses.join("; "))
}

/// "597.8 mm raw penetration", or "597.8 against 566.5 mm raw penetration".
fn penetration(matchup: &ModuleMatchup, against: Option<&ModuleMatchup>) -> String {
    let (value, kind) = match (matchup.raw_penetration_mm, matchup.penetration_mm) {
        (Some(raw), _) => (Some(raw), "raw penetration"),
        (None, penetration) => (penetration, "penetration"),
    };
    let against = against.and_then(|against| against.raw_penetration_mm.or(against.penetration_mm));

    match (value, against) {
        (Some(value), Some(against)) => format!("{} against {} mm {kind}", value.value(), against.value()),
        (Some(value), None) => format!("{} mm {kind}", value.value()),
        (None, _) => "penetration not stated".to_owned(),
    }
}

/// "420 mm guns" when the calibers tell the modules apart, else module names.
fn module_labels(matchups: &[ModuleMatchup]) -> Vec<String> {
    let calibers: BTreeSet<u32> = matchups.iter().map(|matchup| whole_mm(matchup.caliber_mm)).collect();
    if calibers.len() == matchups.len() {
        matchups.iter().map(|matchup| format!("{} mm guns", whole_mm(matchup.caliber_mm))).collect()
    } else {
        matchups.iter().map(|matchup| matchup.module.clone()).collect()
    }
}

fn whole_mm(caliber: Millimeters) -> u32 {
    caliber.value().round() as u32
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}
