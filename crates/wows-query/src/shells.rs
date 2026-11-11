use std::collections::BTreeMap;
use std::collections::HashSet;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Degrees;
use wows_core::units::Km;
use wows_core::units::MetersPerSecond;
use wows_core::units::Millimeters;
use wows_core::units::Percent;
use wows_core::units::Seconds;
use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::OVERMATCH_RATIO;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::is_overmatch;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Projectile;

use crate::builds::BuildAlternative;
use crate::builds::BuildStat;
use crate::builds::BuildValue;
use crate::builds::Builds;
use crate::builds::StatUnit;
use crate::data::GameData;
use crate::error::QueryError;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipArgs;
use crate::ships::ShipClass;
use crate::ships::ShipFilter;
use crate::ships::ShipSummary;
use crate::ships::Tier;

/// Internal name of Inertia Fuse for HE Shells.
pub(crate) const IFHE_SKILL: &str = "HePenetration";

/// Armor a secondary HE shell needs to defeat to matter: cruiser upper hulls
/// and destroyer belts at tiers 8 to 10 (shipbuilds `SECONDARY_ARMOR_THRESHOLD_MM`).
const SECONDARY_ARMOR_THRESHOLD: Millimeters = Millimeters::new(27.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Battery {
    Main,
    Secondary,
}

impl Battery {
    pub fn label(self) -> &'static str {
        match self {
            Self::Main => "main battery",
            Self::Secondary => "secondary battery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub enum Ammo {
    #[serde(rename = "AP")]
    Ap,
    #[serde(rename = "HE")]
    He,
    #[serde(rename = "SAP")]
    Sap,
}

impl Ammo {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ap => "AP",
            Self::He => "HE",
            Self::Sap => "SAP",
        }
    }
}

/// Impact angles from the plate normal at which a shell ricochets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RicochetAngles {
    /// Ricochets start.
    pub from_deg: Degrees,

    /// Every hit that does not overmatch the plate ricochets.
    pub always_from_deg: Degrees,
}

impl RicochetAngles {
    /// "60 deg, always at 67.5 deg".
    fn describe(self) -> String {
        format!("{} deg, always at {} deg", self.from_deg.value(), self.always_from_deg.value())
    }
}

/// The ricochet angles most shells of each ammo type share.
pub(crate) struct RicochetStandards(BTreeMap<Ammo, RicochetAngles>);

impl RicochetStandards {
    fn get(&self, ammo: Ammo) -> Option<RicochetAngles> {
        self.0.get(&ammo).copied()
    }

    /// A note naming the angles of a shell whose angles are not its ammo type's standard.
    fn note(&self, shell: &Shell) -> Option<String> {
        let angles = shell.ricochet?;
        let standard = self.get(shell.ammo)?;
        if angles == standard {
            return None;
        }

        Some(format!(
            "{} ricochets from {} (standard {} / {})",
            shell.ammo.label(),
            angles.describe(),
            standard.from_deg.value(),
            standard.always_from_deg.value()
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Shell {
    /// English name, e.g. "203 mm AP Mk21".
    pub name: String,

    /// GameParams name of the projectile.
    pub param: String,
    pub battery: Battery,
    pub ammo: Ammo,
    pub caliber_mm: Millimeters,

    /// HE or SAP penetration. AP penetration depends on range and impact angle
    /// and has no single value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration_mm: Option<Millimeters>,

    /// HE penetration with Inertia Fuse for HE Shells, for this ship's class.
    /// Absent when the class tree lacks the skill.
    #[serde(skip)]
    pub ifhe_penetration_mm: Option<Millimeters>,

    /// Thickest whole-mm plate AP or SAP overmatches (caliber > 14.3 x thickness).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overmatches_up_to_mm: Option<Millimeters>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_damage: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub muzzle_velocity_mps: Option<MetersPerSecond>,

    /// Base chance to start a fire. Absent when the shell cannot start fires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fire_chance_pct: Option<Percent>,

    /// From the projectile's `bulletRicochetAt` and `bulletAlwaysRicochetAt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ricochet: Option<RicochetAngles>,

    /// AP only: time from arming to detonation (`bulletDetonator`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_time_s: Option<Seconds>,

    /// AP only: the thinnest plate whose penetration arms the fuse
    /// (`bulletDetonatorThreshold`); thinner plates let it pass through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuse_threshold_mm: Option<Millimeters>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShipShells {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,
    pub shells: Vec<ShellReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShellReport {
    /// Battery, caliber and ammo type, e.g. "main battery 203 mm HE".
    pub label: String,

    #[serde(flatten)]
    pub shell: Shell,

    /// HE or SAP penetration on the best-fit build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penetration: Option<BuildStat<Millimeters>>,

    /// Set when the ricochet angles differ from those most shells of the
    /// ammo type share.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ricochet_note: Option<String>,
}

/// HE penetration multipliers Inertia Fuse for HE Shells gives one class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct IfheCoefficients {
    pub(crate) main: f32,
    pub(crate) secondary: f32,
}

impl IfheCoefficients {
    fn for_battery(self, battery: Battery) -> f32 {
        match battery {
            Battery::Main => self.main,
            Battery::Secondary => self.secondary,
        }
    }
}

/// Inertia Fuse for HE Shells in one class tree.
struct IfheBuild {
    name: String,

    /// The first skill in skill order of each cheaper tier, as
    /// [`crate::builds`] takes when every skill of a tier leaves the stat as
    /// it is.
    tier_fillers: Vec<String>,
}

impl GameData {
    /// Main and secondary battery shells of one ship across all its gun
    /// modules, with HE and SAP penetration on the best-fit build.
    pub fn ship_shells(&self, args: &ShipArgs) -> Result<ShipShells, QueryError> {
        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let ifhe = self.ifhe_build(ship.class);
        let standards = self.ricochet_standards();
        let shells = self
            .shells(ship)
            .into_iter()
            .map(|shell| ShellReport {
                label: format!("{} {} mm {}", shell.battery.label(), shell.caliber_mm.value(), shell.ammo.label()),
                penetration: penetration_builds(&shell, ship.tier, ifhe.as_ref()),
                ricochet_note: standards.note(&shell),
                shell,
            })
            .collect();

        Ok(ShipShells { interpretations, ship: ship.summary(), shells })
    }

    /// Where the shell of GameParams name `param` lands when fired at
    /// `range`, times on the in-game clock. `None` beyond its ballistic reach
    /// or when the projectile lacks a ballistic input.
    pub(crate) fn shell_impact(&self, param: &str, range: Km) -> Result<Option<ImpactResult>, QueryError> {
        let projectile = self.projectile(param)?;
        let Some(params) =
            projectile.to_shell_info(param.to_owned()).and_then(|info| ShellParams::from_shell_info(&info))
        else {
            return Ok(None);
        };

        Ok(solve_for_range(&params, range.to_meters()))
    }

    pub(crate) fn shells(&self, ship: &Ship) -> Vec<Shell> {
        let Some(config) = ship.param.vehicle().and_then(|vehicle| vehicle.config_data()) else {
            return Vec::new();
        };
        let ifhe = self.ifhe_coefficients(ship.class);

        let batteries =
            [(Battery::Main, &config.main_battery_ammo), (Battery::Secondary, &config.secondary_battery_ammo)];
        let mut shells = Vec::new();
        for (battery, names) in batteries {
            let mut names: Vec<&String> = names.iter().collect();
            names.sort();
            for name in names {
                let Some(param) = self.provider().game_param_by_name(name) else {
                    continue;
                };
                let Some(shell) = shell_from_param(&param, self.projectile_name(name), battery, ifhe) else {
                    continue;
                };
                shells.push(shell);
            }
        }

        shells
    }

    /// The most common ricochet angles of each ammo type across the distinct
    /// main and secondary shells of released ships, counted on first use.
    fn ricochet_standards(&self) -> &RicochetStandards {
        self.ricochet_standards.get_or_init(|| self.count_ricochet_standards())
    }

    fn count_ricochet_standards(&self) -> RicochetStandards {
        let filter = ShipFilter { exclude_test: true, ..ShipFilter::default() };
        let mut seen: HashSet<&str> = HashSet::new();
        let mut counts: BTreeMap<Ammo, Vec<(RicochetAngles, usize)>> = BTreeMap::new();
        for ship in self.ships.iter().filter(|ship| filter.matches(ship)) {
            let Some(config) = ship.param.vehicle().and_then(|vehicle| vehicle.config_data()) else {
                continue;
            };
            for name in config.main_battery_ammo.iter().chain(&config.secondary_battery_ammo) {
                if !seen.insert(name.as_str()) {
                    continue;
                }
                let Some(param) = self.provider().game_param_by_name(name) else {
                    continue;
                };
                let Some(projectile) = param.projectile() else {
                    continue;
                };
                let Some(ammo) = shell_ammo(projectile) else {
                    continue;
                };
                let Some(angles) = ricochet_angles(ammo, projectile) else {
                    continue;
                };

                let tally = counts.entry(ammo).or_default();
                match tally.iter_mut().find(|(known, _)| *known == angles) {
                    Some((_, count)) => *count += 1,
                    None => tally.push((angles, 1)),
                }
            }
        }

        // Ties go to the lower angles so the choice does not depend on ship order.
        let standards = counts
            .into_iter()
            .filter_map(|(ammo, tally)| {
                let (angles, _) = tally.into_iter().max_by(|(left, left_count), (right, right_count)| {
                    left_count.cmp(right_count).then_with(|| {
                        right
                            .from_deg
                            .value()
                            .total_cmp(&left.from_deg.value())
                            .then_with(|| right.always_from_deg.value().total_cmp(&left.always_from_deg.value()))
                    })
                })?;
                Some((ammo, angles))
            })
            .collect();

        RicochetStandards(standards)
    }

    /// `None` when the class tree has no Inertia Fuse for HE Shells.
    pub(crate) fn ifhe_coefficients(&self, class: ShipClass) -> Option<IfheCoefficients> {
        let skill = self.skills.iter().find(|skill| skill.internal_name().as_str() == IFHE_SKILL)?;
        self.skill_points(skill, class)?;
        let modifiers = skill.modifiers()?;
        let species = class.species();

        // An absent coefficient leaves penetration unchanged, as in the client.
        let coefficient = |name: &str| {
            modifiers
                .iter()
                .find(|modifier| modifier.name() == name)
                .map_or(1.0, |modifier| modifier.get_for_species(&species))
        };

        Some(IfheCoefficients {
            main: coefficient("GMPenetrationCoeffHE"),
            secondary: coefficient("GSPenetrationCoeffHE"),
        })
    }

    /// Inertia Fuse for HE Shells when the class tree has it.
    fn ifhe_build(&self, class: ShipClass) -> Option<IfheBuild> {
        let skill = self.skills.iter().find(|skill| skill.internal_name().as_str() == IFHE_SKILL)?;
        let points = self.skill_points(skill, class)?;
        let tier_fillers = (1..points)
            .filter_map(|tier| self.skills.iter().find(|other| self.skill_points(other, class) == Some(tier)))
            .map(|other| self.skill_name(other))
            .collect();

        Some(IfheBuild { name: self.skill_name(skill), tier_fillers })
    }
}

/// Armor a main battery HE shell of this tier most needs to defeat: cruiser
/// and battleship bows and decks (shipbuilds `tier_armor_threshold_mm`).
fn main_armor_threshold(tier: Tier) -> Millimeters {
    Millimeters::new(match tier.0 {
        ..=4 => 13.0,
        5 | 6 => 19.0,
        7 => 25.0,
        _ => 32.0,
    })
}

/// HE or SAP penetration on the best-fit build. Inertia Fuse for HE Shells
/// raises every HE shell it applies to, but the best-fit build takes it only
/// when it lifts the shell over the armor threshold of its battery and tier
/// (shipbuilds rule `ifhe`). The build that differs is an alternative whose
/// note states both values and the reason.
fn penetration_builds(shell: &Shell, tier: Tier, ifhe: Option<&IfheBuild>) -> Option<BuildStat<Millimeters>> {
    let base = shell.penetration_mm?;
    let stat = format!("{} penetration ({} {})", shell.ammo.label(), shell.battery.label(), shell.name);
    let unchanged = |alternatives| BuildStat {
        stat: stat.clone(),
        unit: Millimeters::UNIT,
        builds: Builds {
            base,
            best: BuildValue { value: base, assumed: Vec::new(), tier_fillers: Vec::new() },
            alternatives,
        },
    };

    let (Some(ifhe), Some(with_ifhe)) = (ifhe, shell.ifhe_penetration_mm) else {
        return Some(unchanged(Vec::new()));
    };
    if with_ifhe.value() <= base.value() {
        return Some(unchanged(Vec::new()));
    }

    let name = &ifhe.name;
    let threshold = match shell.battery {
        Battery::Main => main_armor_threshold(tier),
        Battery::Secondary => SECONDARY_ARMOR_THRESHOLD,
    };
    let plating = match shell.battery {
        Battery::Main => {
            format!("the {threshold} bow and deck plating of the cruisers and battleships a tier {} ship meets", tier.0)
        }
        Battery::Secondary => {
            format!("the {threshold} upper hull plating of cruisers and belts of destroyers at tiers 8 to 10")
        }
    };
    let battery = match shell.battery {
        Battery::Main => "Main battery",
        Battery::Secondary => "Secondary battery",
    };
    let summary = format!("{battery} HE: {base} base; {with_ifhe} with {name}.");

    if base.value() < threshold.value() && with_ifhe.value() >= threshold.value() {
        return Some(BuildStat {
            stat,
            unit: Millimeters::UNIT,
            builds: Builds {
                base,
                best: BuildValue {
                    value: with_ifhe,
                    assumed: vec![name.clone()],
                    tier_fillers: ifhe.tier_fillers.clone(),
                },
                alternatives: vec![BuildAlternative {
                    label: format!("without {name}"),
                    value: base,
                    assumed: Vec::new(),
                    tier_fillers: Vec::new(),
                    note: Some(format!(
                        "{summary} Best-fit builds take the skill because only with it does the shell penetrate {plating}."
                    )),
                }],
            },
        });
    }

    let reason = if base.value() >= threshold.value() {
        format!("{base} already penetrates {plating}")
    } else {
        format!("even {with_ifhe} does not penetrate {plating}")
    };
    Some(unchanged(vec![BuildAlternative {
        label: format!("with {name}"),
        value: with_ifhe,
        assumed: vec![name.clone()],
        tier_fillers: ifhe.tier_fillers.clone(),
        note: Some(format!("{summary} Best-fit builds leave the skill out because {reason}.")),
    }]))
}

fn shell_from_param(param: &Param, name: String, battery: Battery, ifhe: Option<IfheCoefficients>) -> Option<Shell> {
    let projectile = param.projectile()?;
    let ammo = shell_ammo(projectile)?;
    let caliber_mm = Millimeters::new(round_tenth(projectile.bullet_diametr()? * 1000.0));

    // The client floors HE and SAP penetration (FactoryArtillery.py).
    let (penetration_mm, ifhe_penetration_mm) = match ammo {
        Ammo::He => {
            let base = projectile.alpha_piercing_he();
            let ifhe = base.zip(ifhe).map(|(base, ifhe)| Millimeters::new((base * ifhe.for_battery(battery)).floor()));
            (base.map(|base| Millimeters::new(base.floor())), ifhe)
        }
        Ammo::Sap => (projectile.alpha_piercing_cs().map(|base| Millimeters::new(base.floor())), None),
        Ammo::Ap => (None, None),
    };
    let overmatches_up_to_mm = match ammo {
        Ammo::Ap | Ammo::Sap => thickest_overmatched_plate(caliber_mm),
        Ammo::He => None,
    };

    Some(Shell {
        name,
        param: param.name().to_owned(),
        battery,
        ammo,
        caliber_mm,
        penetration_mm,
        ifhe_penetration_mm,
        overmatches_up_to_mm,
        alpha_damage: projectile.alpha_damage(),
        muzzle_velocity_mps: projectile.bullet_speed().map(MetersPerSecond::new),
        fire_chance_pct: projectile
            .burn_prob()
            .filter(|chance| *chance > 0.0)
            .map(|chance| Percent::new(round_tenth(chance * 100.0))),
        ricochet: ricochet_angles(ammo, projectile),
        fuse_time_s: (ammo == Ammo::Ap).then(|| projectile.bullet_detonator()).flatten().map(Seconds::new),
        fuse_threshold_mm: (ammo == Ammo::Ap)
            .then(|| projectile.bullet_detonator_threshold())
            .flatten()
            .map(Millimeters::new),
    })
}

/// `None` for ammo other than AP, HE and SAP, or a projectile with no ammo type.
pub(crate) fn shell_ammo(projectile: &Projectile) -> Option<Ammo> {
    match AmmoType::from_game_str(projectile.ammo_type()?) {
        AmmoType::AP => Some(Ammo::Ap),
        AmmoType::HE => Some(Ammo::He),
        AmmoType::SAP => Some(Ammo::Sap),
        AmmoType::Unknown(_) => None,
    }
}

/// `None` for HE, which does not ricochet whatever angles its projectile lists.
pub(crate) fn ricochet_angles(ammo: Ammo, projectile: &Projectile) -> Option<RicochetAngles> {
    if ammo == Ammo::He {
        return None;
    }

    let always = projectile.bullet_always_ricochet_at()?;
    // GameParams writes bulletRicochetAt = 91 deg, past the always angle, for no
    // random band; every hit from the always angle on ricochets, so ricochets
    // start there.
    let start = projectile.bullet_ricochet_at()?.min(always);

    Some(RicochetAngles { from_deg: Degrees::new(start), always_from_deg: Degrees::new(always) })
}

fn round_tenth(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/// Thickest whole-mm plate `is_overmatch` accepts for `caliber`, `None` below 1 mm.
pub(crate) fn thickest_overmatched_plate(caliber: Millimeters) -> Option<Millimeters> {
    let mut thickness = (caliber.value() / OVERMATCH_RATIO).ceil();
    while thickness >= 1.0 && !is_overmatch(caliber, Millimeters::new(thickness)) {
        thickness -= 1.0;
    }
    (thickness >= 1.0).then(|| Millimeters::new(thickness))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thickest_overmatched_plate_is_the_last_whole_mm_overmatch() {
        for caliber in 100..=510u16 {
            let caliber = Millimeters::new(f32::from(caliber));
            let thickness = thickest_overmatched_plate(caliber).expect("calibers above 14.3 mm overmatch 1 mm");
            assert!(is_overmatch(caliber, thickness), "{caliber:?} overmatches {thickness:?}");
            assert!(
                !is_overmatch(caliber, Millimeters::new(thickness.value() + 1.0)),
                "{caliber:?} does not overmatch past {thickness:?}"
            );
        }
    }

    #[derive(Serialize)]
    struct NonstandardRicochet<'a> {
        standards: &'a BTreeMap<Ammo, RicochetAngles>,

        /// "T10 Des Moines (cruiser, USA): PAPA012_Shell_203mm_AP_AP_Mk_21: AP ricochets from ...".
        main_battery: Vec<String>,
    }

    fn test_game() -> GameData {
        let root =
            std::path::PathBuf::from(std::env::var_os("WOWS_GAME_DATA").expect("WOWS_GAME_DATA names the dump root"));
        let index = wows_data_mgr::builds::BuildsIndex::load(&root.join("builds.toml"))
            .expect("dump root has a readable builds.toml");
        let entry = index.find_by_build(13533378).expect("dump root lists build 13533378");
        let cache = std::env::temp_dir().join("wows-query-test-cache");

        GameData::load_dump(&root.join(&entry.dir), Some(&cache)).expect("dump loads")
    }

    fn released_ships(game: &GameData) -> Vec<&Ship> {
        let filter = ShipFilter { exclude_test: true, ..ShipFilter::default() };
        let mut ships: Vec<&Ship> = game.ships.iter().filter(|ship| filter.matches(ship)).collect();
        ships.sort_by(|left, right| right.tier.cmp(&left.tier).then_with(|| left.name.cmp(&right.name)));

        ships
    }

    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn released_ships_with_nonstandard_main_battery_ricochet_angles() {
        let game = test_game();
        let standards = game.ricochet_standards();

        let mut main_battery = Vec::new();
        for ship in released_ships(&game) {
            for shell in game.shells(ship).iter().filter(|shell| shell.battery == Battery::Main) {
                if let Some(note) = standards.note(shell) {
                    main_battery.push(format!("{}: {}: {note}", ship.label(), shell.name));
                }
            }
        }

        let des_moines = "T10 Des Moines (cruiser, USA): 203 mm AP Mk21";
        assert!(main_battery.iter().any(|line| line.starts_with(des_moines)), "{main_battery:#?}");
        insta::assert_yaml_snapshot!(NonstandardRicochet { standards: &standards.0, main_battery });
    }

    /// A GameParams name such as "PBPA124_457MM_MII_HE_BC" starts with a
    /// six-character index and holds underscores; an English name does not.
    #[test]
    #[cfg_attr(not(has_build_13533378), ignore)]
    fn released_ships_show_no_shell_by_its_gameparams_name() {
        let game = test_game();

        let mut checked = 0;
        for ship in released_ships(&game) {
            for shell in game.shells(ship) {
                assert!(
                    shell.name != shell.param && !shell.name.contains('_'),
                    "{} shows {} as {:?}",
                    ship.label(),
                    shell.param,
                    shell.name
                );
                checked += 1;
            }
        }
        assert!(checked > 1_000, "only {checked} shells checked");
    }
}
