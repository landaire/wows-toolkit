//! Shell dispersion at one firing distance, on the base build and with each
//! fire control module, upgrade, captain skill and consumable that lengthens
//! the battery's range or tightens its accuracy.
//!
//! Horizontal dispersion depends on the firing distance alone. Vertical
//! dispersion is horizontal times a ratio interpolated over the distance as a
//! fraction of the battery's maximum range (`getEllipse` and
//! `getClampedCoeff` in the game scripts), so a longer maximum range changes
//! vertical dispersion at a fixed distance.

use std::cmp::Ordering;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use wows_core::units::Km;
use wows_core::units::Meters;
use wows_core::units::Percent;
use wowsunpack::game_params::ttx::ShipUpgradeSelection;
use wowsunpack::game_params::ttx::constants::DispersionCurve;
use wowsunpack::game_params::ttx::constants::clamped_dispersion_coeff;
use wowsunpack::game_params::ttx::modifiers::ModifierBundle;
use wowsunpack::game_params::ttx::orchestration::ArtilleryDispersion;
use wowsunpack::game_params::ttx::orchestration::artillery_dispersion;
use wowsunpack::game_params::types::GameParamProvider;

use crate::builds::Item;
use crate::builds::Planner;
use crate::data::GameData;
use crate::error::QueryError;
use crate::firing_arcs::round_to;
use crate::loadout::build_failed;
use crate::modules::Module;
use crate::modules::ModuleKind;
use crate::names::Interpretations;
use crate::ships::Ship;
use crate::ships::ShipSummary;

/// Secondary accuracy against the priority target within secondary range.
const PRIORITY_TARGET_ACCURACY: &str = "GSPriorityTargetIdealRadius";

/// Condition of the priority target accuracy of Manual Secondary Battery Aiming.
const PRIORITY_TARGET_CONDITION: &str = "at the priority target within secondary range, after sustained fire";

/// Arguments for `dispersion_at_range`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DispersionAtRangeArgs {
    /// Ship name, e.g. "Yamato".
    pub ship: String,

    /// Firing distance in km, more than 0, e.g. 12.
    pub range_km: Km,

    /// "main_battery" (default) or "secondary".
    pub battery: Option<DispersionBattery>,

    /// Modules to mount instead of stock, e.g. ["Hull B"]. Omit for stock.
    pub modules: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispersionBattery {
    #[default]
    MainBattery,
    Secondary,
}

impl DispersionBattery {
    fn noun(self) -> &'static str {
        match self {
            Self::MainBattery => "main battery",
            Self::Secondary => "secondary battery",
        }
    }
}

/// Vertical semi-axis over horizontal semi-axis of the dispersion ellipse.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct VerticalRatio(f32);

impl VerticalRatio {
    /// The ratio at `distance` for a battery whose maximum range is
    /// `max_range` (`getClampedCoeff`).
    pub fn at(curve: &DispersionCurve, distance: Km, max_range: Km) -> Self {
        Self(clamped_dispersion_coeff(
            curve.radius_on_zero,
            curve.radius_on_delim,
            curve.radius_on_max,
            curve.delim,
            distance,
            max_range,
        ))
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// Raw dispersion fields of one gun, as GameParams states them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GunCurve {
    pub mount: String,
    pub min_radius: f32,
    pub ideal_radius: f32,
    pub ideal_distance: f32,
    pub radius_on_zero: f32,
    pub radius_on_delim: f32,
    pub radius_on_max: f32,
    pub delim: f32,
}

/// What a build row changes against the base build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Base,
    Range,
    Accuracy,
    RangeAndAccuracy,
}

/// The ellipse of one mount at the requested distance.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ellipse {
    pub horizontal_m: Meters,
    pub vertical_m: Meters,
    pub vertical_ratio: VerticalRatio,

    /// Against the base build, absent on the base row or when the base build
    /// does not reach the distance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizontal_change_pct: Option<Percent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_change_pct: Option<Percent>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    Ellipse(Ellipse),

    /// The distance is past the build's max range: the battery cannot fire there.
    BeyondMaxRange,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MountDispersion {
    pub mount: String,
    pub at_range: Reach,
}

/// Dispersion under one build.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DispersionRow {
    /// "base", or the modifiers the row applies joined by " + ".
    pub build: String,
    pub kind: RowKind,

    /// When the effect holds, for a captain skill with a trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,

    pub max_range_km: Km,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_range_change_pct: Option<Percent>,

    pub mounts: Vec<MountDispersion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DispersionAtRange {
    /// Names read as something they do not spell exactly, or as one of several ships sharing a name.
    #[serde(skip_serializing_if = "Interpretations::is_empty")]
    pub interpretations: Interpretations,

    pub ship: ShipSummary,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Module>,

    pub battery: DispersionBattery,
    pub range_km: Km,
    pub conclusion: String,
    pub guns: Vec<GunCurve>,
    pub builds: Vec<DispersionRow>,
    pub notes: Vec<&'static str>,
}

/// What lengthens range or tightens accuracy.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Item(Item),

    /// A fire control module, by its `ShipUpgradeInfo` key.
    FireControl(String),

    /// A consumable's `artilleryDistCoeff`, e.g. Spotting Aircraft.
    RangeConsumable(RangeCoefficient),
}

/// Main battery range multiplier of a consumable: its `artilleryDistCoeff`,
/// applied to the main battery's maximum range.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub(crate) struct RangeCoefficient(pub(crate) f32);

impl RangeCoefficient {
    const NONE: Self = Self(1.0);
}

/// One mount's dispersion profile under a build.
struct MountProfile {
    mount: String,
    profile: ArtilleryDispersion,
}

struct Candidate {
    label: String,
    source: Source,
    kind: RowKind,

    /// When the effect holds, for a captain skill with a trigger.
    condition: Option<String>,

    profiles: Vec<MountProfile>,
}

/// Candidates that hold on every shot, and captain skills that hold only
/// with their trigger met.
struct Candidates {
    lasting: Vec<Candidate>,
    conditional: Vec<Candidate>,
}

/// A secondary mount group as the stat card labels it.
struct SecondaryMountCurve {
    label: String,
    curve: DispersionCurve,
}

/// Semi-axes and vertical ratio of one ellipse.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Axes {
    horizontal: Meters,
    vertical: Meters,
    ratio: VerticalRatio,
}

/// The fixed inputs of one query.
struct Battery<'a> {
    game: &'a GameData,
    ship: &'a Ship,
    kind: DispersionBattery,
    selection: ShipUpgradeSelection,

    /// Secondary mount groups and the base secondary range.
    secondaries: Vec<SecondaryMountCurve>,
    secondary_base_range: Option<Km>,
}

impl Battery<'_> {
    /// Mount profiles under `bundle` on `selection`, the main battery range
    /// times `range`. `None` when the battery or its dispersion data is
    /// missing; that depends on the selection, never on the modifiers.
    fn profiles(
        &self,
        bundle: &ModifierBundle,
        selection: &ShipUpgradeSelection,
        range: RangeCoefficient,
    ) -> Option<Vec<MountProfile>> {
        match self.kind {
            DispersionBattery::MainBattery => {
                let profile = artillery_dispersion(&self.ship.param, selection, bundle)?;
                let profile =
                    ArtilleryDispersion { max_range: Km::new(profile.max_range.value() * range.0), ..profile };

                Some(vec![MountProfile { mount: "main battery".to_owned(), profile }])
            }
            DispersionBattery::Secondary => {
                let base = self.secondary_base_range?;
                let max_range = Km::new(base.value() * bundle.coef("GSMaxDist"));
                let ideal_radius_coef = secondary_accuracy(bundle, self.ship);
                let profiles: Vec<MountProfile> = self
                    .secondaries
                    .iter()
                    .map(|mount| MountProfile {
                        mount: mount.label.clone(),
                        profile: ArtilleryDispersion { curve: mount.curve, max_range, ideal_radius_coef },
                    })
                    .collect();

                (!profiles.is_empty()).then_some(profiles)
            }
        }
    }
}

impl GameData {
    /// Horizontal and vertical dispersion at one distance on the base build
    /// and with each range or accuracy modifier the ship can use.
    pub fn dispersion_at_range(&self, args: &DispersionAtRangeArgs) -> Result<DispersionAtRange, QueryError> {
        const FORMULA_NOTE: &str = "horizontal_m = 30 x (minRadius + X_m / 30 x (idealRadius - minRadius) / idealDistance), X_m the firing distance in meters and both radii times the accuracy coefficient (Aiming Systems Modification 1 and the like); it depends only on the firing distance. vertical_m = horizontal_m x vertical_ratio, where vertical_ratio runs linearly from radiusOnZero at 0 km to radiusOnDelim at delim x max range, then to radiusOnMax at max range. A longer max range lowers X / max range, so at the same X the ratio, and with it vertical dispersion, falls whenever radiusOnZero < radiusOnDelim < radiusOnMax. At max range horizontal_m is the port's maximum dispersion.";
        const MAIN_RANGE_NOTE: &str = "max range is the modified main battery range: fire control module, upgrades, captain skills and Spotting Aircraft all lengthen it. The game client passes the modified range to the gun, and replay measurements (Colbert at 6 km, 13.8 km against 19.2 km range with range skill and upgrade) showed the predicted 16% smaller vertical spread. Spotting Aircraft lengthens the same range value, so it is assumed to act the same way; that case was not measured.";
        const SECONDARY_NOTE: &str = "Secondary max range is the base secondary range times the secondary range modifiers. The accuracy coefficient is the one the game applies in battle: GSIdealRadius times GSIdealRadiusHighLevel on tier 7 and higher ships or GSIdealRadiusLowLevel below, times GSPriorityTargetIdealRadius in rows whose condition names the priority target; the port card applies GSIdealRadius alone. Manual secondary control modes are not modeled.";
        const TAPER_NOTE: &str = "Below the gun's taperDist, 4 to 5 km for most guns, the game narrows horizontal dispersion further toward the muzzle; values at these distances do not apply it and overstate dispersion.";
        /// Distances below this may fall under a gun's taperDist.
        const TAPER_REACH_KM: f32 = 5.0;

        let range = args.range_km;
        if !range.value().is_finite() || range.value() <= 0.0 {
            return Err(QueryError::InvalidArguments { reason: "range_km must be more than 0" });
        }

        let mut interpretations = Interpretations::default();
        let ship = self.resolve_ship(&args.ship, &mut interpretations)?;
        let modules = self.resolve_modules(ship, args.modules.as_deref().unwrap_or_default())?;
        self.check_hull_fit(ship, &modules)?;
        let kind = args.battery.unwrap_or_default();

        let battery = self.dispersion_battery(ship, kind, &modules.selection)?;
        let planner = Planner::new(self, ship);
        let stock = planner.stock()?;
        let Some(base) = battery.profiles(&stock.bundle, &battery.selection, RangeCoefficient::NONE) else {
            return Err(QueryError::WeaponNotFound { ship: ship.name.clone(), weapon: kind.noun().to_owned() });
        };

        let candidates = self.dispersion_candidates(&battery, &planner, &stock.bundle, &base)?;
        let mut rows = vec![row("base".to_owned(), RowKind::Base, &base, None, range)];
        for candidate in candidates.lasting.iter().chain(&candidates.conditional) {
            let mut candidate_row =
                row(candidate.label.clone(), candidate.kind, &candidate.profiles, Some(&base), range);
            candidate_row.condition = candidate.condition.clone();
            rows.push(candidate_row);
        }
        rows.extend(combined_rows(&battery, &planner, &candidates.lasting, &base, range)?);

        let conclusion = conclusion(ship, kind, range, &rows);
        let guns = base.iter().map(|mount| gun_curve(&mount.mount, &mount.profile.curve)).collect();
        let mut notes = vec![FORMULA_NOTE];
        notes.push(match kind {
            DispersionBattery::MainBattery => MAIN_RANGE_NOTE,
            DispersionBattery::Secondary => SECONDARY_NOTE,
        });
        if range.value() < TAPER_REACH_KM {
            notes.push(TAPER_NOTE);
        }

        Ok(DispersionAtRange {
            interpretations,
            ship: ship.summary(),
            modules: modules.chosen,
            battery: kind,
            range_km: range,
            conclusion,
            guns,
            builds: rows,
            notes,
        })
    }

    fn dispersion_battery<'a>(
        &'a self,
        ship: &'a Ship,
        kind: DispersionBattery,
        selection: &ShipUpgradeSelection,
    ) -> Result<Battery<'a>, QueryError> {
        let mut battery = Battery {
            game: self,
            ship,
            kind,
            selection: selection.clone(),
            secondaries: Vec::new(),
            secondary_base_range: None,
        };
        if kind == DispersionBattery::MainBattery {
            return Ok(battery);
        }

        let Some(atba) = ship
            .param
            .vehicle()
            .and_then(|vehicle| vehicle.ttx_components())
            .and_then(|components| components.secondaries(selection.hull.as_deref()?))
        else {
            return Ok(battery);
        };
        battery.secondary_base_range = atba.max_dist.map(|meters| Km::new(meters.value() / 1000.0));

        // The card groups and labels the mounts; the labels do not depend on the build.
        let card = self
            .effective_modifiers(ship, &self.stock_loadout())?
            .build
            .stats(&ship.param, selection, self.provider())
            .map_err(build_failed(ship))?;
        if let Some(secondaries) = card.secondaries.as_ref() {
            battery.secondaries = secondaries
                .mounts
                .iter()
                .filter_map(|mount| {
                    let gun = atba.guns.iter().find(|gun| mount.hardpoints.contains(&gun.hardpoint))?;
                    Some(SecondaryMountCurve { label: mount.label.clone(), curve: gun.stats.dispersion_curve()? })
                })
                .collect();
        }

        Ok(battery)
    }

    /// Every single fire control module, upgrade, captain skill and
    /// consumable that lengthens the battery's range or tightens its
    /// accuracy, longest range first, then tightest accuracy.
    fn dispersion_candidates(
        &self,
        battery: &Battery<'_>,
        planner: &Planner<'_>,
        stock: &ModifierBundle,
        base: &[MountProfile],
    ) -> Result<Candidates, QueryError> {
        let candidate = |label: String, source: Source, profiles: Option<Vec<MountProfile>>| {
            let profiles = profiles?;
            let kind = change_kind(&base[0].profile, &profiles[0].profile)?;
            Some(Candidate { label, source, kind, condition: None, profiles })
        };
        let mut lasting = Vec::new();

        for &item in planner.items() {
            let outcome = planner.with_items(&[item])?;
            let profiles = battery.profiles(&outcome.bundle, &battery.selection, RangeCoefficient::NONE);
            lasting.extend(candidate(planner.label(item), Source::Item(item), profiles));
        }

        if battery.kind == DispersionBattery::MainBattery {
            for module in self.ship_modules(battery.ship) {
                if module.kind != ModuleKind::FireControl
                    || battery.selection.fire_control.as_ref() == Some(&module.param)
                {
                    continue;
                }
                let mut selection = battery.selection.clone();
                module.kind.set(&mut selection, module.param.clone());
                if !self.hull_fits(battery.ship, &selection) {
                    continue;
                }
                let profiles = battery.profiles(stock, &selection, RangeCoefficient::NONE);
                lasting.extend(candidate(module.name, Source::FireControl(module.param), profiles));
            }

            for (name, coefficient) in self.range_consumables(battery.ship) {
                let profiles = battery.profiles(stock, &battery.selection, coefficient);
                lasting.extend(candidate(name, Source::RangeConsumable(coefficient), profiles));
            }
        }

        let mut conditional = Vec::new();
        for &item in planner.triggered_items() {
            let Some(active) = planner.with_skill_triggered(item)? else {
                continue;
            };
            let idle = planner.with_items(&[item])?;
            let active_profiles = battery.profiles(&active.bundle, &battery.selection, RangeCoefficient::NONE);
            let idle_profiles = battery.profiles(&idle.bundle, &battery.selection, RangeCoefficient::NONE);
            let (Some(active_profiles), Some(idle_profiles)) = (active_profiles, idle_profiles) else {
                continue;
            };
            if change_kind(&idle_profiles[0].profile, &active_profiles[0].profile).is_none() {
                continue;
            }

            let condition = if battery.kind == DispersionBattery::Secondary
                && active.bundle.coef(PRIORITY_TARGET_ACCURACY) != 1.0
            {
                Some(PRIORITY_TARGET_CONDITION.to_owned())
            } else {
                planner.trigger_sentence(item)
            };
            if let Some(mut found) = candidate(planner.label(item), Source::Item(item), Some(active_profiles)) {
                found.condition = condition;
                conditional.push(found);
            }
        }

        lasting.sort_by(candidate_order);
        conditional.sort_by(candidate_order);

        Ok(Candidates { lasting, conditional })
    }

    /// Consumables of `ship` that multiply main battery range, by English name.
    pub(crate) fn range_consumables(&self, ship: &Ship) -> Vec<(String, RangeCoefficient)> {
        let slots = ship.param.vehicle().and_then(|vehicle| vehicle.abilities()).unwrap_or_default();
        let mut found: Vec<(String, RangeCoefficient)> = Vec::new();

        for (ability, variant) in slots.iter().flatten() {
            let Some(param) = self.provider().game_param_by_name(ability) else {
                continue;
            };
            let Some(category) = param.ability().and_then(|ability| ability.get_category(variant)) else {
                continue;
            };
            let Some(&coefficient) = category.effect_fields().get("artilleryDistCoeff") else {
                continue;
            };
            if coefficient <= 1.0 {
                continue;
            }

            let name = self.consumable_name(ability, category);
            if !found.iter().any(|(known, _)| *known == name) {
                found.push((name, RangeCoefficient(coefficient)));
            }
        }

        found
    }
}

/// Range sources first, longest range first, then tightest accuracy.
fn candidate_order(left: &Candidate, right: &Candidate) -> Ordering {
    let lengthens = |candidate: &Candidate| matches!(candidate.kind, RowKind::Range | RowKind::RangeAndAccuracy);
    let left_profile = &left.profiles[0].profile;
    let right_profile = &right.profiles[0].profile;

    lengthens(right)
        .cmp(&lengthens(left))
        .then(right_profile.max_range.value().total_cmp(&left_profile.max_range.value()))
        .then(left_profile.ideal_radius_coef.total_cmp(&right_profile.ideal_radius_coef))
        .then(left.label.cmp(&right.label))
}

/// The secondary accuracy coefficient the game applies in battle
/// (`calculateAtbaIdealRadius`, ModifiersApply.py:190-199), times the priority
/// target coefficient when a build carries it.
fn secondary_accuracy(bundle: &ModifierBundle, ship: &Ship) -> f32 {
    /// `MAX_SMALL_CATEGORY_LEVEL` (m63fc2621.py:17).
    const HIGH_LEVEL_FROM: u32 = 7;

    let by_level = if ship.tier.0 >= HIGH_LEVEL_FROM {
        bundle.coef("GSIdealRadiusHighLevel")
    } else {
        bundle.coef("GSIdealRadiusLowLevel")
    };

    bundle.coef("GSIdealRadius") * by_level * bundle.coef(PRIORITY_TARGET_ACCURACY)
}

/// How `candidate` differs from `base`: `None` when it neither lengthens the
/// range nor tightens accuracy.
fn change_kind(base: &ArtilleryDispersion, candidate: &ArtilleryDispersion) -> Option<RowKind> {
    /// Relative change below which two ranges or coefficients are equal.
    const TOLERANCE: f32 = 1e-4;

    let longer = candidate.max_range.value() > base.max_range.value() * (1.0 + TOLERANCE);
    let tighter = candidate.ideal_radius_coef < base.ideal_radius_coef * (1.0 - TOLERANCE);

    match (longer, tighter) {
        (true, true) => Some(RowKind::RangeAndAccuracy),
        (true, false) => Some(RowKind::Range),
        (false, true) => Some(RowKind::Accuracy),
        (false, false) => None,
    }
}

/// The longest range build, and that build with the accuracy modifiers whose
/// upgrade slots it leaves free. A row equal to a single candidate is left
/// out. Skill point limits are not checked: few skills change range or
/// accuracy.
fn combined_rows(
    battery: &Battery<'_>,
    planner: &Planner<'_>,
    candidates: &[Candidate],
    base: &[MountProfile],
    range: Km,
) -> Result<Vec<DispersionRow>, QueryError> {
    let ranged: Vec<&Candidate> = candidates.iter().filter(|candidate| candidate.kind != RowKind::Accuracy).collect();
    let accurate: Vec<&Candidate> = candidates.iter().filter(|candidate| candidate.kind != RowKind::Range).collect();

    let longest = pick(battery, &ranged, |candidate| -candidate.profiles[0].profile.max_range.value());
    let tightest = pick(battery, &accurate, |candidate| candidate.profiles[0].profile.ideal_radius_coef);

    let mut rows = Vec::new();
    if longest.len() > 1 {
        rows.push(combined_row(battery, planner, &longest, base, range)?);
    }

    let mut both = longest.clone();
    for candidate in tightest {
        let slot = upgrade_slot(battery.game, &candidate.source);
        let taken = slot.is_some() && both.iter().any(|chosen| upgrade_slot(battery.game, &chosen.source) == slot);
        if !taken && !both.iter().any(|chosen| chosen.source == candidate.source) {
            both.push(candidate);
        }
    }
    if !longest.is_empty() && both.len() > longest.len() {
        rows.push(combined_row(battery, planner, &both, base, range)?);
    }

    Ok(rows)
}

/// One candidate per upgrade slot, per fire control slot and per consumable
/// slot, the lowest by `order`; every captain skill.
fn pick<'c>(
    battery: &Battery<'_>,
    candidates: &[&'c Candidate],
    order: impl Fn(&Candidate) -> f32,
) -> Vec<&'c Candidate> {
    #[derive(PartialEq)]
    enum Group {
        Skill(usize),
        Upgrade(u8),
        FireControl,
        Consumable,
    }

    let group = |candidate: &Candidate| match &candidate.source {
        Source::Item(Item::Skill { index, .. }) => Group::Skill(*index),
        Source::Item(Item::Upgrade(index)) => Group::Upgrade(battery.game.upgrades[*index].slot),
        Source::FireControl(_) => Group::FireControl,
        Source::RangeConsumable(_) => Group::Consumable,
    };

    let mut chosen: Vec<&Candidate> = Vec::new();
    for &candidate in candidates {
        match chosen.iter().position(|other| group(other) == group(candidate)) {
            Some(index) => {
                if order(candidate) < order(chosen[index]) {
                    chosen[index] = candidate;
                }
            }
            None => chosen.push(candidate),
        }
    }

    chosen
}

fn upgrade_slot(game: &GameData, source: &Source) -> Option<u8> {
    match source {
        Source::Item(Item::Upgrade(index)) => Some(game.upgrades[*index].slot),
        _ => None,
    }
}

fn combined_row(
    battery: &Battery<'_>,
    planner: &Planner<'_>,
    chosen: &[&Candidate],
    base: &[MountProfile],
    range: Km,
) -> Result<DispersionRow, QueryError> {
    let mut items = Vec::new();
    let mut selection = battery.selection.clone();
    let mut coefficient = RangeCoefficient::NONE;
    for candidate in chosen {
        match &candidate.source {
            Source::Item(item) => items.push(*item),
            Source::FireControl(param) => ModuleKind::FireControl.set(&mut selection, param.clone()),
            Source::RangeConsumable(range) => coefficient = RangeCoefficient(coefficient.0 * range.0),
        }
    }

    let outcome = planner.with_items(&items)?;
    let profiles = battery
        .profiles(&outcome.bundle, &selection, coefficient)
        .expect("the battery has profiles on every selection its candidates came from");
    let kind = change_kind(&base[0].profile, &profiles[0].profile)
        .expect("a build of candidates that each lengthen range or tighten accuracy changes one of them");
    let label = chosen.iter().map(|candidate| candidate.label.as_str()).collect::<Vec<_>>().join(" + ");

    Ok(row(label, kind, &profiles, Some(base), range))
}

fn row(
    build: String,
    kind: RowKind,
    profiles: &[MountProfile],
    base: Option<&[MountProfile]>,
    range: Km,
) -> DispersionRow {
    let max_range = profiles[0].profile.max_range;
    let mounts = profiles
        .iter()
        .enumerate()
        .map(|(index, mount)| {
            let base_axes = base.and_then(|base| axes_at(&base[index].profile, range));
            let at_range = match axes_at(&mount.profile, range) {
                Some(axes) => Reach::Ellipse(Ellipse {
                    horizontal_m: Meters::new(round_to(axes.horizontal.value(), 10.0)),
                    vertical_m: Meters::new(round_to(axes.vertical.value(), 10.0)),
                    vertical_ratio: VerticalRatio(round_to(axes.ratio.0, 1000.0)),
                    horizontal_change_pct: base_axes
                        .map(|base| change(base.horizontal.value(), axes.horizontal.value())),
                    vertical_change_pct: base_axes.map(|base| change(base.vertical.value(), axes.vertical.value())),
                }),
                None => Reach::BeyondMaxRange,
            };
            MountDispersion { mount: mount.mount.clone(), at_range }
        })
        .collect();

    DispersionRow {
        build,
        kind,
        condition: None,
        max_range_km: Km::new(round_to(max_range.value(), 100.0)),
        max_range_change_pct: base.map(|base| change(base[0].profile.max_range.value(), max_range.value())),
        mounts,
    }
}

/// The ellipse at `range`, `None` past the max range.
fn axes_at(profile: &ArtilleryDispersion, range: Km) -> Option<Axes> {
    if range.value() > profile.max_range.value() {
        return None;
    }

    let ellipse = profile.ellipse_at(range);
    Some(Axes {
        horizontal: ellipse.horizontal.to_meters(),
        vertical: ellipse.vertical.to_meters(),
        ratio: VerticalRatio::at(&profile.curve, range, profile.max_range),
    })
}

fn change(base: f32, value: f32) -> Percent {
    Percent::new(round_to((value / base - 1.0) * 100.0, 10.0))
}

fn gun_curve(mount: &str, curve: &DispersionCurve) -> GunCurve {
    GunCurve {
        mount: mount.to_owned(),
        min_radius: curve.min_radius,
        ideal_radius: curve.ideal_radius,
        ideal_distance: curve.ideal_distance,
        radius_on_zero: curve.radius_on_zero,
        radius_on_delim: curve.radius_on_delim,
        radius_on_max: curve.radius_on_max,
        delim: curve.delim,
    }
}

fn first_ellipse(row: &DispersionRow) -> Option<&Ellipse> {
    match &row.mounts.first()?.at_range {
        Reach::Ellipse(ellipse) => Some(ellipse),
        Reach::BeyondMaxRange => None,
    }
}

/// One or two sentences on what range and accuracy modifiers do at `range`,
/// read off the first mount, which is named when there are several. Rows with
/// a condition are left out.
fn conclusion(ship: &Ship, kind: DispersionBattery, range: Km, rows: &[DispersionRow]) -> String {
    let base = &rows[0];
    let subject = match base.mounts.as_slice() {
        [first, _, ..] => format!("{} ({})", kind.noun(), first.mount),
        _ => kind.noun().to_owned(),
    };
    let at = format_km(range.value());
    // Reversed so that among equal ranges the first row, with the fewest modifiers, wins.
    let longest = |kinds: &[RowKind]| {
        rows.iter()
            .rev()
            .filter(|row| row.condition.is_none() && kinds.contains(&row.kind))
            .max_by(|left, right| left.max_range_km.value().total_cmp(&right.max_range_km.value()))
    };
    // A row that also tightens accuracy would mix the two effects.
    let ranged = longest(&[RowKind::Range]).or_else(|| longest(&[RowKind::RangeAndAccuracy]));
    let accurate = rows
        .iter()
        .filter(|row| row.condition.is_none() && row.kind == RowKind::Accuracy)
        .filter_map(|row| first_ellipse(row).map(|ellipse| (row, ellipse)))
        .min_by(|left, right| left.1.horizontal_m.value().total_cmp(&right.1.horizontal_m.value()));

    let Some(base_ellipse) = first_ellipse(base) else {
        let mut text =
            format!("{}'s {subject} reaches {} km, short of {at} km.", ship.name, format_km(base.max_range_km.value()));
        let reaching = longest(&[RowKind::Range, RowKind::RangeAndAccuracy]);
        match reaching.map(|row| (row, first_ellipse(row))) {
            Some((row, Some(ellipse))) => text.push_str(&format!(
                " With {} it reaches {} km; at {at} km dispersion is {} m horizontal and {} m vertical.",
                row.build,
                format_km(row.max_range_km.value()),
                format_m(ellipse.horizontal_m),
                format_m(ellipse.vertical_m),
            )),
            Some((row, None)) => text.push_str(&format!(
                " No build reaches it: the longest, {}, reaches {} km.",
                row.build,
                format_km(row.max_range_km.value()),
            )),
            None => text.push_str(" Nothing it can mount lengthens that range."),
        }
        return text;
    };

    let mut text = match ranged.and_then(|row| first_ellipse(row).map(|ellipse| (row, ellipse))) {
        Some((row, ellipse)) => {
            let vertical = match ellipse.vertical_m.value().total_cmp(&base_ellipse.vertical_m.value()) {
                Ordering::Less => format!(
                    "shrinks vertical dispersion from {} m to {} m",
                    format_m(base_ellipse.vertical_m),
                    format_m(ellipse.vertical_m)
                ),
                Ordering::Greater => format!(
                    "widens vertical dispersion from {} m to {} m",
                    format_m(base_ellipse.vertical_m),
                    format_m(ellipse.vertical_m)
                ),
                Ordering::Equal => format!("leaves vertical dispersion at {} m", format_m(base_ellipse.vertical_m)),
            };
            let horizontal = if ellipse.horizontal_m == base_ellipse.horizontal_m {
                format!("horizontal is unchanged at {} m", format_m(base_ellipse.horizontal_m))
            } else {
                format!(
                    "horizontal goes from {} m to {} m",
                    format_m(base_ellipse.horizontal_m),
                    format_m(ellipse.horizontal_m)
                )
            };
            let longer = row.max_range_change_pct.map_or_else(String::new, |pct| format!("{} ", format_pct(pct)));
            format!(
                "At {at} km, a {longer}longer {subject} range ({} km to {} km with {}) {vertical}; {horizontal}.",
                format_km(base.max_range_km.value()),
                format_km(row.max_range_km.value()),
                row.build,
            )
        }
        None if accurate.is_none() => format!(
            "Nothing {} can mount lengthens its {subject} range or tightens its accuracy, so at {at} km its dispersion is {} m horizontal and {} m vertical on every build.",
            ship.name,
            format_m(base_ellipse.horizontal_m),
            format_m(base_ellipse.vertical_m),
        ),
        None => format!(
            "Nothing {} can mount lengthens its {subject} range; at {at} km the base dispersion is {} m horizontal and {} m vertical.",
            ship.name,
            format_m(base_ellipse.horizontal_m),
            format_m(base_ellipse.vertical_m),
        ),
    };

    if let Some((row, ellipse)) = accurate {
        text.push_str(&format!(
            " {} narrows both: horizontal {} m to {} m, vertical {} m to {} m.",
            row.build,
            format_m(base_ellipse.horizontal_m),
            format_m(ellipse.horizontal_m),
            format_m(base_ellipse.vertical_m),
            format_m(ellipse.vertical_m),
        ));
    }

    text
}

fn format_km(value: f32) -> String {
    format!("{}", round_to(value, 100.0))
}

fn format_m(value: Meters) -> String {
    format!("{}", round_to(value.value(), 10.0))
}

fn format_pct(value: Percent) -> String {
    format!("{}%", round_to(value.value(), 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> DispersionCurve {
        DispersionCurve {
            min_radius: 2.8,
            ideal_radius: 10.0,
            ideal_distance: 1000.0,
            radius_on_zero: 0.2,
            radius_on_delim: 0.6,
            radius_on_max: 0.8,
            delim: 0.5,
        }
    }

    #[test]
    fn longer_max_range_lowers_the_vertical_ratio_at_a_fixed_distance() {
        let curve = curve();
        let distance = Km::new(12.0);

        // 12 km is past delim x max range on a 20 km gun and before it on a 30 km gun.
        let short = VerticalRatio::at(&curve, distance, Km::new(20.0)).value();
        let long = VerticalRatio::at(&curve, distance, Km::new(30.0)).value();
        assert!((short - 0.64).abs() < 1e-6, "got {short}");
        assert!((long - (0.2 + 0.4 * 12.0 / 15.0)).abs() < 1e-6, "got {long}");
        assert!(long < short);
    }

    #[test]
    fn longer_max_range_leaves_horizontal_dispersion_unchanged() {
        let curve = curve();
        let distance = Km::new(12.0);
        let short = ArtilleryDispersion { curve, max_range: Km::new(20.0), ideal_radius_coef: 1.0 };
        let long = ArtilleryDispersion { max_range: Km::new(30.0), ..short };

        let short_axes = axes_at(&short, distance).expect("12 km is in range");
        let long_axes = axes_at(&long, distance).expect("12 km is in range");
        assert_eq!(short_axes.horizontal, long_axes.horizontal);
        assert!(long_axes.vertical.value() < short_axes.vertical.value());
        assert_eq!(axes_at(&short, Km::new(21.0)), None);
    }

    #[test]
    fn change_kind_tells_range_from_accuracy() {
        let base = ArtilleryDispersion { curve: curve(), max_range: Km::new(20.0), ideal_radius_coef: 1.0 };
        let longer = ArtilleryDispersion { max_range: Km::new(23.2), ..base };
        let tighter = ArtilleryDispersion { ideal_radius_coef: 0.93, ..base };
        let both = ArtilleryDispersion { max_range: Km::new(23.2), ideal_radius_coef: 0.93, ..base };

        assert_eq!(change_kind(&base, &base), None);
        assert_eq!(change_kind(&base, &longer), Some(RowKind::Range));
        assert_eq!(change_kind(&base, &tighter), Some(RowKind::Accuracy));
        assert_eq!(change_kind(&base, &both), Some(RowKind::RangeAndAccuracy));
    }
}
